//! Physical input → trigger samples.
//!
//! Triggers are described the same way the recovered engine describes them
//! (`trigger_data: {hidCode, page, type, direction}`, MACRO_SPEC.md §D.2), so this module only
//! has to translate an evdev event into that coordinate space once.
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;

use anyhow::{Context, Result};
use evdev::{Device, EventType as EvType};

use crate::config::TriggerData;
use crate::keymap::{self, Keymap};

/// Trigger type values as they appear in `trigger_data.type` / `macro event.type`
/// (MACRO_SPEC.md §A.2).
pub const TRIGGER_MOUSE_BUTTON: u8 = 0;
pub const TRIGGER_MOUSE_WHEEL: u8 = 1;
pub const TRIGGER_KEYBOARD: u8 = 2;
pub const TRIGGER_MOUSE_TILT: u8 = 12;
pub const TRIGGER_EXTRA_BUTTON: u8 = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputSample {
    pub page: u8,
    pub hid_code: u16,
    pub trigger_type: u8,
    pub pressed: bool,
    pub direction: i8,
}

impl InputSample {
    /// Does this physical sample fire that binding?
    ///
    /// `direction` is only meaningful for wheel triggers (§D.2: `1` / `-1`); a binding that
    /// leaves it at the default `1` matches forward wheel only, which is how the client stores
    /// it by default.
    pub fn matches(&self, t: &TriggerData) -> bool {
        if t.page != self.page || t.hid_code != self.hid_code {
            return false;
        }
        let kind_ok = match t.trigger_type {
            TRIGGER_KEYBOARD => self.trigger_type == TRIGGER_KEYBOARD,
            TRIGGER_MOUSE_BUTTON => {
                self.trigger_type == TRIGGER_MOUSE_BUTTON
                    || self.trigger_type == TRIGGER_EXTRA_BUTTON
            }
            TRIGGER_MOUSE_WHEEL => self.trigger_type == TRIGGER_MOUSE_WHEEL,
            TRIGGER_MOUSE_TILT => self.trigger_type == TRIGGER_MOUSE_TILT,
            other => self.trigger_type == other,
        };
        if !kind_ok {
            return false;
        }
        if t.trigger_type == TRIGGER_MOUSE_WHEEL || t.trigger_type == TRIGGER_MOUSE_TILT {
            return t.direction == self.direction;
        }
        true
    }

    /// Does this press match the daemon's toggle hotkey, expressed in HID space?
    pub fn is_hotkey(&self, page: u8, hid_code: u16) -> bool {
        self.pressed
            && self.trigger_type == TRIGGER_KEYBOARD
            && self.page == page
            && self.hid_code == hid_code
    }
}

/// Translate one evdev event. Returns `None` for anything we do not model (SYN, MSC, LED...).
pub fn normalize(ev: &evdev::InputEvent, km: &Keymap) -> Option<InputSample> {
    match ev.event_type() {
        EvType::KEY => {
            let code = ev.code();
            let value = ev.value();
            // value 2 = kernel autorepeat; the physical key is still down, so don't re-fire.
            if value == 2 {
                return None;
            }
            let pressed = value == 1;

            if let Some(btn) = keymap::mouse_button_hid(i32::from(code)) {
                return Some(InputSample {
                    page: 0,
                    hid_code: btn,
                    trigger_type: if btn <= 5 {
                        TRIGGER_MOUSE_BUTTON
                    } else {
                        TRIGGER_EXTRA_BUTTON
                    },
                    pressed,
                    direction: 1,
                });
            }

            let (page, hid) = km.reverse(i32::from(code))?;
            Some(InputSample {
                page,
                hid_code: hid,
                trigger_type: TRIGGER_KEYBOARD,
                pressed,
                direction: 1,
            })
        }
        EvType::RELATIVE => {
            let value = ev.value();
            if value == 0 {
                return None;
            }
            let dir: i8 = if value > 0 { 1 } else { -1 };
            let trigger_type = match i32::from(ev.code()) {
                keymap::REL_WHEEL => TRIGGER_MOUSE_WHEEL,
                keymap::REL_HWHEEL => TRIGGER_MOUSE_TILT,
                _ => return None,
            };
            Some(InputSample {
                page: 0,
                hid_code: 0,
                trigger_type,
                pressed: true,
                direction: dir,
            })
        }
        _ => None,
    }
}

/// Open a device, optionally grab it, and stream samples to `tx` until the device disappears.
pub fn spawn_reader(path: PathBuf, grab: bool, km: Keymap, tx: Sender<InputSample>) -> Result<()> {
    let mut dev =
        Device::open(&path).with_context(|| format!("opening input device {}", path.display()))?;
    let name = dev.name().unwrap_or("<unnamed>").to_string();

    if grab {
        dev.grab()
            .with_context(|| format!("grabbing {} (EVIOCGRAB)", path.display()))?;
    }

    tracing::info!(device = %path.display(), name, grab, "reading triggers");

    thread::Builder::new()
        .name(format!("evdev:{}", path.display()))
        .spawn(move || {
            loop {
                match dev.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            if let Some(sample) = normalize(&ev, &km) {
                                if tx.send(sample).is_err() {
                                    return; // daemon shutting down
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(device = %path.display(), error = %e, "evdev read failed");
                        return;
                    }
                }
            }
        })
        .context("spawning evdev reader thread")?;

    Ok(())
}

/// Discover readable keyboard-ish devices when the config lists none.
///
/// `exclude_name` drops the daemon's own virtual device: it declares every keycode, so without
/// this filter a macro that emits a bound key would feed its own output back in as a trigger.
/// Synthetic devices from other tools (`ydotoold`, `wtype`) are skipped for the same reason.
/// An explicit `-d` path bypasses both checks, because naming a node is a deliberate choice.
pub fn discover_devices(exclude_name: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("event") {
            continue;
        }
        // Only keep nodes we can actually read; opening is the honest test.
        if let Ok(dev) = Device::open(&path) {
            let dev_name = dev.name().unwrap_or("<unnamed>");
            if Some(dev_name) == exclude_name || is_virtual_device_name(dev_name) {
                continue;
            }
            let supported = dev.supported_keys();
            let has_keys = supported.map(|k| k.iter().count() > 20).unwrap_or(false);
            if has_keys {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Names that mark a node as synthetic rather than hardware.
///
/// A virtual keyboard emits whatever it is told to, so reading it as a trigger source lets one
/// macro fire another. Matched case-insensitively on the device name.
pub fn is_virtual_device_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("virtual") || n.contains("uinput") || n.contains("ydotool") || n.contains("wtype")
}

pub fn open_device_names(paths: &[PathBuf]) -> Vec<(PathBuf, String)> {
    paths
        .iter()
        .filter_map(|p| {
            Device::open(p)
                .ok()
                .map(|d| (p.clone(), d.name().unwrap_or("<unnamed>").to_string()))
        })
        .collect()
}
