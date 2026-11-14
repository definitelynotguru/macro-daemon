# macro-studio design notes

## Why this exists

macro-daemon today only serves people who edit `profiles/*.json` by hand. The goal is a real
desktop macro editor, in the spirit of SteelSeries GG's macro editor, built with GPUI, that
talks to the existing `macro_daemon` library. The smallest change that meets the goal is one
new workspace member, `app/`, with no changes to the daemon's engine. The library stays the
single source of truth for the schema: the app never re-parses or re-serializes profile JSON
outside `macro_daemon::config` types.

## Data shape (chosen first, per model-the-domain)

Three layers, one direction of flow:

1. **`Workspace` (entity)** — owns everything. Fields:
   - `profiles: Vec<ProfileDoc>` where `ProfileDoc { file_name: String, file: ProfileFile }`.
     A `Vec`, not a `BTreeMap`, so sidebar order is stable and rename is a move, not a
     re-insert. Sorted by `file_name` at load.
   - `open_profile: Option<String>` (file_name key into `profiles`).
   - `selected_macro: Option<String>` (macro id within the open profile).
   - `selected_event: Option<usize>` (index into the selected macro's events).
   - `dirty: bool`, `status: Status` (idle / error(String) / info(String) with a timestamp
     so the status bar can fade a message after a few seconds).
   - `dir: PathBuf` — the profiles directory the workspace was opened from.
   `Workspace::load_dir(dir)` wraps `ProfileSet::load_dir`-style loading but keeps per-file
   documents so individual saves are possible. A load error for one file skips that file and
   records the error in `status` rather than failing the whole workspace.
2. **Domain rules the UI needs, encoded once** (`schema.rs`):
   - `PlaybackType::ALL` → `(label, client_code)` table, client numbering 0..=4, so the
     playback dropdown and any other surface read the same table.
   - `event_label(MacroEvent) -> String` — one place that turns a `MacroEvent` into
     "Down A", "Up Left Ctrl", "Delay 120ms", "Wheel 255", "Button 1", "Tilt", "Extra Btn".
     Key names come from `macro_daemon::Keymap` (`load_embedded`), which is the same table
     the daemon replays with. Unknown usages render as `usage(page, code)`, mirroring the
     daemon's skip-unknown posture.
   - helpers: `macro_total_ms(events)`, `is_key_down/up`, `human_trigger(TriggerData)`.
3. **Views** render from `Workspace` and mutate it through `&mut self` methods
   (`select_profile`, `update_event`, `save_profile`, …) that keep invariants:
   selection stays valid after delete/rename, `dirty` flips on every mutation, save writes
   through `serde_json::to_string_pretty(&ProfileFile)` — the same struct the daemon reads.

No second parser anywhere: load is `serde_json::from_str::<ProfileFile>`, save is
`serde_json::to_string_pretty(&ProfileFile)`, both from the daemon's own types.

## Layout

```
┌────────────────────────────────────────────────────────────────┐
│ toolbar: New macro · Duplicate · Delete · Record(skip)   Save · Reload │
├──────────────┬─────────────────────────────────┬───────────────┤
│ sidebar      │ timeline                        │ properties    │
│ profiles     │ horizontal strip of event blocks│ trigger       │
│  └ macros    │ (press/release/delay), delays   │ playback      │
│              │ editable inline; click to select│ num_repeat    │
│              │ drag block edges to resize delay│ repeat_delay  │
│              │ +/- buttons nudge delay ±10ms   │ per-app rules │
├──────────────┴─────────────────────────────────┴───────────────┤
│ status bar: saved / errors / dirty marker                      │
└────────────────────────────────────────────────────────────────┘
```

- Left sidebar: profiles list, then the open profile's macros with name + trigger key.
  Click selects. Per profile: a small badge when it has per-app rules.
- Center: event timeline. Blocks sized proportionally to duration (delay events show ms),
  press/release pairs visually joined. Click selects an event (properties panel follows).
  Selected delay shows an inline edit row with nudge buttons and a text field.
- Right: properties panel for the selected macro: trigger binding (key name + direction),
  playback type dropdown (client numbering labels), num_repeat / repeat_delay (enabled only
  when the chosen playback type uses them, per `uses_num_repeat`/`uses_repeat_delay`),
  direction toggle for playback, and the profile's per-app rules list (add/remove path).

## Round-trip discipline

Every save: `ProfileFile` → JSON text → write file. Then the test-suite equivalent of the
daemon's own loader proves semantic equality: the shipped `--validate` path (`ProfileFile::validate`)
runs over every profile we write, and a round-trip test loads, saves, reloads, and compares.

## Motion (per animate skill)

- Hover/selection in sidebar and timeline: background/underline change, 100–150ms,
  strong ease-out. Purpose: feedback. Nothing that moves layout.
- Save success: status text appears with a short opacity transition, 150ms.
- Panel transitions: none. This is a pro tool used hundreds of times a day; per the
  animate skill's frequency gate, keyboard-frequency actions get no animation, and list
  re-selections stay instant. Only color/opacity transitions ship.
- Reduced motion: color-only transitions are already the gentlest tier; nothing moves.

## CLI

- `macro-studio [DIR]` — open DIR as a profiles directory (default `./profiles`).
- `macro-studio --validate [DIR]` — headless check: every `*.json` parses, validates
  (`ProfileFile::validate`), and round-trips semantically. Exit 0/1. This is what CI and
  the daemon's SIGHUP-equivalent confidence check use.

## Refresh semantics

The daemon reloads profiles in-process on SIGHUP. The app's Reload button re-reads the
directory from disk (`Workspace::load_dir`), discarding in-memory edits, and labels itself
"Reload (discards unsaved edits)" in a tooltip. There is no IPC to the daemon in scope.
