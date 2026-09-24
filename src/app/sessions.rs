use super::*;

impl Workspace {
    pub(super) fn connect(&mut self, project_index: usize, agent_index: usize) {
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

    pub(super) fn send_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                self.chat_list.scroll_to(gpui::ListOffset {
                    item_ix: self.chat_list.item_count(),
                    offset_in_item: px(0.),
                });
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

    pub(super) fn cancel_prompt(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn choose_permission(
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

    pub(super) fn poll_events(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn handle_message(&mut self, agent_id: u64, value: Value) {
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
