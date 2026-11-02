//! Configuration schema.
//!
//! Field names and shapes mirror the recovered engine (see `MACRO_SPEC.md` §A):
//! `macros.events[]` is an array of `{type, page, code, extraData, timestamp}`,
//! `bindings.trigger_data` is `{hidCode, page, type, direction}` and
//! `bindings.playback_options_data` is `{type, num_repeat, repeat_delay}`.
//!
//! The database stores `macros.events` as a JSON *string* and the Go decoder wraps it as
//! `{"events": <string>}` before unmarshalling (MACRO_SPEC.md §A.2). [`Macro::db_events_string`]
//! and [`Macro::from_db_events_string`] reproduce that framing byte-for-byte so configs can be
//! round-tripped against real engine data.
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const CONFIG_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Event types  (MACRO_SPEC.md §A.2 table)
// ---------------------------------------------------------------------------

/// `DeviceEvent.Function` / JSON `type`.
///
/// Values are the ones the recovered engine actually emits: the client enum
/// `inputEventTypes` (`{mouseButton:0, mouseWheel:1, keyboardKey:2, mouseTilt:12,
/// mouseExtraButton:13}`) plus `4 = delay` and `5 = repeat marker`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventType {
    MouseButton = 0,
    MouseWheel = 1,
    KeyboardKey = 2,
    Delay = 4,
    /// Only ever produced by the engine itself when appending repeat metadata
    /// (MACRO_SPEC.md §B.4). We never inject it; we skip it on replay.
    RepeatMarker = 5,
    MouseTilt = 12,
    MouseExtraButton = 13,
}

impl EventType {
    pub fn from_code(code: u8) -> Result<Self> {
        Ok(match code {
            0 => EventType::MouseButton,
            1 => EventType::MouseWheel,
            2 => EventType::KeyboardKey,
            4 => EventType::Delay,
            5 => EventType::RepeatMarker,
            12 => EventType::MouseTilt,
            13 => EventType::MouseExtraButton,
            other => {
                bail!("unknown macro event type {other} (MACRO_SPEC.md §A.2 lists 0,1,2,4,5,12,13)")
            }
        })
    }

    pub fn as_code(self) -> u8 {
        self as u8
    }

    /// `true` for event types that carry a standalone delay in `extraData`.
    pub fn is_delay(self) -> bool {
        matches!(self, EventType::Delay)
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_code())
    }
}

impl Serialize for EventType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(self.as_code())
    }
}

impl<'de> Deserialize<'de> for EventType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = u8::deserialize(d)?;
        EventType::from_code(raw).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Playback types — THE dual-numbering trap  (MACRO_SPEC.md §A.4)
// ---------------------------------------------------------------------------

/// Playback mode.
///
/// The discriminants are the **wire/firmware** numbering (`MACRO_SPEC.md` §A.4, second table):
/// `playOnce=1 … holdWhilePressed=6`. That is what an onboard macro header carries.
///
/// The **client/DB** numbering is `playOnce=0 … toggleHold=4` and is what
/// `bindings.playback_options_data.type` holds on disk. It never reaches the execution path:
/// configs are deserialized through [`PlaybackType::from_client`], and anything that has to talk
/// about the firmware form goes through [`PlaybackType::to_wire`].
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackType {
    PlayOnce = 1,
    RepeatNTimes = 2,
    RepeatWhilePressed = 3,
    ToggleRepeat = 4,
    ToggleHold = 5,
    HoldWhilePressed = 6,
}

impl PlaybackType {
    /// Client/DB numbering (0..=4) → enum. Unknown values are a hard error, never coerced.
    pub fn from_client(code: u8) -> Result<Self> {
        Ok(match code {
            0 => PlaybackType::PlayOnce,
            1 => PlaybackType::RepeatNTimes,
            2 => PlaybackType::RepeatWhilePressed,
            3 => PlaybackType::ToggleRepeat,
            4 => PlaybackType::ToggleHold,
            other => bail!(
                "unknown playback type {other} in client numbering (valid: 0..=4, \
                 MACRO_SPEC.md §A.4)"
            ),
        })
    }

    /// Enum → client/DB numbering.
    pub fn to_client(self) -> u8 {
        match self {
            PlaybackType::PlayOnce => 0,
            PlaybackType::RepeatNTimes => 1,
            PlaybackType::RepeatWhilePressed => 2,
            PlaybackType::ToggleRepeat => 3,
            PlaybackType::ToggleHold => 4,
            // Wire-only mode: the client model has no slot for it. ToggleHold is the
            // closest semantics the DB can express; callers that need the distinction
            // must look at `to_wire()`.
            PlaybackType::HoldWhilePressed => 4,
        }
    }

    /// Wire/firmware numbering (1..=6) → enum.
    pub fn from_wire(code: u8) -> Result<Self> {
        Ok(match code {
            1 => PlaybackType::PlayOnce,
            2 => PlaybackType::RepeatNTimes,
            3 => PlaybackType::RepeatWhilePressed,
            4 => PlaybackType::ToggleRepeat,
            5 => PlaybackType::ToggleHold,
            6 => PlaybackType::HoldWhilePressed,
            other => bail!(
                "unknown playback type {other} in wire numbering (valid: 1..=6, \
                 MACRO_SPEC.md §A.4)"
            ),
        })
    }

    /// Enum → wire/firmware numbering.
    pub fn to_wire(self) -> u8 {
        self as u8
    }

    /// `true` when the mode needs the trigger release/re-press signal to stop looping.
    pub fn is_held_mode(self) -> bool {
        matches!(
            self,
            PlaybackType::RepeatWhilePressed
                | PlaybackType::ToggleHold
                | PlaybackType::HoldWhilePressed
        )
    }

    /// `true` when `repeat_delay` is honoured (MACRO_SPEC.md §B.4 table).
    pub fn uses_repeat_delay(self) -> bool {
        matches!(
            self,
            PlaybackType::RepeatNTimes
                | PlaybackType::RepeatWhilePressed
                | PlaybackType::ToggleRepeat
        )
    }

    /// `true` when `num_repeat` is honoured.
    pub fn uses_num_repeat(self) -> bool {
        matches!(self, PlaybackType::RepeatNTimes)
    }
}

impl fmt::Display for PlaybackType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            PlaybackType::PlayOnce => "playOnce",
            PlaybackType::RepeatNTimes => "repeatNTimes",
            PlaybackType::RepeatWhilePressed => "repeatWhilePressed",
            PlaybackType::ToggleRepeat => "toggleRepeat",
            PlaybackType::ToggleHold => "toggleHold",
            PlaybackType::HoldWhilePressed => "holdWhilePressed",
        };
        write!(f, "{name}")
    }
}

// On disk (config + DB) playback type is the *client* numbering.
impl Serialize for PlaybackType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(self.to_client())
    }
}

impl<'de> Deserialize<'de> for PlaybackType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = u8::deserialize(d)?;
        PlaybackType::from_client(raw).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// One `DeviceEvent` record — 12 bytes in the engine (MACRO_SPEC.md §A.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroEvent {
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub page: u8,
    pub code: u16,
    #[serde(rename = "extraData")]
    pub extra_data: u32,
    #[serde(default)]
    pub timestamp: u32,
}

impl MacroEvent {
    pub fn new(event_type: EventType, page: u8, code: u16, extra_data: u32) -> Self {
        Self {
            event_type,
            page,
            code,
            extra_data,
            timestamp: 0,
        }
    }

    pub fn key(code: u16, pressed: bool) -> Self {
        MacroEvent::new(EventType::KeyboardKey, 1, code, u8::from(pressed) as u32)
    }

    pub fn delay(ms: u32) -> Self {
        MacroEvent::new(EventType::Delay, 0, 0, ms)
    }
}

// ---------------------------------------------------------------------------
// Playback options
// ---------------------------------------------------------------------------

fn default_num_repeat() -> u8 {
    1
}
fn default_repeat_delay() -> u16 {
    15 // MACRO_SPEC.md §A.4 — DB default is {"type":0,"num_repeat":1,"repeat_delay":15}
}
fn default_direction() -> i8 {
    1
}

/// `domains.PlaybackOptions` (MACRO_SPEC.md §A.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackOptions {
    #[serde(rename = "type")]
    pub playback_type: PlaybackType,
    #[serde(rename = "num_repeat", default = "default_num_repeat")]
    pub num_repeat: u8,
    #[serde(rename = "repeat_delay", default = "default_repeat_delay")]
    pub repeat_delay: u16,
    /// Present in the client model only (MACRO_SPEC.md §A.4); `1` forward, `-1` reverse.
    #[serde(default = "default_direction", skip_serializing_if = "is_forward")]
    pub direction: i8,
}

fn is_forward(d: &i8) -> bool {
    *d == 1
}

impl Default for PlaybackOptions {
    fn default() -> Self {
        Self {
            playback_type: PlaybackType::PlayOnce,
            num_repeat: default_num_repeat(),
            repeat_delay: default_repeat_delay(),
            direction: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Triggers
// ---------------------------------------------------------------------------

/// `types.Trigger` (MACRO_SPEC.md §D.2) — the physical control that fires a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerData {
    #[serde(rename = "hidCode")]
    pub hid_code: u16,
    pub page: u8,
    #[serde(rename = "type")]
    pub trigger_type: u8,
    #[serde(default = "default_direction")]
    pub direction: i8,
}

impl TriggerData {
    pub fn key(code: u16) -> Self {
        Self {
            hid_code: code,
            page: 1,
            trigger_type: 2, // keyboardKey
            direction: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Bindings
// ---------------------------------------------------------------------------

fn de_bool_or_int<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    // The DB column is INTEGER 0/1 (MACRO_SPEC.md §D.1) while the JSON DTO is a bool.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrInt {
        Bool(bool),
        Int(i64),
    }
    Ok(match BoolOrInt::deserialize(d)? {
        BoolOrInt::Bool(b) => b,
        BoolOrInt::Int(i) => i != 0,
    })
}

fn ser_bool_as_bool<S: Serializer>(v: &bool, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bool(*v)
}

/// `domains.TransparentMacroData` (MACRO_SPEC.md §D.3) — a single key/button binding that
/// needs no macro row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransparentMacroData {
    #[serde(rename = "hidCode")]
    pub hid_code: u16,
    pub page: u8,
    #[serde(rename = "type")]
    pub macro_type: u8,
}

fn de_opt_transparent<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<TransparentMacroData>, D::Error> {
    // The DB column is TEXT NOT NULL DEFAULT '' (MACRO_SPEC.md §D.1), the DTO is an object.
    let raw = Option::<serde_json::Value>::deserialize(d)?;
    match raw {
        None => Ok(None),
        Some(serde_json::Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(v) => serde_json::from_value(v)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// `bindings` row (MACRO_SPEC.md §D.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    #[serde(rename = "type")]
    pub binding_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macro_id: Option<String>,
    pub trigger_data: TriggerData,
    #[serde(default)]
    pub playback_options_data: PlaybackOptions,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os_macro_type: String,
    #[serde(
        default,
        deserialize_with = "de_opt_transparent",
        skip_serializing_if = "Option::is_none"
    )]
    pub transparent_macro_data: Option<TransparentMacroData>,
    #[serde(
        default,
        deserialize_with = "de_bool_or_int",
        serialize_with = "ser_bool_as_bool"
    )]
    pub is_passthrough: bool,
}

/// Binding type strings as shipped by the client (`bindings.type`, MACRO_SPEC.md §D.3).
pub mod binding_types {
    pub const CUSTOM_ACTION: &str = "customAction";
    pub const TRANSPARENT_MACRO: &str = "transparentMacro";
    pub const OS_MACRO_ACTION: &str = "osMacroAction";
    pub const TEXT_MACRO: &str = "text_macro";
    pub const KEYBOARD_BUTTON: &str = "keyboardButtonAction";
    pub const MOUSE_BUTTON: &str = "mouseButtonAction";
}

// ---------------------------------------------------------------------------
// Macros
// ---------------------------------------------------------------------------

fn default_is_new() -> i64 {
    0
}

/// `macros` row (MACRO_SPEC.md §A.1), with `events` kept structured in memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Macro {
    pub id: String,
    pub name: String,
    pub events: Vec<MacroEvent>,
    #[serde(
        rename = "recordingOptions",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub recording_options: Option<RecordingOptions>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cloud_last_updated_at: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub is_deleted: i64,
    /// Not part of the JSON DTO but present in the table; kept so a row can be
    /// reconstructed faithfully.
    #[serde(default = "default_is_new", skip_serializing_if = "is_zero")]
    pub is_new: i64,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

impl Macro {
    /// Rebuild the exact text the Go decoder expects: `{"events": <array>}`.
    ///
    /// `device.unmarshalMacroEvents` (VA 0x1404d2520) builds `{"events": ` (11 bytes,
    /// literal at 0x140a81fe6) + the column text + `}`. MACRO_SPEC.md §A.2.
    pub fn db_events_string(&self) -> String {
        let inner = serde_json::to_string(&self.events).expect("events always serialise");
        format!("{{\"events\": {inner}}}")
    }

    /// Inverse of [`Macro::db_events_string`]. Accepts both the wrapped form and a bare array.
    pub fn from_db_events_string(id: &str, name: &str, raw: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct Wrapped {
            events: Vec<MacroEvent>,
        }

        let trimmed = raw.trim_start();
        let events: Vec<MacroEvent> = if trimmed.starts_with('[') {
            serde_json::from_str(trimmed).context("events column is not a JSON array")?
        } else {
            let w: Wrapped = serde_json::from_str(trimmed)
                .context("events column is not {\"events\": [...]}")?;
            w.events
        };

        Ok(Self {
            id: id.to_string(),
            name: name.to_string(),
            events,
            recording_options: None,
            cloud_last_updated_at: String::new(),
            is_deleted: 0,
            is_new: 0,
        })
    }
}

/// `module_ipc.MacroRecordingOptions` (MACRO_SPEC.md §A.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingOptions {
    pub timeout: i64,
}

// ---------------------------------------------------------------------------
// Profile file
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub name: String,
    /// Per-application rule (MACRO_SPEC.md §D.4): applications are matched by
    /// **executable path**, never window title.
    #[serde(default)]
    pub apps: Vec<String>,
}

/// One `profiles/<name>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub profile: ProfileMeta,
    #[serde(default)]
    pub macros: Vec<Macro>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

impl ProfileFile {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONFIG_VERSION {
            bail!(
                "profile `{}`: unsupported config version {} (this build speaks {})",
                self.profile.name,
                self.version,
                CONFIG_VERSION
            );
        }
        let mut seen = BTreeMap::new();
        for m in &self.macros {
            if m.id.is_empty() {
                bail!("profile `{}`: macro with empty id", self.profile.name);
            }
            if seen.insert(m.id.clone(), ()).is_some() {
                bail!(
                    "profile `{}`: duplicate macro id `{}`",
                    self.profile.name,
                    m.id
                );
            }
            for ev in &m.events {
                // Surfaces unknown discriminants at load time instead of at trigger time.
                EventType::from_code(ev.event_type.as_code())?;
            }
        }
        for b in &self.bindings {
            if let Some(id) = &b.macro_id {
                if !self.macros.iter().any(|m| &m.id == id) {
                    bail!(
                        "profile `{}`: binding references missing macro `{id}`",
                        self.profile.name
                    );
                }
            }
            if b.binding_type == binding_types::CUSTOM_ACTION && b.macro_id.is_none() {
                bail!(
                    "profile `{}`: customAction binding without macro_id",
                    self.profile.name
                );
            }
        }
        Ok(())
    }

    pub fn macro_by_id(&self, id: &str) -> Option<&Macro> {
        self.macros.iter().find(|m| m.id == id)
    }
}

// ---------------------------------------------------------------------------
// Daemon config
// ---------------------------------------------------------------------------

fn default_profiles_dir() -> PathBuf {
    PathBuf::from("profiles")
}
fn default_poll_ms() -> u64 {
    500
}
fn default_debounce_ms() -> u64 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// evdev nodes to read triggers from.
    #[serde(default)]
    pub devices: Vec<PathBuf>,
    #[serde(default = "default_profiles_dir")]
    pub profiles_dir: PathBuf,
    #[serde(default)]
    pub default_profile: String,
    /// Uniform ±jitter around every scheduled delay. 0 = off (the recovered engine has no
    /// jitter field at all — MACRO_SPEC.md §A.4 — so this is opt-in and defaults to 0).
    #[serde(default)]
    pub jitter_ms: u32,
    /// evdev key name to toggle the daemon (e.g. "KEY_SCROLLLOCK").
    #[serde(default)]
    pub toggle_hotkey: Option<String>,
    /// Focus poll interval for per-application rules.
    #[serde(default = "default_poll_ms")]
    pub focus_poll_ms: u64,
    /// Ignore repeat triggers closer than this (milliseconds) per binding.
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
    /// Grab the trigger devices exclusively (EVIOCGRAB). Stops the OS from also seeing them.
    #[serde(default)]
    pub grab: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            profiles_dir: default_profiles_dir(),
            default_profile: String::new(),
            jitter_ms: 0,
            toggle_hotkey: None,
            focus_poll_ms: default_poll_ms(),
            debounce_ms: default_debounce_ms(),
            grab: false,
        }
    }
}

impl DaemonConfig {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading daemon config {}", path.display()))?;
        let cfg: DaemonConfig = serde_json::from_str(&text)
            .with_context(|| format!("parsing daemon config {}", path.display()))?;
        Ok(cfg)
    }
}

// ---------------------------------------------------------------------------
// Profile set
// ---------------------------------------------------------------------------

/// All profiles on disk plus which one is currently active.
#[derive(Debug, Clone, Default)]
pub struct ProfileSet {
    pub profiles: BTreeMap<String, ProfileFile>,
    pub active: String,
}

impl ProfileSet {
    pub fn load_dir(dir: &std::path::Path) -> Result<Self> {
        let mut profiles = BTreeMap::new();
        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("reading profiles dir {}", dir.display()))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading profile {}", path.display()))?;
            let mut prof: ProfileFile = serde_json::from_str(&text)
                .with_context(|| format!("parsing profile {}", path.display()))?;
            if prof.profile.name.is_empty() {
                prof.profile.name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unnamed")
                    .to_string();
            }
            prof.validate()?;
            profiles.insert(prof.profile.name.clone(), prof);
        }
        if profiles.is_empty() {
            bail!("no profiles found in {}", dir.display());
        }
        Ok(Self {
            active: String::new(),
            profiles,
        })
    }

    /// Pick the profile for an active application path: the first profile whose `apps`
    /// list matches wins; otherwise the configured default (MACRO_SPEC.md §D.4).
    pub fn select_for(&self, app: Option<&std::path::Path>, default: &str) -> String {
        if let Some(app) = app {
            let app_str = app.to_string_lossy();
            for (name, prof) in &self.profiles {
                for rule in &prof.profile.apps {
                    if path_matches(rule, &app_str) {
                        return name.clone();
                    }
                }
            }
        }
        if self.profiles.contains_key(default) {
            default.to_string()
        } else {
            self.profiles
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| default.to_string())
        }
    }

    pub fn active_profile(&self) -> Option<&ProfileFile> {
        self.profiles.get(&self.active)
    }
}

/// Exact match or "basename ends with rule" — application scanner stores full paths
/// (`applications.path`, MACRO_SPEC.md §D.3), so suffix matching keeps the rule portable
/// across distributions while still being path-based, never title-based.
pub fn path_matches(rule: &str, app: &str) -> bool {
    if rule.is_empty() {
        return false;
    }
    if app == rule {
        return true;
    }
    let app_base = app.rsplit('/').next().unwrap_or(app);
    let rule_base = rule.rsplit('/').next().unwrap_or(rule);
    !rule_base.is_empty() && app_base == rule_base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_dual_numbering_roundtrip() {
        // client 0..4 -> wire 1..5 (MACRO_SPEC.md §A.4)
        let table = [
            (0u8, PlaybackType::PlayOnce, 1u8),
            (1, PlaybackType::RepeatNTimes, 2),
            (2, PlaybackType::RepeatWhilePressed, 3),
            (3, PlaybackType::ToggleRepeat, 4),
            (4, PlaybackType::ToggleHold, 5),
        ];
        for (client, expected, wire) in table {
            let p = PlaybackType::from_client(client).unwrap();
            assert_eq!(p, expected);
            assert_eq!(p.to_client(), client);
            assert_eq!(p.to_wire(), wire);
            assert_eq!(PlaybackType::from_wire(wire).unwrap(), expected);
        }
        assert_eq!(
            PlaybackType::from_wire(6).unwrap(),
            PlaybackType::HoldWhilePressed
        );
    }

    #[test]
    fn unknown_discriminants_are_errors() {
        assert!(PlaybackType::from_client(5).is_err());
        assert!(PlaybackType::from_wire(0).is_err());
        assert!(PlaybackType::from_wire(7).is_err());
        assert!(EventType::from_code(3).is_err());
        assert!(EventType::from_code(255).is_err());
    }
}
