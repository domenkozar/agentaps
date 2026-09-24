mod acp;
mod config;
mod discovery;
mod theme;

use acp::{Connection, Event};
use agent_client_protocol_schema::{ProtocolVersion, v2};
use config::{AgentConfig, ChatEntry, Config, ProjectConfig, Role, SlashCommand};
use discovery::{AgentChoice, discover_folders, folder_matches, installed_agents, score};
use gpui::{
    App, Application, Bounds, Context, DragMoveEvent, Entity, Focusable, IntoElement, KeyBinding,
    KeyDownEvent, MouseButton, Render, ScrollHandle, StatefulInteractiveElement, Subscription,
    Timer, Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, relative, rems, rgb,
    size,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Root,
    input::{
        Enter, Escape, IndentInline, Input, InputEvent, InputState, MoveDown, MoveUp, Position,
    },
    scroll::ScrollableElement,
    text::{TextView, TextViewStyle},
    tooltip::Tooltip,
};
use gpui_component_assets::Assets;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};
use theme::*;

fn move_sidebar_id(order: &mut Vec<u64>, dragged: u64, target: u64) -> bool {
    let Some(from) = order.iter().position(|id| *id == dragged) else {
        return false;
    };
    let Some(to) = order.iter().position(|id| *id == target) else {
        return false;
    };
    if from == to {
        return false;
    }
    order.remove(from);
    let target_index = order.iter().position(|id| *id == target).unwrap();
    let insert_at = if from < to {
        target_index + 1
    } else {
        target_index
    };
    order.insert(insert_at, dragged);
    true
}

fn tool_run_end(messages: &[ChatEntry], start: usize) -> usize {
    let mut end = start;
    while end < messages.len() && messages[end].role == Role::Tool {
        end += 1;
    }
    end
}

fn tool_title_and_status(text: &str) -> (&str, Option<&str>) {
    let headline = text.lines().next().unwrap_or(text);
    let Some((title, status)) = headline.rsplit_once(" · ") else {
        return (headline, None);
    };
    if matches!(status, "pending" | "in_progress" | "completed" | "failed") {
        (title, Some(status))
    } else {
        (headline, None)
    }
}

fn markdown_code_block(text: &str) -> String {
    let fence_size = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        .max(2)
        + 1;
    let fence = "`".repeat(fence_size);
    format!("{fence}\n{text}\n{fence}")
}

fn is_approval_review(text: &str) -> bool {
    tool_title_and_status(text).0 == "Guardian Review"
}

fn is_generic_tool_title(text: &str) -> bool {
    matches!(tool_title_and_status(text).0, "Terminal" | "Using tool")
}

fn shell_steps(script: &str) -> Vec<String> {
    let mut steps = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut chars = script.chars().peekable();
    while let Some(character) = chars.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            current.push(character);
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            current.push(character);
            if character == open {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            current.push(character);
            quote = Some(character);
            continue;
        }
        if matches!(character, ';' | '|') || (character == '&' && chars.peek() == Some(&'&')) {
            if !current.trim().is_empty() {
                steps.push(current.trim().to_owned());
            }
            current.clear();
            if character == '&' || (character == '|' && chars.peek() == Some(&'|')) {
                chars.next();
            }
            continue;
        }
        current.push(character);
    }
    if !current.trim().is_empty() {
        steps.push(current.trim().to_owned());
    }
    steps
}

fn simple_tool_description(script: &str) -> (String, bool) {
    let words = shell_words::split(script).unwrap_or_default();
    let command = words.first().map(String::as_str).unwrap_or_default();
    let rest = words.get(1..).unwrap_or(&[]);
    if words
        .iter()
        .any(|word| matches!(word.as_str(), ">" | ">>" | "<"))
    {
        return (script.to_owned(), false);
    }
    let basename = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command);
    match basename {
        "cd" => ("Change folder".into(), true),
        "rg" if rest.iter().any(|arg| arg == "--files") => ("List files".into(), true),
        "rg" | "grep" => {
            let operands = rest
                .iter()
                .filter(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .collect::<Vec<_>>();
            if let Some(pattern) = operands.first() {
                let location = if operands.len() > 1 {
                    format!(" in {}", operands[1..].join(", "))
                } else {
                    String::new()
                };
                (format!("Search {pattern}{location}"), true)
            } else {
                (script.to_owned(), false)
            }
        }
        "cat" | "head" | "tail" => {
            let files = rest
                .iter()
                .filter(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .collect::<Vec<_>>();
            if files.is_empty() {
                (script.to_owned(), false)
            } else {
                (format!("Read {}", files.join(", ")), true)
            }
        }
        "sed"
            if !rest
                .iter()
                .any(|arg| arg == "-i" || arg.starts_with("--in-place")) =>
        {
            if let Some(file) = rest.last().filter(|arg| !arg.starts_with('-')) {
                (format!("Read {file}"), true)
            } else {
                (script.to_owned(), false)
            }
        }
        "ls" => {
            let locations = rest
                .iter()
                .filter(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .collect::<Vec<_>>();
            if locations.is_empty() {
                ("List files".into(), true)
            } else {
                (format!("List {}", locations.join(", ")), true)
            }
        }
        "pwd" => ("Show working folder".into(), true),
        "find" => (
            format!(
                "Find files in {}",
                rest.first().map(String::as_str).unwrap_or(".")
            ),
            true,
        ),
        "git" => match rest.first().map(String::as_str) {
            Some("status") => ("Check git status".into(), true),
            Some("log") => ("Inspect git history".into(), true),
            Some("diff") => ("Review changes".into(), true),
            Some("show") => ("Inspect commit".into(), true),
            _ => (script.to_owned(), false),
        },
        "wc" => ("Count output".into(), true),
        "xargs" if rest.first().is_some_and(|arg| arg == "wc") => ("Count output".into(), true),
        _ => (script.to_owned(), false),
    }
}

fn tool_description(text: &str) -> (String, bool) {
    let (title, _) = tool_title_and_status(text);
    let args = shell_words::split(title).unwrap_or_default();
    let script = if args.first().is_some_and(|arg| {
        matches!(
            Path::new(arg).file_name().and_then(|name| name.to_str()),
            Some("bash" | "sh")
        )
    }) {
        args.windows(2)
            .find(|pair| matches!(pair[0].as_str(), "-c" | "-lc"))
            .map(|pair| pair[1].as_str())
            .unwrap_or(title)
    } else {
        title
    };
    let steps = shell_steps(script);
    if steps.len() <= 1 {
        return simple_tool_description(script);
    }
    let descriptions = steps
        .iter()
        .map(|step| simple_tool_description(step))
        .collect::<Vec<_>>();
    if descriptions.iter().any(|(_, exploratory)| !exploratory) {
        return (script.to_owned(), false);
    }
    let mut labels = Vec::new();
    for (label, _) in descriptions {
        if label != "Change folder" && !labels.contains(&label) {
            labels.push(label);
        }
    }
    if labels.is_empty() {
        return ("Inspect folder".into(), true);
    }
    let remaining = labels.len().saturating_sub(2);
    let mut summary = labels.into_iter().take(2).collect::<Vec<_>>().join(" · ");
    if remaining > 0 {
        summary.push_str(&format!(" · {remaining} more"));
    }
    (summary, true)
}

fn tool_group_heading(entries: &[ChatEntry]) -> &'static str {
    let has_action = entries
        .iter()
        .any(|entry| !is_approval_review(&entry.text) && !is_generic_tool_title(&entry.text));
    if !has_action && entries.iter().any(|entry| is_approval_review(&entry.text)) {
        "Approval checks"
    } else {
        "Ran"
    }
}

fn approval_summary(entries: &[ChatEntry]) -> Option<String> {
    let mut total = 0;
    let mut completed = 0;
    let mut failed = 0;
    let mut pending = 0;
    for entry in entries
        .iter()
        .filter(|entry| is_approval_review(&entry.text))
    {
        total += 1;
        match tool_title_and_status(&entry.text).1 {
            Some("completed") => completed += 1,
            Some("failed") => failed += 1,
            Some("pending" | "in_progress") => pending += 1,
            _ => {}
        }
    }
    if total == 0 {
        return None;
    }
    let noun = if total == 1 { "check" } else { "checks" };
    let status = if failed > 0 {
        format!(" · {failed} failed")
    } else if pending > 0 {
        format!(" · {pending} in progress")
    } else if completed == total {
        " · passed".into()
    } else {
        String::new()
    };
    Some(format!("{total} approval {noun}{status}"))
}

actions!(workspace, [QuickOpen]);

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickerMode {
    Closed,
    Folders,
    Agents,
}

#[derive(Clone, Copy)]
enum SlashAction {
    Up,
    Down,
    Complete,
    Dismiss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Connecting,
    Idle,
    Working,
    Done,
    Error,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Done => "done",
            Self::Error => "error",
        }
    }

    fn color(self) -> u32 {
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
enum RestoreMode {
    Resume,
    Load,
}

fn restore_mode(protocol: ProtocolVersion, initialize: &Value) -> Option<RestoreMode> {
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

struct Permission {
    request_id: Value,
    title: String,
    description: Option<String>,
    options: Vec<(String, String)>,
}

struct AgentView {
    config: AgentConfig,
    name: String,
    model: Option<String>,
    context: Option<(u64, u64)>,
    status: Status,
    protocol: Option<ProtocolVersion>,
    active_work: bool,
    awaiting_response: bool,
    cancel_requested: bool,
    session_id: Option<String>,
    restoring: Option<RestoreMode>,
    next_request_id: u64,
    messages: Vec<ChatEntry>,
    permissions: Vec<Permission>,
    connection: Option<Connection>,
}

struct ProjectView {
    path: PathBuf,
    branch: String,
    agents: Vec<AgentView>,
}

#[derive(Clone)]
struct AgentDrag {
    id: u64,
    label: String,
}

impl Render for AgentDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(SELECTED))
            .text_sm()
            .text_color(rgb(TEXT))
            .child(self.label.clone())
    }
}

#[derive(Clone)]
struct SidebarResize;

impl Render for SidebarResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(1.))
    }
}

struct Workspace {
    projects: Vec<ProjectView>,
    selected: Option<(usize, usize)>,
    selected_project: Option<usize>,
    sidebar_order: Vec<u64>,
    sidebar_fraction: f32,
    show_archived: bool,
    collapsed_tool_groups: HashSet<(u64, usize)>,
    expanded_tool_rows: HashSet<(u64, usize)>,
    next_agent_id: u64,
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
    folders_rx: Receiver<Vec<PathBuf>>,
    folders: Vec<PathBuf>,
    available_agents: Vec<AgentChoice>,
    picker: PickerMode,
    picker_selection: usize,
    slash_selection: usize,
    slash_dismissed: bool,
    picker_input: Entity<InputState>,
    sidebar_search: Entity<InputState>,
    composer: Entity<InputState>,
    chat_scroll: ScrollHandle,
    dirty: bool,
    last_saved: Instant,
    notice: Option<String>,
    _subscriptions: Vec<Subscription>,
}

fn branch(path: &Path) -> String {
    for args in [
        vec!["symbolic-ref", "--quiet", "--short", "HEAD"],
        vec!["rev-parse", "--short", "HEAD"],
    ] {
        if let Ok(output) = Command::new("git").arg("-C").arg(path).args(args).output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if !text.is_empty() {
                return text;
            }
        }
    }
    "no git branch".into()
}

fn agent_name(command: &[String]) -> String {
    command
        .first()
        .and_then(|name| Path::new(name).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Agent".into())
}

fn selected_model(config_options: &Value) -> Option<String> {
    let option = config_options.as_array()?.iter().find(|option| {
        option["category"].as_str() == Some("model") || option["id"].as_str() == Some("model")
    })?;
    let current = option["currentValue"].as_str()?;
    let options = option["options"].as_array()?;
    let choice = options
        .iter()
        .find(|choice| choice["value"].as_str() == Some(current))
        .or_else(|| {
            options
                .iter()
                .flat_map(|group| group["options"].as_array().into_iter().flatten())
                .find(|choice| choice["value"].as_str() == Some(current))
        });
    let Some(choice) = choice else {
        return Some(current.to_owned());
    };
    if current == "default"
        && let Some(description) = choice["description"].as_str()
    {
        return Some(
            description
                .split(" · ")
                .next()
                .unwrap_or(description)
                .to_owned(),
        );
    }
    choice["name"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| Some(current.to_owned()))
}

fn compact_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.0}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn submitted_prompt(value: &str) -> String {
    value.strip_suffix('\n').unwrap_or(value).to_owned()
}

fn parse_available_commands(update: &Value) -> Vec<SlashCommand> {
    update["availableCommands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|command| {
            let name = command["name"].as_str()?.trim().trim_start_matches('/');
            if name.is_empty() || name.chars().any(char::is_whitespace) {
                return None;
            }
            Some(SlashCommand {
                name: name.to_owned(),
                description: command["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                input: command.get("input").cloned(),
            })
        })
        .collect()
}

fn slash_query(draft: &str) -> Option<&str> {
    let query = draft.strip_prefix('/')?;
    if query.chars().any(char::is_whitespace) {
        None
    } else {
        Some(query)
    }
}

fn matching_slash_commands(commands: &[SlashCommand], draft: &str) -> Vec<SlashCommand> {
    let Some(query) = slash_query(draft) else {
        return Vec::new();
    };
    let query = query.to_lowercase();
    commands
        .iter()
        .filter(|command| command.name.to_lowercase().starts_with(&query))
        .take(8)
        .cloned()
        .collect()
}

fn completed_slash_text(command: &SlashCommand) -> String {
    let suffix = if command.input_hint().is_some() {
        " "
    } else {
        ""
    };
    format!("/{}{suffix}", command.name)
}

impl AgentView {
    fn new(config: AgentConfig) -> Self {
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
            connection: None,
        }
    }

    fn snapshot(&self) -> AgentConfig {
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
            was_working: self.active_work,
            session_has_activity: self.config.session_has_activity || self.active_work,
        }
    }

    fn has_restorable_activity(&self) -> bool {
        self.config.session_has_activity || self.config.was_working
    }

    fn send(&mut self, value: Value) -> Result<(), String> {
        self.connection
            .as_ref()
            .ok_or("Agent is not connected".into())
            .and_then(|connection| connection.send(value))
    }

    fn start_prompt(&mut self, prompt: String) -> Result<(), String> {
        let session_id = self.session_id.clone().ok_or("Agent is still connecting")?;
        let id = self.next_request_id;
        self.send(
            json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{
                "sessionId":session_id,"prompt":[{"type":"text","text":prompt}]
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

    fn start_next_queued_prompt(&mut self) -> bool {
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

    fn log(&mut self, role: Role, text: impl Into<String>) {
        if role != Role::System {
            self.config.session_has_activity = true;
        }
        self.messages.push(ChatEntry {
            role,
            key: None,
            text: text.into(),
        });
    }

    fn upsert_message(&mut self, role: Role, id: &str, content: Option<&Value>, append: bool) {
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

    fn upsert_tool_call(&mut self, update: &Value) {
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

    fn handle_prompt_response(&mut self, result: &Value) {
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

fn content_text(content: &Value) -> String {
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

impl Workspace {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let picker_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search folders by name or path…"));
        let sidebar_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search sessions…"));
        let composer = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 6)
                .placeholder("Ask your agent…")
        });
        let _subscriptions = vec![
            cx.subscribe_in(
                &sidebar_search,
                window,
                |_, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &picker_input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.picker_selection = 0;
                        cx.notify();
                    }
                    InputEvent::PressEnter { secondary: false } => {
                        this.confirm_picker(window, cx);
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &composer,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.slash_selection = 0;
                        this.slash_dismissed = false;
                        cx.notify();
                    }
                    InputEvent::PressEnter { secondary: false } => this.send_prompt(window, cx),
                    _ => {}
                },
            ),
        ];
        let (events_tx, events_rx) = mpsc::channel();
        let (config, migrate_config, notice) = match config::load() {
            Ok((config, migrate)) => (config, migrate, None),
            Err(error) => (
                Config::default(),
                false,
                Some(format!("Could not load config: {error}")),
            ),
        };
        let next_agent_id = config
            .projects
            .iter()
            .flat_map(|project| &project.agents)
            .map(|agent| agent.id)
            .max()
            .unwrap_or(0)
            + 1;
        let mut sidebar_order = config.sidebar_order;
        let sidebar_fraction = if config.sidebar_fraction.is_finite() {
            config.sidebar_fraction.clamp(0.1, 0.7)
        } else {
            0.2
        };
        let projects: Vec<ProjectView> = config
            .projects
            .into_iter()
            .map(|project| ProjectView {
                branch: branch(&project.path),
                path: project.path,
                agents: project.agents.into_iter().map(AgentView::new).collect(),
            })
            .collect();
        let agent_ids: Vec<u64> = projects
            .iter()
            .flat_map(|project| project.agents.iter().map(|agent| agent.config.id))
            .collect();
        let mut seen = std::collections::HashSet::new();
        sidebar_order.retain(|id| agent_ids.contains(id) && seen.insert(*id));
        for id in agent_ids {
            if seen.insert(id) {
                sidebar_order.push(id);
            }
        }
        let selected_project = (!projects.is_empty()).then_some(0);
        let recent_folders = projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        let (folders_tx, folders_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = folders_tx.send(discover_folders());
        });
        let mut this = Self {
            projects,
            selected: None,
            selected_project,
            sidebar_order,
            sidebar_fraction,
            show_archived: false,
            collapsed_tool_groups: HashSet::new(),
            expanded_tool_rows: HashSet::new(),
            next_agent_id,
            events_tx,
            events_rx,
            folders_rx,
            folders: recent_folders,
            available_agents: installed_agents(),
            picker: PickerMode::Closed,
            picker_selection: 0,
            slash_selection: 0,
            slash_dismissed: false,
            picker_input,
            sidebar_search,
            composer,
            chat_scroll: ScrollHandle::new(),
            dirty: migrate_config,
            last_saved: Instant::now(),
            notice,
            _subscriptions,
        };
        for project_index in 0..this.projects.len() {
            for agent_index in 0..this.projects[project_index].agents.len() {
                if this.projects[project_index].agents[agent_index]
                    .config
                    .archived
                {
                    continue;
                }
                this.connect(project_index, agent_index);
                this.selected.get_or_insert((project_index, agent_index));
                this.selected_project.get_or_insert(project_index);
            }
        }
        if let Some(first_id) = this.sidebar_order.iter().find(|id| {
            this.projects.iter().any(|project| {
                project
                    .agents
                    .iter()
                    .any(|agent| agent.config.id == **id && !agent.config.archived)
            })
        }) {
            for (project_index, project) in this.projects.iter().enumerate() {
                if let Some(agent_index) = project
                    .agents
                    .iter()
                    .position(|agent| agent.config.id == *first_id)
                {
                    this.selected = Some((project_index, agent_index));
                    this.selected_project = Some(project_index);
                    break;
                }
            }
        }
        if this.selected.is_none() {
            this.picker = if this
                .projects
                .iter()
                .any(|project| project.agents.iter().any(|agent| agent.config.archived))
            {
                PickerMode::Closed
            } else if this.selected_project.is_some() {
                PickerMode::Agents
            } else {
                PickerMode::Folders
            };
            this.picker_input
                .update(cx, |input, cx| input.focus(window, cx));
        } else {
            this.composer
                .update(cx, |input, cx| input.focus(window, cx));
        }
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                Timer::after(Duration::from_millis(100)).await;
                if this
                    .update(cx, |this, cx| {
                        this.poll_events(cx);
                        if let Ok(mut folders) = this.folders_rx.try_recv() {
                            folders
                                .extend(this.projects.iter().map(|project| project.path.clone()));
                            folders.sort();
                            folders.dedup();
                            this.folders = folders;
                            cx.notify();
                        }
                        ticks += 1;
                        if ticks >= 50 {
                            ticks = 0;
                            this.refresh_branches(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        this
    }

    fn config(&self) -> Config {
        Config {
            projects: self
                .projects
                .iter()
                .map(|project| ProjectConfig {
                    path: project.path.clone(),
                    agents: project.agents.iter().map(AgentView::snapshot).collect(),
                })
                .collect(),
            sidebar_order: self.sidebar_order.clone(),
            sidebar_fraction: self.sidebar_fraction,
        }
    }

    fn persist(&mut self) {
        if let Err(error) = config::save(&self.config()) {
            self.notice = Some(format!("Could not save config: {error}"));
            self.dirty = true;
        } else {
            self.dirty = false;
            self.last_saved = Instant::now();
        }
    }

    fn open_picker(&mut self, mode: PickerMode, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = mode;
        self.picker_selection = 0;
        self.picker_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_placeholder(
                match mode {
                    PickerMode::Agents => "Search installed agents or enter an ACP command…",
                    _ => "Search folders by name or path…",
                },
                window,
                cx,
            );
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn folder_results(&self, cx: &Context<Self>) -> Vec<PathBuf> {
        let query = self.picker_input.read(cx).value().to_string();
        let mut matches = folder_matches(&self.folders, query.trim(), 14);
        if let Ok(path) = PathBuf::from(query.trim()).canonicalize()
            && path.is_dir()
            && !matches.contains(&path)
        {
            matches.insert(0, path);
            matches.truncate(14);
        }
        matches
    }

    fn agent_results(&self, cx: &Context<Self>) -> Vec<AgentChoice> {
        let query = self.picker_input.read(cx).value().to_string();
        let mut agents = self
            .available_agents
            .iter()
            .filter_map(|agent| {
                let command = agent.command.join(" ");
                let rank = score(query.trim(), &agent.name)
                    .map(|rank| rank + 30)
                    .or_else(|| score(query.trim(), &command))?;
                Some((rank, agent.clone()))
            })
            .collect::<Vec<_>>();
        agents.sort_by(|(a_rank, a), (b_rank, b)| {
            b_rank.cmp(a_rank).then_with(|| a.name.cmp(&b.name))
        });
        agents.into_iter().map(|(_, agent)| agent).collect()
    }

    fn select_folder(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = match path.canonicalize() {
            Ok(path) if path.is_dir() => path,
            _ => {
                self.notice = Some("Choose an existing folder".into());
                cx.notify();
                return;
            }
        };
        let project_index = if let Some(index) = self
            .projects
            .iter()
            .position(|project| project.path == path)
        {
            index
        } else {
            self.projects.push(ProjectView {
                branch: branch(&path),
                path: path.clone(),
                agents: Vec::new(),
            });
            self.folders.push(path);
            self.persist();
            self.projects.len() - 1
        };
        self.selected_project = Some(project_index);
        self.selected = None;
        self.notice = None;
        self.open_picker(PickerMode::Agents, window, cx);
    }

    fn start_agent(
        &mut self,
        command: Vec<String>,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_index) = self.selected_project else {
            self.notice = Some("Choose a project first".into());
            cx.notify();
            return;
        };
        let config = AgentConfig {
            id: self.next_agent_id,
            command,
            archived: false,
            display_name: name,
            session_id: None,
            model: None,
            context: None,
            messages: Vec::new(),
            available_commands: Vec::new(),
            pending_prompts: Vec::new(),
            was_working: false,
            session_has_activity: false,
        };
        self.sidebar_order.push(config.id);
        self.next_agent_id += 1;
        let agent_index = self.projects[project_index].agents.len();
        self.projects[project_index]
            .agents
            .push(AgentView::new(config));
        self.selected = Some((project_index, agent_index));
        self.connect(project_index, agent_index);
        self.picker = PickerMode::Closed;
        self.notice = None;
        self.persist();
        self.composer
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn move_agent(&mut self, dragged: u64, target: u64, cx: &mut Context<Self>) {
        if move_sidebar_id(&mut self.sidebar_order, dragged, target) {
            self.persist();
            cx.notify();
        }
    }

    fn set_archived(
        &mut self,
        project_index: usize,
        agent_index: usize,
        archived: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let agent = &mut self.projects[project_index].agents[agent_index];
        if agent.config.archived == archived {
            return;
        }
        agent.config.archived = archived;
        if archived {
            if self.selected == Some((project_index, agent_index)) {
                self.selected = self.sidebar_order.iter().find_map(|id| {
                    self.projects.iter().enumerate().find_map(|(pi, project)| {
                        project.agents.iter().enumerate().find_map(|(ai, agent)| {
                            (agent.config.id == *id && !agent.config.archived).then_some((pi, ai))
                        })
                    })
                });
                self.selected_project = self.selected.map(|(pi, _)| pi);
            }
        } else {
            if self.projects[project_index].agents[agent_index]
                .connection
                .is_none()
            {
                self.connect(project_index, agent_index);
            }
            self.show_archived = false;
            self.selected = Some((project_index, agent_index));
            self.selected_project = Some(project_index);
            self.composer
                .update(cx, |input, cx| input.focus(window, cx));
        }
        self.persist();
        cx.notify();
    }

    fn confirm_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.picker {
            PickerMode::Folders => {
                if let Some(path) = self.folder_results(cx).get(self.picker_selection).cloned() {
                    self.select_folder(path, window, cx);
                } else {
                    self.notice =
                        Some("No matching folder. Type an existing absolute path.".into());
                    cx.notify();
                }
            }
            PickerMode::Agents => {
                if let Some(agent) = self.agent_results(cx).get(self.picker_selection).cloned() {
                    self.start_agent(agent.command, Some(agent.name), window, cx);
                } else {
                    self.start_custom_agent(window, cx);
                }
            }
            PickerMode::Closed => {}
        }
    }

    fn start_custom_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.picker_input.read(cx).value().to_string();
        match shell_words::split(value.trim()) {
            Ok(command) if !command.is_empty() => self.start_agent(command, None, window, cx),
            _ => {
                self.notice = Some("Enter an ACP executable and its arguments".into());
                cx.notify();
            }
        }
    }

    fn quick_open(&mut self, _: &QuickOpen, window: &mut Window, cx: &mut Context<Self>) {
        self.open_picker(PickerMode::Folders, window, cx);
    }

    fn picker_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picker == PickerMode::Closed {
            return;
        }
        let count = match self.picker {
            PickerMode::Folders => self.folder_results(cx).len(),
            PickerMode::Agents => {
                self.agent_results(cx).len()
                    + usize::from(!self.picker_input.read(cx).value().trim().is_empty())
            }
            PickerMode::Closed => 0,
        };
        match event.keystroke.key.as_str() {
            "down" if count > 0 => {
                self.picker_selection = (self.picker_selection + 1) % count;
                cx.stop_propagation();
                cx.notify();
            }
            "up" if count > 0 => {
                self.picker_selection = (self.picker_selection + count - 1) % count;
                cx.stop_propagation();
                cx.notify();
            }
            "escape" => {
                self.picker = PickerMode::Closed;
                if self.selected.is_none() {
                    self.picker = if self.selected_project.is_some() {
                        PickerMode::Agents
                    } else {
                        PickerMode::Folders
                    };
                }
                if self.picker != PickerMode::Closed {
                    self.picker_input
                        .update(cx, |input, cx| input.focus(window, cx));
                }
                cx.stop_propagation();
                cx.notify();
            }
            _ => {}
        }
    }

    fn slash_results(&self, cx: &Context<Self>) -> Vec<SlashCommand> {
        if self.slash_dismissed || self.picker != PickerMode::Closed {
            return Vec::new();
        }
        let Some((project_index, agent_index)) = self.selected else {
            return Vec::new();
        };
        let draft = self.composer.read(cx).value().to_string();
        matching_slash_commands(
            &self.projects[project_index].agents[agent_index]
                .config
                .available_commands,
            &draft,
        )
    }

    fn complete_slash_command(
        &mut self,
        command: SlashCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = completed_slash_text(&command);
        let column = value.encode_utf16().count() as u32;
        self.composer.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.set_cursor_position(Position::new(0, column), window, cx);
        });
        self.slash_selection = 0;
        self.slash_dismissed = true;
        cx.notify();
    }

    fn handle_slash_action(
        &mut self,
        action: SlashAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.composer.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let commands = self.slash_results(cx);
        if commands.is_empty() {
            return;
        }
        match action {
            SlashAction::Down => {
                self.slash_selection = (self.slash_selection + 1) % commands.len();
            }
            SlashAction::Up => {
                self.slash_selection = (self.slash_selection + commands.len() - 1) % commands.len();
            }
            SlashAction::Complete => {
                let index = self.slash_selection.min(commands.len() - 1);
                self.complete_slash_command(commands[index].clone(), window, cx);
            }
            SlashAction::Dismiss => {
                self.slash_dismissed = true;
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn connect(&mut self, project_index: usize, agent_index: usize) {
        let project = &mut self.projects[project_index];
        let agent = &mut project.agents[agent_index];
        match Connection::spawn(
            agent.config.id,
            &agent.config.command,
            &project.path,
            self.events_tx.clone(),
        ) {
            Ok(connection) => {
                agent.connection = Some(connection);
                let initialize = v2::InitializeRequest::new(
                    ProtocolVersion::V2,
                    v2::Implementation::new("agentaps", env!("CARGO_PKG_VERSION"))
                        .title("Agentaps"),
                );
                if let Err(error) = agent
                    .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":initialize}))
                {
                    agent.status = Status::Error;
                    agent.log(Role::System, error);
                }
            }
            Err(error) => {
                agent.status = Status::Error;
                agent.log(Role::System, error);
            }
        }
    }

    fn send_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((project_index, agent_index)) = self.selected else {
            return;
        };
        let value = self.composer.read(cx).value().to_string();
        let prompt = submitted_prompt(&value);
        if prompt.trim().is_empty() {
            self.composer
                .update(cx, |input, cx| input.set_value("", window, cx));
            return;
        }
        let agent = &mut self.projects[project_index].agents[agent_index];
        let Some(_) = agent.session_id.as_ref() else {
            self.notice = Some("Agent is still connecting".into());
            cx.notify();
            return;
        };
        if agent.active_work || !agent.config.pending_prompts.is_empty() {
            agent.config.pending_prompts.push(prompt);
            self.dirty = true;
            self.composer
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.notice = None;
            cx.notify();
            return;
        }
        match agent.start_prompt(prompt) {
            Ok(()) => {
                self.chat_scroll.scroll_to_bottom();
                self.dirty = true;
                self.composer
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.notice = None;
            }
            Err(error) => {
                agent.status = Status::Error;
                agent.log(Role::System, error);
            }
        }
        cx.notify();
    }

    fn cancel_prompt(&mut self, cx: &mut Context<Self>) {
        if let Some((project_index, agent_index)) = self.selected {
            let agent = &mut self.projects[project_index].agents[agent_index];
            if agent.active_work
                && !agent.cancel_requested
                && let Some(session_id) = &agent.session_id
            {
                match agent.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session_id}})) {
                    Ok(()) => agent.cancel_requested = true,
                    Err(error) => agent.log(Role::System, format!("Could not stop agent: {error}")),
                }
            }
            for permission in agent.permissions.drain(..) {
                let _ = agent.connection.as_ref().map(|connection| connection.send(json!({"jsonrpc":"2.0","id":permission.request_id,"result":{"outcome":{"outcome":"cancelled"}}})));
            }
            agent.awaiting_response = false;
            cx.notify();
        }
    }

    fn choose_permission(
        &mut self,
        permission_index: usize,
        option_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some((project_index, agent_index)) = self.selected else {
            return;
        };
        let agent = &mut self.projects[project_index].agents[agent_index];
        if permission_index >= agent.permissions.len() {
            return;
        }
        let permission = agent.permissions.remove(permission_index);
        if let Err(error) = agent.send(json!({"jsonrpc":"2.0","id":permission.request_id,"result":{
            "outcome":{"outcome":"selected","optionId":option_id}
        }})) {
            agent.log(Role::System, error);
            agent.status = Status::Error;
        }
        cx.notify();
    }

    fn poll_events(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        while let Ok(event) = self.events_rx.try_recv() {
            changed = true;
            match event {
                Event::Message { agent_id, value } => self.handle_message(agent_id, value),
                Event::Disconnected { agent_id, reason } => {
                    if let Some((agent, _)) = self.agent_mut(agent_id) {
                        agent.awaiting_response = false;
                        agent.cancel_requested = false;
                        if agent.status != Status::Error {
                            agent.status = Status::Error;
                            agent.log(Role::System, reason);
                        }
                    }
                }
            }
        }
        for project in &mut self.projects {
            for agent in &mut project.agents {
                changed |= agent.start_next_queued_prompt();
            }
        }
        if changed {
            self.dirty = true;
            cx.notify();
        }
        if self.dirty && self.last_saved.elapsed() >= Duration::from_secs(1) {
            self.persist();
        }
    }

    fn start_new_session(agent: &mut AgentView, path: &Path) {
        agent.restoring = None;
        agent.session_id = None;
        agent.config.session_id = None;
        agent.config.session_has_activity = false;
        if let Err(error) = agent.send(
            json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{
                "cwd":path,"mcpServers":[]
            }}),
        ) {
            agent.status = Status::Error;
            agent.log(Role::System, error);
        }
    }

    fn resume_session(agent: &mut AgentView, path: &Path, mode: RestoreMode, session_id: String) {
        agent.session_id = Some(session_id.clone());
        agent.restoring = Some(mode);
        let method = match mode {
            RestoreMode::Resume => "session/resume",
            RestoreMode::Load => "session/load",
        };
        if let Err(error) = agent.send(json!({"jsonrpc":"2.0","id":2,"method":method,"params":{
            "sessionId":session_id,"cwd":path,"mcpServers":[]
        }})) {
            agent.status = Status::Error;
            agent.log(Role::System, error);
        }
    }

    fn agent_mut(&mut self, agent_id: u64) -> Option<(&mut AgentView, PathBuf)> {
        self.projects.iter_mut().find_map(|project| {
            let path = project.path.clone();
            project
                .agents
                .iter_mut()
                .find(|agent| agent.config.id == agent_id)
                .map(|agent| (agent, path))
        })
    }

    fn handle_message(&mut self, agent_id: u64, value: Value) {
        let Some((agent, path)) = self.agent_mut(agent_id) else {
            return;
        };
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            match method {
                "session/update" => Self::handle_update(agent, &value),
                "session/request_permission" => {
                    let Some(request_id) = value.get("id").cloned() else {
                        return;
                    };
                    let params = &value["params"];
                    if let Some(session_id) = &agent.session_id
                        && params["sessionId"].as_str() != Some(session_id)
                    {
                        return;
                    }
                    let title = params["title"]
                        .as_str()
                        .or_else(|| params["toolCall"]["title"].as_str())
                        .unwrap_or("Agent requests permission")
                        .to_owned();
                    let description = params["description"]
                        .as_str()
                        .or_else(|| params["subject"]["command"].as_str())
                        .map(str::to_owned);
                    let options: Vec<(String, String)> = params["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|option| {
                            Some((
                                option["optionId"].as_str()?.to_owned(),
                                option["name"].as_str()?.to_owned(),
                            ))
                        })
                        .collect();
                    if options.is_empty() {
                        let _ = agent.send(json!({"jsonrpc":"2.0","id":request_id,"result":{"outcome":{"outcome":"cancelled"}}}));
                    } else {
                        agent.permissions.push(Permission {
                            request_id,
                            title,
                            description,
                            options,
                        });
                    }
                }
                _ => {
                    if let Some(id) = value.get("id") {
                        let _ = agent.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not supported by this client"}}));
                    }
                }
            }
            return;
        }
        let Some(id) = value.get("id").and_then(Value::as_u64) else {
            return;
        };
        if let Some(error) = value.get("error") {
            let message = error["message"].as_str().unwrap_or("unknown error");
            if id == 2 && agent.restoring.take().is_some() {
                agent.log(
                    Role::System,
                    format!(
                        "Could not restore the previous session: {message}. Starting a new session."
                    ),
                );
                Self::start_new_session(agent, &path);
                return;
            }
            if id >= 3 && agent.protocol == Some(ProtocolVersion::V2) {
                agent.active_work = false;
                agent.awaiting_response = false;
                agent.cancel_requested = false;
                agent.status = Status::Idle;
            } else {
                agent.status = Status::Error;
            }
            let details = error["data"]["details"].as_str();
            agent.log(
                Role::System,
                match details {
                    Some(details) if !details.is_empty() => {
                        format!("ACP error: {message}\n{details}")
                    }
                    _ => format!("ACP error: {message}"),
                },
            );
            return;
        }
        match id {
            1 => {
                let protocol = match value["result"]["protocolVersion"].as_u64() {
                    Some(2) => {
                        match serde_json::from_value::<v2::InitializeResponse>(
                            value["result"].clone(),
                        ) {
                            Ok(response) if response.capabilities.session.is_some() => {
                                Some(ProtocolVersion::V2)
                            }
                            Ok(_) => {
                                agent.log(
                                    Role::System,
                                    "ACP v2 agent did not advertise session support",
                                );
                                None
                            }
                            Err(error) => {
                                agent.log(
                                    Role::System,
                                    format!("Invalid ACP v2 initialization: {error}"),
                                );
                                None
                            }
                        }
                    }
                    Some(1) => Some(ProtocolVersion::V1),
                    _ => {
                        agent.log(Role::System, "Agent does not support ACP v1 or v2");
                        None
                    }
                };
                if let Some(protocol) = protocol {
                    agent.protocol = Some(protocol);
                    if let Some(previous_session) = agent
                        .config
                        .session_id
                        .clone()
                        .filter(|_| agent.has_restorable_activity())
                    {
                        if let Some(mode) = restore_mode(protocol, &value["result"]) {
                            Self::resume_session(agent, &path, mode, previous_session);
                        } else {
                            agent.log(
                                Role::System,
                                "This agent cannot restore sessions. Starting a new session with the saved activity visible.",
                            );
                            Self::start_new_session(agent, &path);
                        }
                    } else {
                        Self::start_new_session(agent, &path);
                    }
                } else {
                    agent.status = Status::Error;
                }
            }
            2 => {
                if agent.restoring.take().is_some() {
                    agent.model = selected_model(&value["result"]["configOptions"])
                        .or_else(|| agent.model.clone());
                    if agent.status == Status::Connecting {
                        agent.status = Status::Idle;
                    }
                } else if let Some(session_id) = value["result"]["sessionId"].as_str() {
                    agent.session_id = Some(session_id.to_owned());
                    agent.config.session_id = agent.session_id.clone();
                    agent.model = selected_model(&value["result"]["configOptions"])
                        .or_else(|| agent.model.clone());
                    agent.status = Status::Idle;
                } else {
                    agent.status = Status::Error;
                    agent.log(Role::System, "Agent returned no session ID");
                }
            }
            _ => {
                agent.handle_prompt_response(&value["result"]);
            }
        }
    }

    fn handle_update(agent: &mut AgentView, value: &Value) {
        if let Some(session_id) = &agent.session_id
            && value["params"]["sessionId"].as_str() != Some(session_id)
        {
            return;
        }
        let update = &value["params"]["update"];
        if agent.protocol == Some(ProtocolVersion::V2)
            && let Err(error) =
                serde_json::from_value::<v2::UpdateSessionNotification>(value["params"].clone())
        {
            agent.log(Role::System, format!("Invalid ACP v2 update: {error}"));
            return;
        }
        match update["sessionUpdate"].as_str() {
            Some("usage_update") => {
                if let (Some(used), Some(size)) = (update["used"].as_u64(), update["size"].as_u64())
                    && size > 0
                {
                    agent.context = Some((used, size));
                }
            }
            Some("config_option_update") => {
                agent.model = selected_model(&update["configOptions"]);
            }
            Some("available_commands_update") => {
                agent.config.available_commands = parse_available_commands(update);
            }
            _ => {}
        }
        if agent.restoring == Some(RestoreMode::Load) {
            return;
        }
        if agent.protocol == Some(ProtocolVersion::V2) {
            match update["sessionUpdate"].as_str() {
                Some("state_update") => match update["state"].as_str() {
                    Some("running" | "requires_action") => {
                        agent.status = Status::Working;
                        agent.active_work = true;
                    }
                    Some("idle") => {
                        agent.status = if agent.active_work {
                            Status::Done
                        } else {
                            Status::Idle
                        };
                        agent.active_work = false;
                        agent.awaiting_response = false;
                        agent.cancel_requested = false;
                        if let Some(reason) = update["stopReason"].as_str()
                            && reason != "end_turn"
                        {
                            agent.log(Role::System, format!("Stopped: {reason}"));
                        }
                    }
                    _ => {}
                },
                Some("user_message" | "agent_message" | "agent_thought") => {
                    if update["sessionUpdate"].as_str() != Some("user_message") {
                        agent.awaiting_response = false;
                    }
                    if let Some(id) = update["messageId"].as_str() {
                        let role = match update["sessionUpdate"].as_str() {
                            Some("user_message") => Role::User,
                            Some("agent_thought") => Role::Thought,
                            _ => Role::Agent,
                        };
                        agent.upsert_message(role, id, update.get("content"), false);
                    }
                }
                Some("user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk") => {
                    if update["sessionUpdate"].as_str() != Some("user_message_chunk") {
                        agent.awaiting_response = false;
                    }
                    if let Some(id) = update["messageId"].as_str() {
                        let role = match update["sessionUpdate"].as_str() {
                            Some("user_message_chunk") => Role::User,
                            Some("agent_thought_chunk") => Role::Thought,
                            _ => Role::Agent,
                        };
                        agent.upsert_message(role, id, update.get("content"), true);
                    }
                }
                Some("tool_call_update") => {
                    agent.awaiting_response = false;
                    agent.upsert_tool_call(update);
                }
                Some("tool_call_content_chunk") => {
                    agent.awaiting_response = false;
                    if let Some(id) = update["toolCallId"].as_str() {
                        let key = format!("tool:{id}");
                        let content = content_text(&update["content"]["content"]);
                        if let Some(entry) = agent
                            .messages
                            .iter_mut()
                            .find(|entry| entry.key.as_deref() == Some(&key))
                            && !content.is_empty()
                        {
                            entry.text.push('\n');
                            entry.text.push_str(&content);
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => {
                agent.awaiting_response = false;
                if let Some(text) = update["content"]["text"].as_str() {
                    if let Some(last) = agent.messages.last_mut()
                        && matches!(last.role, Role::Agent)
                    {
                        last.text.push_str(text);
                        return;
                    }
                    agent.log(Role::Agent, text);
                }
            }
            Some("tool_call" | "tool_call_update") => {
                agent.awaiting_response = false;
                agent.upsert_tool_call(update);
            }
            _ => {}
        }
    }

    fn refresh_branches(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for project in &mut self.projects {
            let fresh = branch(&project.path);
            if fresh != project.branch {
                project.branch = fresh;
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }
}

fn status_badge(agent_id: u64, status: Status) -> impl IntoElement {
    div()
        .id(("status", agent_id))
        .flex()
        .flex_shrink_0()
        .size(px(12.))
        .items_center()
        .justify_center()
        .child(div().size(px(7.)).rounded_full().bg(rgb(status.color())))
        .tooltip(move |window, cx| Tooltip::new(status.label()).build(window, cx))
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.chat_scroll.offset().y + self.chat_scroll.max_offset().height <= px(24.) {
            self.chat_scroll.scroll_to_bottom();
        }
        let session_query = self.sidebar_search.read(cx).value().trim().to_owned();
        let mut visible_sessions = 0;
        let mut project_list = div().flex().flex_col().gap_1();
        let mut archived_list = div().flex().flex_col().gap_1();
        let archived_count = self
            .projects
            .iter()
            .flat_map(|project| &project.agents)
            .filter(|agent| agent.config.archived)
            .count();
        let mut rows = self
            .projects
            .iter()
            .enumerate()
            .flat_map(|(project_index, project)| {
                project
                    .agents
                    .iter()
                    .enumerate()
                    .map(move |(agent_index, agent)| (project_index, agent_index, project, agent))
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|(_, _, _, agent)| {
            self.sidebar_order
                .iter()
                .position(|id| *id == agent.config.id)
                .unwrap_or(usize::MAX)
        });
        for (project_index, agent_index, project, agent) in rows {
            let archived = agent.config.archived;
            if archived != self.show_archived {
                continue;
            }
            let name = project
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| project.path.display().to_string());
            if !session_query.is_empty()
                && [
                    name.as_str(),
                    project.path.to_str().unwrap_or_default(),
                    project.branch.as_str(),
                    agent.name.as_str(),
                ]
                .iter()
                .all(|value| score(&session_query, value).is_none())
            {
                continue;
            }
            visible_sessions += 1;
            let selected = self.selected == Some((project_index, agent_index));
            let agent_id = agent.config.id;
            let row_group = format!("agent-row-{agent_id}");
            let row = div()
                .id(("agent", agent_id))
                .group(row_group.clone())
                .relative()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(!archived, |element| element.cursor_move())
                .bg(rgb(if selected { SELECTED } else { SIDEBAR }))
                .hover(|style| style.bg(rgb(HOVER)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if archived {
                        this.set_archived(project_index, agent_index, false, window, cx);
                        return;
                    }
                    this.selected = Some((project_index, agent_index));
                    this.selected_project = Some(project_index);
                    this.picker = PickerMode::Closed;
                    this.composer
                        .update(cx, |input, cx| input.focus(window, cx));
                    cx.notify();
                }))
                .when(!archived, |element| {
                    element
                        .on_drag(
                            AgentDrag {
                                id: agent_id,
                                label: name.clone(),
                            },
                            |drag: &AgentDrag, _, _, cx| cx.new(|_| drag.clone()),
                        )
                        .drag_over::<AgentDrag>(|style, _, _, _| style.bg(rgb(DROP_TARGET)))
                        .on_drop(cx.listener(move |this, drag: &AgentDrag, _, cx| {
                            this.move_agent(drag.id, agent_id, cx)
                        }))
                })
                .child(status_badge(agent_id, agent.status))
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(relative(0.65))
                        .truncate()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(if selected { TEXT } else { MUTED }))
                        .child(name),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(project.branch.clone()),
                )
                .child(
                    div()
                        .id(("archive-action", agent_id))
                        .absolute()
                        .right(px(4.))
                        .top(px(2.))
                        .invisible()
                        .group_hover(row_group, |style| style.visible())
                        .rounded_sm()
                        .bg(rgb(HOVER))
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(TEXT))
                        .cursor_pointer()
                        .child(
                            Icon::new(if archived {
                                IconName::Undo2
                            } else {
                                IconName::Inbox
                            })
                            .size(px(14.))
                            .text_color(rgb(TEXT)),
                        )
                        .tooltip(move |window, cx| {
                            Tooltip::new(if archived {
                                "Restore session"
                            } else {
                                "Archive session"
                            })
                            .build(window, cx)
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.set_archived(project_index, agent_index, !archived, window, cx);
                        })),
                );
            if archived {
                archived_list = archived_list.child(row);
            } else {
                project_list = project_list.child(row);
            }
        }
        for (project_index, project) in self.projects.iter().enumerate() {
            if self.show_archived {
                break;
            }
            if project.agents.is_empty() {
                let name = project
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| project.path.display().to_string());
                if !session_query.is_empty()
                    && [
                        name.as_str(),
                        project.path.to_str().unwrap_or_default(),
                        project.branch.as_str(),
                    ]
                    .iter()
                    .all(|value| score(&session_query, value).is_none())
                {
                    continue;
                }
                visible_sessions += 1;
                project_list = project_list.child(
                    div()
                        .id(("project", project_index))
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(HOVER)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.selected_project = Some(project_index);
                            this.selected = None;
                            this.open_picker(PickerMode::Agents, window, cx);
                        }))
                        .child(
                            div()
                                .flex_shrink_0()
                                .max_w(relative(0.54))
                                .truncate()
                                .text_sm()
                                .text_color(rgb(TEXT))
                                .child(name),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .child(project.branch.clone()),
                        )
                        .child(div().text_xs().text_color(rgb(ACCENT)).child("add")),
                );
            }
        }
        if self.show_archived {
            project_list = archived_list;
        }
        if visible_sessions == 0 && !session_query.is_empty() {
            project_list = project_list.child(
                div()
                    .px_3()
                    .py_3()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("No matching sessions"),
            );
        } else if self.show_archived && archived_count == 0 {
            project_list = project_list.child(
                div()
                    .px_3()
                    .py_3()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("No archived sessions"),
            );
        }
        let viewport_width = f32::from(window.viewport_size().width);
        let sidebar_width = (viewport_width * self.sidebar_fraction).max(180.);
        let archive_tooltip = if self.show_archived {
            "Show active sessions"
        } else {
            "Show archived sessions"
        };
        let sidebar = div()
            .w(px(sidebar_width))
            .min_w(px(180.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(SIDEBAR))
            .child(div().p_2().child(Input::new(&self.sidebar_search)))
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_3()
                    .child(project_list),
            )
            .child(
                div()
                    .p_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id("archived-toggle")
                            .cursor_pointer()
                            .flex_1()
                            .min_w(px(0.))
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(if self.show_archived { ACCENT } else { SIDEBAR }))
                            .bg(rgb(if self.show_archived {
                                SELECTED
                            } else {
                                SIDEBAR
                            }))
                            .px_2()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(rgb(if self.show_archived { TEXT } else { MUTED }))
                            .hover(|style| style.bg(rgb(DROP_TARGET)).text_color(rgb(TEXT)))
                            .child(
                                Icon::new(IconName::Inbox)
                                    .size(px(14.))
                                    .text_color(rgb(if self.show_archived { TEXT } else { MUTED })),
                            )
                            .child("Archive")
                            .child(format!("{archived_count}"))
                            .tooltip(move |window, cx| {
                                Tooltip::new(archive_tooltip).build(window, cx)
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_archived = !this.show_archived;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("quick-open")
                            .cursor_pointer()
                            .flex_shrink_0()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(SIDEBAR))
                            .px_2()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .hover(|style| style.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                            .child(Icon::new(IconName::Plus).size(px(14.)))
                            .child("New")
                            .tooltip(|window, cx| Tooltip::new("Open folder").build(window, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_picker(PickerMode::Folders, window, cx)
                            })),
                    ),
            );

        let divider = div()
            .id("sidebar-divider")
            .w(px(6.))
            .h_full()
            .flex_shrink_0()
            .cursor_ew_resize()
            .bg(rgb(BORDER))
            .hover(|style| style.bg(rgb(ACCENT)))
            .on_drag(SidebarResize, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            });

        let mut chat = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(BG));
        if self.picker != PickerMode::Closed {
            let is_folders = self.picker == PickerMode::Folders;
            let query = self.picker_input.read(cx).value().to_string();
            let mut results = div().flex().flex_col().gap_1();
            if is_folders {
                let matches = self.folder_results(cx);
                if matches.is_empty() {
                    results = results.child(div().p_5().text_sm().text_color(rgb(MUTED)).child(
                        if self.folders.is_empty() && query.is_empty() {
                            "Scanning folders in your home directory…"
                        } else {
                            "No folders found. Enter an existing absolute path."
                        },
                    ));
                }
                for (index, path) in matches.into_iter().enumerate() {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let path_label = path.display().to_string();
                    results = results.child(
                        div()
                            .id(("folder-result", index))
                            .cursor_pointer()
                            .rounded_lg()
                            .px_4()
                            .py_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .bg(rgb(if index == self.picker_selection {
                                SELECTED
                            } else {
                                SURFACE
                            }))
                            .hover(|style| style.bg(rgb(HOVER)))
                            .child(
                                div()
                                    .size(px(34.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .bg(rgb(CHIP))
                                    .text_color(rgb(ACCENT))
                                    .child("▣"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_sm().text_color(rgb(TEXT)).child(name))
                                    .child(
                                        div().text_xs().text_color(rgb(MUTED)).child(path_label),
                                    ),
                            )
                            .child(div().text_color(rgb(MUTED)).child("↗"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_folder(path.clone(), window, cx)
                            })),
                    );
                }
            } else {
                let matches = self.agent_results(cx);
                if matches.is_empty() {
                    results =
                        results.child(
                            div().p_5().text_sm().text_color(rgb(MUTED)).child(
                                "No installed ACP agents match. Enter a custom command below.",
                            ),
                        );
                }
                for (index, agent) in matches.into_iter().enumerate() {
                    let command = agent.command.clone();
                    let name = agent.name.clone();
                    results = results.child(
                        div()
                            .id(("agent-result", index))
                            .cursor_pointer()
                            .rounded_lg()
                            .px_4()
                            .py_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .bg(rgb(if index == self.picker_selection {
                                SELECTED
                            } else {
                                SURFACE
                            }))
                            .hover(|style| style.bg(rgb(HOVER)))
                            .child(
                                div()
                                    .size(px(34.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .bg(rgb(AGENT_PICKER_CHIP))
                                    .text_color(rgb(AGENT_PICKER_TEXT))
                                    .child("✦"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_sm().text_color(rgb(TEXT)).child(agent.name))
                                    .child(
                                        div().text_xs().text_color(rgb(MUTED)).child(agent.detail),
                                    ),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child("↗"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_agent(command.clone(), Some(name.clone()), window, cx)
                            })),
                    );
                }
                if !query.trim().is_empty() {
                    let custom_selected = self.picker_selection == self.agent_results(cx).len();
                    results = results.child(
                        div()
                            .id("custom-agent")
                            .cursor_pointer()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(if custom_selected { ACCENT } else { BORDER }))
                            .px_4()
                            .py_3()
                            .text_sm()
                            .text_color(rgb(ACCENT))
                            .child(format!("Run custom ACP command: {}", query.trim()))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_custom_agent(window, cx)
                            })),
                    );
                }
            }
            let shortcut = if cfg!(target_os = "macos") {
                "⌘ P"
            } else {
                "Ctrl P"
            };
            chat = chat.child(
                div()
                    .id("picker-page")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .px_8()
                    .py_8()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(760.))
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(ACCENT))
                                    .child(if is_folders { "01  /  PROJECT" } else { "02  /  AGENT" }),
                            )
                            .child(
                                div()
                                    .text_3xl()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .child(if is_folders {
                                        "Find your next workspace."
                                    } else {
                                        "Choose your agent."
                                    }),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(MUTED))
                                    .child(if is_folders {
                                        "Search your folders, then pair a project with an ACP agent."
                                    } else {
                                        "Detected ACP agents are ready to launch. You can also enter any ACP command."
                                    }),
                            )
                            .child(
                                div()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(rgb(DROP_TARGET))
                                    .bg(rgb(SURFACE))
                                    .p_3()
                                    .child(Input::new(&self.picker_input)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .child(if is_folders { "FOLDERS" } else { "AVAILABLE AGENTS" }),
                                    )
                                    .child(
                                        div()
                                            .rounded_md()
                                            .bg(rgb(SURFACE))
                                            .px_2()
                                            .py_1()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .child(format!("Quick open  {shortcut}")),
                                    ),
                            )
                            .child(results)
                            .child(
                                div()
                                    .pt_3()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child("↑ ↓ to navigate    Enter to open    Esc to return"),
                            ),
                    ),
            );
        } else if let Some((project_index, agent_index)) = self.selected {
            let project = &self.projects[project_index];
            let agent = &project.agents[agent_index];
            let title = div()
                .flex()
                .flex_1()
                .min_w(px(0.))
                .items_center()
                .gap_3()
                .child(
                    div()
                        .flex_shrink_0()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(agent.name.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(project.path.display().to_string()),
                );
            let metadata = div()
                .flex()
                .min_w(px(0.))
                .max_w(relative(0.5))
                .items_center()
                .gap_3()
                .when_some(agent.model.as_ref(), |element, model| {
                    element.child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .child(model.clone()),
                    )
                })
                .when_some(agent.context, |element, (used, size)| {
                    element.child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "Context {} / {}",
                                compact_tokens(used),
                                compact_tokens(size)
                            )),
                    )
                });
            chat = chat.child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .gap_3()
                    .items_center()
                    .child(title)
                    .when(
                        agent.model.is_some() || agent.context.is_some(),
                        |element| element.child(metadata),
                    ),
            );
            let mut entries = div().w_full().min_w(px(0.)).flex().flex_col().gap_2().p_4();
            if agent.messages.is_empty() {
                entries = entries.child(div().mt_8().text_center().text_color(rgb(MUTED)).child(
                    if agent.status == Status::Connecting {
                        "Connecting to agent…"
                    } else {
                        "Ask the agent to work on this project."
                    },
                ));
            }
            let text_style = TextViewStyle {
                paragraph_gap: rems(0.25),
                highlight_theme: cx.theme().highlight_theme.clone(),
                is_dark: true,
                ..Default::default()
            };
            let mut message_index = 0;
            while message_index < agent.messages.len() {
                let entry = &agent.messages[message_index];
                if entry.role == Role::Tool {
                    let end = tool_run_end(&agent.messages, message_index);
                    let tool_entries = &agent.messages[message_index..end];
                    let group_key = (agent.config.id, message_index);
                    let expanded = !self.collapsed_tool_groups.contains(&group_key);
                    let heading = tool_group_heading(tool_entries);
                    let has_specific_actions = tool_entries.iter().any(|entry| {
                        !is_approval_review(&entry.text) && !is_generic_tool_title(&entry.text)
                    });
                    let action_count = tool_entries
                        .iter()
                        .filter(|entry| {
                            !is_approval_review(&entry.text)
                                && (!has_specific_actions || !is_generic_tool_title(&entry.text))
                        })
                        .count();
                    let group_id: gpui::ElementId = ("tool-group", agent.config.id).into();
                    let mut group = div().max_w(px(900.)).min_w(px(0.)).py_1().child(
                        div()
                            .id((group_id, message_index.to_string()))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .child(div().text_color(rgb(TOOL_MARKER)).child("•"))
                            .child(div().text_color(rgb(TEXT)).child(heading))
                            .child(div().text_xs().text_color(rgb(MUTED)).child(if expanded {
                                "⌄"
                            } else {
                                "›"
                            }))
                            .when(!expanded && action_count > 0, |row| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(format!("{action_count} actions")),
                                )
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.collapsed_tool_groups.insert(group_key) {
                                    this.collapsed_tool_groups.remove(&group_key);
                                }
                                cx.notify();
                            })),
                    );
                    if expanded {
                        let mut details = div().pl_4().flex().flex_col().gap_1();
                        let mut first = true;
                        for (offset, entry) in
                            tool_entries.iter().enumerate().filter(|(_, entry)| {
                                !is_approval_review(&entry.text)
                                    && (!has_specific_actions
                                        || !is_generic_tool_title(&entry.text))
                            })
                        {
                            let (description, _) = tool_description(&entry.text);
                            let (_, status) = tool_title_and_status(&entry.text);
                            let label = match status {
                                Some("in_progress") => format!("{description} · running"),
                                Some("failed") => format!("{description} · failed"),
                                _ => description,
                            };
                            let row_key = (agent.config.id, message_index + offset);
                            let row_expanded = self.expanded_tool_rows.contains(&row_key);
                            let row_id: gpui::ElementId = ("tool-row", agent.config.id).into();
                            let mut action = div().min_w(px(0.)).child(
                                div()
                                    .id((row_id, (message_index + offset).to_string()))
                                    .cursor_pointer()
                                    .min_w(px(0.))
                                    .flex()
                                    .items_start()
                                    .gap_2()
                                    .text_sm()
                                    .text_color(rgb(MUTED))
                                    .child(div().w(px(14.)).flex_shrink_0().child(if first {
                                        "└"
                                    } else {
                                        " "
                                    }))
                                    .child(div().min_w(px(0.)).truncate().child(label))
                                    .child(div().text_xs().child(if row_expanded {
                                        "⌄"
                                    } else {
                                        "›"
                                    }))
                                    .tooltip(move |window, cx| {
                                        Tooltip::new("Show command and output").build(window, cx)
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.expanded_tool_rows.insert(row_key) {
                                            this.expanded_tool_rows.remove(&row_key);
                                        }
                                        cx.notify();
                                    })),
                            );
                            if row_expanded {
                                let text_id: gpui::ElementId =
                                    ("tool-detail", agent.config.id).into();
                                action = action.child(
                                    div()
                                        .ml(px(22.))
                                        .mt_1()
                                        .p_2()
                                        .rounded_md()
                                        .bg(rgb(SURFACE))
                                        .child(
                                            TextView::markdown(
                                                (text_id, row_key.1.to_string()),
                                                markdown_code_block(&entry.text),
                                                window,
                                                cx,
                                            )
                                            .style(text_style.clone())
                                            .selectable(true)
                                            .text_xs()
                                            .text_color(rgb(TEXT)),
                                        ),
                                );
                            }
                            details = details.child(action);
                            first = false;
                        }
                        if let Some(summary) = approval_summary(tool_entries) {
                            details = details.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child(div().w(px(14.)).child(if first { "└" } else { " " }))
                                    .child(summary),
                            );
                        }
                        group = group.child(details);
                    }
                    entries = entries.child(group);
                    message_index = end;
                    continue;
                }
                if entry.text.is_empty() {
                    message_index += 1;
                    continue;
                }
                let text_id: gpui::ElementId = ("chat", agent.config.id).into();
                let content = TextView::markdown(
                    (text_id, message_index.to_string()),
                    entry.text.clone(),
                    window,
                    cx,
                )
                .style(text_style.clone())
                .selectable(true)
                .text_sm()
                .text_color(rgb(TEXT));
                match entry.role {
                    Role::User | Role::Agent => {
                        let from_user = matches!(entry.role, Role::User);
                        entries = entries.child(
                            div()
                                .w_full()
                                .min_w(px(0.))
                                .flex()
                                .when(from_user, |element| element.justify_end())
                                .child(
                                    div()
                                        .max_w(relative(0.85))
                                        .min_w(px(0.))
                                        .px_3()
                                        .py_2()
                                        .rounded_md()
                                        .bg(rgb(if from_user { USER_BUBBLE } else { AGENT_BUBBLE }))
                                        .text_sm()
                                        .text_color(rgb(TEXT))
                                        .whitespace_normal()
                                        .child(content),
                                ),
                        );
                    }
                    role => {
                        let (label, color) = match role {
                            Role::Thought => ("THOUGHT", MUTED),
                            Role::Tool => ("TOOL", TOOL_MARKER),
                            Role::System => ("SYSTEM", STATUS_ERROR),
                            _ => unreachable!(),
                        };
                        entries = entries.child(
                            div()
                                .max_w(px(900.))
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .bg(rgb(SURFACE))
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(color))
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .text_sm()
                                        .text_color(rgb(TEXT))
                                        .whitespace_normal()
                                        .child(content),
                                ),
                        );
                    }
                }
                message_index += 1;
            }
            for (index, prompt) in agent.config.pending_prompts.iter().enumerate() {
                entries = entries.child(
                    div().w_full().min_w(px(0.)).flex().justify_end().child(
                        div()
                            .max_w(relative(0.85))
                            .min_w(px(0.))
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(USER_BUBBLE))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ACCENT))
                                    .child(format!("Queued {}", index + 1)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(TEXT))
                                    .whitespace_normal()
                                    .child(prompt.clone()),
                            ),
                    ),
                );
            }
            for (permission_index, permission) in agent.permissions.iter().enumerate() {
                let mut choices = div().flex().gap_2().mt_3();
                for (option_index, (option_id, label)) in permission.options.iter().enumerate() {
                    let option_id = option_id.clone();
                    choices = choices.child(
                        div()
                            .id((
                                "permission",
                                ((permission_index as u64) << 32) | option_index as u64,
                            ))
                            .cursor_pointer()
                            .rounded_md()
                            .bg(rgb(ACCENT_SURFACE))
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(rgb(TEXT))
                            .child(label.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.choose_permission(permission_index, option_id.clone(), cx)
                            })),
                    );
                }
                entries = entries.child(
                    div()
                        .p_4()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(PERMISSION_BORDER))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(TEXT))
                                .child(permission.title.clone()),
                        )
                        .when_some(permission.description.as_ref(), |element, description| {
                            element.child(
                                div()
                                    .mt_2()
                                    .text_sm()
                                    .text_color(rgb(MUTED))
                                    .child(description.clone()),
                            )
                        })
                        .child(choices),
                );
            }
            chat = chat.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .relative()
                    .child(
                        div()
                            .id("chat-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.chat_scroll)
                            .child(entries),
                    )
                    .vertical_scrollbar(&self.chat_scroll),
            );
            if agent.active_work {
                chat = chat.child(
                    div()
                        .px_4()
                        .pb_1()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(
                            div()
                                .size(px(6.))
                                .rounded_full()
                                .bg(rgb(Status::Working.color())),
                        )
                        .child(if agent.cancel_requested {
                            "Stopping agent"
                        } else if agent.awaiting_response {
                            "Waiting for agent"
                        } else {
                            "Agent is working"
                        })
                        .when(!agent.cancel_requested, |row| {
                            row.child(
                                div()
                                    .id("stop-agent")
                                    .cursor_pointer()
                                    .ml_1()
                                    .size(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_sm()
                                    .bg(rgb(SURFACE))
                                    .hover(|style| style.bg(rgb(HOVER)))
                                    .child(div().size(px(9.)).rounded_sm().bg(rgb(TEXT)))
                                    .tooltip(|window, cx| {
                                        Tooltip::new("Stop agent").build(window, cx)
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_prompt(cx))),
                            )
                        }),
                );
            }
            let slash_commands = self.slash_results(cx);
            if !slash_commands.is_empty() {
                let mut menu = div()
                    .mx_4()
                    .mb_1()
                    .p_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .flex()
                    .flex_col();
                for (index, command) in slash_commands.into_iter().enumerate() {
                    let selected = index == self.slash_selection;
                    let name = format!("/{}", command.name);
                    let description = command.description.clone();
                    let hint = command.input_hint().map(str::to_owned);
                    menu = menu.child(
                        div()
                            .id(("slash-command", index))
                            .cursor_pointer()
                            .rounded_md()
                            .px_3()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_3()
                            .when(selected, |row| row.bg(rgb(SELECTED)))
                            .hover(|style| style.bg(rgb(SELECTED)))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_sm()
                                    .text_color(rgb(ACCENT))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child(description),
                            )
                            .when_some(hint, |row, hint| {
                                row.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(hint),
                                )
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.complete_slash_command(command.clone(), window, cx);
                            })),
                    );
                }
                chat = chat.child(menu);
            }
            chat = chat.child(
                div()
                    .p_4()
                    .flex()
                    .min_w(px(0.))
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Input::new(&self.composer)),
                    ),
            );
        } else {
            chat = chat.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        div()
                            .text_2xl()
                            .text_color(rgb(TEXT))
                            .child("Ready when you are."),
                    )
                    .child(
                        div()
                            .text_color(rgb(MUTED))
                            .child("Press Ctrl+P to find a folder and start an agent."),
                    ),
            );
        }
        if let Some(notice) = &self.notice {
            chat = chat.child(
                div()
                    .px_4()
                    .py_2()
                    .bg(rgb(ERROR_SURFACE))
                    .text_sm()
                    .text_color(rgb(ERROR_TEXT))
                    .child(notice.clone()),
            );
        }
        div()
            .size_full()
            .flex()
            .bg(rgb(BG))
            .on_action(cx.listener(Self::quick_open))
            .capture_key_down(cx.listener(Self::picker_key_down))
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                this.handle_slash_action(SlashAction::Up, window, cx)
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                this.handle_slash_action(SlashAction::Down, window, cx)
            }))
            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                if !action.secondary {
                    this.handle_slash_action(SlashAction::Complete, window, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                this.handle_slash_action(SlashAction::Complete, window, cx)
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                this.handle_slash_action(SlashAction::Dismiss, window, cx)
            }))
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<SidebarResize>, window, cx| {
                    let viewport_width = f32::from(window.viewport_size().width);
                    let position = f32::from(event.event.position.x);
                    let max_width = (viewport_width - 320.).max(180.);
                    this.sidebar_fraction = position.clamp(180., max_width) / viewport_width;
                    this.dirty = true;
                    cx.notify();
                }),
            )
            .child(sidebar)
            .child(divider)
            .child(chat)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if self.dirty {
            let _ = config::save(&self.config());
        }
    }
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);
        #[cfg(target_os = "macos")]
        cx.bind_keys([
            KeyBinding::new("cmd-p", QuickOpen, None),
            KeyBinding::new(
                "ctrl-enter",
                gpui_component::input::Enter { secondary: true },
                Some("Input"),
            ),
        ]);
        #[cfg(not(target_os = "macos"))]
        cx.bind_keys([KeyBinding::new("ctrl-p", QuickOpen, None)]);
        theme::apply(cx);
        let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Agentaps");
                let view = cx.new(|cx| Workspace::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("Could not open GPUI window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_icons_are_bundled() {
        assert!(
            gpui::AssetSource::load(&Assets, "icons/inbox.svg")
                .unwrap()
                .is_some()
        );
        assert!(
            gpui::AssetSource::load(&Assets, "icons/undo-2.svg")
                .unwrap()
                .is_some()
        );
    }

    fn agent(protocol: ProtocolVersion) -> AgentView {
        let mut agent = AgentView::new(AgentConfig {
            id: 1,
            command: vec!["fixture".into()],
            archived: false,
            display_name: None,
            session_id: None,
            model: None,
            context: None,
            messages: Vec::new(),
            available_commands: Vec::new(),
            pending_prompts: Vec::new(),
            was_working: false,
            session_has_activity: false,
        });
        agent.protocol = Some(protocol);
        agent.session_id = Some("session-1".into());
        agent.status = Status::Working;
        agent.active_work = true;
        agent
    }

    #[test]
    fn v2_prompt_acknowledgement_is_not_completion() {
        let mut agent = agent(ProtocolVersion::V2);
        agent.awaiting_response = true;
        agent.handle_prompt_response(&json!({"messageId":"user-1"}));
        assert_eq!(agent.status, Status::Working);
        assert!(agent.active_work);
        assert!(agent.awaiting_response);

        Workspace::handle_update(
            &mut agent,
            &json!({"params":{"sessionId":"session-1","update":{
                "sessionUpdate":"agent_message","messageId":"agent-1","content":[{"type":"text","text":"Hello"}]
            }}}),
        );
        Workspace::handle_update(
            &mut agent,
            &json!({"params":{"sessionId":"session-1","update":{
                "sessionUpdate":"agent_message_chunk","messageId":"agent-1","content":{"type":"text","text":" world"}
            }}}),
        );
        assert_eq!(agent.messages.len(), 1);
        assert_eq!(agent.messages[0].text, "Hello world");
        assert!(!agent.awaiting_response);

        Workspace::handle_update(
            &mut agent,
            &json!({"params":{"sessionId":"session-1","update":{
                "sessionUpdate":"state_update","state":"idle","stopReason":"end_turn"
            }}}),
        );
        assert_eq!(agent.status, Status::Done);
        assert!(!agent.active_work);
    }

    #[test]
    fn v1_prompt_response_still_completes_turn() {
        let mut agent = agent(ProtocolVersion::V1);
        agent.handle_prompt_response(&json!({"stopReason":"end_turn"}));
        assert_eq!(agent.status, Status::Done);
        assert!(!agent.active_work);
    }

    #[test]
    fn sidebar_drag_moves_in_both_directions() {
        let mut order = vec![1, 2, 3, 4];
        assert!(move_sidebar_id(&mut order, 1, 3));
        assert_eq!(order, vec![2, 3, 1, 4]);
        assert!(move_sidebar_id(&mut order, 4, 2));
        assert_eq!(order, vec![4, 2, 3, 1]);
        assert!(!move_sidebar_id(&mut order, 4, 4));
    }

    #[test]
    fn adjacent_tool_calls_form_one_group() {
        let mut agent = agent(ProtocolVersion::V2);
        agent.log(Role::Tool, "first tool");
        agent.log(Role::Tool, "second tool");
        agent.log(Role::Agent, "answer");
        agent.log(Role::Tool, "later tool");
        assert_eq!(tool_run_end(&agent.messages, 0), 2);
        assert_eq!(tool_run_end(&agent.messages, 3), 4);
        agent.config.archived = true;
        assert!(agent.snapshot().archived);
    }

    #[test]
    fn tool_activity_summarizes_search_read_and_approval() {
        assert_eq!(
            tool_description("rg -n 'guardian|review|tool call' src"),
            ("Search guardian|review|tool call in src".into(), true)
        );
        assert_eq!(
            tool_description("/bin/bash -lc \"sed -n '1,80p' src/main.rs\""),
            ("Read src/main.rs".into(), true)
        );
        assert_eq!(
            tool_description(
                "cd /project && head -80 README.md && ls && git log --oneline | wc -l"
            ),
            ("Read README.md · List files · 2 more".into(), true)
        );
        let mut agent = agent(ProtocolVersion::V1);
        agent.log(Role::Tool, "rg -n guardian src");
        agent.log(Role::Tool, "Guardian Review");
        agent.log(Role::Tool, "cat README.md");
        assert_eq!(tool_group_heading(&agent.messages), "Ran");
        assert_eq!(
            approval_summary(&agent.messages).as_deref(),
            Some("1 approval check")
        );
        assert_eq!(markdown_code_block("a ``` b"), "````\na ``` b\n````");
    }

    #[test]
    fn v1_tool_updates_replace_the_same_call_and_keep_approval_status() {
        let mut review_agent = agent(ProtocolVersion::V1);
        for update in [
            json!({"sessionUpdate":"tool_call","toolCallId":"guardian-1",
                "title":"Guardian Review","status":"in_progress"}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"guardian-1",
                "status":"completed"}),
        ] {
            Workspace::handle_update(
                &mut review_agent,
                &json!({"params":{"sessionId":"session-1","update":update}}),
            );
        }
        assert_eq!(review_agent.messages.len(), 1);
        assert_eq!(review_agent.messages[0].text, "Guardian Review · completed");
        assert_eq!(
            approval_summary(&review_agent.messages).as_deref(),
            Some("1 approval check · passed")
        );
        let mut tool = agent(ProtocolVersion::V2);
        tool.upsert_tool_call(
            &json!({"toolCallId":"command-1","title":"cat README.md","status":"in_progress"}),
        );
        tool.messages[0].text.push_str("\noutput text");
        tool.upsert_tool_call(&json!({"toolCallId":"command-1","status":"completed"}));
        assert_eq!(
            tool.messages[0].text,
            "cat README.md · completed\noutput text"
        );
    }

    #[test]
    fn model_and_context_follow_acp_session_updates() {
        let mut agent = agent(ProtocolVersion::V1);
        let options = json!([{"id":"model","category":"model","currentValue":"default",
            "options":[{"value":"default","name":"Default","description":"Opus (1M context)"}]}]);
        assert_eq!(
            selected_model(&options).as_deref(),
            Some("Opus (1M context)")
        );
        Workspace::handle_update(
            &mut agent,
            &json!({"params":{"sessionId":"session-1","update":{
                "sessionUpdate":"usage_update","used":42_000,"size":200_000
            }}}),
        );
        assert_eq!(agent.context, Some((42_000, 200_000)));
    }

    #[test]
    fn session_history_survives_config_round_trip() {
        let mut agent = agent(ProtocolVersion::V2);
        agent.model = Some("Example Model".into());
        agent.context = Some((3_000, 64_000));
        agent.log(Role::User, "Please check the build");
        agent.log(Role::Tool, "cargo test · completed");
        agent.config.pending_prompts.push("Follow up next".into());
        let saved = agent.snapshot();
        let encoded = serde_json::to_vec(&saved).unwrap();
        let restored = AgentView::new(serde_json::from_slice(&encoded).unwrap());
        assert_eq!(restored.config.session_id.as_deref(), Some("session-1"));
        assert_eq!(restored.model.as_deref(), Some("Example Model"));
        assert_eq!(restored.context, Some((3_000, 64_000)));
        assert_eq!(restored.messages[0].text, "Please check the build");
        assert_eq!(restored.messages[1].text, "cargo test · completed");
        assert!(restored.messages[2].text.contains("active"));
        assert_eq!(restored.config.pending_prompts, vec!["Follow up next"]);
    }

    #[test]
    fn slash_commands_follow_agent_snapshots_and_complete_with_input_hint() {
        let mut agent = agent(ProtocolVersion::V1);
        Workspace::handle_update(
            &mut agent,
            &json!({"params":{"sessionId":"session-1","update":{
                "sessionUpdate":"available_commands_update","availableCommands":[
                    {"name":"review","description":"Review changes","input":{"hint":"files"}},
                    {"name":"reset","description":"Reset chat"}
                ]
            }}}),
        );
        assert_eq!(
            matching_slash_commands(&agent.config.available_commands, "/re").len(),
            2
        );
        assert_eq!(
            matching_slash_commands(&agent.config.available_commands, "/review ").len(),
            0
        );
        assert_eq!(
            completed_slash_text(&agent.config.available_commands[0]),
            "/review "
        );
        assert_eq!(
            completed_slash_text(&agent.config.available_commands[1]),
            "/reset"
        );

        Workspace::handle_update(
            &mut agent,
            &json!({"params":{"sessionId":"session-1","update":{
                "sessionUpdate":"available_commands_update","availableCommands":[
                    {"name":"plan","description":"Plan work"}
                ]
            }}}),
        );
        assert_eq!(
            matching_slash_commands(&agent.config.available_commands, "/re").len(),
            0
        );

        let mut v2_agent = self::agent(ProtocolVersion::V2);
        Workspace::handle_update(
            &mut v2_agent,
            &json!({"params":{"sessionId":"session-1","update":{
                "sessionUpdate":"available_commands_update","availableCommands":[
                    {"name":"plan","description":"Plan work"}
                ]
            }}}),
        );
        assert_eq!(v2_agent.config.available_commands[0].name, "plan");
    }

    #[test]
    fn queued_prompt_stays_saved_when_agent_cannot_send_it() {
        let mut agent = agent(ProtocolVersion::V2);
        agent.config.pending_prompts.push("Next request".into());
        assert!(!agent.start_next_queued_prompt());
        Workspace::handle_update(
            &mut agent,
            &json!({"params":{"sessionId":"session-1","update":{
                "sessionUpdate":"state_update","state":"idle","stopReason":"end_turn"
            }}}),
        );
        assert!(agent.start_next_queued_prompt());
        assert_eq!(agent.status, Status::Error);
        assert_eq!(agent.config.pending_prompts, vec!["Next request"]);
    }

    #[test]
    fn queued_prompts_reach_the_agent_in_order_after_each_turn() {
        let mut agent = agent(ProtocolVersion::V1);
        let command = vec![
            "/bin/sh".into(),
            "-c".into(),
            "while IFS= read -r line; do printf '%s\\n' \"$line\"; done".into(),
        ];
        let (events_tx, events_rx) = mpsc::channel();
        agent.connection = Some(Connection::spawn(1, &command, Path::new("/"), events_tx).unwrap());
        agent.config.pending_prompts = vec!["First".into(), "Second".into()];

        assert!(!agent.start_next_queued_prompt());
        agent.handle_prompt_response(&json!({"stopReason":"end_turn"}));
        for expected in ["First", "Second"] {
            assert!(agent.start_next_queued_prompt());
            let Event::Message { value, .. } =
                events_rx.recv_timeout(Duration::from_secs(2)).unwrap()
            else {
                panic!("agent disconnected before receiving queued prompt");
            };
            assert_eq!(value["params"]["prompt"][0]["text"], expected);
            agent.handle_prompt_response(&json!({"stopReason":"end_turn"}));
        }
        assert!(agent.config.pending_prompts.is_empty());
    }

    #[test]
    fn restoration_respects_v1_capabilities() {
        assert_eq!(
            restore_mode(ProtocolVersion::V2, &json!({})),
            Some(RestoreMode::Resume)
        );
        assert_eq!(
            restore_mode(
                ProtocolVersion::V1,
                &json!({"agentCapabilities":{"sessionCapabilities":{"resume":{}},"loadSession":true}})
            ),
            Some(RestoreMode::Resume)
        );
        assert_eq!(
            restore_mode(
                ProtocolVersion::V1,
                &json!({"agentCapabilities":{"loadSession":true}})
            ),
            Some(RestoreMode::Load)
        );
        assert_eq!(restore_mode(ProtocolVersion::V1, &json!({})), None);
    }

    #[test]
    fn v1_load_replay_does_not_duplicate_saved_history() {
        let mut agent = agent(ProtocolVersion::V1);
        agent.restoring = Some(RestoreMode::Load);
        agent.log(Role::Agent, "Saved reply");
        Workspace::handle_update(
            &mut agent,
            &json!({"params":{"sessionId":"session-1","update":{
                "sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Saved reply"}
            }}}),
        );
        assert_eq!(agent.messages.len(), 1);
        assert_eq!(agent.messages[0].text, "Saved reply");
    }

    #[test]
    fn empty_sessions_are_not_restored() {
        let mut agent = agent(ProtocolVersion::V2);
        agent.active_work = false;
        agent.log(Role::System, "A prior restore attempt failed");
        assert!(!agent.has_restorable_activity());
        agent.log(Role::User, "Continue this conversation");
        assert!(agent.has_restorable_activity());
    }

    #[test]
    fn submitting_keeps_explicit_newlines() {
        assert_eq!(submitted_prompt("first\nsecond\n"), "first\nsecond");
        assert_eq!(submitted_prompt("first\n\n"), "first\n");
    }
}
