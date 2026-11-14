use gpui::prelude::*;
use gpui::{
    div, px, relative, rgb, AnyElement, Context, Entity, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window,
};

use crate::theme;
use crate::workspace::Workspace;

pub struct Sidebar {
    workspace: Entity<Workspace>,
    _subscription: gpui::Subscription,
}

struct ProfileRow {
    file_name: String,
    name: String,
    badge: Option<String>,
    is_open: bool,
    macros: Vec<(String, String, String)>,
}

impl Sidebar {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&workspace, |_, _, cx| cx.notify());
        Self {
            workspace,
            _subscription: subscription,
        }
    }

    fn row(
        label: String,
        sublabel: Option<String>,
        selected: bool,
        depth: usize,
        id: gpui::ElementId,
        on_click: impl Fn(&mut Sidebar, &gpui::ClickEvent, &mut Window, &mut Context<Sidebar>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bg = if selected {
            theme::ACCENT_DIM
        } else {
            theme::PANEL
        };
        let text = if selected { theme::TEXT } else { theme::MUTED };
        div()
            .id(id)
            .flex()
            .items_center()
            .w_full()
            .h_8()
            .pl(px(8.0 + depth as f32 * 16.0))
            .pr_2()
            .bg(rgb(bg))
            .border_l_2()
            .border_color(rgb(if selected {
                theme::ACCENT
            } else {
                theme::TRANSPARENT
            }))
            .text_size(px(12.0))
            .text_color(rgb(text))
            .hover(|s| s.bg(rgb(theme::PANEL_HOVER)))
            .on_click(cx.listener(move |this, event, window, cx| on_click(this, event, window, cx)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().truncate().child(label))
                    .children(sublabel.map(|s| {
                        div()
                            .text_size(px(10.0))
                            .text_color(rgb(theme::MUTED))
                            .truncate()
                            .child(s)
                    })),
            )
            .into_any_element()
    }

    pub fn render_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let keymap = self.workspace.read(cx).keymap.clone();
        let mut rows: Vec<AnyElement> = Vec::new();
        let snapshot: Vec<ProfileRow> = {
            let ws = self.workspace.read(cx);
            ws.profiles
                .iter()
                .map(|doc| {
                    let is_open = ws.open_profile.as_deref() == Some(doc.file_name.as_str());
                    let name = if doc.file.profile.name.is_empty() {
                        doc.file_name.clone()
                    } else {
                        doc.file.profile.name.clone()
                    };
                    let badge = if doc.file.profile.apps.is_empty() {
                        None
                    } else {
                        Some(format!("{} apps", doc.file.profile.apps.len()))
                    };
                    let macros = doc
                        .file
                        .macros
                        .iter()
                        .filter(|m| m.is_deleted == 0)
                        .map(|m| {
                            let trigger = doc
                                .file
                                .bindings
                                .iter()
                                .find(|b| b.macro_id.as_deref() == Some(m.id.as_str()))
                                .map(|b| crate::schema::human_trigger(&b.trigger_data, &keymap))
                                .unwrap_or_else(|| "no binding".to_string());
                            (m.id.clone(), m.name.clone(), trigger)
                        })
                        .collect();
                    ProfileRow {
                        file_name: doc.file_name.clone(),
                        name,
                        badge,
                        is_open,
                        macros,
                    }
                })
                .collect()
        };
        let selected_macro = self.workspace.read(cx).selected_macro.clone();
        for ProfileRow {
            file_name,
            name,
            badge,
            is_open,
            macros,
        } in &snapshot
        {
            let file_name = file_name.clone();
            let is_open = *is_open;
            rows.push(Self::row(
                name.clone(),
                badge.clone(),
                is_open,
                0,
                gpui::ElementId::Name(gpui::SharedString::from(format!("profile-{file_name}"))),
                {
                    let file_name = file_name.clone();
                    move |this, _, _, cx| {
                        this.workspace.update(cx, |ws, cx| {
                            ws.select_profile(&file_name);
                            cx.notify();
                        })
                    }
                },
                cx,
            ));
            if is_open {
                for (mid, mname, trigger) in macros {
                    let selected = selected_macro.as_deref() == Some(mid.as_str());
                    rows.push(Self::row(
                        mname.clone(),
                        Some(trigger.clone()),
                        selected,
                        1,
                        gpui::ElementId::Name(gpui::SharedString::from(format!("macro-{mid}"))),
                        {
                            let id = mid.clone();
                            move |this, _, _, cx| {
                                this.workspace.update(cx, |ws, cx| {
                                    ws.select_macro(&id);
                                    cx.notify();
                                })
                            }
                        },
                        cx,
                    ));
                }
            }
        }
        div()
            .id("sidebar-scroll")
            .flex()
            .flex_col()
            .w(relative(1.0))
            .h_full()
            .overflow_y_scroll()
            .children(rows)
            .into_any_element()
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = self.render_sidebar(cx);
        div()
            .flex()
            .flex_col()
            .w(px(theme::SIDEBAR_W))
            .h_full()
            .bg(rgb(theme::PANEL))
            .border_r_1()
            .border_color(rgb(theme::BORDER))
            .child(list)
    }
}
