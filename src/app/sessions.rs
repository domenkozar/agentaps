use super::*;

fn initialize_params() -> Value {
    let initialize = v2::InitializeRequest::new(
        ProtocolVersion::V2,
        v2::Implementation::new("agentaps", env!("CARGO_PKG_VERSION")).title("Agentaps"),
    )
    .capabilities(v2::ClientCapabilities::new().elicitation(
        v2::ElicitationCapabilities::new().form(v2::ElicitationFormCapabilities::new()),
    ));
    let mut params = json!(initialize);
    // A v1 agent reads this field when it accepts our v2 version offer.
    params["clientCapabilities"] = json!({"elicitation":{"form":{}}});
    params
}

fn update_config_options(agent: &mut AgentView, options: &Value) {
    if let Some(option) = model_option(options) {
        agent.model = Some(option.label());
        agent.model_option = Some(option);
    } else {
        agent.model_option = None;
    }
    agent.effort_option = effort_option(options);
}

impl Workspace {
    pub(super) fn select_config_option(
        &mut self,
        project_index: usize,
        agent_index: usize,
        kind: ConfigOptionKind,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let agent = &mut self.projects[project_index].agents[agent_index];
        let option = match kind {
            ConfigOptionKind::Model => &agent.model_option,
            ConfigOptionKind::Effort => &agent.effort_option,
        };
        let Some(option) = option else {
            return;
        };
        if agent.pending_model.is_some()
            || agent.pending_effort.is_some()
            || option.current == value
            || !option.choices.iter().any(|choice| choice.value == value)
        {
            return;
        }
        let Some(session_id) = &agent.session_id else {
            return;
        };
        let id = agent.next_request_id;
        let request = set_config_option_request(id, session_id, option, &value);
        match agent.send(request) {
            Ok(()) => {
                agent.next_request_id += 1;
                match kind {
                    ConfigOptionKind::Model => agent.pending_model = Some((id, value)),
                    ConfigOptionKind::Effort => agent.pending_effort = Some((id, value)),
                }
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
        let mut messages = agent.messages.clone();
        messages.push(ChatEntry {
            role: Role::ContextReset,
            key: None,
            text: "Context reset. Earlier messages are visible, but the agent no longer has them in context.".into(),
        });
        let config = agent.reset_config(new_id, messages);
        self.projects[project_index].agents[agent_index] = AgentView::new(config);
        if let Some(id) = self.sidebar_order.iter_mut().find(|id| **id == old_id) {
            *id = new_id;
        } else {
            self.sidebar_order.push(new_id);
        }
        self.collapsed_tool_groups.retain(|(id, _)| *id != old_id);
        self.expanded_tool_history.retain(|(id, _)| *id != old_id);
        self.expanded_tool_rows.retain(|(id, _)| *id != old_id);
        self.chat_list_agent = None;
        self.chat_rows.clear();
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
        });
        self.sidebar_order.push(config.id);
        self.next_agent_id += 1;
        let new_index = self.projects[project_index].agents.len();
        self.projects[project_index]
            .agents
            .push(AgentView::new(config));
        self.set_view(WorkspaceView::Conversation(SessionLocation {
            project_index,
            agent_index: new_index,
        }));
        self.connect(project_index, new_index);
        self.notice = None;
        self.persist();
        self.composer
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
        let value = self.composer.read(cx).value().to_string();
        let prompt = submitted_prompt(&value);
        if prompt.trim().is_empty() || (prompt.starts_with('!') && shell_command(&prompt).is_none())
        {
            self.composer
                .update(cx, |input, cx| input.set_value("", window, cx));
            return;
        }
        let agent = &mut self.projects[project_index].agents[agent_index];
        if agent.status == Status::Error {
            self.notice = Some("Agent is not connected".into());
            cx.notify();
            return;
        }
        if agent.status == Status::Connecting
            || agent.active_work
            || !agent.config.pending_prompts.is_empty()
        {
            agent.config.prompt_history.push(prompt.clone());
            agent.config.pending_prompts.push(prompt);
            self.dirty = true;
            self.prompt_recall = None;
            self.composer
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.notice = None;
            cx.notify();
            return;
        }
        if agent.session_id.is_none() {
            self.notice = Some("Agent is not connected".into());
            cx.notify();
            return;
        }
        match agent.start_prompt(prompt.clone()) {
            Ok(()) => {
                agent.config.prompt_history.push(prompt);
                self.chat_list.scroll_to(gpui::ListOffset {
                    item_ix: self.chat_list.item_count(),
                    offset_in_item: px(0.),
                });
                self.dirty = true;
                self.prompt_recall = None;
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

    pub(super) fn cancel_prompt(&mut self, cx: &mut Context<Self>) {
        if let Some(SessionLocation {
            project_index,
            agent_index,
        }) = self.view.displayed_session()
        {
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
            for elicitation in agent.elicitations.drain(..) {
                let _ = agent.connection.as_ref().map(|connection| connection.send(json!({"jsonrpc":"2.0","id":elicitation.request_id,"result":{"action":"cancel"}})));
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
        if self.diff_watched_project != project_index {
            self.diff_watched_project = project_index;
            self.diff_watch_generation += 1;
            self.diff_watcher = None;
            self.diff_poll_at = None;
            self.diff_refresh_due = project_index.map(|_| Instant::now());
            self.diff_first_change_at = None;
            if let Some(project_index) = project_index {
                let generation = self.diff_watch_generation;
                let path = self.projects[project_index].path.clone();
                let watch_tx = self.diff_watch_tx.clone();
                let watcher_tx = self.diff_watcher_tx.clone();
                std::thread::spawn(move || {
                    let watcher = crate::diff_watch::start(&path, generation, watch_tx);
                    let _ = watcher_tx.send((generation, watcher));
                });
            }
        }
        while let Ok((request_id, result)) = self.diff_rx.try_recv() {
            if request_id != self.diff_request_id {
                continue;
            }
            self.diff_loading = false;
            match result {
                Ok(DiffData::Full(files, stats, presentation, selected_file, rows)) => {
                    let scroll_top = self.diff_list.logical_scroll_top();
                    if self
                        .diff_selected_file
                        .as_ref()
                        .is_some_and(|selected| !files.iter().any(|file| &file.path == selected))
                    {
                        self.diff_selected_file = None;
                    }
                    self.diff_rows = if presentation == self.diff_presentation
                        && selected_file == self.diff_selected_file
                    {
                        rows
                    } else {
                        Arc::new(diff_rows_for_file(
                            &files,
                            self.diff_selected_file.as_deref(),
                            self.diff_presentation,
                        ))
                    };
                    self.diff_list =
                        ListState::new(self.diff_rows.len(), ListAlignment::Top, px(28.));
                    self.diff_list.scroll_to(scroll_top);
                    self.diff_counts = (!stats.is_empty()).then(|| {
                        stats.iter().copied().fold((0, 0), |total, count| {
                            (total.0 + count.0, total.1 + count.1)
                        })
                    });
                    self.diff_files = files;
                    self.diff_file_stats = stats;
                    self.diff_error = None;
                }
                Ok(DiffData::Counts(counts)) => {
                    self.diff_counts = counts;
                    self.diff_error = None;
                }
                Err(error) => {
                    self.diff_counts = None;
                    self.diff_files.clear();
                    self.diff_file_stats.clear();
                    self.diff_error = Some(error);
                }
            }
            cx.notify();
        }
        while let Ok((generation, watcher)) = self.diff_watcher_rx.try_recv() {
            if self.diff_watched_project.is_some() && generation == self.diff_watch_generation {
                self.diff_watcher = watcher.ok();
                if self.diff_watcher.is_none() {
                    self.diff_poll_at = Some(Instant::now() + Duration::from_secs(3));
                }
            }
        }
        let mut watched_change = false;
        while let Ok(generation) = self.diff_watch_rx.try_recv() {
            watched_change |=
                self.diff_watched_project.is_some() && generation == self.diff_watch_generation;
        }
        let now = Instant::now();
        if watched_change {
            let first_change = *self.diff_first_change_at.get_or_insert(now);
            self.diff_refresh_due =
                Some((now + Duration::from_millis(350)).min(first_change + Duration::from_secs(2)));
        }
        if self.diff_poll_at.is_some_and(|poll_at| now >= poll_at) {
            self.diff_refresh_due.get_or_insert(now);
            self.diff_poll_at = Some(now + Duration::from_secs(3));
        }
        if !self.diff_loading && self.diff_refresh_due.is_some_and(|due| now >= due) {
            self.diff_refresh_due = None;
            self.diff_first_change_at = None;
            if let Some(session) = self.view.displayed_session() {
                self.refresh_diff(session.project_index, cx);
            }
        }
        while let Ok(event) = self.events_rx.try_recv() {
            changed = true;
            match event {
                Event::Message { agent_id, value } => {
                    self.handle_message(agent_id, value, window, cx)
                }
                Event::Disconnected { agent_id, reason } => {
                    if let Some((agent, _)) = self.agent_mut(agent_id) {
                        agent.awaiting_response = false;
                        agent.pending_model = None;
                        agent.pending_effort = None;
                        agent.cancel_requested = false;
                        agent.elicitations.clear();
                        agent.permissions.clear();
                        if agent.status != Status::Error {
                            agent.status = Status::Error;
                            agent.log(Role::System, reason);
                        }
                    }
                }
            }
        }
        self.mark_displayed_agent_viewed();
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

    pub(super) fn start_new_session(agent: &mut AgentView, path: &Path) {
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

    pub(super) fn resume_session(
        agent: &mut AgentView,
        path: &Path,
        mode: RestoreMode,
        session_id: String,
    ) {
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
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            match method {
                "session/update" => Self::handle_update(agent, &value),
                "$/cancel_request" => {
                    let request_id = &value["params"]["requestId"];
                    if let Some(index) = agent
                        .elicitations
                        .iter()
                        .position(|question| question.request_id == *request_id)
                    {
                        agent.elicitations.remove(index);
                        let _ = agent.send(json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32800,"message":"Request cancelled"}}));
                    }
                }
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
                "elicitation/create" => {
                    let Some(request_id) = value.get("id").cloned() else {
                        return;
                    };
                    let params = &value["params"];
                    if params["sessionId"].as_str() != agent.session_id.as_deref() {
                        let _ = agent.send(json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32602,"message":"Unknown session"}}));
                        return;
                    }
                    match Elicitation::new(request_id.clone(), params, window, cx) {
                        Ok(question) => agent.elicitations.push(question),
                        Err(message) => {
                            let _ = agent.send(json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32602,"message":message}}));
                        }
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
        let config_kind = if agent
            .pending_model
            .as_ref()
            .is_some_and(|(pending_id, _)| *pending_id == id)
        {
            Some(ConfigOptionKind::Model)
        } else if agent
            .pending_effort
            .as_ref()
            .is_some_and(|(pending_id, _)| *pending_id == id)
        {
            Some(ConfigOptionKind::Effort)
        } else {
            None
        };
        if let Some(kind) = config_kind {
            let (_, selected) = match kind {
                ConfigOptionKind::Model => agent.pending_model.take().unwrap(),
                ConfigOptionKind::Effort => agent.pending_effort.take().unwrap(),
            };
            if let Some(error) = value.get("error") {
                let message = error["message"].as_str().unwrap_or("unknown error");
                agent.log(Role::System, format!("Could not change setting: {message}"));
            } else if value["result"]["configOptions"].is_array() {
                update_config_options(agent, &value["result"]["configOptions"]);
            } else {
                match kind {
                    ConfigOptionKind::Model => {
                        if let Some(option) = &mut agent.model_option {
                            option.current = selected;
                            agent.model = Some(option.label());
                        }
                    }
                    ConfigOptionKind::Effort => {
                        if let Some(option) = &mut agent.effort_option {
                            option.current = selected;
                        }
                    }
                }
            }
            return;
        }
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
                    update_config_options(agent, &value["result"]["configOptions"]);
                    if agent.status == Status::Connecting {
                        agent.status = Status::Idle;
                    }
                } else if let Some(session_id) = value["result"]["sessionId"].as_str() {
                    agent.session_id = Some(session_id.to_owned());
                    agent.config.session_id = agent.session_id.clone();
                    update_config_options(agent, &value["result"]["configOptions"]);
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

    pub(super) fn handle_update(agent: &mut AgentView, value: &Value) {
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
                update_config_options(agent, &update["configOptions"]);
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

    pub(super) fn refresh_branches(&mut self, cx: &mut Context<Self>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_form_questions_to_v1_and_v2_agents() {
        let params = initialize_params();
        let v2: v2::InitializeRequest = serde_json::from_value(params.clone()).unwrap();
        assert!(v2.capabilities.elicitation.unwrap().supports_form());
        let v1: agent_client_protocol_schema::v1::InitializeRequest =
            serde_json::from_value(params).unwrap();
        assert!(v1.client_capabilities.elicitation.unwrap().supports_form());
    }
}
