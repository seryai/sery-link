#!/usr/bin/env python3
"""Generate title-bar / window icon PNGs from the title-bar SVG.

Same rendering technique as the tray icon script, but outputs white-on-
transparent (used by Tauri's window.set_icon() for the Cmd+Tab switcher
and window proxy icon on non-macOS platforms).

Usage:
    python3 docs/scripts/generate-titlebar-icon.py

Requirements:
    - macOS (uses qlmanage)
    - Pillow: pip install Pillow
    - numpy: pip install numpy

Input:  ../datalake/docs/sery.ai.mac-title-bar-icon.svg
Output: src-tauri/icons/titlebar-{16,32,64,128}x{16,32,64,128}.png
"""

from PIL import Image
import numpy as np
import subprocess
import os
import tempfile
import shutil

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SVG_PATH = os.path.join(REPO_ROOT, "..", "datalake", "docs", "sery.ai.mac-title-bar-icon.svg")
ICONS_DIR = os.path.join(REPO_ROOT, "src-tauri", "icons")

SIZES = [16, 32, 64, 128]


def main():
    if not os.path.exists(SVG_PATH):
        print(f"ERROR: SVG not found at {SVG_PATH}")
        return

    # Swap white fill to black for qlmanage rendering
    with open(SVG_PATH) as f:
        svg = f.read()

    svg_black = svg.replace('fill="white"', 'fill="black"')

    tmp_dir = tempfile.mkdtemp()
    try:
        tmp_svg = os.path.join(tmp_dir, "titlebar.svg")
        with open(tmp_svg, "w") as f:
            f.write(svg_black)

        subprocess.run(
            ["qlmanage", "-t", "-s", "1024", "-o", tmp_dir, tmp_svg],
            capture_output=True,
        )

        rendered = next(
            (os.path.join(tmp_dir, fn) for fn in os.listdir(tmp_dir) if fn.endswith(".png")),
            None,
        )
        if not rendered:
            print("ERROR: qlmanage did not produce a PNG")
            return

        img = Image.open(rendered).convert("RGBA")
        data = np.array(img)

        # Threshold: dark pixels become WHITE on alpha (not black like tray)
        r, g, b = data[:, :, 0], data[:, :, 1], data[:, :, 2]
        brightness = r.astype(int) + g.astype(int) + b.astype(int)

        out = np.zeros_like(data)
        mask = brightness < 384
        out[mask] = [255, 255, 255, 255]  # white, fully opaque
        out[~mask] = [0, 0, 0, 0]  # fully transparent

        result = Image.fromarray(out, "RGBA")

        # Crop to content + small padding
        bbox = result.getbbox()
        if bbox:
            result = result.crop(bbox)
            pad = max(4, result.width // 20)
            padded = Image.new("RGBA", (result.width + 2 * pad, result.height + 2 * pad), (0, 0, 0, 0))
            padded.paste(result, (pad, pad))
            result = padded

        # Make square
        w, h = result.size
        size = max(w, h)
        square = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        square.paste(result, ((size - w) // 2, (size - h) // 2))

        for s in SIZES:
            name = f"titlebar-{s}x{s}.png"
            out_path = os.path.join(ICONS_DIR, name)
            square.resize((s, s), Image.LANCZOS).save(out_path)
            print(f"  {name} ({s}x{s})")

    finally:
        shutil.rmtree(tmp_dir)

    print("Done!")


if __name__ == "__main__":
    main()
