use super::*;
use std::hash::{Hash, Hasher};
use std::ops::Range;

// Message text is either replaced with a new String or changed in length when updated.
// These signatures let scroll renders check for changed rows without hashing whole transcripts.
fn text_signature(text: &str, hasher: &mut impl Hasher) {
    (text.as_ptr() as usize).hash(hasher);
    text.len().hash(hasher);
}

fn chat_rows(
    agent: &AgentView,
    collapsed_tool_groups: &HashSet<(u64, usize)>,
    expanded_tool_rows: &HashSet<(u64, usize)>,
) -> Vec<ChatRow> {
    let mut rows = Vec::new();
    if agent.messages.is_empty() {
        rows.push(ChatRow {
            kind: ChatRowKind::Empty,
            signature: u64::from(agent.status == Status::Connecting),
        });
    }
    let mut index = 0;
    while index < agent.messages.len() {
        let entry = &agent.messages[index];
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if entry.role == Role::Tool {
            let end = tool_run_end(&agent.messages, index);
            collapsed_tool_groups
                .contains(&(agent.config.id, index))
                .hash(&mut hasher);
            for (offset, tool) in agent.messages[index..end].iter().enumerate() {
                text_signature(&tool.text, &mut hasher);
                expanded_tool_rows
                    .contains(&(agent.config.id, index + offset))
                    .hash(&mut hasher);
            }
            rows.push(ChatRow {
                kind: ChatRowKind::Tools(index, end),
                signature: hasher.finish(),
            });
            index = end;
        } else {
            if !entry.text.is_empty() {
                (entry.role as u8).hash(&mut hasher);
                text_signature(&entry.text, &mut hasher);
                rows.push(ChatRow {
                    kind: ChatRowKind::Message(index),
                    signature: hasher.finish(),
                });
            }
            index += 1;
        }
    }
    for (index, prompt) in agent.config.pending_prompts.iter().enumerate() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text_signature(prompt, &mut hasher);
        rows.push(ChatRow {
            kind: ChatRowKind::Queued(index),
            signature: hasher.finish(),
        });
    }
    for (index, permission) in agent.permissions.iter().enumerate() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text_signature(&permission.title, &mut hasher);
        if let Some(description) = &permission.description {
            text_signature(description, &mut hasher);
        }
        for (id, label) in &permission.options {
            text_signature(id, &mut hasher);
            text_signature(label, &mut hasher);
        }
        rows.push(ChatRow {
            kind: ChatRowKind::Permission(index),
            signature: hasher.finish(),
        });
    }
    for (index, question) in agent.elicitations.iter().enumerate() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text_signature(&question.message, &mut hasher);
        for field in &question.fields {
            text_signature(&field.title, &mut hasher);
            match &field.kind {
                ElicitationFieldKind::Select { selected, .. } => selected.hash(&mut hasher),
                ElicitationFieldKind::MultiSelect { selected, .. } => {
                    let mut selected = selected.iter().collect::<Vec<_>>();
                    selected.sort();
                    selected.hash(&mut hasher);
                }
                ElicitationFieldKind::Boolean(value) => value.hash(&mut hasher),
                ElicitationFieldKind::Input(_) => {}
            }
        }
        question.error.hash(&mut hasher);
        rows.push(ChatRow {
            kind: ChatRowKind::Elicitation(index),
            signature: hasher.finish(),
        });
    }

    rows
}

fn changed_row_range(old: &[ChatRow], new: &[ChatRow]) -> Option<(Range<usize>, usize)> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    (prefix != old_end || prefix != new_end).then_some((prefix..old_end, new_end - prefix))
}

impl Workspace {
    pub(super) fn sync_chat_rows(&mut self, project_index: usize, agent_index: usize) {
        let agent = &self.projects[project_index].agents[agent_index];
        let rows = chat_rows(agent, &self.collapsed_tool_groups, &self.expanded_tool_rows);

        if self.chat_list_agent != Some(agent.config.id) {
            self.chat_list.reset(rows.len());
            self.chat_list_agent = Some(agent.config.id);
        } else if let Some((range, count)) = changed_row_range(&self.chat_rows, &rows) {
            let near_bottom = self.chat_list.scroll_px_offset_for_scrollbar().y
                + self.chat_list.max_offset_for_scrollbar().height
                <= px(24.);
            self.chat_list.splice(range, count);
            if near_bottom {
                self.chat_list.scroll_to(gpui::ListOffset {
                    item_ix: rows.len(),
                    offset_in_item: px(0.),
                });
            }
        }
        self.chat_rows = rows;
    }

    pub(super) fn render_chat_row(
        &self,
        agent: &AgentView,
        row: ChatRowKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let content = match row {
            ChatRowKind::Empty => div().mt_8().text_center().text_color(rgb(MUTED)).child(
                if agent.status == Status::Connecting {
                    "Connecting to agent…"
                } else {
                    "Ask the agent to work on this project."
                },
            ),
            ChatRowKind::Tools(start, end) => {
                self.render_tool_group(agent, start, end, self.chat_text_style(cx), window, cx)
            }
            ChatRowKind::Message(index) => self.render_message(agent, index, window, cx),
            ChatRowKind::Queued(index) => {
                let prompt = &agent.config.pending_prompts[index];
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
                )
            }
            ChatRowKind::Permission(index) => self.render_permission(agent, index, cx),
            ChatRowKind::Elicitation(index) => self.render_elicitation(agent, index, cx),
        };
        div().w_full().min_w(px(0.)).pb_2().child(content)
    }

    fn chat_text_style(&self, cx: &Context<Self>) -> TextViewStyle {
        TextViewStyle {
            paragraph_gap: rems(0.25),
            highlight_theme: cx.theme().highlight_theme.clone(),
            is_dark: true,
            ..Default::default()
        }
    }

    fn render_message(
        &self,
        agent: &AgentView,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let entry = &agent.messages[index];
        let text_id: gpui::ElementId = ("chat", agent.config.id).into();
        let content =
            TextView::markdown((text_id, index.to_string()), entry.text.clone(), window, cx)
                .style(self.chat_text_style(cx))
                .selectable(true)
                .text_sm()
                .text_color(rgb(TEXT));
        match entry.role {
            Role::User | Role::Agent => {
                let from_user = entry.role == Role::User;
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
                    )
            }
            Role::ContextReset => div()
                .w_full()
                .flex()
                .items_center()
                .gap_3()
                .py_2()
                .child(div().flex_1().h(px(1.)).bg(rgb(BORDER)))
                .child(
                    div()
                        .max_w(relative(0.8))
                        .text_center()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .whitespace_normal()
                        .child(entry.text.clone()),
                )
                .child(div().flex_1().h(px(1.)).bg(rgb(BORDER))),
            role => {
                let (label, color) = match role {
                    Role::Thought => ("THOUGHT", MUTED),
                    Role::Tool => ("TOOL", TOOL_MARKER),
                    Role::System => ("SYSTEM", STATUS_ERROR),
                    _ => unreachable!(),
                };
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
                    )
            }
        }
    }

    fn render_permission(
        &self,
        agent: &AgentView,
        permission_index: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let permission = &agent.permissions[permission_index];
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
            .child(choices)
    }

    fn render_elicitation(
        &self,
        agent: &AgentView,
        question_index: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let question = &agent.elicitations[question_index];
        let mut fields = div().flex().flex_col().gap_3().mt_3();
        for (field_index, field) in question.fields.iter().enumerate() {
            let mut row =
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_sm().text_color(rgb(TEXT)).child(format!(
                        "{}{}",
                        field.title,
                        if field.required { " *" } else { "" }
                    )));
            if let Some(description) = &field.description {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(description.clone()),
                );
            }
            match &field.kind {
                ElicitationFieldKind::Input(input) => {
                    row = row.child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .child(Input::new(input)),
                    );
                }
                ElicitationFieldKind::Select { options, selected } => {
                    row = row.child(self.render_elicitation_options(
                        question_index,
                        field_index,
                        options,
                        |index| *selected == Some(index),
                        cx,
                    ));
                }
                ElicitationFieldKind::MultiSelect { options, selected } => {
                    row = row.child(self.render_elicitation_options(
                        question_index,
                        field_index,
                        options,
                        |index| selected.contains(&index),
                        cx,
                    ));
                }
                ElicitationFieldKind::Boolean(value) => {
                    let options = [("false".into(), "No".into()), ("true".into(), "Yes".into())];
                    row = row.child(self.render_elicitation_options(
                        question_index,
                        field_index,
                        &options,
                        |index| *value == Some(index == 1),
                        cx,
                    ));
                }
            }
            fields = fields.child(row);
        }
        div()
            .max_w(px(900.))
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(rgb(STATUS_QUESTION))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(STATUS_QUESTION))
                    .child(format!("QUESTION FROM {}", agent.name)),
            )
            .child(
                div()
                    .mt_1()
                    .text_sm()
                    .text_color(rgb(TEXT))
                    .child(question.message.clone()),
            )
            .child(fields)
            .when_some(question.error.as_ref(), |card, error| {
                card.child(
                    div()
                        .mt_2()
                        .text_sm()
                        .text_color(rgb(STATUS_ERROR))
                        .child(error.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .mt_4()
                    .child(self.elicitation_action_button(
                        question_index,
                        "Answer",
                        "accept",
                        true,
                        cx,
                    ))
                    .child(self.elicitation_action_button(
                        question_index,
                        "Decline",
                        "decline",
                        false,
                        cx,
                    ))
                    .child(self.elicitation_action_button(
                        question_index,
                        "Cancel",
                        "cancel",
                        false,
                        cx,
                    )),
            )
    }

    fn render_elicitation_options(
        &self,
        question_index: usize,
        field_index: usize,
        options: &[(String, String)],
        is_selected: impl Fn(usize) -> bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut choices = div().flex().flex_wrap().gap_2();
        for (option_index, (_, label)) in options.iter().enumerate() {
            choices = choices.child(
                div()
                    .id((
                        "question-option",
                        ((question_index as u64) << 40)
                            | ((field_index as u64) << 20)
                            | option_index as u64,
                    ))
                    .cursor_pointer()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if is_selected(option_index) {
                        ACCENT
                    } else {
                        BORDER
                    }))
                    .bg(rgb(if is_selected(option_index) {
                        ACCENT_SURFACE
                    } else {
                        SURFACE
                    }))
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(rgb(TEXT))
                    .child(label.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_elicitation_option(
                            question_index,
                            field_index,
                            option_index,
                            cx,
                        )
                    })),
            );
        }
        choices
    }

    fn elicitation_action_button(
        &self,
        question_index: usize,
        label: &'static str,
        action: &'static str,
        primary: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id((
                "question-action",
                ((question_index as u64) << 2)
                    | match action {
                        "accept" => 0,
                        "decline" => 1,
                        _ => 2,
                    },
            ))
            .cursor_pointer()
            .rounded_md()
            .px_3()
            .py_2()
            .text_sm()
            .bg(rgb(if primary { ACCENT_SURFACE } else { SURFACE }))
            .text_color(rgb(TEXT))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.answer_elicitation(question_index, action, cx)
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> AgentView {
        AgentView::new(AgentConfig {
            id: 1,
            command: vec!["agent".into()],
            archived: false,
            display_name: None,
            session_id: None,
            model: None,
            context: None,
            messages: Vec::new(),
            available_commands: Vec::new(),
            pending_prompts: Vec::new(),
            was_working: false,
            session_has_activity: false,
        })
    }

    #[test]
    fn chat_rows_group_tools_and_invalidate_only_changed_content() {
        let mut agent = agent();
        agent.log(Role::User, "Question");
        agent.log(Role::Tool, "Search files");
        agent.log(Role::Tool, "Read file");
        agent.log(Role::Agent, "Answer");
        let collapsed = HashSet::new();
        let expanded = HashSet::new();
        let before = chat_rows(&agent, &collapsed, &expanded);
        assert_eq!(before.len(), 3);
        assert_eq!(before[1].kind, ChatRowKind::Tools(1, 3));

        agent.messages[0].text.push('!');
        let after = chat_rows(&agent, &collapsed, &expanded);
        assert_eq!(changed_row_range(&before, &after), Some((0..1, 1)));
        assert_eq!(changed_row_range(&after, &after), None);

        let collapsed = HashSet::from([(agent.config.id, 1)]);
        let collapsed_rows = chat_rows(&agent, &collapsed, &expanded);
        assert_eq!(changed_row_range(&after, &collapsed_rows), Some((1..2, 1)));

        agent.config.pending_prompts.push("Next".into());
        let queued_rows = chat_rows(&agent, &collapsed, &expanded);
        assert_eq!(
            changed_row_range(&collapsed_rows, &queued_rows),
            Some((3..3, 1))
        );
    }
}
