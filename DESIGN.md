# Design notes

Three decisions shaped this crate. They are recorded here because each one looks like an omission
unless you know why it was chosen.

## 1. Reimplementing semantics, not the runtime

The recovered engine does not type on your behalf. `PlayMacro` is a device-support call that ends
in `deployMacros(handler, …, 0x10)` (`MACRO_SPEC.md` §B.2), and a scan of the whole install finds
no `SendInput`, `keybd_event` or `mouse_event` import in any binary (§B.5). What it ships is a
macro *format* plus a device that executes it.

Cloning that runtime would mean cloning a firmware interface we do not have hardware for. So the
daemon reconstructs the parts that are actually portable — the event schema, the pacing model
(timestamp offsets, `type:4` delays), the repeat table, the HID usage-id key space, the binding
and per-app profile model — and swaps the execution target for `uinput`. Every keyboard becomes an
injection target; that is the point of the tool, not a shortcut around it.

Consequence: `MACRO_SPEC.md` §B.3 (the onboard blob: magic `0x77`, command 113, 5120-byte pool) is
documented in the spec and deliberately not implemented. It is the one part of the original that
only makes sense when talking to their firmware.

## 2. `keymap.json` is the portability layer, and unknown usages are skipped

Macro events carry raw USB HID usage ids with a usage page, not Windows virtual-key codes
(§C.1). That is unusually lucky: HID usage ids are the same physical description the Linux input
layer starts from, so the whole translation is one lookup table and needs no layout model.

The table is embedded at compile time (`assets/keymap.json`, 112 keyboard-page entries with 22
regional label sets). Everything else in the extraction tree — the binaries, the migration SQL,
the beautified bundles — is evidence, not a runtime dependency.

Where a usage has no evdev equivalent, the daemon logs once and skips the event (§C.3). A guessed
keycode produces silent wrong input; a skipped event produces a warning. The second failure is
recoverable, the first is not.

## 3. Two playback numberings, one enum, no raw `u8`

`playback_options_data.type` on disk is the client numbering `0..=4`. An onboard macro header
carries the wire numbering `1..=6` (§A.4). They differ by one and the two tables are printed side
by side in the spec precisely because mixing them is the obvious silent bug: `type: 1` means
*play once* in one table and *repeat N times* in the other.

So `PlaybackType` is one `#[repr(u8)]` enum with explicit wire discriminants, and four functions:
`from_client`, `to_client`, `from_wire`, `to_wire`. There is no `impl From<u8>`, no `as u8` on the
execution path, and unknown values return `Err` rather than falling back to a default. Configs are
deserialized through `from_client`; anything that ever needs to speak firmware goes through
`to_wire()`. `--validate` prints both numbers for every binding so a mismatch is visible without
reading code.

## Smaller decisions worth knowing

**No jitter by default.** The recovered format has no jitter, randomisation or min/max field
anywhere (§A.4): `delay` is a plain millisecond count. `jitter_ms` exists because a user may want
it, but enabling it makes replay *less* faithful, so it ships at 0 and the README says so.

**The daemon excludes its own virtual device from trigger discovery.** Its uinput device declares
every keycode, so a macro that emits a bound key would otherwise feed its own output back in as a
trigger. This was found by running it, not by reading the spec: the double-tap-F5 macro is bound to
a key it also emits.

**Focus detection shells out.** The scope allowed no new crates, and `wlr-foreign-toplevel` would
drag in a Wayland client. `hyprctl` / `swaymsg` / `xdotool` / `xprop` are stable, documented CLIs;
a missing one is logged once and the daemon stays on its current profile. The trigger loop never
blocks on focus, which is the failure mode that actually hurts.

**One `EV_SYN` per logical step.** Injection is batched per key/button/notch transition rather
than per macro, so a consumer sees discrete steps and partial macros cannot leave a modifier stuck
in a half-applied state.
