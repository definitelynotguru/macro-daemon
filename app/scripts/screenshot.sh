#!/usr/bin/env bash
# Boots macro-studio under an Xvfb display and captures the main window into
# app/screenshots/. On this image the desktop environment already runs on
# display :9 (Xvfb + xfwm4), so the script reuses it when present and boots
# its own otherwise.
set -euo pipefail
SCRIPT="$(realpath "$0")"
APPDIR="$(dirname "$(dirname "$SCRIPT")")"
REPO="$(dirname "$APPDIR")"
OUT="$APPDIR/screenshots"
mkdir -p "$OUT"
LOG="$OUT/boot.log"
: > "$LOG"

if xdpyinfo -display :9 >/dev/null 2>&1; then
  export DISPLAY=:9
  OWN_XVFB=""
else
  Xvfb :99 -screen 0 1600x1000x24 >/dev/null 2>&1 &
  XVFB_PID=$!
  export DISPLAY=:99
  OWN_XVFB="$XVFB_PID"
fi
trap '[ -n "$OWN_XVFB" ] && kill "$OWN_XVFB" 2>/dev/null || true' EXIT

"$REPO/target/debug/macro-studio" "$REPO/profiles" >"$LOG" 2>&1 &
APP_PID=$!
sleep 6

WID="$(xdotool search --name "macro-studio" | head -1 || true)"
if [ -n "$WID" ]; then
  wmctrl -i -r "$WID" -e 0,10,58,1440,900 2>>"$LOG" || true
  sleep 1
  import -window "$WID" "$OUT/main-window.png"
fi

kill "$APP_PID" 2>/dev/null || true
wait "$APP_PID" 2>/dev/null || true

if [ -s "$LOG" ]; then
  echo "APP WROTE OUTPUT (potential panic):"
  cat "$LOG"
  exit 1
fi
if [ ! -f "$OUT/main-window.png" ]; then
  echo "no window captured"
  exit 1
fi
python3 - "$OUT/main-window.png" <<'EOF'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
colors = im.getcolors(maxcolors=10_000_000)
non_bg = sum(c for c, col in colors if col != (20, 22, 26))
print(f"{sys.argv[1]}: {im.size}, non-BG px = {non_bg}")
assert non_bg > 100_000, f"{sys.argv[1]} looks blank"
EOF
echo "screenshot OK"
