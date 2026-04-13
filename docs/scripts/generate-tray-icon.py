#!/usr/bin/env python3
"""Generate macOS menu bar (tray) icons from the title-bar SVG.

Renders the white-on-transparent SVG to PNG by:
  1. Swapping fill="white" to fill="black"
  2. Rendering with qlmanage (which adds a white background)
  3. Thresholding to extract the logo shape as black-on-transparent

macOS template images use black pixels for content; the system automatically
handles light/dark mode coloring. Set .icon_as_template(true) in Tauri.

Usage:
    python3 docs/scripts/generate-tray-icon.py

Requirements:
    - macOS (uses qlmanage)
    - Pillow: pip install Pillow
    - numpy: pip install numpy

Input:  ../datalake/docs/sery.ai.mac-title-bar-icon.svg
Output: src-tauri/icons/tray-22x22.png, tray-44x44.png, tray-64x64.png
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

# macOS menu bar: 22pt standard, 44px @2x, 64px for non-mac fallback
TRAY_SIZES = [(22, "tray-22x22.png"), (44, "tray-44x44.png"), (64, "tray-64x64.png")]


def main():
    if not os.path.exists(SVG_PATH):
        print(f"ERROR: SVG not found at {SVG_PATH}")
        return

    # Read SVG and swap white fill to black (so qlmanage's white bg gives contrast)
    with open(SVG_PATH) as f:
        svg = f.read()

    svg_black = svg.replace('fill="white"', 'fill="black"')

    tmp_dir = tempfile.mkdtemp()
    try:
        tmp_svg = os.path.join(tmp_dir, "tray.svg")
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

        # Threshold: dark pixels (logo) become black-on-alpha,
        # light pixels (background) become fully transparent
        r, g, b = data[:, :, 0], data[:, :, 1], data[:, :, 2]
        brightness = r.astype(int) + g.astype(int) + b.astype(int)

        out = np.zeros_like(data)
        mask = brightness < 384  # logo pixels
        out[mask] = [0, 0, 0, 255]  # black, fully opaque (macOS template convention)
        out[~mask] = [0, 0, 0, 0]  # fully transparent

        result = Image.fromarray(out, "RGBA")

        # Crop to content
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

        # Save at each tray size
        for s, name in TRAY_SIZES:
            out_path = os.path.join(ICONS_DIR, name)
            square.resize((s, s), Image.LANCZOS).save(out_path)
            print(f"  {name} ({s}x{s})")

    finally:
        shutil.rmtree(tmp_dir)

    print("Done!")


if __name__ == "__main__":
    main()
