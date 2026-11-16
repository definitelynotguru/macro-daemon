#!/usr/bin/env bash
# Headless PNG of the Macro Editor. No display and no Xvfb: Rune --snap renders
# offscreen. A blank file is a failed run.
set -euo pipefail
SCRIPT="$(realpath "$0")"
APPDIR="$(dirname "$(dirname "$SCRIPT")")"
REPO="$(dirname "$APPDIR")"
OUT="${1:-$APPDIR/screenshots/main-window.png}"
BIN="${MACRO_STUDIO:-$REPO/target/release/macro-studio}"
mkdir -p "$(dirname "$OUT")"
export VK_ICD_FILENAMES="${VK_ICD_FILENAMES:-/usr/share/vulkan/icd.d/lvp_icd.json}"
"$BIN" --snap "$OUT" --scene delay --size 1080x640 --scale 2 "$REPO/profiles"
python3 - "$OUT" << 'PY'
import sys
from pathlib import Path
path = Path(sys.argv[1])
data = path.read_bytes()
if len(data) < 8_000 or not data.startswith(b"\x89PNG"):
    raise SystemExit(f"refusing a blank or tiny PNG: {path} {len(data)} bytes")
print(f"wrote {path} ({len(data)} bytes)")
PY
