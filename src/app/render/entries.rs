use super::*;

impl Workspace {
    pub(super) fn render_entries(
        &self,
        agent: &AgentView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut entries = div().w_full().min_w(px(0.)).flex().flex_col().gap_2().p_4();
        if agent.messages.is_empty() {
            entries = entries.child(div().mt_8().text_center().text_color(rgb(MUTED)).child(
                if agent.status == Status::Connecting {
                    "Connecting to agent…"
                } else {
                    "Ask the agent to work on this project."
                },
            ));
        }
        let text_style = TextViewStyle {
            paragraph_gap: rems(0.25),
            highlight_theme: cx.theme().highlight_theme.clone(),
            is_dark: true,
            ..Default::default()
        };
        let mut message_index = 0;
        while message_index < agent.messages.len() {
            let entry = &agent.messages[message_index];
            if entry.role == Role::Tool {
                let end = tool_run_end(&agent.messages, message_index);
                entries = entries.child(self.render_tool_group(
                    agent,
                    message_index,
                    end,
                    text_style.clone(),
                    window,
                    cx,
                ));
                message_index = end;
                continue;
            }
            if entry.text.is_empty() {
                message_index += 1;
                continue;
            }
            let text_id: gpui::ElementId = ("chat", agent.config.id).into();
            let content = TextView::markdown(
                (text_id, message_index.to_string()),
                entry.text.clone(),
                window,
                cx,
            )
            .style(text_style.clone())
            .selectable(true)
            .text_sm()
            .text_color(rgb(TEXT));
            match entry.role {
                Role::User | Role::Agent => {
                    let from_user = matches!(entry.role, Role::User);
                    entries = entries.child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .flex()
                            .when(from_user, |element| element.justify_end())
                            .child(
                                div()
                                    .max_w(relative(0.85))
                                    .min_w(px(0.))
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(rgb(if from_user { USER_BUBBLE } else { AGENT_BUBBLE }))
                                    .text_sm()
                                    .text_color(rgb(TEXT))
                                    .whitespace_normal()
                                    .child(content),
                            ),
                    );
                }
                role => {
                    let (label, color) = match role {
                        Role::Thought => ("THOUGHT", MUTED),
                        Role::Tool => ("TOOL", TOOL_MARKER),
                        Role::System => ("SYSTEM", STATUS_ERROR),
                        _ => unreachable!(),
                    };
                    entries = entries.child(
                        div()
                            .max_w(px(900.))
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(SURFACE))
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(color))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_sm()
                                    .text_color(rgb(TEXT))
                                    .whitespace_normal()
                                    .child(content),
                            ),
                    );
                }
            }
            message_index += 1;
        }
        for (index, prompt) in agent.config.pending_prompts.iter().enumerate() {
            entries = entries.child(
                div().w_full().min_w(px(0.)).flex().justify_end().child(
                    div()
                        .max_w(relative(0.85))
                        .min_w(px(0.))
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(USER_BUBBLE))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(ACCENT))
                                .child(format!("Queued {}", index + 1)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(TEXT))
                                .whitespace_normal()
                                .child(prompt.clone()),
                        ),
                ),
            );
        }
        for (permission_index, permission) in agent.permissions.iter().enumerate() {
            let mut choices = div().flex().gap_2().mt_3();
            for (option_index, (option_id, label)) in permission.options.iter().enumerate() {
                let option_id = option_id.clone();
                choices = choices.child(
                    div()
                        .id((
                            "permission",
                            ((permission_index as u64) << 32) | option_index as u64,
                        ))
                        .cursor_pointer()
                        .rounded_md()
                        .bg(rgb(ACCENT_SURFACE))
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(rgb(TEXT))
                        .child(label.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_permission(permission_index, option_id.clone(), cx)
                        })),
                );
            }
            entries = entries.child(
                div()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(PERMISSION_BORDER))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(TEXT))
                            .child(permission.title.clone()),
                    )
                    .when_some(permission.description.as_ref(), |element, description| {
                        element.child(
                            div()
                                .mt_2()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(description.clone()),
                        )
                    })
                    .child(choices),
            );
        }
        entries
    }
}
