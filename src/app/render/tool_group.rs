use super::*;

fn tool_status_marker(marker: &'static str, color: u32) -> Div {
    let indicator = div()
        .w(px(14.))
        .h(px(18.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(color));
    if marker == "◌" {
        indicator.child(status_dot(color))
    } else {
        indicator.child(marker)
    }
}

impl Workspace {
    pub(super) fn render_tool_group(
        &self,
        agent: &AgentView,
        message_index: usize,
        end: usize,
        text_style: TextViewStyle,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let tool_entries = &agent.messages[message_index..end];
        let group_key = (agent.config.id, message_index);
        let expanded = !self.collapsed_tool_groups.contains(&group_key);
        let history_expanded = self.expanded_tool_history.contains(&group_key);
        let heading = tool_group_heading(tool_entries);
        let has_specific_actions = tool_entries
            .iter()
            .any(|entry| !is_approval_review(&entry.text) && !is_generic_tool_title(&entry.text));
        let action_count = tool_entries
            .iter()
            .filter(|entry| {
                !is_approval_review(&entry.text)
                    && (!has_specific_actions || !is_generic_tool_title(&entry.text))
            })
            .count();
        let completed_count = tool_entries
            .iter()
            .filter(|entry| {
                !is_approval_review(&entry.text)
                    && (!has_specific_actions || !is_generic_tool_title(&entry.text))
                    && tool_title_and_status(&entry.text).1 == Some("completed")
            })
            .count();
        let (marker, marker_color) = if heading.starts_with("Needs attention") {
            ("!", STATUS_ERROR)
        } else if heading.starts_with("Working") {
            ("◌", STATUS_WORKING)
        } else if heading.starts_with("Waiting") {
            ("○", STATUS_CONNECTING)
        } else if heading == "Completed"
            || (heading == "Approval checks"
                && tool_entries
                    .iter()
                    .all(|entry| tool_title_and_status(&entry.text).1 == Some("completed")))
        {
            ("✓", STATUS_DONE)
        } else {
            ("•", TOOL_MARKER)
        };
        let group_id: gpui::ElementId = ("tool-group", agent.config.id).into();
        let mut group = div().max_w(px(900.)).min_w(px(0.)).py_1().child(
            div()
                .id((group_id, message_index.to_string()))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .child(tool_status_marker(marker, marker_color))
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(rgb(TEXT))
                        .child(heading),
                )
                .child(div().text_xs().text_color(rgb(MUTED)).child(if expanded {
                    "⌄"
                } else {
                    "›"
                }))
                .when(!expanded && action_count > 0, |row| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(format!("{action_count} steps")),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.collapsed_tool_groups.insert(group_key) {
                        this.collapsed_tool_groups.remove(&group_key);
                    }
                    cx.notify();
                })),
        );
        if expanded {
            let mut details = div().pl_4().flex().flex_col().gap_1();
            if completed_count > 1 {
                let history_id: gpui::ElementId = ("tool-history", agent.config.id).into();
                details = details.child(
                    div()
                        .id((history_id, message_index.to_string()))
                        .cursor_pointer()
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(
                            div()
                                .w(px(14.))
                                .flex_shrink_0()
                                .text_color(rgb(STATUS_DONE))
                                .child("✓"),
                        )
                        .child(format!("{completed_count} completed steps"))
                        .child(if history_expanded { "⌄" } else { "›" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.expanded_tool_history.insert(group_key) {
                                this.expanded_tool_history.remove(&group_key);
                            }
                            cx.notify();
                        })),
                );
            }
            for (offset, entry) in tool_entries.iter().enumerate().filter(|(_, entry)| {
                !is_approval_review(&entry.text)
                    && (!has_specific_actions || !is_generic_tool_title(&entry.text))
            }) {
                let (description, _) = tool_description(&entry.text);
                let (_, status) = tool_title_and_status(&entry.text);
                if status == Some("completed") && completed_count > 1 && !history_expanded {
                    continue;
                }
                let label = match status {
                    Some("in_progress") => format!("{description} · running"),
                    Some("pending") => format!("{description} · pending"),
                    Some("failed") => format!("{description} · failed"),
                    _ => description,
                };
                let (status_marker, status_color, label_color) = match status {
                    Some("completed") => ("✓", STATUS_DONE, MUTED),
                    Some("in_progress") => ("◌", STATUS_WORKING, TEXT),
                    Some("pending") => ("○", STATUS_CONNECTING, MUTED),
                    Some("failed") => ("!", STATUS_ERROR, ERROR_TEXT),
                    _ => ("•", MUTED, MUTED),
                };
                let row_key = (agent.config.id, message_index + offset);
                let row_expanded = self.expanded_tool_rows.contains(&row_key);
                let row_id: gpui::ElementId = ("tool-row", agent.config.id).into();
                let mut action = div().min_w(px(0.)).child(
                    div()
                        .id((row_id, (message_index + offset).to_string()))
                        .cursor_pointer()
                        .min_w(px(0.))
                        .flex()
                        .items_start()
                        .gap_2()
                        .text_sm()
                        .child(tool_status_marker(status_marker, status_color))
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_color(rgb(label_color))
                                .child(label),
                        )
                        .child(div().text_xs().child(if row_expanded { "⌄" } else { "›" }))
                        .tooltip(move |window, cx| {
                            Tooltip::new("Show command and output").build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.expanded_tool_rows.insert(row_key) {
                                this.expanded_tool_rows.remove(&row_key);
                            }
                            cx.notify();
                        })),
                );
                if row_expanded {
                    let text_id: gpui::ElementId = ("tool-detail", agent.config.id).into();
                    action = action.child(
                        div()
                            .ml(px(22.))
                            .mt_1()
                            .p_2()
                            .rounded_md()
                            .bg(rgb(SURFACE))
                            .child(
                                TextView::markdown(
                                    (text_id, row_key.1.to_string()),
                                    markdown_code_block(&entry.text),
                                )
                                .style(text_style.clone())
                                .selectable(true)
                                .text_xs()
                                .text_color(rgb(TEXT)),
                            ),
                    );
                }
                details = details.child(action);
            }
            if let Some(summary) = approval_summary(tool_entries) {
                let (status_marker, status_color) = if summary.contains("failed") {
                    ("!", STATUS_ERROR)
                } else if summary.contains("in progress") {
                    ("◌", STATUS_WORKING)
                } else if summary.contains("passed") {
                    ("✓", STATUS_DONE)
                } else {
                    ("•", MUTED)
                };
                details = details.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(tool_status_marker(status_marker, status_color).h(px(16.)))
                        .child(summary),
                );
            }
            group = group.child(details);
        }
        group
    }
}
