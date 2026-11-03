//! Regression tests for the review findings.
//!
//! Each test names the behaviour a user depends on, not the implementation that provides it.
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use macro_daemon::config::{
    binding_types, Binding, Macro, MacroEvent, PlaybackOptions, ProfileFile, ProfileMeta,
    ProfileSet, TriggerData,
};
use macro_daemon::engine::has_mixed_timing;
use macro_daemon::inject::{Injector, LockedInjector, MockInjector};
use macro_daemon::keymap::Keymap;
use macro_daemon::trigger::{is_virtual_device_name, InputSample, TRIGGER_KEYBOARD};

fn keymap() -> Keymap {
    Keymap::load_embedded().unwrap()
}

fn binding(id: &str, hid: u16) -> Binding {
    Binding {
        binding_type: binding_types::CUSTOM_ACTION.into(),
        id: Some(id.into()),
        config_id: None,
        macro_id: Some("m".into()),
        trigger_data: TriggerData::key(hid),
        playback_options_data: PlaybackOptions::default(),
        os_macro_type: String::new(),
        is_passthrough: false,
        transparent_macro_data: None,
    }
}

fn profile_with(bindings: Vec<Binding>, macros: Vec<Macro>) -> ProfileFile {
    ProfileFile {
        version: 1,
        profile: ProfileMeta {
            name: "p".into(),
            apps: vec![],
        },
        macros,
        bindings,
    }
}

fn macro_row(id: &str, is_deleted: i64) -> Macro {
    Macro {
        id: id.into(),
        name: id.into(),
        events: vec![MacroEvent::key(4, true), MacroEvent::key(4, false)],
        recording_options: None,
        cloud_last_updated_at: String::new(),
        is_deleted,
        is_new: 0,
    }
}

#[test]
fn hotkey_is_matched_in_hid_space_not_evdev_space() {
    // The bug: the loop compared an HID usage (0x44 for F11) against an evdev code (87).
    let km = keymap();
    assert_eq!(km.by_name("KEY_F11"), Some(87));
    assert_eq!(km.hotkey_hid("KEY_F11"), Some((1, 0x44)));

    let f11 = InputSample {
        page: 1,
        hid_code: 0x44,
        trigger_type: TRIGGER_KEYBOARD,
        pressed: true,
        direction: 1,
    };
    assert!(f11.is_hotkey(1, 0x44));
    assert!(!f11.is_hotkey(1, 87), "evdev code must not match HID space");

    let release = InputSample {
        pressed: false,
        ..f11
    };
    assert!(!release.is_hotkey(1, 0x44), "release must not toggle");
}

#[test]
fn numeric_hotkey_spec_resolves_through_the_same_table() {
    // `toggle_hotkey: "70"` is documented as an evdev code; it must land in HID space too.
    let km = keymap();
    let evdev: i32 = 70; // KEY_SCROLLLOCK
    assert_eq!(km.reverse(evdev), Some((1, 0x47)));
}

#[test]
fn deleted_macro_rows_do_not_resolve() {
    let live = profile_with(vec![binding("b1", 4)], vec![macro_row("m", 0)]);
    assert!(live.macro_by_id("m").is_some());

    let tombstone = profile_with(vec![binding("b1", 4)], vec![macro_row("m", 1)]);
    assert!(
        tombstone.macro_by_id("m").is_none(),
        "is_deleted = 1 must not be playable"
    );
}

#[test]
fn synthetic_devices_are_recognised_by_name() {
    for name in [
        "ydotoold virtual device",
        "uinput-foo",
        "wtype virtual keyboard",
        "Virtual Keyboard",
        "macro-daemon virtual input",
    ] {
        assert!(is_virtual_device_name(name), "{name} should be filtered");
    }
    for name in [
        "CASUE USB KB",
        "INSTANT USB GAMING MOUSE  Keyboard",
        "AT Translated Set 2 keyboard",
    ] {
        assert!(!is_virtual_device_name(name), "{name} is hardware");
    }
}

#[test]
fn mixed_timing_is_flagged() {
    let mut timed = vec![MacroEvent::key(4, true), MacroEvent::delay(120)];
    assert!(!has_mixed_timing(&timed), "delay only is sequential mode");

    timed[0].timestamp = 50;
    assert!(has_mixed_timing(&timed), "timestamps plus delays must warn");
}

#[test]
fn passthrough_bindings_are_detected_for_the_grab_check() {
    let mut set = ProfileSet::default();
    set.profiles.insert(
        "p".into(),
        profile_with(vec![binding("b1", 4)], vec![macro_row("m", 0)]),
    );
    assert!(!set.has_passthrough());

    let mut passthrough = binding("b2", 5);
    passthrough.is_passthrough = true;
    set.profiles.insert(
        "q".into(),
        profile_with(vec![passthrough], vec![macro_row("m", 0)]),
    );
    assert!(set.has_passthrough());
}

#[test]
fn locked_injector_does_not_hold_the_device_across_a_macro() {
    // Two callers, each: emit, sleep, emit. Per-emission locking lets them overlap, so the
    // wall time stays near one sleep. Holding the lock for a whole run would serialise them.
    let shared: Arc<Mutex<Box<dyn Injector>>> = Arc::new(Mutex::new(
        Box::new(MockInjector::new()) as Box<dyn Injector>
    ));
    let sleep = Duration::from_millis(300);

    let started = Instant::now();
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let shared = shared.clone();
            thread::spawn(move || {
                let mut inj = LockedInjector::new(&shared);
                inj.key(30, true).unwrap();
                thread::sleep(sleep);
                inj.key(30, false).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(500),
        "injection serialised across macros: {elapsed:?} (expected about {sleep:?})"
    );
}

#[test]
fn gate_key_survives_a_profile_switch() {
    // The bug: the key included the profile name, so leaving the profile orphaned the gate.
    let in_default = macro_daemon::config::gate_key(Some("b1"), 0, 4);
    let in_editor = macro_daemon::config::gate_key(Some("b1"), 3, 9);
    assert_eq!(
        in_default, in_editor,
        "an id must identify a gate on its own"
    );

    let anon_a = macro_daemon::config::gate_key(None, 0, 4);
    let anon_b = macro_daemon::config::gate_key(None, 1, 4);
    assert_ne!(anon_a, anon_b, "unnamed bindings need a per-index key");
}
