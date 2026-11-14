use gpui::prelude::*;
use gpui::{
    div, px, rgb, App, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
};

use crate::theme;
use crate::ui::{
    properties::Properties, sidebar::Sidebar, statusbar::Statusbar, timeline::Timeline,
    toolbar::Toolbar,
};
use crate::workspace::Workspace;

pub struct StudioView {
    workspace: Entity<Workspace>,
    sidebar: Entity<Sidebar>,
    timeline: Entity<Timeline>,
    properties: Entity<Properties>,
    toolbar: Entity<Toolbar>,
    statusbar: Entity<Statusbar>,
}

impl StudioView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut App) -> Self {
        let sidebar = cx.new(|cx| Sidebar::new(workspace.clone(), cx));
        let timeline = cx.new(|cx| Timeline::new(workspace.clone(), cx));
        let properties = cx.new(|cx| Properties::new(workspace.clone(), cx));
        let toolbar = cx.new(|cx| Toolbar::new(workspace.clone(), cx));
        let statusbar = cx.new(|cx| Statusbar::new(workspace.clone(), cx));
        Self {
            workspace,
            sidebar,
            timeline,
            properties,
            toolbar,
            statusbar,
        }
    }
}

impl Render for StudioView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.workspace.read(cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme::BG))
            .text_color(rgb(theme::TEXT))
            .font_family("Nimbus Sans")
            .text_size(px(12.0))
            .child(self.toolbar.clone())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar.clone())
                    .child(self.timeline.clone())
                    .child(self.properties.clone()),
            )
            .child(self.statusbar.clone())
    }
}
