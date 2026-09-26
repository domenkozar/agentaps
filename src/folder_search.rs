use nucleo::{
    Config, Injector, Nucleo, Utf32String,
    pattern::{CaseMatching, Normalization},
};
use std::{path::PathBuf, sync::Arc};

const RESULT_LIMIT: usize = 14;

pub struct FolderSearch {
    matcher: Nucleo<PathBuf>,
    recent: Vec<PathBuf>,
    defaults: Vec<PathBuf>,
    results: Vec<PathBuf>,
    direct_path: Option<PathBuf>,
    query: String,
    searching: bool,
}

impl FolderSearch {
    pub fn new(recent: Vec<PathBuf>) -> Self {
        // The workspace polls the matcher, so individual injections do not need to wake GPUI.
        let matcher = Nucleo::new(Config::DEFAULT.match_paths(), Arc::new(|| {}), Some(2), 1);
        let injector = matcher.injector();
        for path in &recent {
            inject_path(&injector, path.clone());
        }
        let defaults: Vec<PathBuf> = recent.iter().take(RESULT_LIMIT).cloned().collect();
        Self {
            matcher,
            recent,
            results: defaults.clone(),
            defaults,
            direct_path: None,
            query: String::new(),
            searching: false,
        }
    }

    pub fn injector(&self) -> Injector<PathBuf> {
        self.matcher.injector()
    }

    pub fn set_paths(&mut self, paths: &[PathBuf]) {
        self.defaults.clear();
        for path in self.recent.iter().chain(paths) {
            if !self.defaults.contains(path) {
                self.defaults.push(path.clone());
                if self.defaults.len() == RESULT_LIMIT {
                    break;
                }
            }
        }
        if self.query.is_empty() {
            self.results.clone_from(&self.defaults);
        }
    }

    pub fn add_recent(&mut self, path: PathBuf) {
        self.recent.retain(|recent| recent != &path);
        self.recent.insert(0, path.clone());
        inject_path(&self.matcher.injector(), path.clone());
        self.defaults.retain(|existing| existing != &path);
        self.defaults.insert(0, path);
        self.defaults.truncate(RESULT_LIMIT);
        if self.query.is_empty() {
            self.results.clone_from(&self.defaults);
        }
    }

    pub fn set_query(&mut self, query: &str) {
        let query = query.trim();
        if self.query == query {
            return;
        }
        self.query.clear();
        self.query.push_str(query);
        self.direct_path = if query.is_empty() {
            None
        } else {
            if crate::remote::parse_project(query).ok().flatten().is_some() {
                Some(PathBuf::from(query))
            } else {
                PathBuf::from(query)
                    .canonicalize()
                    .ok()
                    .filter(|path| path.is_dir())
            }
        };
        self.results.clear();
        if query.is_empty() {
            self.results.clone_from(&self.defaults);
            self.searching = false;
        } else {
            if let Some(path) = &self.direct_path {
                self.results.push(path.clone());
            }
            self.matcher.pattern.reparse(
                0,
                query,
                CaseMatching::Ignore,
                Normalization::Smart,
                false,
            );
            self.searching = true;
        }
    }

    pub fn tick(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        let status = self.matcher.tick(0);
        let was_searching = self.searching;
        let snapshot_is_current = self.matcher.snapshot().pattern().column_pattern(0).atoms
            == self.matcher.pattern.column_pattern(0).atoms;
        self.searching = status.running || !snapshot_is_current;
        if !status.changed || !snapshot_is_current {
            return self.searching != was_searching;
        }
        let previous = self.results.clone();
        self.results.clear();
        if let Some(path) = &self.direct_path {
            self.results.push(path.clone());
        }
        for item in self.matcher.snapshot().matched_items(..).take(RESULT_LIMIT) {
            if !self.results.contains(item.data) {
                self.results.push(item.data.clone());
                if self.results.len() == RESULT_LIMIT {
                    break;
                }
            }
        }
        self.results != previous || self.searching != was_searching
    }

    pub fn results(&self) -> &[PathBuf] {
        &self.results
    }

    pub fn searching(&self) -> bool {
        self.searching
    }
}

pub fn inject_path(injector: &Injector<PathBuf>, path: PathBuf) {
    injector.push(path, |path, columns| {
        columns[0] = Utf32String::from(path.to_string_lossy().into_owned());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn fuzzy_results_replace_the_previous_query_and_empty_query_restores_defaults() {
        let terminal = PathBuf::from("/work/dev-terminal");
        let mut search =
            FolderSearch::new(vec![PathBuf::from("/work/terminal-dev"), terminal.clone()]);

        search.set_query("devterm");
        let deadline = Instant::now() + Duration::from_secs(2);
        while search.searching() && Instant::now() < deadline {
            search.tick();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(search.results(), &[terminal]);

        search.set_query("missing");
        assert!(search.results().is_empty());
        search.set_query("");
        assert_eq!(search.results().len(), 2);
    }

    #[test]
    fn existing_directory_is_available_immediately() {
        let path = std::env::temp_dir().canonicalize().unwrap();
        let mut search = FolderSearch::new(Vec::new());
        search.set_query(path.to_str().unwrap());
        assert_eq!(search.results().first(), Some(&path));
    }
}
