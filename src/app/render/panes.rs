use super::*;
use crate::panes::{Direction, Node};
use gpui_kit::component::menu::PopupMenu;

#[derive(Clone)]
struct PaneResize {
    split_id: u64,
    direction: Direction,
    bounds: Bounds<gpui_kit::Pixels>,
}

impl Render for PaneResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Workspace {
    /// Build pane actions at menu-open time so resize limits reflect the current bounds.
    pub(super) fn pane_menu_items(
        menu: PopupMenu,
        pane_id: u64,
        view: &Entity<Self>,
        cx: &App,
    ) -> PopupMenu {
        let workspace = view.read(cx);
        let can_right = workspace.can_split_pane(pane_id, Direction::Right);
        let can_down = workspace.can_split_pane(pane_id, Direction::Down);
        let can_close = workspace.pane_layout.root.panes().len() > 1;
        let right = view.clone();
        let down = view.clone();
        let close = view.clone();
        menu.item(
            PopupMenuItem::new("Split Right")
                .disabled(!can_right)
                .on_click(move |_, window, cx| {
                    right.update(cx, |this, cx| {
                        if this.activate_pane(pane_id, cx) {
                            this.split_pane(Direction::Right, window, cx);
                        }
                    });
                }),
        )
        .item(
            PopupMenuItem::new("Split Down")
                .disabled(!can_down)
                .on_click(move |_, window, cx| {
                    down.update(cx, |this, cx| {
                        if this.activate_pane(pane_id, cx) {
                            this.split_pane(Direction::Down, window, cx);
                        }
                    });
                }),
        )
        .when(can_close, |menu| {
            menu.item(
                PopupMenuItem::new("Close Pane").on_click(move |_, window, cx| {
                    close.update(cx, |this, cx| {
                        if this.activate_pane(pane_id, cx) {
                            this.close_pane(window, cx);
                        }
                    });
                }),
            )
        })
    }

    fn render_pane_tree(
        &mut self,
        node: &Node,
        width: f32,
        height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let palette = theme::palette(cx);
        match node {
            Node::Pane { id, .. } => {
                let id = *id;
                let focused = self.pane_layout.focused == id;
                let focus = if id == self.pane_id {
                    self.pane_focus.clone()
                } else {
                    self.inactive_panes[&id].focus.clone()
                };
                let bounds = self.pane_bounds.entry(id).or_default().clone();
                let content = self
                    .with_pane(id, cx, |this, cx| {
                        this.conversation.viewport_height = height;
                        this.conversation.resize_composers(this.font_scale, cx);
                        let chat = div()
                            .flex_1()
                            .min_h(px(0.))
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .relative();
                        let chat = if matches!(this.view, WorkspaceView::NewSession { .. }) {
                            this.render_picker(chat, cx)
                        } else if let Some(session) = this.view.displayed_session() {
                            this.sync_chat_rows(session.project_index, session.agent_index);
                            this.render_conversation(
                                chat,
                                session.project_index,
                                session.agent_index,
                                window,
                                cx,
                            )
                        } else {
                            chat.child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(palette.color(MUTED))
                                            .child("Select a sidebar session or start a new one."),
                                    )
                                    .child(
                                        Button::new(("pane-new", id))
                                            .ghost()
                                            .label("New Session")
                                            .on_click(this.pane_listener(
                                                cx,
                                                |this, _, window, cx| {
                                                    this.open_picker(
                                                        PickerStep::Folders,
                                                        window,
                                                        cx,
                                                    )
                                                },
                                            )),
                                    ),
                            )
                        };
                        let needs_menu = this.view.displayed_session().is_none();
                        let view = cx.entity().clone();
                        div()
                            .size_full()
                            .relative()
                            .flex()
                            .flex_col()
                            .child(chat)
                            .when(needs_menu, |pane| {
                                pane.child(
                                    div().absolute().top(px(8.)).right(px(8.)).child(
                                        Button::new(("pane-menu", id))
                                            .ghost()
                                            .compact()
                                            .icon(IconName::Ellipsis)
                                            .tooltip("Pane")
                                            .dropdown_menu_with_anchor(
                                                Anchor::TopRight,
                                                move |menu, _, cx| {
                                                    Self::pane_menu_items(menu, id, &view, cx)
                                                },
                                            ),
                                    ),
                                )
                            })
                    })
                    .unwrap();
                div()
                    .id(("chat-pane", id))
                    .track_focus(&focus)
                    .w(px(width))
                    .h(px(height))
                    .flex_shrink_0()
                    .relative()
                    .border_1()
                    .border_color(palette.color(if focused { ACCENT } else { BORDER }))
                    .overflow_hidden()
                    .capture_any_mouse_down(cx.listener(move |this, _, _, cx| {
                        this.activate_pane(id, cx);
                    }))
                    .child(
                        gpui_kit::canvas(
                            move |rect, _, _| {
                                bounds.set(rect);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(content)
                    .into_any_element()
            }
            Node::Split {
                id,
                direction,
                first,
                second,
                ..
            } => {
                let (a, b) = node.sizes(width, height).unwrap();
                let right = *direction == Direction::Right;
                let first = self.render_pane_tree(
                    first,
                    if right { a } else { width },
                    if right { height } else { a },
                    window,
                    cx,
                );
                let second = self.render_pane_tree(
                    second,
                    if right { b } else { width },
                    if right { height } else { b },
                    window,
                    cx,
                );
                let bounds = self.pane_bounds.entry(*id).or_default().clone();
                let drag_bounds = bounds.clone();
                let split_id = *id;
                let direction = *direction;
                let divider = div()
                    .id(("pane-divider", split_id))
                    .flex_shrink_0()
                    .when(right, |divider| {
                        divider
                            .w(px(crate::panes::DIVIDER))
                            .h_full()
                            .cursor_ew_resize()
                    })
                    .when(!right, |divider| {
                        divider
                            .h(px(crate::panes::DIVIDER))
                            .w_full()
                            .cursor_ns_resize()
                    })
                    .bg(palette.color(BORDER))
                    .hover(|style| style.bg(palette.color(ACCENT)))
                    .on_drag(
                        PaneResize {
                            split_id,
                            direction,
                            bounds: Bounds::default(),
                        },
                        move |drag, _, _, cx| {
                            cx.stop_propagation();
                            let mut drag = drag.clone();
                            drag.bounds = drag_bounds.get();
                            cx.new(|_| drag)
                        },
                    );
                div()
                    .id(("pane-split", split_id))
                    .w(px(width))
                    .h(px(height))
                    .flex_shrink_0()
                    .relative()
                    .flex()
                    .when(!right, |split| split.flex_col())
                    .child(
                        gpui_kit::canvas(
                            move |rect, _, _| {
                                bounds.set(rect);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(first)
                    .child(divider)
                    .child(second)
                    .into_any_element()
            }
        }
    }

    pub(in crate::app) fn render_panes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.reconcile_panes();
        let layout = self.pane_layout.root.clone();
        let minimum = layout.minimum_size();
        let viewport = window.viewport_size();
        let measured = self.pane_area_bounds.get().size;
        let available_width = if measured.width > px(0.) {
            f32::from(measured.width)
        } else {
            f32::from(viewport.width) * (1. - self.sidebar_fraction) - 6.
        };
        let available_height = if measured.height > px(0.) {
            f32::from(measured.height)
        } else {
            f32::from(viewport.height)
        };
        let width = (available_width * if self.diff.visible { 0.5 } else { 1. }).max(minimum.0);
        let height = available_height.max(minimum.1);
        let area_bounds = self.pane_area_bounds.clone();
        let view = cx.entity().downgrade();
        let tree = self.render_pane_tree(&layout, width, height, window, cx);
        let mut content = div()
            .id("pane-area")
            .relative()
            .child(
                gpui_kit::canvas(
                    move |bounds, _, cx| {
                        let previous = area_bounds.replace(bounds);
                        if previous.size != bounds.size {
                            view.update(cx, |_, cx| cx.notify()).ok();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .h_full()
            .flex()
            .child(
                div()
                    .id("pane-layout-scroll")
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_scroll()
                    .child(tree),
            );
        if self.diff.visible {
            let diff = if let Some(session) = self.view.displayed_session() {
                if self.projects[session.project_index].ssh_host.is_none() {
                    self.render_diff(div().flex_1().min_w(px(0.)).h_full().flex().flex_col(), cx)
                } else {
                    div()
                        .flex_1()
                        .p_4()
                        .child("Diff review for SSH projects is not available yet.")
                }
            } else {
                div()
                    .flex_1()
                    .p_4()
                    .child("Select a session to review its diff.")
            };
            content = content.child(diff);
        }
        content
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<PaneResize>, _, cx| {
                    let drag = event.drag(cx);
                    let Some(node @ Node::Split { .. }) =
                        this.pane_layout.root.find_mut(drag.split_id)
                    else {
                        return;
                    };
                    let Node::Split {
                        direction,
                        ratio,
                        first,
                        second,
                        ..
                    } = node
                    else {
                        return;
                    };
                    if *direction != drag.direction {
                        return;
                    }
                    let a = first.minimum_size();
                    let b = second.minimum_size();
                    let (position, total, min_a, min_b) = match direction {
                        Direction::Right => (
                            f32::from(event.event.position.x - drag.bounds.origin.x),
                            f32::from(drag.bounds.size.width) - crate::panes::DIVIDER,
                            a.0,
                            b.0,
                        ),
                        Direction::Down => (
                            f32::from(event.event.position.y - drag.bounds.origin.y),
                            f32::from(drag.bounds.size.height) - crate::panes::DIVIDER,
                            a.1,
                            b.1,
                        ),
                    };
                    if total < min_a + min_b {
                        return;
                    }
                    *ratio = position.clamp(min_a, total - min_b) / total;
                    this.persistence.dirty = true;
                    cx.notify();
                }),
            )
            .into_any_element()
    }
}
