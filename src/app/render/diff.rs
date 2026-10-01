use super::*;
use crate::diff_view::{self, Presentation};
use gpui_kit::component::{Sizable, spinner::Spinner};

impl Workspace {
    pub(in crate::app) fn render_diff(&self, panel: Div, cx: &mut Context<Self>) -> Div {
        let palette = theme::palette(cx);
        let (added, removed) = self
            .diff
            .file_stats
            .iter()
            .copied()
            .fold((0, 0), |total, count| {
                (total.0 + count.0, total.1 + count.1)
            });
        let files = self.diff.files.len();
        let toolbar = div()
            .px_4()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(palette.color(BORDER))
            .text_xs()
            .text_color(palette.color(MUTED))
            .child(
                div()
                    .id("diff-comparison")
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_color(palette.color(TEXT))
                    .child("Checkout vs HEAD")
                    .tooltip(|window, cx| {
                        Tooltip::new("Checkout changes compared with HEAD").build(window, cx)
                    }),
            )
            .child(
                div()
                    .id("diff-loading-spinner")
                    .size(px(16.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(self.diff.loading, |slot| {
                        slot.child(Spinner::new().small().color(palette.color(MUTED)))
                            .tooltip(|window, cx| {
                                Tooltip::new("Loading checkout changes").build(window, cx)
                            })
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .font_family("monospace")
                    .child(format!(
                        "{files} {}",
                        if files == 1 { "file" } else { "files" }
                    ))
                    .child(
                        div()
                            .text_color(palette.color(diff_view::ADDED_TEXT))
                            .child(format!("+{added}")),
                    )
                    .child(
                        div()
                            .text_color(palette.color(diff_view::REMOVED_TEXT))
                            .child(format!("-{removed}")),
                    ),
            )
            .child(self.presentation_button(Presentation::Unified, "Unified", cx))
            .child(self.presentation_button(Presentation::Split, "Split", cx))
            .child(
                div()
                    .id("close-diff")
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(palette.color(TEXT))
                    .hover(|style| style.bg(palette.color(HOVER)))
                    .child("Close")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_diff();
                        this.conversation
                            .composer
                            .update(cx, |input, cx| input.focus(window, cx));
                        cx.notify();
                    })),
            );
        let panel = panel.child(toolbar);
        let body = if let Some(error) = &self.diff.error {
            div()
                .flex_1()
                .p_4()
                .text_color(palette.color(ERROR_TEXT))
                .child(format!("Could not load diff: {error}"))
                .into_any_element()
        } else if self.diff.loading && self.diff.files.is_empty() {
            div().flex_1().into_any_element()
        } else if self.diff.files.is_empty() {
            div()
                .flex_1()
                .p_4()
                .text_color(palette.color(MUTED))
                .child("No changes in this checkout")
                .into_any_element()
        } else {
            let view = cx.entity().clone();
            let rows = self.diff.rows.clone();
            div()
                .flex_1()
                .min_h(px(0.))
                .relative()
                .child(
                    div().id("diff-scroll").size_full().child(
                        gpui_kit::list(self.diff.list.clone(), move |index, _, cx| {
                            view.update(cx, |this, cx| this.render_diff_list_row(&rows[index], cx))
                        })
                        .w_full()
                        .h_full(),
                    ),
                )
                .vertical_scrollbar(&self.diff.list)
                .into_any_element()
        };
        panel.child(body)
    }

    fn render_diff_list_row(
        &self,
        row: &DiffListRow,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let palette = theme::palette(cx);
        match row {
            DiffListRow::File {
                path,
                note,
                added,
                removed,
                bar_width,
                added_width,
                expanded,
            } => {
                let selected_path = path.clone();
                let visible_note = if *expanded { None } else { note.as_ref() };
                div()
                    .id(format!("diff-file-{path}"))
                    .min_h(px(44.))
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(palette.color(BORDER))
                    .cursor_pointer()
                    .hover(|style| style.bg(palette.color(HOVER)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_diff_file(selected_path.clone(), cx);
                    }))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(palette.color(MUTED))
                            .child(if *expanded { "▾" } else { "▸" }),
                    )
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
                                    .text_color(palette.color(TEXT))
                                    .child(path.clone()),
                            )
                            .when_some(visible_note, |element, note| {
                                element.child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(palette.color(MUTED))
                                        .child(note.clone()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family("monospace")
                            .text_xs()
                            .text_color(palette.color(diff_view::ADDED_TEXT))
                            .child(format!("+{added}")),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family("monospace")
                            .text_xs()
                            .text_color(palette.color(diff_view::REMOVED_TEXT))
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
                                    .w(px(*added_width))
                                    .h_full()
                                    .bg(palette.color(diff_view::ADDED_TEXT)),
                            )
                            .child(
                                div()
                                    .w(px(*bar_width - *added_width))
                                    .h_full()
                                    .bg(palette.color(diff_view::REMOVED_TEXT)),
                            ),
                    )
                    .into_any_element()
            }
            DiffListRow::Content(row) => diff_view::render_row(row, palette),
        }
    }

    fn presentation_button(
        &self,
        presentation: Presentation,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = theme::palette(cx);
        let selected = self.diff.presentation == presentation;
        div()
            .id(label)
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .bg(palette.color(if selected { SELECTED } else { SURFACE }))
            .text_color(palette.color(if selected { TEXT } else { MUTED }))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_diff_presentation(presentation, cx);
            }))
    }
}
