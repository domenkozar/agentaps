use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteProject {
    pub host: String,
    pub path: PathBuf,
}

pub fn parse_project(value: &str) -> Result<Option<RemoteProject>, String> {
    let Some(rest) = value.strip_prefix("ssh://") else {
        return Ok(None);
    };
    let Some((host, path)) = rest.split_once('/') else {
        return Err("Use ssh://user@host/absolute/path".into());
    };
    if host.is_empty()
        || host.starts_with('-')
        || !host.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '@' | '.' | '-' | '_' | ':' | '[' | ']')
        })
    {
        return Err("Invalid SSH host".into());
    }
    let path = PathBuf::from(format!("/{path}"));
    if path == Path::new("/") || path.to_string_lossy().chars().any(char::is_control) {
        return Err("Enter an absolute project path on the SSH host".into());
    }
    Ok(Some(RemoteProject {
        host: host.into(),
        path,
    }))
}

pub fn project_label(host: &str, path: &Path) -> String {
    format!("ssh://{host}{}", path.display())
}

pub(crate) fn quote_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn agent_command(command: &[String], path: &Path) -> Result<String, String> {
    if command.is_empty() {
        return Err("Agent command is empty".into());
    }
    let path = path
        .to_str()
        .ok_or("Remote project path is not valid UTF-8")?;
    let command = command
        .iter()
        .map(|part| quote_shell(part))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(format!("cd {} && exec {command}", quote_shell(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_project_and_rejects_ssh_options() {
        assert_eq!(
            parse_project("ssh://alice@server.example/home/alice/repo").unwrap(),
            Some(RemoteProject {
                host: "alice@server.example".into(),
                path: PathBuf::from("/home/alice/repo"),
            })
        );
        assert!(parse_project("ssh://-oProxyCommand=bad/home/repo").is_err());
        assert!(parse_project("ssh://host").is_err());
    }

    #[test]
    fn quotes_remote_command_arguments() {
        assert_eq!(
            agent_command(
                &["agent".into(), "it's safe".into()],
                Path::new("/work/my repo")
            )
            .unwrap(),
            "cd '/work/my repo' && exec 'agent' 'it'\"'\"'s safe'"
        );
    }
}
