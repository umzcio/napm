#!/usr/bin/env python3
"""Generate the napm DMG background (original art; no third-party trade dress)."""
from PIL import Image, ImageDraw, ImageFont
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONT = os.path.join(ROOT, "frontend", "vt323.ttf")
OUT_DIR = os.path.join(ROOT, "src-tauri", "dmg")
os.makedirs(OUT_DIR, exist_ok=True)

FACE = (195, 195, 195)
NAVY = (0, 0, 130)
WHITE = (255, 255, 255)
DGRAY = (128, 128, 128)
INK = (10, 10, 10)
GREEN = (0, 136, 12)

def render(scale, path):
    W, H = 660 * scale, 420 * scale
    img = Image.new("RGB", (W, H), FACE)
    d = ImageDraw.Draw(img)
    # Navy title strip with a beveled feel.
    d.rectangle([0, 0, W, 46 * scale], fill=NAVY)
    d.line([0, 46 * scale, W, 46 * scale], fill=DGRAY, width=max(1, scale))
    big = ImageFont.truetype(FONT, 30 * scale)
    small = ImageFont.truetype(FONT, 17 * scale)
    d.text((20 * scale, 8 * scale), "npstr :: napm", font=big, fill=WHITE)
    d.text((20 * scale, 64 * scale), "drag napm into Applications to install", font=small, fill=INK)
    # Arrow centered in the gap between the icons. Icons sit at x=165 and x=495
    # (each ~128px wide), so the clear gap is roughly x=229..431. Keep the whole
    # arrow inside it so the head does not overlap the Applications folder.
    y = 250 * scale
    d.line([245 * scale, y, 390 * scale, y], fill=GREEN, width=6 * scale)
    d.polygon([(390 * scale, y - 15 * scale), (415 * scale, y), (390 * scale, y + 15 * scale)], fill=GREEN)
    img.save(path)
    print("wrote", path)

render(1, os.path.join(OUT_DIR, "background.png"))
render(2, os.path.join(OUT_DIR, "background@2x.png"))
