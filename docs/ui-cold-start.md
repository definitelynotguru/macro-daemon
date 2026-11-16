# UI cold start

Measured on this machine on 2026-11-16. Release binaries. 4 cores. Load average about 2.4 while the samples ran. One warmup per binary, then 5 samples, interleaved so both sides saw the same machine.

These two columns are not the same boundary. Do not subtract them and call the difference a speedup.

| Binary | Boundary | min | median | max | n |
| --- | --- | --- | --- | --- | --- |
| Rune `macro-studio` (`631e8a67`) | process spawn to the flushed `rune-first-pixels` line | 422.5 ms | 436.7 ms | 454.3 ms | 5 |
| GPUI `macro-studio` from `main` | process spawn to an X11 window whose name contains `macro-studio` | 117.4 ms | 142.8 ms | 150.3 ms | 5 |

The Rune warmup, discarded, was 445.0 ms. The GPUI warmup, discarded, was 756.1 ms. Later GPUI samples are faster because the driver cache is warm. The table is warm-cache process starts, not a reboot.

## What the Rune number is

`--first-pixels` prints after the first offscreen GPU readback and before PNG encode. The clock is wall time from `Popen` to that line. Every reported sample printed `pixel_bytes: 2764800`, which is 1080×640×4. The frame was read back. No sample exited early.

An extra run, not in the table, spent 0.44 s user and 0.06 s system in 0.46 s wall time (107% CPU). The marker on a reported sample splits the process-internal clock into about 198 ms of GPU init, 201 ms of frame CPU, and 31 to 47 ms of readback. App state setup is 1.6 ms. The limiter is lavapipe software Vulkan on one core, not the editor. A hardware GPU would move this number. This machine has no GPU and no `/dev/dri`.

## What the GPUI number is not

The GPUI binary on `main` has no first-pixel marker. The number is the time until `xwininfo` sees the window. It is not a pixel readback and not a presented frame.

An xwd of that window, tried separately, stayed uniform in the window body for about 2 seconds. The GPU surface is not the X pixmap, so this box cannot time GPUI first pixels. The 756 ms warmup is the cold window-map. It is still not first pixels.

## What neither number includes

Neither number is time to a physical display. Neither includes PNG encode. Neither is device-level input injection. START in the editor plays through `MockInjector`.

## Rerun

```bash
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
  python3 app/scripts/cold-start.py \
  --rune target/release/macro-studio \
  --gpui /path/to/main/macro-studio \
  --profiles "$PWD/profiles" \
  --samples 5
```

The GPUI path must be a release build of `main` before this branch. This script does not build it.
