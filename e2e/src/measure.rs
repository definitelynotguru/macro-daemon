//! Injection-boundary clock: records when each injection call reaches the injector.
//!
//! This is the measurement point: it wraps the injector the engine drives, so a
//! timestamp marks the moment the engine released that event to the device layer.
use std::time::Instant;

use anyhow::Result;

use macro_daemon::inject::{Injector, MockInjector, WheelAxis};

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
