//! Engine-level hold timing for macro_daemon: integration tests use the harness crate.

use macro_daemon::clock::FakeWait;
use macro_daemon::config::{MacroEvent, PlaybackOptions, PlaybackType};
use macro_daemon::engine::{plan_offsets, Runner};
use macro_daemon::inject::{MockEvent, MockInjector};
use macro_daemon::keymap::Keymap;

fn hold_events(hold_ms: u32) -> Vec<MacroEvent> {
    vec![
        MacroEvent::key(4, true),
        MacroEvent::delay(hold_ms),
        MacroEvent::key(4, false),
    ]
}

fn play_once() -> PlaybackOptions {
    PlaybackOptions {
        playback_type: PlaybackType::PlayOnce,
        ..PlaybackOptions::default()
    }
}

#[test]
fn plan_offsets_of_hold_macro_are_press_delay_release() {
    assert_eq!(plan_offsets(&hold_events(400)), vec![0, 400, 400]);
}

#[test]
fn one_iteration_through_runner_yields_press_then_release() {
    let keymap = Keymap::load_embedded().unwrap();
    let mut mock = MockInjector::new();
    let mut wait = FakeWait::default();
    let mut runner = Runner::new(&mut mock, &keymap, &mut wait);
    let stats = runner
        .execute(&hold_events(400), &play_once(), &mut |_| true)
        .unwrap();
    assert_eq!(stats.iterations, 1);
    let keys: Vec<(i32, bool)> = mock
        .events
        .iter()
        .filter_map(|ev| match ev {
            MockEvent::Key { code, pressed } => Some((*code, *pressed)),
            _ => None,
        })
        .collect();
    assert_eq!(keys, vec![(30, true), (30, false)]);
    // Deadline pacing: the engine asks for the remaining time to each offset.
    // Offsets [0, 400, 400] mean the release event needs no further sleep, so the
    // requests are [400ms (release offset), 400ms (macro end guard)].
    assert_eq!(
        wait.slept,
        vec![
            std::time::Duration::from_millis(400),
            std::time::Duration::from_millis(400),
        ]
    );
}

#[test]
fn stats_median_and_p95_match_hand_computed_vector() {
    // n=4, even: median is the mean of the middle two; p95 rank = ceil(3.8) = 4 (1-based).
    let s = macro_e2e::stats::Stats::new(&[1.0, 2.0, 3.0, 4.0]);
    assert_eq!(s.min, 1.0);
    assert_eq!(s.median, 2.5);
    assert_eq!(s.p95, 4.0);
    assert_eq!(s.max, 4.0);

    // n=5, odd: median is the middle element; p95 rank = ceil(4.75) = 5.
    let s = macro_e2e::stats::Stats::new(&[5.0, 3.0, 1.0, 4.0, 2.0]);
    assert_eq!(s.median, 3.0);
    assert_eq!(s.p95, 5.0);

    // n=20: p95 rank = ceil(19.0) = 19, the second-largest value.
    let twenty: Vec<f64> = (1..=20).map(f64::from).collect();
    let s = macro_e2e::stats::Stats::new(&twenty);
    assert_eq!(s.median, 10.5);
    assert_eq!(s.p95, 19.0);

    // n=40: p95 rank = ceil(38) = 38.
    let forty: Vec<f64> = (0..40).map(f64::from).collect();
    let s = macro_e2e::stats::Stats::new(&forty);
    assert_eq!(s.p95, 37.0);
}
