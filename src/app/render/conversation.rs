use super::*;

impl Workspace {
    pub(super) fn render_conversation(
        &self,
        mut chat: Div,
        project_index: usize,
        agent_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
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
        let entries = self.render_entries(agent, window, cx);
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
                        .child(Input::new(&self.composer)),
                ),
        );
        chat
    }
}
