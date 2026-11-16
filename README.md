# macro-daemon

![Macro Editor: macro list, event chips, and the save bar.](app/screenshots/main-window.png)

A vendor-neutral macro daemon for Linux. It replays the macro *semantics* recovered from a
commercial peripheral suite (event schema, pacing model, repeat rules, key-code space) on top of
`evdev` (triggers) and `uinput` (injection).

The original engine is onboard-only: every "play" ends in a firmware deploy, and the shipped
product contains no input-injection API at all (see `MACRO_SPEC.md` §B.5). So this is not a port
of their runtime. It is a reimplementation of their data model and timing, with the device
replaced by a virtual one. Details of that decision are in [DESIGN.md](DESIGN.md).

## Features

- **Recovered pacing.** Timestamp offsets and `type:4` delay events replay exactly as recorded.
  `jitter_ms` exists because a user may want it and ships at 0, since the recovered format has no
  jitter field and adding randomness makes replay less faithful.
- **Per-app profiles.** Profile selection follows the focused window's executable path, never a
  window title. Focus comes from `hyprctl`, `swaymsg`, `xdotool`, or `xprop`, tried in that order.
- **Hot-reload.** `SIGHUP` re-reads the profiles directory and swaps it in atomically. A parse
  error logs and keeps the previous set; the daemon never dies on a bad edit.
- **Self-exclusion.** The daemon's own virtual device is excluded from trigger discovery, so a
  macro that emits its own trigger key cannot retrigger itself.
- **Tested without hardware.** 55 daemon tests run with no root and no input devices, so CI runs
  the suite anywhere. `cargo test --workspace` runs all 83.

## Build and install

```bash
cargo build --release          # target/release/macro-daemon (and macro-studio)
cargo test                     # 55 daemon tests, no root, no devices required
```

Rust 1.75+. Daemon dependencies: `evdev`, `uinput`, `serde`/`serde_json`, `clap`, `anyhow`,
`tracing`, `tracing-subscriber`, `signal-hook`. The `macro-studio` app adds Rune. A windowed
run on Linux needs `libxkbcommon-x11-0`. A headless `--snap` run needs a Vulkan driver
(`mesa-vulkan-drivers`, `VK_ICD_FILENAMES` pointing at lavapipe when the machine has no GPU).
No network calls at run time. The Rune git pin is fetched at build time.

The daemon needs write on `/dev/uinput` and read on `/dev/input/event*`:

```bash
# 1. uinput module
sudo modprobe uinput
echo uinput | sudo tee /etc/modules-load.d/uinput.conf    # persist

# 2. permissions
sudo usermod -aG input "$USER"                            # then log out and back in
sudo install -m 644 packaging/60-macro-daemon.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

If `/dev/uinput` is missing or unwritable the daemon says exactly that and names the fix; it does
not panic:

```
Error: /dev/uinput does not exist.
  Load the module:        sudo modprobe uinput
  ...
```

## UI tour

macro-studio is the Rune editor. The tour is in [macro-studio — the GUI editor](#macro-studio--the-gui-editor).

## Run

```bash
macro-daemon                       # uses ./daemon.json if present
macro-daemon --config /etc/macro-daemon/daemon.json
macro-daemon --list-devices        # what can be read
macro-daemon --validate            # parse configs, print bindings + both playback numberings
macro-daemon --trace default:m-f5x2   # timed trace of one macro, exits
macro-daemon --dry-run             # real triggers, logged injections, no /dev/uinput writes
```

`SIGINT`/`SIGTERM` shut down cleanly. Optional hotkey in `daemon.json`
(`"toggle_hotkey": "KEY_F11"`) suspends/resumes the daemon without stopping it; the event is
swallowed and never reaches the macro engine.

## Configuration

Two layers: a daemon config and a directory of profiles.

### `daemon.json`

```json
{
  "devices": [],
  "profiles_dir": "profiles",
  "default_profile": "default",
  "jitter_ms": 0,
  "toggle_hotkey": "KEY_F11",
  "focus_poll_ms": 500,
  "debounce_ms": 5,
  "grab": false
}
```

| field | meaning |
|---|---|
| `devices` | evdev nodes to read. Empty = autodiscover readable keyboards. The daemon's own virtual device is always excluded (otherwise an F5-emitting macro bound to F5 would retrigger itself). |
| `profiles_dir` | directory of `*.json` profiles |
| `default_profile` | profile used when no per-app rule matches |
| `jitter_ms` | uniform ±ms around every delay. **0 = off**, which is the faithful setting: the recovered format has no jitter field (`MACRO_SPEC.md` §A.4) |
| `toggle_hotkey` | `KEY_*` name from `keymap.json`, or a numeric evdev code |
| `focus_poll_ms` | focus poll interval for per-app rules |
| `debounce_ms` | per-binding re-trigger guard |
| `grab` | `EVIOCGRAB` the trigger devices (they stop reaching the rest of the system). Refused at startup when any binding sets `is_passthrough`, because then the key cannot reach the application. |

### `profiles/<name>.json`

Field names are the recovered ones, so a config can be diffed against real engine data:
`macros[].events[]` is `{type, page, code, extraData, timestamp}`, `bindings[].trigger_data` is
`{hidCode, page, type, direction}`, `bindings[].playback_options_data` is
`{type, num_repeat, repeat_delay}`.

```json
{
  "version": 1,
  "profile": { "name": "default", "apps": [] },
  "macros": [
    {
      "id": "m-repeat-demo",
      "name": "two-event-repeat",
      "events": [
        { "type": 2, "page": 1, "code": 4,  "extraData": 1, "timestamp": 0 },
        { "type": 2, "page": 1, "code": 4,  "extraData": 0, "timestamp": 0 }
      ]
    }
  ],
  "bindings": [
    {
      "type": "customAction",
      "macro_id": "m-repeat-demo",
      "trigger_data": { "hidCode": 66, "page": 1, "type": 2, "direction": 1 },
      "playback_options_data": { "type": 1, "num_repeat": 3, "repeat_delay": 50 }
    }
  ]
}
```

**Worked example — "ctrl+shift+v", once, bound to F9:**

`hidCode` 56 is F9 (HID usage `0x38`); modifiers are `0xE0`/`0xE1`; `V` is `0x19`.

```json
{
  "version": 1,
  "profile": { "name": "work", "apps": ["/usr/bin/code", "code"] },
  "macros": [{
    "id": "m-paste-plain",
    "name": "paste-plain",
    "events": [
      { "type": 2, "page": 1, "code": 224, "extraData": 1, "timestamp": 0 },
      { "type": 2, "page": 1, "code": 225, "extraData": 1, "timestamp": 0 },
      { "type": 2, "page": 1, "code": 25,  "extraData": 1, "timestamp": 0 },
      { "type": 2, "page": 1, "code": 25,  "extraData": 0, "timestamp": 0 },
      { "type": 2, "page": 1, "code": 225, "extraData": 0, "timestamp": 0 },
      { "type": 2, "page": 1, "code": 224, "extraData": 0, "timestamp": 0 }
    ]
  }],
  "bindings": [{
    "type": "customAction",
    "macro_id": "m-paste-plain",
    "trigger_data": { "hidCode": 56, "page": 1, "type": 2, "direction": 1 },
    "playback_options_data": { "type": 0, "num_repeat": 1, "repeat_delay": 15 }
  }]
}
```

`apps` entries are matched against the focused process's executable path (exact, or basename
equality), never a window title, which is what the original does (`applications.path`,
`MACRO_SPEC.md` §D.4). If no focus source is available the daemon logs once and stays on the
current profile.

### Playback types — the one place to be careful

`playback_options_data.type` on disk uses the **client** numbering; an onboard macro header uses
the **wire** numbering. They differ by one, and mixing them is the obvious silent bug: `type: 1`
means *play once* in one table and *repeat N times* in the other.

| mode | client (config/DB) | wire (firmware) |
|---|---|---|
| playOnce | 0 | 1 |
| repeatNTimes | 1 | 2 |
| repeatWhilePressed | 2 | 3 |
| toggleRepeat | 3 | 4 |
| toggleHold | 4 | 5 |
| holdWhilePressed | (no client slot) | 6 |

Both are modelled by one `#[repr(u8)]` enum whose discriminants are the **wire** values, with
`from_client` / `to_client` / `from_wire` / `to_wire` conversions. Unknown values are hard errors;
nothing is coerced. `--validate` prints both numbers per binding so mismatches are visible.

## macro-studio — the GUI editor

`macro-studio` is a Rune desktop app for editing profiles without hand-editing JSON. It is a
separate workspace member that talks to this crate: the `macro_daemon` config types are the
single source of truth, so what it saves is exactly what the daemon reads.

```bash
cargo build --workspace          # builds macro-daemon and macro-studio
cargo run -p macro-studio        # opens ./profiles
cargo run -p macro-studio -- ~/my-profiles
cargo run -p macro-studio -- --validate profiles   # parse + validate + round-trip, then exit
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
  cargo run -p macro-studio --release -- --snap app/screenshots/main-window.png --scene delay profiles
```

The window is the Macro Editor. The title is `Macro Editor`.

![Macro list on the left, event chips in a strip, save on the right.](app/screenshots/main-window.png)

- **Left**: a MACROS rail. Each row is a radio and a name. The selected row is orange.
- **Center**: the selected macro's events as chips in one strip. A delay chip shows its milliseconds. The rest of the canvas is empty.
- **Bottom**: gear (prints the profiles directory), CLEAR, START (plays the selected macro through `MockInjector` and reports the plan), record (does not write events; this build has no keyboard capture), REVERT, SAVE.

SAVE validates before writing. A file the daemon would reject is never written. REVERT re-reads the directory and discards unsaved edits, the same semantics as sending the daemon SIGHUP.

Cold-start numbers for this machine are in [docs/ui-cold-start.md](docs/ui-cold-start.md).

## Testing

`cargo test` runs 55 tests across five suites with no root and no devices. The regression suite
pins the bugs that were found by running the daemon, not by reading the spec:

- `hotkey_is_matched_in_hid_space_not_evdev_space` — the toggle hotkey resolves in HID usage
  space, like every other key reference.
- `synthetic_devices_are_recognised_by_name` — `ydotoold`, `wtype`, and the daemon's own virtual
  device are excluded from autodiscovery, so injected output cannot feed back in as input.
- `locked_injector_does_not_hold_the_device_across_a_macro` — the device lock releases between
  macros, so a long macro cannot block the next one.
- `gate_key_survives_a_profile_switch` — the toggle hotkey still resolves after a profile swap.

The end-to-end hold-timing proof is in [docs/e2e-timing.md](docs/e2e-timing.md). The harness
authors a macro of press, 400 ms delay, release, plays it through the real `Runner::execute` path,
and measures the wall-clock gap between the press and the release at the injection boundary. The
harness package carries 9 more tests and the studio 19; `cargo test --workspace` runs all 83:

```
cargo run -p macro-e2e --release -- --hold-ms 400 --iters 30

      stat     hold_ms
       min     400.075
    median     400.097
       p95     400.153
       max     400.158

pass: yes (|median - 400| = 0.097 ms)
```

This is an engine-level result, measured with `MockInjector` behind a timestamping wrapper, which
is exactly what a real device would consume. The container that produced it has no `/dev/uinput`,
so no device-level timing is claimed; the doc shows how to reproduce the device-level probe on a
machine that has the node.

## Compatibility table

Comments in the source cite `MACRO_SPEC.md §X`. That file is the behavioural spec this was written
against; it is not part of this repository, but every citation names the rule being implemented.

| `MACRO_SPEC.md` behaviour | implementation |
|---|---|
| §A.1 `macros` row (id/name/events/recordingOptions/is_deleted/is_new) | `config::Macro` |
| §A.2 event object `{type,page,code,extraData,timestamp}` | `config::MacroEvent`, `config::EventType` |
| §A.2 `{"events": …}` column framing | `config::Macro::db_events_string` / `from_db_events_string` |
| §A.2 event → input mapping (incl. 255 ≡ −1) | `engine::Runner::run_once`, `engine::signed_tiny` |
| §A.4 `playback_options_data` + dual numbering | `config::PlaybackOptions`, `config::PlaybackType` |
| §A.5 `recording_options {timeout}` | `config::RecordingOptions` |
| §B.4 pacing: timestamp offsets, `type:4` delays | `engine::plan_offsets` |
| §B.4 repeat table (`num_repeat`, `repeat_delay` placement) | `engine::Runner::execute` |
| §B.5 software injection (the gap the original has) | `inject::UinputInjector` |
| §C.1/C.3 HID usage → evdev, no guessing | `keymap::Keymap`, `keymap::Keymap::resolve` |
| §C.3 unknown usage: log + skip | `engine::Runner::warn_missing` |
| §D.1 `bindings` row | `config::Binding` |
| §D.2 `trigger_data` | `config::TriggerData`, `trigger::InputSample::matches` |
| §D.3 binding types / transparent macros | `config::binding_types`, `main::daemon` |
| §D.4 per-app profile by executable path | `config::ProfileSet::select_for`, `apprule::FocusWatcher` |
| §B.3 onboard blob authoring | **not implemented** (out of scope) |

## [SPEC-GAP] items

Each is a `TODO`-style comment in code with the spec reference:

1. **§E.3 timestamp origin** — `engine::plan_offsets`. Absolute-from-start reading; degrades to
   sequential order when all timestamps are 0, which is what the editor emits.
2. **§A.2 wheel/tilt values other than 1/255** — `engine::signed_tiny`. Values < 128 positive,
   else −1.
3. **§E.5 consumer page table** — `keymap::consumer_usage_to_evdev`. Rebuilt from the public HID
   consumer page; the original's numeric list lives in encrypted device descriptors.
4. **§B.1 text/OS macro expansion** — `main::daemon`. `text_macro` / `osMacroAction` bindings log a
   warning instead of silently doing nothing; converting text to HID events needs a keyboard
   layout model that the spec does not describe.
5. **§B.4 `holdWhilePressed` / "hold" semantics** — `engine::Runner::execute` treats
   `holdWhilePressed` as a held-mode loop, the conservative reading of the one-line description.

## Troubleshooting

| symptom | cause |
|---|---|
| `no readable input devices` | not in `input` group, or udev rule not applied |
| `per-application rules disabled` | no `hyprctl`/`swaymsg`/`xdotool`/`xprop` on PATH |
| macro fires its own output | the daemon excludes its own device and other synthetic keyboards (`ydotoold`, `wtype`, `uinput` in the name) during autodiscovery. An explicit `-d` path skips that filter, so do not point it at a virtual node |
| nothing happens on Wayland with `grab: true` | grabbing removes the device from the compositor; use `grab: false` unless you know you want it |

## License

MIT — see [LICENSE](LICENSE).
