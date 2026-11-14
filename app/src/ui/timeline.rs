use gpui::{
    div, px, rgb, AnyElement, App, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};

use crate::theme;
use crate::workspace::Workspace;

pub struct Timeline {
    workspace: Entity<Workspace>,
    delay_editing: Option<usize>,
    delay_value: Option<u32>,
    delay_focus: gpui::FocusHandle,
}

struct BlockSpec {
    id: usize,
    label: String,
    width: f32,
    bg: u32,
    text: u32,
    round_left: bool,
    round_right: bool,
}

impl BlockSpec {
    fn render(&self, selected: bool, workspace: gpui::Entity<crate::workspace::Workspace>) -> gpui::AnyElement {
        let base = div()
            .id(gpui::ElementId::from(SharedString::from(format!("ev-{}", self.id))))
            .flex()
            .items_center()
            .justify_center()
            .h_9()
            .w(px(self.width))
            .min_w_0()
            .overflow_hidden()
            .text_size(px(11.0))
            .text_color(rgb(self.text))
            .bg(rgb(self.bg))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(theme::PANEL_HOVER)))
            .child(self.label.clone());
        let base = if self.round_left {
            base.rounded_l_sm()
        } else {
            base
        };
        let base = if self.round_right {
            base.rounded_r_sm()
        } else {
            base
        };
        let base = if selected {
            base.border_1().border_color(rgb(theme::ACCENT))
        } else {
            base
        };
        let event_id = self.id;
        base.on_click(move |_, _, cx: &mut gpui::App| {
            workspace.update(cx, |ws, cx| {
                ws.select_event(event_id);
                cx.notify();
            })
        })
        .into_any_element()
    }
}

impl Timeline {
    pub fn new(workspace: Entity<Workspace>, cx: &mut gpui::App) -> Self {
        Self {
            workspace,
            delay_editing: None,
            delay_value: None,
            delay_focus: cx.focus_handle(),
        }
    }

    fn open_delay_editor(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.delay_editing = Some(index);
        if let Some(ms) = self
            .workspace
            .read(cx)
            .selected()
            .and_then(|m| m.events.get(index))
            .map(|e| e.extra_data)
        {
            self.delay_value = Some(ms);
        }
        self.delay_focus.focus(window);
        cx.notify();
    }

    fn commit_delay(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(ms) = self.delay_value.take() else {
            return;
        };
        self.delay_editing = None;
        self.workspace.update(cx, |ws, cx| {
            let Some(macro_id) = ws.selected_macro.clone() else {
                return;
            };
            ws.update_event(&macro_id, index, |e| e.extra_data = ms);
            cx.notify();
        })
    }

    fn strip(&mut self, cx: &mut Context<Self>) -> (Vec<AnyElement>, Vec<usize>) {
        let ws = self.workspace.read(cx);
        let Some(m) = ws.selected().cloned() else {
            return (Vec::new(), Vec::new());
        };
        let mut blocks: Vec<BlockSpec> = Vec::new();
        let mut delay_ids: Vec<usize> = Vec::new();
        for (i, ev) in m.events.iter().enumerate() {
            let label = crate::schema::event_label(ev, &ws.keymap);
            match ev.event_type {
                macro_daemon::config::EventType::Delay => {
                    delay_ids.push(i);
                    blocks.push(BlockSpec {
                        id: i,
                        width: delay_width(ev.extra_data),
                        label,
                        bg: theme::DELAY_BG,
                        text: theme::DELAY_TEXT,
                        round_left: true,
                        round_right: true,
                    });
                }
                macro_daemon::config::EventType::KeyboardKey => {
                    let down = ev.extra_data > 0;
                    let join = pair_neighbor(&m.events, i, down);
                    blocks.push(BlockSpec {
                        id: i,
                        width: crate::schema::KEY_EVENT_WIDTH_PX,
                        label,
                        bg: if down { theme::KEY_DOWN_BG } else { theme::KEY_UP_BG },
                        text: theme::TEXT,
                        round_left: !join.left,
                        round_right: !join.right,
                    });
                }
                _ => {
                    blocks.push(BlockSpec {
                        id: i,
                        width: crate::schema::KEY_EVENT_WIDTH_PX,
                        label,
                        bg: theme::MOUSE_BG,
                        text: theme::TEXT,
                        round_left: true,
                        round_right: true,
                    });
                }
            }
        }
        let elements = blocks
            .into_iter()
            .map(|b| b.render(ws.selected_event == Some(b.id), self.workspace.clone()))
            .collect();
        (elements, delay_ids)
    }
}

struct PairJoin {
    left: bool,
    right: bool,
}

fn pair_neighbor(events: &[macro_daemon::config::MacroEvent], i: usize, down: bool) -> PairJoin {
    let mut left = false;
    let mut right = false;
    if i > 0 {
        let prev = &events[i - 1];
        if prev.event_type == macro_daemon::config::EventType::KeyboardKey
            && prev.page == events[i].page
            && prev.code == events[i].code
        {
            left = down;
        }
    }
    if i + 1 < events.len() {
        let next = &events[i + 1];
        if next.event_type == macro_daemon::config::EventType::KeyboardKey
            && next.page == events[i].page
            && next.code == events[i].code
        {
            right = !down;
        }
    }
    PairJoin { left, right }
}

fn delay_width(ms: u32) -> f32 {
    let w = 36.0 + (ms as f32).log10().max(0.0) * 6.0;
    w.clamp(44.0, 160.0)
}

impl Render for Timeline {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        let macro_name = ws.selected().map(|m| m.name.clone());
        let total_ms = ws
            .selected()
            .map(|m| crate::schema::macro_total_ms(&m.events))
            .unwrap_or(0);
        let has_selection = ws.selected().is_some();
        let (blocks, delay_ids) = self.strip(cx);
        let selected_event = self.workspace.read(cx).selected_event;
        if let Some(i) = selected_event {
            if delay_ids.contains(&i) && self.delay_editing != Some(i) {
                self.open_delay_editor(i, _window, cx);
            }
        }
        let delay_editor = selected_event
            .filter(|i| delay_ids.contains(i))
            .map(|i| render_delay_editor(self, i, _window, cx));
        let empty_hint = div()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.0))
            .text_color(rgb(theme::MUTED))
            .child("select a macro to see its timeline");
        let strip = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1()
            .p_2()
            .children(blocks);
        div()
            .id("timeline-root")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(rgb(theme::BG))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.workspace.update(cx, |ws, cx| {
                        ws.deselect_event();
                        cx.notify();
                    })
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .h_7()
                    .text_size(px(11.0))
                    .text_color(rgb(theme::MUTED))
                    .child(match macro_name {
                        Some(name) => format!("{name}  ·  {total_ms}ms total"),
                        None => "timeline".to_string(),
                    }),
            )
            .child(
                div()
                    .id("tl-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(if has_selection {
                        strip.into_any_element()
                    } else {
                        empty_hint.into_any_element()
                    }),
            )
            .children(delay_editor)
    }
}

impl Timeline {
    fn delay_input_row(&mut self, index: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let text = self
            .delay_value
            .map(|v| v.to_string())
            .unwrap_or_default();
        let focused = self.delay_focus.is_focused(window);
        div()
            .id(gpui::ElementId::NamedInteger("delay-input".into(), index as u64))
            .min_w_16()
            .px_2()
            .py_1()
            .bg(rgb(theme::BG))
            .rounded_sm()
            .border_b_1()
            .border_color(rgb(if focused { theme::ACCENT } else { theme::BORDER }))
            .text_size(px(12.0))
            .text_color(rgb(if text.is_empty() { theme::MUTED } else { theme::TEXT }))
            .track_focus(&self.delay_focus)
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                let digit = event
                    .keystroke
                    .key_char
                    .clone()
                    .filter(|ch| ch.chars().all(|c| c.is_ascii_digit()))
                    .or_else(|| {
                        (key.len() == 1 && key.chars().next().is_some_and(|c| c.is_ascii_digit()))
                            .then(|| key.to_string())
                    });
                if key == "enter" {
                    this.commit_delay(index, cx);
                    cx.stop_propagation();
                    return;
                }
                if key == "escape" {
                    this.delay_value = None;
                    this.delay_editing = None;
                    cx.notify();
                    return;
                }
                if key == "backspace" {
                    let current = this.delay_value.unwrap_or(0).to_string();
                    this.delay_value = current[..current.len().saturating_sub(1)]
                        .parse::<u32>()
                        .ok();
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if let Some(digit) = digit {
                    let current = this.delay_value.unwrap_or(0).to_string();
                    if let Ok(next) = format!("{current}{digit}").parse::<u32>() {
                        this.delay_value = Some(next);
                        cx.notify();
                    }
                    cx.stop_propagation();
                }
            }))
            .child(if text.is_empty() {
                "ms (Enter)".to_string()
            } else {
                text
            })
            .into_any_element()
    }
}

fn render_delay_editor(
    this: &mut Timeline,
    index: usize,
    window: &mut Window,
    cx: &mut Context<Timeline>,
) -> AnyElement {
    div()
        .id(gpui::ElementId::NamedInteger("delay-editor".into(), index as u64))
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .bg(rgb(theme::PANEL))
        .border_t_1()
        .border_color(rgb(theme::BORDER))
        .on_mouse_down(
            gpui::MouseButton::Left,
            |_, _, cx: &mut App| cx.stop_propagation(),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(theme::MUTED))
                .child(format!("delay #{}", index)),
        )
        .child(nudge_button("delay-minus", "−10", -10i64, index, cx))
        .child(this.delay_input_row(index, window, cx))
        .child(nudge_button("delay-plus", "+10", 10, index, cx))
        .into_any_element()
}

fn nudge_button(
    id: &'static str,
    label: &'static str,
    delta: i64,
    index: usize,
    cx: &mut Context<Timeline>,
) -> AnyElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .bg(rgb(theme::BUTTON))
        .rounded_sm()
        .text_size(px(11.0))
        .text_color(rgb(theme::TEXT))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme::PANEL_HOVER)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.workspace.update(cx, |ws, cx| {
                let Some(macro_id) = ws.selected_macro.clone() else {
                    return;
                };
                let Some(m) = ws.selected() else {
                    return;
                };
                let Some(ev) = m.events.get(index) else {
                    return;
                };
                let next = (ev.extra_data as i64 + delta).clamp(0, u32::MAX as i64) as u32;
                ws.update_event(&macro_id, index, |e| e.extra_data = next);
                cx.notify();
            })
        }))
        .child(label)
        .into_any_element()
}
