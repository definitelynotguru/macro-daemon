use gpui::{
    div, px, rgb, Context, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, Styled, Window,
};

use crate::theme;

pub struct InputField {
    focus_handle: FocusHandle,
    content: String,
    placeholder: String,
}

impl InputField {
    pub fn new(focus_handle: FocusHandle, placeholder: &str) -> Self {
        Self {
            focus_handle,
            content: String::new(),
            placeholder: placeholder.to_string(),
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.content = text.into();
        cx.notify();
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        match key {
            "enter" => {
                cx.stop_propagation();
            }
            "backspace" => {
                self.content.pop();
                cx.notify();
            }
            "escape" => {
                self.content.clear();
                cx.notify();
            }
            _ => {
                let ch = event
                    .keystroke
                    .key_char
                    .clone()
                    .or_else(|| (key.len() == 1).then(|| key.to_string()));
                if let Some(ch) = ch {
                    if !ch.chars().any(|c| c.is_control()) {
                        self.content.push_str(&ch);
                        cx.notify();
                    }
                }
            }
        }
        cx.stop_propagation();
    }
}

impl Focusable for InputField {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InputField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let text = if self.content.is_empty() {
            self.placeholder.clone()
        } else {
            self.content.clone()
        };
        let color = if self.content.is_empty() {
            theme::MUTED
        } else {
            theme::TEXT
        };
        let border = if focused {
            theme::ACCENT
        } else {
            theme::BORDER
        };
        div()
            .id("input-field")
            .flex()
            .items_center()
            .px_2()
            .h_7()
            .min_w_0()
            .flex_1()
            .bg(rgb(theme::BG))
            .rounded_sm()
            .border_b_1()
            .border_color(rgb(border))
            .text_size(px(11.0))
            .text_color(rgb(color))
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .on_key_down(cx.listener(Self::on_key_down))
            .child(SharedString::from(text))
    }
}
