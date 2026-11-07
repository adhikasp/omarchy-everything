#!/usr/bin/env python3
"""Compose demo card grabs onto a backdrop: README stills and demo.gif."""

import json
import os
import subprocess
import sys
import tempfile

frames_dir, manifest, out = sys.argv[1:4]
frames = [json.loads(line) for line in open(manifest, encoding="utf-8") if line.strip()]
# Trust the pixels, not the logged size: grabToImage scales by the output's DPR.
for frame in frames:
    size = subprocess.run(["magick", "identify", "-format", "%w %h",
                           os.path.join(frames_dir, frame["file"])],
                          check=True, capture_output=True, text=True).stdout.split()
    frame["width"], frame["height"] = int(size[0]), int(size[1])
MARGIN_X, MARGIN_TOP, MARGIN_BOTTOM = 150, 110, 150
GIF_WIDTH, STILL_WIDTH = 960, 1400


def run(*args):
    subprocess.run(args, check=True)


def backdrop(path, width, height):
    # A quiet warm gradient; no wallpaper, so nothing competes with the card.
    run("magick", "-size", f"{width}x{height}", "radial-gradient:#f7f3e8-#e4ddcb",
        "-gravity", "center", path)


def compose(frame, canvas_height, bg_path, dest):
    card = os.path.join(frames_dir, frame["file"])
    run("magick", bg_path,
        "(", card, "-repage", f"+{MARGIN_X}+{MARGIN_TOP}",
        "(", "+clone", "-background", "black", "-shadow", "28x34+0+22", ")",
        "+swap", ")",
        "-background", "none", "-layers", "flatten",
        "-crop", f"{frame['width'] + 2 * MARGIN_X}x{canvas_height}+0+0", "+repage", dest)


os.makedirs(out, exist_ok=True)
width = max(f["width"] for f in frames) + 2 * MARGIN_X
gif_frames = [f for f in frames if not f["stillOnly"]]
gif_height = max(f["height"] for f in gif_frames) + MARGIN_TOP + MARGIN_BOTTOM

with tempfile.TemporaryDirectory() as work:
    tall = max(f["height"] for f in frames) + MARGIN_TOP + MARGIN_BOTTOM
    bg = os.path.join(work, "bg.png")
    backdrop(bg, width, tall)

    for frame in frames:
        if not frame["still"]:
            continue
        dest = os.path.join(work, "still.png")
        compose(frame, frame["height"] + MARGIN_TOP + MARGIN_BOTTOM, bg, dest)
        # Lossless WebP: pixel-identical to PNG at about a sixth of the size.
        run("magick", dest, "-resize", f"{STILL_WIDTH}x", "-strip",
            "-define", "webp:lossless=true", "-define", "webp:method=6",
            os.path.join(out, frame["still"] + ".webp"))

    concat = os.path.join(work, "concat.txt")
    with open(concat, "w", encoding="utf-8") as listing:
        for index, frame in enumerate(gif_frames):
            dest = os.path.join(work, f"g{index:04d}.png")
            compose(frame, gif_height, bg, dest)
            listing.write(f"file '{dest}'\nduration {max(frame['duration'], 40) / 1000:.3f}\n")
        listing.write(f"file '{dest}'\n")
    palette = os.path.join(work, "palette.png")
    scale = f"scale={GIF_WIDTH}:-1:flags=lanczos"
    run("ffmpeg", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", concat,
        "-vf", f"{scale},palettegen=stats_mode=diff", palette)
    run("ffmpeg", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", concat,
        "-i", palette, "-lavfi", f"{scale}[x];[x][1:v]paletteuse=dither=sierra2_4a:diff_mode=rectangle",
        "-fps_mode", "vfr", os.path.join(out, "demo.gif"))

print("wrote", out)
