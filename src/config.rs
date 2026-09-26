use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Agent,
    Thought,
    Tool,
    System,
    ContextReset,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatEntry {
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
}

impl SlashCommand {
    pub fn input_hint(&self) -> Option<&str> {
        self.input.as_ref().and_then(|input| input["hint"].as_str())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: u64,
    pub command: Vec<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<(u64, u64)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<ChatEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_commands: Vec<SlashCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_prompts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_history: Vec<String>,
    #[serde(default)]
    pub was_working: bool,
    #[serde(default)]
    pub session_has_activity: bool,
    #[serde(default)]
    pub fork_pending: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_host: Option<String>,
    #[serde(default)]
    pub agents: Vec<AgentConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub projects: Vec<ProjectConfig>,
    #[serde(default)]
    pub sidebar_order: Vec<u64>,
    #[serde(default = "default_sidebar_fraction")]
    pub sidebar_fraction: f32,
}

fn default_sidebar_fraction() -> f32 {
    0.2
}

impl Default for Config {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            sidebar_order: Vec::new(),
            sidebar_fraction: default_sidebar_fraction(),
        }
    }
}

fn config_base() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or("HOME and XDG_CONFIG_HOME are unset")?;
    Ok(base)
}

pub fn path() -> Result<PathBuf, String> {
    Ok(config_base()?.join("agentaps").join("config.json"))
}

pub fn load() -> Result<(Config, bool), String> {
    load_from_base(&config_base()?)
}

fn load_from_base(base: &Path) -> Result<(Config, bool), String> {
    let current = base.join("agentaps").join("config.json");
    let legacy = base.join("devenv-terminal").join("config.json");
    let (path, needs_migration) = if current.exists() {
        (current, false)
    } else if legacy.exists() {
        (legacy, true)
    } else {
        return Ok((Config::default(), false));
    };
    let config = serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok((config, needs_migration))
}

pub fn save(config: &Config) -> Result<(), String> {
    let path = path()?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|error| error.to_string())?;
    let data = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    file.write_all(&data).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn loads_legacy_config_only_when_new_config_is_absent() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base =
            std::env::temp_dir().join(format!("agentaps-config-{}-{stamp}", std::process::id()));
        let legacy = base.join("devenv-terminal").join("config.json");
        let current = base.join("agentaps").join("config.json");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, r#"{"sidebar_order":[1]}"#).unwrap();

        let (config, needs_migration) = load_from_base(&base).unwrap();
        assert_eq!(config.sidebar_order, vec![1]);
        assert!(needs_migration);

        fs::create_dir_all(current.parent().unwrap()).unwrap();
        fs::write(&current, r#"{"sidebar_order":[2]}"#).unwrap();
        let (config, needs_migration) = load_from_base(&base).unwrap();
        assert_eq!(config.sidebar_order, vec![2]);
        assert!(!needs_migration);

        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn sidebar_order_is_saved_and_old_configs_still_load() {
        let old: Config = serde_json::from_str(r#"{"projects":[]}"#).unwrap();
        assert!(old.sidebar_order.is_empty());

        let config = Config {
            projects: vec![],
            sidebar_order: vec![3, 1, 2],
            sidebar_fraction: 0.32,
        };
        let restored: Config =
            serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap();
        assert_eq!(restored.sidebar_order, vec![3, 1, 2]);
        assert_eq!(restored.sidebar_fraction, 0.32);
    }

    #[test]
    fn old_agent_config_loads_without_session_history() {
        let agent: AgentConfig =
            serde_json::from_str(r#"{"id":7,"command":["agent"],"display_name":"Agent"}"#).unwrap();
        assert!(agent.session_id.is_none());
        assert!(!agent.archived);
        assert!(agent.messages.is_empty());
        assert!(!agent.was_working);
        assert!(!agent.session_has_activity);
    }
}
