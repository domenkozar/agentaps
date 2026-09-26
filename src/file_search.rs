use nucleo::{
    Config, Injector, Nucleo, Utf32String,
    pattern::{CaseMatching, Normalization},
};
use std::{
    fs,
    io::BufRead,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
};

const RESULT_LIMIT: usize = 12;
const FILE_LIMIT: usize = 50_000;

pub struct FileSearch {
    matcher: Nucleo<String>,
    results: Vec<String>,
    query: String,
    scanning: bool,
    searching: bool,
}

impl FileSearch {
    pub fn new() -> Self {
        Self {
            matcher: Nucleo::new(Config::DEFAULT.match_paths(), Arc::new(|| {}), Some(2), 1),
            results: Vec::new(),
            query: String::new(),
            scanning: true,
            searching: false,
        }
    }

    pub fn injector(&self) -> Injector<String> {
        self.matcher.injector()
    }

    pub fn set_query(&mut self, query: &str) {
        if self.query == query {
            return;
        }
        self.query = query.to_owned();
        self.results.clear();
        self.matcher
            .pattern
            .reparse(0, query, CaseMatching::Ignore, Normalization::Smart, false);
        self.searching = true;
    }

    pub fn scan_complete(&mut self) {
        self.scanning = false;
    }

    pub fn tick(&mut self) -> bool {
        let status = self.matcher.tick(0);
        let was_searching = self.searching;
        let snapshot_is_current = self.matcher.snapshot().pattern().column_pattern(0).atoms
            == self.matcher.pattern.column_pattern(0).atoms;
        self.searching = status.running || !snapshot_is_current;
        if !status.changed || !snapshot_is_current {
            return was_searching != self.searching;
        }
        let previous = self.results.clone();
        self.results = self
            .matcher
            .snapshot()
            .matched_items(..)
            .take(RESULT_LIMIT)
            .map(|item| item.data.clone())
            .collect();
        self.results != previous || was_searching != self.searching
    }

    pub fn results(&self) -> &[String] {
        &self.results
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn loading(&self) -> bool {
        self.scanning || self.searching
    }
}

pub fn scan_project(root: &Path, injector: &Injector<String>) {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "node_modules" | "target" | ".venv" | "dist" | "build")
                ) {
                    pending.push(path);
                }
            } else if file_type.is_file()
                && let Ok(relative) = path.strip_prefix(root)
            {
                files.push(relative.to_string_lossy().into_owned());
                if files.len() >= FILE_LIMIT {
                    break;
                }
            }
        }
        if files.len() >= FILE_LIMIT {
            break;
        }
    }
    files.sort();
    for file in files {
        injector.push(file, |path, columns| {
            columns[0] = Utf32String::from(path.clone());
        });
    }
}

pub fn scan_remote_project(root: &Path, host: &str, injector: &Injector<String>) {
    let Some(root) = root.to_str() else {
        return;
    };
    let command = format!(
        "cd {} && find . -type d \\( -name .git -o -name node_modules -o -name target -o -name .venv -o -name dist -o -name build \\) -prune -o -type f -print0",
        crate::remote::quote_shell(root),
    );
    let Ok(mut child) = Command::new("ssh")
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=10",
            host,
        ])
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(stdout) = child.stdout.take() {
        let mut output = std::io::BufReader::new(stdout);
        let mut path = Vec::new();
        for _ in 0..FILE_LIMIT {
            path.clear();
            if output
                .read_until(0, &mut path)
                .ok()
                .filter(|read| *read > 0)
                .is_none()
            {
                break;
            }
            if path.last() == Some(&0) {
                path.pop();
            }
            let path = String::from_utf8_lossy(&path);
            if let Some(path) = path.strip_prefix("./") {
                injector.push(path.to_owned(), |path, columns| {
                    columns[0] = Utf32String::from(path.clone());
                });
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn indexes_project_files_and_fuzzy_matches_paths() {
        let project = std::env::temp_dir().join(format!(
            "agentaps-file-search-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(project.join("target")).unwrap();
        fs::write(project.join("src/main.rs"), "").unwrap();
        fs::write(project.join("target/ignored.rs"), "").unwrap();
        let mut search = FileSearch::new();
        scan_project(&project, &search.injector());
        search.scan_complete();
        search.set_query("smain");
        let deadline = Instant::now() + Duration::from_secs(2);
        while search.loading() && Instant::now() < deadline {
            search.tick();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(search.results(), &["src/main.rs"]);
        fs::remove_dir_all(project).unwrap();
    }
}
