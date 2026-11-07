#!/usr/bin/env python3
"""Procedural CC0 wildflower texture atlases (Misty Engine's sample wildflowers).

Usage::

    python flower_textures.py <out_dir> [--only poppy|daisy|buttercup|knapweed] [--preview]

For each flower ``S`` (``Poppy``, ``Daisy``, ``Buttercup``, ``Knapweed``) it writes a
256 x 1024 atlas: ``S.png`` (RGBA sRGB albedo, straight alpha cut-out), ``SNormal.png``
(OpenGL tangent normal: +Y up the image), ``SRoughness.png`` (L, 1 = rough) and
``STranslucency.png`` (L, 1 = lets the most sunlight through).  The top 256 x 256 square is
the flower head seen from directly above, centred; the 256 x 768 below it is the plant from
the side, its stem entering at the bottom centre and its main head centred at (128, 318).
Transparent texels are nearest-colour dilated (albedo), (128, 128, 255) (normal), 255
(roughness) and 0 (translucency).  ``--preview`` writes review sheets to ``<out_dir>/preview``.

The flowers are built entirely from Pillow primitives at 4x resolution.  A small height field
is painted alongside the colour and material layers, then converted to the normal map after
downsampling.  Written with OpenAI Codex from a spec and reviewed in two rounds; deterministic;
no source imagery is used.
"""

from __future__ import annotations

import argparse
import math
import time
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter
from scipy import ndimage


SCALE = 4
W, H = 256, 1024
HW, HH = W * SCALE, H * SCALE
RESAMPLE = Image.Resampling.LANCZOS
SPECIES = ("poppy", "daisy", "buttercup", "knapweed")


def sc(v: float) -> int:
    return int(round(v * SCALE))


def sxy(points):
    return [(sc(x), sc(y)) for x, y in points]


def bezier(p0, p1, p2, p3, n=40):
    t = np.linspace(0.0, 1.0, n)
    a = ((1 - t) ** 3)[:, None] * np.asarray(p0)
    b = (3 * (1 - t) ** 2 * t)[:, None] * np.asarray(p1)
    c = (3 * (1 - t) * t**2)[:, None] * np.asarray(p2)
    d = (t**3)[:, None] * np.asarray(p3)
    return [tuple(p) for p in a + b + c + d]


def petal_polygon(cx, cy, angle, start, length, width, waves=0, blunt=0.0, phase=0.0, steps=18):
    """A tapered, optionally crumpled petal pointing along ``angle``."""
    direction = np.array([math.cos(angle), math.sin(angle)])
    side = np.array([-direction[1], direction[0]])
    left, right = [], []
    for i in range(steps + 1):
        u = i / steps
        # Keep the base and tip narrow without ending in an obviously polygonal flat cap.
        taper = math.sin(math.pi * (0.015 + 0.97 * u)) ** 0.65
        taper *= 0.76 + 0.30 * u
        if blunt:
            taper = max(taper, blunt * math.sin(math.pi * u) ** 2)
        ripple = 1.0 + (0.10 if waves else 0.0) * math.sin((waves * 2 * math.pi * u) + phase)
        half = 0.5 * width * taper * ripple
        center = np.array([cx, cy]) + direction * (start + length * u)
        curl = side * (1.1 * math.sin(2 * math.pi * u + phase)) if waves else 0
        left.append(tuple(center + side * half + curl))
        right.append(tuple(center - side * half + curl))
    return left + right[::-1]


def ray_polygon(cx, cy, angle, start, length, width, curve=0.0, phase=0.0, steps=22):
    """Narrow composite-flower ray with a softly rounded, slightly twisted tip."""
    direction = np.array([math.cos(angle), math.sin(angle)])
    side = np.array([-direction[1], direction[0]])
    left, right = [], []
    for i in range(steps + 1):
        u = i / steps
        base = min(1.0, u / .17) ** .72
        if u <= .82:
            cap = 1.0
        else:
            q = (u - .82) / .18
            cap = math.sqrt(max(0.0, 1.0 - q * q))
        ripple = 1.0 + .055 * math.sin(3 * math.pi * u + phase)
        half = .5 * width * base * cap * ripple
        center = (np.array([cx, cy]) + direction * (start + length * u)
                  + side * curve * math.sin(math.pi * u))
        left.append(tuple(center + side * half))
        right.append(tuple(center - side * half))
    return left + right[::-1]


def leaf_polygon(x0, y0, x1, y1, width, teeth=0, phase=0.0):
    p0, p1 = np.array([x0, y0], float), np.array([x1, y1], float)
    vec = p1 - p0
    length = max(np.linalg.norm(vec), 1.0)
    direction = vec / length
    side = np.array([-direction[1], direction[0]])
    left, right = [], []
    steps = max(12, teeth * 2 + 4)
    for i in range(steps + 1):
        u = i / steps
        envelope = math.sin(math.pi * u) ** 0.8
        serration = 1.0
        if teeth:
            serration += 0.20 * abs(math.sin(teeth * math.pi * u + phase))
        center = p0 + vec * u
        half = width * 0.5 * envelope * serration
        left.append(tuple(center + side * half))
        right.append(tuple(center - side * half))
    return left + right[::-1]


def squash(points, cx, cy, sx=1.0, sy=1.0, shear=0.0):
    """Affine helper used to turn radial petals into soft three-quarter ellipses."""
    out = []
    for x, y in points:
        dx, dy = x - cx, y - cy
        out.append((cx + dx * sx + dy * shear, cy + dy * sy))
    return out


class Atlas:
    def __init__(self):
        self.albedo = Image.new("RGBA", (HW, HH), (0, 0, 0, 0))
        self.height = Image.new("L", (HW, HH), 0)
        self.rough = Image.new("L", (HW, HH), 255)
        self.trans = Image.new("L", (HW, HH), 0)

    @staticmethod
    def mask():
        return Image.new("L", (HW, HH), 0)

    def paint(self, mask, colour, height, rough, trans):
        layer = Image.new("RGBA", (HW, HH), tuple(colour) + (255,))
        self.albedo = Image.composite(layer, self.albedo, mask)
        self.height = Image.composite(Image.new("L", (HW, HH), int(height)), self.height, mask)
        self.rough = Image.composite(Image.new("L", (HW, HH), int(rough)), self.rough, mask)
        self.trans = Image.composite(Image.new("L", (HW, HH), int(trans)), self.trans, mask)
        return mask

    def contact_shadow(self, mask, radius=2.2, offset=(0.0, 1.0), strength=0.22):
        """Darken the material already under a newly added overlapping shape."""
        bbox = mask.getbbox()
        if bbox is None:
            return
        pad = sc(radius * 4 + max(abs(offset[0]), abs(offset[1])))
        box = (max(0, bbox[0] - pad), max(0, bbox[1] - pad),
               min(HW, bbox[2] + pad), min(HH, bbox[3] + pad))
        local = mask.crop(box)
        shifted = ImageChops.offset(local, sc(offset[0]), sc(offset[1]))
        blurred = shifted.filter(ImageFilter.GaussianBlur(sc(radius)))
        ring = ImageChops.subtract(blurred, local)
        ring = ring.point(lambda p: int(p * strength))
        shade = Image.new("RGBA", local.size, (34, 25, 20, 255))
        current = self.albedo.crop(box)
        self.albedo.paste(Image.composite(shade, current, ring), box[:2])

    def paint_shaded(
        self, mask, colour, origin, axis, length, height=150, rough=160, trans=110,
        base_dark=0.22, rim_light=0.10, cup=0.18, veins=0.0, phase=0.0,
    ):
        """Paint a directional colour gradient and matching curved height relief."""
        bbox = mask.getbbox()
        if bbox is None:
            return mask
        x0, y0, x1, y1 = bbox
        mw = np.asarray(mask.crop(bbox), dtype=np.uint8)
        yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
        xx /= SCALE
        yy /= SCALE
        d = np.asarray(axis, np.float32)
        d /= max(float(np.linalg.norm(d)), 1e-5)
        side = np.array([-d[1], d[0]], np.float32)
        rx, ry = xx - origin[0], yy - origin[1]
        u = np.clip((rx * d[0] + ry * d[1]) / max(length, 1e-5), 0.0, 1.0)
        v = (rx * side[0] + ry * side[1]) / max(length, 1e-5)
        shade = -base_dark * (1.0 - u) ** 2 + rim_light * u
        shade += cup * (0.45 - np.abs(v)) * np.sin(np.pi * u)
        shade += 0.025 * np.sin(xx * 0.19 + yy * 0.07 + phase)
        shade += 0.018 * np.sin(xx * 0.047 - yy * 0.113 + phase * 1.7)
        if veins:
            shade -= veins * (0.5 + 0.5 * np.cos(v * length * .24 + phase)) * (1.0 - u) * 0.11
        base = np.asarray(colour, np.float32)
        rgb = np.clip(base[None, None, :] * (1.0 + shade[..., None]), 0, 255).astype(np.uint8)
        alpha = np.where(mw > 0, 255, 0).astype(np.uint8)
        tile = Image.fromarray(np.dstack((rgb, alpha)), "RGBA")
        current = self.albedo.crop(bbox)
        local_mask = Image.fromarray(mw, "L")
        self.albedo.paste(Image.composite(tile, current, local_mask), bbox[:2])

        relief = height + 72.0 * cup * np.sin(np.pi * u) + 34.0 * rim_light * u
        relief += 18.0 * veins * np.sin(v * length * .24 + phase) * np.sin(np.pi * u)
        relief += 9.0 * np.sin(xx * 0.23 + yy * 0.16 + phase) * np.sin(np.pi * u)
        htile = Image.fromarray(np.clip(relief, 0, 255).astype(np.uint8), "L")
        self.height.paste(Image.composite(htile, self.height.crop(bbox), local_mask), bbox[:2])
        for target, value in ((self.rough, rough), (self.trans, trans)):
            fill = Image.new("L", (x1 - x0, y1 - y0), int(value))
            target.paste(Image.composite(fill, target.crop(bbox), local_mask), bbox[:2])
        return mask

    def shaded_polygon(self, points, colour, origin, axis, length, **kwargs):
        mask = self.mask()
        ImageDraw.Draw(mask).polygon(sxy(points), fill=255)
        self.contact_shadow(mask, kwargs.pop("shadow_radius", 1.8), kwargs.pop("shadow_offset", (0, 1.0)),
                            kwargs.pop("shadow_strength", 0.18))
        return self.paint_shaded(mask, colour, origin, axis, length, **kwargs)

    def polygon(self, points, colour, height=150, rough=160, trans=110):
        mask = self.mask()
        ImageDraw.Draw(mask).polygon(sxy(points), fill=255)
        return self.paint(mask, colour, height, rough, trans)

    def ellipse(self, box, colour, height=150, rough=160, trans=110):
        mask = self.mask()
        ImageDraw.Draw(mask).ellipse(tuple(sc(v) for v in box), fill=255)
        return self.paint(mask, colour, height, rough, trans)

    def line(self, points, width, colour, height=150, rough=170, trans=50, joint="curve"):
        mask = self.mask()
        ImageDraw.Draw(mask).line(sxy(points), fill=255, width=max(1, sc(width)), joint=joint)
        return self.paint(mask, colour, height, rough, trans)

    def multi_lines(self, items, colour, height=150, rough=170, trans=50):
        """Paint many independent curved strokes with one material-layer operation."""
        mask = self.mask()
        draw = ImageDraw.Draw(mask)
        for points, width in items:
            draw.line(sxy(points), fill=255, width=max(1, sc(width)), joint="curve")
        return self.paint(mask, colour, height, rough, trans)

    def detail_line(self, points, width, colour, height=None, rough=None, trans=None, clip=None):
        mask = self.mask()
        ImageDraw.Draw(mask).line(sxy(points), fill=255, width=max(1, sc(width)), joint="curve")
        if clip is not None:
            mask = ImageChops.multiply(mask, clip)
        colour_layer = Image.new("RGBA", (HW, HH), tuple(colour) + (255,))
        self.albedo = Image.composite(colour_layer, self.albedo, mask)
        if height is not None:
            self.height = Image.composite(Image.new("L", (HW, HH), int(height)), self.height, mask)
        if rough is not None:
            self.rough = Image.composite(Image.new("L", (HW, HH), int(rough)), self.rough, mask)
        if trans is not None:
            self.trans = Image.composite(Image.new("L", (HW, HH), int(trans)), self.trans, mask)

    def soft_glaze(self, points, width, colour, clip, opacity=.30, blur=2.5):
        """Add a feathered translucent highlight without changing cutout alpha."""
        mask = self.mask()
        ImageDraw.Draw(mask).line(sxy(points), fill=255, width=max(1, sc(width)), joint="curve")
        bbox = mask.getbbox()
        if bbox is None:
            return
        pad = sc(blur * 4)
        box = (max(0, bbox[0] - pad), max(0, bbox[1] - pad),
               min(HW, bbox[2] + pad), min(HH, bbox[3] + pad))
        local = mask.crop(box).filter(ImageFilter.GaussianBlur(sc(blur)))
        local = ImageChops.multiply(local, clip.crop(box)).point(lambda p: int(p * opacity))
        layer = Image.new("RGBA", local.size, tuple(colour) + (255,))
        current = self.albedo.crop(box)
        self.albedo.paste(Image.composite(layer, current, local), box[:2])


    def soft_glaze_lobe(self, center, radii, angle, colour, clip, opacity=.25, blur=4.0):
        """Paint an oriented, feathered specular lobe inside an existing shape."""
        cx, cy = center
        rx, ry = radii
        ca, sa = math.cos(angle), math.sin(angle)
        points = []
        for t in np.linspace(0.0, 2.0 * math.pi, 48, endpoint=False):
            x, y = rx * math.cos(t), ry * math.sin(t)
            points.append((cx + x * ca - y * sa, cy + x * sa + y * ca))
        mask = self.mask()
        ImageDraw.Draw(mask).polygon(sxy(points), fill=255)
        bbox = mask.getbbox()
        if bbox is None:
            return
        pad = sc(blur * 4)
        box = (max(0, bbox[0] - pad), max(0, bbox[1] - pad),
               min(HW, bbox[2] + pad), min(HH, bbox[3] + pad))
        local = mask.crop(box).filter(ImageFilter.GaussianBlur(sc(blur)))
        local = ImageChops.multiply(local, clip.crop(box)).point(lambda p: int(p * opacity))
        layer = Image.new("RGBA", local.size, tuple(colour) + (255,))
        current = self.albedo.crop(box)
        self.albedo.paste(Image.composite(layer, current, local), box[:2])

    def domed_ellipse(self, box, edge_colour, centre_colour, height_edge, height_peak,
                      rough=170, trans=50, texture=0.0, phase=0.0):
        """Paint a continuously shaded elliptical dome with matching height relief."""
        sx0, sy0, sx1, sy1 = (sc(v) for v in box)
        yy, xx = np.mgrid[sy0:sy1, sx0:sx1].astype(np.float32)
        cx, cy = (sx0 + sx1 - 1) * .5, (sy0 + sy1 - 1) * .5
        rx, ry = max((sx1 - sx0) * .5, 1), max((sy1 - sy0) * .5, 1)
        nx, ny = (xx - cx) / rx, (yy - cy) / ry
        radius = np.sqrt(nx * nx + ny * ny)
        inside = radius <= 1.0
        dome = np.sqrt(np.clip(1.0 - radius * radius, 0.0, 1.0))
        edge = np.asarray(edge_colour, np.float32)
        centre = np.asarray(centre_colour, np.float32)
        rgb = edge[None, None, :] * (1.0 - dome[..., None]) + centre[None, None, :] * dome[..., None]
        if texture:
            grain = np.sin(xx * .31 + ny * 7.0 + phase) + np.sin(yy * .23 - nx * 9.0 + phase * 1.7)
            rgb *= 1.0 + texture * grain[..., None]
        alpha = np.where(inside, 255, 0).astype(np.uint8)
        tile = Image.fromarray(np.dstack((np.clip(rgb, 0, 255).astype(np.uint8), alpha)), "RGBA")
        mask = Image.fromarray(alpha, "L")
        crop_box = (sx0, sy0, sx1, sy1)
        self.albedo.paste(Image.composite(tile, self.albedo.crop(crop_box), mask), crop_box[:2])
        relief = height_edge + (height_peak - height_edge) * dome
        htile = Image.fromarray(np.clip(relief, 0, 255).astype(np.uint8), "L")
        self.height.paste(Image.composite(htile, self.height.crop(crop_box), mask), crop_box[:2])
        for target, value in ((self.rough, rough), (self.trans, trans)):
            fill = Image.new("L", mask.size, int(value))
            target.paste(Image.composite(fill, target.crop(crop_box), mask), crop_box[:2])
        return mask


GREEN = (78, 111, 48)
STEM = (96, 126, 62)
DARK_GREEN = (48, 78, 34)


def draw_stem(a, points, width=3.3, colour=STEM, rib=True):
    a.line(points, width + 1.2, DARK_GREEN, 100, 180, 38)
    a.line(points, width, colour, 158, 178, 38)
    if rib:
        shifted = [(x - 0.65, y) for x, y in points]
        a.detail_line(shifted, 0.65, (129, 150, 79), 184, clip=None)


def draw_leaf(a, p0, p1, width, teeth=0, colour=GREEN, divided=False):
    if divided:
        # A central rachis with paired lobes makes the silhouette read as feathery.
        a.line([p0, p1], 2.1, (79, 107, 48), 152, 160, 128)
        p0a, p1a = np.array(p0), np.array(p1)
        for u in (0.24, 0.40, 0.56, 0.71):
            c = p0a + (p1a - p0a) * u
            d = (p1a - p0a) / max(np.linalg.norm(p1a - p0a), 1)
            side = np.array([-d[1], d[0]])
            size = width * math.sin(math.pi * u)
            for sign in (-1, 1):
                tip = c + side * sign * size + d * size * 0.28
                poly = leaf_polygon(c[0], c[1], tip[0], tip[1], max(2.5, size * 0.46), 2)
                m = a.polygon(poly, colour, 137, 155, 126)
                a.detail_line([tuple(c), tuple(tip)], 0.7, (111, 137, 64), 168, clip=m)
        return
    poly = leaf_polygon(*p0, *p1, width, teeth)
    m = a.polygon(poly, colour, 138, 155, 128)
    a.detail_line([p0, p1], 0.9, (120, 145, 69), 180, clip=m)


def draw_poppy_top(a, cx=128, cy=128, radius=115):
    rng = np.random.default_rng(101)
    masks = []
    for i, angle in enumerate((0.12, math.pi / 2 + 0.04, math.pi + 0.10, 3 * math.pi / 2 - 0.05)):
        poly = petal_polygon(cx, cy, angle, 5, radius - 5 + rng.uniform(-3, 3), 168, 5, 0.44, i, 28)
        colour = ((207, 35, 27), (220, 42, 28), (198, 29, 23), (215, 38, 25))[i]
        direction = (math.cos(angle), math.sin(angle))
        m = a.shaded_polygon(poly, colour, (cx, cy), direction, radius,
                             height=148 + i * 4, rough=112, trans=232,
                             base_dark=.32, rim_light=.12, cup=.30, veins=.55, phase=i * 1.7,
                             shadow_radius=2.7, shadow_strength=.24)
        masks.append(m)
        # Dark basal blotch and fine, irregular silky veins.
        blotch = petal_polygon(cx, cy, angle, 3, 36, 34, 1, 0.0, i, 10)
        a.polygon(blotch, (45, 24, 25), 127, 128, 205)
        for off in (-0.21, -0.10, 0.0, 0.11, 0.22):
            ang = angle + off
            end = (cx + math.cos(ang) * (76 + rng.uniform(-6, 6)), cy + math.sin(ang) * (76 + rng.uniform(-6, 6)))
            mid = (cx + math.cos(ang + rng.uniform(-.03, .03)) * 48,
                   cy + math.sin(ang + rng.uniform(-.03, .03)) * 48)
            a.detail_line([(cx, cy), mid, end], rng.uniform(.45, .8), (148, 24, 25),
                          137 + int(rng.integers(-5, 7)), clip=masks[-1])
    a.ellipse((110, 110, 146, 146), (24, 35, 27), 183, 170, 24)
    for i in range(14):
        ang = 2 * math.pi * i / 14 + 0.1
        r0, r1 = 15, 25
        a.line([(cx + math.cos(ang) * r0, cy + math.sin(ang) * r0),
                (cx + math.cos(ang) * r1, cy + math.sin(ang) * r1)], 2.2,
               (30, 28, 24), 196, 160, 24)
    a.ellipse((119, 119, 137, 137), (54, 74, 42), 208, 185, 24)
    for i in range(7):
        ang = 2 * math.pi * i / 7 - math.pi / 2
        a.line([(128, 128), (128 + 8 * math.cos(ang), 128 + 8 * math.sin(ang))], 1.4,
               (17, 22, 18), 220, 180, 18)


def draw_poppy_side(a):
    stem = bezier((128, 1023), (125, 780), (138, 520), (128, 318))
    draw_stem(a, stem, 4.1, (108, 132, 75))
    draw_leaf(a, (126, 817), (83, 746), 24, divided=True)
    draw_leaf(a, (127, 720), (169, 660), 21, divided=True)
    draw_leaf(a, (126, 910), (91, 864), 19, divided=True)
    draw_leaf(a, (127, 952), (161, 914), 17, divided=True)
    # Nodding hairy bud on a short side stalk.
    branch = bezier((131, 602), (159, 564), (169, 499), (166, 454), 24)
    draw_stem(a, branch, 2.8, (105, 130, 73), False)
    for i in range(10):
        u = i / 9
        x, y = branch[int(u * (len(branch) - 1))]
        a.line([(x, y), (x + 3.5 * (-1 if i % 2 else 1), y - 2)], 0.65, (151, 153, 99), 170, 180, 35)
    bud = leaf_polygon(166, 461, 161, 432, 20, teeth=0)
    a.shaded_polygon(bud, (81, 106, 60), (166, 458), (-.18, -1), 29,
                     height=174, rough=186, trans=28, base_dark=.20, rim_light=.10, cup=.22, veins=.15)
    for i in range(13):
        ang = -math.pi / 2 + (i - 6) * .10
        a.line([(166 + math.cos(ang) * 8, 456 + math.sin(ang) * 8),
                (166 + math.cos(ang) * 15, 456 + math.sin(ang) * 16)], .55,
               (153, 158, 100), 182, 184, 30)

    # Four broad, overlapping petals make a shallow open cup seen from above.
    petal_specs = [(-2.30, 55, 78, (184, 25, 23), 145), (-.82, 57, 80, (198, 29, 23), 150),
                   (2.55, 56, 82, (218, 38, 27), 172), (.58, 58, 84, (224, 43, 29), 177)]
    petal_masks = []
    for i, (ang, length, width, colour, h) in enumerate(petal_specs):
        poly = petal_polygon(128, 318, ang, 1, length, width, 3, .55, i * 1.2, 28)
        poly = squash(poly, 128, 318, 1.0, .68, shear=.04)
        m = a.shaded_polygon(poly, colour, (128, 318), (math.cos(ang), math.sin(ang) * .68), length,
                             height=h, rough=110, trans=234, base_dark=.38, rim_light=.14,
                             cup=.34, veins=.60, phase=i * 1.4, shadow_radius=2.2, shadow_strength=.26)
        petal_masks.append(m)
        blotch = squash(petal_polygon(128, 318, ang, 0, 25, 30, 1, .2, i, 16), 128, 318, 1, .68)
        a.polygon(blotch, (48, 24, 25), h + 5, 140, 180)
    a.ellipse((116, 309, 140, 331), (34, 47, 31), 214, 180, 22)
    for i in range(16):
        ang = 2 * math.pi * i / 16
        a.line([(128 + math.cos(ang) * 8, 320 + math.sin(ang) * 5),
                (128 + math.cos(ang) * 19, 320 + math.sin(ang) * 10)], 1.05,
               (27, 24, 22), 205, 170, 20)
    a.ellipse((122, 314, 134, 326), (58, 77, 44), 224, 182, 20)


def draw_daisy_top(a, cx=128, cy=128):
    rng = np.random.default_rng(202)
    for i in range(24):
        angle = 2 * math.pi * i / 24 + rng.uniform(-0.04, 0.04)
        length = rng.uniform(88, 100)
        width = rng.uniform(36, 44)
        poly = ray_polygon(cx, cy, angle, 22, length, width,
                           rng.uniform(-2.4, 2.4), i * .7, 22)
        shade = int(rng.integers(235, 244))
        m = a.shaded_polygon(poly, (shade, shade - 1, shade - 5), (cx, cy),
                             (math.cos(angle), math.sin(angle)), length + 20,
                             height=146 + i % 4, rough=153, trans=168, base_dark=.12,
                             rim_light=.035, cup=.20, veins=.28, phase=i * .7,
                             shadow_radius=1.4, shadow_strength=.14)
        end = (cx + math.cos(angle) * (length + 13), cy + math.sin(angle) * (length + 13))
        a.detail_line([(cx + math.cos(angle) * 25, cy + math.sin(angle) * 25), end],
                      1.0, (207, 211, 211), 126, clip=m)
    # A continuous dome avoids the flat, concentric target profile of stacked discs.
    a.domed_ellipse((cx - 34, cy - 34, cx + 34, cy + 34),
                    (205, 150, 27), (177, 142, 43), 174, 235, 174, 52, .018, .7)
    # Hundreds of apparent florets from small offset dots.
    for ring in range(1, 6):
        n = 7 * ring
        r = ring * 5.3
        for i in range(n):
            ang = 2 * math.pi * i / n + ring * 0.32
            x, y = cx + r * math.cos(ang), cy + r * math.sin(ang)
            col = (235, 186 + ring * 3, 45) if (i + ring) % 2 else (194, 137, 24)
            a.ellipse((x - 1.8, y - 1.8, x + 1.8, y + 1.8), col, 212 - ring * 2, 170, 48)


def draw_daisy_side(a):
    stem = bezier((128, 1023), (125, 800), (129, 530), (128, 318))
    draw_stem(a, stem, 4.0, (91, 126, 55))
    draw_leaf(a, (128, 844), (91, 796), 13, teeth=7)
    draw_leaf(a, (127, 700), (166, 649), 12, teeth=6)
    draw_leaf(a, (128, 565), (98, 527), 10, teeth=5)
    draw_leaf(a, (127, 925), (91, 884), 14, teeth=7)
    draw_leaf(a, (128, 865), (160, 826), 12, teeth=6)
    # Tilted white ray petals: rear rays first, then the domed yellow disc.
    rng = np.random.default_rng(203)
    for i in range(23):
        ang = 2 * math.pi * i / 23
        length = rng.uniform(47, 55)
        poly = ray_polygon(128, 318, ang, 7, length, rng.uniform(13, 17),
                           rng.uniform(-1.4, 1.4), i * .7, 18)
        poly = squash(poly, 128, 318, 1, .38, shear=.03)
        shade = int(rng.integers(235, 244))
        m = a.shaded_polygon(poly, (shade, shade, shade - 4), (128, 318),
                             (math.cos(ang), math.sin(ang) * .38), length + 7,
                             height=145 + int(10 * math.sin(ang)), rough=155, trans=166,
                             base_dark=.13, rim_light=.03, cup=.22, veins=.24, phase=i,
                             shadow_radius=1.0, shadow_strength=.12)
        end = (128 + math.cos(ang) * (length + 4), 318 + math.sin(ang) * length * .38)
        a.detail_line([(128, 318), end], 0.65, (204, 209, 211), 128, clip=m)
    a.domed_ellipse((105, 304, 151, 334), (210, 154, 27), (177, 139, 40),
                    180, 239, 173, 50, .016, 1.1)
    for yy in (311, 318, 325):
        for xx in range(111 + (yy - 318) ** 2 // 14, 147 - (yy - 318) ** 2 // 14, 6):
            a.ellipse((xx - 1.3, yy - 1.1, xx + 1.3, yy + 1.1), (185, 128, 24), 218, 170, 45)


def draw_buttercup_top(a, cx=128, cy=128):
    rng = np.random.default_rng(303)
    for i in range(5):
        angle = -math.pi / 2 + 2 * math.pi * i / 5
        poly = petal_polygon(cx, cy, angle, 8, 107, 142, 1, 0.66, i, 24)
        m = a.shaded_polygon(poly, (242, 197 + (i % 2) * 5, 42), (cx, cy),
                             (math.cos(angle), math.sin(angle)), 112,
                             height=166 + i * 3, rough=50, trans=128, base_dark=.28,
                             rim_light=.08, cup=.34, veins=.25, phase=i * 1.1,
                             shadow_radius=2.0, shadow_strength=.19)
        # Broad elliptical reflections read as waxy petal gloss, not painted bars.
        r = 61
        centre = (cx + math.cos(angle) * r, cy + math.sin(angle) * r)
        a.soft_glaze_lobe(centre, (25, 31), angle, (255, 235, 128), m,
                          opacity=.24, blur=7.0)
        lit_side = angle - .18
        glint = (cx + math.cos(lit_side) * 72, cy + math.sin(lit_side) * 72)
        a.soft_glaze_lobe(glint, (11, 19), angle, (255, 244, 170), m,
                          opacity=.12, blur=5.0)
        for off in (-0.13, 0.12):
            ae = angle + off
            a.detail_line([(cx, cy), (cx + 78 * math.cos(ae), cy + 78 * math.sin(ae))],
                          0.8, (213, 158, 29), 147, clip=m)
    a.ellipse((104, 104, 152, 152), (159, 155, 43), 205, 95, 65)
    for i in range(20):
        ang = 2 * math.pi * i / 20
        r = 15 + 4 * (i % 2)
        x, y = cx + r * math.cos(ang), cy + r * math.sin(ang)
        a.ellipse((x - 2, y - 2, x + 2, y + 2), (225, 184, 42), 220, 90, 65)


def small_buttercup(a, cx, cy, scale=1.0, tilt=0.0):
    for i in range(5):
        ang = tilt + 2 * math.pi * i / 5
        poly = petal_polygon(cx, cy, ang, 1, 24 * scale, 29 * scale, 1, .58, i, 18)
        poly = squash(poly, cx, cy, 1, .52, shear=.03)
        m = a.shaded_polygon(poly, (241, 198 + i % 2 * 4, 42), (cx, cy),
                             (math.cos(ang), math.sin(ang) * .52), 25 * scale,
                             height=172 + i * 2, rough=51, trans=128, base_dark=.30,
                             rim_light=.09, cup=.35, veins=.18, phase=i,
                             shadow_radius=1.2, shadow_strength=.16)
        lobe = (cx + math.cos(ang) * 15 * scale,
                cy + math.sin(ang) * 15 * scale * .52)
        a.soft_glaze_lobe(lobe, (7 * scale, 9 * scale), ang, (255, 235, 126), m,
                          opacity=.22, blur=max(1.8, 3.0 * scale))
    a.ellipse((cx - 7 * scale, cy - 5 * scale, cx + 7 * scale, cy + 5 * scale),
              (176, 164, 40), 209, 90, 60)


def palmate_leaf(a, cx, cy, size, angle=0.0):
    for off, mult in ((-1.0, .80), (-0.52, .95), (0, 1.08), (.52, .95), (1.0, .80)):
        ang = angle + off
        tip = (cx + math.cos(ang) * size * mult, cy + math.sin(ang) * size * mult)
        draw_leaf(a, (cx, cy), tip, size * 0.28, teeth=3)


def draw_buttercup_side(a):
    main = bezier((128, 1023), (128, 810), (121, 520), (128, 318))
    draw_stem(a, main, 4.0, (91, 126, 51))
    left = bezier((125, 620), (103, 568), (86, 503), (78, 442), 25)
    right = bezier((126, 520), (151, 486), (165, 432), (168, 390), 25)
    draw_stem(a, left, 2.7, (89, 123, 52))
    draw_stem(a, right, 2.7, (89, 123, 52))
    palmate_leaf(a, 126, 890, 55, -math.pi / 2)
    palmate_leaf(a, 126, 760, 44, -math.pi / 2 + 0.2)
    draw_leaf(a, (124, 646), (91, 616), 12, teeth=4)
    small_buttercup(a, 128, 318, 1.45, -0.15)
    small_buttercup(a, 78, 442, 0.95, 0.1)
    # A green closed bud terminates the right branch.
    a.ellipse((158, 378, 178, 400), (101, 124, 52), 185, 180, 28)
    a.line([(160, 386), (175, 396)], 1.1, (57, 86, 38), 195, 180, 25)


def draw_knapweed_top(a, cx=128, cy=128):
    rng = np.random.default_rng(404)
    # Uneven shadow lobes close the crown interior without giving it a circular cutout edge.
    crown_mask = a.mask()
    crown_draw = ImageDraw.Draw(crown_mask)
    for i in range(56):
        angle = 2 * math.pi * i / 56 + rng.uniform(-.035, .035)
        poly = petal_polygon(cx, cy, angle, 0, rng.uniform(102, 116),
                             rng.uniform(32, 48), 6, .12, rng.uniform(0, 6.2), 24)
        crown_draw.polygon(sxy(poly), fill=255)
    a.paint(crown_mask, (76, 27, 83), 116, 188, 130)    # The small imbricate involucre sits below the flower brush and only peeks through gaps.
    a.ellipse((116, 117, 140, 139), (59, 61, 42), 174, 194, 35)
    for ring, count in ((15, 11), (10, 8), (5, 5)):
        for i in range(count):
            angle = 2 * math.pi * (i + .35 * (ring % 2)) / count
            radius = ring + rng.uniform(-1.8, 1.8)
            x, y = cx + radius * math.cos(angle), cy + radius * math.sin(angle)
            direction = np.array([math.cos(angle), math.sin(angle)])
            side = np.array([-direction[1], direction[0]])
            tip = np.array([x, y]) + direction * rng.uniform(4.5, 7.0)
            base = np.array([x, y]) - direction * 3.0
            poly = [tuple(base + side * 3.4), tuple(tip), tuple(base - side * 3.4),
                    tuple(base - direction * 2.2)]
            a.polygon(poly, (68 + ring // 3, 68, 43), 181 + ring, 195, 36)
            a.line([tuple(base + side * 2.6), tuple(tip), tuple(base - side * 2.6)],
                   .65, (39, 43, 31), 196, 200, 25)

    # Curved shadow tubes overlap densely, but their varied ends preserve a ragged outline.
    under_groups = [[] for _ in range(4)]
    for i in range(240):
        angle = rng.uniform(0, 2 * math.pi)
        r0, r1 = rng.uniform(5, 35), rng.uniform(60, 114)
        bend = rng.uniform(-.52, .52)
        p0 = (cx + math.cos(angle) * r0, cy + math.sin(angle) * r0)
        p1 = (cx + math.cos(angle + bend * .58) * r1,
              cy + math.sin(angle + bend * .58) * r1)
        c1r, c2r = r0 + (r1 - r0) * .30, r0 + (r1 - r0) * .70
        c1 = (cx + math.cos(angle - bend * .28) * c1r,
              cy + math.sin(angle - bend * .28) * c1r)
        c2 = (cx + math.cos(angle + bend * 1.18) * c2r,
              cy + math.sin(angle + bend * 1.18) * c2r)
        under_groups[i % 4].append((bezier(p0, c1, c2, p1, 13), rng.uniform(4.0, 7.0)))
    under_cols = ((82, 31, 88), (99, 37, 103), (112, 40, 113), (89, 34, 96))
    for group, col in zip(under_groups, under_cols):
        a.multi_lines(group, col, 142 + under_groups.index(group) * 9, 190, 130)

    foreground = [[] for _ in range(5)]
    frayed = [[] for _ in range(3)]
    for i in range(340):
        angle = rng.uniform(0, 2 * math.pi)
        r0, r1 = rng.uniform(4, 41), rng.uniform(66, 116)
        bend = rng.uniform(-.48, .48)
        p0 = (cx + math.cos(angle) * r0, cy + math.sin(angle) * r0)
        end_angle = angle + bend * .62
        p1 = (cx + math.cos(end_angle) * r1, cy + math.sin(end_angle) * r1)
        c1r, c2r = r0 + (r1 - r0) * .27, r0 + (r1 - r0) * .73
        c1 = (cx + math.cos(angle - bend * .31) * c1r,
              cy + math.sin(angle - bend * .31) * c1r)
        c2 = (cx + math.cos(angle + bend * 1.22) * c2r,
              cy + math.sin(angle + bend * 1.22) * c2r)
        foreground[i % 5].append((bezier(p0, c1, c2, p1, 13), rng.uniform(1.0, 2.35)))
        if i % 2 == 0:
            fork_count = 3 + i % 3
            for fork in range(fork_count):
                spread = (fork - (fork_count - 1) / 2) * rng.uniform(.15, .23)
                tip_angle = end_angle + spread
                length = rng.uniform(3.8, 7.8)
                tip = (p1[0] + math.cos(tip_angle) * length,
                       p1[1] + math.sin(tip_angle) * length)
                ctrl = (p1[0] + math.cos(tip_angle - spread * .45) * length * .55,
                        p1[1] + math.sin(tip_angle - spread * .45) * length * .55)
                frayed[i % 3].append((bezier(p1, ctrl, ctrl, tip, 5), rng.uniform(.52, .86)))
    foreground_cols = ((156, 54, 153), (181, 73, 172), (140, 47, 143),
                       (194, 88, 181), (168, 60, 166))
    for index, (group, col) in enumerate(zip(foreground, foreground_cols)):
        a.multi_lines(group, col, 164 + index * 8, 180, 140)
    for index, group in enumerate(frayed):
        a.multi_lines(group, (191 + index * 7, 79 + index * 7, 180 + index * 4),
                      202 + index * 6, 176, 144)

    # Short curling florets hide the involucre's symmetry without forming a round plug.
    inner = [[] for _ in range(4)]
    for i in range(64):
        angle = rng.uniform(0, 2 * math.pi)
        r0, r1 = rng.uniform(1, 9), rng.uniform(18, 43)
        p0 = (cx + math.cos(angle) * r0, cy + math.sin(angle) * r0)
        end_angle = angle + rng.uniform(-.42, .42)
        p1 = (cx + math.cos(end_angle) * r1, cy + math.sin(end_angle) * r1)
        mid = ((p0[0] + p1[0]) * .5 + rng.uniform(-5, 5),
               (p0[1] + p1[1]) * .5 + rng.uniform(-5, 5))
        inner[i % 4].append((bezier(p0, p0, mid, p1, 8), rng.uniform(1.1, 2.0)))
    for index, group in enumerate(inner):
        a.multi_lines(group, (122 + index * 14, 40 + index * 5, 126 + index * 11),
                      177 + index * 8, 183, 132)

def knapweed_head_side(a, cx, cy, scale=1.0):
    # Overlapping imbricate bracts form a shaded, rounded involucre.
    a.domed_ellipse((cx - 33 * scale, cy + 1 * scale, cx + 33 * scale, cy + 48 * scale),
                    (48, 58, 35), (76, 84, 46), 146, 191, 193, 36, .012, cx * .1)
    for row in range(4):
        n = 5 + row
        yy = cy + (12 + row * 9) * scale
        span = (26 - row * 2) * scale
        for i in range(n):
            x = cx - span + 2 * span * (i + 0.5) / n
            poly = leaf_polygon(x, yy + 5 * scale, x, yy - 8 * scale, 10 * scale)
            a.polygon(poly, (73 + row * 3, 82, 45), 182, 193, 36)
            a.line([(x - 4 * scale, yy), (x, yy + 4 * scale), (x + 4 * scale, yy)],
                   max(0.7, 1.1 * scale), (43, 49, 31), 190, 198, 25)

    rng = np.random.default_rng(int(cx * 17 + cy * 13))
    for i in range(max(42, int(112 * scale))):
        x0 = cx + rng.uniform(-22, 22) * scale
        y0 = cy + rng.uniform(-1, 7) * scale
        x1 = x0 + rng.uniform(-18, 18) * scale
        crown = 52 - 18 * abs(x1 - cx) / max(28 * scale, 1)
        y1 = cy - rng.uniform(max(21, crown - 11), max(27, crown)) * scale
        sway = rng.uniform(-8, 8) * scale
        p0, p1 = (x0, y0), (x1, y1)
        c1 = (x0 + sway * .30, y0 - (y0 - y1) * .33)
        c2 = (x1 - sway, y1 + (y0 - y1) * .34)
        col = ((170, 69, 166), (149, 55, 149), (185, 79, 175), (160, 60, 160))[i % 4]
        a.line(bezier(p0, c1, c2, p1, 9), max(.85, rng.uniform(1.15, 2.0) * scale),
               col, 163 + i % 8 * 6, 181, 140)
        if i % 3 == 0:
            fork_count = 3 + i % 2
            stem_angle = math.atan2(y1 - c2[1], x1 - c2[0])
            for fork in range(fork_count):
                spread = (fork - (fork_count - 1) / 2) * .23
                tip_angle = stem_angle + spread
                length = rng.uniform(3.2, 6.3) * scale
                tip = (x1 + math.cos(tip_angle) * length, y1 + math.sin(tip_angle) * length)
                a.line(bezier(p1, p1, ((x1 + tip[0]) * .5 + sway * .06,
                                       (y1 + tip[1]) * .5), tip, 5),
                       max(.55, .82 * scale), (199, 91, 185), 202, 179, 142)

def draw_knapweed_side(a):
    main = bezier((128, 1023), (127, 800), (131, 535), (128, 338))
    draw_stem(a, main, 4.0, (76, 104, 48))
    # Ribs remain visible at distance.
    a.detail_line([(130, 1015), (129, 790), (133, 540), (130, 350)], 0.8, (127, 139, 72), 192)
    branch = bezier((130, 613), (155, 578), (172, 511), (174, 464), 25)
    draw_stem(a, branch, 3.1, (76, 103, 48))
    draw_leaf(a, (128, 830), (86, 762), 15, teeth=1, colour=(71, 100, 48))
    draw_leaf(a, (129, 702), (169, 649), 13, teeth=1, colour=(72, 100, 48))
    draw_leaf(a, (131, 563), (97, 516), 11, teeth=1, colour=(74, 102, 50))
    knapweed_head_side(a, 128, 318, 1.0)
    knapweed_head_side(a, 174, 452, 0.62)


def build(species):
    a = Atlas()
    if species == "poppy":
        draw_poppy_top(a)
        draw_poppy_side(a)
    elif species == "daisy":
        draw_daisy_top(a)
        draw_daisy_side(a)
    elif species == "buttercup":
        draw_buttercup_top(a)
        draw_buttercup_side(a)
    else:
        draw_knapweed_top(a)
        draw_knapweed_side(a)
    return a


def finalise(a: Atlas, seed=0):
    rgba = a.albedo.resize((W, H), RESAMPLE)
    height = np.asarray(a.height.resize((W, H), RESAMPLE), dtype=np.float32) / 255.0
    rough = a.rough.resize((W, H), RESAMPLE)
    trans = a.trans.resize((W, H), RESAMPLE)
    arr = np.array(rgba, dtype=np.uint8)
    alpha = arr[..., 3]

    # Very subtle fibre/petal variation removes sterile flat fills without baking lighting.
    texture = ndimage.gaussian_filter(np.random.default_rng(seed).normal(0, 1, (H, W)), 0.8)
    texture = np.clip(texture * 3.2, -5.0, 5.0)
    arr[..., :3] = np.clip(arr[..., :3].astype(np.float32) + texture[..., None], 0, 255).astype(np.uint8)

    # Nearest-colour dilation prevents dark fringes in lower mips while keeping straight alpha.
    covered = alpha > 0
    if covered.any():
        _, indices = ndimage.distance_transform_edt(~covered, return_indices=True)
        for channel in range(3):
            source = arr[..., channel]
            source[~covered] = source[indices[0][~covered], indices[1][~covered]]
    rgba = Image.fromarray(arr, "RGBA")

    # Smooth only the height field, retaining painted ribs/veins, and convert with +Y up-image.
    # A distance-derived dome rounds stems, leaf blades and petal silhouettes; painted
    # values still carry the species-specific ridges, grooves and floret bumps.
    dome = np.minimum(ndimage.distance_transform_edt(covered) / 7.0, 1.0)
    h = ndimage.gaussian_filter(height * 0.72 + dome * 0.28, 0.72)
    dy, dx = np.gradient(h)
    strength = 12.0
    nx, ny = -dx * strength, dy * strength
    nz = np.ones_like(nx)
    norm = np.sqrt(nx * nx + ny * ny + nz * nz)
    normal = np.stack((nx / norm, ny / norm, nz / norm), axis=-1)
    normal = np.clip(np.rint((normal * 0.5 + 0.5) * 255.0), 0, 255).astype(np.uint8)
    normal[~covered] = (128, 128, 255)

    r = np.array(rough, dtype=np.uint8)
    t = np.array(trans, dtype=np.uint8)
    r[~covered] = 255
    t[~covered] = 0
    return rgba, Image.fromarray(normal, "RGB"), Image.fromarray(r, "L"), Image.fromarray(t, "L")


def composite(arr, bg):
    rgb = arr[..., :3].astype(np.float32)
    alpha = arr[..., 3:4].astype(np.float32) / 255.0
    return np.clip(rgb * alpha + bg.astype(np.float32) * (1 - alpha), 0, 255).astype(np.uint8)


def preview_sheet(rgba, normal, trans, seed):
    arr = np.array(rgba)
    n = np.array(normal).astype(np.float32) / 127.5 - 1.0
    tr = np.array(trans).astype(np.float32)[..., None] / 255.0
    rng = np.random.default_rng(seed)
    yy = np.linspace(0, 1, H)[:, None, None]
    grass_base = np.array([105, 112, 53], dtype=np.float32)[None, None, :]
    grass = np.broadcast_to(grass_base, (H, W, 3)).copy()
    grass += rng.normal(0, 8, (H, W, 1)) + 10 * np.sin(np.arange(H)[:, None, None] * .13)
    grass = np.clip(grass, 0, 255).astype(np.uint8)
    sky = np.empty((H, W, 3), np.uint8)
    sky[:] = (139, 181, 217)
    sky = np.clip(sky.astype(float) + yy * np.array([25, 20, 10]), 0, 255).astype(np.uint8)
    light = np.array([-0.47, 0.53, 0.70], np.float32)
    light /= np.linalg.norm(light)
    ndotl = np.clip((n * light).sum(axis=2, keepdims=True), 0.16, 1.0)
    lit_arr = arr.copy()
    lit_arr[..., :3] = np.clip(arr[..., :3].astype(float) * (0.35 + 0.78 * ndotl), 0, 255)
    back_arr = arr.copy()
    warm = np.array([255, 205, 127], dtype=np.float32)
    back_arr[..., :3] = np.clip(arr[..., :3] * (0.27 + 0.28 * tr) + warm * tr * 0.58, 0, 255)

    grass_view = Image.fromarray(composite(arr, grass), "RGB")
    sky_view = Image.fromarray(composite(arr, sky), "RGB")
    lit_view = Image.fromarray(composite(lit_arr, grass), "RGB")
    back_view = Image.fromarray(composite(back_arr, grass), "RGB")
    head_bg = grass[:256]
    head = Image.fromarray(composite(arr[:256], head_bg), "RGB").resize((1024, 1024), Image.Resampling.NEAREST)
    sheet = Image.new("RGB", (2048, 1280), (47, 52, 44))
    for i, panel in enumerate((grass_view, sky_view, lit_view, back_view)):
        sheet.paste(panel, (i * 256, 0))
    sheet.paste(head, (1024, 0))
    # 25% distant-read strip and material thumbnails along the foot of the sheet.
    for i, panel in enumerate((grass_view, sky_view, lit_view, back_view)):
        small = panel.resize((64, 256), Image.Resampling.LANCZOS)
        sheet.paste(small, (32 + i * 96, 1024))
    nvis = normal.resize((256, 256), RESAMPLE)
    rvis = Image.merge("RGB", (trans, trans, trans)).resize((256, 256), RESAMPLE)
    sheet.paste(nvis, (512, 1024))
    sheet.paste(rvis, (768, 1024))
    return sheet


def validate(name, rgba, normal, rough, trans):
    alpha = np.array(rgba)[..., 3]
    head_cov = float(np.count_nonzero(alpha[:256] >= 128)) / (256 * 256)
    plant_border = np.concatenate((alpha[256:, :4].ravel(), alpha[256:, -4:].ravel(), alpha[-4:, :].ravel()))
    anchor = alpha[304:333, 112:145]
    defaults_ok = bool(np.all(np.array(normal)[alpha == 0] == (128, 128, 255)))
    defaults_ok &= bool(np.all(np.array(rough)[alpha == 0] == 255) and np.all(np.array(trans)[alpha == 0] == 0))
    print(f"{name:10s} head coverage={head_cov:.1%}, plant border pixels={np.count_nonzero(plant_border)}, "
          f"anchor={np.count_nonzero(anchor >= 128)}, transparent defaults={'ok' if defaults_ok else 'BAD'}")
    if not (0.58 <= head_cov <= 0.76):
        print(f"  warning: {name} head coverage is outside the requested ~60-75% silhouette range")
    if np.count_nonzero(anchor >= 128) == 0:
        raise RuntimeError(f"{name}: no flower pixels at the required plant-head anchor")
    if not defaults_ok:
        raise RuntimeError(f"{name}: transparent material defaults are incorrect")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("out_dir", type=Path)
    parser.add_argument("--only", choices=SPECIES)
    parser.add_argument("--preview", action="store_true")
    args = parser.parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    if args.preview:
        (args.out_dir / "preview").mkdir(parents=True, exist_ok=True)
    chosen = (args.only,) if args.only else SPECIES
    started = time.perf_counter()
    for index, species in enumerate(chosen):
        stamp = time.perf_counter()
        images = finalise(build(species), 700 + SPECIES.index(species))
        rgba, normal, rough, trans = images
        title = species.capitalize()
        rgba.save(args.out_dir / f"{title}.png", optimize=True)
        normal.save(args.out_dir / f"{title}Normal.png", optimize=True)
        rough.save(args.out_dir / f"{title}Roughness.png", optimize=True)
        trans.save(args.out_dir / f"{title}Translucency.png", optimize=True)
        validate(title, *images)
        if args.preview:
            preview_sheet(rgba, normal, trans, 900 + index).save(args.out_dir / "preview" / f"{title}Review.png", optimize=True)
        print(f"  wrote {title} in {time.perf_counter() - stamp:.2f}s")
    print(f"Finished {len(chosen)} species in {time.perf_counter() - started:.2f}s")


if __name__ == "__main__":
    main()
