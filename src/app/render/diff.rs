use super::*;
use crate::diff_view::Presentation;

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
        panel = panel.child(
            div()
                .px_4()
                .py_2()
                .flex()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(rgb(BORDER))
                .text_xs()
                .text_color(rgb(MUTED))
                .child("Checkout changes vs HEAD")
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .child("Shared by agents in this folder"),
                )
                .child(self.presentation_button(Presentation::Unified, "Unified", cx))
                .child(self.presentation_button(Presentation::Split, "Split", cx)),
        );
        let body = if let Some(error) = &self.diff_error {
            div()
                .flex_1()
                .p_4()
                .text_color(rgb(ERROR_TEXT))
                .child(format!("Could not load diff: {error}"))
                .into_any_element()
        } else if self.diff_loading && self.diff_rows.is_empty() {
            div()
                .flex_1()
                .p_4()
                .text_color(rgb(MUTED))
                .child("Loading changes…")
                .into_any_element()
        } else if self.diff_rows.is_empty() {
            div()
                .flex_1()
                .p_4()
                .text_color(rgb(MUTED))
                .child("No changes in this checkout")
                .into_any_element()
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
