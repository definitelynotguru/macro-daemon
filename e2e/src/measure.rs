//! Injection-boundary clock: records when each injection call reaches the injector.
//!
//! This is the measurement point: it wraps the injector the engine drives, so a
//! timestamp marks the moment the engine released that event to the device layer.
use std::time::Instant;

use anyhow::Result;

use macro_daemon::inject::{Injector, MockEvent, MockInjector, WheelAxis};

/// Forwards every call to the inner injector and stamps the wall clock on arrival.
pub struct TimestampingInjector<'a> {
    inner: &'a mut MockInjector,
    /// One stamp per forwarded call, in call order.
    stamps: Vec<Instant>,
}

impl<'a> TimestampingInjector<'a> {
    pub fn new(inner: &'a mut MockInjector) -> Self {
        Self {
            inner,
            stamps: Vec::new(),
        }
    }

    /// Instant of the n-th forwarded call (0-based).
    pub fn stamp(&self, call_index: usize) -> Option<Instant> {
        self.stamps.get(call_index).copied()
    }

    /// Inner mock event log, paired one-to-one with calls that emitted something.
    pub fn mock_events(&self) -> &[macro_daemon::inject::MockEvent] {
        &self.inner.events
    }
}

impl Injector for TimestampingInjector<'_> {
    fn key(&mut self, evdev_code: i32, pressed: bool) -> Result<()> {
        self.stamps.push(Instant::now());
        self.inner.key(evdev_code, pressed)
    }

    fn button(&mut self, evdev_code: i32, pressed: bool) -> Result<()> {
        self.stamps.push(Instant::now());
        self.inner.button(evdev_code, pressed)
    }

    fn wheel(&mut self, axis: WheelAxis, delta: i32) -> Result<()> {
        self.stamps.push(Instant::now());
        self.inner.wheel(axis, delta)
    }

    fn sync(&mut self) -> Result<()> {
        self.stamps.push(Instant::now());
        self.inner.sync()
    }

    fn describe(&self) -> &str {
        "timestamping"
    }
}

/// Hold duration from the press and release instants of KEY_A (evdev code 30).
///
/// The mock log always has exactly one Key press and one Key release per iteration,
/// at call indices 0 and 2 (press, sync, release); matching by code and direction keeps
/// that assumption explicit rather than positional.
pub fn hold_ms_of(clocked: &TimestampingInjector) -> Option<f64> {
    let mut press = None;
    let mut release = None;
    for (i, ev) in clocked.mock_events().iter().enumerate() {
        match ev {
            MockEvent::Key {
                code: 30,
                pressed: true,
            } => press = press.or(Some(i)),
            MockEvent::Key {
                code: 30,
                pressed: false,
            } => release = Some(i),
            _ => {}
        }
    }
    let (press, release) = press.zip(release)?;
    let (press_t, release_t) = (clocked.stamp(press)?, clocked.stamp(release)?);
    if release_t < press_t {
        return None;
    }
    Some(release_t.duration_since(press_t).as_secs_f64() * 1000.0)
}
