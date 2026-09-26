use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentChoice {
    pub name: String,
    pub detail: String,
    pub command: Vec<String>,
}

pub fn find_executable(name: &str) -> Option<PathBuf> {
    fn is_executable(path: &Path) -> bool {
        let Ok(metadata) = fs::metadata(path) else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    }
    let path = Path::new(name);
    if path.components().count() > 1 {
        return is_executable(path).then(|| path.to_path_buf());
    }
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|path| is_executable(path))
}

fn executable(name: &str) -> bool {
    find_executable(name).is_some()
}

pub fn installed_agents() -> Vec<AgentChoice> {
    let mut agents = Vec::new();
    let mut add = |name: &str, detail: &str, command: &[&str]| {
        if executable(command[0]) {
            agents.push(AgentChoice {
                name: name.into(),
                detail: detail.into(),
                command: command.iter().map(|part| (*part).into()).collect(),
            });
        }
    };
    add("Codex", "Installed ACP adapter", &["codex-acp"]);
    add("Claude", "Installed ACP adapter", &["claude-agent-acp"]);
    add("Claude", "Installed ACP adapter", &["claude-code-acp"]);
    add("Gemini CLI", "Native ACP mode", &["gemini", "--acp"]);
    add("OpenCode", "Native ACP mode", &["opencode", "acp"]);
    if executable("npx") {
        if executable("codex") && !agents.iter().any(|agent| agent.name == "Codex") {
            agents.push(AgentChoice {
                name: "Codex".into(),
                detail: "Installed CLI · adapter downloaded with npx on first launch".into(),
                command: vec![
                    "npx".into(),
                    "-y".into(),
                    "--prefer-offline".into(),
                    "@agentclientprotocol/codex-acp".into(),
                ],
            });
        }
        if executable("claude") && !agents.iter().any(|agent| agent.name == "Claude") {
            agents.push(AgentChoice {
                name: "Claude".into(),
                detail: "Installed CLI · adapter downloaded with npx on first launch".into(),
                command: vec![
                    "npx".into(),
                    "-y".into(),
                    "--prefer-offline".into(),
                    "@agentclientprotocol/claude-agent-acp".into(),
                ],
            });
        }
    }
    agents
}

pub fn score(query: &str, candidate: &str) -> Option<i32> {
    let query = query.to_lowercase();
    let candidate = candidate.to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let mut chars = candidate.char_indices();
    let mut last = None;
    let mut score = 0;
    for needle in query.chars() {
        let (offset, _) = chars.find(|(_, character)| *character == needle)?;
        score += if offset == 0
            || candidate[..offset]
                .chars()
                .last()
                .is_some_and(|character| matches!(character, '/' | '_' | '-' | ' '))
        {
            12
        } else {
            2
        };
        if last.is_some_and(|previous| previous + 1 == offset) {
            score += 5;
        }
        last = Some(offset);
    }
    Some(score - (candidate.len() as i32 / 8))
}

pub fn discover_folders() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.clone());
        roots.push(home.join("dev"));
        roots.push(home.join("projects"));
    }
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd);
    }
    let mut found = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = roots
        .into_iter()
        .map(|path| (path, 0usize))
        .collect::<Vec<_>>();
    while let Some((path, depth)) = stack.pop() {
        if found.len() >= 30_000 {
            break;
        }
        let Ok(path) = path.canonicalize() else {
            continue;
        };
        if !visited.insert(path.clone()) {
            continue;
        }
        found.push(path.clone());
        if depth >= 5 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                || matches!(
                    name.as_ref(),
                    "node_modules" | "target" | "result" | "vendor" | "dist" | "build"
                )
            {
                continue;
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push((entry.path(), depth + 1));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match_accepts_subsequences() {
        assert!(score("abc", "a_b-c").is_some());
        assert!(score("xyz", "dev-terminal").is_none());
    }
}
