use super::*;
use agentaps_control_protocol::{
    Agent as RemoteAgent, AgentOption, Command as RemoteCommand, Message as RemoteMessage,
    Permission as RemotePermission, PermissionOption, Project as RemoteProject, Response,
};
use gpui::ClipboardItem;
use qrcode::{Color, QrCode};

fn mobile_text(text: &str) -> String {
    let Some((end, _)) = text.char_indices().nth(4_000) else {
        return text.to_owned();
    };
    format!("{}\n[Message shortened on mobile]", &text[..end])
}

impl Workspace {
    pub(super) fn mobile_link(&self) -> Option<String> {
        let endpoint_id = self.mobile_endpoint_id.as_ref()?;
        let token = self.mobile.as_ref()?.pairing_token.lock().ok()?.clone();
        let base =
            std::env::var("AGENTAPS_WEB_URL").unwrap_or_else(|_| "https://agentaps.dev/".into());
        Some(format!(
            "{}#{}:{token}",
            base.trim_end_matches('#'),
            endpoint_id
        ))
    }

    pub(super) fn show_mobile_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.is_none() {
            match crate::mobile::start() {
                Ok(server) => {
                    self.mobile = Some(server);
                    self.notice = Some(
                        "Starting mobile access. Select the phone icon again to copy the link."
                            .into(),
                    );
                }
                Err(error) => {
                    self.mobile_provider_prompt = Some(error);
                    self.notice = None;
                    self.mobile_provider_input
                        .update(cx, |input, cx| input.focus(window, cx));
                }
            }
        } else if let Some(link) = self.mobile_link() {
            cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
            self.mobile_qr = QrCode::new(link.as_bytes()).ok().map(|qr| {
                qr.to_colors()
                    .chunks(qr.width())
                    .map(|row| row.iter().map(|color| *color == Color::Dark).collect())
                    .collect()
            });
            self.mobile_pairing_visible = !self.mobile_pairing_visible;
            self.notice = Some(
                if self.mobile_pairing_visible {
                    "Mobile link copied. Scan the code or open the copied link on your phone."
                } else {
                    "Mobile pairing code hidden. Mobile access is still on."
                }
                .into(),
            );
        } else {
            self.notice = Some("Mobile access is starting".into());
        }
        cx.notify();
    }

    pub(super) fn use_mobile_provider(&mut self, cx: &mut Context<Self>) {
        let provider = self
            .mobile_provider_input
            .read(cx)
            .value()
            .trim()
            .to_owned();
        self.use_mobile_provider_named(&provider, cx);
    }

    pub(super) fn use_mobile_provider_named(&mut self, provider: &str, cx: &mut Context<Self>) {
        if provider.is_empty() {
            self.mobile_provider_prompt = Some("Enter a SecretSpec provider name or URI.".into());
            cx.notify();
            return;
        }
        match crate::mobile::start_with_provider(Some(provider)) {
            Ok(server) => {
                self.mobile = Some(server);
                self.mobile_provider_prompt = None;
                self.notice = Some("Starting mobile access with the selected provider.".into());
            }
            Err(error) => {
                self.mobile_provider_prompt =
                    Some(format!("Could not use the selected provider: {error}"));
            }
        }
        cx.notify();
    }

    pub(super) fn copy_mobile_link(&mut self, cx: &mut Context<Self>) {
        if let Some(link) = self.mobile_link() {
            cx.write_to_clipboard(ClipboardItem::new_string(link));
            self.notice = Some("Mobile link copied. Open it on your phone to pair.".into());
            cx.notify();
        }
    }

    pub(super) fn revoke_mobile_client(&mut self, id: String, cx: &mut Context<Self>) {
        if self.mobile_revoke_confirm.as_deref() != Some(&id) {
            self.mobile_revoke_confirm = Some(id);
            cx.notify();
            return;
        }
        self.mobile_revoke_confirm = None;
        if let Some(server) = &self.mobile {
            server.revoke_client(id);
            self.notice = Some("Revoking linked client…".into());
        }
        cx.notify();
    }

    pub(super) fn poll_mobile(&mut self, cx: &mut Context<Self>) {
        let Some(server) = &self.mobile else {
            return;
        };
        let statuses: Vec<_> = server.status.try_iter().collect();
        let revocations: Vec<_> = server.revoke_results.try_iter().collect();
        let commands: Vec<_> = server.commands.try_iter().collect();
        let handled_command = !commands.is_empty();
        let received_status = !statuses.is_empty();
        for status in statuses {
            match status {
                Ok(endpoint_id) => {
                    let already_ready = self.mobile_endpoint_id.is_some();
                    self.mobile_endpoint_id = Some(endpoint_id);
                    if already_ready {
                        self.mobile_qr = self.mobile_link().and_then(|link| {
                            QrCode::new(link.as_bytes()).ok().map(|qr| {
                                qr.to_colors()
                                    .chunks(qr.width())
                                    .map(|row| {
                                        row.iter().map(|color| *color == Color::Dark).collect()
                                    })
                                    .collect()
                            })
                        });
                        self.notice = Some("Browser linked. A fresh pairing code is ready.".into());
                    } else if let Some(link) = self.mobile_link() {
                        self.mobile_qr = QrCode::new(link.as_bytes()).ok().map(|qr| {
                            qr.to_colors()
                                .chunks(qr.width())
                                .map(|row| row.iter().map(|color| *color == Color::Dark).collect())
                                .collect()
                        });
                        cx.write_to_clipboard(ClipboardItem::new_string(link));
                        self.mobile_pairing_visible = true;
                        self.notice = Some("Mobile link copied. Scan the code or open the copied link on your phone.".into());
                    }
                }
                Err(error) => self.notice = Some(format!("Mobile access failed: {error}")),
            }
            cx.notify();
        }
        for result in revocations {
            self.notice = Some(match result {
                Ok(_) => "Linked client revoked.".into(),
                Err(error) => format!("Could not revoke linked client: {error}"),
            });
            cx.notify();
        }
        for pending in commands {
            let response = match self.apply_mobile_command(pending.command, cx) {
                Ok(response) => response,
                Err(message) => Response::Error { message },
            };
            let _ = pending.reply.send(response);
        }
        if handled_command
            || received_status
            || self
                .last_mobile_snapshot
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(500))
        {
            let snapshot = self.mobile_snapshot();
            if let Some(server) = &self.mobile {
                *server.snapshot.lock().unwrap() = snapshot;
            }
            self.last_mobile_snapshot = Some(Instant::now());
        }
    }

    fn mobile_snapshot(&self) -> Response {
        let mut agent_options: Vec<AgentOption> = self
            .available_agents
            .iter()
            .map(|agent| AgentOption {
                name: agent.name.clone(),
                command: agent.command.clone(),
            })
            .collect();
        for agent in self.projects.iter().flat_map(|project| &project.agents) {
            if !agent_options
                .iter()
                .any(|option| option.command == agent.config.command)
            {
                agent_options.push(AgentOption {
                    name: agent.name.clone(),
                    command: agent.config.command.clone(),
                });
            }
        }
        Response::Snapshot {
            agent_options,
            projects: self
                .projects
                .iter()
                .map(|project| RemoteProject {
                    path: project.display_path(),
                    agents: project
                        .agents
                        .iter()
                        .filter(|agent| !agent.config.archived)
                        .map(|agent| RemoteAgent {
                            id: agent.config.id,
                            name: agent.name.clone(),
                            status: agent.status.label().into(),
                            active: agent.active_work,
                            has_older_messages: agent.messages.len() > 100,
                            messages: agent
                                .messages
                                .iter()
                                .skip(agent.messages.len().saturating_sub(100))
                                .map(|message| RemoteMessage {
                                    role: format!("{:?}", message.role).to_lowercase(),
                                    text: mobile_text(&message.text),
                                })
                                .collect(),
                            permissions: agent
                                .permissions
                                .iter()
                                .map(|permission| RemotePermission {
                                    request_id: permission.request_id.to_string(),
                                    title: permission.title.clone(),
                                    description: permission.description.clone(),
                                    options: permission
                                        .options
                                        .iter()
                                        .map(|(id, label)| PermissionOption {
                                            id: id.clone(),
                                            label: label.clone(),
                                        })
                                        .collect(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    fn mobile_agent_mut(&mut self, agent_id: u64) -> Option<&mut AgentView> {
        self.projects
            .iter_mut()
            .flat_map(|project| &mut project.agents)
            .find(|agent| agent.config.id == agent_id && !agent.config.archived)
    }

    fn apply_mobile_command(
        &mut self,
        command: RemoteCommand,
        cx: &mut Context<Self>,
    ) -> Result<Response, String> {
        match command {
            RemoteCommand::Pair | RemoteCommand::Snapshot => {}
            RemoteCommand::NewSession {
                project,
                command,
                name,
            } => {
                if project.is_empty() || project.len() > 4096 {
                    return Err("Enter a project path".into());
                }
                if command.is_empty()
                    || command.len() > 32
                    || command.iter().any(|arg| arg.is_empty() || arg.len() > 1024)
                {
                    return Err("Enter a valid ACP command".into());
                }
                if name.as_ref().is_some_and(|name| name.len() > 128) {
                    return Err("Agent name is too long".into());
                }
                let project_index = if let Some(index) = self
                    .projects
                    .iter()
                    .position(|existing| existing.display_path() == project)
                {
                    index
                } else {
                    let (path, ssh_host) = match crate::remote::parse_project(&project)? {
                        Some(remote) => (remote.path, Some(remote.host)),
                        None => {
                            let path = PathBuf::from(&project);
                            if !path.is_absolute() {
                                return Err("Enter an absolute project path".into());
                            }
                            let path = path
                                .canonicalize()
                                .map_err(|_| "Project folder does not exist".to_string())?;
                            if !path.is_dir() {
                                return Err("Project path is not a folder".into());
                            }
                            (path, None)
                        }
                    };
                    if let Some(index) = self
                        .projects
                        .iter()
                        .position(|existing| existing.path == path && existing.ssh_host == ssh_host)
                    {
                        index
                    } else {
                        self.projects.push(ProjectView {
                            branch: ssh_host.clone().unwrap_or_else(|| branch(&path)),
                            path: path.clone(),
                            ssh_host: ssh_host.clone(),
                            agents: Vec::new(),
                        });
                        self.folder_search.add_recent(match &ssh_host {
                            Some(host) => PathBuf::from(crate::remote::project_label(host, &path)),
                            None => path,
                        });
                        self.projects.len() - 1
                    }
                };
                let (agent_id, _) = self.create_agent_for_project(project_index, command, name, cx);
                return Ok(Response::SessionCreated { agent_id });
            }
            RemoteCommand::Prompt { agent_id, text } => {
                if text.trim().is_empty() {
                    return Err("Prompt is empty".into());
                }
                let Some(agent) = self.mobile_agent_mut(agent_id) else {
                    return Err("Session not found".into());
                };
                if agent.session_id.is_none() {
                    return Err("Agent is still connecting".into());
                }
                if agent.active_work || !agent.config.pending_prompts.is_empty() {
                    agent.config.pending_prompts.push(text.clone());
                } else if let Err(error) = agent.start_prompt(text.clone()) {
                    agent.status = Status::Error;
                    agent.log(Role::System, error);
                    cx.notify();
                    return Err("Could not send prompt".into());
                }
                agent.config.prompt_history.push(text);
                self.dirty = true;
                cx.notify();
            }
            RemoteCommand::Cancel { agent_id } => {
                let Some(agent) = self.mobile_agent_mut(agent_id) else {
                    return Err("Session not found".into());
                };
                if !agent.active_work || agent.cancel_requested {
                    return Err("Agent is not working".into());
                }
                let Some(session_id) = &agent.session_id else {
                    return Err("Agent is still connecting".into());
                };
                match agent.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session_id}})) {
                    Ok(()) => agent.cancel_requested = true,
                    Err(error) => {
                        agent.log(Role::System, format!("Could not stop agent: {error}"));
                        return Err("Could not stop agent".into());
                    }
                }
                cx.notify();
            }
            RemoteCommand::Permission {
                agent_id,
                request_id,
                option_id,
            } => {
                let Some(agent) = self.mobile_agent_mut(agent_id) else {
                    return Err("Session not found".into());
                };
                let Some(index) = agent.permissions.iter().position(|permission| {
                    permission.request_id.to_string() == request_id
                        && permission.options.iter().any(|(id, _)| *id == option_id)
                }) else {
                    return Err("Permission request is no longer pending".into());
                };
                let permission = agent.permissions.remove(index);
                if let Err(error) =
                    agent.send(json!({"jsonrpc":"2.0","id":permission.request_id,"result":{
                        "outcome":{"outcome":"selected","optionId":option_id}
                    }}))
                {
                    agent.status = Status::Error;
                    agent.log(Role::System, error);
                    return Err("Could not answer permission request".into());
                }
                cx.notify();
            }
        }
        Ok(Response::Accepted)
    }
}
