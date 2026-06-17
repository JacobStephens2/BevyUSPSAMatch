#!/usr/bin/env python3
"""Generate the Android launcher icon: a USPSA cardboard target with A/C/D
zones and a couple of bullet holes, on a range-tan background. Renders a base
image then downsamples into the mipmap densities."""
import os
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
BASE = 432
BG = (44, 54, 40, 255)        # range green
TAN_D = (204, 171, 117, 255)  # D zone / body
TAN_C = (178, 140, 90, 255)   # C zone
TAN_A = (152, 115, 71, 255)   # A zone
HOLE = (16, 16, 16, 255)


def rounded(d, cx, cy, hw, hh, fill, r=16):
    d.rounded_rectangle([cx - hw, cy - hh, cx + hw, cy + hh], radius=r, fill=fill)


def main():
    img = Image.new("RGBA", (BASE, BASE), BG)
    # soft centre glow
    glow = Image.new("RGBA", (BASE, BASE), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse([40, 20, BASE - 40, BASE - 60], fill=(70, 86, 64, 150))
    img.alpha_composite(glow)

    d = ImageDraw.Draw(img)
    cx, cy = BASE / 2, BASE / 2 + 6
    hw, hh = 118, 165
    rounded(d, cx, cy, hw, hh, TAN_D, r=26)
    rounded(d, cx, cy, hw * 0.7, hh * 0.7, TAN_C, r=18)
    rounded(d, cx, cy, hw * 0.34, hh * 0.34, TAN_A, r=10)

    # a couple of bullet holes in the A zone
    for (ox, oy) in [(-14, -18), (12, 6)]:
        d.ellipse([cx + ox - 9, cy + oy - 9, cx + ox + 9, cy + oy + 9], fill=HOLE)

    img.save(os.path.join(HERE, "icon-512.png"))
    for name, size in {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}.items():
        dd = os.path.join(HERE, "res", f"mipmap-{name}")
        os.makedirs(dd, exist_ok=True)
        img.resize((size, size), Image.LANCZOS).save(os.path.join(dd, "ic_launcher.png"))
    print("icons written")


if __name__ == "__main__":
    main()
