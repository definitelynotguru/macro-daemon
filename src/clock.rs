//! Monotonic scheduling.
//!
//! The recovered engine carries timing in the data (per-event `timestamp` and explicit
//! `type:4` delay events) rather than in a host-side timer — see MACRO_SPEC.md §B.4. This module
//! is the only place wall-clock sleeping happens, so pacing can be unit-tested with a fake.
use std::time::{Duration, Instant};

/// Abstraction over "sleep for a while" so tests never touch the real clock.
pub trait Wait {
    fn wait(&mut self, d: Duration);
}

#[derive(Debug, Default)]
pub struct RealWait;

impl Wait for RealWait {
    fn wait(&mut self, d: Duration) {
        if !d.is_zero() {
            std::thread::sleep(d);
        }
    }
}

/// Records requested sleeps instead of performing them.
#[derive(Debug, Default)]
pub struct FakeWait {
    pub slept: Vec<Duration>,
}

impl FakeWait {
    pub fn total(&self) -> Duration {
        self.slept.iter().copied().sum()
    }
}

impl Wait for FakeWait {
    fn wait(&mut self, d: Duration) {
        self.slept.push(d);
    }
}

/// Uniform ±ms jitter.
///
/// The recovered format has no jitter, randomness or min/max bound field at all
/// (MACRO_SPEC.md §A.3/A.4 — `delay` is a plain 16/24-bit millisecond count), so this is off by
/// default and exists only because a user may want it. Enabling it makes replay *less* faithful.
#[derive(Debug, Clone, Copy, Default)]
pub struct Jitter {
    pub max_ms: u32,
}

impl Jitter {
    pub fn off() -> Self {
        Self { max_ms: 0 }
    }

    pub fn is_off(&self) -> bool {
        self.max_ms == 0
    }

    /// Apply jitter using a cheap xorshift so we do not pull in an RNG crate.
    pub fn apply(&self, base: Duration, state: &mut u64) -> Duration {
        if self.max_ms == 0 {
            return base;
        }
        let span = (self.max_ms as u64) * 2 + 1;
        let r = xorshift(state) % span;
        let delta = r as i64 - self.max_ms as i64;
        let ms = base.as_millis() as i64 + delta;
        Duration::from_millis(ms.max(0) as u64)
    }
}

pub fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    if x == 0 {
        x = 0x9E3779B97F4A7C15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

pub fn seed_from_time() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

/// Deadline clock: sleeps the remaining time to an absolute monotonic offset.
#[derive(Debug)]
pub struct Scheduler {
    start: Instant,
    pub jitter: Jitter,
}

impl Scheduler {
    pub fn new(jitter: Jitter) -> Self {
        Self {
            start: Instant::now(),
            jitter,
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Sleep until `offset_ms` from the scheduler's creation, never sleeping negative.
    pub fn sleep_until_offset(&self, offset_ms: u64, w: &mut dyn Wait, rng: &mut u64) {
        let now = self.elapsed_ms();
        if offset_ms > now {
            let d = Duration::from_millis(offset_ms - now);
            w.wait(self.jitter.apply(d, rng));
        }
    }

    pub fn sleep_for(&self, ms: u64, w: &mut dyn Wait, rng: &mut u64) {
        if ms > 0 {
            w.wait(self.jitter.apply(Duration::from_millis(ms), rng));
        }
    }
}
