use gpui::prelude::*;
use gpui::{div, px, rgb, Context, Entity, IntoElement, ParentElement, Render, Styled, Window};

use crate::theme;
use crate::ui::widgets::toolbar_button;
use crate::workspace::Workspace;

pub struct Toolbar {
    workspace: Entity<Workspace>,
    _subscription: gpui::Subscription,
}

impl Toolbar {
    pub fn new(workspace: Entity<Workspace>, cx: &mut gpui::Context<Self>) -> Self {
        let subscription = cx.observe(&workspace, |_, _, cx| cx.notify());
        Self {
            workspace,
            _subscription: subscription,
        }
    }
}

impl Render for Toolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dirty = self.workspace.read(cx).dirty();
        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .h_10()
            .px_3()
            .bg(rgb(theme::PANEL))
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .child(
                toolbar_button("tb-new", "New macro", true).on_click(cx.listener(
                    |this, _, _, cx| {
                        let id = crate::schema::new_macro_id();
                        this.workspace.update(cx, |ws, cx| {
                            ws.new_macro(id.clone(), crate::schema::default_binding(&id));
                            cx.notify();
                        })
                    },
                )),
            )
            .child(
                toolbar_button("tb-dup", "Duplicate", true).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.workspace.update(cx, |ws, cx| {
                            if let Some(sel) = ws.selected_macro.clone() {
                                ws.duplicate_macro(&sel, crate::schema::new_macro_id());
                                cx.notify();
                            }
                        })
                    },
                )),
            )
            .child(
                toolbar_button("tb-del", "Delete", true).on_click(cx.listener(|this, _, _, cx| {
                    this.workspace.update(cx, |ws, cx| {
                        if let Some(sel) = ws.selected_macro.clone() {
                            ws.delete_macro(&sel);
                            cx.notify();
                        }
                    })
                })),
            )
            .child(div().flex_1())
            .child(
                toolbar_button("tb-save", "Save", dirty).on_click(cx.listener(|this, _, _, cx| {
                    this.workspace.update(cx, |ws, cx| {
                        if let Err(err) = ws.save_open_profile() {
                            ws.status = crate::workspace::Status::Error(format!("{err:#}"));
                        }
                        cx.notify();
                    })
                })),
            )
            .child(
                toolbar_button("tb-reload", "Reload", true)
                    .tooltip(|_, cx| {
                        let view: gpui::AnyView = cx.new(|_| TooltipView).into();
                        view
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.workspace.update(cx, |ws, cx| {
                            ws.reload();
                            cx.notify();
                        })
                    })),
            )
    }
}

struct TooltipView;

impl Render for TooltipView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .bg(rgb(0x2a2e35))
            .border_1()
            .border_color(rgb(theme::BORDER))
            .rounded_sm()
            .text_size(px(11.0))
            .text_color(rgb(theme::TEXT))
            .child("Re-reads profiles/ from disk; discards unsaved edits")
    }
}
