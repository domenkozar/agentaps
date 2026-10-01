use super::*;
use crate::diff_view;
use gpui_kit::ExternalPaths;
use gpui_kit::component::attachment::{
    Attachment, AttachmentContent, AttachmentGroup, AttachmentMedia, AttachmentTitle,
};
use gpui_kit::component::progress::Progress;

impl Workspace {
    pub(in crate::app) fn render_conversation(
        &self,
        mut chat: Div,
        project_index: usize,
        agent_index: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let palette = theme::palette(cx);
        let project = &self.projects[project_index];
        let agent = &project.agents[agent_index];
        let diff_counts = (self.pane_id == self.pane_layout.focused)
            .then_some(self.diff.counts)
            .flatten();
        let (added, removed) = diff_counts.unwrap_or_default();
        let available_agents = self.picker.available_agents.clone();
        let current_command = agent.config.command.clone();
        let current_name = agent.name.clone();
        let pane_id = self.pane_id;
        let agent_id = agent.config.id;
        let view = cx.entity().clone();
        let agent_selector = div().flex().flex_shrink_0().items_center().child(
            Button::new(format!("agent-select-{}", agent.config.id))
                .ghost()
                .compact()
                .label(agent.name.clone())
                .icon(IconName::ChevronDown)
                .dropdown_menu(move |mut menu, _, _| {
                    menu = menu.item(PopupMenuItem::label("Start a new session with"));
                    if !available_agents
                        .iter()
                        .any(|choice| choice.command == current_command)
                    {
                        menu = menu.item(
                            PopupMenuItem::new(current_name.clone())
                                .checked(true)
                                .disabled(true),
                        );
                    }
                    for choice in &available_agents {
                        let current = choice.command == current_command;
                        let duplicate_name = available_agents
                            .iter()
                            .filter(|other| other.name == choice.name)
                            .count()
                            > 1;
                        let label = if duplicate_name {
                            format!("{} ({})", choice.name, choice.command[0])
                        } else {
                            choice.name.clone()
                        };
                        let command = choice.command.clone();
                        let name = choice.name.clone();
                        let view = view.clone();
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .checked(current)
                                .disabled(current)
                                .on_click(move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        if !this.activate_session_pane(pane_id, agent_id, cx) {
                                            return;
                                        }
                                        this.start_agent_for_project(
                                            project_index,
                                            command.clone(),
                                            Some(name.clone()),
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        );
                    }
                    let view = view.clone();
                    menu.item(PopupMenuItem::separator())
                        .item(
                            PopupMenuItem::new("Choose agent or custom command…").on_click(
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        if !this.activate_session_pane(pane_id, agent_id, cx) {
                                            return;
                                        }
                                        this.open_picker(
                                            PickerStep::Agents { project_index },
                                            window,
                                            cx,
                                        );
                                    });
                                },
                            ),
                        )
                        .min_w(px(180.))
                        .max_h(px(320.))
                        .scrollable(true)
                }),
        );
        let location = project.display_path();
        let can_reset = agent
            .context
            .is_some_and(|(used, size)| used > 0 && size > 0);
        let agent_controls = div()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .items_center()
            .gap_2()
            .child(status_badge(agent, palette))
            .child(agent_selector)
            .child({
                let agent_id = agent.config.id;
                if let Some(rename) = self
                    .conversation
                    .renaming
                    .as_ref()
                    .filter(|rename| rename.agent_id == agent_id)
                {
                    div()
                        .id(("session-title-input", agent_id))
                        .w(px(240.))
                        .min_w(px(0.))
                        .child(Input::new(&rename.input))
                } else {
                    div()
                        .id(("session-title", agent_id))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(gpui_kit::rgba(0))
                        .min_w(px(0.))
                        .max_w(px(360.))
                        .truncate()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .text_color(palette.color(TEXT))
                        .cursor(gpui_kit::CursorStyle::IBeam)
                        .hover(move |style| {
                            style
                                .bg(palette.color(SURFACE))
                                .border_color(palette.color(BORDER))
                        })
                        .child(
                            agent
                                .config
                                .session_title()
                                .unwrap_or("Name session…")
                                .to_owned(),
                        )
                        .tooltip(|window, cx| Tooltip::new("Rename session").build(window, cx))
                        .on_click(self.pane_listener(cx, move |this, _, window, cx| {
                            this.open_rename_session(agent_id, window, cx);
                        }))
                }
            });
        let pane_id = self.pane_id;
        let agent_id = agent.config.id;
        let menu_view = cx.entity().clone();
        let session_menu = Button::new(format!("session-menu-{}", agent.config.id))
            .ghost()
            .compact()
            .icon(IconName::Ellipsis)
            .tooltip("Session and pane")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
                let rename_view = menu_view.clone();
                let reset_view = menu_view.clone();
                let agent_view = menu_view.clone();
                let archive_view = menu_view.clone();
                let menu = menu
                    .item(
                        PopupMenuItem::new("Rename session…").on_click(move |_, window, cx| {
                            rename_view.update(cx, |this, cx| {
                                if !this.activate_session_pane(pane_id, agent_id, cx) {
                                    return;
                                }
                                this.open_rename_session(agent_id, window, cx);
                            });
                        }),
                    )
                    .item(
                        PopupMenuItem::new("Reset context")
                            .disabled(!can_reset)
                            .on_click(move |_, _, cx| {
                                reset_view.update(cx, |this, cx| {
                                    if !this.activate_session_pane(pane_id, agent_id, cx) {
                                        return;
                                    }
                                    this.reset_context(project_index, agent_index, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("New session with another agent…").on_click(
                            move |_, window, cx| {
                                agent_view.update(cx, |this, cx| {
                                    if !this.activate_session_pane(pane_id, agent_id, cx) {
                                        return;
                                    }
                                    this.open_picker(
                                        PickerStep::Agents { project_index },
                                        window,
                                        cx,
                                    );
                                });
                            },
                        ),
                    )
                    .item(PopupMenuItem::separator())
                    .item(
                        PopupMenuItem::new("Archive session").on_click(move |_, window, cx| {
                            archive_view.update(cx, |this, cx| {
                                if !this.activate_session_pane(pane_id, agent_id, cx) {
                                    return;
                                }
                                this.set_archived(project_index, agent_index, true, window, cx);
                            });
                        }),
                    )
                    .min_w(px(220.));
                Self::pane_menu_items(
                    menu.item(PopupMenuItem::separator()),
                    pane_id,
                    &menu_view,
                    cx,
                )
            });
        let project_controls = div()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .items_center()
            .justify_end()
            .gap_2()
            .child(
                div()
                    .id("project-path")
                    .min_w(px(0.))
                    .truncate()
                    .text_xs()
                    .text_color(palette.color(MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(palette.color(TEXT)))
                    .child(location.clone())
                    .tooltip(move |window, cx| {
                        Tooltip::new(format!("Change folder: {location}")).build(window, cx)
                    })
                    .on_click(self.pane_listener(cx, move |this, _, window, cx| {
                        this.open_change_folder(
                            SessionLocation {
                                project_index,
                                agent_index,
                            },
                            window,
                            cx,
                        );
                    })),
            )
            .child(
                div()
                    .id("open-diff")
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(palette.color(BORDER))
                    .text_sm()
                    .text_color(palette.color(TEXT))
                    .cursor_pointer()
                    .hover(|style| style.bg(palette.color(HOVER)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(if self.diff.visible {
                                "Hide diff"
                            } else {
                                "Diff"
                            })
                            .when(diff_counts.is_some(), |element| {
                                element
                                    .child(
                                        div()
                                            .text_color(palette.color(diff_view::ADDED_TEXT))
                                            .child(format!("+{added}")),
                                    )
                                    .child(
                                        div()
                                            .text_color(palette.color(diff_view::REMOVED_TEXT))
                                            .child(format!("-{removed}")),
                                    )
                            }),
                    )
                    .on_click(self.pane_listener(cx, move |this, _, _, cx| {
                        if this.diff.visible {
                            this.close_diff();
                            cx.notify();
                        } else {
                            this.open_diff(project_index, cx);
                        }
                    })),
            )
            .child(session_menu);
        chat = chat.child(
            div()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(palette.color(BORDER))
                .flex()
                .items_center()
                .gap_3()
                .child(agent_controls)
                .child(project_controls),
        );
        let pane_id = self.pane_id;
        let agent_id = agent.config.id;
        let view = cx.entity().clone();
        let rows = self.conversation.chat_rows.clone();
        let history = gpui_kit::list(
            self.conversation.chat_list.clone(),
            move |index, window, cx| {
                view.update(cx, |this, cx| {
                    this.with_pane(pane_id, cx, |this, cx| {
                        let Some(session) = this.session_location(agent_id) else {
                            return div().into_any_element();
                        };
                        if this.view.displayed_session() != Some(session) {
                            return div().into_any_element();
                        }
                        let project_index = session.project_index;
                        let agent_index = session.agent_index;
                        let agent = &this.projects[project_index].agents[agent_index];
                        this.render_chat_row(
                            agent,
                            project_index,
                            agent_index,
                            rows[index].kind,
                            window,
                            cx,
                        )
                        .into_any_element()
                    })
                    .unwrap_or_else(|| div().into_any_element())
                })
            },
        )
        .w_full()
        .h_full()
        .py_4();
        chat = chat.child(
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .relative()
                .child(div().id("chat-scroll").size_full().px_4().child(history))
                .vertical_scrollbar(&self.conversation.chat_list),
        );
        let file_results = self.file_results(cx);
        if self.file_mention_active(cx) {
            let mut menu = div()
                .w_full()
                .max_w(px(CHAT_COLUMN_WIDTH))
                .mb_1()
                .p_1()
                .rounded_md()
                .border_1()
                .border_color(palette.color(BORDER))
                .bg(palette.color(SURFACE))
                .flex()
                .flex_col();
            if file_results.is_empty() {
                menu = menu.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(palette.color(MUTED))
                        .child(
                            if self
                                .conversation
                                .file_search
                                .as_ref()
                                .is_some_and(FileSearch::loading)
                            {
                                "Looking for files…"
                            } else {
                                "No matching files"
                            },
                        ),
                );
            }
            for (index, file) in file_results.into_iter().enumerate() {
                let name = file.clone();
                menu = menu.child(
                    div()
                        .id(("file-mention", index))
                        .cursor_pointer()
                        .rounded_md()
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_3()
                        .when(index == self.conversation.file_selection, |row| {
                            row.bg(palette.color(SELECTED))
                        })
                        .hover(|style| style.bg(palette.color(SELECTED)))
                        .child(
                            Icon::new(if file.ends_with('/') {
                                IconName::Folder
                            } else {
                                IconName::File
                            })
                            .size(px(16.))
                            .text_color(palette.color(ACCENT)),
                        )
                        .child(div().min_w(px(0.)).truncate().text_sm().child(name))
                        .on_click(self.pane_listener(cx, move |this, _, window, cx| {
                            this.complete_file(&file, window, cx);
                        })),
                );
            }
            chat = chat.child(div().px_4().flex().justify_center().child(menu));
        }
        let slash_commands = self.slash_results(cx);
        if !slash_commands.is_empty() {
            let mut menu = div()
                .w_full()
                .max_w(px(CHAT_COLUMN_WIDTH))
                .mb_1()
                .p_1()
                .rounded_md()
                .border_1()
                .border_color(palette.color(BORDER))
                .bg(palette.color(SURFACE))
                .flex()
                .flex_col();
            for (index, command) in slash_commands.into_iter().enumerate() {
                let selected = index == self.conversation.slash_selection;
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
                        .when(selected, |row| row.bg(palette.color(SELECTED)))
                        .hover(|style| style.bg(palette.color(SELECTED)))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_sm()
                                .text_color(palette.color(ACCENT))
                                .child(name),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_xs()
                                .text_color(palette.color(MUTED))
                                .child(description),
                        )
                        .when_some(hint, |row, hint| {
                            row.child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(palette.color(MUTED))
                                    .child(hint),
                            )
                        })
                        .on_click(self.pane_listener(cx, move |this, _, window, cx| {
                            this.complete_slash_command(command.clone(), window, cx);
                        })),
                );
            }
            chat = chat.child(div().px_4().flex().justify_center().child(menu));
        }
        let shell_mode = self.conversation.composer.read(cx).value().starts_with('!');
        let agent_id = agent.config.id;
        let draft_images = self
            .draft_images
            .get(&agent_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let draft_files = self
            .draft_files
            .get(&agent_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let can_send = !draft_images.is_empty()
            || !draft_files.is_empty()
            || !self
                .conversation
                .composer
                .read(cx)
                .value()
                .trim()
                .is_empty();
        let focused = self
            .conversation
            .composer
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let action_button = if agent.active_work {
            let stopping = agent.cancel_requested;
            div()
                .id("stop-agent")
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(palette.color(BORDER))
                .bg(palette.color(SURFACE))
                .text_xs()
                .text_color(palette.color(if stopping { MUTED } else { TEXT }))
                .child(
                    div()
                        .size(px(8.))
                        .rounded_sm()
                        .bg(palette.color(if stopping { MUTED } else { TEXT })),
                )
                .child(if stopping { "Stopping…" } else { "Stop" })
                .tooltip(|window, cx| Tooltip::new("Stop agent (Esc)").build(window, cx))
                .when(!stopping, |button| {
                    button
                        .cursor_pointer()
                        .hover(|style| style.bg(palette.color(HOVER)))
                        .on_click(self.pane_listener(cx, |this, _, _, cx| this.cancel_prompt(cx)))
                })
        } else {
            div()
                .id("send-prompt")
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .bg(palette.color(if can_send { ACCENT_SURFACE } else { SURFACE }))
                .child(
                    Icon::new(IconName::ArrowUp)
                        .size(px(14.))
                        .text_color(palette.color(if can_send { TEXT } else { MUTED })),
                )
                .tooltip(|window, cx| Tooltip::new("Send (Enter)").build(window, cx))
                .when(can_send, |button| {
                    button
                        .cursor_pointer()
                        .hover(|style| style.bg(palette.color(SELECTED)))
                        .on_click(
                            self.pane_listener(cx, |this, _, window, cx| {
                                this.send_prompt(window, cx)
                            }),
                        )
                })
        };
        let workspace = cx.entity().downgrade();
        let attachments = (!draft_images.is_empty() || !draft_files.is_empty()).then(|| {
            AttachmentGroup::new(("composer-images", agent_id))
                .children(draft_images.iter().enumerate().map(|(index, image)| {
                    Attachment::new()
                        .id(("composer-image", index))
                        .axis(gpui_kit::Axis::Vertical)
                        .media(AttachmentMedia::new().src(self.images.path(image)))
                        .on_remove(self.pane_listener(cx, move |this, _, _, cx| {
                            this.remove_draft_image(agent_id, index, cx);
                        }))
                }))
                .children(draft_files.iter().enumerate().map(|(index, file)| {
                    Attachment::new()
                        .id(("composer-file", index))
                        .tooltip(file.uri.clone())
                        .media(AttachmentMedia::new().child(Icon::new(IconName::File)))
                        .content(
                            AttachmentContent::new().title(AttachmentTitle::new(file.name.clone())),
                        )
                        .on_remove(self.pane_listener(cx, move |this, _, _, cx| {
                            this.remove_draft_file(agent_id, index, cx);
                        }))
                }))
        });
        // A plain border that turns to the accent colour on focus, in place
        // of the text area's own focus ring.
        let composer = div()
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap_1()
            .rounded_lg()
            .border_1()
            .border_color(palette.color(if focused { ACCENT } else { BORDER }))
            .bg(
                if self.theme_choice == crate::appearance::Choice::Agentaps {
                    palette.color(SIDEBAR)
                } else {
                    palette.input
                },
            )
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style.border_color(palette.color(ACCENT))
            })
            .on_drop(
                self.pane_listener(cx, |this, paths: &ExternalPaths, _, cx| {
                    this.drop_paths(paths, cx);
                }),
            )
            .children(attachments.map(|attachments| div().px_2().pt_2().child(attachments)))
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .pr_1p5()
                    .py_1p5()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .relative()
                            .child(
                                Textarea::new(&self.conversation.composer)
                                    .appearance(false)
                                    .when(shell_mode, |textarea| textarea.pr(px(48.)))
                                    .on_paste(move |item, _, cx| {
                                        workspace
                                            .update(cx, |this, cx| {
                                                if !this
                                                    .activate_session_pane(pane_id, agent_id, cx)
                                                {
                                                    return false;
                                                }
                                                this.paste_images(item, cx)
                                            })
                                            .unwrap_or(false)
                                    }),
                            )
                            .when(shell_mode, |element| {
                                element.child(
                                    div()
                                        .absolute()
                                        .right(px(12.))
                                        .bottom(px(6.))
                                        .text_xs()
                                        .text_color(palette.color(MUTED))
                                        .child("shell"),
                                )
                            }),
                    )
                    .child(div().flex_shrink_0().child(action_button)),
            );
        let status = agent.active_work.then(|| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(status_dot(Status::Working.color(), palette))
                .child(if agent.cancel_requested {
                    "Stopping agent"
                } else if agent.awaiting_response {
                    "Waiting for agent · Esc to stop"
                } else {
                    "Agent is working · Esc to stop"
                })
        });
        chat = chat.child(
            div().px_4().pb_3().flex().justify_center().child(
                div()
                    .w_full()
                    .max_w(px(CHAT_COLUMN_WIDTH))
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(composer)
                    .child(
                        div()
                            .min_h(px(24.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(palette.color(MUTED))
                            .child(self.render_attach_menu(project_index, agent_index, cx))
                            .children(self.render_mode_select(project_index, agent_index, cx))
                            .children(self.render_plan_toggle(project_index, agent_index, cx))
                            .children(status)
                            .child(div().flex_1())
                            .child(self.render_session_settings(project_index, agent_index, cx)),
                    ),
            ),
        );
        chat
    }

    /// Actions that add content to the prompt, starting with files and images.
    fn render_attach_menu(
        &self,
        project_index: usize,
        agent_index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let agent = &self.projects[project_index].agents[agent_index];
        let connected = agent.protocol.is_some() && agent.status != Status::Error;
        let label = if agent.accepts_images {
            "Add file or image…"
        } else {
            "Add file…"
        };
        let pane_id = self.pane_id;
        let agent_id = agent.config.id;
        let view = cx.entity().clone();
        Button::new(format!("attach-menu-{}", agent.config.id))
            .ghost()
            .xsmall()
            .icon(IconName::Plus)
            .tooltip(if connected {
                "Add to prompt"
            } else {
                "Wait for the agent to connect"
            })
            .disabled(!connected)
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                let view = view.clone();
                menu.item(PopupMenuItem::new(label).icon(IconName::File).on_click(
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            if this.activate_session_pane(pane_id, agent_id, cx) {
                                this.choose_attachments(window, cx);
                            }
                        });
                    },
                ))
                .min_w(px(160.))
            })
    }

    /// The agent's session mode, shown at the bottom left of the composer
    /// with the agent's own mode names. Hidden when the agent reports none.
    fn render_mode_select(
        &self,
        project_index: usize,
        agent_index: usize,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let agent = &self.projects[project_index].agents[agent_index];
        let option = agent.mode_option.as_ref()?;
        if agent.session_id.is_none() || agent.status == Status::Error || option.choices.is_empty()
        {
            return None;
        }
        let pending = agent.setting_pending();
        let choices = option.choices.clone();
        let current = option.current.clone();
        let pane_id = self.pane_id;
        let agent_id = agent.config.id;
        let view = cx.entity().clone();
        Some(
            Button::new(format!("mode-select-{}", agent.config.id))
                .ghost()
                .xsmall()
                .label(option.label())
                .icon(IconName::ChevronDown)
                .tooltip("Mode")
                .disabled(pending)
                .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, _, _| {
                    for choice in &choices {
                        let value = choice.value.clone();
                        let view = view.clone();
                        menu = menu.item(
                            PopupMenuItem::new(choice.label.clone())
                                .checked(choice.value == current)
                                .on_click(move |_, _, cx| {
                                    view.update(cx, |this, cx| {
                                        if !this.activate_session_pane(pane_id, agent_id, cx) {
                                            return;
                                        }
                                        this.select_config_option(
                                            project_index,
                                            agent_index,
                                            ConfigOptionKind::Mode,
                                            value.clone(),
                                            cx,
                                        );
                                    });
                                }),
                        );
                    }
                    menu.min_w(px(160.)).max_h(px(320.)).scrollable(true)
                }),
        )
    }

    /// Switches planning on and off for agents that report it as a
    /// collaboration mode, such as Codex. A small switch shows the state:
    /// muted and empty when off, accent and filled when on.
    fn render_plan_toggle(
        &self,
        project_index: usize,
        agent_index: usize,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let palette = theme::palette(cx);
        let agent = &self.projects[project_index].agents[agent_index];
        if agent.session_id.is_none() || agent.status == Status::Error {
            return None;
        }
        let plan = agent.plan_toggle()?;
        let active = plan.active;
        let pending = agent.setting_pending();
        let track = div()
            .w(px(20.))
            .h(px(12.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px(px(2.))
            .rounded_full()
            .border_1()
            .map(|track| {
                if active {
                    track
                        .justify_end()
                        .bg(palette.color(ACCENT))
                        .border_color(palette.color(ACCENT))
                } else {
                    track.justify_start().border_color(palette.color(MUTED))
                }
            })
            .child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(palette.color(if active { BG } else { MUTED })),
            );
        let pane_id = self.pane_id;
        let agent_id = agent.config.id;
        let view = cx.entity().clone();
        // The label and switch explain themselves, so there is no tooltip.
        // Assistive technology hears "Plan, switch" with its on/off state.
        Some(
            gpui_kit::base::Switch::new(("plan-toggle", agent.config.id as usize))
                .checked(active)
                .disabled(pending)
                .accessibility_label("Plan")
                .flex()
                .items_center()
                .gap_1p5()
                .px_1p5()
                .py_0p5()
                .rounded_md()
                .text_xs()
                .text_color(palette.color(if active { ACCENT } else { MUTED }))
                .child(track)
                .child("Plan")
                .map(|toggle| {
                    if pending {
                        toggle.opacity(0.5)
                    } else {
                        toggle.cursor_pointer().hover(|style| {
                            style
                                .bg(palette.color(HOVER))
                                .text_color(palette.color(if active { ACCENT } else { TEXT }))
                        })
                    }
                })
                .on_change(move |_, event, window, cx| {
                    let value = plan.next.clone();
                    view.update(cx, |this, cx| {
                        if !this.activate_session_pane(pane_id, agent_id, cx) {
                            return;
                        }
                        this.select_config_option(
                            project_index,
                            agent_index,
                            ConfigOptionKind::Collaboration,
                            value,
                            cx,
                        );
                        // A mouse click returns to typing; keyboard users keep
                        // their place in the tab order.
                        if !event.is_keyboard() {
                            this.conversation
                                .composer
                                .update(cx, |input, cx| input.focus(window, cx));
                        }
                    });
                }),
        )
    }

    /// Model, effort and context usage, shown under the composer. Only the
    /// options the agent reports are shown.
    fn render_session_settings(
        &self,
        project_index: usize,
        agent_index: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let palette = theme::palette(cx);
        let agent = &self.projects[project_index].agents[agent_index];
        let selectable = agent.session_id.is_some() && agent.status != Status::Error;
        let pending = agent.setting_pending();
        let mut row = div().flex().min_w(px(0.)).items_center().gap_1();
        if let Some(model) = agent.model.as_ref() {
            row = match &agent.model_option {
                Some(option) if selectable && !option.choices.is_empty() => {
                    let choices = option.choices.clone();
                    let current = option.current.clone();
                    let pane_id = self.pane_id;
                    let agent_id = agent.config.id;
                    let view = cx.entity().clone();
                    row.child(
                        Button::new(format!("model-select-{}", agent.config.id))
                            .ghost()
                            .xsmall()
                            .label(model.clone())
                            .icon(IconName::ChevronDown)
                            .tooltip("Model")
                            .disabled(pending)
                            .dropdown_menu_with_anchor(
                                Anchor::BottomLeft,
                                move |mut menu, _, _| {
                                    for choice in &choices {
                                        let value = choice.value.clone();
                                        let view = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(choice.label.clone())
                                                .checked(choice.value == current)
                                                .on_click(move |_, _, cx| {
                                                    view.update(cx, |this, cx| {
                                                        if !this.activate_session_pane(
                                                            pane_id, agent_id, cx,
                                                        ) {
                                                            return;
                                                        }
                                                        this.select_config_option(
                                                            project_index,
                                                            agent_index,
                                                            ConfigOptionKind::Model,
                                                            value.clone(),
                                                            cx,
                                                        );
                                                    });
                                                }),
                                        );
                                    }
                                    menu.min_w(px(180.)).max_h(px(320.)).scrollable(true)
                                },
                            ),
                    )
                }
                _ => row.child(div().px_1().truncate().child(model.clone())),
            };
        }
        if let Some(option) = agent.effort_option.as_ref()
            && selectable
            && !option.choices.is_empty()
        {
            let choices = option.choices.clone();
            let current = option.current.clone();
            let pane_id = self.pane_id;
            let agent_id = agent.config.id;
            let view = cx.entity().clone();
            row = row.child(
                Button::new(format!("effort-select-{}", agent.config.id))
                    .ghost()
                    .xsmall()
                    .label(option.label())
                    .icon(IconName::ChevronDown)
                    .tooltip("Reasoning effort")
                    .disabled(pending)
                    .dropdown_menu_with_anchor(Anchor::BottomRight, move |mut menu, _, _| {
                        for choice in &choices {
                            let value = choice.value.clone();
                            let view = view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(choice.label.clone())
                                    .checked(choice.value == current)
                                    .on_click(move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            if !this.activate_session_pane(pane_id, agent_id, cx) {
                                                return;
                                            }
                                            this.select_config_option(
                                                project_index,
                                                agent_index,
                                                ConfigOptionKind::Effort,
                                                value.clone(),
                                                cx,
                                            );
                                        });
                                    }),
                            );
                        }
                        menu.min_w(px(140.)).max_h(px(320.)).scrollable(true)
                    }),
            );
        }
        if let Some((used, size)) = agent.context
            && size > 0
        {
            let percent = ((used as f64 / size as f64) * 100.).clamp(0., 100.) as f32;
            let color = if percent >= 85. {
                STATUS_ERROR
            } else if percent >= 60. {
                STATUS_WORKING
            } else {
                STATUS_DONE
            };
            let usage = format!(
                "{} of {} tokens used",
                compact_tokens(used),
                compact_tokens(size)
            );
            let can_reset = used > 0;
            let pane_id = self.pane_id;
            let agent_id = agent.config.id;
            let view = cx.entity().clone();
            row = row.child(
                Button::new(format!("context-{}", agent.config.id))
                    .ghost()
                    .xsmall()
                    .tooltip(usage.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(format!("Context {}%", percent.floor() as u32))
                            .child(
                                Progress::new(format!("context-progress-{}", agent.config.id))
                                    .value(percent)
                                    .color(palette.color(color))
                                    .accessibility_label("Context usage")
                                    .w(px(80.))
                                    .h(px(6.)),
                            ),
                    )
                    .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                        let view = view.clone();
                        menu.item(PopupMenuItem::label(usage.clone()))
                            .item(PopupMenuItem::separator())
                            .item(
                                PopupMenuItem::new("Reset context")
                                    .disabled(!can_reset)
                                    .on_click(move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            if !this.activate_session_pane(pane_id, agent_id, cx) {
                                                return;
                                            }
                                            this.reset_context(project_index, agent_index, cx);
                                        });
                                    }),
                            )
                            .min_w(px(200.))
                    }),
            );
        }
        row
    }
}
