//! HID → evdev translation.
//!
//! `keymap.json` (extracted from the recovered macro editor, MACRO_SPEC.md §C.3) is the only
//! artifact taken from the original product. Events carry raw USB HID usage ids plus a usage
//! page (§C.1), so the whole translation is a table lookup — no VK codes anywhere.
//!
//! Unknown usages are *skipped with a log*, never guessed (§C.3): a wrong keycode is worse than
//! a missing one.
use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::config::EventType;

/// Embedded at compile time so the daemon has no runtime dependency on the extraction tree.
const KEYMAP_JSON: &str = include_str!("../assets/keymap.json");

pub const PAGE_KEYBOARD: u8 = 1;
pub const PAGE_CONSUMER: u8 = 12;
/// Page 0 is not a HID page — it is where the recovered engine parks mouse buttons,
/// wheel and delay events (MACRO_SPEC.md §A.2 table).
pub const PAGE_PSEUDO: u8 = 0;

// Linux input-event-codes.h values. Spelled out rather than pulled from a second crate so the
// mapping stays auditable against keymap.json.
pub const BTN_LEFT: i32 = 0x110;
pub const BTN_RIGHT: i32 = 0x111;
pub const BTN_MIDDLE: i32 = 0x112;
pub const BTN_SIDE: i32 = 0x113;
pub const BTN_EXTRA: i32 = 0x114;
pub const BTN_FORWARD: i32 = 0x115;
pub const BTN_BACK: i32 = 0x116;
pub const BTN_TASK: i32 = 0x117;

pub const REL_HWHEEL: i32 = 0x06;
pub const REL_WHEEL: i32 = 0x08;

#[derive(Debug, Clone, Deserialize)]
struct RawKey {
    usage_page: u8,
    hid_usage: u16,
    evdev_code: Option<i32>,
    evdev_name: Option<String>,
    /// Regional keycap labels; kept for documentation even though replay never renders them.
    #[serde(default)]
    #[allow(dead_code)]
    labels: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RawDoc {
    keys: Vec<RawKey>,
}

/// One resolved key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub page: u8,
    pub code: u16,
    pub evdev: i32,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct Keymap {
    forward: HashMap<(u8, u16), Key>,
    reverse: HashMap<i32, (u8, u16)>,
    by_name: HashMap<String, i32>,
}

impl Keymap {
    pub fn load_embedded() -> Result<Self> {
        Self::from_json(KEYMAP_JSON)
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let doc: RawDoc = serde_json::from_str(text).context("parsing keymap.json")?;
        let mut forward = HashMap::new();
        let mut reverse = HashMap::new();
        let mut by_name = HashMap::new();

        for raw in doc.keys {
            let (Some(evdev), Some(name)) = (raw.evdev_code, raw.evdev_name) else {
                // keymap.json is fully resolved, but stay defensive: a null evdev entry
                // means "no Linux equivalent", which the spec says to log and skip.
                continue;
            };
            let key = Key {
                page: raw.usage_page,
                code: raw.hid_usage,
                evdev,
                name,
            };
            by_name.insert(key.name.clone(), key.evdev);
            forward.insert((key.page, key.code), key.clone());
            // First writer wins; modifier/keypad aliases that share an evdev code keep the
            // lowest HID usage, which is what a physical press should resolve to.
            reverse.entry(evdev).or_insert((key.page, key.code));
        }

        if forward.is_empty() {
            bail!("keymap.json contained no usable entries");
        }

        Ok(Self {
            forward,
            reverse,
            by_name,
        })
    }

    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    /// HID (page, code) → evdev key.
    pub fn lookup(&self, page: u8, code: u16) -> Option<&Key> {
        self.forward.get(&(page, code))
    }

    /// evdev key → HID (page, code), used to turn a physical press into a trigger match.
    pub fn reverse(&self, evdev: i32) -> Option<(u8, u16)> {
        self.reverse.get(&evdev).copied()
    }

    /// evdev constant name (e.g. `KEY_SCROLLLOCK`) → evdev code, for config hotkeys.
    pub fn by_name(&self, name: &str) -> Option<i32> {
        self.by_name.get(name).copied()
    }

    /// Resolve a key event to the evdev code to inject.
    ///
    /// Page 1 and 12 go through the table; page 0 pseudo-codes are mouse buttons
    /// (MACRO_SPEC.md §A.2: `code 1..8`).
    pub fn resolve(&self, event_type: EventType, page: u8, code: u16) -> Option<i32> {
        match event_type {
            EventType::KeyboardKey => self.lookup(page, code).map(|k| k.evdev),
            EventType::MouseButton | EventType::MouseExtraButton => mouse_button_code(code),
            EventType::MouseWheel | EventType::Delay | EventType::RepeatMarker => None,
            EventType::MouseTilt => None, // horizontal axis, injected as REL_HWHEEL
        }
    }
}

/// `code 1..8` → BTN_LEFT..BTN_TASK. The order is the one the macro editor exposes
/// ("button1".."button8", `pretty/render_463.js:54968`) and the one the onboard serializer
/// turns into the bitmask 1,2,4,8,16,32,64,128 (MACRO_SPEC.md §B.3).
pub fn mouse_button_code(code: u16) -> Option<i32> {
    Some(match code {
        1 => BTN_LEFT,
        2 => BTN_RIGHT,
        3 => BTN_MIDDLE,
        4 => BTN_SIDE,
        5 => BTN_EXTRA,
        6 => BTN_FORWARD,
        7 => BTN_BACK,
        8 => BTN_TASK,
        _ => return None,
    })
}

pub fn mouse_button_hid(evdev: i32) -> Option<u16> {
    Some(match evdev {
        BTN_LEFT => 1,
        BTN_RIGHT => 2,
        BTN_MIDDLE => 3,
        BTN_SIDE => 4,
        BTN_EXTRA => 5,
        BTN_FORWARD => 6,
        BTN_BACK => 7,
        BTN_TASK => 8,
        _ => return None,
    })
}

/// Minimal consumer-page (0x0C) table.
///
/// [SPEC-GAP] MACRO_SPEC.md §E.5: the numeric page-12 list lives in the encrypted `.edevice`
/// descriptors, which are explicitly out of scope. These six controls come from the public HID
/// consumer usage page and cover what a macro realistically emits. Extend as needed —
/// no behaviour is lost relative to the original, since the original's table is unobservable.
pub fn consumer_usage_to_evdev(usage: u16) -> Option<i32> {
    Some(match usage {
        0x00B5 => 163, // KEY_NEXTSONG
        0x00B6 => 165, // KEY_PREVIOUSSONG
        0x00CD => 164, // KEY_PLAYPAUSE
        0x00E2 => 113, // KEY_MUTE
        0x00E9 => 115, // KEY_VOLUMEUP
        0x00EA => 114, // KEY_VOLUMEDOWN
        0x0192 => 156, // KEY_CALC
        0x0221 => 172, // KEY_HOMEPAGE
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_loads() {
        let km = Keymap::load_embedded().unwrap();
        assert!(
            km.len() >= 100,
            "expected the full 112-key table, got {}",
            km.len()
        );
    }

    #[test]
    fn letters_and_modifiers_map() {
        let km = Keymap::load_embedded().unwrap();
        assert_eq!(km.lookup(1, 0x04).unwrap().evdev, 30); // KEY_A
        assert_eq!(km.lookup(1, 0x1d).unwrap().evdev, 44); // KEY_Z
        assert_eq!(km.lookup(1, 0x28).unwrap().evdev, 28); // KEY_ENTER
        assert_eq!(km.lookup(1, 0xe0).unwrap().evdev, 29); // KEY_LEFTCTRL
        assert_eq!(km.lookup(1, 0xe4).unwrap().evdev, 97); // KEY_RIGHTCTRL
    }

    #[test]
    fn reverse_lookup_returns_hid() {
        let km = Keymap::load_embedded().unwrap();
        assert_eq!(km.reverse(30), Some((1, 0x04)));
        assert_eq!(km.reverse(29), Some((1, 0xe0)));
    }

    #[test]
    fn unknown_usage_is_none_not_guessed() {
        let km = Keymap::load_embedded().unwrap();
        assert!(km.lookup(1, 0xFE).is_none());
        assert!(km.lookup(9, 0x04).is_none());
    }

    #[test]
    fn mouse_buttons_roundtrip() {
        for code in 1..=8u16 {
            let ev = mouse_button_code(code).unwrap();
            assert_eq!(mouse_button_hid(ev), Some(code));
        }
        assert!(mouse_button_code(0).is_none());
        assert!(mouse_button_code(9).is_none());
    }
}
