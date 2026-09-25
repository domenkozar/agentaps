use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{collections::HashSet, path::Path, sync::mpsc::Sender};

/// Watch the checkout and the Git files that can change its HEAD or index.
pub fn start(path: &Path, generation: u64, tx: Sender<u64>) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let is_access = event
            .as_ref()
            .is_ok_and(|event| matches!(event.kind, EventKind::Access(_)));
        if !is_access {
            let _ = tx.send(generation);
        }
    })?;
    watcher.watch(path, RecursiveMode::Recursive)?;

    if let Ok(repo) = gix::discover(path) {
        let mut watched = HashSet::new();
        for (git_path, mode) in [
            (repo.git_dir().to_path_buf(), RecursiveMode::NonRecursive),
            (repo.common_dir().to_path_buf(), RecursiveMode::NonRecursive),
            (repo.common_dir().join("refs"), RecursiveMode::Recursive),
        ] {
            if git_path.exists() && !git_path.starts_with(path) && watched.insert(git_path.clone())
            {
                watcher.watch(&git_path, mode)?;
            }
        }
    }
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::mpsc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn reports_checkout_write() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "agentaps-diff-watch-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = start(&path, 7, tx).unwrap();

        fs::write(path.join("changed.txt"), "changed").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 7);

        drop(watcher);
        fs::remove_dir_all(path).unwrap();
    }
}
