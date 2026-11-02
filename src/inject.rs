//! Host-side injection through `/dev/uinput`.
//!
//! The original product never injects input — `PlayMacro` ends in `deployMacros(…, 0x10)` and
//! the install contains no `SendInput`/`keybd_event`/`mouse_event` import at all
//! (MACRO_SPEC.md §B.5). This module is the deliberate replacement: every keyboard becomes a
//! software injection target, which is the whole point of the clone.
//!
//! Sync policy: after each logical step (one key transition, one button transition, one wheel
//! notch) we emit `EV_SYN/SYN_REPORT` so the receiving application sees discrete steps rather
//! than coalesced state.
use std::path::Path;

use anyhow::{Context, Result};
use evdev::EventType as EvType;

use crate::keymap::{
    BTN_BACK, BTN_EXTRA, BTN_FORWARD, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_SIDE, BTN_TASK,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelAxis {
    Vertical,
    Horizontal,
}

/// Minimal injection surface so the engine is testable without a device.
pub trait Injector: Send {
    fn key(&mut self, evdev_code: i32, pressed: bool) -> Result<()>;
    fn button(&mut self, evdev_code: i32, pressed: bool) -> Result<()>;
    fn wheel(&mut self, axis: WheelAxis, delta: i32) -> Result<()>;
    fn sync(&mut self) -> Result<()>;
    fn describe(&self) -> &str;
}

/// Real device backed by uinput.
pub struct UinputInjector {
    dev: uinput::Device,
    name: String,
}

impl UinputInjector {
    /// Create the virtual keyboard + mouse for the daemon's lifetime.
    ///
    /// Failure is reported with the actual fix rather than a panic: on most distributions
    /// `/dev/uinput` is root-only until the udev rule ships (see README).
    pub fn create(name: &str) -> Result<Self> {
        preflight_uinput()?;

        let dev = uinput::default()
            .context("opening /dev/uinput")?
            .name(name)
            .context("setting device name")?
            // All EV_KEY codes: keyboard usages and BTN_* both live in that event type.
            .event(uinput::event::Keyboard::All)
            .context("declaring keyboard capability")?
            .event(uinput::event::Relative::Wheel(
                uinput::event::relative::Wheel::Vertical,
            ))
            .context("declaring vertical wheel capability")?
            .event(uinput::event::Relative::Wheel(
                uinput::event::relative::Wheel::Horizontal,
            ))
            .context("declaring horizontal wheel capability")?
            .create()
            .context("creating uinput device (check /dev/uinput permissions: see README)")?;

        Ok(Self {
            dev,
            name: name.to_string(),
        })
    }

    fn emit(&mut self, kind: EvType, code: i32, value: i32) -> Result<()> {
        self.dev
            .write(kind.0 as i32, code, value)
            .with_context(|| format!("writing {kind:?}/{code}"))
    }
}

impl Injector for UinputInjector {
    fn key(&mut self, evdev_code: i32, pressed: bool) -> Result<()> {
        self.emit(EvType::KEY, evdev_code, i32::from(pressed))
    }

    fn button(&mut self, evdev_code: i32, pressed: bool) -> Result<()> {
        self.emit(EvType::KEY, evdev_code, i32::from(pressed))
    }

    fn wheel(&mut self, axis: WheelAxis, delta: i32) -> Result<()> {
        let code = match axis {
            WheelAxis::Vertical => crate::keymap::REL_WHEEL,
            WheelAxis::Horizontal => crate::keymap::REL_HWHEEL,
        };
        self.emit(EvType::RELATIVE, code, delta)
    }

    fn sync(&mut self) -> Result<()> {
        self.dev.synchronize().context("EV_SYN")
    }

    fn describe(&self) -> &str {
        &self.name
    }
}

fn preflight_uinput() -> Result<()> {
    let path = Path::new("/dev/uinput");
    if !path.exists() {
        anyhow::bail!(
            "/dev/uinput does not exist.\n  \
             Load the module:        sudo modprobe uinput\n  \
             Persist it:             echo uinput | sudo tee /etc/modules-load.d/uinput.conf\n  \
             Then re-run; if it still fails the node is root-only — install the udev rule from README."
        );
    }
    // Access check without opening for write (uinput crate reports EACCES itself, but a clear
    // message up front is friendlier than a raw os error).
    match std::fs::OpenOptions::new().write(true).open(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => anyhow::bail!(
            "/dev/uinput is not writable ({e}).\n  \
             Quick fix:  sudo usermod -aG input $USER   (then log out/in)\n  \
             Or install the udev rule shipped in README so the node is group-writable."
        ),
        Err(e) => Err(e).context("opening /dev/uinput"),
    }
}

// ---------------------------------------------------------------------------
// Mock injector
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockEvent {
    Key { code: i32, pressed: bool },
    Button { code: i32, pressed: bool },
    Wheel { axis: WheelAxis, delta: i32 },
    Sync,
}

/// Records emissions. Used by the unit tests and by `--dry-run`.
#[derive(Debug, Default)]
pub struct MockInjector {
    pub events: Vec<MockEvent>,
    pub syncs: usize,
}

impl MockInjector {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Injector for MockInjector {
    fn key(&mut self, evdev_code: i32, pressed: bool) -> Result<()> {
        self.events.push(MockEvent::Key {
            code: evdev_code,
            pressed,
        });
        Ok(())
    }

    fn button(&mut self, evdev_code: i32, pressed: bool) -> Result<()> {
        self.events.push(MockEvent::Button {
            code: evdev_code,
            pressed,
        });
        Ok(())
    }

    fn wheel(&mut self, axis: WheelAxis, delta: i32) -> Result<()> {
        self.events.push(MockEvent::Wheel { axis, delta });
        Ok(())
    }

    fn sync(&mut self) -> Result<()> {
        self.events.push(MockEvent::Sync);
        self.syncs += 1;
        Ok(())
    }

    fn describe(&self) -> &str {
        "mock"
    }
}

/// All BTN_* codes we may emit, for capability declaration / validation.
pub const MOUSE_BUTTONS: [i32; 8] = [
    BTN_LEFT,
    BTN_RIGHT,
    BTN_MIDDLE,
    BTN_SIDE,
    BTN_EXTRA,
    BTN_FORWARD,
    BTN_BACK,
    BTN_TASK,
];
