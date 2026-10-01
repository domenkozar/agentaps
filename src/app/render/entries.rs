use super::*;
use std::hash::{Hash, Hasher};
use std::ops::Range;

#[derive(Default)]
struct CopyFeedback {
    copied: bool,
}

// Message text is either replaced with a new String or changed in length when updated.
// These signatures let scroll renders check for changed rows without hashing whole transcripts.
fn text_signature(text: &str, hasher: &mut impl Hasher) {
    (text.as_ptr() as usize).hash(hasher);
    text.len().hash(hasher);
}

fn chat_rows(
    agent: &AgentView,
    collapsed_tool_groups: &HashSet<(u64, usize)>,
    expanded_tool_history: &HashSet<(u64, usize)>,
    expanded_tool_rows: &HashSet<(u64, usize)>,
    expanded_thought_rows: &HashSet<(u64, usize)>,
) -> Vec<ChatRow> {
    let mut rows = Vec::new();
    let mut index = 0;
    while index < agent.messages.len() {
        let entry = &agent.messages[index];
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if entry.role == Role::Tool {
            let end = tool_run_end(&agent.messages, index);
            if agent.messages[index..end]
                .iter()
                .any(|tool| !is_approval_review(&tool.text))
            {
                collapsed_tool_groups
                    .contains(&(agent.config.id, index))
                    .hash(&mut hasher);
                expanded_tool_history
                    .contains(&(agent.config.id, index))
                    .hash(&mut hasher);
                for (offset, tool) in agent.messages[index..end].iter().enumerate() {
                    if is_approval_review(&tool.text) {
                        continue;
                    }
                    text_signature(&tool.text, &mut hasher);
                    expanded_tool_rows
                        .contains(&(agent.config.id, index + offset))
                        .hash(&mut hasher);
                }
                rows.push(ChatRow {
                    kind: ChatRowKind::Tools(index, end),
                    signature: hasher.finish(),
                });
            }
            index = end;
        } else {
            if !entry.text.is_empty() || !entry.images.is_empty() {
                (entry.role as u8).hash(&mut hasher);
                if entry.role == Role::Thought {
                    let expanded = expanded_thought_rows.contains(&(agent.config.id, index));
                    expanded.hash(&mut hasher);
                    if expanded {
                        text_signature(&entry.text, &mut hasher);
                        entry.images.hash(&mut hasher);
                    }
                } else {
                    text_signature(&entry.text, &mut hasher);
                    entry.images.hash(&mut hasher);
                }
                rows.push(ChatRow {
                    kind: ChatRowKind::Message(index),
                    signature: hasher.finish(),
                });
            }
            index += 1;
        }
    }
    if rows.is_empty() {
        rows.push(ChatRow {
            kind: ChatRowKind::Empty,
            signature: u64::from(agent.status == Status::Connecting),
        });
    }
    for (index, prompt) in agent.config.pending_prompts.iter().enumerate() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text_signature(&prompt.text, &mut hasher);
        prompt.images.hash(&mut hasher);
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
    pub(in crate::app) fn sync_chat_rows(&mut self, project_index: usize, agent_index: usize) {
        let agent = &self.projects[project_index].agents[agent_index];
        let rows = chat_rows(
            agent,
            &self.conversation.collapsed_tool_groups,
            &self.conversation.expanded_tool_history,
            &self.conversation.expanded_tool_rows,
            &self.conversation.expanded_thought_rows,
        );

        if self.conversation.chat_list_agent != Some(agent.config.id) {
            self.conversation.chat_list.reset(rows.len());
            self.conversation.chat_list_agent = Some(agent.config.id);
        } else if let Some((range, count)) = changed_row_range(&self.conversation.chat_rows, &rows)
        {
            let near_bottom = self
                .conversation
                .chat_list
                .scroll_px_offset_for_scrollbar()
                .y
                + self.conversation.chat_list.max_offset_for_scrollbar().y
                <= px(24.);
            self.conversation.chat_list.splice(range, count);
            if near_bottom {
                self.conversation.chat_list.scroll_to(gpui_kit::ListOffset {
                    item_ix: rows.len(),
                    offset_in_item: px(0.),
                });
            }
        }
        self.conversation.chat_rows = rows;
    }

    pub(super) fn render_chat_row(
        &self,
        agent: &AgentView,
        project_index: usize,
        agent_index: usize,
        row: ChatRowKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let palette = theme::palette(cx);
        let content = match row {
            ChatRowKind::Empty => div()
                .mt_8()
                .text_center()
                .text_color(palette.color(MUTED))
                .child(if agent.status == Status::Connecting {
                    "Connecting to agent…"
                } else {
                    "Ask the agent to work on this project."
                }),
            ChatRowKind::Tools(start, end) => {
                self.render_tool_group(agent, start, end, self.chat_text_style(cx), window, cx)
            }
            ChatRowKind::Message(index) => {
                self.render_message(agent, project_index, agent_index, index, window, cx)
            }
            ChatRowKind::Queued(index) => {
                let prompt = &agent.config.pending_prompts[index];
                div().w_full().min_w(px(0.)).flex().justify_end().child(
                    div()
                        .max_w(relative(0.85))
                        .min_w(px(0.))
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(palette.color(USER_BUBBLE))
                        .child(div().text_xs().text_color(palette.color(ACCENT)).child(
                            if shell_command(&prompt.text).is_some() {
                                format!("Queued {} · Shell command", index + 1)
                            } else {
                                format!("Queued {}", index + 1)
                            },
                        ))
                        .children(self.render_images(
                            format!("queued-{}-{index}", agent.config.id),
                            &prompt.images,
                        ))
                        .when(!prompt.text.is_empty(), |bubble| {
                            bubble.child(
                                div()
                                    .text_sm()
                                    .text_color(palette.color(TEXT))
                                    .whitespace_normal()
                                    .child(prompt.text.clone()),
                            )
                        }),
                )
            }
            ChatRowKind::Permission(index) => self.render_permission(agent, index, cx),
            ChatRowKind::Elicitation(index) => self.render_elicitation(agent, index, cx),
        };
        div()
            .w_full()
            .min_w(px(0.))
            .pb_2()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(CHAT_COLUMN_WIDTH))
                    .min_w(px(0.))
                    .child(content),
            )
    }

    fn render_images(&self, id: String, images: &[ChatImage]) -> Option<Div> {
        (!images.is_empty()).then(|| {
            div()
                .py_1()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(images.iter().enumerate().map(|(index, image)| {
                    let path = self.images.path(image);
                    div()
                        .id((gpui_kit::SharedString::from(id.clone()), index))
                        .max_w_full()
                        .rounded_md()
                        .overflow_hidden()
                        .cursor_pointer()
                        .child(
                            gpui_kit::img(path.clone())
                                .max_w_full()
                                .max_h(px(MESSAGE_IMAGE_HEIGHT))
                                .object_fit(gpui_kit::ObjectFit::Contain),
                        )
                        .tooltip(|window, cx| Tooltip::new("Open image").build(window, cx))
                        .on_click(move |_, _, cx| cx.open_with_system(&path))
                }))
        })
    }

    fn chat_text_style(&self, cx: &Context<Self>) -> TextViewStyle {
        TextViewStyle {
            paragraph_gap: rems(0.25),
            highlight_theme: cx.theme().highlight_theme.clone(),
            is_dark: cx.theme().mode.is_dark(),
            ..Default::default()
        }
    }

    fn render_thought(&self, agent: &AgentView, index: usize, cx: &mut Context<Self>) -> Div {
        let palette = theme::palette(cx);
        let key = (agent.config.id, index);
        let expanded = self.conversation.expanded_thought_rows.contains(&key);
        let row_id: gpui_kit::ElementId = ("thought-row", agent.config.id).into();
        let mut row = div().max_w(px(900.)).min_w(px(0.)).py_1().child(
            div()
                .id((row_id, index.to_string()))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(palette.color(MUTED))
                .child(div().w(px(14.)).flex_shrink_0().text_center().child("•"))
                .child("Thought")
                .child(div().text_xs().child(if expanded { "⌄" } else { "›" }))
                .on_click(self.pane_listener(cx, move |this, _, _, cx| {
                    if !this.conversation.expanded_thought_rows.insert(key) {
                        this.conversation.expanded_thought_rows.remove(&key);
                    }
                    cx.notify();
                })),
        );
        if expanded {
            let text_id: gpui_kit::ElementId = ("thought-detail", agent.config.id).into();
            let entry = &agent.messages[index];
            row = row.child(
                div()
                    .ml(px(22.))
                    .mt_1()
                    .p_2()
                    .rounded_md()
                    .bg(palette.color(SURFACE))
                    .children(self.render_images(
                        format!("thought-images-{}-{index}", agent.config.id),
                        &entry.images,
                    ))
                    .when(!entry.text.is_empty(), |details| {
                        details.child(
                            TextView::markdown((text_id, index.to_string()), entry.text.clone())
                                .style(self.chat_text_style(cx))
                                .selectable(true)
                                .text_sm()
                                .text_color(palette.color(TEXT)),
                        )
                    }),
            );
        }
        row
    }

    fn render_message(
        &self,
        agent: &AgentView,
        project_index: usize,
        agent_index: usize,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let palette = theme::palette(cx);
        let entry = &agent.messages[index];
        if entry.role == Role::Thought {
            return self.render_thought(agent, index, cx);
        }
        let shell = (entry.role == Role::User)
            .then(|| shell_command_in_message(&entry.text))
            .flatten();
        let display_text =
            shell
                .map(|command| format!("!{command}"))
                .unwrap_or_else(|| match entry.role {
                    Role::User => file_context_preview(&entry.text).into_owned(),
                    _ => entry.text.clone(),
                });
        let text_id: gpui_kit::ElementId = ("chat", agent.config.id).into();
        let content = TextView::markdown((text_id, index.to_string()), display_text)
            .style(self.chat_text_style(cx))
            .selectable(true)
            .text_sm()
            .text_color(palette.color(TEXT));
        let images =
            self.render_images(format!("images-{}-{index}", agent.config.id), &entry.images);
        match entry.role {
            Role::User | Role::Agent => {
                let from_user = entry.role == Role::User;
                let reply_text = entry.text.clone();
                if from_user {
                    return div().w_full().min_w(px(0.)).flex().justify_end().child(
                        div()
                            .max_w(relative(0.85))
                            .min_w(px(0.))
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(palette.color(USER_BUBBLE))
                            .text_sm()
                            .text_color(palette.color(TEXT))
                            .whitespace_normal()
                            .when(shell.is_some(), |element| {
                                element.child(
                                    div()
                                        .text_xs()
                                        .text_color(palette.color(ACCENT))
                                        .child("Shell command"),
                                )
                            })
                            .children(images)
                            .when(!entry.text.is_empty(), |bubble| bubble.child(content)),
                    );
                }
                let copy_state = window.use_keyed_state(
                    format!("copy-state-{}-{index}", agent.config.id),
                    cx,
                    |_, _| CopyFeedback::default(),
                );
                let copied = copy_state.read(cx).copied;
                let group = format!("reply-{}-{index}", agent.config.id);
                // Agent replies mirror the user's bubbles on the left; their
                // actions appear under the reply on hover.
                div()
                    .group(group.clone())
                    .w_full()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_1()
                    .child(
                        div()
                            .max_w(relative(0.85))
                            .min_w(px(0.))
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(palette.color(AGENT_BUBBLE))
                            .text_sm()
                            .text_color(palette.color(TEXT))
                            .whitespace_normal()
                            .children(images)
                            .when(!entry.text.is_empty(), |bubble| bubble.child(content)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .when(!copied, |actions| {
                                actions.invisible().group_hover(group, |style| style.visible())
                            })
                            .child(
                                Button::new(format!("copy-{}-{index}", agent.config.id))
                                    .ghost()
                                    .xsmall()
                                    .icon(if copied {
                                        IconName::Check
                                    } else {
                                        IconName::Copy
                                    })
                                    .label(if copied { "Copied" } else { "Copy" })
                                    .when(!copied, |button| {
                                        button.on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(
                                                gpui_kit::ClipboardItem::new_string(
                                                    reply_text.clone(),
                                                ),
                                            );
                                            copy_state.update(cx, |state, cx| {
                                                state.copied = true;
                                                cx.notify();
                                            });
                                            let copy_state = copy_state.clone();
                                            cx.spawn(async move |cx| {
                                                cx.background_executor()
                                                    .timer(Duration::from_secs(2))
                                                    .await;
                                                copy_state.update(cx, |state, cx| {
                                                    state.copied = false;
                                                    cx.notify();
                                                });
                                            })
                                            .detach();
                                        })
                                    }),
                            )
                            .child(
                                Button::new(format!("fork-{}-{index}", agent.config.id))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Network)
                                    .label("Fork from here")
                                    .tooltip("Start a separate session with the conversation up to this reply")
                                    .disabled(agent.active_work)
                                    .on_click(self.pane_listener(cx, move |this, _, window, cx| {
                                        this.fork_conversation(
                                            project_index,
                                            agent_index,
                                            index,
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                    )
            }
            Role::ContextReset => div()
                .w_full()
                .flex()
                .items_center()
                .gap_3()
                .py_2()
                .child(div().flex_1().h(px(1.)).bg(palette.color(BORDER)))
                .child(
                    div()
                        .max_w(relative(0.8))
                        .text_center()
                        .text_xs()
                        .text_color(palette.color(MUTED))
                        .whitespace_normal()
                        .child(entry.text.clone()),
                )
                .child(div().flex_1().h(px(1.)).bg(palette.color(BORDER))),
            role => {
                let (label, color) = match role {
                    Role::Tool => ("TOOL", TOOL_MARKER),
                    Role::System => ("SYSTEM", STATUS_ERROR),
                    _ => unreachable!(),
                };
                div()
                    .max_w(px(900.))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(palette.color(SURFACE))
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(palette.color(color))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_sm()
                            .text_color(palette.color(TEXT))
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
        let palette = theme::palette(cx);
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
                    .bg(palette.color(ACCENT_SURFACE))
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(palette.color(TEXT))
                    .child(label.clone())
                    .on_click(self.pane_listener(cx, move |this, _, _, cx| {
                        this.choose_permission(permission_index, option_id.clone(), cx)
                    })),
            );
        }
        div()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(palette.color(PERMISSION_BORDER))
            .child(
                div()
                    .text_sm()
                    .text_color(palette.color(TEXT))
                    .child(permission.title.clone()),
            )
            .when_some(permission.description.as_ref(), |element, description| {
                element.child(
                    div()
                        .mt_2()
                        .text_sm()
                        .text_color(palette.color(MUTED))
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
        let palette = theme::palette(cx);
        let question = &agent.elicitations[question_index];
        let mut fields = div().flex().flex_col().gap_3().mt_3();
        for (field_index, field) in question.fields.iter().enumerate() {
            let mut row = div().flex().flex_col().gap_1().child(
                div()
                    .text_sm()
                    .text_color(palette.color(TEXT))
                    .child(format!(
                        "{}{}",
                        field.title,
                        if field.required { " *" } else { "" }
                    )),
            );
            if let Some(description) = &field.description {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(palette.color(MUTED))
                        .child(description.clone()),
                );
            }
            match &field.kind {
                ElicitationFieldKind::Input(input) => {
                    row = row.child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(palette.color(BORDER))
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
            .border_color(palette.color(STATUS_QUESTION))
            .child(
                div()
                    .text_xs()
                    .text_color(palette.color(STATUS_QUESTION))
                    .child(format!("QUESTION FROM {}", agent.name)),
            )
            .child(
                div()
                    .mt_1()
                    .text_sm()
                    .text_color(palette.color(TEXT))
                    .child(question.message.clone()),
            )
            .child(fields)
            .when_some(question.error.as_ref(), |card, error| {
                card.child(
                    div()
                        .mt_2()
                        .text_sm()
                        .text_color(palette.color(STATUS_ERROR))
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
        let palette = theme::palette(cx);
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
                    .border_color(palette.color(if is_selected(option_index) {
                        ACCENT
                    } else {
                        BORDER
                    }))
                    .bg(palette.color(if is_selected(option_index) {
                        ACCENT_SURFACE
                    } else {
                        SURFACE
                    }))
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(palette.color(TEXT))
                    .child(label.clone())
                    .on_click(self.pane_listener(cx, move |this, _, _, cx| {
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
        let palette = theme::palette(cx);
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
            .bg(palette.color(if primary { ACCENT_SURFACE } else { SURFACE }))
            .text_color(palette.color(TEXT))
            .child(label)
            .on_click(self.pane_listener(cx, move |this, _, _, cx| {
                this.answer_elicitation(question_index, action, cx)
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> AgentView {
        AgentView::new(
            AgentConfig {
                id: 1,
                command: vec!["agent".into()],
                archived: false,
                display_name: None,
                title: None,
                custom_title: None,
                session_id: None,
                model: None,
                context: None,
                messages: Vec::new(),
                available_commands: Vec::new(),
                pending_prompts: Vec::new(),
                active_prompt: None,
                prompt_history: Vec::new(),
                was_working: false,
                session_has_activity: false,
                fork_pending: false,
                fork_source: None,
            },
            ImageStore::for_tests(),
        )
    }

    #[test]
    fn chat_rows_group_tools_and_invalidate_only_changed_content() {
        let mut agent = agent();
        agent.log(Role::User, "Question");
        agent.log(Role::Tool, "Search files");
        agent.log(Role::Tool, "Read file");
        agent.log(Role::Agent, "Answer");
        let collapsed = HashSet::new();
        let history = HashSet::new();
        let expanded = HashSet::new();
        let before = chat_rows(&agent, &collapsed, &history, &expanded, &HashSet::new());
        assert_eq!(before.len(), 3);
        assert_eq!(before[1].kind, ChatRowKind::Tools(1, 3));

        agent.messages[0].text.push('!');
        let after = chat_rows(&agent, &collapsed, &history, &expanded, &HashSet::new());
        assert_eq!(changed_row_range(&before, &after), Some((0..1, 1)));
        assert_eq!(changed_row_range(&after, &after), None);

        let collapsed = HashSet::from([(agent.config.id, 1)]);
        let collapsed_rows = chat_rows(&agent, &collapsed, &history, &expanded, &HashSet::new());
        assert_eq!(changed_row_range(&after, &collapsed_rows), Some((1..2, 1)));

        let history = HashSet::from([(agent.config.id, 1)]);
        let history_rows = chat_rows(&agent, &collapsed, &history, &expanded, &HashSet::new());
        assert_eq!(
            changed_row_range(&collapsed_rows, &history_rows),
            Some((1..2, 1))
        );

        agent.config.pending_prompts.push("Next".into());
        let queued_rows = chat_rows(&agent, &collapsed, &history, &expanded, &HashSet::new());
        assert_eq!(
            changed_row_range(&history_rows, &queued_rows),
            Some((3..3, 1))
        );
    }

    #[test]
    fn thought_disclosure_remeasures_only_when_visible_content_changes() {
        let mut agent = agent();
        agent.log(Role::Thought, "Consider the options");
        agent.log(Role::Agent, "Answer");
        let empty = HashSet::new();
        let collapsed = chat_rows(&agent, &empty, &empty, &empty, &empty);
        agent.messages[0].text.push_str(" before replying");
        let streaming = chat_rows(&agent, &empty, &empty, &empty, &empty);
        assert_eq!(changed_row_range(&collapsed, &streaming), None);

        let expanded = HashSet::from([(agent.config.id, 0)]);
        let visible = chat_rows(&agent, &empty, &empty, &empty, &expanded);
        assert_eq!(changed_row_range(&streaming, &visible), Some((0..1, 1)));
        agent.messages[0].text.push('.');
        let updated = chat_rows(&agent, &empty, &empty, &empty, &expanded);
        assert_eq!(changed_row_range(&visible, &updated), Some((0..1, 1)));
        agent.messages[0].images.push(ChatImage {
            mime_type: "image/png".into(),
            sha256: "thought-image".into(),
        });
        let with_image = chat_rows(&agent, &empty, &empty, &empty, &expanded);
        assert_eq!(changed_row_range(&updated, &with_image), Some((0..1, 1)));
        let closed = chat_rows(&agent, &empty, &empty, &empty, &empty);
        assert_eq!(changed_row_range(&with_image, &closed), Some((0..1, 1)));

        agent.config.id += 1;
        let other_session = chat_rows(&agent, &empty, &empty, &empty, &expanded);
        assert_eq!(other_session[0].signature, collapsed[0].signature);
    }

    #[test]
    fn chat_rows_hide_automatic_reviews() {
        let mut agent = agent();
        agent.log(Role::Tool, "Guardian Review · pending");
        let empty = HashSet::new();
        let rows = chat_rows(&agent, &empty, &empty, &empty, &empty);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, ChatRowKind::Empty);

        agent.log(Role::Tool, "cat README.md · completed");
        let rows = chat_rows(&agent, &empty, &empty, &empty, &empty);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, ChatRowKind::Tools(0, 2));

        agent.messages[0].text = "Guardian Review · failed".into();
        let updated = chat_rows(&agent, &empty, &empty, &empty, &empty);
        assert_eq!(changed_row_range(&rows, &updated), None);
    }
}
