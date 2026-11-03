//! Macro execution: pacing, repeat semantics, event replay.
//!
//! Every behaviour here traces to MACRO_SPEC.md:
//!   * event → input mapping      §A.2 table
//!   * pacing model               §B.4 (timestamp offsets, `type:4` delay events)
//!   * repeat semantics           §B.4 table + §A.4 dual numbering
//!   * no jitter by default       §A.3/A.4 (the format has no jitter field)
use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;

use crate::clock::{Jitter, Scheduler, Wait};
use crate::config::{EventType, MacroEvent, PlaybackOptions, PlaybackType};
use crate::inject::{Injector, WheelAxis};
use crate::keymap::{self, Keymap};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RunStats {
    pub iterations: u32,
    pub injected: u32,
    pub skipped: u32,
    pub planned_ms: u64,
}

/// Turn the event list into monotonic millisecond offsets from macro start.
///
/// Two readings are possible and the spec only pins one of them:
///   * if any event carries a non-zero `timestamp`, timestamps are absolute offsets —
///     that is what a recorded macro produces;
///   * otherwise the macro is a sequential script and only `type:4` delay events carry time.
///
/// [SPEC-GAP] MACRO_SPEC.md §E.3 — the timestamp origin (recording start vs. previous event)
/// is not observable statically. Absolute-from-start is the conservative reading: it cannot
/// compress a recorded macro, and it degrades exactly to sequential order when all timestamps
/// are zero, which is what the editor emits (`pretty/render_463.js:32242`).
pub fn plan_offsets(events: &[MacroEvent]) -> Vec<u64> {
    let absolute = events.iter().any(|e| e.timestamp > 0);
    let mut out = Vec::with_capacity(events.len());
    let mut running: u64 = 0;
    for ev in events {
        if absolute {
            out.push(ev.timestamp as u64);
        } else if ev.event_type.is_delay() {
            running = running.saturating_add(ev.extra_data as u64);
            out.push(running);
        } else {
            out.push(running);
        }
    }
    out
}

/// True when a macro carries both absolute timestamps and explicit delay events.
///
/// Timestamp mode treats `type:4` delays as redundant, so a macro that mixes both loses the
/// delay rows. The condition is legal but suspicious, and the runner logs it instead of
/// silently replaying without the delays.
pub fn has_mixed_timing(events: &[MacroEvent]) -> bool {
    let timed = events.iter().any(|e| e.timestamp > 0);
    let delayed = events.iter().any(|e| e.event_type.is_delay());
    timed && delayed
}

/// Upper bound on loop iterations so a stuck "held" gate can never pin a thread forever.
const MAX_ITERATIONS: u32 = 10_000;

pub struct Runner<'a> {
    pub injector: &'a mut dyn Injector,
    pub keymap: &'a Keymap,
    pub wait: &'a mut dyn Wait,
    pub jitter: Jitter,
    /// Pairs already reported as unmappable, so a broken macro logs once, not once per event.
    warned: HashSet<(u8, u16)>,
    rng: u64,
}

impl<'a> Runner<'a> {
    pub fn new(injector: &'a mut dyn Injector, keymap: &'a Keymap, wait: &'a mut dyn Wait) -> Self {
        Self {
            injector,
            keymap,
            wait,
            jitter: Jitter::off(),
            warned: HashSet::new(),
            rng: crate::clock::seed_from_time(),
        }
    }

    pub fn with_jitter(mut self, jitter: Jitter) -> Self {
        self.jitter = jitter;
        self
    }

    /// Replay a macro according to its playback options.
    ///
    /// `gate` is consulted before every iteration after the first and only for the looping
    /// modes (`repeatWhilePressed`, `toggleRepeat`, `toggleHold`, `holdWhilePressed`). It
    /// returns whether another run should happen — for held modes that is "is the trigger key
    /// still down", for toggle modes "has the toggle not been switched off".
    pub fn execute(
        &mut self,
        events: &[MacroEvent],
        opts: &PlaybackOptions,
        gate: &mut dyn FnMut(u32) -> bool,
    ) -> Result<RunStats> {
        let mut stats = RunStats::default();
        let planned = plan_offsets(events);
        stats.planned_ms = planned.last().copied().unwrap_or(0);

        if has_mixed_timing(events) {
            tracing::warn!(
                delays = events.iter().filter(|e| e.event_type.is_delay()).count(),
                "macro mixes absolute timestamps with delay events; timestamps win and the \
                 delay rows are not applied (MACRO_SPEC.md §E.3)"
            );
        }

        // MACRO_SPEC.md §B.4: `playOnce` runs once. `repeatNTimes` runs num_repeat times
        // (clamped >= 1 — the DB column is uint8 with a client-side floor of 1). Everything
        // else loops on the gate.
        let fixed_iterations: Option<u32> = match opts.playback_type {
            PlaybackType::PlayOnce => Some(1),
            PlaybackType::RepeatNTimes => Some(u32::from(opts.num_repeat.max(1))),
            _ => None,
        };

        let mut iteration: u32 = 0;
        loop {
            if iteration > 0 {
                if let Some(total) = fixed_iterations {
                    if iteration >= total {
                        break;
                    }
                } else if !gate(iteration) {
                    break;
                }

                // `repeat_delay` sits *between* whole runs, and only for the three modes the
                // recovered client enables it for (§B.4 table: repeatNTimes,
                // repeatWhilePressed, toggleRepeat).
                if opts.playback_type.uses_repeat_delay() {
                    let sched = Scheduler::new(self.jitter);
                    sched.sleep_for(u64::from(opts.repeat_delay), self.wait, &mut self.rng);
                    stats.planned_ms = stats
                        .planned_ms
                        .saturating_add(u64::from(opts.repeat_delay));
                }
            }

            self.run_once(events, &planned, &mut stats)?;
            iteration += 1;
            stats.iterations = iteration;

            if iteration >= MAX_ITERATIONS {
                tracing::warn!(
                    "macro reached the {} iteration safety cap; stopping",
                    MAX_ITERATIONS
                );
                break;
            }
            if let Some(total) = fixed_iterations {
                if iteration >= total {
                    break;
                }
            }
        }

        Ok(stats)
    }

    fn run_once(
        &mut self,
        events: &[MacroEvent],
        planned: &[u64],
        stats: &mut RunStats,
    ) -> Result<()> {
        let scheduler = Scheduler::new(self.jitter);

        for (idx, ev) in events.iter().enumerate() {
            let offset = planned.get(idx).copied().unwrap_or(0);
            scheduler.sleep_until_offset(offset, self.wait, &mut self.rng);

            match ev.event_type {
                // §A.2: page selects the usage page; page 1 keyboard, page 12 consumer.
                EventType::KeyboardKey => {
                    let code = match ev.page {
                        keymap::PAGE_CONSUMER => keymap::consumer_usage_to_evdev(ev.code),
                        _ => self.keymap.lookup(ev.page, ev.code).map(|k| k.evdev),
                    };
                    match code {
                        Some(code) => {
                            let pressed = ev.extra_data != 0;
                            self.injector.key(code, pressed)?;
                            self.injector.sync()?;
                            stats.injected += 1;
                        }
                        None => {
                            self.warn_missing(ev.page, ev.code);
                            stats.skipped += 1;
                        }
                    }
                }
                // §A.2: mouse button, code 1..8, extraData 1 = down / 0 = up.
                EventType::MouseButton | EventType::MouseExtraButton => {
                    match keymap::mouse_button_code(ev.code) {
                        Some(code) => {
                            let pressed = ev.extra_data != 0;
                            self.injector.button(code, pressed)?;
                            self.injector.sync()?;
                            stats.injected += 1;
                        }
                        None => {
                            self.warn_missing(0, ev.code);
                            stats.skipped += 1;
                        }
                    }
                }
                // §A.2: wheel delta is 1 or 255 (−1 as a byte).
                EventType::MouseWheel => {
                    let delta = signed_tiny(ev.extra_data);
                    if delta != 0 {
                        self.injector.wheel(WheelAxis::Vertical, delta)?;
                        self.injector.sync()?;
                        stats.injected += 1;
                    }
                }
                // §A.2 lists tilt as type 12 with the same 1/255 encoding.
                EventType::MouseTilt => {
                    let delta = signed_tiny(ev.extra_data);
                    if delta != 0 {
                        self.injector.wheel(WheelAxis::Horizontal, delta)?;
                        self.injector.sync()?;
                        stats.injected += 1;
                    }
                }
                // Time is consumed by the scheduler above, not injected.
                EventType::Delay => {}
                // The engine's own metadata marker (§B.4); repeats are computed here instead.
                EventType::RepeatMarker => {}
            }
        }
        Ok(())
    }

    fn warn_missing(&mut self, page: u8, code: u16) {
        if self.warned.insert((page, code)) {
            tracing::warn!(
                page,
                code = format!("0x{code:02x}"),
                "no evdev equivalent for this HID usage; event skipped (MACRO_SPEC.md §C.3)"
            );
        }
    }
}

/// Decode the engine's signed-byte-in-a-u32 wheel convention (1 / 255).
fn signed_tiny(v: u32) -> i32 {
    match v {
        0 => 0,
        1 => 1,
        255 => -1,
        // [SPEC-GAP] MACRO_SPEC.md §A.2 documents only 1/255 for wheel and tilt; larger values
        // are not described. Conservative reading: treat values < 128 as positive, else −1.
        v if v < 128 => v as i32,
        _ => -1,
    }
}

/// Total wall time a macro occupies, useful for `--dry-run` reporting.
pub fn estimated_duration(events: &[MacroEvent], opts: &PlaybackOptions) -> Duration {
    let plan = plan_offsets(events);
    let single = plan.last().copied().unwrap_or(0);
    let runs = match opts.playback_type {
        PlaybackType::PlayOnce => 1,
        PlaybackType::RepeatNTimes => u64::from(opts.num_repeat.max(1)),
        _ => 1,
    };
    let gaps = if opts.playback_type.uses_repeat_delay() {
        u64::from(opts.repeat_delay) * runs.saturating_sub(1)
    } else {
        0
    };
    Duration::from_millis(single * runs + gaps)
}
