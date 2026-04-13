#!/usr/bin/env python3
"""Generate dock icon PNGs, .icns, and .ico from the dock SVG.

Uses a green chroma-key background to preserve the SVG's rounded corners
as transparency, then adds ~12% padding to match macOS dock icon sizing.

Usage:
    python3 docs/scripts/generate-dock-icon.py

Requirements:
    - macOS (uses qlmanage + iconutil)
    - Pillow: pip install Pillow
    - numpy: pip install numpy

Input:  ../datalake/docs/sery.ai.mac-dock-icon.svg  (relative to repo root)
Output: src-tauri/icons/  (32x32, 64x64, 128x128, 128x128@2x, icon.png,
        icon.ico, icon.icns, plus Windows Store logos)
"""

from PIL import Image
import numpy as np
import subprocess
import os
import tempfile
import shutil

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SVG_PATH = os.path.join(REPO_ROOT, "..", "datalake", "docs", "sery.ai.mac-dock-icon.svg")
ICONS_DIR = os.path.join(REPO_ROOT, "src-tauri", "icons")

PADDING_PCT = 0.12  # 12% padding on each side


def main():
    if not os.path.exists(SVG_PATH):
        print(f"ERROR: SVG not found at {SVG_PATH}")
        return

    # Read SVG and inject a green chroma-key background behind the rounded rect
    with open(SVG_PATH) as f:
        svg = f.read()

    green_bg = '<rect width="200" height="200" fill="#00FF00"/>'
    svg_with_key = svg.replace(
        '<rect width="200" height="200" rx="40" fill="white"/>',
        green_bg + "\n" + '<rect width="200" height="200" rx="40" fill="white"/>',
    )

    tmp_dir = tempfile.mkdtemp()
    try:
        # Write temp SVG and render at high resolution
        tmp_svg = os.path.join(tmp_dir, "dock.svg")
        with open(tmp_svg, "w") as f:
            f.write(svg_with_key)

        subprocess.run(
            ["qlmanage", "-t", "-s", "2048", "-o", tmp_dir, tmp_svg],
            capture_output=True,
        )

        rendered = next(
            (os.path.join(tmp_dir, fn) for fn in os.listdir(tmp_dir) if fn.endswith(".png")),
            None,
        )
        if not rendered:
            print("ERROR: qlmanage did not produce a PNG")
            return

        source = Image.open(rendered).convert("RGBA")
        data = np.array(source)
        r, g, b = data[:, :, 0], data[:, :, 1], data[:, :, 2]

        # Find the SVG render area (green + non-white content)
        is_green = (r < 30) & (g > 220) & (b < 30)
        is_content = ~is_green & ((r < 245) | (g < 245) | (b < 245))
        has_content = is_green | is_content

        rows = np.any(has_content, axis=1)
        cols = np.any(has_content, axis=0)
        rmin, rmax = np.where(rows)[0][[0, -1]]
        cmin, cmax = np.where(cols)[0][[0, -1]]

        # Crop to content area
        cropped = source.crop((cmin, rmin, cmax + 1, rmax + 1))
        crop_data = np.array(cropped)

        # Replace green chroma-key pixels with transparency
        cr, cg, cb = crop_data[:, :, 0], crop_data[:, :, 1], crop_data[:, :, 2]
        green_mask = (cr < 30) & (cg > 220) & (cb < 30)
        crop_data[green_mask] = [0, 0, 0, 0]

        result = Image.fromarray(crop_data, "RGBA")

        # Make square
        w, h = result.size
        side = max(w, h)
        square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
        square.paste(result, ((side - w) // 2, (side - h) // 2))

        # Add padding
        padded_size = int(side / (1 - 2 * PADDING_PCT))
        padded = Image.new("RGBA", (padded_size, padded_size), (0, 0, 0, 0))
        offset = (padded_size - side) // 2
        padded.paste(square, (offset, offset))

        print(f"Icon: {padded_size}x{padded_size} (content {side}x{side}, {PADDING_PCT*100:.0f}% padding)")

        # --- Save bundle PNGs ---
        for size, name in [
            (32, "32x32.png"),
            (64, "64x64.png"),
            (128, "128x128.png"),
            (256, "128x128@2x.png"),
            (512, "icon.png"),
        ]:
            padded.resize((size, size), Image.LANCZOS).save(os.path.join(ICONS_DIR, name))
            print(f"  {name}")

        # --- Windows Store logos ---
        for size, name in [
            (30, "Square30x30Logo.png"),
            (44, "Square44x44Logo.png"),
            (50, "StoreLogo.png"),
            (71, "Square71x71Logo.png"),
            (89, "Square89x89Logo.png"),
            (107, "Square107x107Logo.png"),
            (142, "Square142x142Logo.png"),
            (150, "Square150x150Logo.png"),
            (284, "Square284x284Logo.png"),
            (310, "Square310x310Logo.png"),
        ]:
            padded.resize((size, size), Image.LANCZOS).save(os.path.join(ICONS_DIR, name))

        # --- .ico ---
        ico_sizes = [(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
        ico_images = [padded.resize(s, Image.LANCZOS) for s in ico_sizes]
        ico_images[0].save(os.path.join(ICONS_DIR, "icon.ico"), format="ICO", sizes=ico_sizes)
        print("  icon.ico")

        # --- .icns ---
        iconset_dir = os.path.join(tmp_dir, "icon.iconset")
        os.makedirs(iconset_dir, exist_ok=True)
        for name, size in {
            "icon_16x16.png": 16,
            "icon_16x16@2x.png": 32,
            "icon_32x32.png": 32,
            "icon_32x32@2x.png": 64,
            "icon_128x128.png": 128,
            "icon_128x128@2x.png": 256,
            "icon_256x256.png": 256,
            "icon_256x256@2x.png": 512,
            "icon_512x512.png": 512,
            "icon_512x512@2x.png": 1024,
        }.items():
            padded.resize((size, size), Image.LANCZOS).save(os.path.join(iconset_dir, name))

        res = subprocess.run(
            ["iconutil", "-c", "icns", iconset_dir, "-o", os.path.join(ICONS_DIR, "icon.icns")],
            capture_output=True,
            text=True,
        )
        if res.returncode == 0:
            print(f"  icon.icns ({os.path.getsize(os.path.join(ICONS_DIR, 'icon.icns'))} bytes)")
        else:
            print(f"  iconutil failed: {res.stderr}")

    finally:
        shutil.rmtree(tmp_dir)

    print("Done!")


if __name__ == "__main__":
    main()
