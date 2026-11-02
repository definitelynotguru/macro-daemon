//! Pacing math: timestamp offsets, delay events, repeat_delay between runs.
use std::time::Duration;

use macro_daemon::clock::{FakeWait, Jitter};
use macro_daemon::config::{MacroEvent, PlaybackOptions, PlaybackType};
use macro_daemon::engine::{estimated_duration, plan_offsets, Runner};
use macro_daemon::inject::MockInjector;
use macro_daemon::keymap::Keymap;

fn keymap() -> Keymap {
    Keymap::load_embedded().unwrap()
}

#[test]
fn sequential_mode_accumulates_delay_events() {
    // §B.4: with all timestamps 0 (what the editor emits), only type:4 delays carry time.
    let events = vec![
        MacroEvent::key(4, true),
        MacroEvent::delay(250),
        MacroEvent::key(4, false),
    ];
    let plan = plan_offsets(&events);
    assert_eq!(plan, vec![0, 250, 250]);
}

#[test]
fn sequential_mode_orders_events_without_delays_at_zero() {
    let events = vec![
        MacroEvent::key(4, true),
        MacroEvent::key(4, false),
        MacroEvent::delay(100),
        MacroEvent::key(5, true),
    ];
    assert_eq!(plan_offsets(&events), vec![0, 0, 100, 100]);
}

#[test]
fn timestamp_mode_is_absolute_from_macro_start() {
    // [SPEC-GAP] §E.3: absolute-origin reading. A recorded macro's timestamps are honoured
    // as offsets, not deltas.
    let mut events = vec![
        MacroEvent::key(4, true),
        MacroEvent::key(4, false),
        MacroEvent::key(5, true),
    ];
    events[0].timestamp = 10;
    events[1].timestamp = 30;
    events[2].timestamp = 500;
    assert_eq!(plan_offsets(&events), vec![10, 30, 500]);
}

#[test]
fn mixed_zero_and_nonzero_timestamps_switch_to_absolute() {
    let mut events = vec![MacroEvent::delay(40), MacroEvent::key(4, true)];
    events[0].timestamp = 0;
    events[1].timestamp = 40;
    // Any non-zero timestamp flips the whole macro to absolute mode.
    assert_eq!(plan_offsets(&events), vec![0, 40]);
}

#[test]
fn repeat_ntimes_runs_n_times_with_delay_between() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![MacroEvent::key(4, true), MacroEvent::key(4, false)];
    let opts = PlaybackOptions {
        playback_type: PlaybackType::RepeatNTimes,
        num_repeat: 3,
        repeat_delay: 50,
        direction: 1,
    };

    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        let stats = runner.execute(&events, &opts, &mut |_| false).unwrap();
        assert_eq!(stats.iterations, 3);
        assert_eq!(stats.injected, 6, "2 events × 3 runs");
    }

    // 3 runs => 2 gaps of repeat_delay (§B.4: delay sits *between* runs).
    let gaps: Vec<Duration> = wait
        .slept
        .iter()
        .copied()
        .filter(|d| *d == Duration::from_millis(50))
        .collect();
    assert_eq!(gaps.len(), 2, "slept {sleeped:?}", sleeped = wait.slept);
}

#[test]
fn play_once_ignores_repeat_fields() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![MacroEvent::key(4, true)];
    let opts = PlaybackOptions {
        playback_type: PlaybackType::PlayOnce,
        num_repeat: 9,
        repeat_delay: 1000,
        direction: 1,
    };
    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        let stats = runner.execute(&events, &opts, &mut |_| true).unwrap();
        assert_eq!(stats.iterations, 1, "playOnce never repeats");
        assert_eq!(stats.injected, 1);
    }
    assert!(
        wait.slept.iter().all(|d| *d != Duration::from_millis(1000)),
        "repeat_delay must not be applied for playOnce"
    );
}

#[test]
fn held_mode_stops_when_gate_closes() {
    let km = keymap();
    let mut inj = MockInjector::new();
    let mut wait = FakeWait::default();
    let events = vec![MacroEvent::key(4, true)];
    let opts = PlaybackOptions {
        playback_type: PlaybackType::RepeatWhilePressed,
        num_repeat: 1,
        repeat_delay: 10,
        direction: 1,
    };

    let mut calls = 0u32;
    {
        let mut runner = Runner::new(&mut inj, &km, &mut wait);
        let stats = runner
            .execute(&events, &opts, &mut |_| {
                calls += 1;
                calls <= 2 // allow two more iterations, then stop
            })
            .unwrap();
        assert_eq!(stats.iterations, 3, "first run + 2 gated runs");
    }
}

#[test]
fn estimated_duration_matches_plan() {
    let events = vec![
        MacroEvent::key(4, true),
        MacroEvent::delay(100),
        MacroEvent::key(4, false),
    ];
    let once = PlaybackOptions {
        playback_type: PlaybackType::PlayOnce,
        ..Default::default()
    };
    assert_eq!(
        estimated_duration(&events, &once),
        Duration::from_millis(100)
    );

    let thrice = PlaybackOptions {
        playback_type: PlaybackType::RepeatNTimes,
        num_repeat: 3,
        repeat_delay: 50,
        direction: 1,
    };
    // 3 × 100ms of work + 2 × 50ms of gaps
    assert_eq!(
        estimated_duration(&events, &thrice),
        Duration::from_millis(400)
    );
}

#[test]
fn jitter_is_off_by_default_and_bounded_when_on() {
    let base = Duration::from_millis(100);
    let off = Jitter::off();
    let mut state = 12345u64;
    assert_eq!(off.apply(base, &mut state), base);

    let on = Jitter { max_ms: 10 };
    let mut state = 99u64;
    for _ in 0..200 {
        let d = on.apply(base, &mut state);
        assert!(
            d >= Duration::from_millis(90) && d <= Duration::from_millis(110),
            "jitter out of range: {d:?}"
        );
    }
}
