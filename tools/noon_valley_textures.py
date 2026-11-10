"""Generate NoonValley's high-resolution, original, deterministic textures (NumPy/Pillow/SciPy).

Usage::

    python noon_valley_textures.py <SampleProject>/Assets/NoonValley/Textures [--only NAME]

Writes (all seamless unless noted, OpenGL normal maps, sRGB albedo):

* ``OakBark*``   2048^2: deep-furrowed oak bark: tall grey-brown plates split by dark,
  branching fissures and cross cracks, fibrous plate tops, lichen; ``OakBark.png`` (RGB
  albedo), ``OakBarkNormal.png``, ``OakBarkRoughness.png``, ``OakBarkAO.png``,
  ``OakBarkHeight.png``.  One tile is about 1.2 m of trunk; image up is trunk up.
* ``BirchBark*`` 2048^2: white birch with dark lenticels and black patches, same five maps.
* ``ForestFloor*`` 2048^2: brown leaf litter, twigs and small sticks over dark soil
  (``ForestFloor.png``, ``ForestFloorNormal.png``, ``ForestFloorHeight.png``); 3 m a tile.
* ``Fern*`` 1024^2 2 x 2 leaf atlas (not tiled): a fern frond in each cell entering at the
  bottom centre, straight alpha (``Fern.png``), ``FernNormal.png``, ``FernRoughness.png``,
  ``FernTranslucency.png`` (the tree leaf-atlas layout, so a ``.tree`` recipe can use it).
* ``Groundcover*`` 1024^2 2 x 2 leaf atlas of broad-leaved woodland plants (same maps).

All imagery is synthesised from periodic noise and analytic shapes; no source imagery is used.
"""

from __future__ import annotations

import argparse
import math
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import gaussian_filter, distance_transform_edt, map_coordinates

BARK = 2048
FLOOR = 2048
ATLAS = 1024


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def value_noise(n, cells_x, cells_y, seed):
    """Periodic smooth value noise on an n x n tile with cells_x x cells_y lattice cells."""
    rng = np.random.default_rng(seed)
    lat = rng.random((cells_y, cells_x)).astype(np.float32)
    y = np.arange(n, dtype=np.float32) * cells_y / n
    x = np.arange(n, dtype=np.float32) * cells_x / n
    y0 = np.floor(y).astype(int); x0 = np.floor(x).astype(int)
    fy = y - y0; fx = x - x0
    fy = fy * fy * (3 - 2 * fy); fx = fx * fx * (3 - 2 * fx)
    y1 = (y0 + 1) % cells_y; x1 = (x0 + 1) % cells_x
    y0 %= cells_y; x0 %= cells_x
    a = lat[np.ix_(y0, x0)]; b = lat[np.ix_(y0, x1)]
    c = lat[np.ix_(y1, x0)]; d = lat[np.ix_(y1, x1)]
    top = a + (b - a) * fx[None, :]
    bot = c + (d - c) * fx[None, :]
    return top + (bot - top) * fy[:, None]


def fbm(n, cx, cy, octaves, seed):
    out = np.zeros((n, n), np.float32); amp = 1.0; norm = 0.0
    for o in range(octaves):
        out += amp * value_noise(n, cx << o, cy << o, seed + o * 17)
        norm += amp; amp *= 0.5
    return out / norm


def worley(n, cols, rows, seed, aspect=1.0, warp=None):
    """F1, F2 distances (in cell units, x scaled by `aspect`) to jittered tileable points."""
    rng = np.random.default_rng(seed)
    pts = rng.random((rows, cols, 2)).astype(np.float32)
    yy, xx = np.mgrid[0:n, 0:n].astype(np.float32)
    u = xx * cols / n; v = yy * rows / n
    if warp is not None:
        u = u + warp[0]; v = v + warp[1]
    cu = np.floor(u).astype(int); cv = np.floor(v).astype(int)
    f1 = np.full((n, n), 1e9, np.float32); f2 = np.full((n, n), 1e9, np.float32)
    idx = np.zeros((n, n), np.int32)
    for dv in (-1, 0, 1):
        for du in (-1, 0, 1):
            gu = cu + du; gv = cv + dv
            p = pts[gv % rows, gu % cols]
            dx = (gu + p[..., 0] - u) * aspect
            dy = gv + p[..., 1] - v
            d = np.sqrt(dx * dx + dy * dy)
            closer = d < f1
            f2 = np.where(closer, f1, np.minimum(f2, d))
            idx = np.where(closer, (gv % rows) * cols + (gu % cols), idx)
            f1 = np.where(closer, d, f1)
    return f1, f2, idx


def to_srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)


def save_rgb(path, lin):
    Image.fromarray((to_srgb(lin) * 255 + 0.5).astype(np.uint8), "RGB").save(path)


def save_gray(path, g):
    Image.fromarray((np.clip(g, 0, 1) * 255 + 0.5).astype(np.uint8), "L").save(path)


def normal_map(h, strength, wrap=True):
    if wrap:
        dx = (np.roll(h, -1, 1) - np.roll(h, 1, 1)) * 0.5
        dy = (np.roll(h, -1, 0) - np.roll(h, 1, 0)) * 0.5
    else:
        dy, dx = np.gradient(h)
    nx = -dx * strength; ny = dy * strength; nz = np.ones_like(h)
    l = np.sqrt(nx * nx + ny * ny + nz * nz)
    n = np.stack([nx / l, ny / l, nz / l], -1)
    return (n * 0.5 + 0.5)


def save_normal(path, n):
    Image.fromarray((np.clip(n, 0, 1) * 255 + 0.5).astype(np.uint8), "RGB").save(path)


def warped(field, dx, dy):
    """`field` sampled at (x + dx, y + dy) pixels, wrapping (tileable)."""
    n = field.shape[0]
    yy, xx = np.mgrid[0:n, 0:n].astype(np.float32)
    return map_coordinates(field, [yy + dy, xx + dx], order=1, mode="grid-wrap")


def furrows(n, cx, cy, seed, dx, dy):
    """Distance-like 0..1 to the meandering lines where a stretched noise crosses its middle
    (lines run along the noise's long axis, join and part again)."""
    f = warped(fbm(n, cx, cy, 4, seed), dx, dy)
    # In standard deviations from the middle (fbm bunches up around 0.5).
    return np.abs(f - f.mean()) / max(float(f.std()), 1e-6)


def oak_bark(out: Path):
    n = BARK
    # One tile is ~1.2 m of trunk: ~20 ridges across, each a few centimetres wide, split by
    # deep vertical furrows that join and part again (an interlaced, elongated diamond
    # network), and short horizontal cracks cutting the ridges into blocks.
    dx = (fbm(n, 3, 5, 3, 11) - 0.5) * n * 0.06
    dy = (fbm(n, 4, 3, 3, 12) - 0.5) * n * 0.02
    width = 0.45 + 0.6 * fbm(n, 6, 4, 3, 13)
    main = smooth(0.15, 1.0, furrows(n, 24, 2, 21, dx, dy) / width)
    second = smooth(0.1, 1.0, furrows(n, 34, 3, 22, dx * 1.3, dy) / (width * 0.6))
    ridge = main * (0.3 + 0.7 * second)
    # Rounded ridge shoulders, a rough broken surface on top.
    rough = fbm(n, 70, 24, 4, 23)
    ridge = ridge * (0.7 + 0.5 * rough)
    # Horizontal cracks, short: only where a fine noise lets them.
    cr = furrows(n, 3, 40, 31, dx * 0.3, dy * 3.0)
    crack = 1.0 - smooth(0.0, 0.25, cr)
    let_crack = smooth(0.45, 0.62, fbm(n, 30, 40, 3, 33))
    ridge = ridge * (1.0 - 0.8 * crack * let_crack)
    # Fibrous, flaky ridge tops: vertical fibres and lifted scales.
    fibre = value_noise(n, 200, 10, 41) * 0.55 + value_noise(n, 460, 24, 43) * 0.45
    scales = smooth(0.4, 0.8, fbm(n, 70, 30, 3, 45))
    fine = fbm(n, 96, 96, 3, 47)
    tone = fbm(n, 10, 4, 3, 49)
    height = ridge * (0.8 + 0.12 * fibre + 0.08 * scales) + 0.03 * fine
    height = gaussian_filter(height, 0.8, mode="wrap")
    lit = smooth(0.3, 0.9, height)
    # Grey weathered ridge faces, warm dark brown deep in the furrows, lichen crusts.
    face = np.array([0.17, 0.15, 0.125], np.float32)
    face = face[None, None, :] * (0.7 + 0.5 * tone[..., None]) * (0.85 + 0.3 * fine[..., None])
    face = face * (0.85 + 0.3 * fibre[..., None]) * (0.55 + 0.7 * rough[..., None])
    deep = np.array([0.03, 0.021, 0.014], np.float32)
    wall = np.array([0.095, 0.067, 0.043], np.float32)
    col = deep + (wall - deep) * smooth(0.0, 0.4, height)[..., None]
    col = col + (face - col) * lit[..., None]
    lichen = smooth(0.7, 0.8, fbm(n, 24, 16, 5, 51)) * lit
    col = col + (np.array([0.30, 0.32, 0.26]) - col) * (lichen * 0.35)[..., None]
    moss = smooth(0.66, 0.82, fbm(n, 6, 12, 5, 53)) * smooth(0.55, 0.2, height)
    col = col + (np.array([0.05, 0.075, 0.02]) - col) * (moss * 0.45)[..., None]
    save_rgb(out / "OakBark.png", col)
    save_normal(out / "OakBarkNormal.png", normal_map(height, 32.0))
    save_gray(out / "OakBarkHeight.png", height)
    save_gray(out / "OakBarkAO.png", 0.15 + 0.85 * smooth(0.0, 0.65, height))
    save_gray(out / "OakBarkRoughness.png", 0.8 + 0.18 * (1 - lit) - 0.12 * lichen)


def birch_bark(out: Path):
    n = BARK
    rng = np.random.default_rng(7)
    band = value_noise(n, 3, 40, 61) * 0.5 + value_noise(n, 6, 120, 62) * 0.5
    fine = fbm(n, 48, 96, 3, 63)
    col = np.zeros((n, n, 3), np.float32)
    white = np.array([0.78, 0.76, 0.71], np.float32)
    col[:] = white * (0.88 + 0.16 * band[..., None]) * (0.94 + 0.08 * fine[..., None])
    height = 0.6 + 0.1 * band + 0.05 * fine
    # Peeling, slightly warmer strips.
    peel = smooth(0.62, 0.7, value_noise(n, 5, 30, 64))
    col = col + (np.array([0.72, 0.58, 0.48]) - col) * (peel * 0.5)[..., None]
    # Black patches: horizontal blotches, rough edged, and diamond scars.
    blot = fbm(n, 4, 10, 5, 65) + 0.25 * fbm(n, 24, 48, 3, 66)
    patch = smooth(0.66, 0.7, blot)
    img = Image.new("L", (n, n), 0)
    dr = ImageDraw.Draw(img)
    for _ in range(6):
        cx, cy = rng.random() * n, rng.random() * n
        w, h = 50 + rng.random() * 110, 60 + rng.random() * 140
        for ox in (-n, 0, n):
            for oy in (-n, 0, n):
                x, y = cx + ox, cy + oy
                dr.polygon([(x, y - h / 2), (x + w / 2, y), (x, y + h / 2), (x - w / 2, y)], fill=255)
    diamonds = gaussian_filter(np.asarray(img, np.float32) / 255.0, 9, mode="wrap")
    rough = fbm(n, 64, 64, 2, 67)
    patch = np.maximum(patch, smooth(0.35, 0.6, diamonds + (rough - 0.5) * 0.5))
    dark = np.array([0.045, 0.04, 0.04], np.float32)
    col = col + (dark - col) * patch[..., None]
    height = height - 0.35 * patch + 0.1 * patch * rough
    # Lenticels: short dark horizontal dashes.
    img = Image.new("L", (n, n), 0)
    dr = ImageDraw.Draw(img)
    for _ in range(2600):
        x, y = rng.random() * n, rng.random() * n
        length = 14 + rng.random() ** 2 * 140
        thick = 1 + rng.random() * 3.5
        for ox in (-n, 0, n):
            dr.line([(x + ox, y), (x + ox + length, y + rng.normal() * 1.5)], fill=255, width=int(thick))
    len_mask = np.asarray(img, np.float32) / 255.0
    len_mask = gaussian_filter(len_mask, 0.7, mode="wrap")
    col = col + (np.array([0.12, 0.09, 0.08]) - col) * (len_mask * 0.85)[..., None]
    height = height - 0.15 * len_mask
    save_rgb(out / "BirchBark.png", col)
    save_normal(out / "BirchBarkNormal.png", normal_map(gaussian_filter(height, 0.8, mode="wrap"), 14.0))
    save_gray(out / "BirchBarkHeight.png", height)
    save_gray(out / "BirchBarkAO.png", 0.55 + 0.45 * smooth(0.2, 0.7, height))
    save_gray(out / "BirchBarkRoughness.png", 0.55 + 0.35 * patch + 0.1 * len_mask)


def stamp_leaf(col, height, cx, cy, length, width, angle, rgb, top, n, lobes):
    reach = int(length) + 2
    ys, xs = np.mgrid[-reach:reach + 1, -reach:reach + 1].astype(np.float32)
    ca, sa = math.cos(angle), math.sin(angle)
    lx = xs * ca + ys * sa
    ly = -xs * sa + ys * ca
    t = lx / length
    w = width * np.sqrt(np.clip(1 - t * t, 0, 1))
    if lobes:
        w = w * (0.8 + 0.25 * np.cos(t * 18.0))
    inside = (np.abs(ly) < w) & (np.abs(t) < 1)
    if not inside.any():
        return
    py = (int(cy) + ys.astype(int)) % n
    px = (int(cx) + xs.astype(int)) % n
    h = top + 0.04 * (1 - np.abs(ly) / np.maximum(w, 1e-3)) - 0.02 * np.abs(t)
    vein = np.exp(-(ly / 0.8) ** 2) * 0.25
    old = height[py, px]
    upd = inside & (h > old)
    height[py[upd], px[upd]] = h[upd]
    shade = (1.0 - vein - 0.15 * np.abs(t))[upd][:, None]
    col[py[upd], px[upd]] = np.asarray(rgb, np.float32)[None, :] * shade


def forest_floor(out: Path):
    n = FLOOR
    rng = np.random.default_rng(9)
    soil_n = fbm(n, 6, 6, 5, 71)
    col = np.zeros((n, n, 3), np.float32)
    col[:] = np.array([0.065, 0.048, 0.032]) * (0.7 + 0.6 * soil_n[..., None])
    height = 0.2 * soil_n
    leaves = 26000
    # Dry oak and beech litter: warm browns, pale tans and a few greyed, older leaves.
    browns = [(0.30, 0.17, 0.07), (0.24, 0.15, 0.07), (0.36, 0.22, 0.09), (0.18, 0.12, 0.06),
              (0.28, 0.21, 0.11), (0.42, 0.27, 0.12), (0.15, 0.11, 0.06), (0.46, 0.33, 0.18),
              (0.40, 0.32, 0.22), (0.30, 0.26, 0.20)]
    for k in range(leaves):
        cx, cy = rng.random() * n, rng.random() * n
        length = 12 + rng.random() ** 1.5 * 30
        width = length * (0.35 + rng.random() * 0.25)
        rgb = np.array(browns[rng.integers(len(browns))]) * (0.75 + rng.random() * 0.5) * 0.72
        rgb = rgb * np.array([1.08, 0.92, 0.85])
        top = 0.25 + 0.7 * k / leaves
        stamp_leaf(col, height, cx, cy, length, width, rng.random() * math.tau, rgb, top, n,
                   lobes=rng.random() < 0.5)
    # Twigs and small sticks on top.
    img_h = np.zeros((n, n), np.float32)
    img_c = np.zeros((n, n, 3), np.float32)
    mask = Image.new("L", (n, n), 0)
    dr = ImageDraw.Draw(mask)
    for _ in range(420):
        x, y = rng.random() * n, rng.random() * n
        a = rng.random() * math.tau
        length = 40 + rng.random() ** 2 * 320
        width = 2 + rng.random() * (5 if length < 200 else 9)
        pts = [(x, y)]
        for s in range(6):
            a += rng.normal() * 0.25
            x += math.cos(a) * length / 6; y += math.sin(a) * length / 6
            pts.append((x, y))
        for ox in (-n, 0, n):
            for oy in (-n, 0, n):
                dr.line([(px + ox, py + oy) for px, py in pts], fill=255, width=int(width))
    twig = gaussian_filter(np.asarray(mask, np.float32) / 255.0, 1.0, mode="wrap")
    tw = twig > 0.35
    height = np.where(tw, np.maximum(height, 1.0 + 0.1 * twig), height)
    twig_col = np.array([0.075, 0.055, 0.04]) * (0.8 + 0.4 * fbm(n, 40, 40, 2, 73))[..., None]
    col = np.where(tw[..., None], twig_col, col)
    save_rgb(out / "ForestFloor.png", col)
    save_normal(out / "ForestFloorNormal.png", normal_map(gaussian_filter(height, 0.7, mode="wrap"), 9.0))
    save_gray(out / "ForestFloorHeight.png", height / max(height.max(), 1e-3))


def atlas_maps(out: Path, name: str, draw_cell):
    """A 2 x 2 leaf atlas: `draw_cell(draw, cell_rgb_draw, size, seed)` paints one cell."""
    cell = ATLAS // 2
    alpha = Image.new("L", (ATLAS, ATLAS), 0)
    tone = Image.new("L", (ATLAS, ATLAS), 128)
    for i in range(4):
        cx, cy = (i % 2) * cell, (i // 2) * cell
        a = Image.new("L", (cell, cell), 0)
        t = Image.new("L", (cell, cell), 128)
        draw_cell(ImageDraw.Draw(a), ImageDraw.Draw(t), cell, 100 + i)
        alpha.paste(a, (cx, cy)); tone.paste(t, (cx, cy))
    a = np.asarray(alpha, np.float32) / 255.0
    t = np.asarray(tone, np.float32) / 255.0
    return a, t


def finish_atlas(out: Path, name: str, a, t, light, dark):
    """Colour, normal, roughness and translucency of an atlas from its alpha and tone maps."""
    light = np.array(light, np.float32); dark = np.array(dark, np.float32)
    col = dark + (light - dark) * t[..., None]
    # Dilate colours into the transparent texels (no dark fringes in mips).
    inside = a > 0.5
    if inside.any():
        _, (iy, ix) = distance_transform_edt(~inside, return_indices=True)
        col = col[iy, ix]
    srgb = (to_srgb(col) * 255 + 0.5).astype(np.uint8)
    rgba = np.dstack([srgb, (np.clip(a, 0, 1) * 255 + 0.5).astype(np.uint8)])
    Image.fromarray(rgba, "RGBA").save(out / f"{name}.png")
    h = gaussian_filter(a, 2.0) * 0.6 + t * 0.4
    nm = normal_map(h, 6.0, wrap=False)
    nm[~inside] = (0.5, 0.5, 1.0)
    save_normal(out / f"{name}Normal.png", nm)
    save_gray(out / f"{name}Roughness.png", np.where(inside, 0.55 + 0.2 * (1 - t), 1.0))
    save_gray(out / f"{name}Translucency.png", np.where(inside, 0.55 + 0.35 * t, 0.0))


def fern(out: Path):
    def cell(da, dt, size, seed):
        rng = np.random.default_rng(seed)
        # A frond from the bottom centre, arching a little to one side.
        bend = (rng.random() - 0.5) * 0.25
        steps = 34
        pts = []
        for s in range(steps + 1):
            u = s / steps
            x = size * 0.5 + math.sin(u * 1.6) * bend * size
            y = size * (0.98 - 0.94 * u)
            pts.append((x, y))
        da.line(pts, fill=255, width=4)
        dt.line(pts, fill=70, width=4)
        for s in range(3, steps):
            u = s / steps
            x, y = pts[s]
            nx, ny = pts[s + 1]
            ang = math.atan2(ny - y, nx - x)
            length = size * 0.3 * math.sin(math.pi * min(1.0, 0.1 + u * 0.95)) * (1.05 - u * 0.5)
            if length < 6:
                continue
            # Pinnae alternate along the rachis, leaving gaps between them.
            for side in ((-1,) if s % 2 else (1,)):
                a = ang + side * (math.pi * 0.5 - 0.35)
                # Pinna: a tapered blade with serrated pinnules.
                segs = 11
                upper, lower = [], []
                for k in range(segs + 1):
                    v = k / segs
                    px = x + math.cos(a) * length * v
                    py = y + math.sin(a) * length * v
                    w = size * 0.02 * (1 - 0.85 * v) * (0.55 + 0.9 * (k % 2))
                    upper.append((px + math.cos(a - math.pi / 2) * w, py + math.sin(a - math.pi / 2) * w))
                    lower.append((px + math.cos(a + math.pi / 2) * w, py + math.sin(a + math.pi / 2) * w))
                poly = upper + lower[::-1]
                da.polygon(poly, fill=255)
                dt.polygon(poly, fill=int(110 + 120 * u + rng.random() * 30))
                dt.line([(x, y), (x + math.cos(a) * length, y + math.sin(a) * length)], fill=60, width=1)
    a, t = atlas_maps(out, "Fern", cell)
    finish_atlas(out, "Fern", a, t, light=(0.26, 0.42, 0.09), dark=(0.06, 0.13, 0.025))


def groundcover(out: Path):
    def cell(da, dt, size, seed):
        rng = np.random.default_rng(seed)
        # Stems from the bottom centre ending in broad, pointed leaves.
        # Dense, overlapping broad leaves (dog's mercury, wood sorrel, young lime).
        for _ in range(14):
            a = -math.pi / 2 + (rng.random() - 0.5) * 2.4
            length = size * (0.18 + rng.random() * 0.38)
            sx, sy = size * 0.5, size * 0.98
            ex, ey = sx + math.cos(a) * length, sy + math.sin(a) * length
            da.line([(sx, sy), (ex, ey)], fill=255, width=3)
            dt.line([(sx, sy), (ex, ey)], fill=80, width=3)
            leaf_len = size * (0.15 + rng.random() * 0.14)
            la = a + (rng.random() - 0.5) * 0.8
            w = leaf_len * 0.42
            poly = []
            for k in range(17):
                v = k / 16
                r = math.sin(math.pi * v) ** 0.8 * w
                poly.append((ex + math.cos(la) * leaf_len * v + math.cos(la + math.pi / 2) * r,
                             ey + math.sin(la) * leaf_len * v + math.sin(la + math.pi / 2) * r))
            for k in range(16, -1, -1):
                v = k / 16
                r = math.sin(math.pi * v) ** 0.8 * w
                poly.append((ex + math.cos(la) * leaf_len * v - math.cos(la + math.pi / 2) * r,
                             ey + math.sin(la) * leaf_len * v - math.sin(la + math.pi / 2) * r))
            da.polygon(poly, fill=255)
            dt.polygon(poly, fill=int(120 + rng.random() * 110))
            dt.line([(ex, ey), (ex + math.cos(la) * leaf_len, ey + math.sin(la) * leaf_len)], fill=90, width=2)
            # Side veins.
            for k in range(1, 6):
                v = k / 6.5
                bx = ex + math.cos(la) * leaf_len * v
                by = ey + math.sin(la) * leaf_len * v
                r = math.sin(math.pi * v) ** 0.8 * w * 0.8
                for side in (-1, 1):
                    va = la + side * 0.9
                    dt.line([(bx, by), (bx + math.cos(va) * r, by + math.sin(va) * r)], fill=100, width=1)
    a, t = atlas_maps(out, "Groundcover", cell)
    finish_atlas(out, "Groundcover", a, t, light=(0.22, 0.4, 0.1), dark=(0.05, 0.12, 0.03))


JOBS = {"oak": oak_bark, "birch": birch_bark, "floor": forest_floor, "fern": fern,
        "groundcover": groundcover}


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("out_dir", type=Path)
    p.add_argument("--only", choices=sorted(JOBS))
    args = p.parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    for name, job in JOBS.items():
        if args.only and args.only != name:
            continue
        job(args.out_dir)
        print(f"{name} done")


if __name__ == "__main__":
    main()
