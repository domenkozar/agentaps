//! Pane-local presentation state. The focused pane occupies the workspace's
//! existing view slots; inactive panes retain the same complete state.
use super::*;
use crate::panes::{Direction, Layout, Node};

type PaneListener<E> = Box<dyn Fn(&E, &mut Window, &mut App)>;

pub(super) struct PaneState {
    pub(super) focus: gpui_kit::FocusHandle,
    view: WorkspaceView,
    conversation: ConversationState,
    picker: PickerState,
}

impl Workspace {
    fn swap_pane(&mut self, id: u64) -> bool {
        if self.pane_id == id {
            return true;
        }
        let Some(mut state) = self.inactive_panes.remove(&id) else {
            return false;
        };
        std::mem::swap(&mut self.pane_focus, &mut state.focus);
        std::mem::swap(&mut self.view, &mut state.view);
        std::mem::swap(&mut self.conversation, &mut state.conversation);
        std::mem::swap(&mut self.picker, &mut state.picker);
        self.inactive_panes.insert(self.pane_id, state);
        self.pane_id = id;
        true
    }

    /// Lazy list rendering needs the originating pane without changing focus.
    pub(super) fn with_pane<R>(
        &mut self,
        id: u64,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Context<Self>) -> R,
    ) -> Option<R> {
        let previous = self.pane_id;
        if !self.swap_pane(id) {
            return None;
        }
        let result = f(self, cx);
        self.swap_pane(previous);
        Some(result)
    }

    pub(super) fn activate_pane(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        if !self.swap_pane(id) {
            return false;
        }
        if self.pane_layout.focused != id {
            self.pane_layout.focused = id;
            self.persistence.dirty = true;
            self.diff.request_id += 1;
            self.diff.loading = false;
            self.diff.counts = None;
            self.diff.files.clear();
            self.diff.file_stats.clear();
            self.diff.error = None;
            self.diff.selected_file = None;
            self.diff.rows = Arc::new(Vec::new());
            self.diff.list = ListState::new(0, ListAlignment::Top, px(28.));
            if let Some(session) = self.view.displayed_session() {
                self.refresh_diff(session.project_index, cx);
            }
            self.mark_displayed_agent_viewed();
            cx.notify();
        }
        true
    }

    pub(super) fn activate_keyboard_pane(&mut self, window: &Window, cx: &mut Context<Self>) {
        let id = self
            .inactive_panes
            .iter()
            .find_map(|(&id, state)| state.focus.contains_focused(window, cx).then_some(id));
        if let Some(id) = id {
            self.activate_pane(id, cx);
        }
    }

    pub(super) fn pane_listener<
        E: ?Sized,
        F: Fn(&mut Self, &E, &mut Window, &mut Context<Self>) + 'static,
    >(
        &self,
        cx: &Context<Self>,
        listener: F,
    ) -> PaneListener<E> {
        let pane_id = self.pane_id;
        let session = self.view.return_to().map(|location| {
            self.projects[location.project_index].agents[location.agent_index]
                .config
                .id
        });
        Box::new(cx.listener(move |this, event, window, cx| {
            if this.activate_pane(pane_id, cx) {
                let current = this.view.return_to().map(|location| {
                    this.projects[location.project_index].agents[location.agent_index]
                        .config
                        .id
                });
                if current == session {
                    listener(this, event, window, cx);
                }
            }
        }))
    }

    pub(super) fn activate_session_pane(
        &mut self,
        pane_id: u64,
        session_id: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.activate_pane(pane_id, cx) {
            return false;
        }
        self.view.displayed_session().is_some_and(|location| {
            self.projects[location.project_index].agents[location.agent_index]
                .config
                .id
                == session_id
        })
    }

    pub(super) fn session_location(&self, id: u64) -> Option<SessionLocation> {
        self.projects
            .iter()
            .enumerate()
            .find_map(|(project_index, project)| {
                project
                    .agents
                    .iter()
                    .position(|agent| agent.config.id == id && !agent.config.archived)
                    .map(|agent_index| SessionLocation {
                        project_index,
                        agent_index,
                    })
            })
    }

    pub(super) fn saved_pane_layout(&self) -> Layout {
        let mut layout = self.pane_layout.clone();
        for (id, _) in layout.root.panes() {
            let view = if id == self.pane_id {
                self.view
            } else {
                self.inactive_panes[&id].view
            };
            let session = view.return_to().and_then(|location| {
                self.projects
                    .get(location.project_index)
                    .and_then(|project| project.agents.get(location.agent_index))
                    .map(|agent| agent.config.id)
            });
            if let Some(Node::Pane { session: saved, .. }) = layout.root.find_mut(id) {
                *saved = session;
            }
        }
        layout
    }

    fn add_pane_state(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let composer = ConversationState::new_composer(window, cx);
        self.subscribe_composer(&composer, window, cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search recent folders or enter a local or SSH path…")
        });
        self.subscribe_picker(&input, window, cx);
        let folders = self
            .projects
            .iter()
            .map(|project| PathBuf::from(project.display_path()))
            .collect();
        self.inactive_panes.insert(
            id,
            PaneState {
                focus: cx.focus_handle(),
                view: WorkspaceView::Empty,
                conversation: ConversationState::new(composer, window, self.font_scale),
                picker: PickerState {
                    folder_search: FolderSearch::new(folders),
                    folder_dialog_open: false,
                    available_agents: self.picker.available_agents.clone(),
                    selection: 0,
                    input,
                },
            },
        );
    }

    pub(super) fn restore_panes(
        &mut self,
        saved: Option<Layout>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut layout) = saved else {
            return;
        };
        let valid = self
            .projects
            .iter()
            .flat_map(|project| &project.agents)
            .filter(|agent| !agent.config.archived)
            .map(|agent| agent.config.id)
            .collect::<Vec<_>>();
        self.next_pane_id = layout.normalize(&valid);
        let panes = layout.root.panes();
        // Transfer the initial presentation into the first restored pane.
        self.pane_id = panes[0].0;
        self.pane_layout = layout;
        for &(id, _) in panes.iter().skip(1) {
            self.add_pane_state(id, window, cx);
        }
        for (id, session) in panes {
            self.with_pane(id, cx, |this, cx| {
                let view = session
                    .and_then(|id| this.session_location(id))
                    .map(WorkspaceView::Conversation)
                    .unwrap_or(WorkspaceView::Empty);
                this.set_pane_view(view, window, cx);
            });
        }
        self.swap_pane(self.pane_layout.focused);
        if self.view.displayed_session().is_some() {
            self.conversation
                .composer
                .update(cx, |input, cx| input.focus(window, cx));
        } else {
            window.focus(&self.workspace_focus, cx);
        }
    }

    pub(super) fn split_pane(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_split(direction) {
            return;
        }
        let split_id = self.next_pane_id;
        let pane_id = split_id + 1;
        self.next_pane_id += 2;
        self.add_pane_state(pane_id, window, cx);
        self.pane_layout.root = self.saved_pane_layout().root;
        self.pane_layout
            .split(self.pane_id, direction, split_id, pane_id);
        // activate_pane must observe the focus change to refresh the shared diff.
        self.pane_layout.focused = self.pane_id;
        self.activate_pane(pane_id, cx);
        window.focus(&self.workspace_focus, cx);
        self.persist();
        cx.notify();
    }

    pub(super) fn can_split(&self, direction: Direction) -> bool {
        self.can_split_pane(self.pane_id, direction)
    }

    pub(super) fn can_split_pane(&self, pane_id: u64, direction: Direction) -> bool {
        let Some(bounds) = self.pane_bounds.get(&pane_id).map(|bounds| bounds.get()) else {
            return false;
        };
        let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        match direction {
            Direction::Right => width >= crate::panes::MIN_WIDTH * 2. + crate::panes::DIVIDER,
            Direction::Down => height >= crate::panes::MIN_HEIGHT * 2. + crate::panes::DIVIDER,
        }
    }

    pub(super) fn close_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.pane_id;
        if !self.pane_layout.close(id) {
            return;
        }
        let neighbor = self.pane_layout.focused;
        self.pane_layout.focused = id;
        self.activate_pane(neighbor, cx);
        self.inactive_panes.remove(&id);
        self.pane_bounds.remove(&id);
        if self.view.displayed_session().is_some() {
            self.conversation
                .composer
                .update(cx, |input, cx| input.focus(window, cx));
        } else {
            window.focus(&self.workspace_focus, cx);
        }
        self.persist();
        cx.notify();
    }

    pub(super) fn composer_pane(&self, input: &Entity<TextareaState>) -> Option<u64> {
        if self.conversation.composer.entity_id() == input.entity_id() {
            return Some(self.pane_id);
        }
        self.inactive_panes.iter().find_map(|(&id, state)| {
            (state.conversation.composer.entity_id() == input.entity_id()).then_some(id)
        })
    }

    pub(super) fn subscribe_picker(
        &mut self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscriptions.push(cx.subscribe_in(
            input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                let id = if this.picker.input.entity_id() == input.entity_id() {
                    Some(this.pane_id)
                } else {
                    this.inactive_panes.iter().find_map(|(&id, state)| {
                        (state.picker.input.entity_id() == input.entity_id()).then_some(id)
                    })
                };
                let Some(id) = id else {
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
                this.activate_pane(id, cx);
                match event {
                    InputEvent::Change => {
                        this.picker.selection = 0;
                        if matches!(
                            this.view,
                            WorkspaceView::NewSession {
                                step: PickerStep::Folders | PickerStep::ChangeFolder { .. },
                                ..
                            }
                        ) {
                            this.picker.folder_search.set_query(&input.read(cx).value());
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.confirm_picker(window, cx),
                    _ => {}
                }
            },
        ));
    }

    pub(super) fn tick_panes(&mut self, cx: &mut Context<Self>) {
        for (id, _) in self.pane_layout.root.panes() {
            self.with_pane(id, cx, |this, cx| {
                if this.picker.folder_search.tick() {
                    cx.notify();
                }
                while let Ok(generation) = this.conversation.file_scan_rx.try_recv() {
                    if this.conversation.file_scan_generation == generation
                        && let Some(search) = this.conversation.file_search.as_mut()
                    {
                        search.scan_complete();
                        cx.notify();
                    }
                }
                if this
                    .conversation
                    .file_search
                    .as_mut()
                    .is_some_and(FileSearch::tick)
                {
                    cx.notify();
                }
            });
        }
    }
}

impl Workspace {
    /// Resolve leaves by stable session ID after controller lifecycle changes.
    pub(super) fn reconcile_panes(&mut self) {
        for (id, session) in self.pane_layout.root.panes() {
            let location = session.and_then(|session| self.session_location(session));
            let view = if id == self.pane_id {
                &mut self.view
            } else {
                &mut self.inactive_panes.get_mut(&id).unwrap().view
            };
            if matches!(view, WorkspaceView::Conversation(_)) {
                *view = location
                    .map(WorkspaceView::Conversation)
                    .unwrap_or(WorkspaceView::Empty);
            }
        }
    }
}
