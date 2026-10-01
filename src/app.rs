use crate::acp::{Connection, Event};
use crate::config::{
    AgentConfig, ChatEntry, ChatFile, ChatImage, Config, ProjectConfig, Prompt, Role, SlashCommand,
};
use crate::diff_view::{File as DiffFile, Presentation as DiffPresentation, Row as DiffRow};
use crate::discovery::{AgentChoice, installed_agents, score};
use crate::file_search::{FileSearch, scan_project};
use crate::folder_search::FolderSearch;
use crate::images::ImageStore;
use crate::theme::*;
use crate::{config, theme};
#[cfg(test)]
use agent_client_protocol_schema::ProtocolVersion;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName,
    button::{Button, ButtonVariants},
    input::{
        Enter, Escape, IndentInline, Input, InputEvent, InputState, MoveDown, MoveUp, Position,
        Textarea, TextareaState,
    },
    menu::{DropdownMenu, PopupMenuItem},
    scroll::ScrollableElement,
    text::{TextView, TextViewStyle},
    theme::Theme,
    tooltip::Tooltip,
};
use gpui_kit::{
    App, Bounds, Context, DragMoveEvent, Entity, Focusable, IntoElement, KeyBinding, KeyDownEvent,
    ListAlignment, ListState, MouseButton, PathPromptOptions, Render, StatefulInteractiveElement,
    Subscription, Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, relative,
    rems, rgb, size,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

mod agent;
mod attachments;
mod panes;
mod state;
use state::*;
mod assets;
mod elicitation;
mod mobile;
mod render;
mod sessions;
#[cfg(test)]
mod tests;
mod tool_activity;

use self::{agent::*, elicitation::*, tool_activity::*};
use crate::session::*;
use assets::AppAssets;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Notice {
    Info(String),
    Error(String),
}

impl Notice {
    fn message(&self) -> &str {
        match self {
            Self::Info(message) | Self::Error(message) => message,
        }
    }
}

enum DiffData {
    Full(
        Vec<DiffFile>,
        Vec<(usize, usize)>,
        DiffPresentation,
        Option<String>,
        Arc<Vec<DiffListRow>>,
    ),
    Counts(Option<(usize, usize)>),
}

type DiffLoadResult = Result<DiffData, String>;

#[derive(Clone)]
enum DiffListRow {
    File {
        path: String,
        note: Option<String>,
        added: usize,
        removed: usize,
        bar_width: f32,
        added_width: f32,
        expanded: bool,
    },
    Content(DiffRow),
}

enum SyncUpdate {
    Counts(PathBuf, Option<String>, Option<(usize, usize)>),
    OperationFinished {
        path: PathBuf,
        host: Option<String>,
        action: crate::git_sync::SyncAction,
        result: Result<(usize, usize), String>,
        counts: Option<(usize, usize)>,
    },
    Finished,
}

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

actions!(workspace, [QuickOpen, ZoomIn, ZoomOut, ZoomReset, Quit]);

// Font sizes of the gpui-component dark theme that theme::apply installs.
// Zoom scales the UI by editing these on the global Theme, because the
// gpui-component Root plugin resets window rem_size to theme.font_size
// before every frame, discarding direct window.set_rem_size calls.
const BASE_FONT_SIZE: f32 = 16.;
const BASE_MONO_FONT_SIZE: f32 = 13.;
const MIN_FONT_SCALE: f32 = 0.75;
const MAX_FONT_SCALE: f32 = 2.0;
const FONT_SCALE_STEP: f32 = 0.1;
const ZOOM_NOTICE_TIMEOUT: Duration = Duration::from_millis(1500);

pub(crate) fn set_theme_font_scale(theme: &mut Theme, scale: f32) {
    theme.font_size = px(BASE_FONT_SIZE * scale);
    theme.mono_font_size = px(BASE_MONO_FONT_SIZE * scale);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SessionLocation {
    project_index: usize,
    agent_index: usize,
}

fn session_search_score(
    query: &str,
    path: &Path,
    branch: &str,
    agent: &str,
    title: Option<&str>,
) -> Option<i32> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    [
        score(query, &name).map(|rank| rank + 30),
        score(query, agent).map(|rank| rank + 30),
        title
            .and_then(|title| score(query, title))
            .map(|rank| rank + 30),
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
    ChangeFolder { session: SessionLocation },
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
                step: PickerStep::ChangeFolder { session },
                ..
            } if session == archived => Self::NewSession {
                step: PickerStep::Folders,
                return_to: next,
            },
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
    ssh_host: Option<String>,
    branch: String,
    sync_counts: Option<(usize, usize)>,
    agents: Vec<AgentView>,
}

impl ProjectView {
    fn display_path(&self) -> String {
        self.ssh_host.as_ref().map_or_else(
            || self.path.display().to_string(),
            |host| crate::remote::project_label(host, &self.path),
        )
    }
}

#[derive(Clone)]
struct AgentDrag {
    id: u64,
    label: String,
    order_index: usize,
}

impl Render for AgentDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(palette.color(SELECTED))
            .text_sm()
            .text_color(palette.color(TEXT))
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
    workspace_focus: gpui_kit::FocusHandle,
    pane_focus: gpui_kit::FocusHandle,
    pane_area_bounds: std::rc::Rc<std::cell::Cell<Bounds<gpui_kit::Pixels>>>,
    pane_layout: crate::panes::Layout,
    pane_id: u64,
    next_pane_id: u64,
    inactive_panes: HashMap<u64, panes::PaneState>,
    pane_bounds: HashMap<u64, std::rc::Rc<std::cell::Cell<Bounds<gpui_kit::Pixels>>>>,
    projects: Vec<ProjectView>,
    view: WorkspaceView,
    sidebar_order: Vec<u64>,
    sidebar_fraction: f32,
    font_scale: f32,
    theme_choice: crate::appearance::Choice,
    session_composers: HashMap<u64, Entity<TextareaState>>,
    draft_images: HashMap<u64, Vec<ChatImage>>,
    draft_files: HashMap<u64, Vec<ChatFile>>,
    conversation: ConversationState,
    next_agent_id: u64,
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
    deferred_connections: Vec<u64>,
    deferred_connections_deadline: Option<Instant>,
    picker: PickerState,
    sidebar_selection: usize,
    sidebar_search: Entity<InputState>,
    mobile_access: MobileState,
    diff: DiffState,
    sync: SyncState,
    persistence: crate::persistence::Persistence,
    images: ImageStore,
    notice: Option<Notice>,
    zoom_notice_generation: u64,
    _subscriptions: Vec<Subscription>,
}

fn diff_list_rows(
    files: &[DiffFile],
    stats: &[(usize, usize)],
    path: Option<&str>,
    presentation: DiffPresentation,
) -> Vec<DiffListRow> {
    let max_changed = stats
        .iter()
        .map(|(added, removed)| added + removed)
        .max()
        .unwrap_or(1)
        .max(1);
    let mut rows = Vec::new();
    for (file, &(added, removed)) in files.iter().zip(stats) {
        let changed = added + removed;
        let bar_width = if changed == 0 {
            0.0
        } else {
            (changed as f32 / max_changed as f32 * 96.0).max(4.0)
        };
        let added_width = if changed == 0 {
            0.0
        } else {
            bar_width * added as f32 / changed as f32
        };
        let expanded = path == Some(file.path.as_str());
        rows.push(DiffListRow::File {
            path: file.path.clone(),
            note: file.note.clone(),
            added,
            removed,
            bar_width,
            added_width,
            expanded,
        });
        if expanded {
            rows.extend(
                crate::diff_view::flatten(std::slice::from_ref(file), presentation)
                    .into_iter()
                    .skip(1)
                    .map(DiffListRow::Content),
            );
        }
    }
    rows
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
    fn subscribe_composer(
        &mut self,
        composer: &Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscriptions.push(cx.subscribe_in(
            composer,
            window,
            |this, input, event: &InputEvent, window, cx| {
                let Some(pane_id) = this.composer_pane(input) else {
                    return;
                };
                if !matches!(
                    event,
                    InputEvent::Focus
                        | InputEvent::Change
                        | InputEvent::PressEnter {
                            secondary: false,
                            shift: false
                        }
                ) {
                    return;
                }
                this.activate_pane(pane_id, cx);
                match event {
                    InputEvent::Change => {
                        if this
                            .conversation
                            .prompt_recall
                            .as_ref()
                            .is_some_and(|recall| {
                                recall.displayed
                                    != this.conversation.composer.read(cx).value().as_ref()
                            })
                        {
                            this.conversation.prompt_recall = None;
                        }
                        this.conversation.slash_selection = 0;
                        this.conversation.slash_dismissed = false;
                        this.conversation.file_selection = 0;
                        this.conversation.file_dismissed = false;
                        this.update_file_query(cx);
                        cx.notify();
                    }
                    InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    } => this.send_prompt(window, cx),
                    _ => {}
                }
            },
        ));
    }

    fn set_view(&mut self, view: WorkspaceView, window: &mut Window, cx: &mut Context<Self>) {
        if let WorkspaceView::Conversation(session) = view {
            let agent_id = self.projects[session.project_index].agents[session.agent_index]
                .config
                .id;
            if let Some((id, _)) = self
                .saved_pane_layout()
                .root
                .panes()
                .into_iter()
                .find(|(id, session)| *id != self.pane_id && *session == Some(agent_id))
            {
                self.activate_pane(id, cx);
                return;
            }
        }
        self.set_pane_view(view, window, cx);
    }

    fn set_pane_view(&mut self, view: WorkspaceView, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_rename_session(true, cx);
        if let Some(session) = view.displayed_session() {
            let agent_id = self.projects[session.project_index].agents[session.agent_index]
                .config
                .id;
            if !self.session_composers.contains_key(&agent_id) {
                let composer =
                    if !self.session_composers.values().any(|composer| {
                        composer.entity_id() == self.conversation.composer.entity_id()
                    }) {
                        self.conversation.composer.clone()
                    } else {
                        let composer = ConversationState::new_composer(window, cx);
                        self.subscribe_composer(&composer, window, cx);
                        composer
                    };
                self.session_composers.insert(agent_id, composer);
            }
            let composer = self.session_composers[&agent_id].clone();
            if self.conversation.composer.entity_id() != composer.entity_id() {
                self.conversation.max_rows = 0;
            }
            self.conversation.composer = composer;
            if self.deferred_connections.contains(&agent_id) {
                self.connect(session.project_index, session.agent_index);
            }
        }
        if self.view.displayed_session() != view.displayed_session() {
            self.conversation.prompt_recall = None;
            // The shared diff remains open and follows the focused session.
            self.diff.selected_file = None;
            self.diff.error = None;
            self.diff.rows = Arc::new(Vec::new());
            self.diff.list = ListState::new(0, ListAlignment::Top, px(28.));
        }
        let project_index = view
            .displayed_session()
            .map(|session| session.project_index);
        if self
            .view
            .displayed_session()
            .map(|session| session.project_index)
            != project_index
        {
            self.diff.request_id += 1;
            self.diff.loading = false;
            self.diff.counts = None;
            self.diff.files.clear();
            self.diff.file_stats.clear();
        }
        if self.conversation.file_search_project != project_index {
            self.conversation.file_search_project = project_index;
            self.conversation.file_scan_generation += 1;
            let generation = self.conversation.file_scan_generation;
            self.conversation.file_selection = 0;
            self.conversation.file_dismissed = false;
            self.conversation.file_search = project_index.map(|index| {
                let project = &self.projects[index];
                let search = FileSearch::new();
                let injector = search.injector();
                let root = project.path.clone();
                let host = project.ssh_host.clone();
                let sender = self.conversation.file_scan_tx.clone();
                std::thread::spawn(move || {
                    if let Some(host) = host {
                        crate::file_search::scan_remote_project(&root, &host, &injector);
                    } else {
                        scan_project(&root, &injector);
                    }
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
        self.pane_layout.root = self.saved_pane_layout().root;
        self.persistence.dirty = true;
        if self.pane_id == self.pane_layout.focused {
            self.mark_displayed_agent_viewed();
        }
    }

    fn open_diff(&mut self, project_index: usize, cx: &mut Context<Self>) {
        if self.projects[project_index].ssh_host.is_some() {
            self.notice = Some(Notice::Info(
                "Diff review for SSH projects is not available yet".into(),
            ));
            cx.notify();
            return;
        }
        self.diff.visible = true;
        self.diff.selected_file = None;
        self.diff.files.clear();
        self.diff.file_stats.clear();
        self.diff.rows = Arc::new(Vec::new());
        self.diff.list = ListState::new(0, ListAlignment::Top, px(28.));
        self.refresh_diff(project_index, cx);
    }

    fn close_diff(&mut self) {
        self.diff.visible = false;
        self.diff.selected_file = None;
    }

    fn refresh_diff(&mut self, project_index: usize, cx: &mut Context<Self>) {
        self.diff.request_id += 1;
        let request_id = self.diff.request_id;
        let path = self.projects[project_index].path.clone();
        let presentation = self.diff.presentation;
        let selected_file = self.diff.selected_file.clone();
        let visible = self.diff.visible;
        let tx = self.diff.tx.clone();
        self.diff.loading = true;
        self.diff.error = None;
        std::thread::spawn(move || {
            let result = if visible {
                crate::git_diff::load(&path).map(|files| {
                    let stats: Vec<_> = files.iter().map(crate::diff_view::stats).collect();
                    let rows = Arc::new(diff_list_rows(
                        &files,
                        &stats,
                        selected_file.as_deref(),
                        presentation,
                    ));
                    DiffData::Full(files, stats, presentation, selected_file, rows)
                })
            } else {
                crate::git_diff::line_counts(&path).map(DiffData::Counts)
            };
            let _ = tx.send((request_id, result));
        });
        cx.notify();
    }

    fn set_diff_presentation(
        &mut self,
        presentation: crate::diff_view::Presentation,
        cx: &mut Context<Self>,
    ) {
        let scroll_top = self.diff.list.logical_scroll_top();
        self.diff.presentation = presentation;
        self.diff.rows = Arc::new(diff_list_rows(
            &self.diff.files,
            &self.diff.file_stats,
            self.diff.selected_file.as_deref(),
            presentation,
        ));
        self.diff.list = ListState::new(self.diff.rows.len(), ListAlignment::Top, px(28.));
        self.diff.list.scroll_to(scroll_top);
        cx.notify();
    }

    fn select_diff_file(&mut self, path: String, cx: &mut Context<Self>) {
        let clicked_path = path.clone();
        let old_scroll_top = self.diff.list.logical_scroll_top();
        let old_file_index = self.diff.rows.iter().position(|row| {
            matches!(row, DiffListRow::File { path: row_path, .. } if row_path == &clicked_path)
        });
        self.diff.selected_file =
            (self.diff.selected_file.as_deref() != Some(path.as_str())).then_some(path);
        self.set_diff_presentation(self.diff.presentation, cx);
        if let Some(old_file_index) = old_file_index
            && let Some(new_file_index) = self.diff.rows.iter().position(|row| {
                matches!(row, DiffListRow::File { path: row_path, .. } if row_path == &clicked_path)
            })
        {
            let distance = old_file_index.saturating_sub(old_scroll_top.item_ix);
            self.diff.list.scroll_to(gpui_kit::ListOffset {
                item_ix: new_file_index.saturating_sub(distance),
                offset_in_item: old_scroll_top.offset_in_item,
            });
        }
    }

    fn mark_displayed_agent_viewed(&mut self) {
        if let Some(session) = self.view.displayed_session() {
            self.projects[session.project_index].agents[session.agent_index].mark_viewed();
        }
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let picker_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search recent folders or enter a local or SSH path…")
        });
        let sidebar_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search sessions…"));
        let mobile_provider_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("keyring, onepassword, or provider URI")
        });
        let composer = ConversationState::new_composer(window, cx);
        let _subscriptions = vec![
            cx.subscribe_in(
                &mobile_provider_input,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if matches!(
                        event,
                        InputEvent::PressEnter {
                            secondary: false,
                            shift: false
                        }
                    ) {
                        this.use_mobile_provider(cx);
                    }
                },
            ),
            cx.subscribe_in(
                &sidebar_search,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.sidebar_selection = 0;
                        this.select_sidebar_session(window, cx);
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
        ];
        let (events_tx, events_rx) = mpsc::channel();
        let (diff_tx, diff_rx) = mpsc::channel();
        let (diff_watch_tx, diff_watch_rx) = mpsc::channel();
        let (diff_watcher_tx, diff_watcher_rx) = mpsc::channel();
        let (sync_tx, sync_rx) = mpsc::channel();
        let (mobile_start_tx, mobile_start_rx) = mpsc::channel();
        let (config, migrate_config, notice) = match config::load() {
            Ok((config, migrate)) => (config, migrate, None),
            Err(error) => (
                Config::default(),
                false,
                Some(Notice::Error(format!("Could not load config: {error}"))),
            ),
        };
        let images = ImageStore::open();
        images.remove_unused(&config);
        let next_agent_id = config
            .projects
            .iter()
            .flat_map(|project| &project.agents)
            .map(|agent| agent.id)
            .max()
            .unwrap_or(0)
            + 1;
        let saved_layout = config.pane_layout;
        let mut sidebar_order = config.sidebar_order;
        let sidebar_fraction = if config.sidebar_fraction.is_finite() {
            config.sidebar_fraction.clamp(0.1, 0.7)
        } else {
            0.2
        };
        let font_scale = if config.font_scale.is_finite() {
            config.font_scale.clamp(MIN_FONT_SCALE, MAX_FONT_SCALE)
        } else {
            1.0
        };
        let theme_choice = config.theme;
        if let Err(error) = crate::appearance::select(theme_choice, font_scale, cx) {
            eprintln!("could not apply theme: {error:#}");
        }
        let (composer_viewport_height, composer_base_line_height) =
            ConversationState::composer_geometry(window);
        let composer_max_rows = ConversationState::composer_max_rows(
            composer_viewport_height,
            composer_base_line_height,
            font_scale,
        );
        composer.update(cx, |input, cx| {
            input.set_auto_grow(1, composer_max_rows, cx)
        });
        let projects: Vec<ProjectView> = config
            .projects
            .into_iter()
            .map(|project| ProjectView {
                branch: project
                    .ssh_host
                    .clone()
                    .unwrap_or_else(|| branch(&project.path)),
                sync_counts: None,
                path: project.path,
                ssh_host: project.ssh_host,
                agents: project
                    .agents
                    .into_iter()
                    .map(|agent| AgentView::new(agent, images.clone()))
                    .collect(),
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
            .map(|project| PathBuf::from(project.display_path()))
            .collect();
        if let Ok(cwd) = std::env::current_dir().and_then(|path| path.canonicalize())
            && !recent_folders.contains(&cwd)
        {
            recent_folders.insert(0, cwd);
        }
        let folder_search = FolderSearch::new(recent_folders);
        let mut this = Self {
            workspace_focus: cx.focus_handle(),
            pane_focus: cx.focus_handle(),
            pane_area_bounds: Default::default(),
            pane_layout: crate::panes::Layout::default(),
            pane_id: 1,
            next_pane_id: 2,
            inactive_panes: HashMap::new(),
            pane_bounds: HashMap::new(),
            projects,
            view: WorkspaceView::Empty,
            sidebar_order,
            sidebar_fraction,
            font_scale,
            theme_choice,
            session_composers: HashMap::new(),
            draft_images: HashMap::new(),
            draft_files: HashMap::new(),
            conversation: ConversationState::new(composer, window, font_scale),
            next_agent_id,
            events_tx,
            events_rx,
            deferred_connections: Vec::new(),
            deferred_connections_deadline: None,
            picker: PickerState {
                folder_search,
                folder_dialog_open: false,
                available_agents: installed_agents(),
                selection: 0,
                input: picker_input,
            },
            sidebar_selection: 0,
            sidebar_search,
            mobile_access: MobileState {
                provider_input: mobile_provider_input,
                server: None,
                start_tx: mobile_start_tx,
                start_rx: mobile_start_rx,
                loading: false,
                endpoint_id: None,
                pairing_visible: false,
                revoke_confirm: None,
                provider_prompt: None,
                qr: None,
                last_snapshot: None,
            },
            diff: DiffState {
                visible: false,
                selected_file: None,
                presentation: crate::diff_view::Presentation::Unified,
                rows: Arc::new(Vec::new()),
                list: ListState::new(0, ListAlignment::Top, px(28.)),
                tx: diff_tx,
                rx: diff_rx,
                request_id: 0,
                watch_tx: diff_watch_tx,
                watch_rx: diff_watch_rx,
                watcher_tx: diff_watcher_tx,
                watcher_rx: diff_watcher_rx,
                watch_generation: 0,
                watched_project: None,
                watcher: None,
                refresh_due: None,
                first_change_at: None,
                poll_at: None,
                loading: false,
                error: None,
                counts: None,
                files: Vec::new(),
                file_stats: Vec::new(),
            },
            sync: SyncState {
                tx: sync_tx,
                rx: sync_rx,
                loading: false,
                in_progress: HashSet::new(),
                refresh_pending: HashSet::new(),
                last_remote_sync: Instant::now() - Duration::from_secs(30),
                last_upstream_fetch: Instant::now() - Duration::from_secs(300),
            },
            persistence: crate::persistence::Persistence::new(migrate_config),
            images,
            notice,
            zoom_notice_generation: 0,
            _subscriptions,
        };
        #[cfg(not(target_os = "linux"))]
        this._subscriptions
            .push(cx.observe_window_appearance(window, |_, window, cx| {
                Theme::sync_system_appearance(Some(window), cx);
                crate::theming::set_input_background(Theme::global(cx).input_background(), cx);
                crate::appearance::system_changed(cx);
            }));
        let picker_input = this.picker.input.clone();
        this.subscribe_picker(&picker_input, window, cx);
        let composer = this.conversation.composer.clone();
        this.subscribe_composer(&composer, window, cx);
        this._subscriptions
            .push(cx.observe_window_bounds(window, |this, window, cx| {
                (
                    this.conversation.viewport_height,
                    this.conversation.base_line_height,
                ) = ConversationState::composer_geometry(window);
                this.conversation.resize_composers(this.font_scale, cx);
            }));
        let mut selected = None;
        for project_index in 0..this.projects.len() {
            for agent_index in 0..this.projects[project_index].agents.len() {
                if this.projects[project_index].agents[agent_index]
                    .config
                    .archived
                {
                    continue;
                }
                this.deferred_connections
                    .push(this.projects[project_index].agents[agent_index].config.id);
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
            this.set_view(WorkspaceView::Conversation(session), window, cx);
            if !this.deferred_connections.is_empty() {
                this.deferred_connections_deadline = Some(Instant::now() + Duration::from_secs(2));
            }
            this.conversation
                .composer
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
            this.set_view(view, window, cx);
            if matches!(this.view, WorkspaceView::NewSession { .. }) {
                this.picker
                    .input
                    .update(cx, |input, cx| input.focus(window, cx));
            }
        }
        this.restore_panes(saved_layout, window, cx);
        this.refresh_branches(cx);
        let background_executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let mut tick_interval = Duration::from_millis(25);
            let mut last_branch_refresh = Instant::now();
            let mut fast_poll_session = None;
            let mut fast_poll_started = Instant::now();
            loop {
                background_executor.timer(tick_interval).await;
                let Ok(connecting_session) = this.update_in(cx, |this, window, cx| {
                    this.poll_events(window, cx);
                    this.poll_sync_counts(cx);
                    this.tick_panes(cx);
                    if last_branch_refresh.elapsed() >= Duration::from_secs(5) {
                        last_branch_refresh = Instant::now();
                        this.refresh_branches(cx);
                    }
                    this.view.displayed_session().and_then(|session| {
                        let agent =
                            &this.projects[session.project_index].agents[session.agent_index];
                        (agent.status == Status::Connecting).then_some(agent.config.id)
                    })
                }) else {
                    break;
                };
                if fast_poll_session != connecting_session {
                    fast_poll_session = connecting_session;
                    fast_poll_started = Instant::now();
                }
                tick_interval = if connecting_session.is_some()
                    && fast_poll_started.elapsed() < Duration::from_secs(10)
                {
                    Duration::from_millis(25)
                } else {
                    Duration::from_millis(100)
                };
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
                    ssh_host: project.ssh_host.clone(),
                    agents: project.agents.iter().map(AgentView::snapshot).collect(),
                })
                .collect(),
            sidebar_order: self.sidebar_order.clone(),
            sidebar_fraction: self.sidebar_fraction,
            font_scale: self.font_scale,
            theme: self.theme_choice,
            pane_layout: Some(self.saved_pane_layout()),
        }
    }

    fn persist(&mut self) {
        self.persistence.submit(self.config());
    }

    fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        cx.notify();
    }

    fn open_picker(&mut self, step: PickerStep, window: &mut Window, cx: &mut Context<Self>) {
        self.set_view(self.view.open_picker(step), window, cx);
        self.picker.open(step, window, cx);
        cx.notify();
    }

    fn open_change_folder(
        &mut self,
        session: SessionLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let agent = &self.projects[session.project_index].agents[session.agent_index];
        if agent.active_work
            || agent.config.was_working
            || agent.config.active_prompt.is_some()
            || !agent.config.pending_prompts.is_empty()
        {
            self.notice = Some(Notice::Info(
                "Wait for the agent and queued prompts to finish before changing folders".into(),
            ));
            cx.notify();
            return;
        }
        self.open_picker(PickerStep::ChangeFolder { session }, window, cx);
    }

    fn back_from_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.view {
            WorkspaceView::NewSession {
                step: PickerStep::Agents { .. },
                ..
            } => self.open_picker(PickerStep::Folders, window, cx),
            WorkspaceView::NewSession {
                step: PickerStep::Folders | PickerStep::ChangeFolder { .. },
                return_to: Some(session),
            } => {
                self.set_view(WorkspaceView::Conversation(session), window, cx);
                self.conversation
                    .composer
                    .update(cx, |input, cx| input.focus(window, cx));
                cx.notify();
            }
            _ => {}
        }
    }

    fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.folder_dialog_open {
            return;
        }
        self.picker.folder_dialog_open = true;
        let pane_id = self.pane_id;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = selection.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.activate_pane(pane_id, cx) {
                    return;
                }
                this.picker.folder_dialog_open = false;
                if !matches!(
                    this.view,
                    WorkspaceView::NewSession {
                        step: PickerStep::Folders | PickerStep::ChangeFolder { .. },
                        ..
                    }
                ) {
                    return;
                }
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.select_folder(path, window, cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        this.notice = Some(Notice::Error(format!(
                            "Could not open folder chooser: {error}"
                        )));
                        cx.notify();
                    }
                    Err(error) => {
                        this.notice = Some(Notice::Error(format!(
                            "Folder chooser closed unexpectedly: {error}"
                        )));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
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
                            agent.config.session_title(),
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
            self.set_view(WorkspaceView::Conversation(location), window, cx);
            self.conversation
                .composer
                .update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
        }
    }

    fn select_sidebar_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            self.set_view(WorkspaceView::Conversation(location), window, cx);
        }
    }

    fn agent_results(&self, cx: &Context<Self>) -> Vec<AgentChoice> {
        let query = self.picker.input.read(cx).value().to_string();
        let mut agents = self
            .picker
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
        if let WorkspaceView::NewSession {
            step: PickerStep::ChangeFolder { session },
            ..
        } = self.view
        {
            let agent = &self.projects[session.project_index].agents[session.agent_index];
            if agent.active_work
                || agent.config.was_working
                || agent.config.active_prompt.is_some()
                || !agent.config.pending_prompts.is_empty()
            {
                self.notice = Some(Notice::Info(
                    "Wait for the agent and queued prompts to finish before changing folders"
                        .into(),
                ));
                cx.notify();
                return;
            }
        }
        let (path, ssh_host) = match crate::remote::parse_project(&path.to_string_lossy()) {
            Ok(Some(remote)) => (remote.path, Some(remote.host)),
            Ok(None) => match path.canonicalize() {
                Ok(path) if path.is_dir() => (path, None),
                _ => {
                    self.notice = Some(Notice::Error("Choose an existing folder".into()));
                    cx.notify();
                    return;
                }
            },
            Err(error) => {
                self.notice = Some(Notice::Error(error));
                cx.notify();
                return;
            }
        };
        let project_index = if let Some(index) = self
            .projects
            .iter()
            .position(|project| project.path == path && project.ssh_host == ssh_host)
        {
            index
        } else {
            self.projects.push(ProjectView {
                branch: ssh_host.clone().unwrap_or_else(|| branch(&path)),
                sync_counts: None,
                path: path.clone(),
                ssh_host: ssh_host.clone(),
                agents: Vec::new(),
            });
            self.picker.folder_search.add_recent(match &ssh_host {
                Some(host) => PathBuf::from(crate::remote::project_label(host, &path)),
                None => path,
            });
            self.persist();
            self.projects.len() - 1
        };
        self.notice = None;
        if let WorkspaceView::NewSession {
            step: PickerStep::ChangeFolder { session },
            ..
        } = self.view
        {
            self.move_session_to_project(session, project_index, window, cx);
        } else {
            self.open_picker(PickerStep::Agents { project_index }, window, cx);
        }
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
            self.notice = Some(Notice::Info("Choose a project first".into()));
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
        let (_, agent_index) = self.create_agent_for_project(project_index, command, name, cx);
        self.set_view(
            WorkspaceView::Conversation(SessionLocation {
                project_index,
                agent_index,
            }),
            window,
            cx,
        );
        self.conversation
            .composer
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn create_agent_for_project(
        &mut self,
        project_index: usize,
        command: Vec<String>,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) -> (u64, usize) {
        let id = self.next_agent_id;
        let config = AgentConfig {
            id,
            command,
            archived: false,
            display_name: name,
            title: None,
            custom_title: None,
            session_id: None,
            model: None,
            context: None,
            messages: Vec::new(),
            available_commands: Vec::new(),
            pending_prompts: Vec::new(),
            active_prompt: None,
            prompt_history: Vec::new(),
            was_working: false,
            session_has_activity: false,
            fork_pending: false,
            fork_source: None,
        };
        self.sidebar_order.push(config.id);
        self.next_agent_id += 1;
        let agent_index = self.projects[project_index].agents.len();
        self.projects[project_index]
            .agents
            .push(AgentView::new(config, self.images.clone()));
        self.connect(project_index, agent_index);
        if !self.deferred_connections.is_empty() {
            self.deferred_connections_deadline = Some(Instant::now() + Duration::from_secs(2));
        }
        self.notice = None;
        self.persist();
        cx.notify();
        (id, agent_index)
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
                self.set_view(self.view.session_archived(location, next), window, cx);
            }
        } else {
            if self.projects[project_index].agents[agent_index]
                .connection
                .is_none()
            {
                self.connect(project_index, agent_index);
            }
            self.set_view(
                WorkspaceView::Conversation(SessionLocation {
                    project_index,
                    agent_index,
                }),
                window,
                cx,
            );
            self.conversation
                .composer
                .update(cx, |input, cx| input.focus(window, cx));
        }
        self.persist();
        cx.notify();
    }

    fn confirm_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.view {
            WorkspaceView::NewSession {
                step: PickerStep::Folders | PickerStep::ChangeFolder { .. },
                ..
            } => {
                if let Some(path) = self
                    .picker
                    .folder_search
                    .results()
                    .get(self.picker.selection)
                    .cloned()
                {
                    self.select_folder(path, window, cx);
                } else {
                    self.notice = Some(Notice::Error(
                        "Enter a local absolute path or ssh://host/absolute/path.".into(),
                    ));
                    cx.notify();
                }
            }
            WorkspaceView::NewSession {
                step: PickerStep::Agents { .. },
                ..
            } => {
                if let Some(agent) = self.agent_results(cx).get(self.picker.selection).cloned() {
                    self.start_agent(agent.command, Some(agent.name), window, cx);
                } else {
                    self.start_custom_agent(window, cx);
                }
            }
            _ => {}
        }
    }

    fn start_custom_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.picker.input.read(cx).value().to_string();
        match shell_words::split(value.trim()) {
            Ok(command) if !command.is_empty() => self.start_agent(command, None, window, cx),
            _ => {
                self.notice = Some(Notice::Error(
                    "Enter an ACP executable and its arguments".into(),
                ));
                cx.notify();
            }
        }
    }

    fn quick_open(&mut self, _: &QuickOpen, window: &mut Window, cx: &mut Context<Self>) {
        self.open_picker(PickerStep::Folders, window, cx);
    }

    fn zoom_in(&mut self, _: &ZoomIn, _window: &mut Window, cx: &mut Context<Self>) {
        self.set_font_scale(self.font_scale + FONT_SCALE_STEP, cx);
    }

    fn zoom_out(&mut self, _: &ZoomOut, _window: &mut Window, cx: &mut Context<Self>) {
        self.set_font_scale(self.font_scale - FONT_SCALE_STEP, cx);
    }

    fn zoom_reset(&mut self, _: &ZoomReset, _window: &mut Window, cx: &mut Context<Self>) {
        self.set_font_scale(1.0, cx);
    }

    fn select_theme(&mut self, choice: crate::appearance::Choice, cx: &mut Context<Self>) {
        match crate::appearance::select(choice, self.font_scale, cx) {
            Ok(()) => {
                self.theme_choice = choice;
                // Font and background changes can alter Markdown layout.
                let scroll_top = self.conversation.chat_list.logical_scroll_top();
                self.conversation
                    .chat_list
                    .reset(self.conversation.chat_rows.len());
                self.conversation.chat_list.scroll_to(scroll_top);
                self.persist();
            }
            Err(error) => {
                self.notice = Some(Notice::Error(format!("Could not apply theme: {error:#}")))
            }
        }
        cx.notify();
    }

    fn set_font_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        let scale = ((scale * 10.).round() / 10.).clamp(MIN_FONT_SCALE, MAX_FONT_SCALE);
        if scale == self.font_scale {
            return;
        }
        self.font_scale = scale;
        crate::appearance::set_scale(scale, cx);
        let acknowledgement = format!("Zoom {:.0}%", scale * 100.);
        self.notice = Some(Notice::Info(acknowledgement.clone()));
        self.persist();
        // Theme::update also refreshes every window, so the next frame lays
        // text out at the new sizes.
        Theme::update(cx, |theme| set_theme_font_scale(theme, scale));
        self.conversation.resize_composers(self.font_scale, cx);
        self.zoom_notice_generation += 1;
        let generation = self.zoom_notice_generation;
        if self.notice.as_ref().map(Notice::message) == Some(acknowledgement.as_str()) {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ZOOM_NOTICE_TIMEOUT).await;
                this.update(cx, |this, cx| {
                    if this.zoom_notice_generation == generation
                        && this.notice.as_ref().map(Notice::message)
                            == Some(acknowledgement.as_str())
                    {
                        this.notice = None;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn workspace_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_keyboard_pane(window, cx);
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
                        self.select_sidebar_session(window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "up" if count > 0 => {
                        self.sidebar_selection = (self.sidebar_selection + count - 1) % count;
                        self.select_sidebar_session(window, cx);
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
            PickerStep::Folders | PickerStep::ChangeFolder { .. } => {
                self.picker.folder_search.results().len()
            }
            PickerStep::Agents { .. } => {
                self.agent_results(cx).len()
                    + usize::from(!self.picker.input.read(cx).value().trim().is_empty())
            }
        };
        match event.keystroke.key.as_str() {
            "down" if count > 0 => {
                self.picker.selection = (self.picker.selection + 1) % count;
                cx.stop_propagation();
                cx.notify();
            }
            "up" if count > 0 => {
                self.picker.selection = (self.picker.selection + count - 1) % count;
                cx.stop_propagation();
                cx.notify();
            }
            "escape" => {
                if self.picker.input.read(cx).value().is_empty() {
                    self.back_from_picker(window, cx);
                } else {
                    self.picker
                        .input
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn slash_results(&self, cx: &Context<Self>) -> Vec<SlashCommand> {
        if self.conversation.slash_dismissed {
            return Vec::new();
        }
        let Some(SessionLocation {
            project_index,
            agent_index,
        }) = self.view.displayed_session()
        else {
            return Vec::new();
        };
        let draft = self.conversation.composer.read(cx).value().to_string();
        matching_slash_commands(
            &self.projects[project_index].agents[agent_index]
                .config
                .available_commands,
            &draft,
        )
    }

    fn update_file_query(&mut self, cx: &Context<Self>) {
        let input = self.conversation.composer.read(cx);
        let draft = input.value().to_string();
        let query = file_mention(&draft, input.cursor_position()).map(|(_, query)| query);
        if let (Some(search), Some(query)) = (self.conversation.file_search.as_mut(), query) {
            search.set_query(query);
        }
    }

    fn file_results(&self, cx: &Context<Self>) -> Vec<String> {
        if self.conversation.file_dismissed || self.view.displayed_session().is_none() {
            return Vec::new();
        }
        let input = self.conversation.composer.read(cx);
        let draft = input.value().to_string();
        let Some((_, query)) = file_mention(&draft, input.cursor_position()) else {
            return Vec::new();
        };
        self.conversation
            .file_search
            .as_ref()
            .filter(|search| search.query() == query)
            .map_or_else(Vec::new, |search| search.results().to_vec())
    }

    fn file_mention_active(&self, cx: &Context<Self>) -> bool {
        if self.conversation.file_dismissed || self.view.displayed_session().is_none() {
            return false;
        }
        let input = self.conversation.composer.read(cx);
        file_mention(&input.value(), input.cursor_position()).is_some()
    }

    fn complete_file(&mut self, file: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.conversation.composer.read(cx);
        let draft = input.value().to_string();
        let Some((range, _)) = file_mention(&draft, input.cursor_position()) else {
            return;
        };
        let (value, cursor) = completed_file_text(&draft, range, file);
        self.conversation.composer.update(cx, |input, cx| {
            input.replace_all(value, window, cx);
            input.set_cursor_position(cursor, window, cx);
        });
        self.conversation.file_dismissed = true;
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
        self.conversation.composer.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.set_cursor_position(Position::new(0, column), window, cx);
        });
        self.conversation.slash_selection = 0;
        self.conversation.slash_dismissed = true;
        cx.notify();
    }

    fn handle_slash_action(
        &mut self,
        action: SlashAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_keyboard_pane(window, cx);
        if !self
            .conversation
            .composer
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
        {
            return;
        }
        let files = self.file_results(cx);
        if !files.is_empty() {
            match action {
                SlashAction::Down => {
                    self.conversation.file_selection =
                        (self.conversation.file_selection + 1) % files.len()
                }
                SlashAction::Up => {
                    self.conversation.file_selection =
                        (self.conversation.file_selection + files.len() - 1) % files.len()
                }
                SlashAction::Complete => {
                    let index = self.conversation.file_selection.min(files.len() - 1);
                    self.complete_file(&files[index], window, cx);
                }
                SlashAction::Dismiss => self.conversation.file_dismissed = true,
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
                self.conversation.file_dismissed = true;
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
                self.conversation.slash_selection =
                    (self.conversation.slash_selection + 1) % commands.len();
            }
            SlashAction::Up => {
                self.conversation.slash_selection =
                    (self.conversation.slash_selection + commands.len() - 1) % commands.len();
            }
            SlashAction::Complete => {
                let index = self.conversation.slash_selection.min(commands.len() - 1);
                self.complete_slash_command(commands[index].clone(), window, cx);
            }
            SlashAction::Dismiss => {
                self.conversation.slash_dismissed = true;
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn handle_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.activate_keyboard_pane(window, cx);
        if self.conversation.renaming.is_some() {
            self.finish_rename_session(false, cx);
            self.conversation
                .composer
                .update(cx, |input, cx| input.focus(window, cx));
            cx.stop_propagation();
            return;
        }
        if self.mobile_access.provider_prompt.take().is_some() {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.mobile_access.pairing_visible {
            self.mobile_access.pairing_visible = false;
            cx.stop_propagation();
            cx.notify();
            return;
        }
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
        let input = self.conversation.composer.read(cx);
        let current = input.value().to_string();
        let line = input.cursor_position().line as usize;
        if (up && line != 0) || (!up && line < current.matches('\n').count()) {
            return;
        }
        let agent = &self.projects[session.project_index].agents[session.agent_index];
        let Some(value) = PromptRecall::step(
            &mut self.conversation.prompt_recall,
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
        self.conversation.composer.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.set_cursor_position(Position::new(line, column), window, cx);
        });
        cx.stop_propagation();
        cx.notify();
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if self.persistence.dirty {
            self.persistence.submit(self.config());
        }
    }
}

pub(crate) fn run() {
    gpui_kit::application()
        .with_assets(AppAssets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            cx.bind_keys([KeyBinding::new(
                "ctrl-enter",
                gpui_kit::component::input::Enter {
                    secondary: true,
                    shift: true,
                },
                Some("Input"),
            )]);
            #[cfg(target_os = "macos")]
            {
                cx.bind_keys([
                    KeyBinding::new("cmd-p", QuickOpen, None),
                    KeyBinding::new("cmd-=", ZoomIn, None),
                    KeyBinding::new("cmd-+", ZoomIn, None),
                    KeyBinding::new("cmd--", ZoomOut, None),
                    KeyBinding::new("cmd-0", ZoomReset, None),
                    KeyBinding::new("cmd-q", Quit, None),
                ]);
                cx.on_action(|_: &Quit, cx| cx.quit());
                cx.set_menus(vec![
                    gpui_kit::Menu::new("Agentaps")
                        .items([gpui_kit::MenuItem::action("Quit Agentaps", Quit)]),
                    gpui_kit::Menu::new("View").items([
                        gpui_kit::MenuItem::action("Zoom In", ZoomIn),
                        gpui_kit::MenuItem::action("Zoom Out", ZoomOut),
                        gpui_kit::MenuItem::separator(),
                        gpui_kit::MenuItem::action("Reset Zoom", ZoomReset),
                    ]),
                ]);
            }
            #[cfg(not(target_os = "macos"))]
            cx.bind_keys([
                KeyBinding::new("ctrl-p", QuickOpen, None),
                KeyBinding::new("ctrl-=", ZoomIn, None),
                KeyBinding::new("ctrl-+", ZoomIn, None),
                KeyBinding::new("ctrl--", ZoomOut, None),
                KeyBinding::new("ctrl-0", ZoomReset, None),
            ]);
            crate::theming::initialize(cx);
            crate::appearance::initialize(cx);
            let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
            let window_handle = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("Agentaps");
                    cx.new(|cx| Workspace::new(window, cx))
                },
            )
            .expect("Could not open GPUI window");
            // App-level handlers so View menu items and shortcuts reach the
            // workspace regardless of which element holds focus.
            let (_, workspace) = window_handle;
            let workspace_out = workspace.clone();
            let workspace_reset = workspace.clone();
            cx.on_action(move |_: &ZoomIn, cx| {
                workspace.update(cx, |this, cx| {
                    this.set_font_scale(this.font_scale + FONT_SCALE_STEP, cx);
                });
            });
            cx.on_action(move |_: &ZoomOut, cx| {
                workspace_out.update(cx, |this, cx| {
                    this.set_font_scale(this.font_scale - FONT_SCALE_STEP, cx);
                });
            });
            cx.on_action(move |_: &ZoomReset, cx| {
                workspace_reset.update(cx, |this, cx| {
                    this.set_font_scale(1.0, cx);
                });
            });
            cx.activate(true);
        });
}
