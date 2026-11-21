#!/usr/bin/env python3
"""Adds camera moves to the raw demo recording: docs/demo/zoom.py <raw.mp4> <out dir>.

Writes demo.mp4 (1920x1080) and demo.gif (1120x630, 15 fps, 128 colors, about 5 MB) to <out dir>. Only ffmpeg is needed.
SHOTS and CUTS are in seconds of the raw recording; if you change demo.tape, re-time them
by looking at the raw video (for example one frame per second:
ffmpeg -i raw.mp4 -vf "fps=1,scale=720:-1,tile=4x4" sheet%d.png).
"""
import math
import subprocess
import sys
import tempfile

# Raw frame is 2880x1620: window (80,80)-(2800,1540), title bar 60 px, text starts at x=114.
# Text rows are 33.4 px apart, the first centered at y=188; shots start between two rows.
# First build 188-388, second build 456-622, the mistake and its error 656-956.
FULL = (1440, 810, 1.0)
# 1.6x keeps about 106 columns in view, enough for the whole error line.
TOP = (960, 606, 1.6)         # the first build in "app": cold
REST = (960, 844, 1.6)        # both summary lines (build=5.3s cold, 1.5s from the new worktree) and the mistake
ERROR = (960, 1011, 1.6)      # the error with this worktree's path, and the failed summary

# (time, camera): the camera holds a shot until the next entry, then eases to it.
# Every move redraws whole GIF frames (about 1 MB per second of movement), so moves are few.
SHOTS = [
    (0.0, FULL), (0.4, FULL),
    (1.2, TOP), (10.6, TOP),          # the first build, cold
    (11.6, REST), (26.2, REST),       # the second worktree reuses the slot, then a mistake
    (27.0, ERROR),                    # the error, with this worktree's path
]
# Idle stretches cut from the output: (start, end).
CUTS = []
FPS = 25


def ease(a, b, t0, t1, log=False):
    """ffmpeg expression for a smooth move from a to b between t0 and t1 (variable T)."""
    s = f"(((T-{t0})/{t1 - t0}))"
    s = f"({s}*{s}*(3-2*{s}))"
    if log:
        return f"exp({math.log(a)}+({math.log(b) - math.log(a)})*{s})"
    return f"({a}+({b - a})*{s})"


def track(i, log=False):
    """Piecewise expression over SHOTS for camera component i."""
    expr = str(SHOTS[-1][1][i])
    for (t0, c0), (t1, c1) in reversed(list(zip(SHOTS, SHOTS[1:]))):
        seg = str(c0[i]) if c0[i] == c1[i] else ease(c0[i], c1[i], t0, t1, log)
        expr = f"if(lt(T,{t1}),{seg},{expr})"
    return expr.replace("T", f"(in/{FPS})")


def main():
    raw, out = sys.argv[1], sys.argv[2]
    keep = " * ".join(f"not(between(t,{a},{b}))" for a, b in CUTS) or "1"
    graph = (
        # zoompan crops at whole input pixels; at 2x the steps are too small to see as jitter.
        f"[0:v]fps={FPS},scale=iw*2:ih*2:flags=lanczos,"
        f"zoompan=z='{track(2, log=True)}'"
        f":x='clip(2*{track(0)}-iw/zoom/2,0,iw-iw/zoom)'"
        f":y='clip(2*{track(1)}-ih/zoom/2,0,ih-ih/zoom)'"
        f":d=1:fps={FPS}:s=1920x1080,"
        f"select='{keep}',setpts=N/{FPS}/TB,split[m][g];"
        # The GIF is made from the same frames, not from the encoded mp4: re-encoding noise would
        # make every held frame differ from the last and roughly double the GIF.
        "[g]fps=15,scale=1120:-1:flags=lanczos,split[a][b];"
        "[a]palettegen=max_colors=128:stats_mode=diff[p];"
        "[b][p]paletteuse=dither=none:diff_mode=rectangle[gif]"
    )
    with tempfile.NamedTemporaryFile("w", suffix=".txt") as script:
        script.write(graph)
        script.flush()
        subprocess.run(
            ["ffmpeg", "-loglevel", "error", "-y", "-i", raw, "-/filter_complex", script.name,
             "-map", "[m]", "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p",
             "-movflags", "+faststart", f"{out}/demo.mp4",
             "-map", "[gif]", f"{out}/demo.gif"],
            check=True,
        )

if __name__ == "__main__":
    main()
