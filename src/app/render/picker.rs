use super::*;

impl Workspace {
    pub(super) fn render_picker(&self, mut chat: Div, cx: &mut Context<Self>) -> Div {
        let is_folders = self.picker == PickerMode::Folders;
        let query = self.picker_input.read(cx).value().to_string();
        let mut results = div().flex().flex_col().gap_1();
        if is_folders {
            let matches = self.folder_results(cx);
            if matches.is_empty() {
                results = results.child(div().p_5().text_sm().text_color(rgb(MUTED)).child(
                    if !self.folder_scan_complete && query.is_empty() {
                        "Looking for folders…"
                    } else {
                        "No matching folders. Enter an existing absolute path."
                    },
                ));
            }
            for (index, path) in matches.into_iter().enumerate() {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let path_label = path.display().to_string();
                results = results.child(
                    div()
                        .id(("folder-result", index))
                        .cursor_pointer()
                        .rounded_lg()
                        .px_4()
                        .py_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .bg(rgb(if index == self.picker_selection {
                            SELECTED
                        } else {
                            SURFACE
                        }))
                        .hover(|style| style.bg(rgb(HOVER)))
                        .child(
                            Icon::new(IconName::Folder)
                                .size(px(18.))
                                .text_color(rgb(ACCENT)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().truncate().text_sm().text_color(rgb(TEXT)).child(name))
                                .child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(path_label),
                                ),
                        )
                        .child(
                            Icon::new(IconName::ChevronRight)
                                .size(px(16.))
                                .text_color(rgb(MUTED)),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_folder(path.clone(), window, cx)
                        })),
                );
            }
        } else {
            let matches = self.agent_results(cx);
            let match_count = matches.len();
            if matches.is_empty() {
                results = results.child(
                    div()
                        .p_5()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child("No matching installed agents. Enter an ACP command below."),
                );
            }
            for (index, agent) in matches.into_iter().enumerate() {
                let command = agent.command.clone();
                let name = agent.name.clone();
                results = results.child(
                    div()
                        .id(("agent-result", index))
                        .cursor_pointer()
                        .rounded_lg()
                        .px_4()
                        .py_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .bg(rgb(if index == self.picker_selection {
                            SELECTED
                        } else {
                            SURFACE
                        }))
                        .hover(|style| style.bg(rgb(HOVER)))
                        .child(
                            Icon::new(IconName::Bot)
                                .size(px(18.))
                                .text_color(rgb(ACCENT)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .truncate()
                                        .text_sm()
                                        .text_color(rgb(TEXT))
                                        .child(agent.name),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(agent.detail),
                                ),
                        )
                        .child(
                            Icon::new(IconName::ChevronRight)
                                .size(px(16.))
                                .text_color(rgb(MUTED)),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start_agent(command.clone(), Some(name.clone()), window, cx)
                        })),
                );
            }
            if !query.trim().is_empty() {
                let custom_selected = self.picker_selection == match_count;
                results = results.child(
                    div()
                        .id("custom-agent")
                        .cursor_pointer()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(if custom_selected { ACCENT } else { BORDER }))
                        .bg(rgb(SURFACE))
                        .px_4()
                        .py_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(ACCENT))
                                .child("Run custom ACP command"),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .child(query.trim().to_owned()),
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.start_custom_agent(window, cx)),
                        ),
                );
            }
        }
        let project_path = self
            .selected_project
            .and_then(|index| self.projects.get(index))
            .map(|project| project.path.display().to_string())
            .unwrap_or_default();
        chat = chat.child(
            div()
                .id("picker-page")
                .flex_1()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .items_center()
                .px_6()
                .py_6()
                .child(
                    div()
                        .w_full()
                        .max_w(px(680.))
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(ACCENT))
                                        .child("NEW SESSION"),
                                )
                                .when(!is_folders || self.selected.is_some(), |element| {
                                    element.child(
                                        div()
                                            .id("picker-back")
                                            .cursor_pointer()
                                            .rounded_md()
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .text_color(rgb(MUTED))
                                            .hover(|style| {
                                                style.bg(rgb(HOVER)).text_color(rgb(TEXT))
                                            })
                                            .child(if is_folders { "Close" } else { "Back" })
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.back_from_picker(window, cx)
                                            })),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child(if is_folders {
                                    "Choose a project"
                                } else {
                                    "Choose an agent"
                                }),
                        )
                        .child(div().text_sm().text_color(rgb(MUTED)).child(if is_folders {
                            "Search your folders or enter an absolute path."
                        } else {
                            "Select an installed agent or enter an ACP command."
                        }))
                        .when(!is_folders, |element| {
                            element.child(
                                div()
                                    .min_w(px(0.))
                                    .rounded_md()
                                    .bg(rgb(SURFACE))
                                    .px_3()
                                    .py_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_sm()
                                    .text_color(rgb(MUTED))
                                    .child(
                                        Icon::new(IconName::Folder)
                                            .size(px(16.))
                                            .text_color(rgb(ACCENT)),
                                    )
                                    .child(div().min_w(px(0.)).truncate().child(project_path)),
                            )
                        })
                        .child(
                            div()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(BORDER))
                                .bg(rgb(SURFACE))
                                .p_2()
                                .child(Input::new(&self.picker_input)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(MUTED))
                                .child(if is_folders {
                                    "FOLDERS"
                                } else {
                                    "AVAILABLE AGENTS"
                                }),
                        )
                        .child(results)
                        .child(div().pt_3().text_xs().text_color(rgb(MUTED)).child(
                            if is_folders && self.selected.is_none() {
                                "↑ ↓ Navigate  ·  Enter Select"
                            } else {
                                "↑ ↓ Navigate  ·  Enter Select  ·  Esc Back"
                            },
                        )),
                ),
        );
        chat
    }
}
