use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use macro_daemon::config::{Binding, EventType, MacroEvent, PlaybackOptions, TriggerData, binding_types};
use macro_daemon::keymap::{Keymap, PAGE_PSEUDO};

pub const KEY_EVENT_WIDTH_PX: f32 = 56.0;

pub struct PlaybackEntry {
    pub code: u8,
    pub label: &'static str,
}

pub const PLAYBACK_TABLE: [PlaybackEntry; 5] = [
    PlaybackEntry {
        code: 0,
        label: "play once",
    },
    PlaybackEntry {
        code: 1,
        label: "repeat N times",
    },
    PlaybackEntry {
        code: 2,
        label: "repeat while pressed",
    },
    PlaybackEntry {
        code: 3,
        label: "toggle repeat",
    },
    PlaybackEntry {
        code: 4,
        label: "toggle hold",
    },
];

pub fn key_name(page: u8, code: u16, keymap: &Keymap) -> String {
    keymap
        .lookup(page, code)
        .map(|k| key_display_name(&k.name))
        .unwrap_or_else(|| format!("usage({page},{code})"))
}

fn key_display_name(evdev_name: &str) -> String {
    let Some(rest) = evdev_name.strip_prefix("KEY_") else {
        return evdev_name.to_string();
    };
    rest.split('_')
        .map(title_case_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn title_case_word(word: &str) -> String {
    if let Some(split) = split_acronym(word) {
        return split;
    }
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
        None => word.to_string(),
    }
}

fn split_acronym(word: &str) -> Option<String> {
    let table: [(&str, &str); 4] = [
        ("LEFTCTRL", "Left Ctrl"),
        ("RIGHTCTRL", "Right Ctrl"),
        ("CAPSLOCK", "Caps Lock"),
        ("SCROLLLOCK", "Scroll Lock"),
    ];
    table
        .iter()
        .find(|(raw, _)| raw.eq_ignore_ascii_case(word))
        .map(|(_, label)| label.to_string())
}

pub fn event_label(event: &MacroEvent, keymap: &Keymap) -> String {
    match event.event_type {
        EventType::KeyboardKey => {
            let name = key_name(event.page, event.code, keymap);
            if event.extra_data > 0 {
                format!("v {name}")
            } else {
                format!("^ {name}")
            }
        }
        EventType::Delay => format!("{}ms", event.extra_data),
        EventType::MouseButton => format!("Btn {}", event.code),
        EventType::MouseExtraButton => format!("Btn {}", event.code),
        EventType::MouseWheel => format!("Wheel {}", event.extra_data),
        EventType::MouseTilt => format!("Tilt {}", event.extra_data),
        EventType::RepeatMarker => "repeat".to_string(),
    }
}

pub fn macro_total_ms(events: &[MacroEvent]) -> u64 {
    events
        .iter()
        .filter(|e| e.event_type == EventType::Delay)
        .map(|e| e.extra_data as u64)
        .sum()
}

pub fn human_trigger(trigger: &TriggerData, keymap: &Keymap) -> String {
    let direction = match trigger.direction {
        -1 => "release",
        1 => "press",
        _ => "dir?",
    };
    if trigger.trigger_type == 2 && trigger.page != PAGE_PSEUDO {
        format!(
            "{} ({direction})",
            key_name(trigger.page, trigger.hid_code, keymap)
        )
    } else if trigger.page == PAGE_PSEUDO && (1..=8).contains(&trigger.hid_code) {
        format!("Mouse {direction}")
    } else {
        format!("usage({},{}) ({direction})", trigger.page, trigger.hid_code)
    }
}

pub fn direction_label(direction: i8) -> &'static str {
    if direction < 0 {
        "reverse"
    } else {
        "forward"
    }
}

pub fn default_binding(macro_id: &str) -> Binding {
    Binding {
        binding_type: binding_types::CUSTOM_ACTION.to_string(),
        id: Some(format!("b-{macro_id}")),
        config_id: None,
        macro_id: Some(macro_id.to_string()),
        trigger_data: TriggerData::key(0),
        playback_options_data: PlaybackOptions::default(),
        os_macro_type: String::new(),
        transparent_macro_data: None,
        is_passthrough: false,
    }
}

pub fn new_macro_id() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("m-{stamp:x}")
}

fn cached_keymap() -> &'static Keymap {
    static KEYMAP: OnceLock<Keymap> = OnceLock::new();
    KEYMAP.get_or_init(|| Keymap::load_embedded().unwrap_or_default())
}

pub fn keymap_cached() -> Keymap {
    cached_keymap().clone()
}
