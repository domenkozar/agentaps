use crate::diff_view::{File, Hunk, Line, Mark, Side};
use gix_imara_diff::{Algorithm, Diff, InternedInput};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const CONTEXT_LINES: usize = 3;

enum Change<'a> {
    Text {
        before: &'a str,
        after: &'a str,
        had_head_entry: bool,
        has_worktree_entry: bool,
    },
    Note(&'static str),
}

/// Read the selected checkout's current files against HEAD. Includes staged,
/// unstaged, and untracked paths reported by Git, while respecting ignore rules.
pub fn load(path: &Path) -> Result<Vec<File>, String> {
    scan(path, |path, change| match change {
        Change::Note(note) => Some(File {
            path,
            hunks: Vec::new(),
            note: Some(note.into()),
        }),
        Change::Text {
            before,
            after,
            had_head_entry,
            has_worktree_entry,
        } => {
            let hunks = text_hunks(before, after);
            let note = (hunks.is_empty() && before == after).then(|| {
                if !had_head_entry {
                    "Empty file added".into()
                } else if !has_worktree_entry {
                    "Empty file deleted".into()
                } else {
                    "Git status changed without a text difference".into()
                }
            });
            (!hunks.is_empty() || note.is_some()).then_some(File { path, hunks, note })
        }
    })
}

/// Count changed lines without creating hunks or display rows.
pub fn line_counts(path: &Path) -> Result<Option<(usize, usize)>, String> {
    let counts = scan(path, |_, change| match change {
        Change::Note(_) => Some((0, 0)),
        Change::Text { before, after, .. } => {
            let input = InternedInput::new(before, after);
            let mut diff = Diff::compute(Algorithm::Histogram, &input);
            diff.postprocess_lines(&input);
            let added = diff.count_additions() as usize;
            let removed = diff.count_removals() as usize;
            (added != 0 || removed != 0 || before == after).then_some((added, removed))
        }
    })?;
    if counts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(counts.into_iter().fold((0, 0), |total, count| {
            (total.0 + count.0, total.1 + count.1)
        })))
    }
}

fn scan<T>(
    path: &Path,
    mut visit: impl FnMut(String, Change<'_>) -> Option<T>,
) -> Result<Vec<T>, String> {
    let repo = gix::discover(path).map_err(|error| error.to_string())?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| "This Git repository has no working directory".to_owned())?;
    let head = repo.head_tree().ok();
    let mut paths = BTreeSet::new();
    let changes = repo
        .status(gix::progress::Discard)
        .map_err(|error| error.to_string())?
        .untracked_files(gix::status::UntrackedFiles::Files)
        .into_iter(Vec::new())
        .map_err(|error| error.to_string())?;
    for change in changes {
        let change = change.map_err(|error| error.to_string())?;
        let relative = gix::path::from_bstr(change.location()).into_owned();
        if workdir.join(&relative).starts_with(path) {
            paths.insert(relative);
        }
    }

    // Repeated path lookups walk a tree for every file. For a large changeset,
    // traverse HEAD once and retain only the paths that status reported.
    let head_entries: Option<
        HashMap<PathBuf, (gix::object::tree::EntryMode, gix::hash::ObjectId)>,
    > = if paths.len() >= 256 {
        head.as_ref()
            .map(|tree| {
                tree.traverse()
                    .breadthfirst
                    .files()
                    .map_err(|error| error.to_string())
                    .map(|entries| {
                        entries
                            .into_iter()
                            .filter_map(|entry| {
                                let path = gix::path::from_bstr(&entry.filepath).into_owned();
                                paths
                                    .contains(&path)
                                    .then_some((path, (entry.mode, entry.oid)))
                            })
                            .collect()
                    })
            })
            .transpose()?
    } else {
        None
    };

    let mut files = Vec::with_capacity(paths.len());
    for relative in paths {
        let label = relative.to_string_lossy().into_owned();
        let head_entry = match &head_entries {
            Some(entries) => entries.get(&relative).copied(),
            None => head
                .as_ref()
                .map(|tree| tree.lookup_entry_by_path(&relative))
                .transpose()
                .map_err(|error| error.to_string())?
                .flatten()
                .map(|entry| (entry.mode(), entry.object_id())),
        };
        let had_head_entry = head_entry.is_some();
        let old = match head_entry {
            Some((mode, oid)) => {
                if mode.kind() == gix::object::tree::EntryKind::Commit {
                    if let Some(file) = visit(label, Change::Note("Git submodule changed")) {
                        files.push(file);
                    }
                    continue;
                }
                if repo
                    .find_header(oid)
                    .map_err(|error| error.to_string())?
                    .size()
                    > MAX_FILE_BYTES
                {
                    if let Some(file) = visit(label, Change::Note("Large file, diff unavailable")) {
                        files.push(file);
                    }
                    continue;
                }
                repo.find_blob(oid)
                    .map_err(|error| error.to_string())?
                    .data
                    .clone()
            }
            None => Vec::new(),
        };
        let absolute = workdir.join(&relative);
        let metadata = fs::symlink_metadata(&absolute);
        let has_worktree_entry = metadata.is_ok();
        let new = match metadata {
            Ok(metadata) if metadata.file_type().is_symlink() => fs::read_link(&absolute)
                .map(|target| target.to_string_lossy().into_owned().into_bytes())
                .map_err(|error| format!("{}: {error}", absolute.display()))?,
            Ok(metadata) if metadata.is_file() && metadata.len() > MAX_FILE_BYTES => {
                if let Some(file) = visit(label, Change::Note("Large file, diff unavailable")) {
                    files.push(file);
                }
                continue;
            }
            Ok(metadata) if metadata.is_file() => {
                fs::read(&absolute).map_err(|error| format!("{}: {error}", absolute.display()))?
            }
            Ok(_) => {
                if let Some(file) = visit(label, Change::Note("Directory or non-file change")) {
                    files.push(file);
                }
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(format!("{}: {error}", absolute.display())),
        };
        let change = if old.len() as u64 > MAX_FILE_BYTES || new.len() as u64 > MAX_FILE_BYTES {
            Change::Note("Large file, diff unavailable")
        } else if old.contains(&0) || new.contains(&0) {
            Change::Note("Binary file changed")
        } else if let (Ok(before), Ok(after)) =
            (std::str::from_utf8(&old), std::str::from_utf8(&new))
        {
            Change::Text {
                before,
                after,
                had_head_entry,
                has_worktree_entry,
            }
        } else {
            Change::Note("Non-UTF-8 file changed")
        };
        if let Some(file) = visit(label, change) {
            files.push(file);
        }
    }
    Ok(files)
}

fn text_hunks(before: &str, after: &str) -> Vec<Hunk> {
    let old_lines: Vec<&str> = before.lines().collect();
    let new_lines: Vec<&str> = after.lines().collect();
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let changes: Vec<_> = diff.hunks().collect();
    let mut grouped: Vec<(usize, usize, usize, usize, Vec<gix_imara_diff::Hunk>)> = Vec::new();
    for change in changes {
        let old_start = (change.before.start as usize).saturating_sub(CONTEXT_LINES);
        let new_start = (change.after.start as usize).saturating_sub(CONTEXT_LINES);
        let old_end = (change.before.end as usize + CONTEXT_LINES).min(old_lines.len());
        let new_end = (change.after.end as usize + CONTEXT_LINES).min(new_lines.len());
        if let Some(group) = grouped.last_mut()
            && old_start <= group.1
            && new_start <= group.3
        {
            group.1 = group.1.max(old_end);
            group.3 = group.3.max(new_end);
            group.4.push(change);
        } else {
            grouped.push((old_start, old_end, new_start, new_end, vec![change]));
        }
    }
    grouped
        .into_iter()
        .map(|(old_start, old_end, new_start, new_end, changes)| {
            let mut lines = Vec::new();
            let mut old_at = old_start;
            let mut new_at = new_start;
            for change in changes {
                let old_change = change.before.start as usize;
                let new_change = change.after.start as usize;
                while old_at < old_change && new_at < new_change {
                    lines.push(context(old_at, new_at, old_lines[old_at]));
                    old_at += 1;
                    new_at += 1;
                }
                let old_stop = change.before.end as usize;
                let new_stop = change.after.end as usize;
                while old_at < old_stop || new_at < new_stop {
                    let old = (old_at < old_stop).then(|| {
                        let side = Side {
                            number: old_at + 1,
                            text: old_lines[old_at].to_owned(),
                            mark: Mark::Removed,
                        };
                        old_at += 1;
                        side
                    });
                    let new = (new_at < new_stop).then(|| {
                        let side = Side {
                            number: new_at + 1,
                            text: new_lines[new_at].to_owned(),
                            mark: Mark::Added,
                        };
                        new_at += 1;
                        side
                    });
                    lines.push(Line { old, new });
                }
            }
            while old_at < old_end && new_at < new_end {
                lines.push(context(old_at, new_at, old_lines[old_at]));
                old_at += 1;
                new_at += 1;
            }
            Hunk {
                header: format!(
                    "@@ -{},{} +{},{} @@",
                    old_start + 1,
                    old_end - old_start,
                    new_start + 1,
                    new_end - new_start
                ),
                lines,
            }
        })
        .collect()
}

fn context(old: usize, new: usize, text: &str) -> Line {
    Line {
        old: Some(Side {
            number: old + 1,
            text: text.to_owned(),
            mark: Mark::Context,
        }),
        new: Some(Side {
            number: new + 1,
            text: text.to_owned(),
            mark: Mark::Context,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn replacement_has_one_split_row_and_two_unified_rows() {
        let hunks = text_hunks("first\nbefore\nlast\n", "first\nafter\nlast\n");
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            crate::diff_view::stats(&File {
                path: "changed.txt".into(),
                hunks: hunks.clone(),
                note: None,
            }),
            (1, 1)
        );
        assert!(hunks[0].lines.iter().any(|line| {
            line.old.as_ref().is_some_and(|side| side.text == "before")
                && line.new.as_ref().is_some_and(|side| side.text == "after")
        }));
    }

    #[test]
    fn loads_staged_unstaged_and_untracked_files() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("agentaps-diff-{}-{stamp}", std::process::id()));
        fs::create_dir(&path).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&path)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q"]);
        fs::write(path.join("staged.txt"), "before\n").unwrap();
        fs::write(path.join("unstaged.txt"), "before\n").unwrap();
        fs::write(path.join(".gitignore"), "ignored.txt\n").unwrap();
        assert_eq!(load(&path).unwrap().len(), 3);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Agentaps",
            "-c",
            "user.email=agentaps@example.invalid",
            "commit",
            "-qm",
            "initial",
        ]);
        assert_eq!(line_counts(&path).unwrap(), None);
        fs::write(path.join("staged.txt"), "after\n").unwrap();
        git(&["add", "staged.txt"]);
        fs::write(path.join("unstaged.txt"), "after\n").unwrap();
        fs::write(path.join("untracked.txt"), "new\n").unwrap();
        fs::write(path.join("ignored.txt"), "skip\n").unwrap();
        let files = load(&path).unwrap();
        let names: Vec<_> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(names, ["staged.txt", "unstaged.txt", "untracked.txt"]);
        assert!(files.iter().all(|file| !file.hunks.is_empty()));
        let expected = files.iter().map(crate::diff_view::stats).fold(
            (0, 0),
            |(added, removed), (file_added, file_removed)| {
                (added + file_added, removed + file_removed)
            },
        );
        assert_eq!(line_counts(&path).unwrap(), Some(expected));
        fs::write(path.join("binary.bin"), b"\0").unwrap();
        fs::write(path.join("empty.txt"), "").unwrap();
        assert_eq!(line_counts(&path).unwrap(), Some(expected));
        fs::remove_dir_all(path).unwrap();
    }
}
