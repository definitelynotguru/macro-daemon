//! Engine-level hold timing for macro_daemon: integration tests use the harness crate.

use macro_daemon::clock::FakeWait;
use macro_daemon::config::{MacroEvent, PlaybackOptions, PlaybackType};
use macro_daemon::engine::{plan_offsets, Runner};
use macro_daemon::inject::{Injector, MockEvent, MockInjector};
use macro_daemon::keymap::Keymap;
use macro_e2e::measure::hold_ms_of;
use macro_e2e::measure::TimestampingInjector;

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

#[test]
fn hold_ms_of_pairs_press_and_release_stamps_across_sync() {
    let mut mock = MockInjector::new();
    let mut clocked = TimestampingInjector::new(&mut mock);
    clocked.key(30, true).unwrap();
    clocked.sync().unwrap();
    clocked.key(30, false).unwrap();
    let hold = hold_ms_of(&clocked).expect("press and release both present");
    assert!(hold >= 0.0, "release must not precede press");
    // The Sync entry between the two key calls must not shift the pairing.
    assert_eq!(clocked.mock_events().len(), 3);
    assert!(matches!(clocked.mock_events()[1], MockEvent::Sync));
}

#[test]
fn hold_ms_of_returns_none_without_a_release() {
    let mut mock = MockInjector::new();
    let mut clocked = TimestampingInjector::new(&mut mock);
    clocked.key(30, true).unwrap();
    assert!(hold_ms_of(&clocked).is_none());
}

#[test]
fn hold_ms_of_keeps_first_press_on_duplicate() {
    let mut mock = MockInjector::new();
    let mut clocked = TimestampingInjector::new(&mut mock);
    clocked.key(30, true).unwrap();
    clocked.sync().unwrap();
    clocked.key(30, true).unwrap();
    clocked.key(30, false).unwrap();
    let hold = hold_ms_of(&clocked).expect("release present");
    // stamps: 0 first press, 1 sync, 2 duplicate press, 3 release.
    // Hold must start at the FIRST press, so it strictly exceeds the
    // duplicate-press-to-release gap that a last-press pairing would report.
    let from_dup = clocked
        .stamp(3)
        .unwrap()
        .duration_since(clocked.stamp(2).unwrap());
    assert!(hold > from_dup.as_secs_f64() * 1000.0);
}

#[test]
#[should_panic(expected = "empty sample set")]
fn stats_rejects_empty_samples() {
    macro_e2e::stats::Stats::new(&[]);
}

#[test]
fn cli_rejects_zero_iters_with_clear_error() {
    let bin = env!("CARGO_BIN_EXE_macro-e2e");
    let out = std::process::Command::new(bin)
        .args(["--hold-ms", "400", "--iters", "0"])
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "zero iters must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--iters must be at least 1"),
        "clear error expected, got: {stderr}"
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("median"),
        "no partial results on stdout"
    );
}

#[test]
fn cli_json_stdout_is_one_object_even_with_device_flag() {
    let bin = env!("CARGO_BIN_EXE_macro-e2e");
    let out = std::process::Command::new(bin)
        .args(["--hold-ms", "400", "--iters", "2", "--json", "--device"])
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        1,
        "stdout must be exactly one line: {stdout:?}"
    );
    let parsed: serde_json::Value = serde_json::from_str(lines[0]).expect("valid json object");
    assert_eq!(parsed["iters"], 2);
    assert_eq!(parsed["level"], "engine");
}
