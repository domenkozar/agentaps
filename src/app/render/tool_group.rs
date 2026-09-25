use super::*;

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
        let group_id: gpui::ElementId = ("tool-group", agent.config.id).into();
        let mut group = div().max_w(px(900.)).min_w(px(0.)).py_1().child(
            div()
                .id((group_id, message_index.to_string()))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .child(div().text_color(rgb(TOOL_MARKER)).child("•"))
                .child(div().text_color(rgb(TEXT)).child(heading))
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
                            .child(format!("{action_count} actions")),
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
            let mut first = true;
            for (offset, entry) in tool_entries.iter().enumerate().filter(|(_, entry)| {
                !is_approval_review(&entry.text)
                    && (!has_specific_actions || !is_generic_tool_title(&entry.text))
            }) {
                let (description, _) = tool_description(&entry.text);
                let (_, status) = tool_title_and_status(&entry.text);
                let label = match status {
                    Some("in_progress") => format!("{description} · running"),
                    Some("failed") => format!("{description} · failed"),
                    _ => description,
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
                        .text_color(rgb(MUTED))
                        .child(div().w(px(14.)).flex_shrink_0().child(if first {
                            "└"
                        } else {
                            " "
                        }))
                        .child(div().min_w(px(0.)).truncate().child(label))
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
                first = false;
            }
            if let Some(summary) = approval_summary(tool_entries) {
                details = details.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(div().w(px(14.)).child(if first { "└" } else { " " }))
                        .child(summary),
                );
            }
            group = group.child(details);
        }
        group
    }
}
