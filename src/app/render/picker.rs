use super::*;

impl Workspace {
    pub(in crate::app) fn render_picker(&self, mut chat: Div, cx: &mut Context<Self>) -> Div {
        let palette = theme::palette(cx);
        let WorkspaceView::NewSession { step, return_to } = self.view else {
            return chat;
        };
        let is_folders = matches!(step, PickerStep::Folders | PickerStep::ChangeFolder { .. });
        let changing_folder = matches!(step, PickerStep::ChangeFolder { .. });
        let query = self.picker.input.read(cx).value().to_string();
        let mut results = div().flex().flex_col().gap_1();
        if is_folders {
            let matches = self.picker.folder_search.results();
            if matches.is_empty() {
                results = results.child(div().p_5().text_sm().text_color(palette.color(MUTED)).child(
                    if self.picker.folder_search.searching() {
                        "Searching recent folders…"
                    } else {
                        "No matching recent folders. Enter a local absolute path or ssh://host/absolute/path."
                    },
                ));
            }
            for (index, path) in matches.iter().cloned().enumerate() {
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
                        .bg(palette.color(if index == self.picker.selection {
                            SELECTED
                        } else {
                            SURFACE
                        }))
                        .hover(|style| style.bg(palette.color(HOVER)))
                        .child(
                            Icon::new(IconName::Folder)
                                .size(px(18.))
                                .text_color(palette.color(ACCENT)),
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
                                        .text_color(palette.color(TEXT))
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(palette.color(MUTED))
                                        .child(path_label),
                                ),
                        )
                        .child(
                            Icon::new(IconName::ChevronRight)
                                .size(px(16.))
                                .text_color(palette.color(MUTED)),
                        )
                        .on_click(self.pane_listener(cx, move |this, _, window, cx| {
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
                        .text_color(palette.color(MUTED))
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
                        .bg(palette.color(if index == self.picker.selection {
                            SELECTED
                        } else {
                            SURFACE
                        }))
                        .hover(|style| style.bg(palette.color(HOVER)))
                        .child(
                            Icon::new(IconName::Bot)
                                .size(px(18.))
                                .text_color(palette.color(ACCENT)),
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
                                        .text_color(palette.color(TEXT))
                                        .child(agent.name),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(palette.color(MUTED))
                                        .child(agent.detail),
                                ),
                        )
                        .child(
                            Icon::new(IconName::ChevronRight)
                                .size(px(16.))
                                .text_color(palette.color(MUTED)),
                        )
                        .on_click(self.pane_listener(cx, move |this, _, window, cx| {
                            this.start_agent(command.clone(), Some(name.clone()), window, cx)
                        })),
                );
            }
            if !query.trim().is_empty() {
                let custom_selected = self.picker.selection == match_count;
                results = results.child(
                    div()
                        .id("custom-agent")
                        .cursor_pointer()
                        .rounded_lg()
                        .border_1()
                        .border_color(palette.color(if custom_selected { ACCENT } else { BORDER }))
                        .bg(palette.color(SURFACE))
                        .px_4()
                        .py_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(palette.color(ACCENT))
                                .child("Run custom ACP command"),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(palette.color(MUTED))
                                .child(query.trim().to_owned()),
                        )
                        .on_click(self.pane_listener(cx, |this, _, window, cx| {
                            this.start_custom_agent(window, cx)
                        })),
                );
            }
        }
        let project_path = match step {
            PickerStep::Agents { project_index } => self.projects.get(project_index),
            PickerStep::ChangeFolder { session } => self.projects.get(session.project_index),
            PickerStep::Folders => None,
        }
        .map(ProjectView::display_path)
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
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(palette.color(ACCENT))
                                        .child(if changing_folder {
                                            "SESSION FOLDER"
                                        } else {
                                            "NEW SESSION"
                                        }),
                                )
                                .when(!is_folders || return_to.is_some(), |element| {
                                    element.child(
                                        div()
                                            .id("picker-back")
                                            .cursor_pointer()
                                            .rounded_md()
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .text_color(palette.color(MUTED))
                                            .hover(|style| {
                                                style.bg(palette.color(HOVER)).text_color(palette.color(TEXT))
                                            })
                                            .child(if is_folders { "Close" } else { "Back" })
                                            .on_click(self.pane_listener(cx, |this, _, window, cx| {
                                                this.back_from_picker(window, cx)
                                            })),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(palette.color(TEXT))
                                .child(if changing_folder {
                                    "Change session folder"
                                } else if is_folders {
                                    "Choose a project"
                                } else {
                                    "Choose an agent"
                                }),
                        )
                        .child(div().text_sm().text_color(palette.color(MUTED)).child(if is_folders {
                            if changing_folder {
                                "Choose a local or SSH folder. The agent will reconnect there with fresh context; the previous session will be archived."
                            } else {
                                "Choose a local folder, select a recent project, or enter a local or SSH path."
                            }
                        } else {
                            "Select an installed agent or enter an ACP command."
                        }))
                        .when(!is_folders || changing_folder, |element| {
                            element.child(
                                div()
                                    .min_w(px(0.))
                                    .rounded_md()
                                    .bg(palette.color(SURFACE))
                                    .px_3()
                                    .py_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_sm()
                                    .text_color(palette.color(MUTED))
                                    .child(
                                        Icon::new(IconName::Folder)
                                            .size(px(16.))
                                            .text_color(palette.color(ACCENT)),
                                    )
                                    .child(div().min_w(px(0.)).truncate().child(project_path)),
                            )
                        })
                        .child(
                            div()
                                .rounded_lg()
                                .border_1()
                                .border_color(palette.color(BORDER))
                                .bg(palette.color(SURFACE))
                                .p_2()
                                .child(Input::new(&self.picker.input).cleanable(true)),
                        )
                        .when(is_folders, |element| {
                            element.child(
                                div()
                                    .id("choose-folder")
                                    .cursor_pointer()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(palette.color(BORDER))
                                    .bg(palette.color(SURFACE))
                                    .px_4()
                                    .py_3()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .hover(|style| style.bg(palette.color(HOVER)))
                                    .child(
                                        Icon::new(IconName::Folder)
                                            .size(px(18.))
                                            .text_color(palette.color(ACCENT)),
                                    )
                                    .child("Choose Folder…")
                                    .on_click(self.pane_listener(cx, |this, _, window, cx| {
                                        this.choose_folder(window, cx)
                                    })),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(palette.color(MUTED))
                                .child(if is_folders {
                                    "RECENT FOLDERS"
                                } else {
                                    "AVAILABLE AGENTS"
                                }),
                        )
                        .child(results)
                        .child(div().pt_3().text_xs().text_color(palette.color(MUTED)).child(
                            if is_folders && return_to.is_none() {
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
