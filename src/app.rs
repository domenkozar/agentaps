use crate::acp::{Connection, Event};
use crate::config::{AgentConfig, ChatEntry, Config, ProjectConfig, Role, SlashCommand};
use crate::diff_view::{File as DiffFile, Presentation as DiffPresentation, Row as DiffRow};
use crate::discovery::{AgentChoice, discover_folders, installed_agents, score};
use crate::file_search::{FileSearch, scan_project};
use crate::folder_search::{FolderSearch, inject_path};
use crate::theme::*;
use crate::{config, theme};
use agent_client_protocol_schema::{ProtocolVersion, v2};
use gpui::{
    App, Bounds, Context, DragMoveEvent, Entity, Focusable, IntoElement, KeyBinding, KeyDownEvent,
    ListAlignment, ListState, MouseButton, Render, StatefulInteractiveElement, Subscription,
    Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, relative, rems, rgb, size,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Root,
    button::{Button, ButtonVariants},
    input::{
        Enter, Escape, IndentInline, Input, InputEvent, InputState, MoveDown, MoveUp, Position,
        Textarea, TextareaState,
    },
    menu::{DropdownMenu, PopupMenuItem},
    scroll::ScrollableElement,
    text::{TextView, TextViewStyle},
    tooltip::Tooltip,
};
use gpui_component_assets::Assets;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

mod agent;
mod elicitation;
mod render;
mod sessions;
#[cfg(test)]
mod tests;
mod tool_activity;

use self::{agent::*, elicitation::*, tool_activity::*};

type DiffLoadResult = Result<
    (
        Vec<DiffFile>,
        Vec<(usize, usize)>,
        DiffPresentation,
        Option<String>,
        Arc<Vec<DiffRow>>,
    ),
    String,
>;

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

actions!(workspace, [QuickOpen]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SessionLocation {
    project_index: usize,
    agent_index: usize,
}

fn session_search_score(query: &str, path: &Path, branch: &str, agent: &str) -> Option<i32> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    [
        score(query, &name).map(|rank| rank + 30),
        score(query, agent).map(|rank| rank + 30),
        score(query, &path.to_string_lossy()),
        score(query, branch),
    ]
    .into_iter()
    .flatten()
    .max()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerStep {
    Folders,
    Agents { project_index: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceView {
    Empty,
    Conversation(SessionLocation),
    Archive {
        return_to: Option<SessionLocation>,
    },
    NewSession {
        step: PickerStep,
        return_to: Option<SessionLocation>,
    },
}

impl WorkspaceView {
    fn highlighted_session(self) -> Option<SessionLocation> {
        match self {
            Self::Conversation(session) => Some(session),
            _ => None,
        }
    }

    fn return_to(self) -> Option<SessionLocation> {
        match self {
            Self::Conversation(session) => Some(session),
            Self::Archive { return_to } | Self::NewSession { return_to, .. } => return_to,
            Self::Empty => None,
        }
    }

    fn displayed_session(self) -> Option<SessionLocation> {
        match self {
            Self::Conversation(session) => Some(session),
            Self::Archive { return_to } => return_to,
            Self::Empty | Self::NewSession { .. } => None,
        }
    }

    fn open_picker(self, step: PickerStep) -> Self {
        Self::NewSession {
            step,
            return_to: self.return_to(),
        }
    }

    fn toggle_archive(self) -> Self {
        match self {
            Self::Archive { return_to } => return_to.map(Self::Conversation).unwrap_or(Self::Empty),
            _ => Self::Archive {
                return_to: self.return_to(),
            },
        }
    }

    fn session_archived(self, archived: SessionLocation, next: Option<SessionLocation>) -> Self {
        match self {
            Self::Conversation(session) if session == archived => {
                next.map(Self::Conversation).unwrap_or(Self::Empty)
            }
            Self::Archive {
                return_to: Some(session),
            } if session == archived => Self::Archive { return_to: next },
            Self::NewSession {
                step,
                return_to: Some(session),
            } if session == archived => Self::NewSession {
                step,
                return_to: next,
            },
            _ => self,
        }
    }
}

#[derive(Clone, Copy)]
enum SlashAction {
    Up,
    Down,
    Complete,
    Dismiss,
}

fn cursor_byte_offset(text: &str, position: Position) -> usize {
    let mut offset = 0;
    for (line, content) in text.split_inclusive('\n').enumerate() {
        if line == position.line as usize {
            let mut column = 0;
            for (index, character) in content.char_indices() {
                if column >= position.character as usize {
                    return offset + index;
                }
                column += character.len_utf16();
            }
            return offset + content.len();
        }
        offset += content.len();
    }
    text.len()
}

fn file_mention(text: &str, cursor: Position) -> Option<(std::ops::Range<usize>, &str)> {
    let end = cursor_byte_offset(text, cursor);
    let before = &text[..end];
    let token_start = before
        .rmatch_indices(char::is_whitespace)
        .next()
        .map_or(0, |(index, whitespace)| index + whitespace.len());
    let token = &before[token_start..];
    let at = token.rfind('@')? + token_start;
    if at > 0
        && !text[..at]
            .chars()
            .last()
            .is_some_and(|c| c.is_whitespace() || matches!(c, '(' | '[' | '{' | '"' | '\''))
    {
        return None;
    }
    let rest = &text[end..];
    let suffix_len = rest
        .find(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ')' | ']' | '}' | '"' | '\'')
        })
        .unwrap_or(rest.len());
    Some((at..end + suffix_len, &text[at + 1..end]))
}

fn completed_file_text(
    text: &str,
    range: std::ops::Range<usize>,
    file: &str,
) -> (String, Position) {
    let following_space = text[range.end..].starts_with(' ');
    let replacement = format!("@{file}{}", if following_space { "" } else { " " });
    let mut completed = text.to_owned();
    completed.replace_range(range.start..range.end, &replacement);
    let cursor = range.start + replacement.len() + usize::from(following_space);
    let before = &completed[..cursor];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .encode_utf16()
        .count() as u32;
    (completed, Position::new(line, column))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatRowKind {
    Empty,
    Message(usize),
    Tools(usize, usize),
    Queued(usize),
    Permission(usize),
    Elicitation(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ChatRow {
    kind: ChatRowKind,
    signature: u64,
}

struct PromptRecall {
    agent_id: u64,
    index: usize,
    draft: String,
    displayed: String,
}

impl PromptRecall {
    fn step(
        state: &mut Option<Self>,
        agent_id: u64,
        history: &[String],
        current: &str,
        up: bool,
    ) -> Option<String> {
        if history.is_empty() {
            return None;
        }
        if state
            .as_ref()
            .is_some_and(|recall| recall.agent_id != agent_id)
        {
            *state = None;
        }
        if state.is_none() {
            if !up {
                return None;
            }
            *state = Some(Self {
                agent_id,
                index: history.len(),
                draft: current.to_owned(),
                displayed: current.to_owned(),
            });
        }
        let recall = state.as_mut().unwrap();
        recall.index = if up {
            recall.index.saturating_sub(1)
        } else {
            (recall.index + 1).min(history.len())
        };
        recall.displayed = if recall.index == history.len() {
            recall.draft.clone()
        } else {
            history[recall.index].clone()
        };
        Some(recall.displayed.clone())
    }
}

struct Workspace {
    projects: Vec<ProjectView>,
    view: WorkspaceView,
    sidebar_order: Vec<u64>,
    sidebar_fraction: f32,
    collapsed_tool_groups: HashSet<(u64, usize)>,
    expanded_tool_rows: HashSet<(u64, usize)>,
    next_agent_id: u64,
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
    folders_rx: Receiver<Vec<PathBuf>>,
    folder_search: FolderSearch,
    folder_scan_complete: bool,
    file_search: Option<FileSearch>,
    file_search_project: Option<usize>,
    file_scan_tx: Sender<u64>,
    file_scan_rx: Receiver<u64>,
    file_scan_generation: u64,
    file_selection: usize,
    file_dismissed: bool,
    available_agents: Vec<AgentChoice>,
    picker_selection: usize,
    sidebar_selection: usize,
    slash_selection: usize,
    slash_dismissed: bool,
    prompt_recall: Option<PromptRecall>,
    picker_input: Entity<InputState>,
    sidebar_search: Entity<InputState>,
    composer: Entity<TextareaState>,
    chat_list: ListState,
    chat_list_agent: Option<u64>,
    chat_rows: Vec<ChatRow>,
    diff_visible: bool,
    diff_selected_file: Option<String>,
    diff_presentation: DiffPresentation,
    diff_rows: Arc<Vec<DiffRow>>,
    diff_list: ListState,
    diff_tx: Sender<(u64, DiffLoadResult)>,
    diff_rx: Receiver<(u64, DiffLoadResult)>,
    diff_request_id: u64,
    diff_watch_tx: Sender<u64>,
    diff_watch_rx: Receiver<u64>,
    diff_watcher_tx: Sender<(u64, notify::Result<notify::RecommendedWatcher>)>,
    diff_watcher_rx: Receiver<(u64, notify::Result<notify::RecommendedWatcher>)>,
    diff_watch_generation: u64,
    diff_watcher: Option<notify::RecommendedWatcher>,
    diff_refresh_due: Option<Instant>,
    diff_first_change_at: Option<Instant>,
    diff_poll_at: Option<Instant>,
    diff_loading: bool,
    diff_error: Option<String>,
    diff_files: Vec<DiffFile>,
    diff_file_stats: Vec<(usize, usize)>,
    dirty: bool,
    last_saved: Instant,
    notice: Option<String>,
    _subscriptions: Vec<Subscription>,
}

fn diff_rows_for_file(
    files: &[DiffFile],
    path: Option<&str>,
    presentation: DiffPresentation,
) -> Vec<DiffRow> {
    path.and_then(|path| files.iter().find(|file| file.path == path))
        .map_or_else(Vec::new, |file| {
            crate::diff_view::flatten(std::slice::from_ref(file), presentation)
        })
}

fn branch(path: &Path) -> String {
    if let Ok(repo) = gix::discover(path) {
        if let Ok(Some(name)) = repo.head_name() {
            return name.shorten().to_string();
        }
        if let Ok(id) = repo.head_id() {
            return id.shorten_or_id().to_string();
        }
    }
    "no git branch".into()
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

impl Workspace {
    fn set_view(&mut self, view: WorkspaceView) {
        if self.view.displayed_session() != view.displayed_session() {
            self.prompt_recall = None;
            self.close_diff();
            self.diff_error = None;
            self.diff_files.clear();
            self.diff_rows = Arc::new(Vec::new());
            self.diff_list = ListState::new(0, ListAlignment::Top, px(28.));
        }
        let project_index = view
            .displayed_session()
            .map(|session| session.project_index);
        if self.file_search_project != project_index {
            self.file_search_project = project_index;
            self.file_scan_generation += 1;
            let generation = self.file_scan_generation;
            self.file_selection = 0;
            self.file_dismissed = false;
            self.file_search = project_index.map(|index| {
                let search = FileSearch::new();
                let injector = search.injector();
                let root = self.projects[index].path.clone();
                let sender = self.file_scan_tx.clone();
                std::thread::spawn(move || {
                    scan_project(&root, &injector);
                    let _ = sender.send(generation);
                });
                search
            });
        }
        if matches!(self.view, WorkspaceView::Archive { .. })
            != matches!(view, WorkspaceView::Archive { .. })
        {
            self.sidebar_selection = 0;
        }
        self.view = view;
        self.mark_displayed_agent_viewed();
    }

    fn open_diff(&mut self, project_index: usize, cx: &mut Context<Self>) {
        self.diff_visible = true;
        self.diff_selected_file = None;
        self.diff_files.clear();
        self.diff_file_stats.clear();
        self.diff_rows = Arc::new(Vec::new());
        self.diff_list = ListState::new(0, ListAlignment::Top, px(28.));
        self.diff_watch_generation += 1;
        let generation = self.diff_watch_generation;
        let path = self.projects[project_index].path.clone();
        let watch_tx = self.diff_watch_tx.clone();
        let watcher_tx = self.diff_watcher_tx.clone();
        self.diff_watcher = None;
        self.diff_poll_at = None;
        self.diff_refresh_due = None;
        self.diff_first_change_at = None;
        self.refresh_diff(project_index, cx);
        std::thread::spawn(move || {
            let watcher = crate::diff_watch::start(&path, generation, watch_tx);
            let _ = watcher_tx.send((generation, watcher));
        });
    }

    fn close_diff(&mut self) {
        self.diff_visible = false;
        self.diff_selected_file = None;
        self.diff_request_id += 1;
        self.diff_loading = false;
        self.diff_watcher = None;
        self.diff_watch_generation += 1;
        self.diff_refresh_due = None;
        self.diff_first_change_at = None;
        self.diff_poll_at = None;
    }

    fn refresh_diff(&mut self, project_index: usize, cx: &mut Context<Self>) {
        self.diff_request_id += 1;
        let request_id = self.diff_request_id;
        let path = self.projects[project_index].path.clone();
        let presentation = self.diff_presentation;
        let selected_file = self.diff_selected_file.clone();
        let tx = self.diff_tx.clone();
        self.diff_loading = true;
        self.diff_error = None;
        std::thread::spawn(move || {
            let result = crate::git_diff::load(&path).map(|files| {
                let stats = files.iter().map(crate::diff_view::stats).collect();
                let rows = Arc::new(diff_rows_for_file(
                    &files,
                    selected_file.as_deref(),
                    presentation,
                ));
                (files, stats, presentation, selected_file, rows)
            });
            let _ = tx.send((request_id, result));
        });
        cx.notify();
    }

    fn set_diff_presentation(
        &mut self,
        presentation: crate::diff_view::Presentation,
        cx: &mut Context<Self>,
    ) {
        self.diff_presentation = presentation;
        self.diff_rows = Arc::new(diff_rows_for_file(
            &self.diff_files,
            self.diff_selected_file.as_deref(),
            presentation,
        ));
        self.diff_list = ListState::new(self.diff_rows.len(), ListAlignment::Top, px(28.));
        cx.notify();
    }

    fn select_diff_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.diff_selected_file = Some(path);
        self.set_diff_presentation(self.diff_presentation, cx);
    }

    fn show_diff_summary(&mut self, cx: &mut Context<Self>) {
        self.diff_selected_file = None;
        self.set_diff_presentation(self.diff_presentation, cx);
    }

    fn mark_displayed_agent_viewed(&mut self) {
        if let Some(session) = self.view.displayed_session() {
            self.projects[session.project_index].agents[session.agent_index].mark_viewed();
        }
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let picker_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search folders by name or path…"));
        let sidebar_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search sessions…"));
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 6)
                .submit_on_enter(true)
                .placeholder("Ask your agent…")
        });
        let _subscriptions = vec![
            cx.subscribe_in(
                &sidebar_search,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.sidebar_selection = 0;
                        this.select_sidebar_session(cx);
                        cx.notify();
                    }
                    InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    } => {
                        this.confirm_sidebar_session(window, cx);
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &picker_input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.picker_selection = 0;
                        if matches!(
                            this.view,
                            WorkspaceView::NewSession {
                                step: PickerStep::Folders,
                                ..
                            }
                        ) {
                            this.folder_search
                                .set_query(&this.picker_input.read(cx).value());
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    } => {
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
                        if this.prompt_recall.as_ref().is_some_and(|recall| {
                            recall.displayed != this.composer.read(cx).value().as_ref()
                        }) {
                            this.prompt_recall = None;
                        }
                        this.slash_selection = 0;
                        this.slash_dismissed = false;
                        this.file_selection = 0;
                        this.file_dismissed = false;
                        this.update_file_query(cx);
                        cx.notify();
                    }
                    InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    } => this.send_prompt(window, cx),
                    _ => {}
                },
            ),
        ];
        let (events_tx, events_rx) = mpsc::channel();
        let (file_scan_tx, file_scan_rx) = mpsc::channel();
        let (diff_tx, diff_rx) = mpsc::channel();
        let (diff_watch_tx, diff_watch_rx) = mpsc::channel();
        let (diff_watcher_tx, diff_watcher_rx) = mpsc::channel();
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
        let mut recent_folders: Vec<PathBuf> = projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        if let Ok(cwd) = std::env::current_dir().and_then(|path| path.canonicalize())
            && !recent_folders.contains(&cwd)
        {
            recent_folders.insert(0, cwd);
        }
        let folder_search = FolderSearch::new(recent_folders.clone());
        let folder_injector = folder_search.injector();
        let (folders_tx, folders_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let folders = discover_folders();
            for path in &folders {
                if !recent_folders.contains(path) {
                    inject_path(&folder_injector, path.clone());
                }
            }
            let _ = folders_tx.send(folders);
        });
        let mut this = Self {
            projects,
            view: WorkspaceView::Empty,
            sidebar_order,
            sidebar_fraction,
            collapsed_tool_groups: HashSet::new(),
            expanded_tool_rows: HashSet::new(),
            next_agent_id,
            events_tx,
            events_rx,
            folders_rx,
            folder_search,
            folder_scan_complete: false,
            file_search: None,
            file_search_project: None,
            file_scan_tx,
            file_scan_rx,
            file_scan_generation: 0,
            file_selection: 0,
            file_dismissed: false,
            available_agents: installed_agents(),
            picker_selection: 0,
            sidebar_selection: 0,
            slash_selection: 0,
            slash_dismissed: false,
            prompt_recall: None,
            picker_input,
            sidebar_search,
            composer,
            chat_list: ListState::new(0, ListAlignment::Bottom, px(300.)),
            chat_list_agent: None,
            chat_rows: Vec::new(),
            diff_visible: false,
            diff_selected_file: None,
            diff_presentation: crate::diff_view::Presentation::Unified,
            diff_rows: Arc::new(Vec::new()),
            diff_list: ListState::new(0, ListAlignment::Top, px(28.)),
            diff_tx,
            diff_rx,
            diff_request_id: 0,
            diff_watch_tx,
            diff_watch_rx,
            diff_watcher_tx,
            diff_watcher_rx,
            diff_watch_generation: 0,
            diff_watcher: None,
            diff_refresh_due: None,
            diff_first_change_at: None,
            diff_poll_at: None,
            diff_loading: false,
            diff_error: None,
            diff_files: Vec::new(),
            diff_file_stats: Vec::new(),
            dirty: migrate_config,
            last_saved: Instant::now(),
            notice,
            _subscriptions,
        };
        let mut selected = None;
        for project_index in 0..this.projects.len() {
            for agent_index in 0..this.projects[project_index].agents.len() {
                if this.projects[project_index].agents[agent_index]
                    .config
                    .archived
                {
                    continue;
                }
                this.connect(project_index, agent_index);
                selected.get_or_insert(SessionLocation {
                    project_index,
                    agent_index,
                });
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
                    selected = Some(SessionLocation {
                        project_index,
                        agent_index,
                    });
                    break;
                }
            }
        }
        if let Some(session) = selected {
            this.set_view(WorkspaceView::Conversation(session));
            this.composer
                .update(cx, |input, cx| input.focus(window, cx));
        } else {
            let view = if this
                .projects
                .iter()
                .any(|project| project.agents.iter().any(|agent| agent.config.archived))
            {
                WorkspaceView::Empty
            } else if !this.projects.is_empty() {
                WorkspaceView::NewSession {
                    step: PickerStep::Agents { project_index: 0 },
                    return_to: None,
                }
            } else {
                WorkspaceView::NewSession {
                    step: PickerStep::Folders,
                    return_to: None,
                }
            };
            this.set_view(view);
            if matches!(this.view, WorkspaceView::NewSession { .. }) {
                this.picker_input
                    .update(cx, |input, cx| input.focus(window, cx));
            }
        }
        let background_executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                background_executor.timer(Duration::from_millis(100)).await;
                if this
                    .update_in(cx, |this, window, cx| {
                        this.poll_events(window, cx);
                        if let Ok(mut folders) = this.folders_rx.try_recv() {
                            folders
                                .extend(this.projects.iter().map(|project| project.path.clone()));
                            folders.sort();
                            folders.dedup();
                            this.folder_search.set_paths(&folders);
                            this.folder_scan_complete = true;
                            cx.notify();
                        }
                        if this.folder_search.tick() {
                            cx.notify();
                        }
                        while let Ok(generation) = this.file_scan_rx.try_recv() {
                            if this.file_scan_generation == generation
                                && let Some(search) = this.file_search.as_mut()
                            {
                                search.scan_complete();
                                cx.notify();
                            }
                        }
                        if this.file_search.as_mut().is_some_and(FileSearch::tick) {
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

    fn open_picker(&mut self, step: PickerStep, window: &mut Window, cx: &mut Context<Self>) {
        self.set_view(self.view.open_picker(step));
        self.picker_selection = 0;
        if step == PickerStep::Folders {
            self.folder_search.set_query("");
        }
        self.picker_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_placeholder(
                match step {
                    PickerStep::Agents { .. } => "Search installed agents or enter an ACP command…",
                    PickerStep::Folders => "Search folders by name or path…",
                },
                window,
                cx,
            );
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn back_from_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.view {
            WorkspaceView::NewSession {
                step: PickerStep::Agents { .. },
                ..
            } => self.open_picker(PickerStep::Folders, window, cx),
            WorkspaceView::NewSession {
                step: PickerStep::Folders,
                return_to: Some(session),
            } => {
                self.set_view(WorkspaceView::Conversation(session));
                self.composer
                    .update(cx, |input, cx| input.focus(window, cx));
                cx.notify();
            }
            _ => {}
        }
    }

    fn sidebar_results(&self, query: &str) -> Vec<SessionLocation> {
        let archive_view = matches!(self.view, WorkspaceView::Archive { .. });
        let mut matches = self
            .projects
            .iter()
            .enumerate()
            .flat_map(|(project_index, project)| {
                project
                    .agents
                    .iter()
                    .enumerate()
                    .filter_map(move |(agent_index, agent)| {
                        if agent.config.archived != archive_view {
                            return None;
                        }
                        let rank = session_search_score(
                            query,
                            &project.path,
                            &project.branch,
                            &agent.name,
                        )?;
                        let order = self
                            .sidebar_order
                            .iter()
                            .position(|id| *id == agent.config.id)
                            .unwrap_or(usize::MAX);
                        Some((
                            rank,
                            order,
                            SessionLocation {
                                project_index,
                                agent_index,
                            },
                        ))
                    })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| {
            if query.is_empty() {
                a.1.cmp(&b.1)
            } else {
                b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1))
            }
        });
        matches
            .into_iter()
            .map(|(_, _, location)| location)
            .collect()
    }

    fn confirm_sidebar_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.sidebar_search.read(cx).value().trim().to_owned();
        if query.is_empty() {
            return;
        }
        let Some(location) = self
            .sidebar_results(&query)
            .get(self.sidebar_selection)
            .copied()
        else {
            return;
        };
        if self.projects[location.project_index].agents[location.agent_index]
            .config
            .archived
        {
            self.set_archived(
                location.project_index,
                location.agent_index,
                false,
                window,
                cx,
            );
        } else {
            self.set_view(WorkspaceView::Conversation(location));
            self.composer
                .update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
        }
    }

    fn select_sidebar_session(&mut self, cx: &mut Context<Self>) {
        let query = self.sidebar_search.read(cx).value().trim().to_owned();
        if query.is_empty() {
            return;
        }
        let Some(location) = self
            .sidebar_results(&query)
            .get(self.sidebar_selection)
            .copied()
        else {
            return;
        };
        if !self.projects[location.project_index].agents[location.agent_index]
            .config
            .archived
        {
            self.set_view(WorkspaceView::Conversation(location));
        }
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
            self.folder_search.add_recent(path);
            self.persist();
            self.projects.len() - 1
        };
        self.notice = None;
        self.open_picker(PickerStep::Agents { project_index }, window, cx);
    }

    fn start_agent(
        &mut self,
        command: Vec<String>,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let WorkspaceView::NewSession {
            step: PickerStep::Agents { project_index },
            ..
        } = self.view
        else {
            self.notice = Some("Choose a project first".into());
            cx.notify();
            return;
        };
        self.start_agent_for_project(project_index, command, name, window, cx);
    }

    fn start_agent_for_project(
        &mut self,
        project_index: usize,
        command: Vec<String>,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            prompt_history: Vec::new(),
            was_working: false,
            session_has_activity: false,
        };
        self.sidebar_order.push(config.id);
        self.next_agent_id += 1;
        let agent_index = self.projects[project_index].agents.len();
        self.projects[project_index]
            .agents
            .push(AgentView::new(config));
        self.set_view(WorkspaceView::Conversation(SessionLocation {
            project_index,
            agent_index,
        }));
        self.connect(project_index, agent_index);
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
            let location = SessionLocation {
                project_index,
                agent_index,
            };
            if self.view.return_to() == Some(location) {
                let next = self.sidebar_order.iter().find_map(|id| {
                    self.projects.iter().enumerate().find_map(|(pi, project)| {
                        project.agents.iter().enumerate().find_map(|(ai, agent)| {
                            (agent.config.id == *id && !agent.config.archived).then_some(
                                SessionLocation {
                                    project_index: pi,
                                    agent_index: ai,
                                },
                            )
                        })
                    })
                });
                self.set_view(self.view.session_archived(location, next));
            }
        } else {
            if self.projects[project_index].agents[agent_index]
                .connection
                .is_none()
            {
                self.connect(project_index, agent_index);
            }
            self.set_view(WorkspaceView::Conversation(SessionLocation {
                project_index,
                agent_index,
            }));
            self.composer
                .update(cx, |input, cx| input.focus(window, cx));
        }
        self.persist();
        cx.notify();
    }

    fn confirm_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.view {
            WorkspaceView::NewSession {
                step: PickerStep::Folders,
                ..
            } => {
                if let Some(path) = self
                    .folder_search
                    .results()
                    .get(self.picker_selection)
                    .cloned()
                {
                    self.select_folder(path, window, cx);
                } else {
                    self.notice =
                        Some("No matching folder. Type an existing absolute path.".into());
                    cx.notify();
                }
            }
            WorkspaceView::NewSession {
                step: PickerStep::Agents { .. },
                ..
            } => {
                if let Some(agent) = self.agent_results(cx).get(self.picker_selection).cloned() {
                    self.start_agent(agent.command, Some(agent.name), window, cx);
                } else {
                    self.start_custom_agent(window, cx);
                }
            }
            _ => {}
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
        self.open_picker(PickerStep::Folders, window, cx);
    }

    fn workspace_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .sidebar_search
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
        {
            let value = self.sidebar_search.read(cx).value().to_string();
            if event.keystroke.key == "escape" && !value.is_empty() {
                self.sidebar_search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                cx.stop_propagation();
                return;
            }
            let query = value.trim();
            if !query.is_empty() {
                let count = self.sidebar_results(query).len();
                match event.keystroke.key.as_str() {
                    "down" if count > 0 => {
                        self.sidebar_selection = (self.sidebar_selection + 1) % count;
                        self.select_sidebar_session(cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "up" if count > 0 => {
                        self.sidebar_selection = (self.sidebar_selection + count - 1) % count;
                        self.select_sidebar_session(cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    _ => {}
                }
            }
            return;
        }
        let WorkspaceView::NewSession { step, .. } = self.view else {
            return;
        };
        let count = match step {
            PickerStep::Folders => self.folder_search.results().len(),
            PickerStep::Agents { .. } => {
                self.agent_results(cx).len()
                    + usize::from(!self.picker_input.read(cx).value().trim().is_empty())
            }
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
                if self.picker_input.read(cx).value().is_empty() {
                    self.back_from_picker(window, cx);
                } else {
                    self.picker_input
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn slash_results(&self, cx: &Context<Self>) -> Vec<SlashCommand> {
        if self.slash_dismissed {
            return Vec::new();
        }
        let Some(SessionLocation {
            project_index,
            agent_index,
        }) = self.view.displayed_session()
        else {
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

    fn update_file_query(&mut self, cx: &Context<Self>) {
        let input = self.composer.read(cx);
        let draft = input.value().to_string();
        let query = file_mention(&draft, input.cursor_position()).map(|(_, query)| query);
        if let (Some(search), Some(query)) = (self.file_search.as_mut(), query) {
            search.set_query(query);
        }
    }

    fn file_results(&self, cx: &Context<Self>) -> Vec<String> {
        if self.file_dismissed || self.view.displayed_session().is_none() {
            return Vec::new();
        }
        let input = self.composer.read(cx);
        let draft = input.value().to_string();
        let Some((_, query)) = file_mention(&draft, input.cursor_position()) else {
            return Vec::new();
        };
        self.file_search
            .as_ref()
            .filter(|search| search.query() == query)
            .map_or_else(Vec::new, |search| search.results().to_vec())
    }

    fn file_mention_active(&self, cx: &Context<Self>) -> bool {
        if self.file_dismissed || self.view.displayed_session().is_none() {
            return false;
        }
        let input = self.composer.read(cx);
        file_mention(&input.value(), input.cursor_position()).is_some()
    }

    fn complete_file(&mut self, file: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.composer.read(cx);
        let draft = input.value().to_string();
        let Some((range, _)) = file_mention(&draft, input.cursor_position()) else {
            return;
        };
        let (value, cursor) = completed_file_text(&draft, range, file);
        self.composer.update(cx, |input, cx| {
            input.replace_all(value, window, cx);
            input.set_cursor_position(cursor, window, cx);
        });
        self.file_dismissed = true;
        cx.notify();
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
        let files = self.file_results(cx);
        if !files.is_empty() {
            match action {
                SlashAction::Down => self.file_selection = (self.file_selection + 1) % files.len(),
                SlashAction::Up => {
                    self.file_selection = (self.file_selection + files.len() - 1) % files.len()
                }
                SlashAction::Complete => {
                    let index = self.file_selection.min(files.len() - 1);
                    self.complete_file(&files[index], window, cx);
                }
                SlashAction::Dismiss => self.file_dismissed = true,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.file_mention_active(cx)
            && matches!(
                action,
                SlashAction::Up | SlashAction::Down | SlashAction::Dismiss
            )
        {
            if matches!(action, SlashAction::Dismiss) {
                self.file_dismissed = true;
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        let commands = self.slash_results(cx);
        if commands.is_empty() {
            if matches!(action, SlashAction::Up | SlashAction::Down) {
                self.handle_prompt_recall(matches!(action, SlashAction::Up), window, cx);
            }
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

    fn handle_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_mention_active(cx) {
            self.handle_slash_action(SlashAction::Dismiss, window, cx);
            return;
        }
        if !matches!(self.view, WorkspaceView::NewSession { .. })
            && let Some(SessionLocation {
                project_index,
                agent_index,
            }) = self.view.displayed_session()
        {
            let agent = &self.projects[project_index].agents[agent_index];
            if agent.active_work && !agent.cancel_requested {
                self.cancel_prompt(cx);
                cx.stop_propagation();
                return;
            }
        }
        self.handle_slash_action(SlashAction::Dismiss, window, cx);
    }

    fn handle_prompt_recall(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.view.displayed_session() else {
            return;
        };
        let input = self.composer.read(cx);
        let current = input.value().to_string();
        let line = input.cursor_position().line as usize;
        if (up && line != 0) || (!up && line < current.matches('\n').count()) {
            return;
        }
        let agent = &self.projects[session.project_index].agents[session.agent_index];
        let Some(value) = PromptRecall::step(
            &mut self.prompt_recall,
            agent.config.id,
            &agent.config.prompt_history,
            &current,
            up,
        ) else {
            return;
        };
        let line = value.matches('\n').count() as u32;
        let column = value
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .encode_utf16()
            .count() as u32;
        self.composer.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.set_cursor_position(Position::new(line, column), window, cx);
        });
        cx.stop_propagation();
        cx.notify();
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if self.dirty {
            let _ = config::save(&self.config());
        }
    }
}

pub(crate) fn run() {
    gpui_ce_platform::application()
        .with_assets(Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            cx.bind_keys([KeyBinding::new(
                "ctrl-enter",
                gpui_component::input::Enter {
                    secondary: true,
                    shift: true,
                },
                Some("Input"),
            )]);
            #[cfg(target_os = "macos")]
            cx.bind_keys([KeyBinding::new("cmd-p", QuickOpen, None)]);
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
