#!/bin/bash
# Re-records the demo in an isolated directory, then adds the camera moves:
# docs/demo/demo.gif and demo.mp4. Needs Xcode, vhs, ffmpeg and python3.
# Set AGB_DEMO_SKIP_RECORD=1 to only redo the camera moves from the last raw recording
# (/tmp/agb-demo/raw.mp4).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
demo=/tmp/agb-demo
if [[ -z ${AGB_DEMO_SKIP_RECORD:-} ]]; then
  # Record with this checkout's agb, not whatever agb is installed.
  (cd "$here/../.." && cargo build --release --quiet)
  export AGB_BIN_DIR="$here/../../target/release"
  "$here/setup.sh" "$demo"
  # Background around the terminal window, in the logo's colors.
  ffmpeg -loglevel error -y -f lavfi \
    -i "gradients=s=2880x1620:c0=0x2a2f8f:c1=0x7b3fd1:x0=0:y0=0:x1=2880:y1=1620:nb_colors=2" \
    -frames:v 1 "$demo/bg.png"
  (cd "$here/../.." && vhs docs/demo/demo.tape)
fi
python3 "$here/zoom.py" "$demo/raw.mp4" "$here"
