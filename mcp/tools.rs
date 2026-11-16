use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use macro_daemon::clock::RealWait;
use macro_daemon::config::{
    binding_types, Binding, Macro, MacroEvent, PlaybackOptions, PlaybackType, ProfileFile,
    ProfileMeta, ProfileSet, TriggerData, CONFIG_VERSION,
};
use macro_daemon::engine::Runner;
use macro_daemon::inject::{MockEvent, MockInjector};
use macro_daemon::keymap::Keymap;
use serde::Serialize;
use serde_json::{json, Value};

const MAX_HOLD_MS: u32 = 60_000;
const MAX_ITERS: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedKey {
    pub query: String,
    pub evdev_name: String,
    pub page: u8,
    pub hid_usage: u16,
    pub evdev: i32,
}

pub fn resolve_key(name: &str) -> Result<ResolvedKey> {
    let keymap = Keymap::load_embedded().context("loading embedded keymap")?;
    resolve_with(&keymap, name)
}

fn resolve_with(keymap: &Keymap, name: &str) -> Result<ResolvedKey> {
    let query = name.trim();
    if query.is_empty() {
        bail!("unknown key `{name}`");
    }
    let mut hits: Vec<ResolvedKey> = Vec::new();
    for candidate in candidate_evdev_names(query) {
        let Some(evdev) = keymap.by_name(&candidate) else {
            continue;
        };
        let Some((page, code)) = keymap.reverse(evdev) else {
            continue;
        };
        let Some(key) = keymap.lookup(page, code) else {
            continue;
        };
        if hits
            .iter()
            .any(|hit| hit.evdev == key.evdev && hit.hid_usage == key.code)
        {
            continue;
        }
        hits.push(ResolvedKey {
            query: query.to_string(),
            evdev_name: key.name.clone(),
            page: key.page,
            hid_usage: key.code,
            evdev: key.evdev,
        });
    }
    match hits.len() {
        0 => bail!("unknown key `{query}`"),
        1 => Ok(hits.remove(0)),
        _ => {
            let names: Vec<&str> = hits.iter().map(|hit| hit.evdev_name.as_str()).collect();
            bail!("ambiguous key `{query}`: {}", names.join(", "));
        }
    }
}

fn candidate_evdev_names(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let upper = query.to_ascii_uppercase();
    push_unique(&mut out, upper.clone());
    if !upper.starts_with("KEY_") {
        push_unique(&mut out, format!("KEY_{upper}"));
    }

    let mut body = query.to_ascii_lowercase();
    if let Some(rest) = body.strip_prefix("key_") {
        body = rest.to_string();
    } else if let Some(rest) = body.strip_prefix("key ") {
        body = rest.to_string();
    }
    body = body.replace("numpad", "kp").replace("keypad", "kp");
    let squashed: String = body
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    if !squashed.is_empty() {
        let named = if squashed.starts_with("KEY_") {
            squashed
        } else {
            format!("KEY_{squashed}")
        };
        push_unique(&mut out, named);
    }
    out
}

fn push_unique(out: &mut Vec<String>, value: String) {
    if !value.is_empty() && !out.iter().any(|existing| existing == &value) {
        out.push(value);
    }
}

pub struct BindHold {
    pub profiles_dir: PathBuf,
    pub profile: String,
    pub trigger: String,
    pub hold_key: String,
    pub hold_ms: u32,
}

pub fn bind_hold(args: BindHold) -> Result<Value> {
    if args.hold_ms > MAX_HOLD_MS {
        bail!("hold_ms must be at most {MAX_HOLD_MS}");
    }
    let trigger = resolve_key(&args.trigger)?;
    let hold = resolve_key(&args.hold_key)?;
    let path = profile_path(&args.profiles_dir, &args.profile)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut file = if path.exists() {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str::<ProfileFile>(&text)
            .with_context(|| format!("parsing {}", path.display()))?
    } else {
        ProfileFile {
            version: CONFIG_VERSION,
            profile: ProfileMeta {
                name: args.profile.clone(),
                apps: Vec::new(),
            },
            macros: Vec::new(),
            bindings: Vec::new(),
        }
    };
    if file.profile.name.is_empty() {
        file.profile.name = args.profile.clone();
    }

    let binding_id = format!("trig-{}", trigger.evdev_name);
    let macro_id = format!("hold-{}", trigger.evdev_name);
    let events = vec![
        MacroEvent::key(hold.hid_usage, true),
        MacroEvent::delay(args.hold_ms),
        MacroEvent::key(hold.hid_usage, false),
    ];
    let macro_row = Macro {
        id: macro_id.clone(),
        name: format!("hold {} for {} ms", hold.evdev_name, args.hold_ms),
        events,
        recording_options: None,
        cloud_last_updated_at: String::new(),
        is_deleted: 0,
        is_new: 0,
    };
    match file.macros.iter_mut().find(|row| row.id == macro_id) {
        Some(existing) => *existing = macro_row,
        None => file.macros.push(macro_row),
    }

    file.bindings.retain(|binding| {
        !same_trigger(binding, &trigger) || binding.id.as_deref() == Some(&binding_id)
    });
    let binding = Binding {
        binding_type: binding_types::CUSTOM_ACTION.to_string(),
        id: Some(binding_id.clone()),
        config_id: None,
        macro_id: Some(macro_id.clone()),
        trigger_data: TriggerData {
            hid_code: trigger.hid_usage,
            page: trigger.page,
            trigger_type: 2,
            direction: 1,
        },
        playback_options_data: PlaybackOptions {
            playback_type: PlaybackType::PlayOnce,
            ..PlaybackOptions::default()
        },
        os_macro_type: String::new(),
        transparent_macro_data: None,
        is_passthrough: false,
    };
    match file
        .bindings
        .iter_mut()
        .find(|row| row.id.as_deref() == Some(&binding_id))
    {
        Some(existing) => *existing = binding,
        None => file.bindings.push(binding),
    }
    file.validate()
        .with_context(|| format!("profile `{}` failed validation", file.profile.name))?;
    write_profile(&path, &file)?;

    Ok(json!({
        "path": path.display().to_string(),
        "profile": file.profile.name,
        "macro_id": macro_id,
        "binding_id": binding_id,
        "hold_ms": args.hold_ms,
        "trigger": trigger,
        "hold_key": hold,
    }))
}

fn same_trigger(binding: &Binding, trigger: &ResolvedKey) -> bool {
    binding.trigger_data.hid_code == trigger.hid_usage
        && binding.trigger_data.page == trigger.page
        && binding.trigger_data.direction == 1
}

fn profile_path(dir: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        bail!("profile name must be a single path segment, got `{name}`");
    }
    Ok(dir.join(format!("{name}.json")))
}

fn write_profile(path: &Path, file: &ProfileFile) -> Result<()> {
    let body = serde_json::to_string_pretty(file).context("serializing profile")?;
    let tmp_name = format!(
        ".{}.part",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("profile.json")
    );
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, format!("{body}\n"))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

pub fn validate_profiles(dir: &Path) -> Result<Value> {
    let set = ProfileSet::load_dir(dir).with_context(|| {
        format!(
            "validate failed for {} (same load ProfileSet::load_dir uses for --validate)",
            dir.display()
        )
    })?;
    let profiles: Vec<Value> = set
        .profiles
        .iter()
        .map(|(name, file)| {
            json!({
                "name": name,
                "macros": file.macros.len(),
                "bindings": file.bindings.len(),
            })
        })
        .collect();
    Ok(json!({
        "ok": true,
        "profiles_dir": dir.display().to_string(),
        "profiles": profiles,
    }))
}

pub fn hold_proof(hold_ms: u32, iters: usize) -> Result<Value> {
    if hold_ms > MAX_HOLD_MS {
        bail!("hold_ms must be at most {MAX_HOLD_MS}");
    }
    if iters == 0 || iters > MAX_ITERS {
        bail!("iters must be from 1 to {MAX_ITERS}");
    }
    let key = resolve_key("E")?;
    let events = vec![
        MacroEvent::key(key.hid_usage, true),
        MacroEvent::delay(hold_ms),
        MacroEvent::key(key.hid_usage, false),
    ];
    let samples = measure_hold(&events, key.evdev, iters)?;
    proof_json(
        hold_ms,
        &samples,
        "engine-level hold of KEY_E through Runner, not a device",
    )
}

pub fn measure_hold(events: &[MacroEvent], evdev: i32, iters: usize) -> Result<Vec<f64>> {
    let keymap = Keymap::load_embedded().context("loading embedded keymap")?;
    let opts = PlaybackOptions {
        playback_type: PlaybackType::PlayOnce,
        ..PlaybackOptions::default()
    };
    let mut samples = Vec::with_capacity(iters);
    for index in 0..iters {
        let mut mock = MockInjector::new();
        let mut clocked = ClockedInjector {
            inner: &mut mock,
            stamps: Vec::new(),
        };
        let mut wait = RealWait;
        let mut runner = Runner::new(&mut clocked, &keymap, &mut wait);
        runner
            .execute(events, &opts, &mut |_| true)
            .with_context(|| format!("engine iteration {index}"))?;
        let hold = hold_ms_of(&clocked, evdev).with_context(|| {
            format!("iteration {index} did not press and release evdev {evdev}")
        })?;
        samples.push(hold);
    }
    Ok(samples)
}

struct ClockedInjector<'a> {
    inner: &'a mut MockInjector,
    stamps: Vec<Instant>,
}

impl macro_daemon::inject::Injector for ClockedInjector<'_> {
    fn key(&mut self, evdev_code: i32, pressed: bool) -> Result<()> {
        self.stamps.push(Instant::now());
        self.inner.key(evdev_code, pressed)
    }

    fn button(&mut self, evdev_code: i32, pressed: bool) -> Result<()> {
        self.stamps.push(Instant::now());
        self.inner.button(evdev_code, pressed)
    }

    fn wheel(&mut self, axis: macro_daemon::inject::WheelAxis, delta: i32) -> Result<()> {
        self.stamps.push(Instant::now());
        self.inner.wheel(axis, delta)
    }

    fn sync(&mut self) -> Result<()> {
        self.stamps.push(Instant::now());
        self.inner.sync()
    }

    fn describe(&self) -> &str {
        "engine-level clock"
    }
}

fn hold_ms_of(clocked: &ClockedInjector<'_>, evdev: i32) -> Option<f64> {
    let mut press = None;
    let mut release = None;
    for (index, event) in clocked.inner.events.iter().enumerate() {
        match event {
            MockEvent::Key {
                code,
                pressed: true,
            } if *code == evdev => {
                press = press.or(Some(index));
            }
            MockEvent::Key {
                code,
                pressed: false,
            } if *code == evdev => {
                release = Some(index);
            }
            _ => {}
        }
    }
    let (press, release) = press.zip(release)?;
    let (press_at, release_at) = (clocked.stamps.get(press)?, clocked.stamps.get(release)?);
    if release_at < press_at {
        return None;
    }
    Some(release_at.duration_since(*press_at).as_secs_f64() * 1000.0)
}

pub fn proof_json(target_ms: u32, samples: &[f64], note: &str) -> Result<Value> {
    if samples.is_empty() {
        bail!("no hold samples");
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    let target = f64::from(target_ms);
    Ok(json!({
        "level": "engine",
        "note": note,
        "limiter": "std::thread::sleep inside engine::Runner until the delay offset",
        "hold_ms_target": target_ms,
        "iters": n,
        "min_ms": round3(sorted[0]),
        "median_ms": round3(median),
        "max_ms": round3(sorted[n - 1]),
        "within_10ms": (median - target).abs() <= 10.0,
        "samples_ms": samples.iter().copied().map(round3).collect::<Vec<_>>(),
    }))
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_sentence_needs_resolve_through_the_keymap() {
        let e = resolve_key("E").unwrap();
        let numpad = resolve_key("Numpad 4").unwrap();
        let long = resolve_key("KEY_KP4").unwrap();
        let short = resolve_key("KP4").unwrap();
        assert_eq!(e.evdev_name, "KEY_E");
        assert_eq!(e.page, 1);
        assert_eq!(numpad.evdev_name, "KEY_KP4");
        assert_eq!(long.evdev_name, short.evdev_name);
        assert_eq!(long.hid_usage, short.hid_usage);
        assert_eq!(long.evdev, short.evdev);
        assert_eq!(long.hid_usage, numpad.hid_usage);
        assert_eq!(long.evdev, numpad.evdev);
    }

    #[test]
    fn unknown_names_are_errors() {
        let err = resolve_key("NotAKey").unwrap_err().to_string();
        assert!(err.contains("unknown key"), "{err}");
        assert!(resolve_key("").is_err());
        assert!(resolve_key("   ").is_err());
    }

    #[test]
    fn bind_hold_is_idempotent_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        let args = BindHold {
            profiles_dir: dir.path().to_path_buf(),
            profile: "sentence".to_string(),
            trigger: "Numpad 4".to_string(),
            hold_key: "E".to_string(),
            hold_ms: 400,
        };
        bind_hold(args).unwrap();
        let again = BindHold {
            profiles_dir: dir.path().to_path_buf(),
            profile: "sentence".to_string(),
            trigger: "Numpad 4".to_string(),
            hold_key: "E".to_string(),
            hold_ms: 400,
        };
        let written = bind_hold(again).unwrap();
        let path = PathBuf::from(written["path"].as_str().unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        let file: ProfileFile = serde_json::from_str(&text).unwrap();
        file.validate().unwrap();
        assert_eq!(file.bindings.len(), 1);
        assert_eq!(file.macros.len(), 1);
        assert_eq!(file.bindings[0].trigger_data.direction, 1);
        assert_eq!(
            file.bindings[0].playback_options_data.playback_type,
            PlaybackType::PlayOnce
        );
        assert_eq!(file.macros[0].events.len(), 3);
        assert!(file.macros[0].events[1].event_type.is_delay());
        assert_eq!(file.macros[0].events[1].extra_data, 400);
        let report = validate_profiles(dir.path()).unwrap();
        assert_eq!(report["ok"], true);
    }

    #[test]
    fn profile_name_cannot_escape_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let err = bind_hold(BindHold {
            profiles_dir: dir.path().to_path_buf(),
            profile: "../escape".to_string(),
            trigger: "E".to_string(),
            hold_key: "E".to_string(),
            hold_ms: 1,
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("path segment"), "{err}");
    }
}
