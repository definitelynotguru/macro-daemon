use macro_daemon::config::{EventType, MacroEvent};
use rune::tok::{motion, TextStyle};
use rune::{color, Color, Cx, El, ScrollSpec, Shape, col, el, row, text};

use crate::schema::{default_binding, event_label, key_name, new_macro_id};
use crate::workspace::{MacroEntry, Status, Workspace};

fn srgb(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: color::srgb_decode(r as f32 / 255.0),
        g: color::srgb_decode(g as f32 / 255.0),
        b: color::srgb_decode(b as f32 / 255.0),
        a: 1.0,
    }
}

fn window() -> Color { srgb(42, 47, 54) }
fn rail_bg() -> Color { srgb(34, 39, 45) }
fn center_bg() -> Color { srgb(46, 52, 59) }
fn canvas_bg() -> Color { srgb(106, 114, 124) }
fn orange() -> Color { srgb(255, 106, 0) }
fn teal() -> Color { srgb(47, 143, 140) }
fn delay_bg() -> Color { srgb(22, 24, 28) }
fn button_bg() -> Color { srgb(74, 82, 92) }
fn revert_bg() -> Color { srgb(58, 65, 74) }
fn text_color() -> Color { srgb(232, 234, 236) }
fn muted() -> Color { srgb(154, 160, 168) }
fn row_text() -> Color { srgb(210, 214, 218) }
fn dot() -> Color { srgb(141, 148, 156) }
fn record_red() -> Color { srgb(226, 57, 50) }
fn line() -> Color { srgb(210, 214, 218) }

const LABEL: TextStyle = TextStyle::new(11.0, 600.0);
const ROW: TextStyle = TextStyle::new(12.5, 500.0);
const TITLE: TextStyle = TextStyle::new(13.0, 600.0);
const CHIP: TextStyle = TextStyle::new(12.0, 500.0);
const BTN: TextStyle = TextStyle::new(11.0, 600.0);
const CAPTION: TextStyle = TextStyle::new(11.0, 450.0);

pub fn view(ws: &Workspace, _: &mut Cx<Workspace>) -> El {
    row()
        .key("editor")
        .w_full()
        .h_full()
        .fill(window())
        .child(rail(ws))
        .child(stage(ws))
}

pub fn select_opening(ws: &mut Workspace) {
    let error = load_error(ws);
    let entries = ws.entries();
    let pick = entries
        .iter()
        .find(|entry| entry_has_delay(ws, entry))
        .or(entries.first())
        .cloned();
    if let Some(entry) = pick {
        ws.select_entry(&entry.file_name, &entry.id);
    }
    restore_load_error(ws, error);
}

fn load_error(ws: &Workspace) -> Option<String> {
    match &ws.status {
        Status::Error(text) => Some(text.clone()),
        _ => None,
    }
}

fn restore_load_error(ws: &mut Workspace, error: Option<String>) {
    if let Some(text) = error {
        ws.status = Status::Error(text);
    }
}

fn entry_has_delay(ws: &Workspace, entry: &MacroEntry) -> bool {
    ws.profiles
        .iter()
        .find(|doc| doc.file_name == entry.file_name)
        .and_then(|doc| doc.file.macro_by_id(&entry.id))
        .is_some_and(|macro_def| {
            macro_def
                .events
                .iter()
                .any(|event| event.event_type == EventType::Delay)
        })
}

fn rail(ws: &Workspace) -> El {
    let entries = ws.entries();
    let rows = entries.iter().map(|entry| {
        let selected = ws.open_profile.as_deref() == Some(entry.file_name.as_str())
            && ws.selected_macro.as_deref() == Some(entry.id.as_str());
        macro_row(entry, selected)
    });
    col()
        .key("rail")
        .w(248.0)
        .h_full()
        .fill(rail_bg())
        .child(
            el().px(14.0)
                .pt(14.0)
                .pb(8.0)
                .child(text("MACROS").font(LABEL).color(muted())),
        )
        .child(
            col()
                .key("macro-list")
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec { stick_bottom: false })
                .children(rows),
        )
        .child(
            el().px(10.0)
                .py(10.0)
                .child(action("new", "+  NEW", button_bg(), text_color(), new_macro)),
        )
}

fn macro_row(entry: &MacroEntry, selected: bool) -> El {
    let file_name = entry.file_name.clone();
    let id = entry.id.clone();
    let name = entry.name.clone();
    row()
        .key(format!("row/{file_name}/{id}"))
        .w_full()
        .h(30.0)
        .px(12.0)
        .gap(8.0)
        .items_center()
        .fill(if selected { orange() } else { rail_bg() })
        .motion(motion::SNAPPY)
        .role("radio")
        .label(name.clone())
        .checked(selected)
        .focusable(true)
        .on_press(move |ws: &mut Workspace| ws.select_entry(&file_name, &id))
        .child(radio(selected))
        .child(text(name).font(ROW).color(if selected { Color::WHITE } else { row_text() }))
}

fn radio(on: bool) -> El {
    let mark = el().size(8.0, 8.0).radius(4.0);
    if on {
        mark.fill(Color::WHITE)
    } else {
        mark.border(1.5, dot())
    }
}

fn stage(ws: &Workspace) -> El {
    col()
        .key("stage")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(center_bg())
        .padding(16.0)
        .gap(10.0)
        .child(heading(ws))
        .child(
            col()
                .key("canvas")
                .grow(1.0)
                .min_h(0.0)
                .radius(4.0)
                .fill(canvas_bg())
                .child(chip_strip(ws)),
        )
        .child(bottom_bar(ws))
}

fn heading(ws: &Workspace) -> El {
    let name = ws
        .selected()
        .map(|macro_def| macro_def.name.to_uppercase())
        .unwrap_or_else(|| "NO MACRO".to_string());
    let mut block = col().gap(4.0).child(text(name).font(TITLE).color(text_color()));
    let note = match &ws.status {
        Status::Idle => None,
        Status::Info(text) | Status::Error(text) => Some(text.clone()),
    };
    if let Some(note) = note {
        block = block.child(text(note).font(CAPTION).color(muted()));
    }
    block
}

fn chip_strip(ws: &Workspace) -> El {
    let mut strip = row()
        .key("strip")
        .padding(14.0)
        .gap(8.0)
        .items_center()
        .scroll_x();
    let Some(macro_def) = ws.selected() else {
        return strip;
    };
    let mut first = true;
    for (index, event) in macro_def.events.iter().enumerate() {
        if !first {
            strip = strip.child(connector());
        }
        first = false;
        strip = strip.child(chip(ws, index, event));
    }
    strip
}

fn chip(ws: &Workspace, index: usize, event: &MacroEvent) -> El {
    match event.event_type {
        EventType::Delay => delay_chip(index, event.extra_data),
        EventType::KeyboardKey => {
            let label = key_name(event.page, event.code, &ws.keymap);
            key_chip(index, &label, event.extra_data != 0)
        }
        _ => {
            let label = event_label(event, &ws.keymap);
            row()
                .key(format!("event-{index}"))
                .h(28.0)
                .px(8.0)
                .radius(3.0)
                .fill(teal())
                .items_center()
                .child(text(label).font(CHIP).color(Color::WHITE))
        }
    }
}

fn key_chip(index: usize, label: &str, down: bool) -> El {
    row()
        .key(format!("key-{index}"))
        .h(28.0)
        .px(6.0)
        .gap(5.0)
        .radius(3.0)
        .fill(teal())
        .items_center()
        .child(key_glyph())
        .child(text(label.to_string()).font(CHIP).color(Color::WHITE))
        .child(arrow(down))
}

fn delay_chip(index: usize, ms: u32) -> El {
    row()
        .key(format!("delay-{index}"))
        .h(28.0)
        .px(8.0)
        .gap(5.0)
        .radius(3.0)
        .fill(delay_bg())
        .items_center()
        .child(clock_glyph())
        .child(text(format!("{ms} ms")).font(CHIP).color(Color::WHITE))
}

fn connector() -> El {
    el().size(14.0, 1.5).radius(1.0).fill(line())
}

fn key_glyph() -> El {
    el().size(14.0, 10.0)
        .radius(1.5)
        .border(1.0, Color::WHITE)
        .child(
            row()
                .absolute(2.0, 2.0)
                .gap(1.0)
                .child(el().size(2.0, 2.0).radius(0.4).fill(Color::WHITE))
                .child(el().size(2.0, 2.0).radius(0.4).fill(Color::WHITE))
                .child(el().size(2.0, 2.0).radius(0.4).fill(Color::WHITE)),
        )
}

fn clock_glyph() -> El {
    el().size(12.0, 12.0)
        .radius(6.0)
        .border(1.0, Color::WHITE)
        .child(el().absolute(5.5, 2.0).size(1.0, 4.0).fill(Color::WHITE))
        .child(el().absolute(5.5, 5.5).size(3.0, 1.0).fill(Color::WHITE))
}

fn arrow(down: bool) -> El {
    let points = if down {
        vec![[1.0, 1.0], [5.0, 7.0], [9.0, 1.0]]
    } else {
        vec![[1.0, 7.0], [5.0, 1.0], [9.0, 7.0]]
    };
    el().size(10.0, 8.0)
        .shape(Shape::new().stroke(points, 1.3, Color::WHITE))
}

fn bottom_bar(ws: &Workspace) -> El {
    let dirty = ws.dirty();
    row()
        .key("bottom")
        .w_full()
        .h(36.0)
        .gap(8.0)
        .items_center()
        .child(gear())
        .child(action("clear", "CLEAR", button_bg(), text_color(), Workspace::clear_selected_events))
        .child(el().grow(1.0))
        .child(action("start", "START", button_bg(), text_color(), Workspace::play_selected))
        .child(record())
        .child(revert(dirty))
        .child(action("save", "SAVE", orange(), Color::WHITE, save_profile))
}

fn gear() -> El {
    el().key("gear")
        .size(28.0, 28.0)
        .radius(4.0)
        .fill(button_bg())
        .center()
        .label("Settings")
        .on_press(|ws: &mut Workspace| {
            ws.status = Status::Info(format!("{}", ws.dir.display()));
        })
        .child(
            el().size(12.0, 12.0)
                .radius(6.0)
                .border(1.5, text_color())
                .child(el().absolute(4.0, 4.0).size(4.0, 4.0).radius(2.0).fill(button_bg())),
        )
}

fn record() -> El {
    el().key("record")
        .size(28.0, 28.0)
        .radius(14.0)
        .fill(record_red())
        .label("Record")
        .on_press(|ws: &mut Workspace| {
            ws.status = Status::Info(
                "recording needs a keyboard; nothing was written".to_string(),
            );
        })
}

fn revert(dirty: bool) -> El {
    let fill = revert_bg();
    let fg = muted();
    let button = action("revert", "REVERT", fill, fg, revert_edits).opacity(if dirty { 1.0 } else { 0.45 });
    button
}

fn action(
    key: &'static str,
    label: &'static str,
    fill: Color,
    fg: Color,
    press: fn(&mut Workspace),
) -> El {
    el().key(key)
        .h(28.0)
        .px(12.0)
        .radius(4.0)
        .fill(fill)
        .center()
        .label(label)
        .on_press(press)
        .child(text(label).font(BTN).color(fg))
}

fn new_macro(ws: &mut Workspace) {
    if ws.open().is_none() {
        ws.status = Status::Error("no profile open".to_string());
        return;
    }
    let id = new_macro_id();
    let binding = default_binding(&id);
    if ws.new_macro(id, binding) {
        ws.status = Status::Info("new macro".to_string());
    } else {
        ws.status = Status::Error("could not add macro".to_string());
    }
}

fn save_profile(ws: &mut Workspace) {
    match ws.save_open_profile() {
        Ok(()) => {}
        Err(err) => ws.status = Status::Error(format!("{err:#}")),
    }
}

fn revert_edits(ws: &mut Workspace) {
    if !ws.dirty() {
        return;
    }
    let selected = ws.selected_macro.clone();
    let open = ws.open_profile.clone();
    ws.reload();
    let error = load_error(ws);
    if let (Some(file_name), Some(id)) = (open, selected) {
        ws.select_entry(&file_name, &id);
    } else {
        select_opening(ws);
    }
    if error.is_some() {
        restore_load_error(ws, error);
        return;
    }
    ws.status = Status::Info("reverted".to_string());
}
