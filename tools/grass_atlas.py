"""Regenerate the leaf cells (0-3) of the built-in grass atlas.

crates/misty_engine/src/render/assets/grass_atlas.png holds eight 128x1024 cells side by
side, root at the bottom. Cells 4-7 are photographed seed heads and stay as they are; this
script redraws cells 0-3 as clumps of thin field-grass blades (each 2-4 mm wide on a card
about 4 cm across: 6-10 px of 128 at the base), tapering to a fine point, each with its own
length, soft droop and lean, so one card reads as a strip of meadow grass rather than one
broad leaf. Prints the cells' average linear colours for CELL_AVG in grass.wgsl.

    python tools/grass_atlas.py
"""

import math
import pathlib
import random

import numpy as np
from PIL import Image

ATLAS = pathlib.Path(__file__).resolve().parents[1] / (
    "crates/misty_engine/src/render/assets/grass_atlas.png"
)
CELL_W, H = 128, 1024
# Blades per leaf cell.
BLADES = [14, 16, 13, 18]


def srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)


def lin(c):
    return np.where(c <= 0.04045, c / 12.92, np.power((c + 0.055) / 1.055, 2.4))


def cell(seed, count):
    rng = random.Random(seed)
    color = np.zeros((H, CELL_W, 3), np.float32)
    alpha = np.zeros((H, CELL_W), np.float32)
    xs = np.arange(CELL_W, dtype=np.float32) + 0.5
    # Longest blades first, so the short ones lie in front of them.
    lengths = sorted((rng.uniform(0.55, 1.0) for _ in range(count)), reverse=True)
    lengths[0] = 1.0
    for length in lengths:
        # Roots spread across the card (a strip of turf, not a fan from one point).
        base = rng.uniform(12.0, CELL_W - 12.0)
        # A lean of its own plus a droop towards the tip (px at the tip).
        spread = (base - CELL_W / 2) * rng.uniform(0.0, 0.6) + rng.gauss(0.0, 12.0)
        droop = rng.choice([-1, 1]) * rng.uniform(18.0, 46.0)
        # Keep the tip inside the cell (scale the lean and droop down rather than clip).
        reach = spread + droop
        room = (CELL_W - 8.0 - base) if reach > 0 else (base - 8.0)
        if abs(reach) > room:
            k = max(room, 0.0) / abs(reach)
            spread, droop = spread * k, droop * k
        half = rng.uniform(3.0, 5.0)
        tone = rng.uniform(0.85, 1.15)
        dry_tip = rng.random() < 0.3
        wave = rng.uniform(0.0, math.tau)
        rows = int(length * (H - 4))
        for i in range(rows):
            t = i / rows
            y = H - 2 - i
            # The droop grows towards the tip: a soft arc, not a straight slant.
            cx = base + spread * t + droop * t**2.2 + 1.5 * math.sin(t * 7.0 + wave)
            # Pointed tip, slightly narrower at the sheath.
            hw = half * (1.0 - t) ** 0.65 * (0.88 + 0.12 * min(t * 8.0, 1.0))
            if hw < 0.05:
                continue
            s = (xs - cx) / max(hw, 1e-3)
            cover = np.clip(hw - np.abs(xs - cx) + 0.5, 0.0, 1.0)
            if cover.max() <= 0.0:
                continue
            # Darker sheath, green blade, lighter and warmer tip (straw on some).
            root = np.array([0.09, 0.15, 0.025])
            mid = np.array([0.21, 0.30, 0.04])
            tip = np.array([0.30, 0.32, 0.07])
            c = root + (mid - root) * min(t / 0.45, 1.0)
            if t > 0.45:
                c = mid + (tip - mid) * ((t - 0.45) / 0.55)
            if dry_tip and t > 0.72:
                c = c + (np.array([0.42, 0.34, 0.12]) - c) * min((t - 0.72) / 0.2, 1.0)
            c = c * tone
            a = np.abs(s)
            # A lighter midrib, darker edges, fine parallel veins.
            shade = np.where(a < 0.22, 1.12, 1.0) * np.where(a > 0.75, 0.86, 1.0)
            shade = shade * (1.0 - 0.07 * np.cos(s * math.pi * 3.0) ** 2)
            px = c[None, :] * shade[:, None]
            k = cover[:, None]
            color[y] = color[y] * (1.0 - k) + px * k
            alpha[y] = np.maximum(alpha[y], cover)
    return color, alpha


def main():
    atlas = np.asarray(Image.open(ATLAS).convert("RGBA")).astype(np.float32) / 255.0
    out = atlas.copy()
    for c, count in enumerate(BLADES):
        color, alpha = cell(0x6A11 + c * 97, count)
        x0 = c * CELL_W
        out[:, x0 : x0 + CELL_W, :3] = srgb(color)
        out[:, x0 : x0 + CELL_W, 3] = alpha
    Image.fromarray((out * 255.0 + 0.5).astype(np.uint8), "RGBA").save(ATLAS, optimize=True)
    for c in range(8):
        a = out[:, c * CELL_W : (c + 1) * CELL_W, 3]
        l = lin(out[:, c * CELL_W : (c + 1) * CELL_W, :3])
        avg = (l * a[..., None]).sum((0, 1)) / a.sum()
        cov = (a > 0.5).mean()
        print(f"cell {c}: avg ({avg[0]:.3f}, {avg[1]:.3f}, {avg[2]:.3f}), coverage {cov:.3f}")


if __name__ == "__main__":
    main()
