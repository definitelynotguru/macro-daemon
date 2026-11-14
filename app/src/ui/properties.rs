use gpui::{
    div, px, rgb, App, AppContext, Focusable as _, Context, Div, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, Stateful, StatefulInteractiveElement, Styled, Window,
};

use crate::theme;
use crate::ui::input::InputField;
use crate::workspace::Workspace;

pub struct Properties {
    workspace: Entity<Workspace>,
    app_rule_input: Entity<InputField>,
}

impl Properties {
    pub fn new(workspace: Entity<Workspace>, cx: &mut gpui::App) -> Self {
        let app_rule_input = cx.new(|cx| InputField::new(cx.focus_handle(), "add path rule (Enter)"));
        Self {
            workspace,
            app_rule_input,
        }
    }

    fn add_rule_from_input(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.app_rule_input.read(cx).text().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.workspace.update(cx, |ws, cx| {
            ws.add_app_rule(text);
            cx.notify();
        });
        self.app_rule_input.update(cx, |input, cx| {
            input.set_text("", cx);
        });
    }
}

fn section_label(text: &str) -> gpui::AnyElement {
    div()
        .text_size(px(10.0))
        .text_color(rgb(theme::MUTED))
        .child(text.to_string())
        .into_any_element()
}

fn pill(id: &'static str, label: &str, muted: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(theme::BUTTON))
        .text_size(px(11.0))
        .text_color(rgb(if muted { theme::MUTED } else { theme::TEXT }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme::PANEL_HOVER)))
        .child(label.to_string())
}

fn field_box(text: String) -> gpui::AnyElement {
    div()
        .px_2()
        .py_1()
        .min_w_0()
        .bg(rgb(theme::BG))
        .rounded_sm()
        .text_size(px(12.0))
        .text_color(rgb(theme::TEXT))
        .child(text)
        .into_any_element()
}

fn named_id(prefix: &'static str, n: usize) -> gpui::ElementId {
    gpui::ElementId::NamedInteger(prefix.into(), n as u64)
}

impl Render for Properties {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        if ws.selected_macro.is_none() {
            return div()
                .flex()
                .flex_col()
                .w(px(theme::PROPS_W))
                .h_full()
                .bg(rgb(theme::PANEL))
                .border_l_1()
                .border_color(rgb(theme::BORDER))
                .p_3()
                .text_size(px(12.0))
                .text_color(rgb(theme::MUTED))
                .child("select a macro to edit its properties")
                .into_any_element();
        }
        let (Some(doc), Some(macro_id)) = (ws.open(), ws.selected_macro.clone()) else {
            return div().w(px(theme::PROPS_W)).h_full().into_any_element();
        };
        let trigger = doc
            .file
            .bindings
            .iter()
            .find(|b| b.macro_id.as_deref() == Some(macro_id.as_str()))
            .map(|b| b.trigger_data);
        let playback = doc
            .file
            .bindings
            .iter()
            .find(|b| b.macro_id.as_deref() == Some(macro_id.as_str()))
            .map(|b| b.playback_options_data);
        let apps = &doc.file.profile.apps;
        let mut children: Vec<gpui::AnyElement> = Vec::new();
        children.push(section_label("trigger").into_any_element());
        match trigger {
            Some(t) => {
                children.push(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .children(
                            crate::keymenu::KEY_CHOICES.iter().map(|(code, label)| {
                                pill_trigger_direction(
                                    *code,
                                    *label,
                                    t,
                                    macro_id.clone(),
                                    self.workspace.clone(),
                                )
                                .into_any_element()
                            }),
                        )
                        .into_any_element(),
                );
                children.push(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            pill_direction(
                                "dir-press",
                                "press",
                                t.direction > 0,
                                macro_id.clone(),
                                self.workspace.clone(),
                            )
                            .into_any_element(),
                        )
                        .child(
                            pill_direction(
                                "dir-release",
                                "release",
                                t.direction < 0,
                                macro_id.clone(),
                                self.workspace.clone(),
                            )
                            .into_any_element(),
                        )
                        .into_any_element(),
                );
            }
            None => children.push(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(theme::MUTED))
                    .child("no binding")
                    .into_any_element(),
            ),
        }
        children.push(section_label("playback").into_any_element());
        if let Some(p) = playback {
            children.push(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(
                        crate::schema::PLAYBACK_TABLE.iter().map(|entry| {
                            pill_playback(
                                entry.code,
                                entry.label,
                                p.playback_type,
                                macro_id.clone(),
                                self.workspace.clone(),
                            )
                            .into_any_element()
                        }),
                    )
                    .into_any_element(),
            );
            if p.playback_type.uses_num_repeat() {
                children.push(section_label("num_repeat").into_any_element());
                children.push(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            pill_num_repeat("nr-minus", macro_id.clone(), self.workspace.clone(), -1)
                                .into_any_element(),
                        )
                        .child(field_box(p.num_repeat.to_string()))
                        .child(
                            pill_num_repeat("nr-plus", macro_id.clone(), self.workspace.clone(), 1)
                                .into_any_element(),
                        )
                        .into_any_element(),
                );
            }
            if p.playback_type.uses_repeat_delay() {
                children.push(section_label("repeat_delay (ms)").into_any_element());
                children.push(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            pill_repeat_delay(
                                "rd-minus",
                                macro_id.clone(),
                                self.workspace.clone(),
                                -5,
                            )
                            .into_any_element(),
                        )
                        .child(field_box(p.repeat_delay.to_string()))
                        .child(
                            pill_repeat_delay("rd-plus", macro_id.clone(), self.workspace.clone(), 5)
                                .into_any_element(),
                        )
                        .into_any_element(),
                );
            }
            children.push(section_label("direction").into_any_element());
            children.push(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        pill_direction_playback(
                            "pb-forward",
                            crate::schema::direction_label(1),
                            p.direction > 0,
                            macro_id.clone(),
                            self.workspace.clone(),
                        )
                        .into_any_element(),
                    )
                    .child(
                        pill_direction_playback(
                            "pb-reverse",
                            crate::schema::direction_label(-1),
                            p.direction < 0,
                            macro_id.clone(),
                            self.workspace.clone(),
                        )
                        .into_any_element(),
                    )
                    .into_any_element(),
            );
        }
        children.push(section_label("per-app rules").into_any_element());
        for (i, app) in apps.iter().enumerate() {
            children.push(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(theme::TEXT))
                            .truncate()
                            .child(app.clone()),
                    )
                    .child(
                        div()
                            .id(named_id("app-x", i))
                            .px_1()
                            .text_color(rgb(theme::MUTED))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(theme::ACCENT)))
                            .child("×")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.workspace.update(cx, |ws, cx| {
                                    ws.remove_app_rule(i);
                                    cx.notify();
                                })
                            })),
                    )
                    .into_any_element(),
            );
        }
        children.push(
            div()
                .flex()
                .items_center()
                .gap_1()
                .key_context("AppRuleInput")
                .track_focus(&self.app_rule_input.read(cx).focus_handle(cx))
                .capture_key_down(cx.listener(
                    |this, event: &gpui::KeyDownEvent, window, cx| {
                        if event.keystroke.key == "enter" {
                            this.add_rule_from_input(window, cx);
                            cx.stop_propagation();
                        }
                    },
                ))
                .child(self.app_rule_input.clone())
                .child(
                    div()
                        .id("app-add")
                        .px_2()
                        .py_1()
                        .bg(rgb(theme::BUTTON))
                        .rounded_sm()
                        .text_size(px(11.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(theme::PANEL_HOVER)))
                        .child("+ add")
                        .on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, window, cx| {
                                this.add_rule_from_input(window, cx)
                            },
                        ))
                        .into_any_element(),
                )
                .into_any_element(),
        );
        div()
            .id("props-scroll")
            .flex()
            .flex_col()
            .gap_2()
            .w(px(theme::PROPS_W))
            .h_full()
            .bg(rgb(theme::PANEL))
            .border_l_1()
            .border_color(rgb(theme::BORDER))
            .p_3()
            .overflow_y_scroll()
            .text_color(rgb(theme::TEXT))
            .children(children)
            .into_any_element()
    }
}

fn pill_trigger_direction(
    code: u16,
    label: &'static str,
    t: macro_daemon::config::TriggerData,
    macro_id: String,
    workspace: Entity<Workspace>,
) -> Stateful<Div> {
    let selected = t.hid_code == code && t.page == 1;
    let display = if selected {
        crate::schema::key_name(t.page, t.hid_code, &crate::schema::keymap_cached())
    } else {
        label.to_string()
    };
    let id: &'static str = Box::leak(format!("trg-{code}").into_boxed_str());
    pill(id, &display, !selected).on_click(move |_, _, cx: &mut App| {
        workspace.update(cx, |ws, cx| {
            ws.set_trigger(&macro_id, |tr| {
                tr.hid_code = code;
                tr.page = 1;
                tr.trigger_type = 2;
            });
            cx.notify();
        })
    })
}

fn pill_direction(
    id: &'static str,
    label: &'static str,
    selected: bool,
    macro_id: String,
    workspace: Entity<Workspace>,
) -> Stateful<Div> {
    let delta: i8 = if id == "dir-press" { 1 } else { -1 };
    pill(id, label, !selected).on_click(move |_, _, cx: &mut App| {
        workspace.update(cx, |ws, cx| {
            ws.set_trigger(&macro_id, |tr| tr.direction = delta);
            cx.notify();
        })
    })
}

fn pill_playback(
    code: u8,
    label: &'static str,
    current: macro_daemon::config::PlaybackType,
    macro_id: String,
    workspace: Entity<Workspace>,
) -> Stateful<Div> {
    let selected = current
        == macro_daemon::config::PlaybackType::from_client(code).unwrap_or(current);
    let id: &'static str = Box::leak(format!("pb-{code}").into_boxed_str());
    pill(id, label, !selected).on_click(move |_, _, cx: &mut App| {
        workspace.update(cx, |ws, cx| {
            ws.set_playback(&macro_id, |p| {
                if let Ok(t) = macro_daemon::config::PlaybackType::from_client(code) {
                    p.playback_type = t;
                }
            });
            cx.notify();
        })
    })
}

fn pill_direction_playback(
    id: &'static str,
    label: &'static str,
    selected: bool,
    macro_id: String,
    workspace: Entity<Workspace>,
) -> Stateful<Div> {
    let delta: i8 = if id == "pb-forward" { 1 } else { -1 };
    pill(id, label, !selected).on_click(move |_, _, cx: &mut App| {
        workspace.update(cx, |ws, cx| {
            ws.set_direction(&macro_id, delta);
            cx.notify();
        })
    })
}

fn pill_num_repeat(
    id: &'static str,
    macro_id: String,
    workspace: Entity<Workspace>,
    delta: i16,
) -> Stateful<Div> {
    pill(id, if delta < 0 { "−" } else { "+" }, false)
        .on_click(move |_, _, cx: &mut App| {
            workspace.update(cx, |ws, cx| {
                let current = ws
                    .selected_binding()
                    .map(|b| b.playback_options_data.num_repeat)
                    .unwrap_or(1);
                let next = if delta < 0 {
                    current.saturating_sub(delta.unsigned_abs() as u8).max(1)
                } else {
                    current.saturating_add(delta as u8)
                };
                ws.set_num_repeat(&macro_id, next);
                cx.notify();
            })
        })
}

fn pill_repeat_delay(
    id: &'static str,
    macro_id: String,
    workspace: Entity<Workspace>,
    delta: i16,
) -> Stateful<Div> {
    pill(id, if delta < 0 { "−" } else { "+" }, false)
        .on_click(move |_, _, cx: &mut App| {
            workspace.update(cx, |ws, cx| {
                let current = ws
                    .selected_binding()
                    .map(|b| b.playback_options_data.repeat_delay)
                    .unwrap_or(15);
                let next = if delta < 0 {
                    current.saturating_sub(delta.unsigned_abs() as u16)
                } else {
                    current.saturating_add(delta as u16)
                };
                ws.set_repeat_delay(&macro_id, next);
                cx.notify();
            })
        })
}
