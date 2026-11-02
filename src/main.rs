//! CLI + daemon lifecycle.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Parser;

use macro_daemon::apprule;
use macro_daemon::clock::Jitter;
use macro_daemon::config::{
    binding_types, Binding, DaemonConfig, MacroEvent, PlaybackOptions, PlaybackType, ProfileSet,
    TriggerData,
};
use macro_daemon::engine::{plan_offsets, Runner};
use macro_daemon::inject::{Injector, MockInjector, UinputInjector};
use macro_daemon::keymap::Keymap;
use macro_daemon::trigger::{self, InputSample};

const MAX_CONCURRENT_MACROS: usize = 4;
/// Name of the virtual device we create; excluded from trigger discovery.
const VIRTUAL_DEVICE_NAME: &str = "macro-daemon virtual input";

#[derive(Parser, Debug)]
#[command(
    name = "macro-daemon",
    about = "Vendor-neutral macro daemon (evdev triggers, uinput injection)",
    version
)]
struct Cli {
    /// Path to daemon config (JSON). Defaults to ./daemon.json when present.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Override the profiles directory.
    #[arg(long)]
    profiles: Option<PathBuf>,

    /// evdev device nodes to read triggers from (repeatable). Overrides the config.
    #[arg(short = 'd', long = "device")]
    devices: Vec<PathBuf>,

    /// Validate configuration and exit.
    #[arg(long)]
    validate: bool,

    /// List readable input devices and exit.
    #[arg(long)]
    list_devices: bool,

    /// Execute nothing for real: log what would be injected.
    #[arg(long)]
    dry_run: bool,

    /// Print a timed trace of one macro (`PROFILE:MACRO_ID`) and exit.
    #[arg(long, value_name = "PROFILE:MACRO")]
    trace: Option<String>,

    /// Log filter (tracing EnvFilter syntax).
    #[arg(long, default_value = "info")]
    log: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log)),
        )
        .with_target(false)
        .init();

    let cfg = load_config(&cli)?;

    if cli.list_devices {
        list_devices();
        return Ok(());
    }

    let profiles = ProfileSet::load_dir(&cfg.profiles_dir)
        .with_context(|| format!("loading profiles from {}", cfg.profiles_dir.display()))?;

    if let Some(spec) = cli.trace {
        return trace_macro(&cfg, &profiles, &spec);
    }

    if cli.validate {
        validate_report(&cfg, &profiles);
        return Ok(());
    }

    daemon(cfg, profiles, cli.dry_run)
}

fn load_config(cli: &Cli) -> Result<DaemonConfig> {
    let mut cfg = match &cli.config {
        Some(path) => DaemonConfig::load(path)?,
        None => {
            let default = PathBuf::from("daemon.json");
            if default.exists() {
                DaemonConfig::load(&default)?
            } else {
                DaemonConfig::default()
            }
        }
    };
    if let Some(dir) = &cli.profiles {
        cfg.profiles_dir = dir.clone();
    }
    if !cli.devices.is_empty() {
        cfg.devices = cli.devices.clone();
    }
    Ok(cfg)
}

fn list_devices() {
    let found = trigger::discover_devices(Some(VIRTUAL_DEVICE_NAME));
    if found.is_empty() {
        eprintln!(
            "no readable input devices found.\n\
             Add your user to the `input` group and log back in:\n  \
             sudo usermod -aG input $USER"
        );
        return;
    }
    for (path, name) in trigger::open_device_names(&found) {
        println!("{}\t{}", path.display(), name);
    }
}

fn validate_report(cfg: &DaemonConfig, profiles: &ProfileSet) {
    println!("profiles dir: {}", cfg.profiles_dir.display());
    for (name, prof) in &profiles.profiles {
        let apps = if prof.profile.apps.is_empty() {
            String::from("-")
        } else {
            prof.profile.apps.join(", ")
        };
        println!(
            "  {:<16} macros={:<3} bindings={:<3} apps={}",
            name,
            prof.macros.len(),
            prof.bindings.len(),
            apps
        );
        for b in &prof.bindings {
            let opts = &b.playback_options_data;
            println!(
                "      {:<22} trigger hid=0x{:02x} page={} type={} dir={:<2}  playback={} (client {} / wire {}) repeats={} delay={}ms",
                b.binding_type,
                b.trigger_data.hid_code,
                b.trigger_data.page,
                b.trigger_data.trigger_type,
                b.trigger_data.direction,
                opts.playback_type,
                opts.playback_type.to_client(),
                opts.playback_type.to_wire(),
                opts.num_repeat,
                opts.repeat_delay
            );
        }
    }
    println!(
        "default profile: {}",
        profiles.select_for(None, &cfg.default_profile)
    );
}

// ---------------------------------------------------------------------------
// Trace mode
// ---------------------------------------------------------------------------

fn trace_macro(_cfg: &DaemonConfig, profiles: &ProfileSet, spec: &str) -> Result<()> {
    let (profile_name, macro_id) = spec
        .split_once(':')
        .context("--trace expects PROFILE:MACRO_ID")?;
    let prof = profiles
        .profiles
        .get(profile_name)
        .with_context(|| format!("no profile named `{profile_name}`"))?;
    let mac = prof
        .macro_by_id(macro_id)
        .with_context(|| format!("no macro `{macro_id}` in profile `{profile_name}`"))?;

    let binding = prof
        .bindings
        .iter()
        .find(|b| b.macro_id.as_deref() == Some(macro_id))
        .cloned()
        .unwrap_or_else(|| Binding {
            binding_type: binding_types::CUSTOM_ACTION.into(),
            id: None,
            config_id: None,
            macro_id: Some(macro_id.to_string()),
            trigger_data: TriggerData::key(0),
            playback_options_data: PlaybackOptions::default(),
            os_macro_type: String::new(),
            is_passthrough: false,
            transparent_macro_data: None,
        });

    let opts = binding.playback_options_data;
    let keymap = Keymap::load_embedded()?;
    let plan = plan_offsets(&mac.events);

    println!(
        "macro `{}` ({}) from profile `{}`",
        mac.name, mac.id, profile_name
    );
    println!(
        "playback: {}  client#{} wire#{}  num_repeat={} repeat_delay={}ms",
        opts.playback_type,
        opts.playback_type.to_client(),
        opts.playback_type.to_wire(),
        opts.num_repeat,
        opts.repeat_delay
    );
    println!();

    let runs = match opts.playback_type {
        PlaybackType::PlayOnce => 1u32,
        PlaybackType::RepeatNTimes => u32::from(opts.num_repeat.max(1)),
        _ => 1,
    };

    let mut t_total = 0u64;
    for run in 0..runs {
        if run > 0 && opts.playback_type.uses_repeat_delay() {
            println!("  +{:>5}ms  [repeat_delay]", u64::from(opts.repeat_delay));
            t_total += u64::from(opts.repeat_delay);
        }
        for (i, ev) in mac.events.iter().enumerate() {
            let at = plan.get(i).copied().unwrap_or(0);
            println!(
                "  +{:>5}ms  run{}  type={} page={} code=0x{:02x} extra={} -> {}",
                at,
                run + 1,
                ev.event_type,
                ev.page,
                ev.code,
                ev.extra_data,
                describe_event(&keymap, ev)
            );
            t_total = t_total.max(at);
        }
    }
    println!();
    println!(
        "runs={runs}  single_run_ms={}  total_ms≈{}",
        plan.last().copied().unwrap_or(0),
        t_total
    );
    Ok(())
}

fn describe_event(km: &Keymap, ev: &MacroEvent) -> String {
    use macro_daemon::config::EventType;
    match ev.event_type {
        EventType::KeyboardKey => {
            if ev.page == macro_daemon::keymap::PAGE_CONSUMER {
                match macro_daemon::keymap::consumer_usage_to_evdev(ev.code) {
                    Some(c) => format!("consumer evdev {c} {}", updown(ev.extra_data)),
                    None => "consumer usage unmapped (skipped)".into(),
                }
            } else {
                match km.lookup(ev.page, ev.code) {
                    Some(k) => format!("{} ({}) {}", k.name, k.evdev, updown(ev.extra_data)),
                    None => format!("unmapped HID 0x{:02x} (skipped)", ev.code),
                }
            }
        }
        EventType::MouseButton | EventType::MouseExtraButton => {
            match macro_daemon::keymap::mouse_button_code(ev.code) {
                Some(c) => format!("BTN 0x{c:x} {}", updown(ev.extra_data)),
                None => "bad button code (skipped)".into(),
            }
        }
        EventType::MouseWheel => format!("REL_WHEEL {}", signed_tiny(ev.extra_data)),
        EventType::MouseTilt => format!("REL_HWHEEL {}", signed_tiny(ev.extra_data)),
        EventType::Delay => format!("sleep {}ms", ev.extra_data),
        EventType::RepeatMarker => "engine metadata (ignored)".into(),
    }
}

fn updown(v: u32) -> &'static str {
    if v != 0 {
        "down"
    } else {
        "up"
    }
}

fn signed_tiny(v: u32) -> i32 {
    match v {
        255 => -1,
        other => other as i32,
    }
}

// ---------------------------------------------------------------------------
// Daemon
// ---------------------------------------------------------------------------

type SharedInjector = Arc<Mutex<Box<dyn Injector>>>;

fn daemon(cfg: DaemonConfig, mut profiles: ProfileSet, dry_run: bool) -> Result<()> {
    let keymap = Arc::new(Keymap::load_embedded()?);
    tracing::info!(keys = keymap.len(), "keymap loaded");

    let injector: SharedInjector = if dry_run {
        tracing::warn!("dry-run: no events will be injected");
        Arc::new(Mutex::new(
            Box::new(MockInjector::new()) as Box<dyn Injector>
        ))
    } else {
        let dev = UinputInjector::create(VIRTUAL_DEVICE_NAME)?;
        tracing::info!(device = dev.describe(), "uinput device created");
        Arc::new(Mutex::new(Box::new(dev) as Box<dyn Injector>))
    };

    let mut devices = cfg.devices.clone();
    if devices.is_empty() {
        devices = trigger::discover_devices(Some(VIRTUAL_DEVICE_NAME));
        if devices.is_empty() {
            bail!(
                "no trigger devices. List candidates with --list-devices, then pass \
                 -d /dev/input/eventN (needs membership in the `input` group)"
            );
        }
        tracing::info!(count = devices.len(), "auto-discovered trigger devices");
    }
    for d in &devices {
        if !d.exists() {
            bail!("trigger device {} does not exist", d.display());
        }
        if let Ok(dev) = evdev::Device::open(d) {
            if dev.name() == Some(VIRTUAL_DEVICE_NAME) {
                bail!(
                    "refusing to read {}: it is this daemon's own virtual device",
                    d.display()
                );
            }
        }
    }

    let default_profile = if cfg.default_profile.is_empty() {
        profiles.profiles.keys().next().cloned().unwrap_or_default()
    } else {
        cfg.default_profile.clone()
    };
    profiles.active = profiles.select_for(None, &default_profile);
    tracing::info!(profile = %profiles.active, "active profile");

    let hotkey = match &cfg.toggle_hotkey {
        Some(name) => Some(resolve_hotkey(name, &keymap)?),
        None => None,
    };

    // Shared runtime state.
    let held: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>> = Arc::new(Mutex::new(HashMap::new()));
    let toggles: Arc<Mutex<HashMap<String, bool>>> = Arc::new(Mutex::new(HashMap::new()));
    let last_fire: Arc<Mutex<HashMap<String, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    let inflight = Arc::new(AtomicUsize::new(0));
    let enabled = Arc::new(AtomicBool::new(true));
    let stop = Arc::new(AtomicBool::new(false));
    let reload = Arc::new(AtomicBool::new(false));
    let mut current_app: Option<PathBuf> = None;

    spawn_signal_thread(stop.clone(), reload.clone())?;

    // Focus watcher: per-application profile switching (MACRO_SPEC.md §D.4).
    let (focus_tx, focus_rx) = std::sync::mpsc::channel();
    apprule::spawn_watcher(cfg.focus_poll_ms, focus_tx, stop.clone());

    // Trigger readers.
    let (tx, rx) = std::sync::mpsc::channel::<InputSample>();
    for dev in &devices {
        trigger::spawn_reader(dev.clone(), cfg.grab, (*keymap).clone(), tx.clone())?;
    }
    drop(tx);

    tracing::info!("running (SIGHUP reloads profiles, Ctrl-C stops)");

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        if reload.swap(false, Ordering::SeqCst) {
            match ProfileSet::load_dir(&cfg.profiles_dir) {
                Ok(mut fresh) => {
                    let name = profiles.active.clone();
                    fresh.active = if fresh.profiles.contains_key(&name) {
                        name
                    } else {
                        fresh.select_for(current_app.as_deref(), &default_profile)
                    };
                    profiles = fresh;
                    tracing::info!(profile = %profiles.active, "profiles reloaded (SIGHUP)");
                }
                Err(e) => tracing::error!(error = %e, "reload failed; keeping previous profiles"),
            }
        }

        while let Ok(app) = focus_rx.try_recv() {
            current_app = app;
            let next = profiles.select_for(current_app.as_deref(), &default_profile);
            if next != profiles.active {
                tracing::info!(
                    from = %profiles.active,
                    to = %next,
                    app = %current_app
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    "profile switch"
                );
                profiles.active = next;
            }
        }

        let sample = match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(s) => s,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if let Some(hk) = hotkey {
            if sample.trigger_type == trigger::TRIGGER_KEYBOARD
                && i32::from(sample.hid_code) == hk
                && sample.pressed
            {
                let was = enabled.fetch_xor(true, Ordering::SeqCst);
                tracing::warn!(enabled = !was, "daemon toggled by hotkey");
                continue;
            }
        }

        if !enabled.load(Ordering::Relaxed) {
            continue;
        }

        let Some(profile) = profiles.active_profile().cloned() else {
            continue;
        };

        for (idx, binding) in profile.bindings.iter().enumerate() {
            if !sample.matches(&binding.trigger_data) {
                continue;
            }
            let key = format!(
                "{}:{}:{}",
                profile.profile.name, idx, binding.trigger_data.hid_code
            );

            if !sample.pressed {
                if let Some(flag) = held.lock().unwrap().get(&key) {
                    flag.store(false, Ordering::SeqCst);
                }
                continue;
            }

            // Debounce: skip re-triggers inside the configured window.
            {
                let mut lf = last_fire.lock().unwrap();
                let now = Instant::now();
                if let Some(prev) = lf.get(&key) {
                    if now.duration_since(*prev) < Duration::from_millis(cfg.debounce_ms) {
                        continue;
                    }
                }
                lf.insert(key.clone(), now);
            }

            match binding.binding_type.as_str() {
                binding_types::CUSTOM_ACTION => {
                    let Some(mac) = binding
                        .macro_id
                        .as_deref()
                        .and_then(|id| profile.macro_by_id(id))
                        .cloned()
                    else {
                        tracing::error!(binding = idx, "customAction without a resolvable macro");
                        continue;
                    };
                    fire(
                        &cfg,
                        &keymap,
                        &injector,
                        &inflight,
                        &held,
                        &toggles,
                        key,
                        mac.events,
                        binding.playback_options_data,
                        mac.name,
                    );
                }
                binding_types::TRANSPARENT_MACRO => {
                    let Some(data) = binding.transparent_macro_data else {
                        tracing::error!(binding = idx, "transparentMacro without data");
                        continue;
                    };
                    let event_type = match data.macro_type {
                        0 => macro_daemon::config::EventType::MouseButton,
                        2 => macro_daemon::config::EventType::KeyboardKey,
                        other => {
                            tracing::warn!(kind = other, "unsupported transparent macro kind");
                            continue;
                        }
                    };
                    let pressed = MacroEvent {
                        event_type,
                        page: data.page,
                        code: data.hid_code,
                        extra_data: 1,
                        timestamp: 0,
                    };
                    let release = MacroEvent {
                        extra_data: 0,
                        ..pressed
                    };
                    fire(
                        &cfg,
                        &keymap,
                        &injector,
                        &inflight,
                        &held,
                        &toggles,
                        key,
                        vec![pressed, release],
                        PlaybackOptions {
                            playback_type: PlaybackType::PlayOnce,
                            ..Default::default()
                        },
                        format!("transparent#{idx}"),
                    );
                }
                binding_types::OS_MACRO_ACTION | binding_types::TEXT_MACRO => {
                    // §D.3: the original converts these into HID events. We have no
                    // text→HID mapping (that needs a Windows keyboard layout), so refuse
                    // loudly rather than silently doing nothing.
                    tracing::warn!(
                        binding = idx,
                        kind = %binding.binding_type,
                        "[SPEC-GAP] MACRO_SPEC.md §B.1: text/OS macro expansion not implemented"
                    );
                }
                other => {
                    tracing::debug!(binding = idx, kind = other, "unhandled binding type");
                }
            }
        }
    }

    tracing::info!("shutting down");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn fire(
    cfg: &DaemonConfig,
    keymap: &Arc<Keymap>,
    injector: &SharedInjector,
    inflight: &Arc<AtomicUsize>,
    held: &Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    toggles: &Arc<Mutex<HashMap<String, bool>>>,
    key: String,
    events: Vec<MacroEvent>,
    opts: PlaybackOptions,
    label: String,
) {
    if inflight.fetch_add(1, Ordering::SeqCst) >= MAX_CONCURRENT_MACROS {
        inflight.fetch_sub(1, Ordering::SeqCst);
        tracing::warn!(macro = %label, "too many concurrent macros; trigger ignored");
        return;
    }

    // Decide the loop gate before spawning, so the flag is set the instant the trigger fires.
    let gate_flag: Option<Arc<AtomicBool>> = match opts.playback_type {
        PlaybackType::PlayOnce | PlaybackType::RepeatNTimes => None,
        PlaybackType::RepeatWhilePressed
        | PlaybackType::ToggleHold
        | PlaybackType::HoldWhilePressed => {
            let flag = Arc::new(AtomicBool::new(true));
            held.lock().unwrap().insert(key.clone(), flag.clone());
            Some(flag)
        }
        PlaybackType::ToggleRepeat => {
            let mut map = toggles.lock().unwrap();
            let entry = map.entry(key.clone()).or_insert(false);
            *entry = !*entry;
            let on = *entry;
            tracing::info!(macro = %label, on, "toggleRepeat");
            if !on {
                // Second press: clear the gate the running loop is watching.
                if let Some(flag) = held.lock().unwrap().get(&key) {
                    flag.store(false, Ordering::SeqCst);
                }
                inflight.fetch_sub(1, Ordering::SeqCst);
                return;
            }
            let flag = Arc::new(AtomicBool::new(true));
            held.lock().unwrap().insert(key.clone(), flag.clone());
            Some(flag)
        }
    };

    let injector = injector.clone();
    let keymap = keymap.clone();
    let inflight = inflight.clone();
    let held_worker = held.clone();
    let jitter = Jitter {
        max_ms: cfg.jitter_ms,
    };
    let run_key = key.clone();
    let opts_copy = opts;
    // The closure needs its own handles: `label`/`inflight` are still used below for the
    // spawn-failure path.
    let label_worker = label.clone();
    let inflight_worker = inflight.clone();

    let spawn = thread::Builder::new()
        .name(format!("macro:{}", label))
        .spawn(move || {
            let mut guard = injector.lock().unwrap_or_else(|e| e.into_inner());
            let mut wait = macro_daemon::clock::RealWait;
            let mut runner = Runner::new(&mut **guard, &keymap, &mut wait).with_jitter(jitter);

            let mut gate = move |_iteration: u32| -> bool {
                match &gate_flag {
                    Some(f) => f.load(Ordering::SeqCst),
                    None => false,
                }
            };

            match runner.execute(&events, &opts_copy, &mut gate) {
                Ok(stats) => tracing::info!(
                    macro = %label_worker,
                    iterations = stats.iterations,
                    injected = stats.injected,
                    skipped = stats.skipped,
                    planned_ms = stats.planned_ms,
                    "macro finished"
                ),
                Err(e) => tracing::error!(macro = %label_worker, error = %e, "macro failed"),
            }

            held_worker.lock().unwrap().remove(&run_key);
            inflight_worker.fetch_sub(1, Ordering::SeqCst);
        });

    if let Err(e) = spawn {
        inflight.fetch_sub(1, Ordering::SeqCst);
        tracing::error!(macro = %label, error = %e, "could not spawn macro thread");
    }
}

fn resolve_hotkey(spec: &str, keymap: &Keymap) -> Result<i32> {
    if let Ok(code) = spec.parse::<i32>() {
        return Ok(code);
    }
    keymap
        .by_name(spec)
        .with_context(|| format!("unknown hotkey `{spec}` (use a KEY_* name from keymap.json)"))
}

fn spawn_signal_thread(stop: Arc<AtomicBool>, reload: Arc<AtomicBool>) -> Result<()> {
    // `signal_hook::flag` installs a plain handler that flips an atomic — no iterator thread,
    // no self-pipe, nothing that can wedge or take the process down with it.
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::flag;

    flag::register(SIGHUP, reload.clone()).context("registering SIGHUP")?;
    flag::register(SIGINT, stop.clone()).context("registering SIGINT")?;
    flag::register(SIGTERM, stop.clone()).context("registering SIGTERM")?;
    tracing::debug!("signal handlers installed (SIGHUP=reload, SIGINT/SIGTERM=stop)");
    Ok(())
}
