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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ChatImage {
    pub mime_type: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatEntry {
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ChatImage>,
}

pub fn with_image_placeholders(text: &str, images: usize) -> String {
    std::iter::once(text)
        .filter(|text| !text.is_empty())
        .chain(std::iter::repeat_n("[image]", images))
        .collect::<Vec<_>>()
        .join("\n")
}

impl ChatEntry {
    pub fn transcript_text(&self) -> String {
        with_image_placeholders(&self.text, self.images.len())
    }
}

/// A file attached to a prompt. `text` holds the contents of a UTF-8 file
/// read when it was attached; other files are sent as a link to `uri`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatFile {
    pub name: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

pub fn file_placeholder(name: &str) -> String {
    format!("[file: {name}]")
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(from = "SavedPrompt")]
pub struct Prompt {
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ChatImage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ChatFile>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SavedPrompt {
    Text(String),
    Prompt {
        text: String,
        #[serde(default)]
        images: Vec<ChatImage>,
        #[serde(default)]
        files: Vec<ChatFile>,
    },
}

impl From<SavedPrompt> for Prompt {
    fn from(saved: SavedPrompt) -> Self {
        match saved {
            SavedPrompt::Text(text) => text.into(),
            SavedPrompt::Prompt {
                text,
                images,
                files,
            } => Self {
                text,
                images,
                files,
            },
        }
    }
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }
}

impl From<&str> for Prompt {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
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
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<(u64, u64)>,
    // Conversation text belongs to the harness. These fields are transient UI state.
    #[serde(skip)]
    pub messages: Vec<ChatEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_commands: Vec<SlashCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_prompts: Vec<Prompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_prompt: Option<String>,
    #[serde(skip)]
    pub prompt_history: Vec<String>,
    #[serde(default)]
    pub was_working: bool,
    #[serde(default)]
    pub session_has_activity: bool,
    #[serde(default)]
    pub fork_pending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_source: Option<ForkSource>,
}

impl AgentConfig {
    pub fn session_title(&self) -> Option<&str> {
        self.custom_title.as_deref().or(self.title.as_deref())
    }

    pub fn rename_session(&mut self, title: &str) {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        self.custom_title = (!title.is_empty()).then_some(title);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForkSource {
    pub session_id: String,
    /// Zero-based agent reply number in the source harness session.
    pub reply_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_key: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pane_layout: Option<crate::panes::Layout>,
    #[serde(default)]
    pub theme: crate::appearance::Choice,
    #[serde(default)]
    pub projects: Vec<ProjectConfig>,
    #[serde(default)]
    pub sidebar_order: Vec<u64>,
    #[serde(default = "default_sidebar_fraction")]
    pub sidebar_fraction: f32,
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
}

fn default_sidebar_fraction() -> f32 {
    0.2
}

fn default_font_scale() -> f32 {
    1.0
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: crate::appearance::Choice::default(),
            pane_layout: None,
            projects: Vec::new(),
            sidebar_order: Vec::new(),
            sidebar_fraction: default_sidebar_fraction(),
            font_scale: default_font_scale(),
        }
    }
}

fn config_base() -> Result<PathBuf, String> {
    use etcetera::base_strategy::{BaseStrategy, choose_base_strategy};
    // XDG on Unix preserves existing Linux and macOS locations; Windows uses
    // its known-folder API and does not require HOME to be set.
    choose_base_strategy()
        .map(|strategy| strategy.config_dir())
        .map_err(|error| error.to_string())
}

pub fn path() -> Result<PathBuf, String> {
    Ok(config_base()?.join("agentaps").join("config.json"))
}

pub fn images_path() -> Result<PathBuf, String> {
    Ok(config_base()?.join("agentaps").join("images"))
}

pub fn load() -> Result<(Config, bool), String> {
    let base = config_base()?;
    #[cfg(windows)]
    {
        // Only Windows changes platform location. An explicit Unix XDG root
        // must not fall back to a different workspace under HOME.
        let mut legacy_bases = Vec::new();
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            legacy_bases.push(PathBuf::from(xdg));
        }
        if let Some(home) = std::env::var_os("HOME") {
            legacy_bases.push(PathBuf::from(home).join(".config"));
        }
        load_from_locations(&base, &legacy_bases)
    }
    #[cfg(not(windows))]
    {
        load_from_locations(&base, &[])
    }
}

#[cfg(test)]
fn load_from_base(base: &Path) -> Result<(Config, bool), String> {
    load_from_locations(base, &[])
}

fn load_from_locations(base: &Path, legacy_bases: &[PathBuf]) -> Result<(Config, bool), String> {
    let current = base.join("agentaps").join("config.json");
    let legacy = base.join("devenv-terminal").join("config.json");
    let candidates = std::iter::once((current, false))
        .chain(std::iter::once((legacy, true)))
        .chain(legacy_bases.iter().flat_map(|base| {
            ["agentaps", "devenv-terminal"].map(|name| (base.join(name).join("config.json"), true))
        }));
    let Some((path, needs_migration)) = candidates.into_iter().find(|(path, _)| path.exists())
    else {
        return Ok((Config::default(), false));
    };
    let value: Value = serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let has_local_history = value["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|project| {
            project["agents"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|agent| {
                    agent.get("messages").is_some() || agent.get("prompt_history").is_some()
                })
        });
    let config =
        serde_json::from_value(value).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok((config, needs_migration || has_local_history))
}

pub(crate) fn save_to(path: &Path, config: &Config) -> Result<(), String> {
    fs::create_dir_all(path.parent().unwrap()).map_err(|error| error.to_string())?;
    let data = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    if fs::read(path).ok().as_ref() == Some(&data) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(path)
                .map_err(|error| error.to_string())?
                .permissions()
                .mode()
                & 0o777
                != 0o600
            {
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                    .map_err(|error| error.to_string())?;
            }
        }
        return Ok(());
    }
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
    drop(file);
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn migrates_old_platform_location_without_overriding_native_config() {
        let temp = tempfile::tempdir().unwrap();
        let native = temp.path().join("native");
        let old = temp.path().join("old");
        let old_path = old.join("agentaps/config.json");
        fs::create_dir_all(old_path.parent().unwrap()).unwrap();
        fs::write(&old_path, r#"{"sidebar_order":[7]}"#).unwrap();
        let (config, migrate) = load_from_locations(&native, std::slice::from_ref(&old)).unwrap();
        assert_eq!(config.sidebar_order, vec![7]);
        assert!(migrate);
        save_to(&native.join("agentaps/config.json"), &Config::default()).unwrap();
        let (config, migrate) = load_from_locations(&native, &[old]).unwrap();
        assert!(config.sidebar_order.is_empty());
        assert!(!migrate);
    }

    #[test]
    fn replaces_saved_config_and_keeps_private_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agentaps/config.json");
        save_to(&path, &Config::default()).unwrap();
        let expected = Config {
            sidebar_order: vec![9],
            ..Config::default()
        };
        save_to(&path, &expected).unwrap();
        let actual: Config = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(actual.sidebar_order, vec![9]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn legacy_transcripts_are_removed_while_session_references_and_queued_work_remain() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agentaps/config.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy = serde_json::json!({
            "projects": [{"path": "project", "agents": [{
                "id": 7, "command": ["agent"], "session_id": "harness-session",
                "session_has_activity": true,
                "messages": [{"role": "agent", "text": "history ".repeat(1_000_000)}],
                "prompt_history": ["earlier request"],
                "pending_prompts": ["queued request"], "active_prompt": "current request",
                "was_working": true
            }]}]
        });
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let (mut config, migrate) = load_from_base(temp.path()).unwrap();
        assert!(migrate);
        let agent = &config.projects[0].agents[0];
        assert!(agent.messages.is_empty());
        assert!(agent.prompt_history.is_empty());
        assert_eq!(agent.session_id.as_deref(), Some("harness-session"));
        assert_eq!(agent.pending_prompts, [Prompt::from("queued request")]);
        assert_eq!(agent.active_prompt.as_deref(), Some("current request"));
        assert!(agent.was_working);
        save_to(&path, &config).unwrap();
        assert!(fs::metadata(&path).unwrap().len() < 1024);
        assert!(!load_from_base(temp.path()).unwrap().1);
        let original = fs::read(&path).unwrap();
        let old_time = UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        // Streaming and prompt recall only change transient UI state.
        config.projects[0].agents[0].messages.push(ChatEntry {
            role: Role::Agent,
            key: None,
            text: "new reply".into(),
            images: Vec::new(),
        });
        config.projects[0].agents[0]
            .prompt_history
            .push("another past request".into());
        save_to(&path, &config).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }

    #[cfg(windows)]
    #[test]
    fn windows_config_does_not_require_unix_environment() {
        const CHILD: &str = "AGENTAPS_CONFIG_TEST_ROOT";
        if let Some(root) = std::env::var_os(CHILD) {
            assert_eq!(
                path().unwrap(),
                PathBuf::from(root).join("agentaps/config.json")
            );
            return;
        }
        // Environment removal occurs only in the child, keeping parallel tests safe.
        let temp = tempfile::tempdir().unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "config::tests::windows_config_does_not_require_unix_environment",
            ])
            .env_remove("HOME")
            .env_remove("XDG_CONFIG_HOME")
            .env("APPDATA", temp.path())
            .env(CHILD, temp.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

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
        assert_eq!(old.font_scale, 1.0);
        assert_eq!(old.theme, crate::appearance::Choice::Agentaps);

        let config = Config {
            projects: vec![],
            sidebar_order: vec![3, 1, 2],
            sidebar_fraction: 0.32,
            font_scale: 1.25,
            theme: crate::appearance::Choice::SolarizedLight,
            pane_layout: None,
        };
        let restored: Config =
            serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap();
        assert_eq!(restored.sidebar_order, vec![3, 1, 2]);
        assert_eq!(restored.sidebar_fraction, 0.32);
        assert_eq!(restored.font_scale, 1.25);
        assert_eq!(restored.theme, crate::appearance::Choice::SolarizedLight);
    }

    #[test]
    fn pane_layout_is_optional_in_legacy_configs_and_survives_saving() {
        let old: Config = serde_json::from_str(r#"{"sidebar_order":[1]}"#).unwrap();
        assert!(old.pane_layout.is_none());
        let mut layout = crate::panes::Layout::default();
        layout.split(1, crate::panes::Direction::Down, 2, 3);
        let config = Config {
            pane_layout: Some(layout.clone()),
            ..old
        };
        let restored: Config =
            serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap();
        assert_eq!(restored.pane_layout, Some(layout));
    }

    #[test]
    fn old_agent_config_loads_without_session_history() {
        let agent: AgentConfig =
            serde_json::from_str(r#"{"id":7,"command":["agent"],"display_name":"Agent"}"#).unwrap();
        assert!(agent.session_id.is_none());
        assert!(agent.title.is_none());
        assert!(!agent.archived);
        assert!(agent.messages.is_empty());
        assert!(!agent.was_working);
        assert!(!agent.session_has_activity);
    }
}
