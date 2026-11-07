"""Generate original, seamless PBR rock, moss, and lichen textures.

Usage::

    python rock_textures.py <out_dir> [--preview]

The script writes 1024 x 1024 Albedo (sRGB), OpenGL tangent-space Normal
(+X right, +Y toward smaller image rows, +Z out), Roughness, AO, and 16-bit
Height maps for Granite, Sandstone, Cobble, Moss, and Lichen.  Lichen albedo
is straight-alpha RGBA.  One tile represents respectively 1.0 m, 1.5 m,
0.5 m, 0.5 m, and 0.6 m.  Every field and filter is periodic on both axes.

All imagery is synthesised from fixed-seed periodic noise and analytic shapes;
no source imagery is used.  The generated imagery may be released as CC0.

These tileable sets are the inputs of the sample rocks' texture bake
(``crates/misty_editor/src/sample_rocks``): ``python tools/rock_textures.py
target/rock-textures``, then ``cargo test --release -p misty_editor --
--ignored write_rocks``.  Written with OpenAI Codex (two review rounds), see
``docs/pending/rocks.md``.
"""

from __future__ import annotations

import argparse
import math
import time
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy.ndimage import gaussian_filter, gaussian_filter1d

SIZE = 1024
SETS = ("Granite", "Sandstone", "Cobble", "Moss", "Lichen")
TILE_METRES = {"Granite": 1.0, "Sandstone": 1.5, "Cobble": 0.5,
                "Moss": 0.5, "Lichen": 0.6}


def smoothstep(x: np.ndarray) -> np.ndarray:
    x = np.clip(x, 0.0, 1.0)
    return x*x*(3.0-2.0*x)


def normalize01(a: np.ndarray, p_lo: float = 0.05, p_hi: float = 99.95) -> np.ndarray:
    lo, hi = np.percentile(a, (p_lo, p_hi))
    return np.clip((a-lo)/max(float(hi-lo), 1e-6), 0, 1).astype(np.float32)


def value_noise(grid: tuple[int, int], seed: int,
                shape: tuple[int, int] = (SIZE, SIZE)) -> np.ndarray:
    """Smooth, exactly periodic bilinear value noise."""
    h, w = shape
    gy, gx = grid
    v = np.random.default_rng(seed).random((gy, gx), dtype=np.float32)
    py = np.arange(h, dtype=np.float32)*gy/h
    px = np.arange(w, dtype=np.float32)*gx/w
    iy, ix = np.floor(py).astype(np.int32), np.floor(px).astype(np.int32)
    fy, fx = smoothstep(py-iy)[:, None], smoothstep(px-ix)[None, :]
    a = v[iy[:, None] % gy, ix[None, :] % gx]
    b = v[iy[:, None] % gy, (ix[None, :]+1) % gx]
    c = v[(iy[:, None]+1) % gy, ix[None, :] % gx]
    d = v[(iy[:, None]+1) % gy, (ix[None, :]+1) % gx]
    return ((a*(1-fx)+b*fx)*(1-fy)+(c*(1-fx)+d*fx)*fy).astype(np.float32)


def fbm(seed: int, grids: tuple[tuple[int, int], ...], persistence: float = .5) -> np.ndarray:
    out = np.zeros((SIZE, SIZE), np.float32)
    weight = total = 0.0
    weight = 1.0
    for i, grid in enumerate(grids):
        out += weight*value_noise(grid, seed+i*1009)
        total += weight
        weight *= persistence
    return out/total


def cellular(grid: tuple[int, int], seed: int) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """Toroidal Worley F1/F2 plus the closest site's two random attributes."""
    gy, gx = grid
    rng = np.random.default_rng(seed)
    oy = rng.uniform(.12, .88, (gy, gx)).astype(np.float32)
    ox = rng.uniform(.12, .88, (gy, gx)).astype(np.float32)
    va = rng.random((gy, gx), dtype=np.float32)
    vb = rng.random((gy, gx), dtype=np.float32)
    y = np.arange(SIZE, dtype=np.float32)[:, None]*gy/SIZE
    x = np.arange(SIZE, dtype=np.float32)[None, :]*gx/SIZE
    iy, ix = np.floor(y).astype(np.int32), np.floor(x).astype(np.int32)
    f1 = np.full((SIZE, SIZE), 1e6, np.float32)
    f2 = np.full_like(f1, 1e6)
    aout = np.zeros_like(f1)
    bout = np.zeros_like(f1)
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            jy, jx = iy+dy, ix+dx
            py = jy+oy[jy % gy, jx % gx]
            px = jx+ox[jy % gy, jx % gx]
            dist = np.sqrt((py-y)**2+(px-x)**2).astype(np.float32)
            nearer = dist < f1
            f2 = np.where(nearer, f1, np.minimum(f2, dist))
            f1 = np.minimum(f1, dist)
            aout = np.where(nearer, va[jy % gy, jx % gx], aout)
            bout = np.where(nearer, vb[jy % gy, jx % gx], bout)
    return f1, f2, aout, bout


def periodic_lines(seed: int, count: int, width: tuple[int, int],
                   orientation: str = "free", blur: float = 0.0) -> np.ndarray:
    """Rasterise irregular polylines and their eight toroidal neighbours."""
    rng = np.random.default_rng(seed)
    image = Image.new("L", (SIZE, SIZE), 0)
    draw = ImageDraw.Draw(image)
    for _ in range(count):
        if orientation == "horizontal":
            x0, y0 = rng.uniform(0, SIZE), rng.uniform(0, SIZE)
            length, angle = rng.uniform(80, 390), rng.normal(0, .10)
        elif orientation == "vertical":
            x0, y0 = rng.uniform(0, SIZE), rng.uniform(0, SIZE)
            length, angle = rng.uniform(100, 440), math.pi/2+rng.normal(0, .12)
        else:
            x0, y0 = rng.uniform(0, SIZE), rng.uniform(0, SIZE)
            length, angle = rng.uniform(50, 260), rng.uniform(0, 2*math.pi)
        normal = np.array([-math.sin(angle), math.cos(angle)])
        points = []
        for j in range(9):
            u = j/8-.5
            along = np.array([math.cos(angle), math.sin(angle)])*length*u
            wander = normal*(math.sin((u+.5)*math.pi*2*rng.integers(1, 3)+rng.uniform(-.3, .3))
                              * rng.uniform(2, 9)+rng.normal(0, 1.2))
            points.append((x0+along[0]+wander[0], y0+along[1]+wander[1]))
        line_width = int(rng.integers(width[0], width[1]+1))
        for sy in (-SIZE, 0, SIZE):
            for sx in (-SIZE, 0, SIZE):
                draw.line([(x+sx, y+sy) for x, y in points], fill=255,
                          width=line_width, joint="curve")
    result = np.asarray(image, np.float32)/255
    return gaussian_filter(result, blur, mode="wrap") if blur else result


def speckle(seed: int, density: float, sigma: float = .45) -> np.ndarray:
    rng = np.random.default_rng(seed)
    impulses = (rng.random((SIZE, SIZE)) < density).astype(np.float32)
    strength = rng.uniform(.35, 1.0, (SIZE, SIZE)).astype(np.float32)
    return gaussian_filter(impulses*strength, sigma, mode="wrap")


def normal_from_height(height: np.ndarray, strength: float) -> np.ndarray:
    dx = .5*(np.roll(height, -1, 1)-np.roll(height, 1, 1))
    dy_up = .5*(np.roll(height, 1, 0)-np.roll(height, -1, 0))
    nx, ny = -dx*strength, -dy_up*strength
    inv = 1/np.sqrt(nx*nx+ny*ny+1)
    n = np.stack((nx*inv, ny*inv, inv), axis=-1)
    return np.clip(np.rint((n*.5+.5)*255), 0, 255).astype(np.uint8)


def cavity_ao(height: np.ndarray, amount: float, floor: float = .68) -> np.ndarray:
    cavity = np.zeros_like(height)
    for sigma, weight in ((2.0, .45), (7.0, .34), (22.0, .21)):
        cavity += weight*np.maximum(gaussian_filter(height, sigma, mode="wrap")-height, 0)
    ao = 1-amount*cavity
    return np.clip(ao, floor, 1).astype(np.float32)


def granite() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    broad = fbm(101, ((3, 4), (7, 9), (15, 18)))
    mid = fbm(111, ((17, 19), (39, 43), (79, 83)))
    # Two crystal populations break the regular "one grain per cell" scale.
    # Cell edges affect colour only: putting a Worley boundary in the height is
    # what makes granite look like paving stones under directional light.
    _, _, mineral_fine, tint_fine = cellular((218, 211), 121)
    _, _, mineral_coarse, tint_coarse = cellular((112, 119), 122)
    scale_mix = fbm(123, ((11, 13), (29, 31))) > .50
    mineral = np.where(scale_mix, mineral_coarse, mineral_fine)
    tint = np.where(scale_mix, tint_coarse, tint_fine)
    quartz = mineral < .34
    biotite = mineral > .925
    feldspar = ~(quartz | biotite)
    color = np.empty((SIZE, SIZE, 3), np.float32)
    color[:] = (154, 146, 135)
    color += ((broad-.5)*20+(mid-.5)*5)[..., None]*np.array([1.0, .95, .78])
    # Cream/pink feldspar is the matrix, quartz is cooler and glassier, and
    # mica occupies small irregular grains rather than identical black dots.
    color += feldspar[..., None]*((tint-.5)*14)[..., None]*np.array([1.0, .72, .55])
    color += quartz[..., None]*(np.array([-14, -7, 1])+(tint-.5)[..., None]*10)
    color -= biotite[..., None]*(np.array([88, 83, 73])+(tint-.5)[..., None]*10)
    # Sparse 1-2 cm feldspar phenocrysts, eroded by fine noise so their edges
    # remain angular/irregular instead of reading as perfect Voronoi tiles.
    pf1, _, pvar, _ = cellular((51, 47), 126)
    phenocrysts = smoothstep((.42+.08*pvar-pf1)/.10)*(pvar > .86)
    phenocrysts *= smoothstep((value_noise((171, 179), 127)-.22)/.42)
    color = color*(1-.62*phenocrysts[..., None])+np.array([173, 153, 139])*(.62*phenocrysts[..., None])
    for channel in range(3):
        color[..., channel] = gaussian_filter(color[..., channel], .38, mode="wrap")
    # Extra sub-grain mica flakes, weathered pits, hairline cracks and iron.
    black = np.clip(speckle(131, .004, .52)*2.2, 0, 1)
    color -= black[..., None]*np.array([62, 60, 54])
    pits = smoothstep((value_noise((91, 97), 141)-.86)/.12)
    cracks = periodic_lines(151, 5, (1, 2), "free", .65)
    stains = periodic_lines(161, 11, (5, 13), "vertical", 8.0)*smoothstep((broad-.42)/.34)
    color = color*(1-.12*stains[..., None])+np.array([139, 104, 72])*(.12*stains[..., None])
    color -= pits[..., None]*np.array([15, 14, 12])
    # Relief deliberately ignores cell boundaries.  Broad weathering dominates;
    # mineral masks are softened so quartz sits just proud and mica just low.
    weather = gaussian_filter(.62*broad+.38*mid, 3.2, mode="wrap")
    quartz_relief = gaussian_filter(quartz.astype(np.float32), 1.15, mode="wrap")
    mica_relief = gaussian_filter(biotite.astype(np.float32), .85, mode="wrap")
    micro = fbm(171, ((139, 149), (281, 293), (439, 449)), .42)
    height = (.50+.115*(weather-.5)+.018*(micro-.5)-.12*pits-.065*cracks+
              .017*quartz_relief-.026*mica_relief+.012*phenocrysts)
    height = normalize01(gaussian_filter(height, .60, mode="wrap"), .02, 99.98)
    ao = cavity_ao(height, 1.35, .73)
    rough = np.clip(.82+.07*(1-height)+.025*(mid-.5)+.04*pits, .75, .91)
    return np.clip(color, 18, 205).astype(np.uint8), height, rough, ao


def _sample_periodic_profile(profile: np.ndarray, q: np.ndarray) -> np.ndarray:
    q = np.mod(q, len(profile))
    i = np.floor(q).astype(np.int32)
    f = q-i
    return profile[i]*(1-f)+profile[(i+1) % len(profile)]*f


def sandstone() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    rng = np.random.default_rng(201)
    y, x = np.mgrid[:SIZE, :SIZE].astype(np.float32)
    waviness = (value_noise((5, 9), 211)-.5)*22+(value_noise((12, 23), 212)-.5)*7
    qy = y+waviness
    profile = rng.normal(0, 1, SIZE).astype(np.float32)
    profile = gaussian_filter1d(profile, 3.2, mode="wrap")
    profile += .62*gaussian_filter1d(rng.normal(0, 1, SIZE), 10.5, mode="wrap")
    profile = (profile-profile.mean())/max(profile.std(), 1e-6)
    layers = _sample_periodic_profile(profile, qy)
    fine = fbm(221, ((83, 89), (173, 181), (313, 327)), .44)
    broad = fbm(231, ((3, 4), (8, 10), (17, 21)))
    # Two broad soft beds and thin ferric laminae follow the same warped y coordinate.
    recessed = np.zeros((SIZE, SIZE), np.float32)
    iron = np.zeros_like(recessed)
    for centre, width in ((173, 14), (687, 21)):
        d = np.minimum(np.abs(np.mod(qy-centre, SIZE)), SIZE-np.abs(np.mod(qy-centre, SIZE)))
        recessed += np.exp(-.5*(d/width)**2)
    for centre, width in ((82, 4.0), (452, 6.0), (823, 3.2)):
        d = np.minimum(np.abs(np.mod(qy-centre, SIZE)), SIZE-np.abs(np.mod(qy-centre, SIZE)))
        iron += np.exp(-.5*(d/width)**2)
    # Local cross-bedded ripples, faded in and out rather than hard wedges.
    wedge = smoothstep((value_noise((3, 4), 241)-.53)/.19)
    diagonal = np.sin(2*np.pi*(y/42+x/205+value_noise((7, 9), 242)*.24))
    cross = wedge*diagonal
    # Soft weathered zones interrupt the bed contrast and stop the tile from
    # reading as a perfectly stacked contour drawing.
    disruption = smoothstep((fbm(244, ((3, 5), (7, 11), (15, 23)))-.58)/.25)
    layers *= 1-.42*disruption
    pockets = smoothstep((value_noise((59, 67), 251)-.83)/.13)
    color = np.zeros((SIZE, SIZE, 3), np.float32)+np.array([153, 143, 122])
    color += layers[..., None]*np.array([6.2, 4.3, 1.7])
    color += (broad-.5)[..., None]*np.array([11, 9, 5])
    color += (fine-.5)[..., None]*np.array([10, 9, 7])
    color += cross[..., None]*np.array([5.0, 3.4, 1.7])
    color -= recessed[..., None]*np.array([17, 15, 12])
    color = color*(1-.20*iron[..., None])+np.array([164, 102, 55])*(.20*iron[..., None])
    darkgrain = speckle(261, .018, .4)
    color -= darkgrain[..., None]*np.array([42, 37, 29])
    height = .52+.038*layers+.025*cross+.025*(fine-.5)-.14*recessed-.07*pockets
    height = normalize01(gaussian_filter(height, .35, mode="wrap"), .02, 99.98)
    ao = cavity_ao(height, 1.05, .76)
    rough = np.clip(.90+.035*(1-height)+.018*(fine-.5), .85, .96)
    return np.clip(color, 35, 208).astype(np.uint8), height, rough, ao


def cobble() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    broad = fbm(301, ((3, 3), (7, 8), (15, 17)))
    mid = fbm(311, ((19, 23), (41, 47), (83, 91)))
    fine = fbm(321, ((107, 113), (223, 229), (397, 409)), .43)
    color = np.zeros((SIZE, SIZE, 3), np.float32)+np.array([119, 120, 116])
    color += (broad-.5)[..., None]*np.array([22, 18, 13])
    color += (mid-.5)[..., None]*np.array([9, 10, 10])
    color += (fine-.5)[..., None]*np.array([5, 5, 5])
    brown = smoothstep((fbm(331, ((3, 4), (8, 9), (17, 19)))-.60)/.22)
    color = color*(1-.15*brown[..., None])+np.array([126, 105, 82])*(.15*brown[..., None])
    veins = periodic_lines(341, 5, (2, 5), "free", 1.1)
    veins *= smoothstep((value_noise((7, 8), 342)-.23)/.35)
    color = color*(1-.72*veins[..., None])+np.array([188, 187, 173])*(.72*veins[..., None])
    flecks = speckle(351, .009, .5)
    color += flecks[..., None]*np.array([27, 28, 26])
    height = .50+.045*(broad-.5)+.035*(mid-.5)+.022*(fine-.5)+.025*veins
    height = normalize01(gaussian_filter(height, .55, mode="wrap"), .02, 99.98)
    ao = cavity_ao(height, .52, .86)
    rough = np.clip(.64+.055*(mid-.5)+.035*(1-height), .55, .75)
    return np.clip(color, 40, 205).astype(np.uint8), height, rough, ao


def moss() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    # Packed circular clumps make real rounded domes rather than Voronoi slabs.
    # A few small clumps occupy spaces between a jittered set of larger ones.
    rng = np.random.default_rng(401)
    yy, xx = np.mgrid[:SIZE, :SIZE].astype(np.float32)
    top = np.zeros((SIZE, SIZE), np.float32)
    second = np.zeros_like(top)
    centres: list[tuple[float, float, float]] = []
    for gy in range(9):
        for gx in range(10):
            cy = (gy+rng.uniform(.12, .88))*SIZE/9
            cx = (gx+rng.uniform(.12, .88))*SIZE/10
            centres.append((cy, cx, rng.uniform(52, 92)))
    centres.extend((rng.uniform(0, SIZE), rng.uniform(0, SIZE), rng.uniform(31, 54))
                   for _ in range(20))
    cushion_warp = fbm(402, ((13, 17), (29, 35), (59, 67)), .48)-.5
    for cy, cx, radius in centres:
        dy = np.mod(yy-cy+SIZE/2, SIZE)-SIZE/2
        dx = np.mod(xx-cx+SIZE/2, SIZE)-SIZE/2
        local_radius = radius*(1+.18*cushion_warp)
        profile = np.exp(-1.75*(dx*dx+dy*dy)/(local_radius*local_radius))
        higher = profile > top
        second = np.where(higher, top, np.maximum(second, profile))
        top = np.maximum(top, profile)
    dome = gaussian_filter(top, 1.2, mode="wrap")
    # Low saddles between neighbouring radial profiles become narrow crevices;
    # there is no hard circular support edge to make the clumps look like stones.
    gaps = gaussian_filter(smoothstep((.34-dome)/.24), .85, mode="wrap")
    shoot_f1, _, shoot_var, dry_var = cellular((166, 159), 406)
    shoot_crowns = np.exp(-((shoot_f1/(.28+.10*shoot_var))**2))
    micro = fbm(411, ((109, 113), (211, 223), (401, 419)), .46)
    shoots = speckle(421, .052, .50)
    tips = .58*shoot_crowns+.27*smoothstep((micro-.52)/.30)+np.clip(shoots*2.0, 0, .48)
    dry = smoothstep((fbm(431, ((3, 4), (7, 9), (15, 17)))-.67)/.18)
    dry *= smoothstep((dry_var-.35)/.55)
    base = np.array([74, 79, 34], np.float32)
    color = np.zeros((SIZE, SIZE, 3), np.float32)+base
    color += dome[..., None]*np.array([17, 17, 5])
    color += (micro-.5)[..., None]*np.array([16, 17, 7])
    color += np.clip(tips, 0, 1)[..., None]*np.array([13, 13, 5])
    color -= gaps[..., None]*np.array([30, 29, 11])
    color = color*(1-.55*dry[..., None])+np.array([111, 91, 45])*(.55*dry[..., None])
    height = (.25+.56*dome+.055*(micro-.5)+.072*shoot_crowns+
              .04*np.clip(shoots*3, 0, 1)-.16*gaps)
    height = normalize01(gaussian_filter(height, .45, mode="wrap"), .02, 99.98)
    ao = cavity_ao(height, 2.0, .63)
    rough = np.clip(.95+.035*(1-height)+.018*(micro-.5), .90, 1.0)
    return np.clip(color, 18, 156).astype(np.uint8), height, rough, ao


def lichen() -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    # Independent periodic rosettes provide a true size distribution.  Their
    # lobed analytic margins overlap into colonies while small ones remain dots.
    rng = np.random.default_rng(501)
    yy, xx = np.mgrid[:SIZE, :SIZE].astype(np.float32)
    best = np.full((SIZE, SIZE), -1e6, np.float32)
    best_radial = np.ones((SIZE, SIZE), np.float32)
    colony_tone = np.zeros((SIZE, SIZE), np.float32)
    colony_ochre = np.zeros((SIZE, SIZE), np.float32)
    colony_cracked = np.zeros((SIZE, SIZE), np.float32)
    radii = np.concatenate((rng.uniform(5, 17, 70), rng.uniform(18, 48, 54),
                            rng.uniform(50, 103, 18))).astype(np.float32)
    rng.shuffle(radii)
    edge_warp = fbm(511, ((17, 19), (37, 41), (73, 79)), .50)-.5
    for radius in radii:
        cy, cx = rng.uniform(0, SIZE, 2)
        dy = np.mod(yy-cy+SIZE/2, SIZE)-SIZE/2
        dx = np.mod(xx-cx+SIZE/2, SIZE)-SIZE/2
        distance = np.sqrt(dx*dx+dy*dy)
        theta = np.arctan2(dy, dx)
        lobes = int(rng.integers(5, 12))
        phase = rng.uniform(0, 2*math.pi)
        boundary_radius = radius*(1+.075*np.sin(lobes*theta+phase)+
                                  .030*np.sin((lobes+3)*theta-phase*.7)+
                                  .105*edge_warp)
        margin = boundary_radius-distance
        nearer = margin > best
        radial = distance/np.maximum(boundary_radius, 1)
        best = np.where(nearer, margin, best)
        best_radial = np.where(nearer, radial, best_radial)
        colony_tone = np.where(nearer, rng.uniform(-1, 1), colony_tone)
        colony_ochre = np.where(nearer, rng.random() < .13, colony_ochre)
        colony_cracked = np.where(nearer, (radius > 48 and rng.random() < .45), colony_cracked)
    hard_coverage = best > 0
    alpha = smoothstep((gaussian_filter(hard_coverage.astype(np.float32), 1.05,
                                        mode="wrap")-.04)/.92)
    coverage = alpha > .5
    # A dark growing margin and pale mature centre replace repeated bullseyes.
    rim = np.exp(-((best_radial-.86)/.105)**2)
    centre = smoothstep((.72-best_radial)/.55)
    texture = fbm(521, ((39, 43), (89, 97), (181, 193)), .46)
    pale = np.array([165, 170, 145], np.float32)
    ochre = np.array([190, 160, 75], np.float32)
    ochre_mask = colony_ochre
    color = pale[None, None, :]*(1-ochre_mask[..., None])+ochre[None, None, :]*ochre_mask[..., None]
    color += colony_tone[..., None]*np.array([8, 7, 6])
    color += (texture-.5)[..., None]*np.array([16, 15, 12])
    color += centre[..., None]*np.array([9, 9, 7])-rim[..., None]*np.array([18, 17, 13])
    # Occasional areolate cracking is confined to the centres of large colonies.
    af1, af2, _, _ = cellular((79, 83), 525)
    crack_lines = np.exp(-((af2-af1)/.045)**2)
    cracks = crack_lines*colony_cracked*smoothstep((.72-best_radial)/.18)*coverage
    color -= cracks[..., None]*np.array([24, 23, 18])
    spots = np.clip(speckle(531, .010, .72)*2.3, 0, .92)*coverage
    color = color*(1-spots[..., None])+np.array([55, 50, 40])*spots[..., None]
    # RGB intentionally remains valid beyond alpha (a broad colour dilation).
    height = alpha*(.60+.075*(texture-.5)+.065*rim-.045*cracks)
    height = normalize01(gaussian_filter(height, .45, mode="wrap"), 0, 100)
    ao = cavity_ao(height, .72, .82)
    rough = np.clip(.87+.055*(texture-.5)+.025*rim, .80, .95)
    rgba = np.dstack((np.clip(color, 38, 220).astype(np.uint8),
                      np.clip(np.rint(alpha*255), 0, 255).astype(np.uint8)))
    return rgba, height, rough, ao, alpha


def as_u8(a: np.ndarray) -> np.ndarray:
    return np.clip(np.rint(a*255), 0, 255).astype(np.uint8)


def save_maps(out_dir: Path, name: str, albedo: np.ndarray, height: np.ndarray,
              rough: np.ndarray, ao: np.ndarray, normal_strength: float) -> dict[str, np.ndarray]:
    normal = normal_from_height(height, normal_strength)
    Image.fromarray(albedo, "RGBA" if albedo.shape[2] == 4 else "RGB").save(out_dir/f"{name}_Albedo.png")
    Image.fromarray(normal, "RGB").save(out_dir/f"{name}_Normal.png")
    Image.fromarray(as_u8(rough), "L").save(out_dir/f"{name}_Roughness.png")
    Image.fromarray(as_u8(ao), "L").save(out_dir/f"{name}_AO.png")
    Image.fromarray(np.clip(np.rint(height*65535), 0, 65535).astype(np.uint16)).save(out_dir/f"{name}_Height.png")
    return {"albedo": albedo, "normal": normal, "rough": rough, "ao": ao, "height": height}


def _rgb_on_checker(albedo: np.ndarray) -> np.ndarray:
    if albedo.shape[2] == 3:
        return albedo
    yy, xx = np.mgrid[:SIZE, :SIZE]
    checker = np.where(((xx//32+yy//32) & 1)[..., None], 102, 132).astype(np.float32)
    a = albedo[..., 3:4].astype(np.float32)/255
    return np.uint8(albedo[..., :3]*a+checker*(1-a))


def lit_preview(d: dict[str, np.ndarray]) -> np.ndarray:
    rgb = _rgb_on_checker(d["albedo"]).astype(np.float32)
    n = d["normal"].astype(np.float32)/127.5-1
    light = np.array([-.62, .55, .56], np.float32)
    light /= np.linalg.norm(light)
    ndotl = np.maximum(np.sum(n*light, axis=-1), 0)
    shade = .42*d["ao"]+.73*ndotl
    lit = np.clip(rgb*shade[..., None], 0, 255)
    if d["albedo"].shape[2] == 4:
        a = d["albedo"][..., 3:4].astype(np.float32)/255
        lit = lit*a+np.array([105, 106, 102])*(1-a)
    return lit.astype(np.uint8)


def labelled(image: Image.Image, text: str, side: int = 224) -> Image.Image:
    image = image.convert("RGB").resize((side, side), Image.Resampling.LANCZOS)
    panel = Image.new("RGB", (side, side+24), (31, 33, 31))
    panel.paste(image, (0, 24))
    ImageDraw.Draw(panel).text((6, 6), text, fill=(235, 234, 226), font=ImageFont.load_default())
    return panel


def write_previews(out_dir: Path, records: dict[str, dict[str, np.ndarray]]) -> None:
    preview = out_dir/"preview"
    preview.mkdir(parents=True, exist_ok=True)
    side = 224
    sheet = Image.new("RGB", (side*5, (side+24)*len(SETS)), (28, 29, 28))
    for row, name in enumerate(SETS):
        d = records[name]
        images = (_rgb_on_checker(d["albedo"]), d["normal"],
                  np.repeat(as_u8(d["rough"])[..., None], 3, 2),
                  np.repeat(as_u8(d["ao"])[..., None], 3, 2),
                  np.repeat(as_u8(d["height"])[..., None], 3, 2))
        labels = ("Albedo", "Normal", "Roughness", "AO", "Height")
        for col, (arr, label) in enumerate(zip(images, labels)):
            sheet.paste(labelled(Image.fromarray(arr), f"{name} {label}", side),
                        (col*side, row*(side+24)))
        rgb = _rgb_on_checker(d["albedo"])
        tile = np.tile(rgb, (2, 2, 1))
        Image.fromarray(tile).resize((768, 768), Image.Resampling.LANCZOS).save(preview/f"{name}_tiled_2x2.png")
        crop = lit_preview(d)[256:768, 256:768]
        Image.fromarray(crop).save(preview/f"{name}_lit_crop.png")
    sheet.save(preview/"contact_sheet.png")


def generate(out_dir: Path, preview: bool) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    records: dict[str, dict[str, np.ndarray]] = {}
    makers = {"Granite": granite, "Sandstone": sandstone, "Cobble": cobble,
              "Moss": moss, "Lichen": lichen}
    strengths = {"Granite": 4.0, "Sandstone": 4.8, "Cobble": 3.5,
                 "Moss": 6.0, "Lichen": 7.0}
    for name in SETS:
        result = makers[name]()
        albedo, height, rough, ao = result[:4]
        records[name] = save_maps(out_dir, name, albedo, height, rough, ao, strengths[name])
        rgb = albedo[..., :3]
        extra = f", coverage {np.mean(albedo[..., 3] > 127)*100:.1f}%" if name == "Lichen" else ""
        print(f"{name}: mean RGB {tuple(np.mean(rgb, axis=(0, 1)).round(1))}, AO {ao.mean()*255:.1f}{extra}")
    if preview:
        write_previews(out_dir, records)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("out_dir", type=Path)
    parser.add_argument("--preview", action="store_true")
    args = parser.parse_args()
    start = time.perf_counter()
    generate(args.out_dir, args.preview)
    elapsed = time.perf_counter()-start
    print(f"Generated {len(SETS)*5} maps"+(" and previews" if args.preview else "")+
          f" in {elapsed:.2f} s")
    descriptions = {
        "Granite": "warm weathered grey, softened interlocking crystals, dark mica and sparse pits",
        "Sandstone": "muted buff layered stone, wavy laminae, ferric bands and eroded soft beds",
        "Cobble": "smooth blue-brown river stone, fine mottling and sparse pale quartz veins",
        "Moss": "dense olive cushion moss, soft dark gaps, tiny shoots and occasional dry patches",
        "Lichen": "pale grey-green lobed crusts, merging rosettes, ochre colonies and dark specks",
    }
    print("Material summary:")
    for name in SETS:
        print(f"  {name}: {descriptions[name]}")
    print(f"Runtime: {elapsed:.2f} s")


if __name__ == "__main__":
    main()
