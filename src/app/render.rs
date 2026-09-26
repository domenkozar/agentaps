use super::*;
use gpui::Div;

mod conversation;
mod diff;
mod entries;
mod picker;
mod sidebar;
mod tool_group;

fn status_dot(color: u32) -> Div {
    div().size(px(7.)).rounded_full().bg(rgb(color))
}

fn status_badge(agent: &AgentView) -> impl IntoElement {
    let pending = agent.elicitations.len();
    let color = if pending > 0 {
        STATUS_QUESTION
    } else {
        agent.status.color()
    };
    let label = if pending > 0 {
        format!(
            "{pending} question{} pending · {}",
            if pending == 1 { "" } else { "s" },
            agent.status.label()
        )
    } else {
        agent.status.label().to_owned()
    };
    div()
        .id(("status", agent.config.id))
        .flex()
        .flex_shrink_0()
        .size(px(12.))
        .items_center()
        .justify_center()
        .child(status_dot(color))
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = self.render_sidebar(window, cx);
        let divider = div()
            .id("sidebar-divider")
            .w(px(6.))
            .h_full()
            .flex_shrink_0()
            .cursor_ew_resize()
            .bg(rgb(BORDER))
            .hover(|style| style.bg(rgb(ACCENT)))
            .on_drag(SidebarResize, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            });

        let mut chat = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(BG));
        if matches!(self.view, WorkspaceView::NewSession { .. }) {
            chat = self.render_picker(chat, cx);
        } else if let Some(SessionLocation {
            project_index,
            agent_index,
        }) = self.view.displayed_session()
        {
            if self.diff_visible {
                chat = self.render_diff(chat, project_index, agent_index, cx);
            } else {
                self.sync_chat_rows(project_index, agent_index);
                chat = self.render_conversation(chat, project_index, agent_index, window, cx);
            }
        } else {
            let archived = matches!(self.view, WorkspaceView::Archive { .. });
            chat = chat.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(div().text_2xl().text_color(rgb(TEXT)).child(if archived {
                        "Archived sessions"
                    } else {
                        "Ready when you are."
                    }))
                    .child(div().text_color(rgb(MUTED)).child(if archived {
                        "Select a session to restore it."
                    } else {
                        "Press Ctrl+P to find a folder and start an agent."
                    })),
            );
        }
        if let Some(notice) = &self.notice {
            chat = chat.child(
                div()
                    .px_4()
                    .py_2()
                    .bg(rgb(ERROR_SURFACE))
                    .text_sm()
                    .text_color(rgb(ERROR_TEXT))
                    .child(notice.clone()),
            );
        }
        div()
            .size_full()
            .flex()
            .bg(rgb(BG))
            .on_action(cx.listener(Self::quick_open))
            .capture_key_down(cx.listener(Self::workspace_key_down))
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                this.handle_slash_action(SlashAction::Up, window, cx)
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                this.handle_slash_action(SlashAction::Down, window, cx)
            }))
            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                if !action.secondary {
                    this.handle_slash_action(SlashAction::Complete, window, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                this.handle_slash_action(SlashAction::Complete, window, cx)
            }))
            .capture_action(
                cx.listener(|this, _: &Escape, window, cx| this.handle_escape(window, cx)),
            )
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<SidebarResize>, window, cx| {
                    let viewport_width = f32::from(window.viewport_size().width);
                    let position = f32::from(event.event.position.x);
                    let max_width = (viewport_width - 320.).max(180.);
                    this.sidebar_fraction = position.clamp(180., max_width) / viewport_width;
                    this.dirty = true;
                    cx.notify();
                }),
            )
            .child(sidebar)
            .child(divider)
            .child(chat)
    }
}
