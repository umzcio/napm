#!/usr/bin/env python3
"""Generate the napm DMG background (original art; no third-party trade dress).

Draws the installer window as a late-90s dialog in the app's own palette: a
gradient title bar with the npstr logo, white list-view wells behind the two
icons, a transfer bar between them, and a status bar. Renders 1x and 2x and
combines them into one HiDPI TIFF so the background stays sharp on Retina
displays (Finder picks the matching resolution from the TIFF).

Requires Pillow and rsvg-convert (brew install librsvg). Output:
src-tauri/dmg/background.tiff, consumed by scripts/make-dmg.sh. Icon positions
here must match the --icon / --app-drop-link coordinates in that script.
"""
from PIL import Image, ImageDraw, ImageFont
import os
import subprocess
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MONO = os.path.join(ROOT, "frontend", "vt323.ttf")
UI = "/System/Library/Fonts/Supplemental/Tahoma.ttf"
UI_BOLD = "/System/Library/Fonts/Supplemental/Tahoma Bold.ttf"
LOGO = os.path.join(ROOT, "assets", "npstr-logo.svg")
OUT = os.path.join(ROOT, "src-tauri", "dmg", "background.tiff")

# Window content size and icon centers, in points. Keep in sync with make-dmg.sh.
# The canvas runs PAD points taller than the layout because the window's title
# bar height varies by macOS release (28 to 32pt), so the visible content area
# does too; the extra face-colored strip keeps the bottom edge from showing
# Finder's white fill on releases with a shorter title bar.
W, H, PAD = 660, 420, 20
APP_X, DROP_X, ICON_Y = 165, 495, 215

# Palette from frontend/index.html :root.
FACE = (192, 192, 192)
NAVY = (0, 0, 128)
NAVY2 = (16, 132, 208)
WHITE = (255, 255, 255)
LGRAY = (223, 223, 223)
DGRAY = (128, 128, 128)
DDGRAY = (64, 64, 64)
INK = (0, 0, 0)
GREEN = (0, 136, 12)


def sunken(d, box, s, fill=None):
    """Two-pixel Win98 sunken bevel: dark top/left, light bottom/right."""
    x0, y0, x1, y1 = box
    if fill:
        d.rectangle(box, fill=fill)
    for i, (tl, br) in enumerate(((DGRAY, WHITE), (DDGRAY, LGRAY))):
        o = i * s
        d.rectangle([x0 + o, y0 + o, x1 - o, y0 + o + s - 1], fill=tl)
        d.rectangle([x0 + o, y0 + o, x0 + o + s - 1, y1 - o], fill=tl)
        d.rectangle([x0 + o, y1 - o - s + 1, x1 - o, y1 - o], fill=br)
        d.rectangle([x1 - o - s + 1, y0 + o, x1 - o, y1 - o], fill=br)


def logo(height_px):
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "logo.png")
        subprocess.run(["rsvg-convert", "-h", str(height_px), LOGO, "-o", path], check=True)
        return Image.open(path).convert("RGBA")


def render(s):
    img = Image.new("RGB", (W * s, (H + PAD) * s), FACE)
    d = ImageDraw.Draw(img)

    # Title bar: navy to light-blue gradient, as in the app's .titlebar.
    bar_h = 46 * s
    for x in range(W * s):
        t = x / (W * s - 1)
        c = tuple(round(a + (b - a) * t) for a, b in zip(NAVY, NAVY2))
        d.line([x, 0, x, bar_h - 1], fill=c)
    mark = logo(34 * s)
    img.paste(mark, (14 * s, 6 * s), mark)
    d.text((56 * s, 6 * s), "napm", font=ImageFont.truetype(MONO, 32 * s), fill=WHITE)
    d.text((126 * s, 17 * s), "Setup", font=ImageFont.truetype(UI_BOLD, 13 * s), fill=WHITE)
    d.line([0, bar_h, W * s, bar_h], fill=DGRAY, width=s)

    ui = ImageFont.truetype(UI, 12 * s)
    d.text((20 * s, 60 * s), "Drag napm onto Applications to install it.", font=ui, fill=INK)

    # White list-view wells behind each icon (128pt icon plus its label).
    for cx in (APP_X, DROP_X):
        sunken(d, [(cx - 96) * s, (ICON_Y - 92) * s, (cx + 96) * s, (ICON_Y + 104) * s], s, fill=WHITE)

    # Transfer bar between the wells: a Win98 block progress bar, then an arrowhead.
    bx0, bx1, by = 278, 372, ICON_Y
    sunken(d, [bx0 * s, (by - 11) * s, bx1 * s, (by + 11) * s], s, fill=WHITE)
    x = bx0 + 4
    while x + 8 <= bx1 - 4:
        d.rectangle([x * s, (by - 7) * s, (x + 8) * s - 1, (by + 7) * s], fill=GREEN)
        x += 10
    d.polygon([((bx1 + 6) * s, (by - 14) * s), ((bx1 + 24) * s, by * s), ((bx1 + 6) * s, (by + 14) * s)], fill=GREEN)
    cap = ImageFont.truetype(MONO, 18 * s)
    label = "transfer"
    tw = d.textlength(label, font=cap)
    d.text((((bx0 + bx1) / 2) * s - tw / 2, (by + 16) * s), label, font=cap, fill=DDGRAY)

    # Status bar along the bottom edge.
    sy0, sy1 = H - 30, H - 8
    small = ImageFont.truetype(UI, 11 * s)
    sunken(d, [8 * s, sy0 * s, 420 * s, sy1 * s], s)
    d.text((16 * s, (sy0 + 5) * s), "Ready", font=small, fill=INK)
    sunken(d, [426 * s, sy0 * s, (W - 8) * s, sy1 * s], s)
    d.text((434 * s, (sy0 + 5) * s), "Signed and notarized", font=small, fill=INK)
    return img


with tempfile.TemporaryDirectory() as tmp:
    one, two = os.path.join(tmp, "bg.png"), os.path.join(tmp, "bg@2x.png")
    render(1).save(one, dpi=(72, 72))
    render(2).save(two, dpi=(144, 144))
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    subprocess.run(["tiffutil", "-cathidpicheck", one, two, "-out", OUT], check=True)
print("wrote", OUT)
