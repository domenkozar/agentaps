use super::*;

impl Workspace {
    pub(super) fn open_rename_session(
        &mut self,
        agent_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((agent, _)) = self.agent_mut(agent_id) else {
            return;
        };
        let title = agent.config.session_title().unwrap_or_default().to_owned();
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("Session name");
            input.set_value(title, window, cx);
            input
        });
        let pane_id = self.pane_id;
        let subscription =
            cx.subscribe_in(
                &input,
                window,
                move |this, _, event, window, cx| match event {
                    InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    } => {
                        if this.activate_pane(pane_id, cx) {
                            this.finish_rename_session(true, cx);
                            this.conversation
                                .composer
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                    }
                    InputEvent::Blur => {
                        this.with_pane(pane_id, cx, |this, cx| {
                            this.finish_rename_session(true, cx)
                        });
                    }
                    _ => {}
                },
            );
        self.conversation.renaming = Some(SessionRename {
            agent_id,
            input: input.clone(),
            _subscription: subscription,
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn finish_rename_session(&mut self, save: bool, cx: &mut Context<Self>) {
        let Some(rename) = self.conversation.renaming.take() else {
            return;
        };
        if save {
            let title = rename.input.read(cx).value().to_string();
            if let Some((agent, _)) = self.agent_mut(rename.agent_id) {
                agent.config.rename_session(&title);
                self.persist();
            }
        }
        cx.notify();
    }

    pub(super) fn move_session_to_project(
        &mut self,
        session: SessionLocation,
        target_project_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if session.project_index == target_project_index {
            self.set_view(WorkspaceView::Conversation(session), window, cx);
            self.conversation
                .composer
                .update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
            return;
        }

        let source = &self.projects[session.project_index].agents[session.agent_index];
        let old_id = source.config.id;
        let new_id = self.next_agent_id;
        let messages = vec![ChatEntry {
            role: Role::System,
            key: None,
            text: format!(
                "Folder changed to {}. The previous session is archived; this session starts with fresh context.",
                self.projects[target_project_index].display_path()
            ),
            images: Vec::new(),
        }];
        let mut config = source.reset_config(new_id, messages);
        config.title = source.config.title.clone();

        let mut archived = source.snapshot();
        archived.archived = true;
        self.projects[session.project_index].agents[session.agent_index] =
            AgentView::new(archived, self.images.clone());
        let agent_index = self.projects[target_project_index].agents.len();
        self.projects[target_project_index]
            .agents
            .push(AgentView::new(config, self.images.clone()));
        self.next_agent_id += 1;
        if let Some(id) = self.sidebar_order.iter_mut().find(|id| **id == old_id) {
            *id = new_id;
        }
        if let Some(composer) = self.session_composers.remove(&old_id) {
            self.session_composers.insert(new_id, composer);
        }
        if let Some(images) = self.draft_images.remove(&old_id) {
            self.draft_images.insert(new_id, images);
        }
        if let Some(files) = self.draft_files.remove(&old_id) {
            self.draft_files.insert(new_id, files);
        }
        self.deferred_connections.retain(|id| *id != old_id);
        self.conversation
            .collapsed_tool_groups
            .retain(|(id, _)| *id != old_id);
        self.conversation
            .expanded_tool_history
            .retain(|(id, _)| *id != old_id);
        self.conversation
            .expanded_tool_rows
            .retain(|(id, _)| *id != old_id);
        self.conversation
            .expanded_thought_rows
            .retain(|(id, _)| *id != old_id);
        self.conversation.chat_list_agent = None;
        self.conversation.chat_rows.clear();

        self.set_view(
            WorkspaceView::Conversation(SessionLocation {
                project_index: target_project_index,
                agent_index,
            }),
            window,
            cx,
        );
        self.connect(target_project_index, agent_index);
        self.notice = None;
        self.persist();
        self.conversation
            .composer
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn select_config_option(
        &mut self,
        project_index: usize,
        agent_index: usize,
        kind: ConfigOptionKind,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let agent = &mut self.projects[project_index].agents[agent_index];
        if agent.setting_pending() {
            return;
        }
        let Some(option) = agent.setting_option(kind).clone() else {
            return;
        };
        if option.current == value || !option.choices.iter().any(|choice| choice.value == value) {
            return;
        }
        let Some(session_id) = &agent.controller.session_id else {
            return;
        };
        let id = agent.next_request_id;
        let request = match kind {
            ConfigOptionKind::Mode => {
                mode_request(id, session_id, &option, agent.legacy_modes, &value)
            }
            _ => set_config_option_request(id, session_id, &option, &value),
        };
        match agent.send(request) {
            Ok(()) => {
                agent.next_request_id += 1;
                *agent.pending_setting(kind) = Some((id, value));
            }
            Err(error) => agent.log(Role::System, format!("Could not change setting: {error}")),
        }
        cx.notify();
    }

    pub(super) fn reset_context(
        &mut self,
        project_index: usize,
        agent_index: usize,
        cx: &mut Context<Self>,
    ) {
        let old_id = self.projects[project_index].agents[agent_index].config.id;
        let new_id = self.next_agent_id;
        self.next_agent_id += 1;
        let agent = &self.projects[project_index].agents[agent_index];
        let messages = vec![ChatEntry {
            role: Role::ContextReset,
            key: None,
            text: "Context reset. The previous session is archived; this session starts with fresh context.".into(),
            images: Vec::new(),
        }];
        let config = agent.reset_config(new_id, messages);
        let mut archived = agent.snapshot();
        archived.archived = true;
        self.projects[project_index].agents[agent_index] =
            AgentView::new(config, self.images.clone());
        self.projects[project_index]
            .agents
            .push(AgentView::new(archived, self.images.clone()));
        if let Some(id) = self.sidebar_order.iter_mut().find(|id| **id == old_id) {
            *id = new_id;
        } else {
            self.sidebar_order.push(new_id);
        }
        if let Some(composer) = self.session_composers.remove(&old_id) {
            self.session_composers.insert(new_id, composer);
        }
        if let Some(images) = self.draft_images.remove(&old_id) {
            self.draft_images.insert(new_id, images);
        }
        if let Some(files) = self.draft_files.remove(&old_id) {
            self.draft_files.insert(new_id, files);
        }
        self.conversation
            .collapsed_tool_groups
            .retain(|(id, _)| *id != old_id);
        self.conversation
            .expanded_tool_history
            .retain(|(id, _)| *id != old_id);
        self.conversation
            .expanded_tool_rows
            .retain(|(id, _)| *id != old_id);
        self.conversation
            .expanded_thought_rows
            .retain(|(id, _)| *id != old_id);
        self.conversation.chat_list_agent = None;
        self.conversation.chat_rows.clear();
        self.pane_layout.root = self.saved_pane_layout().root;
        self.connect(project_index, agent_index);
        self.notice = None;
        self.persist();
        cx.notify();
    }

    pub(super) fn fork_conversation(
        &mut self,
        project_index: usize,
        agent_index: usize,
        response_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self
            .projects
            .get(project_index)
            .and_then(|project| project.agents.get(agent_index))
        else {
            return;
        };
        if source.active_work {
            return;
        }
        let Some(mut config) = source.fork_config(self.next_agent_id, response_index) else {
            return;
        };
        config.messages.push(ChatEntry {
            role: Role::System,
            key: None,
            text: "Forked here. The earlier conversation will be provided as context with your first message. Project files reflect their current state.".into(),
            images: Vec::new(),
        });
        self.sidebar_order.push(config.id);
        self.next_agent_id += 1;
        let new_index = self.projects[project_index].agents.len();
        self.projects[project_index]
            .agents
            .push(AgentView::new(config, self.images.clone()));
        self.set_view(
            WorkspaceView::Conversation(SessionLocation {
                project_index,
                agent_index: new_index,
            }),
            window,
            cx,
        );
        self.connect(project_index, new_index);
        self.notice = None;
        self.persist();
        self.conversation
            .composer
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn connect(&mut self, project_index: usize, agent_index: usize) {
        let agent_id = self.projects[project_index].agents[agent_index].config.id;
        self.deferred_connections.retain(|id| *id != agent_id);
        let project = &mut self.projects[project_index];
        let agent = &mut project.agents[agent_index];
        match Connection::spawn(
            agent.config.id,
            &agent.config.command,
            &project.path,
            project.ssh_host.as_deref(),
            self.events_tx.clone(),
        ) {
            Ok(connection) => {
                agent.connection = Some(connection);
                if let Err(error) = agent
                    .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":initialize_params()}))
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

    pub(super) fn send_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(SessionLocation {
            project_index,
            agent_index,
        }) = self.view.displayed_session()
        else {
            return;
        };
        let agent = &mut self.projects[project_index].agents[agent_index];
        let value = self.conversation.composer.read(cx).value().to_string();
        let prompt = Prompt {
            text: submitted_prompt(&value),
            images: self
                .draft_images
                .get(&agent.config.id)
                .cloned()
                .unwrap_or_default(),
            files: self
                .draft_files
                .get(&agent.config.id)
                .cloned()
                .unwrap_or_default(),
        };
        if prompt.images.is_empty()
            && prompt.files.is_empty()
            && (prompt.text.trim().is_empty()
                || (prompt.text.starts_with('!') && shell_command(&prompt.text).is_none()))
        {
            self.conversation
                .composer
                .update(cx, |input, cx| input.set_value("", window, cx));
            return;
        }
        if agent.status == Status::Error {
            self.notice = Some(Notice::Error("Agent is not connected".into()));
            cx.notify();
            return;
        }
        let queue = agent.status == Status::Connecting
            || agent.active_work
            || !agent.config.pending_prompts.is_empty();
        if !queue && agent.session_id.is_none() {
            self.notice = Some(Notice::Error("Agent is not connected".into()));
            cx.notify();
            return;
        }
        let text = prompt.text.clone();
        if queue {
            agent.config.pending_prompts.push(prompt);
        } else {
            if let Err(error) = agent.start_prompt(prompt) {
                agent.status = Status::Error;
                agent.log(Role::System, error);
                cx.notify();
                return;
            }
            self.conversation.chat_list.scroll_to(gpui_kit::ListOffset {
                item_ix: self.conversation.chat_list.item_count(),
                offset_in_item: px(0.),
            });
        }
        agent.config.prompt_history.push(text);
        self.draft_images.remove(&agent.config.id);
        self.draft_files.remove(&agent.config.id);
        self.persistence.dirty = true;
        self.conversation.prompt_recall = None;
        self.conversation
            .composer
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.notice = None;
        cx.notify();
    }

    pub(super) fn cancel_prompt(&mut self, cx: &mut Context<Self>) {
        if let Some(SessionLocation {
            project_index,
            agent_index,
        }) = self.view.displayed_session()
        {
            let agent = &mut self.projects[project_index].agents[agent_index];
            if agent
                .oauth_retry
                .as_ref()
                .is_some_and(|retry| retry.due.is_some())
            {
                agent.oauth_retry = None;
                agent.active_work = false;
                agent.awaiting_response = false;
                agent.config.active_prompt = None;
                agent.status = Status::Idle;
                agent.log(Role::System, "Turn cancelled");
                cx.notify();
                return;
            }
            if agent.active_work
                && !agent.cancel_requested
                && let Some(session_id) = &agent.controller.session_id
            {
                match agent.controller.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session_id}})) {
                    Ok(()) => agent.cancel_requested = true,
                    Err(error) => agent.log(Role::System, format!("Could not stop agent: {error}")),
                }
            }
            for permission in agent.controller.permissions.drain(..) {
                let _ = agent.controller.connection.as_ref().map(|connection| connection.send(json!({"jsonrpc":"2.0","id":permission.request_id,"result":{"outcome":{"outcome":"cancelled"}}})));
            }
            for elicitation in agent.elicitations.drain(..) {
                let _ = agent.controller.connection.as_ref().map(|connection| connection.send(json!({"jsonrpc":"2.0","id":elicitation.request_id,"result":{"action":"cancel"}})));
            }
            agent.awaiting_response = false;
            cx.notify();
        }
    }

    pub(super) fn choose_permission(
        &mut self,
        permission_index: usize,
        option_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(SessionLocation {
            project_index,
            agent_index,
        }) = self.view.displayed_session()
        else {
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

    pub(super) fn answer_elicitation(
        &mut self,
        question_index: usize,
        action: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(location) = self.view.displayed_session() else {
            return;
        };
        let agent = &mut self.projects[location.project_index].agents[location.agent_index];
        let Some(question) = agent.elicitations.get(question_index) else {
            return;
        };
        let result = if action == "accept" {
            match question.content(cx) {
                Ok(content) => json!({"action":"accept","content":content}),
                Err(error) => {
                    agent.elicitations[question_index].error = Some(error);
                    cx.notify();
                    return;
                }
            }
        } else {
            json!({"action":action})
        };
        let question = agent.elicitations.remove(question_index);
        if let Err(error) =
            agent.send(json!({"jsonrpc":"2.0","id":question.request_id,"result":result}))
        {
            agent.log(Role::System, format!("Could not answer question: {error}"));
            agent.status = Status::Error;
        }
        cx.notify();
    }

    pub(super) fn select_elicitation_option(
        &mut self,
        question_index: usize,
        field_index: usize,
        option_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(location) = self.view.displayed_session() else {
            return;
        };
        let Some(field) = self.projects[location.project_index].agents[location.agent_index]
            .elicitations
            .get_mut(question_index)
            .and_then(|question| question.fields.get_mut(field_index))
        else {
            return;
        };
        match &mut field.kind {
            ElicitationFieldKind::Select { selected, .. } => *selected = Some(option_index),
            ElicitationFieldKind::MultiSelect { selected, .. } => {
                if !selected.insert(option_index) {
                    selected.remove(&option_index);
                }
            }
            ElicitationFieldKind::Boolean(value) => *value = Some(option_index == 1),
            ElicitationFieldKind::Input(_) => return,
        }
        cx.notify();
    }

    pub(super) fn poll_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = false;
        if !self.deferred_connections.is_empty() {
            let visible_ready = self.view.displayed_session().is_some_and(|session| {
                self.projects[session.project_index].agents[session.agent_index].status
                    != Status::Connecting
            });
            if visible_ready
                || self
                    .deferred_connections_deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
            {
                let agent_id = self.deferred_connections.remove(0);
                if let Some((project_index, agent_index)) = self
                    .projects
                    .iter()
                    .enumerate()
                    .find_map(|(project_index, project)| {
                        project
                            .agents
                            .iter()
                            .position(|agent| agent.config.id == agent_id && !agent.config.archived)
                            .map(|agent_index| (project_index, agent_index))
                    })
                {
                    self.connect(project_index, agent_index);
                }
            }
        }
        let project_index = self
            .view
            .displayed_session()
            .map(|session| session.project_index);
        if self.diff.watched_project != project_index {
            self.diff.watched_project = project_index;
            self.diff.watch_generation += 1;
            self.diff.watcher = None;
            self.diff.poll_at = None;
            self.diff.refresh_due = project_index.map(|_| Instant::now());
            self.diff.first_change_at = None;
            if let Some(project_index) = project_index {
                let generation = self.diff.watch_generation;
                let path = self.projects[project_index].path.clone();
                let watch_tx = self.diff.watch_tx.clone();
                let watcher_tx = self.diff.watcher_tx.clone();
                std::thread::spawn(move || {
                    let watcher = crate::diff_watch::start(&path, generation, watch_tx);
                    let _ = watcher_tx.send((generation, watcher));
                });
            }
        }
        if self.diff.poll_results() {
            cx.notify();
        }
        while let Ok((generation, watcher)) = self.diff.watcher_rx.try_recv() {
            if self.diff.watched_project.is_some() && generation == self.diff.watch_generation {
                self.diff.watcher = watcher.ok();
                if self.diff.watcher.is_none() {
                    self.diff.poll_at = Some(Instant::now() + Duration::from_secs(3));
                }
            }
        }
        let mut watched_change = false;
        while let Ok(generation) = self.diff.watch_rx.try_recv() {
            watched_change |=
                self.diff.watched_project.is_some() && generation == self.diff.watch_generation;
        }
        let now = Instant::now();
        if watched_change {
            let first_change = *self.diff.first_change_at.get_or_insert(now);
            self.diff.refresh_due =
                Some((now + Duration::from_millis(350)).min(first_change + Duration::from_secs(2)));
        }
        if self.diff.poll_at.is_some_and(|poll_at| now >= poll_at) {
            self.diff.refresh_due.get_or_insert(now);
            self.diff.poll_at = Some(now + Duration::from_secs(3));
        }
        if !self.diff.loading && self.diff.refresh_due.is_some_and(|due| now >= due) {
            self.diff.refresh_due = None;
            self.diff.first_change_at = None;
            if let Some(session) = self.view.displayed_session() {
                self.refresh_diff(session.project_index, cx);
            }
        }
        while let Ok(event) = self.events_rx.try_recv() {
            changed = true;
            match event {
                Event::Message { agent_id, value } => {
                    let may_finish = value["params"]["update"]["sessionUpdate"] == "state_update"
                        && value["params"]["update"]["state"] == "idle"
                        || value["id"].as_u64().is_some_and(|id| id >= 3);
                    let running_project = may_finish
                        .then(|| {
                            self.projects.iter().find_map(|project| {
                                project
                                    .agents
                                    .iter()
                                    .any(|agent| agent.config.id == agent_id && agent.active_work)
                                    .then(|| (project.path.clone(), project.ssh_host.clone()))
                            })
                        })
                        .flatten();
                    self.handle_message(agent_id, value, window, cx);
                    if let Some((path, host)) = running_project
                        && self.projects.iter().any(|project| {
                            project.path == path
                                && project.ssh_host == host
                                && project
                                    .agents
                                    .iter()
                                    .any(|agent| agent.config.id == agent_id && !agent.active_work)
                        })
                    {
                        self.sync
                            .refresh_pending
                            .insert((path.clone(), host.clone()));
                        if let Some(project) = self
                            .projects
                            .iter_mut()
                            .find(|project| project.path == path && project.ssh_host == host)
                        {
                            project.sync_counts = None;
                        }
                    }
                }
                Event::Disconnected { agent_id, reason } => {
                    if let Some((agent, _)) = self.agent_mut(agent_id) {
                        agent.elicitations.clear();
                        agent.controller.disconnected(reason);
                    }
                }
            }
        }
        self.mark_displayed_agent_viewed();
        for project in &mut self.projects {
            for agent in &mut project.agents {
                changed |= agent.retry_oauth_request();
                changed |= agent.auto_continue_interrupted_turn();
                changed |= agent.start_next_queued_prompt();
            }
        }
        if changed {
            self.persistence.dirty = true;
            cx.notify();
        }
        self.refresh_pending_sync_counts(cx);
        self.poll_mobile(cx);
        if let Some(error) = self.persistence.poll() {
            self.notice = Some(Notice::Error(format!("Could not save config: {error}")));
            cx.notify();
        }
        if self.persistence.due() {
            self.persist();
        }
    }

    pub(super) fn agent_mut(&mut self, agent_id: u64) -> Option<(&mut AgentView, PathBuf)> {
        self.projects.iter_mut().find_map(|project| {
            let path = project.path.clone();
            project
                .agents
                .iter_mut()
                .find(|agent| agent.config.id == agent_id)
                .map(|agent| (agent, path))
        })
    }

    pub(super) fn handle_message(
        &mut self,
        agent_id: u64,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((agent, path)) = self.agent_mut(agent_id) else {
            return;
        };
        match agent.controller.handle_message(&path, &value) {
            Some(UiRequest::CancelElicitation(request_id)) => {
                if let Some(index) = agent
                    .elicitations
                    .iter()
                    .position(|question| question.request_id == request_id)
                {
                    agent.elicitations.remove(index);
                    let _ = agent.send(json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32800,"message":"Request cancelled"}}));
                }
            }
            Some(UiRequest::Elicitation { request_id, params }) => {
                match Elicitation::new(request_id.clone(), &params, window, cx) {
                    Ok(question) => agent.elicitations.push(question),
                    Err(message) => {
                        let _ = agent.send(json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32602,"message":message}}));
                    }
                }
            }
            None => {}
        }
    }

    fn refresh_pending_sync_counts(&mut self, cx: &mut Context<Self>) {
        if !self.sync.loading
            && self
                .sync
                .refresh_pending
                .iter()
                .any(|key| !self.sync.in_progress.contains(key))
        {
            self.refresh_branches(cx);
        }
    }

    pub(super) fn refresh_branches(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for project in &mut self.projects {
            let fresh = project
                .ssh_host
                .clone()
                .unwrap_or_else(|| branch(&project.path));
            if fresh != project.branch {
                project.branch = fresh;
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
        self.sync.schedule(&self.projects);
    }

    pub(super) fn start_git_sync(
        &mut self,
        project_index: usize,
        action: crate::git_sync::SyncAction,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.projects.get(project_index) else {
            return;
        };
        if project.agents.iter().any(|agent| agent.active_work) {
            self.notice = Some(Notice::Info(
                "Wait for the agent to finish before syncing this project".into(),
            ));
            cx.notify();
            return;
        }
        let path = project.path.clone();
        let host = project.ssh_host.clone();
        let key = (path.clone(), host.clone());
        if !self.sync.in_progress.insert(key) {
            return;
        }
        self.notice = Some(Notice::Info(match action {
            crate::git_sync::SyncAction::Push => "Pushing commits…".into(),
            crate::git_sync::SyncAction::Pull => "Pulling commits…".into(),
        }));
        let tx = self.sync.tx.clone();
        std::thread::spawn(move || {
            let result = crate::git_sync::sync(&path, host.as_deref(), action);
            if result.is_err() && host.is_none() {
                crate::git_sync::fetch(&path);
            }
            let counts = host.as_ref().map_or_else(
                || crate::git_sync::counts(&path),
                |host| crate::git_sync::remote_counts(host, &path, result.is_err()),
            );
            let _ = tx.send(SyncUpdate::OperationFinished {
                path,
                host,
                action,
                result,
                counts,
            });
        });
        cx.notify();
    }

    pub(super) fn poll_sync_counts(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        while let Ok(update) = self.sync.rx.try_recv() {
            match update {
                SyncUpdate::Counts(path, host, counts) => {
                    if self
                        .sync
                        .in_progress
                        .contains(&(path.clone(), host.clone()))
                        || self
                            .sync
                            .refresh_pending
                            .contains(&(path.clone(), host.clone()))
                    {
                        continue;
                    }
                    if let Some(project) = self
                        .projects
                        .iter_mut()
                        .find(|project| project.ssh_host == host && project.path == path)
                        && project.sync_counts != counts
                    {
                        project.sync_counts = counts;
                        changed = true;
                    }
                }
                SyncUpdate::OperationFinished {
                    path,
                    host,
                    action,
                    result,
                    counts,
                } => {
                    let key = (path.clone(), host.clone());
                    self.sync.in_progress.remove(&key);
                    if !self.sync.refresh_pending.contains(&key)
                        && let Some(project) = self
                            .projects
                            .iter_mut()
                            .find(|project| project.ssh_host == host && project.path == path)
                    {
                        project.sync_counts = counts;
                    }
                    self.notice = Some(match result {
                        Ok(_) => Notice::Info(match action {
                            crate::git_sync::SyncAction::Push => "Commits pushed.".into(),
                            crate::git_sync::SyncAction::Pull => "Commits pulled.".into(),
                        }),
                        Err(error) => Notice::Error(format!("Could not sync commits: {error}")),
                    });
                    changed = true;
                }
                SyncUpdate::Finished => self.sync.loading = false,
            }
        }
        if changed {
            cx.notify();
        }
        self.refresh_pending_sync_counts(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_form_questions_to_v1_and_v2_agents() {
        let params = initialize_params();
        let v2: agent_client_protocol_schema::v2::InitializeRequest =
            serde_json::from_value(params.clone()).unwrap();
        assert!(v2.capabilities.elicitation.unwrap().supports_form());
        let v1: agent_client_protocol_schema::v1::InitializeRequest =
            serde_json::from_value(params).unwrap();
        assert!(v1.client_capabilities.elicitation.unwrap().supports_form());
    }
}
