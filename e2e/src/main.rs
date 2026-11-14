//! End-to-end hold-timing proof harness.

use macro_e2e::stats;

use anyhow::{Context, Result};
use clap::Parser;

use macro_daemon::clock::{FakeWait, RealWait};
use macro_daemon::config::{Macro, MacroEvent, PlaybackOptions, PlaybackType};
use macro_daemon::engine::{plan_offsets, Runner};
use macro_daemon::inject::{MockInjector, UinputInjector};
use macro_daemon::keymap::Keymap;

use macro_e2e::measure::{hold_ms_of, TimestampingInjector};

/// Author the hold macro exactly as a user would: press A, hold, release.
///
/// HID usage 4 (usage page 1) is KEY_A and maps to evdev code 30.
pub fn hold_macro(hold_ms: u32) -> Macro {
    Macro {
        id: "e2e-hold".to_string(),
        name: "e2e-hold".to_string(),
        events: vec![
            MacroEvent::key(4, true),
            MacroEvent::delay(hold_ms),
            MacroEvent::key(4, false),
        ],
        recording_options: None,
        cloud_last_updated_at: String::new(),
        is_deleted: 0,
        is_new: 0,
    }
}

fn play_once() -> PlaybackOptions {
    PlaybackOptions {
        playback_type: PlaybackType::PlayOnce,
        ..PlaybackOptions::default()
    }
}

/// Run one iteration through the real engine against a fresh recording injector.
fn run_iteration(hold_ms: u32) -> Result<f64> {
    let keymap = Keymap::load_embedded()?;
    let mut inner = MockInjector::new();
    let mut clocked = TimestampingInjector::new(&mut inner);
    let mut wait = RealWait;
    let mut runner = Runner::new(&mut clocked, &keymap, &mut wait);
    runner.execute(&hold_macro(hold_ms).events, &play_once(), &mut |_| true)?;
    hold_ms_of(&clocked).with_context(|| {
        "iteration did not produce exactly one press and one release of KEY_A (evdev 30)"
    })
}

fn describe_device(json: bool) -> bool {
    let path = std::path::Path::new("/dev/uinput");
    if !path.exists() {
        // Human modes get the plain one-liner; json mode keeps stdout to the one object.
        if json {
            eprintln!("device preflight: /dev/uinput does not exist on this machine");
        } else {
            println!("device preflight: /dev/uinput does not exist on this machine");
            println!("engine level still runs below; device level needs the real node");
        }
        return false;
    }
    let writable = std::fs::OpenOptions::new().write(true).open(path).is_ok();
    if json {
        eprintln!("device preflight: /dev/uinput present, writable: {writable}");
    } else if writable {
        println!("device preflight: /dev/uinput present and writable");
    } else {
        println!("device preflight: /dev/uinput present but not writable");
    }
    true
}

/// Device-level probe: inject through the real uinput node.
fn device_level(hold_ms: u32, iters: usize, json: bool) -> Result<()> {
    let keymap = Keymap::load_embedded()?;
    let mut injector = UinputInjector::create("macro-e2e-probe")
        .context("creating uinput device (device-level probe)")?;
    let note = |msg: &str| {
        if json {
            eprintln!("{msg}");
        } else {
            println!("{msg}");
        }
    };
    note(&format!(
        "device ready as virtual keyboard \"macro-e2e-probe\"; attach your reader \
         now, playback of {iters} hold-{hold_ms}ms macros starts in 5 s"
    ));
    std::thread::sleep(std::time::Duration::from_secs(5));
    let mut wait = RealWait;
    let mut runner = Runner::new(&mut injector, &keymap, &mut wait);
    for _ in 0..iters {
        runner.execute(&hold_macro(hold_ms).events, &play_once(), &mut |_| true)?;
    }
    note(&format!(
        "device level: injected {iters} hold-{hold_ms}ms macros through /dev/uinput \
         as virtual keyboard \"macro-e2e-probe\"; automated readback is not wired \
         into this harness (see docs/e2e-timing.md for manual verification)"
    ));
    Ok(())
}

#[derive(Parser, Debug)]
#[command(
    name = "macro-e2e",
    about = "Hold-timing proof harness for macro_daemon"
)]
struct Cli {
    /// Hold duration the authored macro carries, in milliseconds.
    #[arg(long, default_value_t = 400)]
    hold_ms: u32,

    /// Iterations to run and fold into the statistics.
    #[arg(long)]
    iters: Option<usize>,

    /// Machine-readable output: one JSON object on stdout, nothing else.
    #[arg(long)]
    json: bool,

    /// Also run the device-level probe through /dev/uinput.
    #[arg(long)]
    device: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let target = f64::from(cli.hold_ms);
    let tolerance = 10.0;

    let iters = cli.iters.unwrap_or(30);
    anyhow::ensure!(iters > 0, "--iters must be at least 1");

    let device_available = describe_device(cli.json);
    if cli.device {
        if device_available {
            device_level(cli.hold_ms, iters, cli.json)?;
        } else if cli.json {
            eprintln!("--device given but /dev/uinput does not exist; device-level run skipped");
        } else {
            println!("--device given but /dev/uinput does not exist; device-level run skipped");
        }
    }

    let mut holds = Vec::with_capacity(iters);
    for _ in 0..iters {
        holds.push(run_iteration(cli.hold_ms)?);
    }

    let s = stats::Stats::new(&holds);
    let pass = (s.median - target).abs() <= tolerance;
    if cli.json {
        println!(
            "{}",
            serde_json::json!({
                "binary": "macro-e2e",
                "hold_ms_target": cli.hold_ms,
                "iters": holds.len(),
                "injector": "engine-level: MockInjector behind TimestampingInjector",
                "level": "engine",
                "stats": {
                    "min_ms": round3(s.min),
                    "median_ms": round3(s.median),
                    "p95_ms": round3(s.p95),
                    "max_ms": round3(s.max),
                },
                "pass": pass,
                "per_iteration_hold_ms": holds.iter().map(|h| round3(*h)).collect::<Vec<_>>(),
            })
        );
    } else {
        println!();
        println!(
            "engine-level hold timing, target {} ms (tolerance {} ms on the median)",
            cli.hold_ms, tolerance as u32
        );
        println!("iterations: {}", holds.len());
        println!();
        println!("{:>10}  {:>10}", "stat", "hold_ms");
        println!("{:>10}  {:>10.3}", "min", s.min);
        println!("{:>10}  {:>10.3}", "median", s.median);
        println!("{:>10}  {:>10.3}", "p95", s.p95);
        println!("{:>10}  {:>10.3}", "max", s.max);
        println!();
        println!(
            "pass: {} (|median - {}| = {:.3} ms)",
            if pass { "yes" } else { "no" },
            cli.hold_ms,
            (s.median - target).abs()
        );
        println!(
            "authored macro plan offsets: {:?} (press at 0, release at {} ms)",
            plan_offsets(&hold_macro(cli.hold_ms).events),
            cli.hold_ms
        );
        print_engine_sleep(cli.hold_ms);
    }
    Ok(())
}

fn print_engine_sleep(hold_ms: u32) {
    let mut wait = FakeWait::default();
    let mut mock = MockInjector::new();
    {
        let keymap = Keymap::load_embedded().expect("embedded keymap always parses");
        let mut runner = Runner::new(&mut mock, &keymap, &mut wait);
        runner
            .execute(&hold_macro(hold_ms).events, &play_once(), &mut |_| true)
            .expect("engine execution on the authored macro");
    }
    println!(
        "engine asked the clock for a total of {} ms per iteration (no jitter, deadline pacing)",
        wait.total().as_millis()
    );
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}
