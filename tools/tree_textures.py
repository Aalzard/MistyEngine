"""Procedurally generate original broadleaf-tree PBR texture sets.

Usage: ``python tree_textures.py <out_dir>``

Writes seamless 512x1024 bark albedos and tangent-space normal maps, plus
1024x1024 leafy-twig albedos and normals. Each leaf atlas is a row-major 2x2
grid of 512x512 cells; a twig cluster occupies each cell and enters at bottom
centre. Albedos use straight (not premultiplied) alpha. Transparent albedo RGB
is nearest-colour dilated for safe mip generation. Normals use the OpenGL
convention: +X points right, +Y (green) points toward smaller image rows, and
+Z points out.

Generation is deterministic and uses only NumPy and Pillow. The generated
textures are original and released under CC0.
"""

from __future__ import annotations

import argparse
import math
import time
from pathlib import Path

import numpy as np
from PIL import Image

BW, BH, CELL, ATLAS = 512, 1024, 512, 1024


def smoothstep(t):
    return t * t * (3.0 - 2.0 * t)


def tile_value_noise(shape, grid, seed):
    """Periodic, smoothly interpolated value noise."""
    h, w = shape
    gy, gx = grid
    values = np.random.default_rng(seed).random((gy, gx), dtype=np.float32)
    py = np.arange(h, dtype=np.float32) * gy / h
    px = np.arange(w, dtype=np.float32) * gx / w
    iy, ix = np.floor(py).astype(np.int32), np.floor(px).astype(np.int32)
    fy, fx = smoothstep(py - iy)[:, None], smoothstep(px - ix)[None, :]
    y0, y1 = iy % gy, (iy + 1) % gy
    x0, x1 = ix % gx, (ix + 1) % gx
    a = values[y0[:, None], x0[None, :]]
    b = values[y0[:, None], x1[None, :]]
    c = values[y1[:, None], x0[None, :]]
    d = values[y1[:, None], x1[None, :]]
    return ((a*(1-fx)+b*fx)*(1-fy) + (c*(1-fx)+d*fx)*fy).astype(np.float32)


def fbm(shape, seed, grids, persistence=0.52):
    out = np.zeros(shape, np.float32)
    weight = 1.0
    total = 0.0
    for octave, grid in enumerate(grids):
        out += weight * tile_value_noise(shape, grid, seed + octave * 977)
        total += weight
        weight *= persistence
    return out / total


def tile_worley(shape, grid, seed, metric="euclidean"):
    """Periodic Worley F1/F2 distances in feature-cell units."""
    h, w = shape
    gy, gx = grid
    rng = np.random.default_rng(seed)
    oy = rng.random((gy, gx), dtype=np.float32)
    ox = rng.random((gy, gx), dtype=np.float32)
    cy = np.arange(h, dtype=np.float32)[:, None] * gy / h
    cx = np.arange(w, dtype=np.float32)[None, :] * gx / w
    iy, ix = np.floor(cy).astype(np.int32), np.floor(cx).astype(np.int32)
    f1 = np.full(shape, 1e6, np.float32)
    f2 = np.full(shape, 1e6, np.float32)
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            jy, jx = iy + dy, ix + dx
            yy = jy + oy[jy % gy, jx % gx]
            xx = jx + ox[jy % gy, jx % gx]
            d = (np.abs(yy-cy)+np.abs(xx-cx)) if metric == "manhattan" else np.sqrt((yy-cy)**2 + (xx-cx)**2)
            previous = f1
            f1 = np.minimum(f1, d)
            f2 = np.minimum(f2, np.maximum(previous, d))
    return f1, f2


def height_normal(height, strength, alpha=None, wrap=True):
    """Convert height to an OpenGL tangent-space normal."""
    if wrap:
        dx = 0.5 * (np.roll(height, -1, 1) - np.roll(height, 1, 1))
        dy_up = 0.5 * (np.roll(height, 1, 0) - np.roll(height, -1, 0))
    else:
        p = np.pad(height, 1, mode="edge")
        dx = 0.5 * (p[1:-1, 2:] - p[1:-1, :-2])
        dy_up = 0.5 * (p[:-2, 1:-1] - p[2:, 1:-1])
    nx, ny, nz = -dx*strength, -dy_up*strength, np.ones_like(height)
    inv = 1.0 / np.sqrt(nx*nx + ny*ny + nz*nz)
    rgb = np.stack((nx*inv, ny*inv, nz*inv), -1)
    rgb = np.clip(np.rint((rgb*0.5 + 0.5)*255), 0, 255).astype(np.uint8)
    if alpha is not None:
        rgb[alpha < (0.5/255.0)] = (128, 128, 255)
    return rgb


def rgba(rgb, alpha=255):
    out = np.empty((*rgb.shape[:2], 4), np.uint8)
    out[..., :3] = np.clip(np.rint(rgb), 0, 255).astype(np.uint8)
    out[..., 3] = int(alpha) if np.isscalar(alpha) else np.clip(np.rint(alpha*255), 0, 255).astype(np.uint8)
    return out


def save(path, pixels):
    Image.fromarray(pixels, "RGBA").save(path, compress_level=6)


def mean_linear(pixels):
    s = pixels[..., :3][pixels[..., 3] > 127].astype(np.float64) / 255.0
    lin = np.where(s <= 0.04045, s/12.92, ((s+0.055)/1.055)**2.4)
    return lin.mean(0)


def oak_bark():
    """Build deeply furrowed, blocky mature-oak bark as a tiled height field.

    Primary fissures are periodic wandering paths. Shorter paths leave a
    fissure, bow across a ridge, and return to it, producing the split/merge
    pattern characteristic of old oak. Cross-cracks stay inside ridge bands.
    """
    shape = (BH, BW)
    y, x = np.mgrid[0:BH, 0:BW].astype(np.float32)
    broad = fbm(shape, 101, ((3, 4), (7, 8), (14, 16)))
    crumb = fbm(shape, 121, ((15, 18), (31, 38), (61, 72)), 0.48)
    speckle = tile_value_noise(shape, (128, 128), 127)
    rng = np.random.default_rng(132)
    row = np.arange(BH, dtype=np.float32)
    furrow_profile = np.ones(shape, np.float32)
    furrow_core = np.zeros(shape, np.float32)
    centres = []

    def carve_path(centre, half_width, active=None):
        """Combine a V-bottomed fissure and its rounded shoulders."""
        nonlocal furrow_profile, furrow_core
        dx = np.abs(x-centre[:, None])
        dx = np.minimum(dx, BW-dx)
        hw = np.broadcast_to(np.asarray(half_width, np.float32)[:, None], shape)
        inside = 0.055+0.30*np.clip(dx/np.maximum(hw, 0.5), 0, 1)
        wall_t = np.clip((dx-hw)/(13.0+0.22*hw), 0, 1)
        profile = np.where(dx < hw, inside, 0.355+0.645*smoothstep(wall_t))
        core = np.clip(1.0-dx/np.maximum(hw, 0.5), 0, 1)
        if active is not None:
            profile = np.where(active[:, None], profile, 1.0)
            core *= active[:, None]
        furrow_profile = np.minimum(furrow_profile, profile)
        furrow_core = np.maximum(furrow_core, core)

    # Eight 8--20 px fissures leave 35--70 px ridges between them.
    for i in range(8):
        base = (i+0.47+rng.uniform(-0.10, 0.10))*BW/8.0
        wander = tile_value_noise((BH, 1), (5, 1), 140+i)[:, 0]-0.5
        wander += 0.52*(tile_value_noise((BH, 1), (11, 1), 170+i)[:, 0]-0.5)
        wander += 0.20*np.sin(2*np.pi*row/BH*(i % 3+1)+rng.uniform(0, 2*np.pi))
        centre = (base+wander*rng.uniform(17, 25)) % BW
        width_noise = tile_value_noise((BH, 1), (13, 1), 200+i)[:, 0]
        half_width = 4.0+6.0*width_noise
        centres.append(centre)
        carve_path(centre, half_width)

    # Bowed fissures split a ridge and merge back into a primary fissure.
    for _ in range(7):
        parent = centres[int(rng.integers(0, len(centres)))]
        start = float(rng.uniform(0, BH))
        span = float(rng.uniform(190, 390))
        u = ((row-start) % BH)/span
        active = u < 1.0
        bell = np.clip(np.sin(np.pi*np.clip(u, 0, 1)), 0, 1)**1.05
        side = -1.0 if rng.random() < 0.5 else 1.0
        offset = side*rng.uniform(19, 34)*bell
        offset += bell*3.0*np.sin(4*np.pi*u+rng.uniform(0, 2*np.pi))
        branch = (parent+offset) % BW
        width = np.full(BH, rng.uniform(3.3, 6.2), np.float32)
        carve_path(branch, width, active)

    # Short rough cross-cracks break each raised band into long blocks.
    cross = np.zeros(shape, np.float32)
    cross_core = np.zeros(shape, np.float32)
    for i in range(66):
        cy = float((i*BH/66+rng.uniform(-7, 7)) % BH)
        yi = int(cy)
        ordered = np.sort(np.array([c[yi] for c in centres], np.float32))
        left = float(ordered[int(rng.integers(0, len(ordered)))])
        after = ordered[ordered > left]
        right = float(after[0]) if after.size else float(ordered[0]+BW)
        gap = right-left
        cx = (left+0.5*gap+rng.uniform(-0.10, 0.10)*gap) % BW
        half_len = np.clip(0.32*gap+rng.uniform(5, 13), 17, 34)
        dx = np.abs(x-cx)
        dx = np.minimum(dx, BW-dx)
        dy = (y-cy+BH*0.5) % BH-BH*0.5
        wav = rng.uniform(-0.13, 0.13)*(x-cx)
        wav += 1.6*np.sin((x-cx)*0.11+rng.uniform(0, 2*np.pi))
        width = rng.uniform(2.0, 4.5)
        body = np.clip(1.0-np.abs(dy-wav)/width, 0, 1)
        ends = smoothstep(np.clip((half_len-dx)/5.0, 0, 1))
        crack = body*ends
        cross_core = np.maximum(cross_core, crack)
        shoulder = smoothstep(np.clip(1.0-np.abs(dy-wav)/(width+5.0), 0, 1))*ends
        cross = np.maximum(cross, shoulder)

    ridge = furrow_profile
    top_mask = smoothstep(np.clip((ridge-0.58)/0.35, 0, 1))
    height = ridge-0.34*cross-0.14*cross_core
    height += top_mask*(0.105*(broad-0.5)+0.085*(crumb-0.5))
    height += (1.0-top_mask)*0.025*(broad-0.5)
    height = np.clip(height, 0.025, 1.0)

    bottom = np.array([47., 37., 29.])
    ridge_top = np.array([126., 114., 98.])
    tone = smoothstep(np.clip((height-0.08)/0.84, 0, 1))[..., None]
    color = bottom+(ridge_top-bottom)*tone
    color += ((broad-0.5)*13+(crumb-0.5)*15)[..., None]*top_mask[..., None]
    color -= (furrow_core*9+cross_core*17)[..., None]
    lichen_noise = fbm(shape, 152, ((3, 4), (7, 9), (15, 18)))
    lichen = smoothstep(np.clip((lichen_noise-0.62)/0.16, 0, 1))*top_mask
    lichen *= smoothstep(np.clip((broad-0.43)/0.28, 0, 1))
    color = color*(1-0.31*lichen[..., None])+np.array([91., 99., 78.])*(0.31*lichen[..., None])
    spots = (speckle > 0.78)*rng.choice([-1.0, 1.0], shape)*3.5
    color += spots[..., None]*top_mask[..., None]
    return rgba(np.clip(color, 25, 150)), rgba(height_normal(height, 12.5))
def linden_bark():
    shape = (BH, BW)
    y, x = np.mgrid[0:BH, 0:BW].astype(np.float32)
    low = fbm(shape, 201, ((3,3),(7,7),(15,13)))
    fine = fbm(shape, 211, ((18,20),(45,42),(92,86)))
    w1 = (tile_value_noise(shape,(9,4),221)-0.5)*0.42
    w2 = (tile_value_noise(shape,(7,5),222)-0.5)*0.46
    p1 = 2*np.pi*(11*x/BW+w1+0.10*np.sin(2*np.pi*y/BH))
    p2 = 2*np.pi*(8*x/BW-w2+0.13*np.sin(4*np.pi*y/BH+1.2))
    f1 = np.exp(-(np.abs(np.sin(p1*0.5))/0.105)**2)
    f2 = np.exp(-(np.abs(np.sin(p2*0.5))/0.085)**2)
    g1 = smoothstep(np.clip((tile_value_noise(shape,(15,9),225)-0.24)/0.32,0,1))
    g2 = smoothstep(np.clip((tile_value_noise(shape,(12,11),226)-0.30)/0.30,0,1))
    fiss = np.maximum(f1*g1, 0.72*f2*g2)
    height = np.clip(0.56+0.10*(low-0.5)+0.025*(fine-0.5)-0.20*fiss,0.18,0.82)
    color = np.array([118.,112.,104.])+((low-0.5)*18+(fine-0.5)*8)[...,None]
    color -= fiss[...,None]*np.array([38.,35.,31.])
    cp = smoothstep(np.clip((fbm(shape,231,((3,4),(7,9)))-0.68)/0.18,0,1))
    color = color*(1-0.16*cp[...,None])+np.array([102.,108.,98.])*(0.16*cp[...,None])
    return rgba(np.clip(color,48,154)), rgba(height_normal(height,13.0))


def birch_bark():
    shape = (BH, BW)
    low = fbm(shape,301,((3,4),(7,9),(14,18)))
    fine = fbm(shape,311,((18,22),(42,54),(88,104)))
    rng = np.random.default_rng(321)

    def region(cx,cy,x_radius,y_radius):
        """Return a small toroidally wrapped sampling region."""
        xi = np.arange(math.floor(cx-x_radius),math.ceil(cx+x_radius)+1)
        yi = np.arange(math.floor(cy-y_radius),math.ceil(cy+y_radius)+1)
        xm,ym = xi % BW,yi % BH
        dx = np.abs(xi.astype(np.float32)-cx)[None,:]
        dy = np.abs(yi.astype(np.float32)-cy)[:,None]
        return ym,xm,dy,dx

    patch = np.zeros(shape,np.float32)
    for _ in range(94):
        cx,cy = rng.uniform(0,BW),rng.uniform(0,BH)
        rx,ry = rng.uniform(16,32),rng.uniform(5.0,10.5)
        ym,xm,dy,dx = region(cx,cy,rx+1,ry+1)
        q = (dx/rx)**1.55+(dy/ry)**1.75
        lens = 1.0-smoothstep(np.clip((q-0.58)/0.42,0,1))
        area = np.ix_(ym,xm)
        patch[area] = np.maximum(patch[area],lens*rng.uniform(0.62,0.93))

    dashes = np.zeros(shape,np.float32)
    for _ in range(190):
        cx,cy = rng.uniform(0,BW),rng.uniform(0,BH)
        rx,ry = rng.uniform(7,25),rng.uniform(0.55,1.35)
        ym,xm,dy,dx = region(cx,cy,rx+4,5*ry)
        dash = np.exp(-(dy/ry)**2)*smoothstep(np.clip((rx-dx)/3.5,0,1))
        area = np.ix_(ym,xm)
        dashes[area] = np.maximum(dashes[area],dash*rng.uniform(0.30,0.66))

    peel = np.zeros(shape,np.float32)
    for _ in range(24):
        cx,cy = rng.uniform(0,BW),rng.uniform(0,BH)
        rx = rng.uniform(13,34)
        ym,xm,dy,dx = region(cx,cy,rx+5,12)
        curve_y = 1.2+4.0*(dx/np.maximum(rx,1))**2
        arc = np.exp(-((dy-curve_y)/0.95)**2)
        arc *= smoothstep(np.clip((rx-dx)/5,0,1))
        area = np.ix_(ym,xm)
        peel[area] = np.maximum(peel[area],arc*rng.uniform(0.18,0.42))

    warm = fbm(shape,333,((3,4),(7,9)))
    height = np.clip(0.57+0.052*(low-0.5)+0.018*(fine-0.5)-
                     0.075*dashes-0.095*patch+0.060*peel,0.25,0.80)
    base = np.array([211.,207.,198.])
    warm_tint = np.stack((10*(warm-0.5),5*(warm-0.5),-2*(warm-0.5)),-1)
    color = base+warm_tint+((low-0.5)*14+(fine-0.5)*6)[...,None]
    color -= dashes[...,None]*np.array([58.,55.,49.])
    mark_color = np.array([78.,70.,59.])+(fine-0.5)[...,None]*10
    color = color*(1-0.72*patch[...,None])+mark_color*(0.72*patch[...,None])
    color = color*(1-0.32*peel[...,None])+np.array([229.,218.,199.])*(0.32*peel[...,None])
    return rgba(np.clip(color,58,235)),rgba(height_normal(height,10.5))
def composite(dst_rgb, dst_alpha, dst_h, ys, xs, src_rgb, src_alpha, src_h):
    da, sa = dst_alpha[ys,xs], src_alpha
    oa = sa + da*(1-sa)
    numer = src_rgb*sa[...,None] + dst_rgb[ys,xs]*da[...,None]*(1-sa[...,None])
    dst_rgb[ys,xs] = np.divide(numer, oa[...,None], out=np.zeros_like(numer), where=oa[...,None]>1e-6)
    dst_h[ys,xs] = src_h*sa + dst_h[ys,xs]*(1-sa)
    dst_alpha[ys,xs] = oa


def draw_segment(rgb, alpha, height, a, b, radius, color, layer=0.34):
    x0,y0 = a
    x1,y1 = b
    pad = radius+2
    xa, xb = max(0,int(math.floor(min(x0,x1)-pad))), min(CELL,int(math.ceil(max(x0,x1)+pad+1)))
    ya, yb = max(0,int(math.floor(min(y0,y1)-pad))), min(CELL,int(math.ceil(max(y0,y1)+pad+1)))
    if xa >= xb or ya >= yb:
        return
    yy,xx = np.mgrid[ya:yb,xa:xb].astype(np.float32)
    vx,vy = x1-x0,y1-y0
    d2 = max(vx*vx+vy*vy,1e-5)
    t = np.clip(((xx-x0)*vx+(yy-y0)*vy)/d2,0,1)
    qx,qy = x0+t*vx,y0+t*vy
    dist = np.sqrt((xx-qx)**2+(yy-qy)**2)
    sa = np.clip(radius+0.75-dist,0,1).astype(np.float32)
    dome = np.sqrt(np.clip(1-(dist/max(radius,0.1))**2,0,1))
    src = np.broadcast_to(color,(*sa.shape,3)).copy()*(0.82+0.20*dome)[...,None]
    composite(rgb,alpha,height,slice(ya,yb),slice(xa,xb),src,sa,layer+0.09*dome)


def draw_curve(rgb, alpha, height, p0, control, p1, radius0, radius1,
               color, layer=0.31, steps=12):
    """Draw a gently curved, tapered quadratic twig."""
    previous = np.asarray(p0, np.float32)
    for i in range(1, steps+1):
        t = i/steps
        omt = 1.0-t
        point = omt*omt*np.asarray(p0)+2*omt*t*np.asarray(control)+t*t*np.asarray(p1)
        ta, tb = (i-1)/steps, t
        radius = radius0*(1-0.5*(ta+tb))+radius1*0.5*(ta+tb)
        draw_segment(rgb, alpha, height, previous, point, radius, color, layer)
        previous = point


STYLES = {
    "oak": ((150,165),(70.,110.),(0.62,0.78),np.array([69.,94.,39.]),np.array([112.,132.,57.])),
    "linden": ((105,115),(80.,120.),(0.74,0.90),np.array([82.,112.,45.]),np.array([132.,147.,63.])),
    "birch": ((260,285),(45.,65.),(0.68,0.84),np.array([101.,126.,53.]),np.array([153.,160.,72.])),
}


def leaf_width(species,t,lateral,width,lobes,phase):
    tt = np.clip(t,0,1)
    if species == "oak":
        # English oak: obovate, narrow-based blade with deep, smoothly
        # rounded lobes. The basal bump makes the paired auricles.
        core = 0.46*width*np.clip(np.sin(np.pi*tt),0,1)**0.68*(0.55+0.95*tt)
        core *= smoothstep(np.clip(tt/0.15, 0, 1))
        wave = 0.60+0.40*(0.5+0.5*np.cos(2*np.pi*lobes*tt+phase))**0.58
        auricle = 0.085*width*np.exp(-((tt-0.095)/0.040)**2)
        half = core*wave+auricle
        base = np.zeros_like(t)
    elif species == "linden":
        ln = np.clip(np.abs(lateral)/max(0.5*width,1),0,1)
        base = 0.13*(1-ln)**2
        u = np.clip((tt-base)/np.maximum(1-base,1e-3),0,1)
        half = 0.5*width*np.clip(np.sin(np.pi*u),0,1)**0.55*(1.12-0.24*u)
        half *= 1+0.030*np.sin(2*np.pi*lobes*u+phase)
    else:
        base = np.zeros_like(t)
        half = 0.5*width*np.clip(np.sin(np.pi*tt),0,1)**0.72*(1.18-0.38*tt)
        half *= 1+0.045*np.sin(2*np.pi*lobes*tt+phase)
    return half,base


def draw_leaf(rgb,alpha,height,species,centre,length,width,angle,squash,color,front,underside,rng):
    direction = np.array([math.cos(angle),math.sin(angle)],np.float32)
    right = np.array([-direction[1],direction[0]],np.float32)
    ew = width*squash
    box = 0.54*length+0.58*ew+3
    xa,xb = max(0,int(centre[0]-box)),min(CELL,int(centre[0]+box+2))
    ya,yb = max(0,int(centre[1]-box)),min(CELL,int(centre[1]+box+2))
    yy,xx = np.mgrid[ya:yb,xa:xb].astype(np.float32)
    dx,dy = xx-centre[0],yy-centre[1]
    along = dx*direction[0]+dy*direction[1]
    lateral = dx*right[0]+dy*right[1]
    t = along/length+0.5
    if species == "oak":
        lobes = int(rng.integers(4,7))
        phase = float(rng.uniform(-0.22,0.22))
    else:
        lobes = int(rng.integers(14,20))
        phase = float(rng.uniform(0,2*np.pi))
    half,base = leaf_width(species,t,lateral,ew,lobes,phase)
    edge = half-np.abs(lateral)
    ends = np.minimum((t-base)*length,(1-t)*length)
    sa = np.nan_to_num(np.clip(np.minimum(edge,ends)+0.72,0,1), nan=0.0).astype(np.float32)
    if not np.any(sa>0):
        return
    q = np.clip(np.abs(lateral)/np.maximum(half,0.8),0,1)
    dome = np.sqrt(np.clip(1-q*q,0,1))*np.clip(np.sin(np.pi*np.clip(t,0,1)),0,1)**0.35
    mid = np.exp(-(lateral/max(0.95,ew*0.030))**2)
    vc = 5.0 if species=="birch" else 6.0
    vp = np.abs(((np.clip(t,0,1)*vc-q*0.34+0.5)%1)-0.5)
    veins = np.exp(-(vp/0.075)**2)*q**0.65
    grain = 0.5*np.sin(xx*0.37+yy*0.21+phase)+0.5*np.sin(xx*0.13-yy*0.17+2*phase)
    base_color = color.copy()
    if underside:
        grey = float(np.mean(base_color))
        base_color = base_color*0.77+grey*0.23+np.array([7.,9.,4.])

    # Gentle per-leaf lighting: darker at the base, lighter at the tip, and
    # slightly shaded on the side facing away from the top-left key light.
    tip_gradient = 0.93+0.09*np.clip(t,0,1)
    light = np.array([-0.7071,-0.7071],np.float32)
    side_sign = float(np.dot(right,light))
    side_gradient = 1.0+0.045*side_sign*np.clip(lateral/np.maximum(half,1),-1,1)
    src = base_color*(tip_gradient*side_gradient*(0.955+0.055*dome)+0.014*grain)[...,None]
    src = np.clip(src+(4.5*veins+7.0*mid)[...,None],24,194)
    src_h = 0.36+0.30*front+0.14*dome-0.020*mid+0.010*veins
    composite(rgb,alpha,height,slice(ya,yb),slice(xa,xb),src,sa,src_h)

def nearest_fill(rgb,alpha):
    """Fill transparent RGB from the nearest visible texel (Manhattan metric)."""
    h,w = alpha.shape
    valid = alpha>0.01
    xx = np.arange(w,dtype=np.int32)[None,:]

    left = np.maximum.accumulate(np.where(valid,xx,-w),axis=1)
    right = np.minimum.accumulate(np.where(valid,xx,w)[:,::-1],axis=1)[:,::-1]
    left_dist = np.where(left>=0,xx-left,h+w)
    right_dist = np.where(right<w,right-xx,h+w)
    use_right = right_dist<left_dist
    sx = np.where(use_right,right,left).astype(np.int32)
    dist = np.minimum(left_dist,right_dist).astype(np.int32)
    sy = np.broadcast_to(np.arange(h,dtype=np.int32)[:,None],(h,w)).copy()
    empty_rows = ~np.any(valid,axis=1)
    dist[empty_rows] = h+w
    sx[empty_rows] = 0

    # Min-plus propagation completes the exact Manhattan distance transform.
    for iy in range(1,h):
        candidate = dist[iy-1]+1
        take = candidate<dist[iy]
        dist[iy,take] = candidate[take]
        sx[iy,take] = sx[iy-1,take]
        sy[iy,take] = sy[iy-1,take]
    for iy in range(h-2,-1,-1):
        candidate = dist[iy+1]+1
        take = candidate<dist[iy]
        dist[iy,take] = candidate[take]
        sx[iy,take] = sx[iy+1,take]
        sy[iy,take] = sy[iy+1,take]

    out = rgb.copy()
    missing = ~valid
    out[missing] = rgb[sy[missing],sx[missing]]
    return out
def leaf_cell(species,seed,variant):
    rng = np.random.default_rng(seed)
    counts,length_range,ratio_range,lo,hi = STYLES[species]
    rgb = np.zeros((CELL,CELL,3),np.float32)
    alpha = np.zeros((CELL,CELL),np.float32)
    height = np.zeros((CELL,CELL),np.float32)
    root = np.array([256.,515.])
    junction = np.array([256+rng.uniform(-12,12),365+rng.uniform(-10,12)])
    twig = np.array([68.,58.,47.])*rng.uniform(0.92,1.06)

    # Curved, tapered branch scaffold. It is drawn first and is subsequently
    # concealed by the dense leaf layers.
    control = np.array([256+rng.uniform(-18,18),438+rng.uniform(-7,7)])
    draw_curve(rgb,alpha,height,root,control,junction,4.6,2.2,twig,0.28,14)
    branch_tips = []
    angles = np.linspace(-2.88,-0.25,8)+rng.normal(0,0.14,8)
    for angle in angles:
        reach = rng.uniform(105,178)
        tip = junction+np.array([math.cos(angle)*reach,math.sin(angle)*reach])
        tip[0] = np.clip(tip[0],55,457)
        tip[1] = np.clip(tip[1],58,390)
        midpoint = 0.52*(junction+tip)
        normal = np.array([-math.sin(angle),math.cos(angle)])
        bend = midpoint+normal*rng.uniform(-22,22)
        draw_curve(rgb,alpha,height,junction,bend,tip,2.15,0.72,twig*1.02,0.30,10)
        branch_tips.append(tip)

    count = int(rng.integers(counts[0],counts[1]+1))
    golden = np.pi*(3.0-math.sqrt(5.0))
    order = np.arange(count)
    rng.shuffle(order)
    theta = order*golden+rng.uniform(0,2*np.pi)+rng.normal(0,0.17,count)
    radius = np.sqrt((np.arange(count)+0.45)/count)
    radius *= rng.uniform(0.91,1.03,count)
    rng.shuffle(radius)

    # Keep the interior dense while shaping only the outer ring. Broad
    # harmonics make the crown lopsided; narrow inset sectors make notches.
    lobe_phase = rng.uniform(0,2*np.pi)
    outline = 1.0+0.12*np.cos(theta-lobe_phase)
    outline += 0.06*np.cos(2*theta+lobe_phase*0.7)
    outer = smoothstep(np.clip((radius-0.58)/0.38,0,1))
    radius *= 1.0+(outline-1.0)*outer
    for _ in range(4):
        notch = rng.uniform(-np.pi,np.pi)
        delta = np.arctan2(np.sin(theta-notch),np.cos(theta-notch))
        depth = rng.uniform(0.17,0.24)*np.exp(-(delta/rng.uniform(0.16,0.26))**2)
        radius *= 1.0-outer*depth

    rx = {"oak":188.0,"linden":187.0,"birch":195.0}[species]+rng.uniform(-8,9)
    ry = {"oak":184.0,"linden":182.0,"birch":188.0}[species]+rng.uniform(-8,9)
    cluster_centre = np.array([256+rng.uniform(-13,13),242+rng.uniform(-11,12)])
    centres = np.column_stack((
        cluster_centre[0]+rx*radius*np.cos(theta)+rng.normal(0,7,count),
        cluster_centre[1]+ry*radius*np.sin(theta)+rng.normal(0,7,count)
    )).astype(np.float32)

    front = rng.uniform(0,1,count)
    base_lengths = rng.uniform(*length_range,count)
    lengths = base_lengths*(0.86+0.14*front)

    # Four adjacent pairs project beyond the main mass. Shorter blades let the
    # protrusions remain fully inside the cell rather than being clipped flat.
    edge_order = np.argsort(radius)[::-1]
    protruding = np.zeros(count,bool)
    anchor_angles = np.array([-2.50,-1.58,-0.60,0.05])+rng.normal(0,0.10,4)
    for anchor in anchor_angles:
        available = edge_order[~protruding[edge_order]]
        delta = np.abs(np.arctan2(np.sin(theta[available]-anchor),
                                  np.cos(theta[available]-anchor)))
        first = int(available[np.argmin(delta)])
        protruding[first] = True
        target = anchor+rng.choice((-1.0,1.0))*rng.uniform(0.09,0.16)
        available = edge_order[~protruding[edge_order]]
        delta = np.abs(np.arctan2(np.sin(theta[available]-target),
                                  np.cos(theta[available]-target)))
        second = int(available[np.argmin(delta)])
        protruding[second] = True
        for idx,offset in ((first,-0.05),(second,0.05)):
            radial = anchor+offset
            reach = rng.uniform(1.10,1.17)
            centres[idx] = cluster_centre+np.array(
                [rx*reach*np.cos(radial),ry*reach*np.sin(radial)])
            theta[idx] = radial
            lengths[idx] *= rng.uniform(0.66,0.78)

    margin = 8+(0.60 if species=="linden" else 0.55)*lengths
    centres[:,0] = np.clip(centres[:,0],margin,CELL-1-margin)
    centres[:,1] = np.clip(centres[:,1],margin,CELL-1-margin)

    leaves = []
    for idx in np.argsort(front):
        centre = centres[idx]
        radial = math.atan2(centre[1]-cluster_centre[1],centre[0]-cluster_centre[0])
        angle = radial+rng.normal(0,0.26 if protruding[idx] else 0.68)
        if rng.random() < 0.11:
            angle = 0.5*np.pi+rng.normal(0,0.32)
        length = float(lengths[idx])
        width = length*float(rng.uniform(*ratio_range))
        squash = float(rng.uniform(0.72 if species=="oak" else 0.66,1.0))
        direction = np.array([math.cos(angle),math.sin(angle)])
        base = centre-direction*(0.49*length)

        mix = float(rng.random())
        color = lo*(1-mix)+hi*mix
        grey = float(np.mean(color))
        back = 1.0-float(front[idx])
        cool = np.array([grey*0.78,grey*0.88,grey*0.86])
        color = color*(1-0.28*back)+cool*(0.28*back)
        color *= 0.65+0.35*float(front[idx])
        if front[idx] > 0.66 and rng.random() < 0.16:
            color *= np.array([1.11,1.06,0.87])

        petiole_len = {"oak":(5.,10.),"linden":(11.,19.),"birch":(7.,13.)}[species]
        plen = rng.uniform(*petiole_len)
        p0 = base-direction*plen
        pc = 0.5*(p0+base)+np.array([-direction[1],direction[0]])*rng.uniform(-2.5,2.5)
        draw_curve(rgb,alpha,height,p0,pc,base,0.85,0.52,twig*1.04,0.33,4)
        leaves.append((centre,length,width,angle,squash,color,
                       float(front[idx]),bool(rng.random()<0.14)))

    for item in leaves:
        draw_leaf(rgb,alpha,height,species,*item,rng)

    albedo = rgba(nearest_fill(rgb,alpha),alpha)
    normal = rgba(height_normal(height,5.6,alpha,wrap=False),alpha)
    return albedo,normal

def leaf_atlas(species,seed):
    albedo=np.zeros((ATLAS,ATLAS,4),np.uint8)
    normal=np.zeros_like(albedo)
    normal[...,:3]=(128,128,255)
    for variant in range(4):
        a,n=leaf_cell(species,seed+variant*1009,variant)
        cy,cx=(variant//2)*CELL,(variant%2)*CELL
        albedo[cy:cy+CELL,cx:cx+CELL]=a
        normal[cy:cy+CELL,cx:cx+CELL]=n
    return albedo,normal


def report(name,pixels):
    m=mean_linear(pixels)
    print(f"{name}: mean linear RGB(alpha>0.5) = ({m[0]:.6f}, {m[1]:.6f}, {m[2]:.6f})")


def generate(out_dir):
    out_dir.mkdir(parents=True,exist_ok=True)
    for name,maker in (("OakBark",oak_bark),("LindenBark",linden_bark),("BirchBark",birch_bark)):
        a,n=maker()
        save(out_dir/f"{name}.png",a)
        save(out_dir/f"{name}Normal.png",n)
        report(f"{name}.png",a)
    for species,name,seed in (("oak","OakLeaves",4101),("linden","LindenLeaves",5101),("birch","BirchLeaves",6101)):
        a,n=leaf_atlas(species,seed)
        save(out_dir/f"{name}.png",a)
        save(out_dir/f"{name}Normal.png",n)
        report(f"{name}.png",a)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("out_dir",type=Path)
    args=parser.parse_args()
    start=time.perf_counter()
    generate(args.out_dir)
    print(f"Generated 12 textures in {time.perf_counter()-start:.2f} s")


if __name__=="__main__":
    main()















