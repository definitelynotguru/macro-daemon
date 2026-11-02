//! End-to-end replay against a mock injector: ordering, sync, skips, repeat.
use macro_daemon::clock::FakeWait;
use macro_daemon::config::{EventType, MacroEvent, PlaybackOptions, PlaybackType};
use macro_daemon::engine::Runner;
use macro_daemon::inject::{MockEvent, MockInjector, WheelAxis};
use macro_daemon::keymap::Keymap;

fn keymap() -> Keymap {
    Keymap::load_embedded().unwrap()
}

#[test]
fn two_event_macro_emits_down_then_up_with_sync_each_step() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![MacroEvent::key(4, true), MacroEvent::key(4, false)]; // HID 0x04 = A
    let opts = PlaybackOptions::default();

    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        let stats = runner.execute(&events, &opts, &mut |_| false).unwrap();
        assert_eq!(stats.injected, 2);
        assert_eq!(stats.iterations, 1);
    }

    assert_eq!(
        inj.events,
        vec![
            MockEvent::Key {
                code: 30,
                pressed: true
            },
            MockEvent::Sync,
            MockEvent::Key {
                code: 30,
                pressed: false
            },
            MockEvent::Sync,
        ]
    );
    assert_eq!(inj.syncs, 2, "one EV_SYN per logical step");
}

#[test]
fn modifiers_and_letters_resolve_to_the_right_codes() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    // ctrl + v
    let events = vec![
        MacroEvent::key(224, true),
        MacroEvent::key(25, true),
        MacroEvent::key(25, false),
        MacroEvent::key(224, false),
    ];
    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        runner
            .execute(&events, &PlaybackOptions::default(), &mut |_| false)
            .unwrap();
    }
    let codes: Vec<i32> = inj
        .events
        .iter()
        .filter_map(|e| match e {
            MockEvent::Key { code, .. } => Some(*code),
            _ => None,
        })
        .collect();
    assert_eq!(
        codes,
        vec![29, 47, 47, 29],
        "KEY_LEFTCTRL, KEY_V, KEY_V, KEY_LEFTCTRL"
    );
}

#[test]
fn repeat_runs_are_interleaved_in_order() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![MacroEvent::key(4, true), MacroEvent::key(4, false)];
    let opts = PlaybackOptions {
        playback_type: PlaybackType::RepeatNTimes,
        num_repeat: 2,
        repeat_delay: 25,
        direction: 1,
    };
    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        runner.execute(&events, &opts, &mut |_| false).unwrap();
    }
    assert_eq!(
        inj.events,
        vec![
            MockEvent::Key {
                code: 30,
                pressed: true
            },
            MockEvent::Sync,
            MockEvent::Key {
                code: 30,
                pressed: false
            },
            MockEvent::Sync,
            // repeat #2
            MockEvent::Key {
                code: 30,
                pressed: true
            },
            MockEvent::Sync,
            MockEvent::Key {
                code: 30,
                pressed: false
            },
            MockEvent::Sync,
        ]
    );
}

#[test]
fn wheel_direction_decoding() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![
        MacroEvent::new(EventType::MouseWheel, 0, 0, 1),
        MacroEvent::new(EventType::MouseWheel, 0, 0, 255),
    ];
    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        runner
            .execute(&events, &PlaybackOptions::default(), &mut |_| false)
            .unwrap();
    }
    assert_eq!(
        inj.events,
        vec![
            MockEvent::Wheel {
                axis: WheelAxis::Vertical,
                delta: 1
            },
            MockEvent::Sync,
            MockEvent::Wheel {
                axis: WheelAxis::Vertical,
                delta: -1
            },
            MockEvent::Sync,
        ]
    );
}

#[test]
fn mouse_buttons_map_to_btn_codes() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![
        MacroEvent::new(EventType::MouseButton, 0, 1, 1),
        MacroEvent::new(EventType::MouseButton, 0, 1, 0),
    ];
    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        runner
            .execute(&events, &PlaybackOptions::default(), &mut |_| false)
            .unwrap();
    }
    assert_eq!(
        inj.events[0],
        MockEvent::Button {
            code: 0x110,
            pressed: true
        }
    );
    assert_eq!(
        inj.events[2],
        MockEvent::Button {
            code: 0x110,
            pressed: false
        }
    );
}

#[test]
fn unmapped_usage_is_skipped_not_guessed() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![
        MacroEvent::key(0x04, true),
        MacroEvent::new(EventType::KeyboardKey, 1, 0xFE, 1), // not in the table
        MacroEvent::key(0x04, false),
    ];
    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        let stats = runner
            .execute(&events, &PlaybackOptions::default(), &mut |_| false)
            .unwrap();
        assert_eq!(stats.injected, 2);
        assert_eq!(stats.skipped, 1);
    }
    // The unknown event produced no injection at all — no placeholder keycode.
    assert_eq!(inj.events.len(), 4);
}

#[test]
fn repeat_marker_events_are_ignored() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![
        MacroEvent::key(4, true),
        MacroEvent::new(EventType::RepeatMarker, 0, 0, 3),
        MacroEvent::key(4, false),
    ];
    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        let stats = runner
            .execute(&events, &PlaybackOptions::default(), &mut |_| false)
            .unwrap();
        assert_eq!(stats.injected, 2);
        assert_eq!(stats.skipped, 0);
    }
}

#[test]
fn consumer_page_key_resolves_through_consumer_table() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![MacroEvent::new(EventType::KeyboardKey, 12, 0x00E2, 1)]; // mute
    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        runner
            .execute(&events, &PlaybackOptions::default(), &mut |_| false)
            .unwrap();
    }
    assert_eq!(
        inj.events[0],
        MockEvent::Key {
            code: 113,
            pressed: true
        }
    );
}

#[test]
fn profile_binding_matches_its_trigger() {
    use macro_daemon::config::TriggerData;
    use macro_daemon::trigger::{InputSample, TRIGGER_KEYBOARD};

    let t = TriggerData {
        hid_code: 71,
        page: 1,
        trigger_type: 2,
        direction: 1,
    };
    let hit = InputSample {
        page: 1,
        hid_code: 71,
        trigger_type: TRIGGER_KEYBOARD,
        pressed: true,
        direction: 1,
    };
    let miss_code = InputSample {
        hid_code: 72,
        ..hit
    };
    let miss_page = InputSample { page: 0, ..hit };
    assert!(hit.matches(&t));
    assert!(!miss_code.matches(&t));
    assert!(!miss_page.matches(&t));
}
