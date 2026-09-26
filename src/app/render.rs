use super::*;
use gpui::Div;
use std::time::{SystemTime, UNIX_EPOCH};

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

fn paired_ago(paired_at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let elapsed = now.saturating_sub(paired_at);
    if elapsed < 60 {
        "Paired just now".into()
    } else if elapsed < 3_600 {
        format!("Paired {} minutes ago", elapsed / 60)
    } else if elapsed < 86_400 {
        format!("Paired {} hours ago", elapsed / 3_600)
    } else {
        format!("Paired {} days ago", elapsed / 86_400)
    }
}

impl Workspace {
    fn render_linked_clients(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let clients = self
            .mobile
            .as_ref()
            .map(crate::mobile::Server::linked_clients)
            .unwrap_or_default();
        let mut list = div()
            .id("linked-mobile-clients")
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_lg().child("Linked clients"));
        if clients.is_empty() {
            list = list.child(
                div()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child("No browsers linked yet."),
            );
        }
        for client in clients {
            let id = client.id;
            let legacy = id == crate::mobile::LEGACY_CLIENT_ID;
            let confirming = self.mobile_revoke_confirm.as_deref() == Some(&id);
            let description = client.paired_at.map_or_else(
                || "Revoking this entry disconnects all older pairings.".into(),
                paired_ago,
            );
            let revoke_id = id.clone();
            let mut row = div()
                .id(format!("linked-mobile-{id}"))
                .rounded_md()
                .border_1()
                .border_color(rgb(BORDER))
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_sm().child(client.name))
                .child(div().text_xs().text_color(rgb(MUTED)).child(description))
                .child(
                    div().flex().gap_2().child(
                        div()
                            .id(format!("revoke-mobile-{id}"))
                            .cursor_pointer()
                            .rounded_md()
                            .px_3()
                            .py_1()
                            .bg(rgb(if confirming { ERROR_SURFACE } else { SURFACE }))
                            .text_xs()
                            .text_color(rgb(if confirming { ERROR_TEXT } else { TEXT }))
                            .child(if confirming {
                                "Confirm revoke"
                            } else {
                                "Revoke"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.revoke_mobile_client(revoke_id.clone(), cx)
                            })),
                    ),
                );
            if confirming {
                row = row.child(
                    div()
                        .id(format!("cancel-revoke-mobile-{id}"))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.mobile_revoke_confirm = None;
                            cx.notify();
                        })),
                );
            }
            if legacy {
                row = row.border_color(rgb(ACCENT));
            }
            list = list.child(row);
        }
        list
    }

    fn render_mobile_provider(&self, error: &str, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        div()
            .id("mobile-provider-overlay")
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .w_full()
                    .max_w(px(560.))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(div().text_2xl().child("Choose a SecretSpec provider"))
                    .child(div().text_sm().text_color(rgb(ERROR_TEXT)).child(error.to_owned()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child("Use a provider for this run:"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .id("mobile-provider-keyring")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .px_4()
                                    .py_2()
                                    .bg(rgb(ACCENT_SURFACE))
                                    .child("System keyring")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.use_mobile_provider_named("keyring", cx)
                                    })),
                            )
                            .child(
                                div()
                                    .id("mobile-provider-onepassword")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .px_4()
                                    .py_2()
                                    .bg(rgb(ACCENT_SURFACE))
                                    .child("1Password")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.use_mobile_provider_named("onepassword", cx)
                                    })),
                            ),
                    )
                    .child(Input::new(&self.mobile_provider_input))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .id("mobile-provider-custom")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .px_4()
                                    .py_2()
                                    .bg(rgb(ACCENT_SURFACE))
                                    .child("Use provider")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.use_mobile_provider(cx)
                                    })),
                            )
                            .child(
                                div()
                                    .id("close-mobile-provider")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .px_4()
                                    .py_2()
                                    .bg(rgb(BG))
                                    .child("Close")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.mobile_provider_prompt = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child("To keep a default provider for future launches, run `secretspec config global init`."),
                    ),
            )
    }

    fn render_mobile_pairing(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let viewport = window.viewport_size();
        let side_by_side = f32::from(viewport.width) >= 760.;
        let available = (f32::from(viewport.width) - if side_by_side { 340. } else { 64. })
            .min(f32::from(viewport.height) - 230.)
            .min(640.)
            .max(80.);
        let mut qr = div().flex().flex_col().flex_shrink_0().bg(rgb(0xffffff));
        if let Some(rows) = &self.mobile_qr {
            let module_size = (available / (rows.len() as f32 + 8.)).floor().max(1.);
            qr = qr.p(px(module_size * 4.));
            for row in rows {
                let mut line = div().flex().flex_shrink_0();
                for dark in row {
                    line = line.child(
                        div()
                            .size(px(module_size))
                            .flex_shrink_0()
                            .bg(rgb(if *dark { 0x000000 } else { 0xffffff })),
                    );
                }
                qr = qr.child(line);
            }
        }
        let site = self
            .mobile_link()
            .and_then(|link| link.split_once('#').map(|(site, _)| site.to_owned()))
            .unwrap_or_default();
        let controls = div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .id("copy-mobile-link")
                    .cursor_pointer()
                    .rounded_md()
                    .px_4()
                    .py_2()
                    .bg(rgb(ACCENT_SURFACE))
                    .child("Copy link")
                    .on_click(cx.listener(|this, _, _, cx| this.copy_mobile_link(cx))),
            )
            .child(
                div()
                    .id("close-mobile-pairing")
                    .cursor_pointer()
                    .rounded_md()
                    .px_4()
                    .py_2()
                    .bg(rgb(SURFACE))
                    .child("Close")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.mobile_pairing_visible = false;
                        this.mobile_revoke_confirm = None;
                        cx.notify();
                    })),
            );
        let qr_column = div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .child(qr)
            .child(controls);
        let clients = self.render_linked_clients(cx);
        div()
            .id("mobile-pairing-overlay")
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .p_4()
            .overflow_y_scroll()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(div().text_2xl().child("Connect your phone"))
            .child(
                div()
                    .text_color(rgb(MUTED))
                    .text_center()
                    .child(format!("Open {site} on your phone and scan this code.")),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(960.))
                    .flex()
                    .items_start()
                    .gap_5()
                    .when(side_by_side, |body| body.flex_row())
                    .when(!side_by_side, |body| body.flex_col().items_center())
                    .child(qr_column)
                    .child(div().w_full().max_w(px(280.)).child(clients)),
            )
            .when_some(self.notice.as_ref(), |overlay, notice| {
                overlay.child(div().text_color(rgb(MUTED)).child(notice.clone()))
            })
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mobile_pairing = self
            .mobile_pairing_visible
            .then(|| self.render_mobile_pairing(window, cx));
        let mobile_provider = self
            .mobile_provider_prompt
            .as_ref()
            .map(|error| self.render_mobile_provider(error, cx));
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
            .relative()
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
            .when_some(mobile_pairing, |root, pairing| root.child(pairing))
            .when_some(mobile_provider, |root, provider| root.child(provider))
    }
}
