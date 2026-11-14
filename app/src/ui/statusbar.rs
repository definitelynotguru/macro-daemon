use gpui::prelude::*;
use gpui::{div, px, rgb, Context, Entity, IntoElement, ParentElement, Render, Styled, Window};

use crate::theme;
use crate::workspace::{Status, Workspace};

pub struct Statusbar {
    workspace: Entity<Workspace>,
    _subscription: gpui::Subscription,
}

impl Statusbar {
    pub fn new(workspace: Entity<Workspace>, cx: &mut gpui::Context<Self>) -> Self {
        let subscription = cx.observe(&workspace, |_, _, cx| cx.notify());
        Self {
            workspace,
            _subscription: subscription,
        }
    }
}

impl Render for Statusbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (dirty, status) = {
            let ws = self.workspace.read(cx);
            (ws.dirty(), ws.status.clone())
        };
        let marker = if dirty { "● unsaved" } else { "clean" };
        let (message, is_error) = match status {
            Status::Idle => (String::new(), false),
            Status::Info(msg) => (msg, false),
            Status::Error(msg) => (msg, true),
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .w_full()
            .h_7()
            .px_3()
            .bg(rgb(theme::PANEL))
            .border_t_1()
            .border_color(rgb(theme::BORDER))
            .text_size(px(11.0))
            .child(
                div()
                    .text_color(rgb(if dirty { theme::ACCENT } else { theme::MUTED }))
                    .child(marker),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(rgb(if is_error { theme::ERROR } else { theme::MUTED }))
                    .when(!message.is_empty(), |el| el.child(message)),
            )
    }
}
