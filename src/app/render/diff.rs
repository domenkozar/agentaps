use super::*;
use crate::diff_view::{self, Presentation};
use gpui_component::progress::Progress;

impl Workspace {
    pub(super) fn render_diff(
        &self,
        mut panel: Div,
        project_index: usize,
        agent_index: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let project = &self.projects[project_index];
        let agent = &project.agents[agent_index];
        let path = project.path.display().to_string();
        panel = panel.child(
            div()
                .px_4()
                .py_3()
                .border_b_1()
                .border_color(rgb(BORDER))
                .flex()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .gap_3()
                        .items_center()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_sm()
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
                                .child(path),
                        ),
                )
                .child(
                    div()
                        .id("back-to-chat")
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .text_sm()
                        .text_color(rgb(TEXT))
                        .hover(|style| style.bg(rgb(HOVER)))
                        .child("Chat")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_diff();
                            this.composer
                                .update(cx, |input, cx| input.focus(window, cx));
                            cx.notify();
                        })),
                ),
        );
        let mut toolbar = div()
            .px_4()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(rgb(BORDER))
            .text_xs()
            .text_color(rgb(MUTED));
        if let Some(selected) = &self.diff_selected_file {
            toolbar = toolbar
                .child(
                    div()
                        .id("back-to-diff-files")
                        .flex_shrink_0()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .text_color(rgb(TEXT))
                        .hover(|style| style.bg(rgb(HOVER)))
                        .child("Files")
                        .on_click(cx.listener(|this, _, _, cx| this.show_diff_summary(cx))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .child(selected.clone()),
                )
                .child(self.presentation_button(Presentation::Unified, "Unified", cx))
                .child(self.presentation_button(Presentation::Split, "Split", cx));
        } else {
            toolbar = toolbar.child("Checkout changes vs HEAD").child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .child("Shared by agents in this folder"),
            );
        }
        panel = panel.child(toolbar);
        if self.diff_loading {
            panel = panel.child(
                div()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("Loading changes…")
                    .child(
                        Progress::new("diff-loading")
                            .loading(true)
                            .accessibility_label("Loading checkout changes"),
                    ),
            );
        }
        let body = if let Some(error) = &self.diff_error {
            div()
                .flex_1()
                .p_4()
                .text_color(rgb(ERROR_TEXT))
                .child(format!("Could not load diff: {error}"))
                .into_any_element()
        } else if self.diff_loading && self.diff_files.is_empty() {
            div().flex_1().into_any_element()
        } else if self.diff_files.is_empty() {
            div()
                .flex_1()
                .p_4()
                .text_color(rgb(MUTED))
                .child("No changes in this checkout")
                .into_any_element()
        } else if self.diff_selected_file.is_none() {
            self.render_diff_summary(cx).into_any_element()
        } else {
            div()
                .flex_1()
                .min_h(px(0.))
                .relative()
                .child(
                    div()
                        .id("diff-scroll")
                        .size_full()
                        .child(crate::diff_view::view(
                            self.diff_list.clone(),
                            self.diff_rows.clone(),
                        )),
                )
                .vertical_scrollbar(&self.diff_list)
                .into_any_element()
        };
        panel.child(body)
    }

    fn render_diff_summary(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (added, removed) = self.diff_file_stats.iter().copied().fold(
            (0, 0),
            |(total_added, total_removed), (added, removed)| {
                (total_added + added, total_removed + removed)
            },
        );
        let max_changed = self
            .diff_file_stats
            .iter()
            .map(|(added, removed)| added + removed)
            .max()
            .unwrap_or(1)
            .max(1);
        let file_count = self.diff_files.len();
        let summary = format!(
            "{file_count} {} changed, {added} {}(+), {removed} {}(-)",
            if file_count == 1 { "file" } else { "files" },
            if added == 1 {
                "insertion"
            } else {
                "insertions"
            },
            if removed == 1 {
                "deletion"
            } else {
                "deletions"
            },
        );
        let mut list = div()
            .id("diff-file-summary")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .font_family("monospace")
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(summary),
            );
        for (index, file) in self.diff_files.iter().enumerate() {
            let (added, removed) = self.diff_file_stats[index];
            let changed = added + removed;
            let bar_width = if changed == 0 {
                0.0
            } else {
                (changed as f32 / max_changed as f32 * 96.0).max(4.0)
            };
            let added_width = if changed == 0 {
                0.0
            } else {
                bar_width * added as f32 / changed as f32
            };
            let path = file.path.clone();
            list = list.child(
                div()
                    .id(("diff-file", index))
                    .min_h(px(44.))
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(HOVER)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_diff_file(path.clone(), cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .font_family("monospace")
                                    .text_sm()
                                    .text_color(rgb(TEXT))
                                    .child(file.path.clone()),
                            )
                            .when_some(file.note.as_ref(), |element, note| {
                                element.child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(note.clone()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family("monospace")
                            .text_xs()
                            .text_color(rgb(diff_view::ADDED_TEXT))
                            .child(format!("+{added}")),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family("monospace")
                            .text_xs()
                            .text_color(rgb(diff_view::REMOVED_TEXT))
                            .child(format!("-{removed}")),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .w(px(96.))
                            .h(px(8.))
                            .flex()
                            .rounded_sm()
                            .overflow_hidden()
                            .child(
                                div()
                                    .w(px(added_width))
                                    .h_full()
                                    .bg(rgb(diff_view::ADDED_TEXT)),
                            )
                            .child(
                                div()
                                    .w(px(bar_width - added_width))
                                    .h_full()
                                    .bg(rgb(diff_view::REMOVED_TEXT)),
                            ),
                    ),
            );
        }
        list
    }

    fn presentation_button(
        &self,
        presentation: Presentation,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.diff_presentation == presentation;
        div()
            .id(label)
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .bg(rgb(if selected { SELECTED } else { SURFACE }))
            .text_color(rgb(if selected { TEXT } else { MUTED }))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_diff_presentation(presentation, cx);
            }))
    }
}
