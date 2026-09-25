use super::*;

impl Workspace {
    pub(super) fn render_conversation(
        &self,
        mut chat: Div,
        project_index: usize,
        agent_index: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let project = &self.projects[project_index];
        let agent = &project.agents[agent_index];
        let viewport_width = f32::from(window.viewport_size().width);
        let sidebar_width = (viewport_width * self.sidebar_fraction).max(180.);
        let chat_width = viewport_width - sidebar_width - 6.;
        let compact_header = chat_width < 1050.;
        let stacked_header = chat_width < 560.;
        let available_agents = self.available_agents.clone();
        let current_command = agent.config.command.clone();
        let current_name = agent.name.clone();
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
        let metadata = div()
            .flex()
            .flex_wrap()
            .min_w(px(0.))
            .when(!compact_header, |element| element.max_w(relative(0.6)))
            .items_center()
            .when(compact_header, |element| element.gap_2())
            .when(!compact_header, |element| element.gap_3())
            .when_some(agent.model.as_ref(), |element, model| {
                if let Some(option) = &agent.model_option
                    && !option.choices.is_empty()
                    && agent.session_id.is_some()
                    && agent.status != Status::Error
                {
                    let choices = option.choices.clone();
                    let current = option.current.clone();
                    let pending = agent.pending_model.is_some() || agent.pending_effort.is_some();
                    let view = cx.entity().clone();
                    element.child(
                        Button::new(format!("model-select-{}", agent.config.id))
                            .ghost()
                            .compact()
                            .label(model.clone())
                            .icon(IconName::ChevronDown)
                            .disabled(pending)
                            .dropdown_menu(move |mut menu, _, _| {
                                for choice in &choices {
                                    let value = choice.value.clone();
                                    let view = view.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(choice.label.clone())
                                            .checked(choice.value == current)
                                            .on_click(move |_, _, cx| {
                                                view.update(cx, |this, cx| {
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
                                menu.min_w(px(160.)).max_h(px(320.)).scrollable(true)
                            }),
                    )
                } else {
                    element.child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .child(model.clone()),
                    )
                }
            })
            .when_some(agent.effort_option.as_ref(), |element, option| {
                if option.choices.is_empty()
                    || agent.session_id.is_none()
                    || agent.status == Status::Error
                {
                    return element;
                }
                let choices = option.choices.clone();
                let current = option.current.clone();
                let pending = agent.pending_model.is_some() || agent.pending_effort.is_some();
                let view = cx.entity().clone();
                element.child(
                    Button::new(format!("effort-select-{}", agent.config.id))
                        .ghost()
                        .compact()
                        .tooltip("Reasoning effort")
                        .label(if compact_header {
                            option.label()
                        } else {
                            format!("Effort: {}", option.label())
                        })
                        .icon(IconName::ChevronDown)
                        .disabled(pending)
                        .dropdown_menu(move |mut menu, _, _| {
                            for choice in &choices {
                                let value = choice.value.clone();
                                let view = view.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(choice.label.clone())
                                        .checked(choice.value == current)
                                        .on_click(move |_, _, cx| {
                                            view.update(cx, |this, cx| {
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
                )
            })
            .when_some(agent.context, |element, (used, size)| {
                element.child(
                    div()
                        .id(("context", agent.config.id))
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(if compact_header {
                            format!("{}/{}", compact_tokens(used), compact_tokens(size))
                        } else {
                            format!(
                                "Context {} / {}",
                                compact_tokens(used),
                                compact_tokens(size)
                            )
                        })
                        .tooltip(move |window, cx| {
                            Tooltip::new(format!(
                                "Context {} / {}",
                                compact_tokens(used),
                                compact_tokens(size)
                            ))
                            .build(window, cx)
                        }),
                )
            });
        let reset_context = div()
            .id("reset-context")
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .text_sm()
            .text_color(rgb(TEXT))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(HOVER)))
            .child(
                Icon::new(IconName::Redo2)
                    .size(px(14.))
                    .text_color(rgb(TEXT)),
            )
            .when(!compact_header, |element| element.child("Reset context"))
            .tooltip(|window, cx| {
                Tooltip::new("Start fresh here. Earlier messages stay visible.").build(window, cx)
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.reset_context(project_index, agent_index, cx);
            }));
        let agent_controls = div()
            .flex()
            .flex_wrap()
            .min_w(px(0.))
            .items_center()
            .when(compact_header, |element| element.gap_2())
            .when(!compact_header, |element| element.gap_3())
            .when(!stacked_header, |element| element.flex_1())
            .when(stacked_header, |element| element.w_full())
            .child(agent_selector)
            .when(
                agent.model.is_some() || agent.context.is_some() || agent.effort_option.is_some(),
                |element| element.child(metadata),
            )
            .child(reset_context);
        let project_controls = div()
            .flex()
            .min_w(px(0.))
            .items_center()
            .when(compact_header, |element| element.gap_2())
            .when(!compact_header, |element| element.gap_3())
            .when(stacked_header, |element| element.w_full().justify_end())
            .child(
                div()
                    .id("project-path")
                    .min_w(px(0.))
                    .max_w(px(if stacked_header {
                        180.
                    } else if compact_header {
                        75.
                    } else {
                        300.
                    }))
                    .truncate()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(if compact_header {
                        project
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| project.path.display().to_string())
                    } else {
                        project.path.display().to_string()
                    })
                    .tooltip({
                        let path = project.path.display().to_string();
                        move |window, cx| Tooltip::new(path.clone()).build(window, cx)
                    }),
            )
            .child(
                div()
                    .id("open-diff")
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .text_sm()
                    .text_color(rgb(TEXT))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(HOVER)))
                    .child("Diff")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_diff(project_index, cx);
                    })),
            );
        chat = chat.child(
            div()
                .when(compact_header, |element| element.px_3().py_2())
                .when(!compact_header, |element| element.px_4().py_3())
                .border_b_1()
                .border_color(rgb(BORDER))
                .flex()
                .when(compact_header, |element| element.gap_2())
                .when(!compact_header, |element| element.gap_3())
                .when(stacked_header, |element| element.flex_col())
                .when(!stacked_header, |element| element.items_center())
                .child(agent_controls)
                .child(project_controls),
        );
        let view = cx.entity().clone();
        let rows = self.chat_rows.clone();
        let history = gpui::list(self.chat_list.clone(), move |index, window, cx| {
            view.update(cx, |this, cx| {
                let agent = &this.projects[project_index].agents[agent_index];
                this.render_chat_row(agent, rows[index].kind, window, cx)
                    .into_any_element()
            })
        })
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
                .vertical_scrollbar(&self.chat_list),
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
                                .tooltip(|window, cx| Tooltip::new("Stop agent").build(window, cx))
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
                        .child(Textarea::new(&self.composer)),
                ),
        );
        chat
    }
}
