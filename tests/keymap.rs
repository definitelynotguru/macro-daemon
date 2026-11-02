//! Keymap sanity: the recovered HID table must translate cleanly to evdev.
use macro_daemon::config::EventType;
use macro_daemon::keymap::{self, Keymap};

#[test]
fn table_is_the_full_recovered_set() {
    let km = Keymap::load_embedded().unwrap();
    // keymap.json ships 112 keyboard-page entries (MACRO_SPEC.md §C.3).
    assert_eq!(km.len(), 112, "unexpected key count: {}", km.len());
    assert!(!km.is_empty());
}

#[test]
fn no_entry_is_missing_an_evdev_code() {
    // §C.3: every usage present in the table resolves to a Linux keycode.
    let km = Keymap::load_embedded().unwrap();
    for hid in 0u16..=0xFF {
        if let Some(k) = km.lookup(1, hid) {
            assert!(
                k.evdev > 0,
                "HID 0x{hid:02x} ({}) resolved to a non-positive evdev code",
                k.name
            );
            assert!(k.name.starts_with("KEY_"), "unexpected name {}", k.name);
        }
    }
}

#[test]
fn known_anchors_are_stable() {
    let km = Keymap::load_embedded().unwrap();
    let cases = [
        (0x04u16, 30, "KEY_A"),
        (0x05, 48, "KEY_B"),
        (0x1D, 44, "KEY_Z"),
        (0x28, 28, "KEY_ENTER"),
        (0x29, 1, "KEY_ESC"),
        (0x2C, 57, "KEY_SPACE"),
        (0x39, 58, "KEY_CAPSLOCK"),
        (0x3A, 59, "KEY_F1"),
        (0xE0, 29, "KEY_LEFTCTRL"),
        (0xE1, 42, "KEY_LEFTSHIFT"),
        (0xE4, 97, "KEY_RIGHTCTRL"),
    ];
    for (hid, evdev, name) in cases {
        let k = km
            .lookup(1, hid)
            .unwrap_or_else(|| panic!("missing HID 0x{hid:02x}"));
        assert_eq!(k.evdev, evdev, "HID 0x{hid:02x} mapped to {:?}", k.name);
        assert_eq!(k.name, name);
    }
}

#[test]
fn reverse_map_covers_every_forward_entry_once() {
    let km = Keymap::load_embedded().unwrap();
    for hid in 4u16..=0xE7 {
        if let Some(k) = km.lookup(1, hid) {
            let back = km.reverse(k.evdev);
            assert!(back.is_some(), "no reverse entry for {}", k.name);
            let (page, code) = back.unwrap();
            assert_eq!(page, 1);
            // Aliases may collapse (e.g. two HID usages on the same evdev code), but the
            // resolved entry must itself look up to the same evdev code.
            let resolved = km.lookup(page, code).unwrap();
            assert_eq!(resolved.evdev, k.evdev);
        }
    }
}

#[test]
fn unknown_usages_are_none_not_guessed() {
    // §C.3: no invented mappings — queue a log and skip.
    let km = Keymap::load_embedded().unwrap();
    assert!(km.lookup(1, 0x00).is_none());
    assert!(km.lookup(1, 0x03).is_none());
    assert!(km.lookup(1, 0xFE).is_none());
    assert!(
        km.lookup(7, 0x04).is_none(),
        "page 7 is not shipped in the table"
    );
    assert!(km.lookup(2, 0x04).is_none(), "page 2 is not keyboard");
}

#[test]
fn resolve_routes_by_event_type() {
    let km = Keymap::load_embedded().unwrap();
    assert_eq!(km.resolve(EventType::KeyboardKey, 1, 0x04), Some(30));
    assert_eq!(
        km.resolve(EventType::MouseButton, 0, 1),
        Some(keymap::BTN_LEFT)
    );
    assert_eq!(
        km.resolve(EventType::MouseButton, 0, 8),
        Some(keymap::BTN_TASK)
    );
    assert_eq!(km.resolve(EventType::MouseButton, 0, 0), None);
    assert_eq!(km.resolve(EventType::Delay, 0, 0), None);
}

#[test]
fn consumer_page_has_the_common_controls() {
    // §E.5 [SPEC-GAP]: rebuilt from the public HID consumer page, not from the encrypted
    // descriptors. These six are the ones a macro realistically emits.
    assert_eq!(keymap::consumer_usage_to_evdev(0x00CD), Some(164)); // play/pause
    assert_eq!(keymap::consumer_usage_to_evdev(0x00E2), Some(113)); // mute
    assert_eq!(keymap::consumer_usage_to_evdev(0x00B5), Some(163)); // next
    assert_eq!(keymap::consumer_usage_to_evdev(0x0FFF), None);
}

#[test]
fn by_name_resolves_hotkeys() {
    let km = Keymap::load_embedded().unwrap();
    assert_eq!(km.by_name("KEY_F11"), Some(87));
    assert_eq!(km.by_name("KEY_SCROLLLOCK"), Some(70));
    assert_eq!(km.by_name("KEY_NOT_A_KEY"), None);
}
