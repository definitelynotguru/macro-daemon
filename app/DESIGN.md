# macro-studio design notes

## Why this exists

macro-daemon today only serves people who edit `profiles/*.json` by hand. The goal is a real
desktop macro editor, in the spirit of SteelSeries GG's macro editor, built with Rune, that
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

The window title is `Macro Editor`. The view is one function of `Workspace`, not a second model.

- Left rail, titled MACROS. Rows are a radio and the macro name, flattened across profile files and sorted by name. The selected row is orange with a filled dot. Selecting a row sets `open_profile` and `selected_macro`.
- Center. The macro name, then one horizontal chip strip: key down, delay (`N ms`), key up. The rest of the canvas is empty.
- Bottom bar. Gear prints the profiles directory. CLEAR empties the selected macro's events. START plays that macro through `MockInjector` and reports injected count and planned milliseconds. Record does not write events. REVERT reloads the directory when the workspace is dirty. SAVE writes the open profile after `ProfileFile::validate`.

## Round-trip discipline

Every save: `ProfileFile` → JSON text → write file. Then the test-suite equivalent of the
daemon's own loader proves semantic equality: the shipped `--validate` path (`ProfileFile::validate`)
runs over every profile we write, and a round-trip test loads, saves, reloads, and compares.

## Motion

Selection fill uses Rune's `motion::SNAPPY`. Purpose is state indication on a control used tens of times a day, so the motion is the token spring and nothing else. Chips, buttons, and playback do not animate. Rune snaps spatial travel under reduced motion. The first frame does not animate.

## CLI

- `macro-studio [DIR]` — open DIR as a profiles directory (default `./profiles`).
- `macro-studio --validate [DIR]` — headless check: every `*.json` parses, validates
  (`ProfileFile::validate`), and round-trips semantically. Exit 0/1. This is what CI and
  the daemon's SIGHUP-equivalent confidence check use.

## Refresh semantics

The daemon reloads profiles in-process on SIGHUP. REVERT calls `Workspace::reload`, which re-reads the directory and drops unsaved edits. There is no IPC to the daemon.
