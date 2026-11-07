"""Generate original, deterministic PBR tree textures with NumPy/Pillow/SciPy.

Usage::

    python tree_textures.py <out_dir> [--only oak|linden|birch] [--preview]

For each species ``S`` (``Oak``, ``Linden``, ``Birch``), the generator writes
five seamless 1024 x 1024 bark maps: ``SBark.png`` (RGB sRGB albedo),
``SBarkNormal.png`` (RGB OpenGL tangent normal), ``SBarkRoughness.png`` (L),
``SBarkAO.png`` (L), and ``SBarkHeight.png`` (L).  Bark tiles on both axes;
image up is trunk up and one tile represents approximately 0.9 x 0.9 metres.

It also writes a 1024 x 1024, row-major 2 x 2 leaf atlas in
``SLeaves.png`` (straight-alpha RGBA), plus ``SLeavesNormal.png`` (RGB),
``SLeavesRoughness.png`` (L), and ``SLeavesTranslucency.png`` (L).  Each
512-square cell contains one twig spray entering at bottom centre. Transparent
albedo RGB is nearest-colour dilated; transparent normals are (128,128,255),
roughness is 255, and translucency is 0.

Normal maps use OpenGL convention: +X points right, +Y/green points toward
smaller image rows, and +Z points out.  ``--preview`` adds labelled bark and
leaf review sheets plus 512-square lit bark crops under
``<out_dir>/preview``.  All imagery is synthesised from periodic noise and
analytic geometry; no source imagery is used.
"""

from __future__ import annotations

import argparse
import math
import time
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy.ndimage import distance_transform_edt, gaussian_filter

SIZE = 1024
CELL = 512
ATLAS = 1024
SPECIES = ("oak", "linden", "birch")
NAMES = {"oak": "Oak", "linden": "Linden", "birch": "Birch"}


def smoothstep(x: np.ndarray) -> np.ndarray:
    x = np.clip(x, 0.0, 1.0)
    return x * x * (3.0 - 2.0 * x)


def normalize01(a: np.ndarray, lo: float | None = None,
                hi: float | None = None) -> np.ndarray:
    lo = float(a.min()) if lo is None else lo
    hi = float(a.max()) if hi is None else hi
    return np.clip((a - lo) / max(hi - lo, 1e-6), 0.0, 1.0).astype(np.float32)


def value_noise(shape: tuple[int, int], grid: tuple[int, int], seed: int) -> np.ndarray:
    """Periodic bicubic-like value noise using smooth bilinear interpolation."""
    h, w = shape
    gy, gx = grid
    values = np.random.default_rng(seed).random((gy, gx), dtype=np.float32)
    py = np.arange(h, dtype=np.float32) * gy / h
    px = np.arange(w, dtype=np.float32) * gx / w
    iy, ix = np.floor(py).astype(np.int32), np.floor(px).astype(np.int32)
    fy = smoothstep(py - iy)[:, None]
    fx = smoothstep(px - ix)[None, :]
    a = values[iy[:, None] % gy, ix[None, :] % gx]
    b = values[iy[:, None] % gy, (ix[None, :] + 1) % gx]
    c = values[(iy[:, None] + 1) % gy, ix[None, :] % gx]
    d = values[(iy[:, None] + 1) % gy, (ix[None, :] + 1) % gx]
    return ((a * (1 - fx) + b * fx) * (1 - fy) +
            (c * (1 - fx) + d * fx) * fy).astype(np.float32)


def fbm(shape: tuple[int, int], seed: int,
        grids: tuple[tuple[int, int], ...], persistence: float = 0.52) -> np.ndarray:
    out = np.zeros(shape, np.float32)
    weight = 1.0
    total = 0.0
    for octave, grid in enumerate(grids):
        out += weight * value_noise(shape, grid, seed + octave * 1009)
        total += weight
        weight *= persistence
    return out / total


def gradient_perlin(gy: int, gx: int, seed: int,
                    warp: tuple[np.ndarray, np.ndarray] | None = None) -> np.ndarray:
    """Periodic 2-D gradient noise on an integer ``gy`` by ``gx`` lattice."""
    rng = np.random.default_rng(seed)
    angle = rng.random((gy, gx), dtype=np.float32) * np.float32(2*np.pi)
    gradients = np.stack((np.cos(angle), np.sin(angle)), axis=-1).astype(np.float32)
    yy, xx = np.mgrid[:SIZE, :SIZE].astype(np.float32)
    if warp is not None:
        xx = xx + warp[0]
        yy = yy + warp[1]
    fy = (yy % SIZE) * gy / SIZE
    fx = (xx % SIZE) * gx / SIZE
    iy, ix = np.floor(fy).astype(np.int32), np.floor(fx).astype(np.int32)
    ty, tx = fy-iy, fx-ix

    def dot(oy: int, ox: int) -> np.ndarray:
        g = gradients[(iy+oy) % gy, (ix+ox) % gx]
        return g[..., 0]*(tx-ox) + g[..., 1]*(ty-oy)

    sy = ty*ty*ty*(ty*(ty*6-15)+10)
    sx = tx*tx*tx*(tx*(tx*6-15)+10)
    a = dot(0, 0)*(1-sx)+dot(0, 1)*sx
    b = dot(1, 0)*(1-sx)+dot(1, 1)*sx
    return ((a*(1-sy)+b*sy)*1.41).astype(np.float32)


def gradient_fbm(gy: int, gx: int, seed: int, octaves: int = 4,
                 persistence: float = .5,
                 warp: tuple[np.ndarray, np.ndarray] | None = None) -> np.ndarray:
    """Periodic gradient-fBm used for the photographic bark structures."""
    out = np.zeros((SIZE, SIZE), np.float32)
    weight = total = 0.0
    weight = 1.0
    for octave in range(octaves):
        out += weight*gradient_perlin(gy << octave, gx << octave,
                                      seed+101*octave, warp)
        total += weight
        weight *= persistence
    return out/total


def ridged(gy: int, gx: int, seed: int, octaves: int,
           warp: tuple[np.ndarray, np.ndarray] | None = None) -> np.ndarray:
    """Periodic ridged multifractal with sharp, fibrous zero-crossing crests."""
    out = np.zeros((SIZE, SIZE), np.float32)
    weight, total = 1.0, 0.0
    previous: np.ndarray | float = 1.0
    for octave in range(octaves):
        ridge = 1-np.abs(gradient_perlin(gy << octave, gx << octave,
                                         seed+97*octave, warp))
        ridge *= ridge
        out += weight*ridge*(.5+.5*previous)
        previous = ridge
        total += weight
        weight *= .5
    return out/total


def worley_edge(shape: tuple[int, int], grid: tuple[int, int], seed: int) -> np.ndarray:
    """Periodic Worley F2-F1 field; zero is the irregular cell boundary."""
    h, w = shape
    gy, gx = grid
    rng = np.random.default_rng(seed)
    oy = rng.random((gy, gx), dtype=np.float32)
    ox = rng.random((gy, gx), dtype=np.float32)
    cy = np.arange(h, dtype=np.float32)[:, None]*gy/h
    cx = np.arange(w, dtype=np.float32)[None, :]*gx/w
    iy, ix = np.floor(cy).astype(np.int32), np.floor(cx).astype(np.int32)
    f1 = np.full(shape, 1e6, np.float32)
    f2 = np.full(shape, 1e6, np.float32)
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            jy, jx = iy+dy, ix+dx
            py = jy + oy[jy % gy, jx % gx]
            px = jx + ox[jy % gy, jx % gx]
            d = np.sqrt((py-cy)**2+(px-cx)**2)
            old = f1
            f1 = np.minimum(f1, d)
            f2 = np.minimum(f2, np.maximum(old, d))
    return (f2-f1).astype(np.float32)


def warped_worley(shape: tuple[int, int], grid: tuple[int, int], seed: int,
                   qx: np.ndarray, qy: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Periodic anisotropic Worley edge distance in pixels and cell variation.

    Unlike a rasterised crack mask, this is a continuous surface field.  The
    sites live in elongated cells but distance is measured in pixel space, so
    a 12-pixel furrow stays 12 pixels wide on both vertical and cross cracks.
    """
    h, w = shape
    gy, gx = grid
    rng = np.random.default_rng(seed)
    cell_h, cell_w = h / gy, w / gx
    sy = (np.arange(gy, dtype=np.float32)[:, None] +
          rng.uniform(.18, .82, (gy, gx)).astype(np.float32)) * cell_h
    sx = (np.arange(gx, dtype=np.float32)[None, :] +
          rng.uniform(.16, .84, (gy, gx)).astype(np.float32)) * cell_w
    variation = rng.random((gy, gx), dtype=np.float32)
    qx = np.mod(qx, w).astype(np.float32)
    qy = np.mod(qy, h).astype(np.float32)
    iy = np.floor(qy / cell_h).astype(np.int32)
    ix = np.floor(qx / cell_w).astype(np.int32)
    f1 = np.full(shape, 1e9, np.float32)
    f2 = np.full(shape, 1e9, np.float32)
    cell_value = np.zeros(shape, np.float32)
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            jy, jx = iy + dy, ix + dx
            py = sy[jy % gy, jx % gx] + np.floor_divide(jy, gy) * h
            px = sx[jy % gy, jx % gx] + np.floor_divide(jx, gx) * w
            # Select neighbours in the cell's anisotropic metric; this keeps
            # the tessellation elongated along the trunk rather than turning
            # a sparse row of sites into square Euclidean cells.
            d = np.sqrt((px-qx)**2 + ((py-qy)*cell_w/cell_h)**2).astype(np.float32)
            nearer = d < f1
            f2 = np.where(nearer, f1, np.minimum(f2, d))
            f1 = np.minimum(f1, d)
            cell_value = np.where(nearer, variation[jy % gy, jx % gx], cell_value)
    raw = (f2-f1).astype(np.float32)
    # Convert the anisotropic implicit edge to approximate screen-pixel
    # distance.  It keeps both vertical furrows and cross cracks controllable
    # with the same width parameter.
    gy_raw, gx_raw = np.gradient(raw)
    edge_px = raw/np.maximum(np.sqrt(gx_raw*gx_raw+gy_raw*gy_raw), .08)
    return np.clip(edge_px, 0, 160).astype(np.float32), cell_value


def bark_domain(seed: int, warp_px: float = 18.0) -> tuple[np.ndarray, np.ndarray]:
    """A seamless low-frequency domain warp with slow sideways trunk drift."""
    yy, xx = np.mgrid[:SIZE, :SIZE].astype(np.float32)
    wx = fbm((SIZE, SIZE), seed, ((2, 3), (5, 7), (11, 13))) - .5
    wy = fbm((SIZE, SIZE), seed + 101, ((2, 2), (4, 5), (9, 11))) - .5
    drift = value_noise((SIZE, 1), (5, 1), seed + 211)[:, 0] - .5
    qx = xx + warp_px * 2.0 * wx + drift[:, None] * warp_px * 1.35
    qy = yy + warp_px * .72 * wy
    return qx.astype(np.float32), qy.astype(np.float32)


def periodic_lines(lines: list[tuple[list[tuple[float, float]], int]],
                   blur: float = 0.0) -> np.ndarray:
    """Rasterise polylines with copies around the toroidal tile boundary."""
    im = Image.new("L", (SIZE, SIZE), 0)
    draw = ImageDraw.Draw(im)
    for points, width in lines:
        for oy in (-SIZE, 0, SIZE):
            for ox in (-SIZE, 0, SIZE):
                pts = [(x + ox, y + oy) for x, y in points]
                draw.line(pts, fill=255, width=width, joint="curve")
    a = np.asarray(im, dtype=np.float32) / 255.0
    return gaussian_filter(a, blur, mode="wrap") if blur else a


def periodic_shapes(polygons: list[list[tuple[float, float]]], blur: float = 0.0) -> np.ndarray:
    im = Image.new("L", (SIZE, SIZE), 0)
    draw = ImageDraw.Draw(im)
    for points in polygons:
        for oy in (-SIZE, 0, SIZE):
            for ox in (-SIZE, 0, SIZE):
                draw.polygon([(x + ox, y + oy) for x, y in points], fill=255)
    a = np.asarray(im, dtype=np.float32) / 255.0
    return gaussian_filter(a, blur, mode="wrap") if blur else a


def normal_from_height(height: np.ndarray, strength: float, wrap: bool = True) -> np.ndarray:
    if wrap:
        dx = 0.5 * (np.roll(height, -1, 1) - np.roll(height, 1, 1))
        dy_up = 0.5 * (np.roll(height, 1, 0) - np.roll(height, -1, 0))
    else:
        p = np.pad(height, 1, mode="edge")
        dx = 0.5 * (p[1:-1, 2:] - p[1:-1, :-2])
        dy_up = 0.5 * (p[:-2, 1:-1] - p[2:, 1:-1])
    nx, ny = -dx * strength, -dy_up * strength
    nz = np.ones_like(height)
    inv = 1.0 / np.sqrt(nx * nx + ny * ny + nz * nz)
    n = np.stack((nx * inv, ny * inv, nz * inv), axis=-1)
    return np.clip(np.rint((n * 0.5 + 0.5) * 255), 0, 255).astype(np.uint8)


def horizon_ao(height: np.ndarray, relief: float) -> np.ndarray:
    """Wrapped eight-direction, multi-radius horizon/cavity AO estimate."""
    horizon = np.zeros_like(height)
    directions = ((1, 0), (-1, 0), (0, 1), (0, -1),
                  (1, 1), (1, -1), (-1, 1), (-1, -1))
    for radius in (2, 5, 11, 23, 47, 79):
        denom = max(radius ** 0.72, 1.0)
        for dy, dx in directions:
            neighbour = np.roll(height, (dy * radius, dx * radius), (0, 1))
            horizon = np.maximum(horizon, (neighbour - height) / denom)
    cavity = np.zeros_like(height)
    for sigma, weight in ((2.0, 0.45), (7.0, 0.34), (21.0, 0.21)):
        cavity += weight * np.maximum(gaussian_filter(height, sigma, mode="wrap") - height, 0)
    # Retain metric response rather than contrast-normalising it: shallow
    # texture must not acquire the same black cavities as deep oak fissures.
    ao = np.exp(-relief * (5.2 * horizon + 2.2 * cavity))
    return np.clip(ao, 0.34, 1.0).astype(np.float32)


def bark_paths(count: int, seed: int, wander: tuple[float, float],
               width: tuple[float, float]) -> tuple[np.ndarray, np.ndarray]:
    """Distance and core masks for irregular periodic vertical fissures."""
    rng = np.random.default_rng(seed)
    y = np.arange(SIZE, dtype=np.float32)
    x = np.arange(SIZE, dtype=np.float32)[None, :]
    distance = np.full((SIZE, SIZE), 1e4, np.float32)
    widths = np.zeros((SIZE, SIZE), np.float32)
    # A closed ring of unequal gaps avoids the picket-fence rhythm produced by
    # jittering a regular grid, while preserving the requested mean spacing.
    gaps = rng.lognormal(0.0, .29, count).astype(np.float32)
    gaps *= SIZE/gaps.sum()
    bases = (np.cumsum(gaps)-gaps*.5+rng.uniform(0, SIZE)) % SIZE
    for i, base in enumerate(bases):
        slow = value_noise((SIZE, 1), (rng.integers(3, 7), 1), seed + 31 + i)[:, 0] - 0.5
        mid = value_noise((SIZE, 1), (rng.integers(9, 16), 1), seed + 101 + i)[:, 0] - 0.5
        cycles = int(rng.integers(1, 4))
        wave = np.sin(2 * np.pi * cycles * y / SIZE + rng.uniform(0, 2*np.pi))
        centre = (base + slow * rng.uniform(*wander) + mid * rng.uniform(5, 13) +
                  wave * rng.uniform(2, 7)) % SIZE
        half = rng.uniform(*width) * (0.72 + 0.55 * value_noise(
            (SIZE, 1), (rng.integers(7, 15), 1), seed + 301 + i)[:, 0])
        d = np.abs(x - centre[:, None])
        d = np.minimum(d, SIZE - d)
        take = d < distance
        distance[take] = d[take]
        widths[take] = np.broadcast_to(half[:, None], d.shape)[take]
    return distance, widths


def _oak_bark_round1() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    rng = np.random.default_rng(1101)
    broad = fbm((SIZE, SIZE), 1111, ((3, 4), (7, 9), (15, 18)))
    medium = fbm((SIZE, SIZE), 1121, ((17, 21), (37, 43), (73, 87)))
    grain = fbm((SIZE, SIZE), 1131, ((67, 73), (137, 149), (251, 269)), 0.44)
    dist, widths = bark_paths(17, 1141, (46, 82), (4.5, 11.5))
    shoulder = widths + 5.0 + 4.0 * broad
    ridge = smoothstep((dist - widths) / np.maximum(shoulder - widths, 1.0))
    ridge = 0.035 + 0.965 * ridge
    # Elongated, randomly seeded cells supply the terminating cross-cracks and
    # blocky plates that a collection of infinite paths cannot produce.
    plate_edge = worley_edge((SIZE, SIZE), (9, 18), 1147)
    cell_crack = 1.0-smoothstep((plate_edge-.025)/.15)
    crack_depth = .20+.62*fbm((SIZE, SIZE), 1149, ((3, 5), (8, 11), (17, 23)))
    plates = 1.0-cell_crack*crack_depth
    ridge = np.minimum(ridge, plates)

    # Secondary paths leave and rejoin a primary rhythm, creating forks.
    branch_lines: list[tuple[list[tuple[float, float]], int]] = []
    for _ in range(23):
        cy = rng.uniform(0, SIZE)
        span = rng.uniform(150, 410)
        cx = rng.uniform(0, SIZE)
        side = rng.choice((-1.0, 1.0))
        pts = []
        for j in range(9):
            u = j / 8
            pts.append((cx + side * rng.uniform(18, 38) * math.sin(math.pi*u) +
                        3*math.sin(5*u+rng.uniform(-.2, .2)), cy + span*(u-.5)))
        branch_lines.append((pts, int(rng.integers(5, 10))))
    branch = periodic_lines(branch_lines, 2.0)
    ridge *= 1.0 - 0.73 * np.clip(branch, 0, 1)

    # Horizontal and diagonal cracks stop within raised plates.
    crack_lines: list[tuple[list[tuple[float, float]], int]] = []
    for _ in range(175):
        cx, cy = rng.uniform(0, SIZE, 2)
        length = rng.uniform(25, 83)
        tilt = rng.normal(0, 0.15)
        pts = []
        for j in range(5):
            u = j / 4 - 0.5
            pts.append((cx + length*u, cy + tilt*length*u +
                        rng.normal(0, 1.8) + 2.0*math.sin(u*5+rng.random())))
        crack_lines.append((pts, int(rng.integers(3, 8))))
    cracks = periodic_lines(crack_lines, 1.35) * smoothstep((ridge - 0.38) / 0.36)
    pits = smoothstep((value_noise((SIZE, SIZE), (93, 101), 1151) - 0.72) / 0.22)
    pits *= smoothstep((ridge - 0.55) / 0.35)
    height = ridge - 0.38 * cracks - 0.15 * pits
    top = smoothstep((ridge - 0.48) / 0.46)
    height += top * (0.11*(broad-.5) + 0.075*(medium-.5) + 0.035*(grain-.5))
    height = normalize01(height, np.percentile(height, .05), np.percentile(height, 99.9))

    furrow = 1.0 - smoothstep((height - .09) / .56)
    weather = smoothstep((height - .42) / .52)
    dark = np.array([45., 35., 29.], np.float32)
    light = np.array([131., 119., 101.], np.float32)
    color = dark + (light-dark) * smoothstep(height)[..., None]
    color += ((broad-.5)*15 + (medium-.5)*13 + (grain-.5)*7)[..., None] * weather[..., None]
    color += furrow[..., None] * np.array([7., 2., -2.])
    lichen_n = fbm((SIZE, SIZE), 1161, ((3, 4), (8, 10), (19, 23)))
    lichen = smoothstep((lichen_n-.62)/.17) * weather * smoothstep((medium-.42)/.35)
    algae = smoothstep((fbm((SIZE, SIZE), 1171, ((4, 3), (11, 8))) - .68)/.15) * furrow
    color = color*(1-.27*lichen[..., None]) + np.array([104., 111., 91.])*(.27*lichen[..., None])
    color = color*(1-.12*algae[..., None]) + np.array([54., 65., 42.])*(.12*algae[..., None])
    ao = horizon_ao(height, 8.2)
    color *= (0.94 + 0.06*ao)[..., None]
    rough = np.clip(.80 + .14*(1-height) + .045*(grain-.5) - .16*lichen, .62, .97)
    return np.clip(color, 28, 158).astype(np.uint8), height, rough, ao


def _linden_bark_round1() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    rng = np.random.default_rng(2101)
    broad = fbm((SIZE, SIZE), 2111, ((3, 4), (8, 9), (17, 19)))
    fibre = fbm((SIZE, SIZE), 2121, ((31, 9), (67, 17), (131, 31)), .47)
    grain = fbm((SIZE, SIZE), 2131, ((61, 73), (129, 143), (249, 263)), .43)
    dist, widths = bark_paths(46, 2141, (18, 39), (1.15, 2.8))
    shoulder = widths + 2.3 + 2.5*broad
    height = .32 + .56*smoothstep((dist-widths)/np.maximum(shoulder-widths, 1))
    fine_edge = worley_edge((SIZE, SIZE), (12, 39), 2147)
    cell_crack = 1.0-smoothstep((fine_edge-.018)/.115)
    net_depth = .11+.27*fbm((SIZE, SIZE), 2149, ((4, 5), (11, 13)))
    fine_net = 1.0-cell_crack*net_depth
    height = np.minimum(height, .08+.86*fine_net)

    # Fine slanting connectors turn the vertical fibres into long diamonds.
    links: list[tuple[list[tuple[float, float]], int]] = []
    for _ in range(330):
        cx, cy = rng.uniform(0, SIZE, 2)
        length = rng.uniform(13, 39)
        slope = rng.choice((-1, 1))*rng.uniform(.24, .62)
        links.append(([(cx-length/2, cy-slope*length/2),
                       (cx, cy+rng.normal(0, 1.0)),
                       (cx+length/2, cy+slope*length/2)], int(rng.integers(1, 4))))
    cross = periodic_lines(links, .75) * smoothstep((height-.45)/.35)
    height -= .13*cross
    height += .055*(broad-.5) + .035*(fibre-.5) + .015*(grain-.5)
    # Some broad, comparatively smooth islands.
    smooth_area = smoothstep((fbm((SIZE, SIZE), 2151, ((3, 5), (7, 11))) - .67)/.18)
    blurred = gaussian_filter(height, 7.0, mode="wrap")
    height = height*(1-.45*smooth_area) + blurred*(.45*smooth_area)
    height = normalize01(height, np.percentile(height, .1), np.percentile(height, 99.9))

    fiss = 1-smoothstep((height-.10)/.56)
    color = np.array([116., 111., 103.]) + ((broad-.5)*18 + (fibre-.5)*13 + (grain-.5)*5)[..., None]
    color -= fiss[..., None]*np.array([45., 42., 38.])
    warm = smoothstep((fbm((SIZE, SIZE), 2161, ((4, 4), (9, 11))) - .58)/.24)
    color += warm[..., None]*np.array([6., 3., -1.])
    lichen = smoothstep((fbm((SIZE, SIZE), 2171, ((3, 4), (8, 9), (18, 21))) - .68)/.14)
    color = color*(1-.17*lichen[..., None]) + np.array([104., 111., 100.])*(.17*lichen[..., None])
    ao = horizon_ao(height, 4.4)
    color *= (.95+.05*ao)[..., None]
    rough = np.clip(.77 + .15*(1-height) + .035*(grain-.5) - .14*lichen, .60, .94)
    return np.clip(color, 43, 158).astype(np.uint8), height, rough, ao


def _birch_bark_round1() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    rng = np.random.default_rng(3101)
    broad = fbm((SIZE, SIZE), 3111, ((3, 4), (7, 9), (15, 19)))
    fine = fbm((SIZE, SIZE), 3121, ((29, 37), (67, 79), (139, 157)), .46)
    yy = np.arange(SIZE, dtype=np.float32)[:, None]
    band = .55*np.sin(2*np.pi*yy/91 + 1.4*value_noise((SIZE, SIZE), (9, 4), 3131))
    band += .24*np.sin(2*np.pi*yy/37 + 3.1)

    lenticels: list[tuple[list[tuple[float, float]], int]] = []
    for row in np.linspace(18, SIZE-15, 31):
        for _ in range(int(rng.integers(4, 10))):
            cx = rng.uniform(0, SIZE)
            cy = row + rng.normal(0, 9)
            length = rng.uniform(12, 67)
            pts = [(cx-length/2, cy+rng.normal(0, .8)), (cx, cy+rng.normal(0, .7)),
                   (cx+length/2, cy+rng.normal(0, .8))]
            lenticels.append((pts, int(rng.integers(2, 6))))
    lent = periodic_lines(lenticels, .65)

    peel_lines: list[tuple[list[tuple[float, float]], int]] = []
    peel_polys: list[list[tuple[float, float]]] = []
    for _ in range(20):
        cx, cy = rng.uniform(0, SIZE, 2)
        length = rng.uniform(55, 190)
        depth = rng.uniform(7, 23)
        n = 9
        upper, lower = [], []
        for j in range(n):
            u = j/(n-1)-.5
            rag = rng.normal(0, 1.8)
            upper.append((cx+length*u, cy + 2.4*math.sin(u*7+rng.random()) + rag))
            lower.append((cx+length*u, cy+depth*(1-4*u*u) + rag+rng.uniform(-1, 2)))
        peel_polys.append(upper + lower[::-1])
        peel_lines.append((upper, int(rng.integers(2, 5))))
        peel_lines.append((lower, int(rng.integers(2, 5))))
    exposed = periodic_shapes(peel_polys, 1.0)
    peel_edge = periodic_lines(peel_lines, .8)

    scar_polys: list[list[tuple[float, float]]] = []
    for _ in range(11):
        cx, cy = rng.uniform(0, SIZE, 2)
        rx, ry = rng.uniform(15, 42), rng.uniform(12, 35)
        points = []
        for j in range(10):
            a = 2*np.pi*j/10
            r = rng.uniform(.75, 1.18)
            # Pointed eye/diamond silhouettes.
            points.append((cx + math.cos(a)*rx*r, cy + math.sin(a)*ry*r*(.60+.40*abs(math.sin(a)))))
        scar_polys.append(points)
    scars = periodic_shapes(scar_polys, 1.1)
    scar_cracks: list[tuple[list[tuple[float, float]], int]] = []
    for poly in scar_polys:
        cx = sum(p[0] for p in poly)/len(poly)
        cy = sum(p[1] for p in poly)/len(poly)
        for _ in range(4):
            ang = rng.uniform(0, 2*np.pi)
            length = rng.uniform(8, 27)
            scar_cracks.append(([(cx, cy), (cx+math.cos(ang)*length, cy+math.sin(ang)*length)], 2))
    scar_fiss = periodic_lines(scar_cracks, .55) * scars

    height = .57 + .035*(broad-.5) + .012*(fine-.5) + .006*band
    height -= .035*lent + .075*scars + .035*exposed
    height += .105*peel_edge + .018*scar_fiss
    height = normalize01(height, np.percentile(height, .05), np.percentile(height, 99.95))

    color = np.empty((SIZE, SIZE, 3), np.float32)
    base = np.array([216., 212., 202.])
    color[:] = base
    color += ((broad-.5)*13 + (fine-.5)*5 + band*2.2)[..., None]
    color -= lent[..., None]*np.array([80., 76., 68.])
    warm_layer = np.array([177., 127., 96.]) + (broad-.5)[..., None]*12
    color = color*(1-.78*exposed[..., None]) + warm_layer*(.78*exposed[..., None])
    scar_col = np.array([61., 55., 48.]) + (fine-.5)[..., None]*18
    color = color*(1-.88*scars[..., None]) + scar_col*(.88*scars[..., None])
    color -= scar_fiss[..., None]*16
    color += peel_edge[..., None]*np.array([7., 7., 5.])
    ao = horizon_ao(height, 3.8)
    color *= (.96+.04*ao)[..., None]
    rough = np.clip(.68 + .10*lent + .16*scars + .06*exposed - .10*peel_edge + .025*(fine-.5), .54, .91)
    return np.clip(color, 36, 229).astype(np.uint8), height, rough, ao


def oak_bark() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """Old oak from warped anisotropic fBm zero-crossings and corky ridges."""
    wx = gradient_fbm(3, 2, 1201, 3)*55 + gradient_fbm(12, 10, 1202, 2)*6
    wy = gradient_fbm(2, 3, 1203, 3)*40
    warp = (wx, wy)

    # The zero set naturally meanders, forks and rejoins.  A spatially varying
    # shoulder keeps the dark channels broad without making stamped outlines.
    network = gradient_fbm(3, 11, 1211, 3, .42, warp)
    width = .29+.15*(gradient_fbm(5, 4, 1213, 2)*.5+.5)
    plate = smoothstep(np.abs(network)/width)**.70
    depth = .80+.20*(gradient_fbm(6, 3, 1217, 2)*.5+.5)

    fibre = ridged(6, 28, 1221, 4, warp)
    cork = gradient_fbm(8, 16, 1223, 4, .60)*.5+.5
    cross_field = gradient_fbm(14, 2, 1229, 2, .50, warp)
    cross_gate = np.clip((gradient_fbm(10, 5, 1231, 2)-.20)*5, 0, 1)
    cross = np.clip(1-np.abs(cross_field)/.08, 0, 1)**2*cross_gate
    surface = .45+.35*fibre+.20*cork
    raw = plate*depth*surface*(1-.45*cross)
    raw = .80*raw+.20*gaussian_filter(plate, 8, mode="wrap")*plate

    pits = np.clip((gradient_fbm(60, 60, 1237, 2)-.40)*5, 0, 1)*plate
    fine = gradient_fbm(96, 160, 1241, 2)
    raw -= .052*pits
    raw += .030*fine
    height = normalize01(raw)
    ao = horizon_ao(height, 4.2)

    top = smoothstep((plate-.28)/.66)
    broad = gradient_fbm(3, 3, 1251, 3)*.5+.5
    medium = gradient_fbm(12, 20, 1253, 3)*.5+.5
    grain = gradient_fbm(128, 128, 1259, 2)*.5+.5
    furrow = np.array([62., 52., 43.], np.float32)
    wall = np.array([112., 100., 86.], np.float32)
    crown = np.array([154., 145., 130.], np.float32)
    lower = furrow+(wall-furrow)*np.clip(height/.48, 0, 1)[..., None]
    upper = wall+(crown-wall)*np.clip((height-.48)/.52, 0, 1)[..., None]
    color = np.where((height < .48)[..., None], lower, upper)
    color *= (.84+.26*broad[..., None])*(.91+.18*medium[..., None])
    color *= .93+.14*grain[..., None]
    # Only gentle cavity darkening belongs in the PBR albedo.
    color *= (.93+.07*ao)[..., None]

    lichen_large = gradient_fbm(5, 5, 1261, 3)
    lichen_detail = gradient_fbm(60, 60, 1267, 3)
    lichen = np.clip((lichen_large-.14)*3.0, 0, 1)
    lichen *= np.clip((lichen_detail+.04)*3.0, 0, 1)*top
    color = color*(1-.35*lichen[..., None])+np.array([105., 111., 91.])*(.35*lichen[..., None])
    cavity = 1-smoothstep(plate/.62)
    rough = np.clip(.82+.12*cavity+.035*(grain-.5)-.055*lichen, .80, .95)
    return np.clip(color, 44, 178).astype(np.uint8), height, rough, ao


def linden_bark() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """Lime bark: finer, shallower zero-crossing fissures and long fibres."""
    wx = gradient_fbm(3, 2, 2201, 3)*34+gradient_fbm(14, 9, 2203, 2)*4
    wy = gradient_fbm(2, 3, 2207, 3)*24
    warp = (wx, wy)
    network = gradient_fbm(4, 24, 2211, 3, .43, warp)
    width = .135+.065*(gradient_fbm(7, 5, 2213, 2)*.5+.5)
    plate = smoothstep(np.abs(network)/width)**.82
    long_fibre = ridged(7, 42, 2221, 4, warp)
    soft_grain = gradient_fbm(15, 34, 2227, 3, .54)*.5+.5
    cross_field = gradient_fbm(18, 3, 2231, 2, .50, warp)
    cross_gate = np.clip((gradient_fbm(12, 6, 2233, 2)-.28)*4, 0, 1)
    cross = np.clip(1-np.abs(cross_field)/.055, 0, 1)**2*cross_gate

    surface = .67+.20*long_fibre+.13*soft_grain
    raw = plate*surface*(1-.20*cross)
    raw = .86*raw+.14*gaussian_filter(plate, 5.0, mode="wrap")*plate
    raw += .018*gradient_fbm(96, 180, 2239, 2)
    height = normalize01(raw)
    ao = horizon_ao(height, 3.0)

    ridge = smoothstep((plate-.10)/.76)
    cavity = 1-ridge
    broad = gradient_fbm(4, 4, 2243, 3)*.5+.5
    fine = gradient_fbm(80, 120, 2249, 2)*.5+.5
    furrow = np.array([62., 60., 56.], np.float32)
    crown = np.array([124., 121., 114.], np.float32)
    color = furrow+(crown-furrow)*ridge[..., None]
    color += ((broad-.5)*15+(soft_grain-.5)*8+(fine-.5)*4)[..., None]
    color *= (.95+.05*ao)[..., None]
    mottle = np.clip((gradient_fbm(5, 6, 2251, 3)-.10)*2.2, 0, 1)
    mottle *= np.clip((gradient_fbm(31, 43, 2257, 2)+.10)*2, 0, 1)
    color = color*(1-.18*mottle[..., None])+np.array([102., 111., 101.])*(.18*mottle[..., None])
    rough = np.clip(.79+.13*cavity+.025*(fine-.5)-.07*mottle, .74, .94)
    return np.clip(color, 52, 145).astype(np.uint8), height, rough, ao


def birch_bark() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """Papery silver birch with ragged lenticels and three torn peel bands."""
    broad = fbm((SIZE, SIZE), 3211, ((2, 3), (5, 7), (11, 15)), .53)
    paper = fbm((SIZE, SIZE), 3221, ((19, 23), (43, 51), (91, 107), (181, 211)), .46)
    grain = fbm((SIZE, SIZE), 3231,
                ((47, 53), (101, 113), (211, 229), (397, 431)), .44)
    # Papery grain has a definite horizontal fibre direction on real birch.
    horizontal = ridged(42, 6, 3237, 3)
    hairline = gradient_fbm(96, 12, 3239, 2, .46)
    rng = np.random.default_rng(3241)
    lent_polys: list[list[tuple[float, float]]] = []
    core_polys: list[list[tuple[float, float]]] = []
    for row in range(22, SIZE, 33):
        group_shift = rng.uniform(-15, 15)
        for _ in range(int(rng.integers(4, 10))):
            cx = rng.uniform(0, SIZE); cy = row+group_shift+rng.normal(0, 7)
            length = rng.uniform(15, 60); thick = rng.uniform(2.0, 5.0)
            slope = rng.normal(0, .018)
            upper: list[tuple[float, float]] = []
            lower: list[tuple[float, float]] = []
            for j in range(8):
                u = j/7*2-1
                taper = max(0.08, (1-u*u)**.55)
                centre_y = cy+slope*length*u*.5+rng.normal(0, .32)
                half = thick*.5*taper*rng.uniform(.78, 1.18)
                x = cx+length*u*.5+rng.normal(0, .24)
                upper.append((x, centre_y-half)); lower.append((x, centre_y+half))
            lent_polys.append(upper+lower[::-1])
            # A broken, narrower dark centre prevents a flat stamped look.
            if length > 23 and rng.random() < .72:
                core_polys.append([(x, cy+(y-cy)*.33) for x, y in
                                   (upper[1:-1]+lower[-2:0:-1])])
    lent = periodic_shapes(lent_polys, .24)
    lent_core = periodic_shapes(core_polys, .16)

    peel_polys: list[list[tuple[float, float]]] = []
    upper_edges: list[tuple[list[tuple[float, float]], int]] = []
    lower_edges: list[tuple[list[tuple[float, float]], int]] = []
    for _ in range(3):
        cx, cy = rng.uniform(0, SIZE, 2)
        length, depth = rng.uniform(170, 340), rng.uniform(10, 28)
        upper: list[tuple[float, float]] = []; lower: list[tuple[float, float]] = []
        for j in range(13):
            u = j/12-.5
            rag = rng.normal(0, 1.5)+1.5*math.sin(u*17+rng.uniform(0, .4))
            upper.append((cx+length*u, cy+rag))
            lower.append((cx+length*u, cy+depth*(1-3.3*u*u)+rag+rng.uniform(-1.5, 1.5)))
        peel_polys.append(upper+lower[::-1])
        upper_edges.append((upper, int(rng.integers(2, 4))))
        lower_edges.append((lower, int(rng.integers(1, 3))))
    peel = periodic_shapes(peel_polys, .55)
    lip = periodic_lines(upper_edges, .45)
    peel_shadow = np.roll(gaussian_filter(lip, (2.2, .8), mode="wrap"), 4, axis=0)
    torn = periodic_lines(lower_edges, .35)

    scar_polys: list[list[tuple[float, float]]] = []
    for _ in range(2):
        cx, cy = rng.uniform(0, SIZE, 2); rx, ry = rng.uniform(29, 54), rng.uniform(20, 39)
        pts = []
        for j in range(24):
            a = 2*np.pi*j/24; r = rng.uniform(.66, 1.24)
            pts.append((cx+math.cos(a)*rx*r, cy+math.sin(a)*ry*r))
        scar_polys.append(pts)
    scar_soft = periodic_shapes(scar_polys, 1.15)
    fray = gradient_fbm(34, 31, 3251, 2)*.5+.5
    scar = smoothstep((scar_soft*(.70+.58*fray)-.07)/.84)
    # A miniature oak-like zero-crossing network, confined to the soft blobs.
    swx = gradient_fbm(5, 4, 3253, 2)*11
    swy = gradient_fbm(4, 5, 3257, 2)*8
    scar_field = gradient_fbm(15, 58, 3259, 3, .44, (swx, swy))
    scar_fiss = np.clip(1-np.abs(scar_field)/.095, 0, 1)**2*scar
    scar_ridge = ridged(25, 68, 3263, 3, (swx, swy))*scar

    raw = (.56+.028*(broad-.5)+.020*(paper-.5)+.014*(grain-.5)
           +.009*(horizontal-.5)+.006*hairline)
    # Lenticels are chiefly pigmentation, not raised bars; keeping their
    # relief tiny avoids the grey normal-map halos seen in the previous round.
    raw += (.009*lent-.010*lent_core-.050*peel+.085*lip-.050*scar
            -.030*scar_fiss+.014*scar_ridge)
    raw += .018*torn
    raw = .91*raw+.09*gaussian_filter(raw, .75, mode="wrap")
    height = normalize01(raw, np.percentile(raw, .05), np.percentile(raw, 99.95))

    base = np.array([216., 212., 202.], np.float32)
    color = np.empty((SIZE, SIZE, 3), np.float32); color[:] = base
    color += ((broad-.5)*11+(paper-.5)*7+(grain-.5)*6
              +(horizontal-.5)*7+hairline*3)[..., None]
    color -= lent[..., None]*np.array([104., 99., 91.])
    color -= lent_core[..., None]*np.array([13., 11., 9.])
    warm_layer = np.array([181., 132., 99.])+(grain-.5)[..., None]*12
    color = color*(1-.72*peel[..., None]) + warm_layer*(.72*peel[..., None])
    color += lip[..., None]*np.array([17., 16., 13.])
    color -= peel_shadow[..., None]*np.array([19., 15., 11.])
    color += torn[..., None]*np.array([5., 2., -1.])
    scar_color = np.array([60., 55., 49.])+(paper-.5)[..., None]*15
    color = color*(1-.80*scar[..., None])+scar_color*(.80*scar[..., None])
    color -= scar_fiss[..., None]*np.array([28., 26., 23.])
    color += scar_ridge[..., None]*np.array([11., 10., 8.])
    # AO uses the unnormalised physical relief.  The exported height still
    # spans 0--255 as required, but tiny paper grain must not occlude like the
    # deep fissures of oak.
    ao = horizon_ao(raw, 2.3)
    color *= (.96+.04*ao)[..., None]
    rough = np.clip(.66 + .10*lent + .18*scar + .07*peel - .10*lip + .03*(paper-.5), .54, .92)
    return np.clip(color, 38, 228).astype(np.uint8), height, rough, ao


BARK_MAKERS = {"oak": oak_bark, "linden": linden_bark, "birch": birch_bark}
BARK_NORMAL_STRENGTH = {"oak": 34.0, "linden": 18.0, "birch": 5.0}


def new_leaf_canvas() -> dict[str, np.ndarray]:
    return {
        "rgb": np.zeros((CELL, CELL, 3), np.float32),
        "alpha": np.zeros((CELL, CELL), np.float32),
        "normal": np.zeros((CELL, CELL, 3), np.float32),
        "rough": np.ones((CELL, CELL), np.float32),
        "trans": np.zeros((CELL, CELL), np.float32),
    }


def composite_surface(canvas: dict[str, np.ndarray], ys: slice, xs: slice,
                      src_rgb: np.ndarray, src_alpha: np.ndarray,
                      src_normal: np.ndarray, src_rough: np.ndarray,
                      src_trans: np.ndarray) -> None:
    da = canvas["alpha"][ys, xs]
    sa = np.clip(src_alpha, 0, 1)
    out_a = sa + da*(1-sa)
    for key, src in (("rgb", src_rgb), ("normal", src_normal)):
        dst = canvas[key][ys, xs]
        num = src*sa[..., None] + dst*da[..., None]*(1-sa[..., None])
        canvas[key][ys, xs] = np.divide(num, out_a[..., None], out=np.zeros_like(num),
                                                   where=out_a[..., None] > 1e-6)
    for key, src in (("rough", src_rough), ("trans", src_trans)):
        dst = canvas[key][ys, xs]
        num = src*sa + dst*da*(1-sa)
        canvas[key][ys, xs] = np.divide(num, out_a, out=np.zeros_like(num), where=out_a > 1e-6)
    canvas["alpha"][ys, xs] = out_a


def draw_branch(canvas: dict[str, np.ndarray], p0: np.ndarray, p1: np.ndarray,
                r0: float, r1: float, color: np.ndarray) -> None:
    pad = max(r0, r1)+2
    xa = max(0, int(min(p0[0], p1[0])-pad)); xb = min(CELL, int(max(p0[0], p1[0])+pad+2))
    ya = max(0, int(min(p0[1], p1[1])-pad)); yb = min(CELL, int(max(p0[1], p1[1])+pad+2))
    if xa >= xb or ya >= yb:
        return
    yy, xx = np.mgrid[ya:yb, xa:xb].astype(np.float32)
    v = p1-p0
    ll = max(float(np.dot(v, v)), 1e-5)
    t = np.clip(((xx-p0[0])*v[0]+(yy-p0[1])*v[1])/ll, 0, 1)
    qx, qy = p0[0]+t*v[0], p0[1]+t*v[1]
    ox, oy = xx-qx, yy-qy
    radius = r0*(1-t)+r1*t
    dist = np.sqrt(ox*ox+oy*oy)
    alpha = np.clip(radius+.72-dist, 0, 1)
    side = np.clip(dist/np.maximum(radius, .1), 0, 1)
    z = np.sqrt(np.clip(1-side*side, 0, 1))
    nx = np.divide(ox, np.maximum(radius, .1), out=np.zeros_like(ox), where=radius>.1)
    ny = -np.divide(oy, np.maximum(radius, .1), out=np.zeros_like(oy), where=radius>.1)
    normal = np.stack((nx, ny, z), -1)
    normal /= np.maximum(np.linalg.norm(normal, axis=-1, keepdims=True), 1e-5)
    rgb = color[None, None, :]*(.76+.24*z[..., None])
    composite_surface(canvas, slice(ya, yb), slice(xa, xb), rgb, alpha, normal,
                      np.full_like(alpha, .90), np.zeros_like(alpha))


def draw_curved_branch(canvas: dict[str, np.ndarray], p0: np.ndarray, pc: np.ndarray,
                       p1: np.ndarray, r0: float, r1: float,
                       color: np.ndarray, steps: int = 10) -> None:
    prev = p0
    for i in range(1, steps+1):
        t = i/steps
        pt = (1-t)**2*p0 + 2*(1-t)*t*pc + t*t*p1
        ta = (i-.5)/steps
        draw_branch(canvas, prev, pt, r0*(1-ta)+r1*ta, r0*(1-t)+r1*t, color)
        prev = pt


def leaf_half_width(species: str, u: np.ndarray, width: float,
                    phase: float) -> tuple[np.ndarray, np.ndarray]:
    u0 = np.clip(u, 0, 1)
    profile = np.maximum(np.sin(np.pi*u0), 0.0)
    if species == "oak":
        core = .53*width*profile**.61*(.61+.62*u0)
        lobes = 5
        modulation = .48+.52*(.5+.5*np.cos(2*np.pi*lobes*u0+phase))**.84
        half = core*modulation + .055*width*np.exp(-((u0-.08)/.04)**2)
        base_cut = np.zeros_like(u)
    elif species == "linden":
        core = .53*width*profile**.50*(1.23-.37*u0)
        core += .115*width*np.exp(-((u0-.12)/.10)**2)
        half = core*(1+.025*np.sin(2*np.pi*19*u0+phase))
        base_cut = .22
    else:
        # Triangular-ovate birch: broad lower third, straight-ish base and a
        # long pointed tip.  The previous symmetric profile read as a coin.
        core = .56*width*profile**.54*(1.48-.88*u0)
        core *= smoothstep(u0/.055)
        serr1 = .065*np.sin(2*np.pi*18*u0+phase)
        serr2 = .026*np.sin(2*np.pi*37*u0+phase*.7)
        half = core*(1+serr1+serr2)
        base_cut = np.zeros_like(u)
    return np.maximum(half, 0), base_cut


def draw_leaf(canvas: dict[str, np.ndarray], species: str, base: np.ndarray,
              length: float, width: float, angle: float, squash: float,
              color: np.ndarray, depth: float, underside: bool,
              spotted: bool, rng: np.random.Generator) -> None:
    direction = np.array([math.cos(angle), math.sin(angle)], np.float32)
    right = np.array([-direction[1], direction[0]], np.float32)
    centre = base + direction*(length*.49)
    effective_width = width*squash
    # Tight rotated bounds avoid evaluating a large empty square around every
    # blade (important for the large foreground leaves in a 4-cell atlas).
    ex = .54*length*abs(direction[0])+.64*effective_width*abs(right[0])+3
    ey = .54*length*abs(direction[1])+.64*effective_width*abs(right[1])+3
    xa = max(0, int(centre[0]-ex)); xb = min(CELL, int(centre[0]+ex+2))
    ya = max(0, int(centre[1]-ey)); yb = min(CELL, int(centre[1]+ey+2))
    if xa >= xb or ya >= yb:
        return
    yy, xx = np.mgrid[ya:yb, xa:xb].astype(np.float32)
    dx, dy = xx-centre[0], yy-centre[1]
    along = dx*direction[0]+dy*direction[1]
    lateral = dx*right[0]+dy*right[1]
    u = along/length+.5
    phase = float(rng.uniform(-.25, .25) if species == "oak" else rng.uniform(0, 2*np.pi))
    half, base_cut = leaf_half_width(species, u, effective_width, phase)
    q = np.clip(np.abs(lateral)/np.maximum(half, .75), 0, 1)
    if species == "linden":
        # The basal notch is deepest at the midrib and asymmetric across it.
        notch = base_cut*(1-q)**2
        asym = np.where(lateral > 0, 1.04, .96)
        edge = half*asym-np.abs(lateral)
        ends = np.minimum((u-notch)*length, (1-u)*length)
    else:
        edge = half-np.abs(lateral)
        ends = np.minimum(u*length, (1-u)*length)
    alpha = np.clip(np.minimum(edge, ends)+.72, 0, 1).astype(np.float32)
    if not np.any(alpha > 0):
        return

    # A foreground blade softly shadows surfaces already in the stack.  This
    # contact term is deliberately local and offset down-right, so overlaps
    # read as a layered spray rather than a pile of flat vector cut-outs.
    shadow = gaussian_filter(alpha, 5.5, mode="constant")
    shadow = np.roll(shadow, (4, 4), (0, 1))
    shadow[:4] = 0; shadow[:, :4] = 0
    existing = canvas["alpha"][slice(ya, yb), slice(xa, xb)]
    canvas["rgb"][slice(ya, yb), slice(xa, xb)] *= (
        1.0-(.10+.09*depth)*shadow[..., None]*existing[..., None])

    dome = np.sqrt(np.clip(1-q*q, 0, 1))*np.maximum(
        np.sin(np.pi*np.clip(u, 0, 1)), 0.0)**.34
    fold = .11*np.tanh(lateral/max(effective_width*.055, .9))*np.sin(np.pi*np.clip(u, 0, 1))
    mid = np.exp(-(lateral/max(.75, effective_width*.026))**2)
    # Secondary veins fan from the midrib towards the margin rather than
    # forming transverse bands.  They are visible, but remain much finer than
    # the fold and silhouette.
    vein = np.zeros_like(u)
    for root in np.linspace(.18, .79, 6 if species != "birch" else 5):
        vein = np.maximum(vein, np.exp(-((u-(root+.12*q))/.012)**2)*q**.55)
    surface = .18*dome + fold - .010*mid - .005*vein
    gy, gx = np.gradient(surface)
    nx, ny = -gx*11.5, gy*11.5  # screen-down gradient maps to OpenGL +Y up
    nz = np.ones_like(nx)
    normal = np.stack((nx, ny, nz), -1)
    normal /= np.maximum(np.linalg.norm(normal, axis=-1, keepdims=True), 1e-5)

    blade = color.copy()
    if underside:
        blade = blade*.73 + np.array([32., 42., 34.])*.27
    base_yellow = (1-np.clip(u, 0, 1))[..., None]*np.array([9.0, 7.0, -3.0])
    edge_light = (q**1.8)[..., None]*2.6
    fine = (np.sin(xx*.69+phase*3)+np.sin(yy*.53-phase*2))*1.1
    half_fold = np.tanh(lateral/max(effective_width*.045, 1.0))
    fold_sign = -1.0 if phase < 0 else 1.0
    fold_tone = 1.0+fold_sign*.085*half_fold*np.sin(np.pi*np.clip(u, 0, 1))
    tip_light = 5.0*smoothstep((u-.35)/.58)
    rgb = blade[None, None, :]*(.95+.07*dome[..., None])*fold_tone[..., None]
    rgb += base_yellow+edge_light+tip_light[..., None]
    rgb += fine[..., None]
    rgb += (mid*7.0+vein*4.5)[..., None]
    if not underside and rng.random() < .34:
        gloss = np.exp(-(((u-.66)/.17)**2+((lateral/effective_width-.16)/.20)**2))
        rgb += (2.0+2.5*depth)*gloss[..., None]
    if spotted:
        spot = np.exp(-(((u-.64)/.075)**2 + ((lateral/effective_width-.12)/.13)**2))
        rgb = rgb*(1-.55*spot[..., None]) + np.array([76., 57., 26.])*(.55*spot[..., None])
    rgb *= (.60+.40*depth)
    rgb = np.clip(rgb, 20, 150)
    rough = np.full_like(alpha, .77 if underside else (.53+.05*(1-depth)))
    trans = np.full_like(alpha, .82 if not underside else .76)
    trans -= .30*mid + .20*vein
    trans = np.clip(trans, .42, .90)
    composite_surface(canvas, slice(ya, yb), slice(xa, xb), rgb, alpha, normal,
                      rough, trans)


def twig_scaffold(species: str, rng: np.random.Generator,
                  variant: int) -> tuple[list[tuple[np.ndarray, np.ndarray, np.ndarray]],
                                         list[tuple[np.ndarray, float]]]:
    """Return curved branches and candidate leaf bases/angles."""
    root = np.array([256., 516.], np.float32)
    junction = np.array([256+rng.uniform(-15, 15), 365+rng.uniform(-12, 12)], np.float32)
    stem_control = np.array([256+rng.uniform(-18, 18), 438+rng.uniform(-8, 8)], np.float32)
    branches: list[tuple[np.ndarray, np.ndarray, np.ndarray]] = [(root, stem_control, junction)]
    candidates: list[tuple[np.ndarray, float]] = []
    nshoot = {"oak": 7, "linden": 7, "birch": 10}[species]
    if variant == 2:
        nshoot -= 1
    # Crown-facing fan: all shoots emerge near the low central junction, then
    # overlap. This reads as a detached leafy spray rather than a small tree.
    fan_angles = np.linspace(-2.92, -.22, nshoot)
    fan_angles += rng.normal(0, .11, nshoot)
    for i, direction in enumerate(fan_angles):
        origin = junction + np.array([rng.uniform(-9, 9), rng.uniform(-18, 7)], np.float32)
        if species == "birch":
            reach = rng.uniform(178, 270)
            tip = origin + np.array([math.cos(direction)*reach,
                                     math.sin(direction)*reach+rng.uniform(30, 82)], np.float32)
            # Arched origin and drooping distal half.
            control = origin + np.array([math.cos(direction)*reach*.62,
                                         math.sin(direction)*reach*.52-rng.uniform(15, 42)], np.float32)
        else:
            reach = rng.uniform(195, 285)
            tip = origin + np.array([math.cos(direction)*reach, math.sin(direction)*reach], np.float32)
            bend = np.array([-math.sin(direction), math.cos(direction)])*rng.uniform(-28, 28)
            control = (origin+tip)*.5+bend
        tip[0] = np.clip(tip[0], 26, 486)
        tip[1] = np.clip(tip[1], 24, 445)
        branches.append((origin, control, tip))
        shoot_angle = math.atan2(tip[1]-origin[1], tip[0]-origin[0])
        if species == "oak":
            # Terminal rosettes plus side leaves hide most of each twig.
            for j in range(6):
                a = shoot_angle + (j-2.5)*.42 + rng.normal(0, .13)
                candidates.append((tip + rng.normal(0, 6, 2), a))
            for u in (.42, .70):
                p = (1-u)**2*origin+2*(1-u)*u*control+u*u*tip
                candidates.append((p, shoot_angle+rng.choice((-1, 1))*rng.uniform(.72, 1.12)))
        elif species == "linden":
            for u in (.28, .55, .80):
                p = (1-u)**2*origin+2*(1-u)*u*control+u*u*tip
                candidates.append((p, shoot_angle+rng.choice((-1, 1))*rng.uniform(.70, 1.18)))
            candidates.append((tip, shoot_angle+rng.normal(0, .18)))
        else:
            for u in (.16, .34, .52, .70, .87):
                p = (1-u)**2*origin+2*(1-u)*u*control+u*u*tip
                candidates.append((p, shoot_angle+rng.choice((-1, 1))*rng.uniform(.65, 1.25)))
            candidates.append((tip, shoot_angle+rng.normal(0, .20)))
    # A few central leaves conceal the junction while preserving open gaps.
    extra = {"oak": 4, "linden": 2, "birch": 5}[species]
    for _ in range(extra):
        p = junction + rng.normal(0, (25, 18), 2)
        candidates.append((p, rng.uniform(-2.85, -.25)))
    return branches, candidates


def _leaf_cell_round1(species: str, seed: int, variant: int) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    rng = np.random.default_rng(seed)
    canvas = new_leaf_canvas()
    twig_color = {"oak": np.array([69., 55., 38.]),
                  "linden": np.array([74., 61., 42.]),
                  "birch": np.array([78., 67., 51.])}[species]
    branches, candidates = twig_scaffold(species, rng, variant)
    for i, (p0, pc, p1) in enumerate(branches):
        if i == 0:
            draw_curved_branch(canvas, p0, pc, p1, 3.7, .95, twig_color, 16)
        else:
            draw_curved_branch(canvas, p0, pc, p1, 1.45, .42, twig_color, 10)

    ranges = {
        "oak": ((45., 62.), (.61, .79), np.array([48., 77., 30.]), np.array([104., 124., 53.])),
        "linden": ((57., 78.), (.75, .94), np.array([55., 91., 35.]), np.array([119., 139., 60.])),
        "birch": ((31., 45.), (.58, .77), np.array([66., 96., 34.]), np.array([132., 145., 58.])),
    }
    length_range, ratio_range, low, high = ranges[species]
    leaves = []
    for base, angle in candidates:
        length = rng.uniform(*length_range)
        width = length*rng.uniform(*ratio_range)
        # Strong foreshortening on roughly a third of leaves.
        narrow_chance = .27 if species == "oak" else .34
        squash = rng.uniform(.27, .60) if rng.random() < narrow_chance else rng.uniform(.62, 1.0)
        depth = rng.uniform(.12, 1.0)
        mix = rng.random()
        color = low*(1-mix)+high*mix
        hue = rng.normal(0, 1)
        color *= np.array([1+.055*hue, 1+.025*hue, 1-.055*hue])
        color *= rng.uniform(.87, 1.14)
        underside = rng.random() < .15
        spotted = rng.random() < .045
        petiole = rng.uniform(4, 9) if species == "oak" else rng.uniform(9, 18)
        actual_base = base + np.array([math.cos(angle), math.sin(angle)])*petiole
        leaves.append((depth, actual_base, length, width, angle+rng.normal(0, .10),
                       squash, color, underside, spotted))
    leaves.sort(key=lambda item: item[0])
    for depth, base, length, width, angle, squash, color, underside, spotted in leaves:
        draw_leaf(canvas, species, base, length, width, angle, squash, color,
                  depth, underside, spotted, rng)

    alpha = np.clip(canvas["alpha"], 0, 1)
    valid = alpha > .01
    if np.any(valid):
        indices = distance_transform_edt(~valid, return_distances=False, return_indices=True)
        filled = canvas["rgb"][indices[0], indices[1]]
    else:
        filled = canvas["rgb"]
    albedo = np.empty((CELL, CELL, 4), np.uint8)
    albedo[..., :3] = np.clip(np.rint(filled), 0, 255).astype(np.uint8)
    albedo[..., 3] = np.clip(np.rint(alpha*255), 0, 255).astype(np.uint8)
    normal = np.clip(np.rint((canvas["normal"]*.5+.5)*255), 0, 255).astype(np.uint8)
    normal[~valid] = (128, 128, 255)
    rough = np.clip(np.rint(canvas["rough"]*255), 0, 255).astype(np.uint8)
    rough[~valid] = 255
    trans = np.clip(np.rint(canvas["trans"]*255), 0, 255).astype(np.uint8)
    trans[~valid] = 0
    return albedo, normal, rough, trans


def dense_twig_scaffold(rng: np.random.Generator, species: str,
                        variant: int) -> tuple[
                            list[tuple[np.ndarray, np.ndarray, np.ndarray]],
                            list[np.ndarray]]:
    """Build a wide asymmetric fan ending in five separated shoot tips."""
    root = np.array([256.+rng.uniform(-5, 5), 499.], np.float32)
    junction = np.array([256.+rng.uniform(-15, 15), 392.+rng.uniform(-8, 8)], np.float32)
    branches = [(root, np.array([252.+rng.uniform(-16, 16), 445.], np.float32), junction)]
    layouts = (
        ((-168, -82), (-105, -207), (-12, -265), (101, -205), (170, -83)),
        ((-176, -111), (-85, -221), (18, -255), (115, -176), (163, -48)),
        ((-153, -68), (-119, -181), (-26, -244), (78, -218), (171, -105)),
        ((-174, -61), (-72, -190), (8, -272), (124, -216), (157, -112)),
    )[variant]
    tips: list[np.ndarray] = []
    for ox, oy in layouts:
        tip = junction+np.array([ox, oy], np.float32)+rng.normal(0, (9, 9), 2)
        if species == "birch":
            # Hanging distal twigs make the birch fan looser and less domed.
            tip[1] += rng.uniform(7, 24)
        tip[0] = np.clip(tip[0], 76, 436); tip[1] = np.clip(tip[1], 86, 330)
        origin = junction+rng.normal(0, (11, 6), 2)
        v = tip-origin
        a = math.atan2(v[1], v[0])
        bend = np.array([-math.sin(a), math.cos(a)], np.float32)*rng.uniform(-30, 30)
        if species == "birch":
            bend[1] -= rng.uniform(8, 24)
        control = (origin+tip)*.5+bend
        branches.append((origin, control, tip))
        tips.append(tip)
    return branches, tips


def leaf_cell(species: str, seed: int, variant: int) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """Render a flattened fan of distinct shoot-tip rosettes and open gaps."""
    rng = np.random.default_rng(seed)
    canvas = new_leaf_canvas()
    twig_color = {"oak": np.array([68., 54., 37.]),
                  "linden": np.array([73., 60., 41.]),
                  "birch": np.array([77., 66., 49.])}[species]
    branches, tips = dense_twig_scaffold(rng, species, variant)
    for i, (p0, pc, p1) in enumerate(branches):
        draw_curved_branch(canvas, p0, pc, p1, 3.3 if i == 0 else 1.25,
                           .82 if i == 0 else .34, twig_color, 14 if i == 0 else 10)

    counts = {"oak": (58, 60, 54, 58),
              "linden": (28, 30, 26, 29),
              "birch": (68, 70, 63, 68)}[species]
    ranges = {
        "oak": ((120., 148.), (.84, 1.02), np.array([43., 70., 29.]), np.array([100., 119., 52.])),
        "linden": ((128., 156.), (.88, 1.02), np.array([49., 84., 34.]), np.array([112., 134., 59.])),
        "birch": ((117., 137.), (.80, 1.0), np.array([59., 91., 34.]), np.array([124., 143., 61.])),
    }
    length_range, ratio_range, low, high = ranges[species]
    junction = branches[0][2]
    leaves = []
    total = counts[variant]
    # Unequal allocation makes the five lobes visibly different; variant 2 is
    # deliberately sparse, with larger notches between its rosettes.
    weights = rng.uniform(.78, 1.22, len(tips)); weights /= weights.sum()
    allocations = np.maximum(3, np.floor(weights*total).astype(int))
    while allocations.sum() < total: allocations[int(rng.integers(0, len(tips)))] += 1
    while allocations.sum() > total:
        k = int(np.argmax(allocations)); allocations[k] -= 1

    for tip_index, (tip, amount) in enumerate(zip(tips, allocations)):
      outward = math.atan2(tip[1]-junction[1], tip[0]-junction[0])
      for j in range(int(amount)):
        # Rosette leaves fan around each terminal bud.  A few side leaves sit
        # back on the shoot, leaving sky holes between neighbouring rosettes.
        angle = outward+rng.uniform(-1.15, 1.15)+rng.normal(0, .10)
        anchor = tip+rng.normal(0, (30, 26), 2)
        if j == 0:  # a solitary outline leaf points decisively outwards
            angle = outward+rng.normal(0, .08)
            anchor = tip+np.array([math.cos(outward), math.sin(outward)])*rng.uniform(4, 11)
        elif j == 1 and tip_index in (1, 3):
            # Two existing leaves are placed low on the inner shoots.  This
            # hides the bare fan of sticks without inflating atlas coverage or
            # exceeding the species leaf-count ranges.
            u = rng.uniform(.06, .22)
            p0, pc, p1 = branches[tip_index+1]
            anchor = (1-u)**2*p0+2*(1-u)*u*pc+u*u*p1
            angle = outward+rng.choice((-1, 1))*rng.uniform(.70, 1.02)
        elif rng.random() < {"oak": .40, "linden": .47, "birch": .59}[species]:
            u = rng.uniform(.22, .82)
            p0, pc, p1 = branches[tip_index+1]
            anchor = (1-u)**2*p0+2*(1-u)*u*pc+u*u*p1
            angle = outward+rng.choice((-1, 1))*rng.uniform(.62, 1.12)
        length = rng.uniform(*length_range)
        width = length*rng.uniform(*ratio_range)
        narrow_chance = {"oak": .24, "linden": .22, "birch": .27}[species]
        squash = rng.uniform(.20, .46) if rng.random() < narrow_chance else rng.uniform(.72, 1.0)
        effective_width = width*squash
        direction = np.array([math.cos(angle), math.sin(angle)], np.float32)
        centre = anchor+direction*(length*.31)
        c, s = abs(math.cos(angle)), abs(math.sin(angle))
        half_factor = {"oak": .56, "linden": .60, "birch": .58}[species]
        ex = .52*length*c+half_factor*effective_width*s
        ey = .52*length*s+half_factor*effective_width*c
        centre[0] = np.clip(centre[0], 9+ex, 503-ex); centre[1] = np.clip(centre[1], 9+ey, 503-ey)
        base = centre-direction*(length*.49)
        layer = int(rng.integers(0, 5))
        depth = np.clip((layer+.25+.5*rng.random())/5, .08, 1.0)
        mix = rng.random()
        color = low*(1-mix)+high*mix
        hue = rng.normal(0, 1)
        color *= np.array([1+.055*hue, 1+.020*hue, 1-.050*hue])
        color *= rng.uniform(.87, 1.14)
        underside = rng.random() < .15
        spotted = rng.random() < .05
        leaves.append((depth, base, length, width, angle, squash, color, underside, spotted))
    leaves.sort(key=lambda item: item[0])
    for item in leaves:
        depth, base, length, width, angle, squash, color, underside, spotted = item
        draw_leaf(canvas, species, base, length, width, angle, squash, color,
                  depth, underside, spotted, rng)

    alpha = np.clip(canvas["alpha"], 0, 1)
    valid = alpha > .01
    indices = distance_transform_edt(~valid, return_distances=False, return_indices=True)
    filled = canvas["rgb"][indices[0], indices[1]]
    albedo = np.empty((CELL, CELL, 4), np.uint8)
    albedo[..., :3] = np.clip(np.rint(filled), 0, 255).astype(np.uint8)
    albedo[..., 3] = np.clip(np.rint(alpha*255), 0, 255).astype(np.uint8)
    normal = np.clip(np.rint((canvas["normal"]*.5+.5)*255), 0, 255).astype(np.uint8)
    normal[~valid] = (128, 128, 255)
    rough = np.clip(np.rint(canvas["rough"]*255), 0, 255).astype(np.uint8); rough[~valid] = 255
    trans = np.clip(np.rint(canvas["trans"]*255), 0, 255).astype(np.uint8); trans[~valid] = 0
    return albedo, normal, rough, trans


def leaf_atlas(species: str, seed: int) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    albedo = np.zeros((ATLAS, ATLAS, 4), np.uint8)
    normal = np.empty((ATLAS, ATLAS, 3), np.uint8); normal[:] = (128, 128, 255)
    rough = np.full((ATLAS, ATLAS), 255, np.uint8)
    trans = np.zeros((ATLAS, ATLAS), np.uint8)
    for variant in range(4):
        a, n, r, t = leaf_cell(species, seed+variant*1009, variant)
        y, x = (variant//2)*CELL, (variant%2)*CELL
        albedo[y:y+CELL, x:x+CELL] = a
        normal[y:y+CELL, x:x+CELL] = n
        rough[y:y+CELL, x:x+CELL] = r
        trans[y:y+CELL, x:x+CELL] = t
    return albedo, normal, rough, trans


def save_image(path: Path, array: np.ndarray) -> None:
    if array.ndim == 2:
        mode = "L"
    elif array.shape[2] == 4:
        mode = "RGBA"
    else:
        mode = "RGB"
    Image.fromarray(array, mode).save(path, compress_level=6)


def as_u8(a: np.ndarray) -> np.ndarray:
    return np.clip(np.rint(a*255), 0, 255).astype(np.uint8)


def lit_rgb(albedo: np.ndarray, normal: np.ndarray, ao: np.ndarray | None = None) -> np.ndarray:
    n = normal.astype(np.float32)/127.5-1.0
    light = np.array([-.579, .579, .574], np.float32)
    ndotl = np.maximum(np.sum(n*light, axis=-1), 0)
    # AO affects only ambient light.  Direct light remains exposed enough to
    # judge the material instead of turning deep bark almost black.
    shade = .45*(ao if ao is not None else 1.0)+.75*ndotl
    return np.clip(albedo.astype(np.float32)*shade[..., None], 0, 255).astype(np.uint8)


def label_panel(image: Image.Image, label: str) -> Image.Image:
    out = Image.new("RGB", (image.width, image.height+25), (32, 34, 33))
    out.paste(image.convert("RGB"), (0, 25))
    ImageDraw.Draw(out).text((7, 6), label, fill=(235, 235, 230), font=ImageFont.load_default())
    return out


def bark_preview(records: dict[str, dict[str, np.ndarray]], path: Path) -> None:
    panel = 384
    rows = []
    for species, d in records.items():
        alb, normal, height, rough, ao = d["albedo"], d["normal"], d["height"], d["rough"], d["ao"]
        lit = lit_rgb(alb, normal, ao)
        Image.fromarray(lit[256:768, 256:768]).save(
            path.parent/f"{species}_lit_crop.png", compress_level=6)
        tiled = np.tile(lit, (2, 2, 1))
        ims = [Image.fromarray(alb), Image.fromarray(lit), Image.fromarray(tiled),
               Image.fromarray(np.repeat(as_u8(height)[..., None], 3, 2)),
               Image.fromarray(np.repeat(as_u8(rough)[..., None], 3, 2)),
               Image.fromarray(np.repeat(as_u8(ao)[..., None], 3, 2))]
        labels = [f"{NAMES[species]} albedo", "lit", "2x2 tile", "height", "roughness", "AO"]
        panels = [label_panel(im.resize((panel, panel), Image.Resampling.LANCZOS), lab)
                  for im, lab in zip(ims, labels)]
        row = Image.new("RGB", (panel*6, panel+25))
        for i, im in enumerate(panels): row.paste(im, (i*panel, 0))
        rows.append(row)
    sheet = Image.new("RGB", (panel*6, (panel+25)*len(rows)))
    for i, row in enumerate(rows): sheet.paste(row, (0, i*(panel+25)))
    sheet.save(path, compress_level=6)


def leaf_preview(records: dict[str, dict[str, np.ndarray]], path: Path) -> None:
    panel = 512
    rows = []
    for species, d in records.items():
        rgba = d["albedo"]
        alpha = rgba[..., 3:4].astype(np.float32)/255
        rgb = rgba[..., :3].astype(np.float32)
        gray = rgb*alpha + np.array([116, 118, 116])*(1-alpha)
        sky = rgb*alpha + np.array([126, 165, 196])*(1-alpha)
        lit = lit_rgb(rgba[..., :3], d["normal"]).astype(np.float32)
        lit = lit*alpha + np.array([105, 108, 106])*(1-alpha)
        trans = np.repeat(d["trans"][..., None], 3, 2)
        ims = [Image.fromarray(np.uint8(gray)), Image.fromarray(np.uint8(sky)),
               Image.fromarray(np.uint8(lit)), Image.fromarray(trans)]
        labels = [f"{NAMES[species]} / grey", "sky", "lit", "translucency"]
        panels = [label_panel(im.resize((panel, panel), Image.Resampling.LANCZOS), lab)
                  for im, lab in zip(ims, labels)]
        row = Image.new("RGB", (panel*4, panel+25))
        for i, im in enumerate(panels): row.paste(im, (i*panel, 0))
        rows.append(row)
    sheet = Image.new("RGB", (panel*4, (panel+25)*len(rows)))
    for i, row in enumerate(rows): sheet.paste(row, (0, i*(panel+25)))
    sheet.save(path, compress_level=6)


def generate(out_dir: Path, selected: list[str], preview: bool) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    bark_records: dict[str, dict[str, np.ndarray]] = {}
    leaf_records: dict[str, dict[str, np.ndarray]] = {}
    for species in selected:
        name = NAMES[species]
        albedo, height, rough, ao = BARK_MAKERS[species]()
        normal = normal_from_height(height, BARK_NORMAL_STRENGTH[species])
        save_image(out_dir/f"{name}Bark.png", albedo)
        save_image(out_dir/f"{name}BarkNormal.png", normal)
        save_image(out_dir/f"{name}BarkRoughness.png", as_u8(rough))
        save_image(out_dir/f"{name}BarkAO.png", as_u8(ao))
        save_image(out_dir/f"{name}BarkHeight.png", as_u8(height))
        bark_records[species] = {"albedo": albedo, "normal": normal, "height": height,
                                 "rough": rough, "ao": ao}
        print(f"{name} bark complete")

    seeds = {"oak": 4101, "linden": 5101, "birch": 6101}
    for species in selected:
        name = NAMES[species]
        albedo, normal, rough, trans = leaf_atlas(species, seeds[species])
        save_image(out_dir/f"{name}Leaves.png", albedo)
        save_image(out_dir/f"{name}LeavesNormal.png", normal)
        save_image(out_dir/f"{name}LeavesRoughness.png", rough)
        save_image(out_dir/f"{name}LeavesTranslucency.png", trans)
        leaf_records[species] = {"albedo": albedo, "normal": normal,
                                 "rough": rough, "trans": trans}
        coverage = float(np.mean(albedo[..., 3] > 127))*100
        print(f"{name} leaves complete ({coverage:.1f}% opaque coverage)")

    if preview:
        preview_dir = out_dir/"preview"
        preview_dir.mkdir(parents=True, exist_ok=True)
        bark_preview(bark_records, preview_dir/"bark_sheet.png")
        leaf_preview(leaf_records, preview_dir/"leaf_sheet.png")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("out_dir", type=Path)
    parser.add_argument("--only", choices=SPECIES)
    parser.add_argument("--preview", action="store_true")
    args = parser.parse_args()
    selected = [args.only] if args.only else list(SPECIES)
    start = time.perf_counter()
    generate(args.out_dir, selected, args.preview)
    count = len(selected)*9
    if args.preview:
        count += 2+len(selected)
    print(f"Generated {count} files in {time.perf_counter()-start:.2f} s")


if __name__ == "__main__":
    main()
