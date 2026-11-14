use gpui::{div, px, rgb, Div, InteractiveElement, ParentElement, Stateful, Styled};

trait When: Sized {
    fn when(self, condition: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if condition {
            f(self)
        } else {
            self
        }
    }
}

impl When for Div {}
impl<T: Sized> When for Stateful<T> {}

use crate::theme;

pub fn toolbar_button(id: &'static str, label: &'static str, enabled: bool) -> Stateful<Div> {
    let text = if enabled { theme::TEXT } else { theme::MUTED };
    let bg = if enabled { theme::BUTTON } else { theme::PANEL };
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(bg))
        .text_size(px(12.0))
        .text_color(rgb(text))
        .when(enabled, |b| {
            b.cursor_pointer().hover(|s| s.bg(rgb(theme::PANEL_HOVER)))
        })
        .child(label.to_string())
}
