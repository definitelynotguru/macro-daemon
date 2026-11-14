# End-to-end hold timing

## Why

Imagine a website that asks you to hold a key for exactly 400 ms. Can our macro do
that when the timing is authored into it? This harness answers that question by
running the daemon's real engine on a macro that carries a 400 ms hold, and
measuring how long the engine actually held the key.

## What it proves and what it does not

The `macro-e2e` binary authors a macro of three events with the public
`macro_daemon` API:

1. press KEY_A (HID usage 4, usage page 1; evdev code 30)
2. delay 400 ms (`MacroEvent::delay(400)`)
3. release KEY_A

It plays the macro once (`PlaybackType::PlayOnce`) through the real
`Runner::execute` path with `RealWait` and jitter off, exactly as the daemon
would. The measurement happens at the injection boundary: a
`TimestampingInjector` wraps a `MockInjector` and records `Instant::now()` for
each forwarded call. The hold time is the time between the press call and the
release call.

This proves the engine paces an authored hold to the tolerance you ask for, at
the layer a real device would consume. This is an engine-level result. It does
not prove device-level timing: the container has no `/dev/uinput`, so no real
virtual-device emission was timed. Readback of real key events also needs an
input node, which this environment does not have.

## How to run

Human-readable table:

```
cargo run -p macro-e2e -- --hold-ms 400 --iters 30
```

Machine-readable output, one JSON object on stdout:

```
cargo run -p macro-e2e -- --hold-ms 400 --iters 30 --json
```

Pass condition: the median hold stays within 10 ms of the target
(`|median - 400| <= 10`). The `pass` JSON field and the table both carry it.
Add `--device` to also run a device-level probe; the probe needs a real
`/dev/uinput` and is skipped with a one-line note when the node is absent.

## Results from this run

Command:

```
cargo run -p macro-e2e --release -- --hold-ms 400 --iters 30
```

Date: 2026-11-14. Environment: containerized x86_64 Linux, 2 cores,
`/dev/uinput` absent. Injector: `MockInjector` behind `TimestampingInjector`.

```
engine-level hold timing, target 400 ms (tolerance 10 ms on the median)
iterations: 30

      stat     hold_ms
       min     400.087
    median     400.107
       p95     400.155
       max     400.166

pass: yes (|median - 400| = 0.107 ms)
```

These numbers are engine-level. Every iteration landed within 0.2 ms of the
400 ms target, so the acceptance test passes with a wide margin. Repeat runs:
three independent runs in this environment (two release, one debug build) gave
medians 400.102 to 400.117 ms; the spread of the median across runs is under
0.02 ms.

The measured hold sits slightly above 400 ms by design. The engine's scheduler
paces to absolute offsets but truncates elapsed time to whole milliseconds
(`clock.rs`, `Scheduler::elapsed_ms` uses `as_millis()`), so each sleep is a
fraction of a millisecond short of the remaining time. The remainder lands
after the wake, before the release reaches the injector. The gap is bounded by
one scheduling quantum and stayed under 0.2 ms across 30 iterations, far inside
the 10 ms tolerance.

## Environmental limits

The container runs no `/dev/uinput` node, so the `uinput` crate cannot create
the virtual keyboard that real injection needs. Device-level timing therefore
cannot run here, and no device-level numbers are claimed. The `--device` path
compiles and is wired into the binary; it exits with a clear message when the
node is missing. Reading injected events back also needs an input node under
`/dev/input`, which this environment does not expose either.

## Proving device level on a real Linux box

Run these on a Linux machine where you have sudo:

1. Load the uinput module and make it persist across reboots.

   ```
   sudo modprobe uinput
   echo uinput | sudo tee /etc/modules-load.d/uinput.conf
   ```

2. Check that the node exists and is writable by your user. The command prints
   your groups; `input` must be one of them.

   ```
   ls -l /dev/uinput
   groups
   ```

   If you are not in the `input` group, add yourself, then log out and back in:

   ```
   sudo usermod -aG input $USER
   ```

3. Build and run the device-level probe. It creates the virtual keyboard
   `macro-e2e-probe` and injects the same hold-400 ms macro through it.

   ```
   cargo run -p macro-e2e --release -- --hold-ms 400 --iters 30 --device
   ```

4. Verify the timing at the device level in a second terminal. Find the virtual
   device and record its events while the probe runs.

   ```
   sudo apt install evtest
   ls /dev/input/event*
   sudo evtest
   ```

   Pick the device named `macro-e2e-probe` from the evtest list. You see
   `EV_KEY KEY_A` with value 1 (press), a gap of about 400 ms, and value 0
   (release). Timestamps in the evtest output show the gap directly.

   For a machine-readable capture, use `libinput record` instead:

   ```
   sudo libinput record /dev/input/event<N>
   ```

   The recorded YAML carries kernel timestamps for the press and release events;
   the difference is the measured hold.

5. To measure many iterations at the device level, run the probe in one
   terminal while `sudo evtest /dev/input/event<N>` writes the event stream to
   a file, then compute press-to-release gaps from the timestamps. A reading
   within 10 ms of 400 ms on the median is the same pass condition.
