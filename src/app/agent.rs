use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Connecting,
    Idle,
    Working,
    Done,
    Error,
}

impl Status {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Done => "done",
            Self::Error => "error",
        }
    }

    pub(super) fn color(self) -> u32 {
        match self {
            Self::Connecting => STATUS_CONNECTING,
            Self::Idle => STATUS_IDLE,
            Self::Working => STATUS_WORKING,
            Self::Done => STATUS_DONE,
            Self::Error => STATUS_ERROR,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RestoreMode {
    Resume,
    Load,
}

pub(super) fn restore_mode(protocol: ProtocolVersion, initialize: &Value) -> Option<RestoreMode> {
    match protocol {
        ProtocolVersion::V2 => Some(RestoreMode::Resume),
        ProtocolVersion::V1 => {
            let capabilities = &initialize["agentCapabilities"];
            if capabilities["sessionCapabilities"]["resume"].is_object() {
                Some(RestoreMode::Resume)
            } else if capabilities["loadSession"].as_bool() == Some(true) {
                Some(RestoreMode::Load)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(super) struct Permission {
    pub(super) request_id: Value,
    pub(super) title: String,
    pub(super) description: Option<String>,
    pub(super) options: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConfigChoice {
    pub(super) value: String,
    pub(super) label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConfigSelectOption {
    pub(super) id: String,
    pub(super) current: String,
    pub(super) choices: Vec<ConfigChoice>,
}

impl ConfigSelectOption {
    pub(super) fn label(&self) -> String {
        self.choices
            .iter()
            .find(|choice| choice.value == self.current)
            .map(|choice| choice.label.clone())
            .unwrap_or_else(|| self.current.clone())
    }
}

#[derive(Clone, Copy)]
pub(super) enum ConfigOptionKind {
    Model,
    Effort,
}

pub(super) struct AgentView {
    pub(super) config: AgentConfig,
    pub(super) name: String,
    pub(super) model: Option<String>,
    pub(super) model_option: Option<ConfigSelectOption>,
    pub(super) pending_model: Option<(u64, String)>,
    pub(super) effort_option: Option<ConfigSelectOption>,
    pub(super) pending_effort: Option<(u64, String)>,
    pub(super) context: Option<(u64, u64)>,
    pub(super) status: Status,
    pub(super) protocol: Option<ProtocolVersion>,
    pub(super) active_work: bool,
    pub(super) awaiting_response: bool,
    pub(super) cancel_requested: bool,
    pub(super) session_id: Option<String>,
    pub(super) restoring: Option<RestoreMode>,
    pub(super) next_request_id: u64,
    pub(super) messages: Vec<ChatEntry>,
    pub(super) permissions: Vec<Permission>,
    pub(super) elicitations: Vec<Elicitation>,
    pub(super) connection: Option<Connection>,
}

pub(super) fn agent_name(command: &[String]) -> String {
    command
        .first()
        .and_then(|name| Path::new(name).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Agent".into())
}

pub(super) fn model_option(config_options: &Value) -> Option<ConfigSelectOption> {
    let option = config_options.as_array()?.iter().find(|option| {
        option["category"].as_str() == Some("model")
            || option["configId"].as_str() == Some("model")
            || option["id"].as_str() == Some("model")
    })?;
    parse_config_select_option(option)
}

pub(super) fn effort_option(config_options: &Value) -> Option<ConfigSelectOption> {
    let option = config_options.as_array()?.iter().find(|option| {
        let id = option["configId"]
            .as_str()
            .or_else(|| option["id"].as_str())
            .unwrap_or_default();
        option["category"].as_str() == Some("thought_level")
            || id.contains("effort")
            || (option["category"].as_str() == Some("model_config")
                && option["name"]
                    .as_str()
                    .is_some_and(|name| name.to_lowercase().contains("reasoning")))
    })?;
    parse_config_select_option(option)
}

fn parse_config_select_option(option: &Value) -> Option<ConfigSelectOption> {
    let id = option["configId"]
        .as_str()
        .or_else(|| option["id"].as_str())?;
    let current = option["currentValue"].as_str()?;
    let options = option["options"].as_array()?;
    let choices = options
        .iter()
        .flat_map(|entry| {
            entry["options"]
                .as_array()
                .map_or_else(|| vec![entry], |group| group.iter().collect())
        })
        .filter_map(|choice| {
            let value = choice["value"].as_str()?;
            let label = if value == "default" && option["category"].as_str() == Some("model") {
                choice["description"]
                    .as_str()
                    .and_then(|description| description.split(" · ").next())
                    .or_else(|| choice["name"].as_str())
            } else {
                choice["name"].as_str()
            }
            .unwrap_or(value);
            Some(ConfigChoice {
                value: value.to_owned(),
                label: label.to_owned(),
            })
        })
        .collect();
    Some(ConfigSelectOption {
        id: id.to_owned(),
        current: current.to_owned(),
        choices,
    })
}

pub(super) fn set_config_option_request(
    id: u64,
    session_id: &str,
    option: &ConfigSelectOption,
    value: &str,
) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"session/set_config_option","params":{
        "sessionId":session_id,"configId":option.id,"type":"id","value":value
    }})
}

impl AgentView {
    pub(super) fn reset_config(&self, id: u64, mut messages: Vec<ChatEntry>) -> AgentConfig {
        for entry in &mut messages {
            entry.key = None;
        }
        AgentConfig {
            id,
            command: self.config.command.clone(),
            archived: self.config.archived,
            display_name: self.config.display_name.clone(),
            session_id: None,
            model: None,
            context: None,
            messages,
            available_commands: Vec::new(),
            pending_prompts: Vec::new(),
            prompt_history: self.config.prompt_history.clone(),
            was_working: false,
            session_has_activity: false,
        }
    }

    pub(super) fn mark_viewed(&mut self) {
        if self.status == Status::Done {
            self.status = Status::Idle;
        }
    }

    pub(super) fn new(mut config: AgentConfig) -> Self {
        if config.prompt_history.is_empty() {
            config.prompt_history = config
                .messages
                .iter()
                .filter(|entry| entry.role == Role::User)
                .map(|entry| entry.text.clone())
                .chain(config.pending_prompts.iter().cloned())
                .collect();
        }
        let name = config
            .display_name
            .clone()
            .unwrap_or_else(|| agent_name(&config.command));
        let mut messages = config
            .messages
            .iter()
            .filter(|entry| {
                config.session_has_activity
                    || entry.role != Role::System
                    || !(entry
                        .text
                        .starts_with("Could not restore the previous session:")
                        || entry
                            .text
                            .starts_with("This agent cannot restore sessions."))
            })
            .cloned()
            .collect::<Vec<_>>();
        if config.was_working {
            messages.push(ChatEntry {
                role: Role::System,
                key: None,
                text: "App closed while this turn was active.".into(),
            });
        }
        let model = config.model.clone();
        let context = config.context;
        Self {
            config,
            name,
            model,
            model_option: None,
            pending_model: None,
            effort_option: None,
            pending_effort: None,
            context,
            status: Status::Connecting,
            protocol: None,
            active_work: false,
            awaiting_response: false,
            cancel_requested: false,
            session_id: None,
            restoring: None,
            next_request_id: 3,
            messages,
            permissions: Vec::new(),
            elicitations: Vec::new(),
            connection: None,
        }
    }

    pub(super) fn snapshot(&self) -> AgentConfig {
        AgentConfig {
            id: self.config.id,
            command: self.config.command.clone(),
            archived: self.config.archived,
            display_name: self.config.display_name.clone(),
            session_id: self
                .session_id
                .clone()
                .or_else(|| self.config.session_id.clone()),
            model: self.model.clone(),
            context: self.context,
            messages: self.messages.clone(),
            available_commands: self.config.available_commands.clone(),
            pending_prompts: self.config.pending_prompts.clone(),
            prompt_history: self.config.prompt_history.clone(),
            was_working: self.active_work,
            session_has_activity: self.config.session_has_activity || self.active_work,
        }
    }

    pub(super) fn has_restorable_activity(&self) -> bool {
        self.config.session_has_activity || self.config.was_working
    }

    pub(super) fn send(&mut self, value: Value) -> Result<(), String> {
        self.connection
            .as_ref()
            .ok_or("Agent is not connected".into())
            .and_then(|connection| connection.send(value))
    }

    pub(super) fn start_prompt(&mut self, prompt: String) -> Result<(), String> {
        let session_id = self.session_id.clone().ok_or("Agent is still connecting")?;
        let id = self.next_request_id;
        let agent_prompt = prompt_for_agent(&prompt);
        self.send(
            json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{
                "sessionId":session_id,"prompt":[{"type":"text","text":agent_prompt}]
            }}),
        )?;
        self.next_request_id += 1;
        if self.protocol == Some(ProtocolVersion::V1) {
            self.log(Role::User, prompt);
        }
        self.active_work = true;
        self.awaiting_response = true;
        self.cancel_requested = false;
        self.config.session_has_activity = true;
        self.status = Status::Working;
        Ok(())
    }

    pub(super) fn start_next_queued_prompt(&mut self) -> bool {
        if self.active_work
            || self.restoring.is_some()
            || !matches!(self.status, Status::Idle | Status::Done)
        {
            return false;
        }
        let Some(prompt) = self.config.pending_prompts.first().cloned() else {
            return false;
        };
        match self.start_prompt(prompt) {
            Ok(()) => {
                self.config.pending_prompts.remove(0);
            }
            Err(error) => {
                self.status = Status::Error;
                self.log(
                    Role::System,
                    format!("Could not send queued message: {error}"),
                );
            }
        }
        true
    }

    pub(super) fn log(&mut self, role: Role, text: impl Into<String>) {
        if role != Role::System {
            self.config.session_has_activity = true;
        }
        self.messages.push(ChatEntry {
            role,
            key: None,
            text: text.into(),
        });
    }

    pub(super) fn upsert_message(
        &mut self,
        role: Role,
        id: &str,
        content: Option<&Value>,
        append: bool,
    ) {
        self.config.session_has_activity = true;
        let key = format!("message:{id}");
        let entry = if let Some(index) = self
            .messages
            .iter()
            .position(|entry| entry.key.as_deref() == Some(&key))
        {
            &mut self.messages[index]
        } else {
            self.messages.push(ChatEntry {
                role,
                key: Some(key),
                text: String::new(),
            });
            self.messages.last_mut().unwrap()
        };
        if let Some(content) = content {
            let text = content_text(content);
            if append {
                entry.text.push_str(&text);
            } else {
                entry.text = text;
            }
        }
    }

    pub(super) fn upsert_tool_call(&mut self, update: &Value) {
        let Some(id) = update["toolCallId"].as_str() else {
            if let Some(title) = update["title"].as_str() {
                self.log(Role::Tool, title);
            }
            return;
        };
        self.config.session_has_activity = true;
        let key = format!("tool:{id}");
        let existing = self
            .messages
            .iter()
            .position(|entry| entry.key.as_deref() == Some(&key));
        let previous = existing.map(|index| tool_title_and_status(&self.messages[index].text));
        let title = update["title"]
            .as_str()
            .or_else(|| previous.map(|(title, _)| title))
            .unwrap_or("Using tool");
        let status = update["status"]
            .as_str()
            .or_else(|| previous.and_then(|(_, status)| status));
        let text = if let Some(status) = status {
            format!("{title} · {status}")
        } else {
            title.to_owned()
        };
        if let Some(index) = existing {
            let details = self.messages[index]
                .text
                .split_once('\n')
                .map(|(_, details)| details.to_owned());
            self.messages[index].text = if let Some(details) = details {
                format!("{text}\n{details}")
            } else {
                text
            };
        } else {
            self.messages.push(ChatEntry {
                role: Role::Tool,
                key: Some(key),
                text,
            });
        }
    }

    pub(super) fn handle_prompt_response(&mut self, result: &Value) {
        if self.protocol == Some(ProtocolVersion::V2) {
            if let Err(error) = serde_json::from_value::<v2::PromptResponse>(result.clone()) {
                self.status = Status::Error;
                self.active_work = false;
                self.awaiting_response = false;
                self.cancel_requested = false;
                self.log(
                    Role::System,
                    format!("Invalid ACP v2 prompt response: {error}"),
                );
            }
        } else {
            self.active_work = false;
            self.awaiting_response = false;
            self.cancel_requested = false;
            self.status = Status::Done;
            if result["stopReason"].as_str() == Some("cancelled") {
                self.log(Role::System, "Turn cancelled");
            }
        }
    }
}

pub(super) fn content_text(content: &Value) -> String {
    if let Some(items) = content.as_array() {
        return items
            .iter()
            .map(content_text)
            .collect::<Vec<_>>()
            .join("\n");
    }
    match content["type"].as_str() {
        Some("text") => content["text"].as_str().unwrap_or_default().to_owned(),
        Some("resource_link") => content["uri"].as_str().unwrap_or("[resource]").to_owned(),
        Some("image") => "[image]".into(),
        Some("audio") => "[audio]".into(),
        Some("resource") => "[resource]".into(),
        Some(kind) => format!("[{kind}]"),
        None => String::new(),
    }
}
