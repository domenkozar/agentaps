use super::*;
use gpui_kit::component::Sizable;
use gpui_kit::component::progress::Progress;
use gpui_kit::{Anchor, Div};
use std::time::{SystemTime, UNIX_EPOCH};

mod conversation;
mod diff;
mod entries;
mod panes;
mod picker;
mod sidebar;
mod tool_group;

/// Maximum width of the conversation and composer, for comfortable line length.
const CHAT_COLUMN_WIDTH: f32 = 760.;
const MESSAGE_IMAGE_HEIGHT: f32 = 240.;

fn status_dot(color: u32, palette: theme::Palette) -> Div {
    div().size(px(7.)).rounded_full().bg(palette.color(color))
}

fn status_badge(agent: &AgentView, palette: theme::Palette) -> impl IntoElement {
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
        .justify_start()
        .when(agent.status != Status::Idle || pending > 0, |badge| {
            badge.child(status_dot(color, palette))
        })
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
    fn render_notice(
        &self,
        notice: &Notice,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = theme::palette(cx);
        let (background, foreground) = match notice {
            Notice::Info(_) => (ACCENT_SURFACE, ACCENT),
            Notice::Error(_) => (ERROR_SURFACE, ERROR_TEXT),
        };
        div()
            .id(id)
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .bg(palette.color(background))
            .text_sm()
            .text_color(palette.color(foreground))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(notice.message().to_owned()),
            )
            .child(
                Button::new(format!("{id}-dismiss"))
                    .ghost()
                    .compact()
                    .icon(IconName::Close)
                    .tooltip("Dismiss notification")
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_notice(cx))),
            )
    }

    fn render_mobile_loading(&self, cx: &mut Context<Self>) -> gpui_kit::Stateful<Div> {
        let palette = theme::palette(cx);
        div()
            .id("mobile-loading-overlay")
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .p_6()
            .bg(palette.color(BG))
            .text_color(palette.color(TEXT))
            .child(div().text_2xl().child("Connect your phone"))
            .child(div().text_sm().text_color(palette.color(MUTED)).child(
                if self.mobile_access.loading {
                    "Loading mobile secrets from SecretSpec…"
                } else {
                    "Starting mobile connection…"
                },
            ))
            .child(
                div().w_full().max_w(px(360.)).child(
                    Progress::new("mobile-secrets-loading")
                        .loading(true)
                        .accessibility_label("Loading mobile secrets"),
                ),
            )
            .child(
                div()
                    .id("close-mobile-loading")
                    .cursor_pointer()
                    .rounded_md()
                    .px_4()
                    .py_2()
                    .bg(palette.color(SURFACE))
                    .child("Close")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.mobile_access.pairing_visible = false;
                        cx.notify();
                    })),
            )
    }

    fn render_linked_clients(&self, cx: &mut Context<Self>) -> gpui_kit::Stateful<Div> {
        let palette = theme::palette(cx);
        let clients = self
            .mobile_access
            .server
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
                    .text_color(palette.color(MUTED))
                    .child("No browsers linked yet."),
            );
        }
        for client in clients {
            let id = client.id;
            let legacy = id == crate::mobile::LEGACY_CLIENT_ID;
            let confirming = self.mobile_access.revoke_confirm.as_deref() == Some(&id);
            let description = client.paired_at.map_or_else(
                || "Revoking this entry disconnects all older pairings.".into(),
                paired_ago,
            );
            let revoke_id = id.clone();
            let mut row = div()
                .id(format!("linked-mobile-{id}"))
                .rounded_md()
                .border_1()
                .border_color(palette.color(BORDER))
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_sm().child(client.name))
                .child(
                    div()
                        .text_xs()
                        .text_color(palette.color(MUTED))
                        .child(description),
                )
                .child(
                    div().flex().gap_2().child(
                        div()
                            .id(format!("revoke-mobile-{id}"))
                            .cursor_pointer()
                            .rounded_md()
                            .px_3()
                            .py_1()
                            .bg(palette.color(if confirming { ERROR_SURFACE } else { SURFACE }))
                            .text_xs()
                            .text_color(palette.color(if confirming { ERROR_TEXT } else { TEXT }))
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
                        .text_color(palette.color(MUTED))
                        .child("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.mobile_access.revoke_confirm = None;
                            cx.notify();
                        })),
                );
            }
            if legacy {
                row = row.border_color(palette.color(ACCENT));
            }
            list = list.child(row);
        }
        list
    }

    fn render_mobile_provider(
        &self,
        error: &str,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<Div> {
        let palette = theme::palette(cx);
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
            .bg(palette.color(BG))
            .text_color(palette.color(TEXT))
            .child(
                div()
                    .w_full()
                    .max_w(px(560.))
                    .rounded_md()
                    .border_1()
                    .border_color(palette.color(BORDER))
                    .bg(palette.color(SURFACE))
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(div().text_2xl().child("Choose a SecretSpec provider"))
                    .child(div().text_sm().text_color(palette.color(ERROR_TEXT)).child(error.to_owned()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(palette.color(MUTED))
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
                                    .bg(palette.color(ACCENT_SURFACE))
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
                                    .bg(palette.color(ACCENT_SURFACE))
                                    .child("1Password")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.use_mobile_provider_named("onepassword", cx)
                                    })),
                            ),
                    )
                    .child(Input::new(&self.mobile_access.provider_input))
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
                                    .bg(palette.color(ACCENT_SURFACE))
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
                                    .bg(palette.color(BG))
                                    .child("Close")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.mobile_access.provider_prompt = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(palette.color(MUTED))
                            .child("To keep a default provider for future launches, run `secretspec config global init`."),
                    ),
            )
    }

    fn render_mobile_pairing(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<Div> {
        let palette = theme::palette(cx);
        let viewport = window.viewport_size();
        let sidebar_width = (f32::from(viewport.width) * self.sidebar_fraction).max(180.);
        let available = (f32::from(viewport.width) - sidebar_width - 70.)
            .min(f32::from(viewport.height) - 230.)
            .clamp(80., 640.);
        let mut qr = div().flex().flex_col().flex_shrink_0().bg(rgb(0xffffff));
        if let Some(rows) = &self.mobile_access.qr {
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
                    .bg(palette.color(ACCENT_SURFACE))
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
                    .bg(palette.color(SURFACE))
                    .child("Close")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.mobile_access.pairing_visible = false;
                        this.mobile_access.revoke_confirm = None;
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
            .justify_start()
            .gap_4()
            .p_4()
            .overflow_y_scroll()
            .bg(palette.color(BG))
            .text_color(palette.color(TEXT))
            .child(div().text_2xl().child("Connect your phone"))
            .child(
                div()
                    .text_color(palette.color(MUTED))
                    .text_center()
                    .child(format!("Open {site} on your phone and scan this code.")),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(640.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_5()
                    .child(qr_column)
                    .child(div().w_full().max_w(px(560.)).child(clients)),
            )
            .when_some(self.notice.as_ref(), |overlay, notice| {
                overlay.child(self.render_notice(notice, "pairing-notice", cx))
            })
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let mobile_pairing = (self.mobile_access.pairing_visible
            && self.mobile_access.endpoint_id.is_some())
        .then(|| self.render_mobile_pairing(window, cx));
        let mobile_loading = (self.mobile_access.pairing_visible
            && self.mobile_access.endpoint_id.is_none())
        .then(|| self.render_mobile_loading(cx));
        let mobile_provider = self
            .mobile_access
            .provider_prompt
            .as_ref()
            .map(|error| self.render_mobile_provider(error, cx));
        let sidebar = self.render_sidebar(window, cx);
        let divider = div()
            .id("sidebar-divider")
            .w(px(6.))
            .h_full()
            .flex_shrink_0()
            .cursor_ew_resize()
            .bg(palette.color(BORDER))
            .hover(|style| style.bg(palette.color(ACCENT)))
            .on_drag(SidebarResize, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            });

        let mut chat = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .relative()
            .flex()
            .flex_col()
            .bg(palette.color(BG));
        chat = chat.child(self.render_panes(window, cx));
        if let Some(notice) = &self.notice {
            chat = chat.child(self.render_notice(notice, "workspace-notice", cx));
        }
        chat = chat
            .when_some(mobile_loading, |chat, loading| chat.child(loading))
            .when_some(mobile_pairing, |chat, pairing| chat.child(pairing))
            .when_some(mobile_provider, |chat, provider| chat.child(provider));
        div()
            .track_focus(&self.workspace_focus)
            .size_full()
            .flex()
            .bg(palette.color(BG))
            .on_action(cx.listener(Self::quick_open))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
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
                    this.persistence.dirty = true;
                    cx.notify();
                }),
            )
            .child(sidebar)
            .child(divider)
            .child(chat)
    }
}
