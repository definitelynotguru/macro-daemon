#!/usr/bin/env bash
# Boots macro-studio under Xvfb, forces a repaint via a window resize, and
# captures screenshots of the running app into app/screenshots/.
set -euo pipefail
SCRIPT="$(realpath "$0")"
APPDIR="$(dirname "$SCRIPT")"
REPO="$(dirname "$APPDIR")"
OUT="$APPDIR/screenshots"
mkdir -p "$OUT"
LOG="$OUT/boot.log"
: > "$LOG"

Xvfb :99 -screen 0 1600x1000x24 >/dev/null 2>&1 &
XVFB_PID=$!
trap 'kill $XVFB_PID 2>/dev/null || true' EXIT
sleep 1
export DISPLAY=:99

"$REPO/target/debug/macro-studio" "$REPO/profiles" >"$LOG" 2>&1 &
APP_PID=$!
sleep 5

# Xvfb does not composite the first frame until something forces a repaint.
wmctrl -r "macro-studio" -e 0,0,0,1440,900 2>>"$LOG" || true
sleep 1
import -window root "$OUT/main-window.png"

wmctrl -r "macro-studio" -e 0,0,0,1441,900 2>>"$LOG" || true
sleep 0.6
import -window root "$OUT/main-window-2.png"

kill $APP_PID 2>/dev/null || true
wait $APP_PID 2>/dev/null || true

if [ -s "$LOG" ]; then
  echo "APP WROTE OUTPUT (potential panic):"
  cat "$LOG"
  exit 1
fi
for f in "$OUT"/main-window*.png; do
  python3 - "$f" <<'EOF'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
colors = im.getcolors(maxcolors=10_000_000)
non_bg = sum(c for c, (r, g, b) in colors if abs(r-20)+abs(g-22)+abs(b-26) > 30)
print(f"{sys.argv[1]}: {im.size}, non-background px = {non_bg}")
assert non_bg > 50_000, f"{sys.argv[1]} looks blank"
EOF
done
echo "screenshots OK"
