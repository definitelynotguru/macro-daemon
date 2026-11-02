//! Vendor-neutral macro daemon.
//!
//! Reimplements the *semantics* recovered from a commercial peripheral suite's macro
//! subsystem (`MACRO_SPEC.md`) on top of Linux evdev/uinput:
//!
//! * schema and field names — [`config`]
//! * HID usage → evdev translation — [`keymap`]
//! * pacing and repeat rules — [`engine`]
//! * virtual device output — [`inject`]
//! * physical triggers — [`trigger`]
//! * per-application profiles — [`apprule`]
//!
//! The original engine is onboard-only (`PlayMacro` → `deployMacros`), so nothing here is a
//! port of their runtime; it is a reimplementation of their data model and timing.
pub mod apprule;
pub mod clock;
pub mod config;
pub mod engine;
pub mod inject;
pub mod keymap;
pub mod trigger;

pub use config::{
    Binding, DaemonConfig, EventType, Macro, MacroEvent, PlaybackOptions, PlaybackType,
    ProfileFile, ProfileSet, RecordingOptions, TransparentMacroData, TriggerData,
};
pub use engine::{plan_offsets, RunStats, Runner};
pub use inject::{Injector, MockInjector, UinputInjector, WheelAxis};
pub use keymap::Keymap;

/// `MACRO_SPEC.md` sections this crate implements, for the README compatibility table.
pub const SPEC_COVERAGE: &[(&str, &str)] = &[
    ("A.1 macros table", "config::Macro"),
    (
        "A.2 event object",
        "config::MacroEvent, engine::Runner::run_once",
    ),
    ("A.3 example object", "config::Macro::db_events_string"),
    (
        "A.4 playback options + dual numbering",
        "config::PlaybackType",
    ),
    ("A.5 recording options", "config::RecordingOptions"),
    (
        "B.3 onboard blob",
        "not implemented (explicitly out of scope)",
    ),
    (
        "B.4 pacing and repeat semantics",
        "engine::plan_offsets, engine::Runner::execute",
    ),
    ("B.5 software injection", "inject::UinputInjector"),
    ("C key code mapping", "keymap::Keymap"),
    (
        "D profile/binding model",
        "config::ProfileFile, trigger::InputSample::matches",
    ),
    ("D.4 per-application rules", "apprule::FocusWatcher"),
];
