#!/usr/bin/env python3
"""Time process spawn to first frame for the Rune editor and, if given, the GPUI binary.

Rune: wall clock from Popen to the flushed `rune-first-pixels` line. That line is printed
after the first offscreen GPU readback and before PNG encode. The JSON on the line is
Rune's own clock from inside the process, recorded beside the external clock.

GPUI has no first-pixel marker. With --gpui, this script starts Xvfb, spawns the binary,
and times until `xwininfo -root -tree` shows a window whose name contains macro-studio.
That is window map, not first pixels. Do not compare the two columns as the same boundary.
"""

import argparse
import os
import shutil
import signal
import subprocess
import time


def median(values):
    ordered = sorted(values)
    mid = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[mid]
    return (ordered[mid - 1] + ordered[mid]) / 2


def time_rune(binary, profiles, samples):
    rows = []
    for i in range(samples):
        cmd = [
            binary,
            "--first-pixels",
            "--snap",
            os.devnull,
            "--size",
            "1080x640",
            "--scale",
            "1",
            profiles,
        ]
        env = os.environ.copy()
        env["VK_ICD_FILENAMES"] = "/usr/share/vulkan/icd.d/lvp_icd.json"
        start = time.perf_counter()
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            env=env,
        )
        marker = None
        assert proc.stdout is not None
        for line in proc.stdout:
            if line.startswith("rune-first-pixels"):
                marker = line.strip()
                elapsed = time.perf_counter() - start
                break
        else:
            elapsed = time.perf_counter() - start
        proc.send_signal(signal.SIGKILL)
        proc.wait(timeout=10)
        if marker is None:
            raise SystemExit(f"rune sample {i} produced no rune-first-pixels line in {elapsed:.3f}s")
        rows.append((elapsed * 1000.0, marker))
        print(f"rune {i} {rows[-1][0]:.1f} ms {marker}")
    return rows


_display = 40


def time_gpui(binary, profiles, samples):
    global _display
    if shutil.which("Xvfb") is None or shutil.which("xwininfo") is None:
        raise SystemExit("Xvfb and xwininfo are required for the GPUI window-map timing")
    rows = []
    for i in range(samples):
        display = f":{_display}"
        _display += 1
        xvfb = subprocess.Popen(
            ["Xvfb", display, "-screen", "0", "1280x800x24"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        time.sleep(0.4)
        env = os.environ.copy()
        env["DISPLAY"] = display
        env["VK_ICD_FILENAMES"] = "/usr/share/vulkan/icd.d/lvp_icd.json"
        start = time.perf_counter()
        proc = subprocess.Popen(
            [binary, profiles],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        mapped = None
        deadline = start + 30
        while time.perf_counter() < deadline:
            if proc.poll() is not None:
                err = proc.stderr.read() if proc.stderr else ""
                xvfb.kill()
                raise SystemExit(f"gpui sample {i} exited {proc.returncode} before a window: {err[:500]}")
            probe = subprocess.run(
                ["xwininfo", "-root", "-tree", "-display", display],
                capture_output=True,
                text=True,
            )
            if "macro-studio" in probe.stdout.lower() or "macro editor" in probe.stdout.lower():
                mapped = (time.perf_counter() - start) * 1000.0
                break
            time.sleep(0.02)
        proc.send_signal(signal.SIGKILL)
        proc.wait(timeout=10)
        xvfb.kill()
        xvfb.wait(timeout=5)
        if mapped is None:
            raise SystemExit(f"gpui sample {i} never mapped a window")
        rows.append(mapped)
        print(f"gpui {i} {mapped:.1f} ms window-map")
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rune", required=True)
    parser.add_argument("--gpui")
    parser.add_argument("--profiles", required=True)
    parser.add_argument("--samples", type=int, default=5)
    args = parser.parse_args()
    if args.gpui:
        print("warmup")
        time_rune(args.rune, args.profiles, 1)
        time_gpui(args.gpui, args.profiles, 1)
        rune_rows = []
        gpui = []
        for i in range(args.samples):
            rune_rows.extend(time_rune(args.rune, args.profiles, 1))
            gpui.extend(time_gpui(args.gpui, args.profiles, 1))
        times = [row[0] for row in rune_rows]
        print(
            f"rune spawn-to-first-pixels ms min={min(times):.1f} median={median(times):.1f} max={max(times):.1f} n={len(times)}"
        )
        print(
            f"gpui spawn-to-window-map ms min={min(gpui):.1f} median={median(gpui):.1f} max={max(gpui):.1f} n={len(gpui)}"
        )
        return
    rune_rows = time_rune(args.rune, args.profiles, args.samples)
    times = [row[0] for row in rune_rows]
    print(
        f"rune spawn-to-first-pixels ms min={min(times):.1f} median={median(times):.1f} max={max(times):.1f} n={len(times)}"
    )


if __name__ == "__main__":
    main()
