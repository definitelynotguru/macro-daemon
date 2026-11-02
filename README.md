# macro-daemon

A vendor-neutral macro daemon for Linux. It replays the macro *semantics* recovered from a
commercial peripheral suite — event schema, pacing model, repeat rules, key-code space — on top of
`evdev` (triggers) and `uinput` (injection).

The original engine is onboard-only: every "play" ends in a firmware deploy, and the shipped
product contains no input-injection API at all (see `MACRO_SPEC.md` §B.5). So this is not a port
of their runtime. It is a reimplementation of their data model and timing, with the device
replaced by a virtual one. Details of that decision are in [DESIGN.md](DESIGN.md).

---

## Build

```bash
cargo build --release     # target/release/macro-daemon
cargo test                # 47 tests, no root, no devices required
```

Rust 1.75+. Dependencies: `evdev`, `uinput`, `serde`/`serde_json`, `clap`, `anyhow`, `tracing`,
`tracing-subscriber`, `signal-hook`. No network calls at build or run time.

## Install

```bash
# 1. uinput module
sudo modprobe uinput
echo uinput | sudo tee /etc/modules-load.d/uinput.conf    # persist

# 2. permissions: the daemon needs write on /dev/uinput and read on /dev/input/event*
sudo usermod -aG input "$USER"                            # then log out and back in
sudo install -m 644 packaging/60-macro-daemon.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

If `/dev/uinput` is missing or unwritable the daemon says exactly that and names the fix — it
does not panic:

```
Error: /dev/uinput does not exist.
  Load the module:        sudo modprobe uinput
  ...
```

## Run

```bash
macro-daemon                       # uses ./daemon.json if present
macro-daemon --config /etc/macro-daemon/daemon.json
macro-daemon --list-devices        # what can be read
macro-daemon --validate            # parse configs, print bindings + both playback numberings
macro-daemon --trace default:m-f5x2   # timed trace of one macro, exits
macro-daemon --dry-run             # real triggers, logged injections, no /dev/uinput writes
```

`SIGHUP` re-reads the profiles directory and swaps it in atomically; a parse error logs and keeps
the previous set (the daemon never dies on a bad edit). `SIGINT`/`SIGTERM` shut down cleanly.

Optional hotkey in `daemon.json` (`"toggle_hotkey": "KEY_F11"`) suspends/resumes the daemon
without stopping it; the event is swallowed and never reaches the macro engine.

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
| `grab` | `EVIOCGRAB` the trigger devices (they stop reaching the rest of the system) |

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
equality) — never a window title, which is what the original does (`applications.path`,
`MACRO_SPEC.md` §D.4). Focus comes from `hyprctl`, `swaymsg`, `xdotool` or `xprop`, tried in that
order; if none is available the daemon logs once and stays on the current profile.

### Playback types — the one place to be careful

`playback_options_data.type` on disk uses the **client** numbering; an onboard macro header uses
the **wire** numbering. They differ by one:

| mode | client (config/DB) | wire (firmware) |
|---|---|---|
| playOnce | 0 | 1 |
| repeatNTimes | 1 | 2 |
| repeatWhilePressed | 2 | 3 |
| toggleRepeat | 3 | 4 |
| toggleHold | 4 | 5 |
| holdWhilePressed | — (no client slot) | 6 |

Both are modelled by one `#[repr(u8)]` enum whose discriminants are the **wire** values, with
`from_client` / `to_client` / `from_wire` / `to_wire` conversions. Unknown values are hard errors;
nothing is coerced. `--validate` prints both numbers per binding so mismatches are visible.

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
| macro fires its own output | you bound a key the macro itself emits; the daemon excludes its own device, but a *second* daemon instance would not |
| nothing happens on Wayland with `grab: true` | grabbing removes the device from the compositor; use `grab: false` unless you know you want it |

## License

MIT — see [LICENSE](LICENSE).
