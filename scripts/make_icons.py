#!/usr/bin/env python3
"""Generate the NEXORA app icon set from a single vector-ish drawing.

Produces the files referenced by src-tauri/tauri.conf.json:
  32x32.png, 128x128.png, 128x128@2x.png (256), icon.ico, icon.icns

Re-run any time the mark changes. Requires Pillow.
"""
from PIL import Image, ImageDraw
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
OUT.mkdir(parents=True, exist_ok=True)

S = 1024
INK_TOP = (26, 31, 38)     # #1a1f26
INK_BOT = (11, 13, 16)     # #0b0d10
AMBER = (224, 128, 58)     # #e0803a accent
AMBER_SOFT = (240, 163, 94)


def rounded_mask(size, radius):
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=255)
    return m


def vgradient(size, top, bot):
    g = Image.new("RGB", (1, size))
    for y in range(size):
        t = y / (size - 1)
        g.putpixel((0, y), tuple(int(top[i] + (bot[i] - top[i]) * t) for i in range(3)))
    return g.resize((size, size))


def build_master():
    base = vgradient(S, INK_TOP, INK_BOT).convert("RGBA")

    # Subtle amber glow behind the mark for a "material shelf" warmth.
    glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse([S * 0.18, S * 0.18, S * 0.82, S * 0.82], fill=(224, 128, 58, 45))
    base = Image.alpha_composite(base, glow)

    draw = ImageDraw.Draw(base)

    # Bold geometric "N" made of three strokes.
    stroke = int(S * 0.11)
    left_x = int(S * 0.30)
    right_x = int(S * 0.70) - stroke
    top_y = int(S * 0.28)
    bot_y = int(S * 0.72)

    # Left vertical
    draw.rounded_rectangle([left_x, top_y, left_x + stroke, bot_y],
                           radius=stroke // 3, fill=AMBER)
    # Right vertical
    draw.rounded_rectangle([right_x, top_y, right_x + stroke, bot_y],
                           radius=stroke // 3, fill=AMBER)
    # Diagonal connecting top-left to bottom-right
    draw.line([left_x + stroke // 2, top_y + stroke // 2,
               right_x + stroke // 2, bot_y - stroke // 2],
              fill=AMBER_SOFT, width=stroke)

    # Round the whole thing into an app-icon squircle.
    mask = rounded_mask(S, int(S * 0.22))
    out = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    out.paste(base, (0, 0), mask)
    return out


def main():
    master = build_master()

    for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256)]:
        master.resize((size, size), Image.LANCZOS).save(OUT / name)

    # Windows .ico with a range of sizes.
    master.save(OUT / "icon.ico",
                sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])

    # macOS .icns (Pillow writes the standard set from a large master).
    master.resize((512, 512), Image.LANCZOS).save(OUT / "icon.icns")

    # A general-purpose 512 PNG too (used by some installers).
    master.resize((512, 512), Image.LANCZOS).save(OUT / "icon.png")

    print("wrote:", ", ".join(sorted(p.name for p in OUT.glob("*"))))


if __name__ == "__main__":
    main()
