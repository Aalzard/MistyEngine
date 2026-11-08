//! Geometry of the Image types: 9-slice (Sliced), repeated tiles (Tiled), the cut of a Filled
//! image (horizontal, vertical, radial 90 / 180 / 360) and Preserve Aspect. Everything here is
//! in a local y-up space with the origin at the image rect's lower-left corner; uvs are 0–1
//! across the sprite (v up). `ui::draw` maps it onto the screen and the sprite's texture rect.

use glam::Vec2;

use super::layout::Rect;
use crate::scene::FillMethod;

/// A rectangle of an image and the part of the sprite it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub min: Vec2,
    pub max: Vec2,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
}

/// One axis cut into spans: (start, end, uv start, uv end).
type Span = (f32, f32, f32, f32);

/// Most tiles one image draws (more get larger tiles instead).
const MAX_TILES: usize = 4096;

/// Borders that fit: when the rect is narrower than both borders of an axis, both shrink in
/// proportion (the sprite's corners squeeze rather than overlap).
pub fn fit_borders(size: Vec2, border: [f32; 4]) -> [f32; 4] {
    let [mut l, mut b, mut r, mut t] = border.map(|v| v.max(0.0));
    if l + r > size.x && l + r > 0.0 {
        let k = size.x.max(0.0) / (l + r);
        l *= k;
        r *= k;
    }
    if b + t > size.y && b + t > 0.0 {
        let k = size.y.max(0.0) / (b + t);
        b *= k;
        t *= k;
    }
    [l, b, r, t]
}

/// Three spans along one axis: start border, middle, end border.
fn slice_axis(len: f32, lo: f32, hi: f32, uv_lo: f32, uv_hi: f32) -> [Span; 3] {
    [
        (0.0, lo, 0.0, uv_lo),
        (lo, len - hi, uv_lo, 1.0 - uv_hi),
        (len - hi, len, 1.0 - uv_hi, 1.0),
    ]
}

/// Spans along one axis for tiling: the borders, then the middle repeated every `tile` (the
/// last copy cut short, its uv cut to match).
fn tile_axis(len: f32, lo: f32, hi: f32, uv_lo: f32, uv_hi: f32, tile: f32) -> Vec<Span> {
    let mut out = vec![(0.0, lo, 0.0, uv_lo)];
    let (start, end) = (lo, len - hi);
    let (u0, u1) = (uv_lo, 1.0 - uv_hi);
    if tile <= 1e-4 || u1 <= u0 {
        out.push((start, end, u0, u1));
    } else {
        let mut x = start;
        while x < end - 1e-4 {
            let x1 = (x + tile).min(end);
            let f = (x1 - x) / tile;
            out.push((x, x1, u0, u0 + (u1 - u0) * f));
            x = x1;
        }
    }
    out.push((end, len, 1.0 - uv_hi, 1.0));
    out
}

/// Every non-empty piece of the column × row grid; `skip_centre` leaves out the middle spans of
/// both axes (Fill Center off).
fn grid(cols: &[Span], rows: &[Span], skip_centre: bool) -> Vec<Piece> {
    let mut out = Vec::new();
    let (cn, rn) = (cols.len(), rows.len());
    for (j, r) in rows.iter().enumerate() {
        for (i, c) in cols.iter().enumerate() {
            let centre = i > 0 && i + 1 < cn && j > 0 && j + 1 < rn;
            if (skip_centre && centre) || c.1 - c.0 <= 1e-5 || r.1 - r.0 <= 1e-5 {
                continue;
            }
            out.push(Piece {
                min: Vec2::new(c.0, r.0),
                max: Vec2::new(c.1, r.1),
                uv_min: Vec2::new(c.2, r.2),
                uv_max: Vec2::new(c.3, r.3),
            });
        }
    }
    out
}

/// A 9-slice image of `size`: `border` (left, bottom, right, top) in the same units, `border_uv`
/// the borders as fractions of the sprite. Up to nine pieces (eight without the centre).
pub fn sliced(size: Vec2, border: [f32; 4], border_uv: [f32; 4], fill_center: bool) -> Vec<Piece> {
    let [l, b, r, t] = fit_borders(size, border);
    let [ul, ub, ur, ut] = border_uv;
    let cols = slice_axis(size.x, l, r, ul, ur);
    let rows = slice_axis(size.y, b, t, ub, ut);
    grid(&cols, &rows, !fill_center)
}

/// A tiled image of `size`: the sprite repeated at `tile` (its size in the same units) from the
/// lower-left corner; with borders, the borders stay at the edges and the middle repeats.
pub fn tiled(
    size: Vec2,
    tile: Vec2,
    border: [f32; 4],
    border_uv: [f32; 4],
    fill_center: bool,
) -> Vec<Piece> {
    let [l, b, r, t] = fit_borders(size, border);
    let [ul, ub, ur, ut] = border_uv;
    // The repeated middle of the sprite, at the sprite's scale.
    let mut middle = Vec2::new(
        tile.x * (1.0 - ul - ur).max(0.0),
        tile.y * (1.0 - ub - ut).max(0.0),
    );
    // Keep the number of tiles bounded.
    let inner = Vec2::new(size.x - l - r, size.y - b - t).max(Vec2::ZERO);
    let count = |m: Vec2| {
        let n = |len: f32, m: f32| if m > 1e-4 { (len / m).ceil() } else { 1.0 };
        n(inner.x, m.x) * n(inner.y, m.y)
    };
    while count(middle) > MAX_TILES as f32 {
        middle *= 2.0;
    }
    let cols = tile_axis(size.x, l, r, ul, ur, middle.x);
    let rows = tile_axis(size.y, b, t, ub, ut, middle.y);
    grid(&cols, &rows, !fill_center)
}

/// Unit square corners, anticlockwise from the lower-left.
const CORNERS: [Vec2; 4] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(1.0, 1.0),
    Vec2::new(0.0, 1.0),
];

/// Where a ray from `c` (inside or on the unit square) at `degrees` leaves the square.
fn exit_point(c: Vec2, degrees: f32) -> Vec2 {
    let a = degrees.to_radians();
    let snap = |v: f32| if v.abs() < 1e-6 { 0.0 } else { v };
    let d = Vec2::new(snap(a.cos()), snap(a.sin()));
    let mut t = f32::INFINITY;
    for axis in 0..2 {
        if d[axis] > 0.0 {
            t = t.min((1.0 - c[axis]) / d[axis]);
        } else if d[axis] < 0.0 {
            t = t.min(-c[axis] / d[axis]);
        }
    }
    (c + d * t).clamp(Vec2::ZERO, Vec2::ONE)
}

/// Centre, start angle (degrees, anticlockwise from +x) and sweep (degrees) of a radial fill,
/// before the amount and direction are applied: the sweep runs from `start` anticlockwise.
fn radial_frame(method: FillMethod, origin: u32) -> (Vec2, f32, f32) {
    let o = origin % 4;
    match method {
        FillMethod::Radial90 => {
            // Bottom Left, Top Left, Top Right, Bottom Right: the quarter inside the square.
            let (c, start) = [
                (Vec2::new(0.0, 0.0), 0.0),
                (Vec2::new(0.0, 1.0), 270.0),
                (Vec2::new(1.0, 1.0), 180.0),
                (Vec2::new(1.0, 0.0), 90.0),
            ][o as usize];
            (c, start, 90.0)
        }
        FillMethod::Radial180 => {
            // Bottom, Left, Top, Right edge middles: the half inside the square.
            let (c, start) = [
                (Vec2::new(0.5, 0.0), 0.0),
                (Vec2::new(0.0, 0.5), 270.0),
                (Vec2::new(0.5, 1.0), 180.0),
                (Vec2::new(1.0, 0.5), 90.0),
            ][o as usize];
            (c, start, 180.0)
        }
        _ => {
            // Bottom, Left, Top, Right: the hand starts pointing there.
            let start = [270.0, 180.0, 90.0, 0.0][o as usize];
            (Vec2::splat(0.5), start, 360.0)
        }
    }
}

/// The filled part of a unit square (y up) as a convex-from-its-first-point polygon (a fan
/// around the first vertex covers it). Empty when nothing is filled.
pub fn filled_polygon(method: FillMethod, origin: u32, amount: f32, clockwise: bool) -> Vec<Vec2> {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return Vec::new();
    }
    let rect =
        |min: Vec2, max: Vec2| vec![min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
    match method {
        FillMethod::Horizontal => {
            if origin.is_multiple_of(2) {
                rect(Vec2::ZERO, Vec2::new(amount, 1.0))
            } else {
                rect(Vec2::new(1.0 - amount, 0.0), Vec2::ONE)
            }
        }
        FillMethod::Vertical => {
            if origin.is_multiple_of(2) {
                rect(Vec2::ZERO, Vec2::new(1.0, amount))
            } else {
                rect(Vec2::new(0.0, 1.0 - amount), Vec2::ONE)
            }
        }
        _ => {
            let (c, lo, sweep) = radial_frame(method, origin);
            // Clockwise (seen with y up) runs from the far end of the range backwards; a full
            // turn starts and ends at the origin direction either way.
            let (start, dir) = match (method, clockwise) {
                (FillMethod::Radial360, true) => (lo, -1.0),
                (FillMethod::Radial360, false) => (lo, 1.0),
                (_, true) => (lo + sweep, -1.0),
                (_, false) => (lo, 1.0),
            };
            let span = amount * sweep;
            let end = start + dir * span;
            let mut corners: Vec<(f32, Vec2)> = CORNERS
                .iter()
                .filter(|k| k.distance(c) > 1e-6)
                .filter_map(|k| {
                    let d = *k - c;
                    let a = d.y.atan2(d.x).to_degrees();
                    let rel = (dir * (a - start)).rem_euclid(360.0);
                    (rel > 1e-4 && rel < span - 1e-4).then_some((rel, *k))
                })
                .collect();
            corners.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut poly = vec![c, exit_point(c, start)];
            poly.extend(corners.into_iter().map(|(_, k)| k));
            poly.push(exit_point(c, end));
            poly
        }
    }
}

/// Twice the signed area of a polygon (anticlockwise positive).
pub fn polygon_area(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| poly[i].perp_dot(poly[(i + 1) % n]))
        .sum::<f32>()
        .abs()
        * 0.5
}

/// Clip a polygon (with a uv per vertex) to an axis-aligned rect (Sutherland–Hodgman), for
/// masked Filled images.
pub fn clip_polygon(poly: &[(Vec2, Vec2)], clip: Rect) -> Vec<(Vec2, Vec2)> {
    let mut out: Vec<(Vec2, Vec2)> = poly.to_vec();
    // (axis, keep >= value when true / <= value when false, value)
    let edges = [
        (0, true, clip.min.x),
        (0, false, clip.max.x),
        (1, true, clip.min.y),
        (1, false, clip.max.y),
    ];
    for (axis, ge, v) in edges {
        if out.is_empty() {
            break;
        }
        let inside = |p: Vec2| if ge { p[axis] >= v } else { p[axis] <= v };
        let input = std::mem::take(&mut out);
        for i in 0..input.len() {
            let (a, b) = (input[i], input[(i + 1) % input.len()]);
            let (ia, ib) = (inside(a.0), inside(b.0));
            if ia {
                out.push(a);
            }
            if ia != ib {
                let t = (v - a.0[axis]) / (b.0[axis] - a.0[axis]);
                out.push((a.0.lerp(b.0, t), a.1.lerp(b.1, t)));
            }
        }
    }
    out
}

/// Preserve Aspect: the largest rect of the sprite's `aspect` (width / height) inside `rect`,
/// pushed towards the pivot.
pub fn preserve_aspect(rect: Rect, pivot: Vec2, aspect: f32) -> Rect {
    if aspect <= 0.0 || !aspect.is_finite() {
        return rect;
    }
    let size = rect.size();
    if size.y <= 0.0 {
        return rect;
    }
    let mut out = rect;
    if size.x / size.y > aspect {
        let w = size.y * aspect;
        out.min.x += (size.x - w) * pivot.x;
        out.max.x = out.min.x + w;
    } else {
        let h = size.x / aspect;
        out.min.y += (size.y - h) * pivot.y;
        out.max.y = out.min.y + h;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn sliced_keeps_corners_and_maps_border_uvs() {
        // 200×100 image, 10 px borders all round, borders 25 % of the sprite.
        let p = sliced(Vec2::new(200.0, 100.0), [10.0; 4], [0.25; 4], true);
        assert_eq!(p.len(), 9);
        // Lower-left corner: 10×10 showing the sprite's lower-left quarter of each axis.
        assert_eq!(p[0].min, Vec2::ZERO);
        assert_eq!(p[0].max, Vec2::splat(10.0));
        assert_eq!((p[0].uv_min, p[0].uv_max), (Vec2::ZERO, Vec2::splat(0.25)));
        // Centre stretches 180×80 over the sprite's middle half.
        assert_eq!(p[4].min, Vec2::splat(10.0));
        assert_eq!(p[4].max, Vec2::new(190.0, 90.0));
        assert_eq!(
            (p[4].uv_min, p[4].uv_max),
            (Vec2::splat(0.25), Vec2::splat(0.75))
        );
        // Upper-right corner.
        assert_eq!(p[8].max, Vec2::new(200.0, 100.0));
        assert_eq!(p[8].uv_max, Vec2::ONE);
        assert!(close(p[8].uv_min.x, 0.75));
        // Fill Center off drops the middle piece.
        let hollow = sliced(Vec2::new(200.0, 100.0), [10.0; 4], [0.25; 4], false);
        assert_eq!(hollow.len(), 8);
        assert!(
            !hollow
                .iter()
                .any(|q| q.min == Vec2::splat(10.0) && q.max.x == 190.0)
        );
    }

    #[test]
    fn sliced_borders_squeeze_when_the_rect_is_too_small() {
        let p = sliced(
            Vec2::new(10.0, 100.0),
            [10.0, 0.0, 10.0, 0.0],
            [0.4, 0.0, 0.4, 0.0],
            true,
        );
        // Both 10-unit side borders become 5 and the middle column vanishes.
        let xs: Vec<(f32, f32)> = p.iter().map(|q| (q.min.x, q.max.x)).collect();
        assert_eq!(xs, [(0.0, 5.0), (5.0, 10.0)]);
        assert_eq!(fit_borders(Vec2::splat(100.0), [10.0; 4]), [10.0; 4]);
    }

    #[test]
    fn tiles_repeat_and_the_last_is_cut() {
        // 50×20 rect, 20×20 tiles: two whole tiles and half a tile.
        let p = tiled(
            Vec2::new(50.0, 20.0),
            Vec2::splat(20.0),
            [0.0; 4],
            [0.0; 4],
            true,
        );
        assert_eq!(p.len(), 3);
        assert_eq!((p[2].min.x, p[2].max.x), (40.0, 50.0));
        assert!(close(p[2].uv_max.x, 0.5));
        assert_eq!(p[0].uv_max, Vec2::ONE);
        // With 5-unit borders the middle (10 units of each 20-unit tile) repeats between them.
        let b = tiled(
            Vec2::new(40.0, 20.0),
            Vec2::splat(20.0),
            [5.0; 4],
            [0.25; 4],
            true,
        );
        let middle_row: Vec<(f32, f32)> = b
            .iter()
            .filter(|q| q.min.y == 5.0)
            .map(|q| (q.min.x, q.max.x))
            .collect();
        assert_eq!(
            middle_row,
            [
                (0.0, 5.0),
                (5.0, 15.0),
                (15.0, 25.0),
                (25.0, 35.0),
                (35.0, 40.0)
            ]
        );
        // A huge rect with tiny tiles stays bounded.
        let many = tiled(
            Vec2::splat(10000.0),
            Vec2::splat(1.0),
            [0.0; 4],
            [0.0; 4],
            true,
        );
        assert!(many.len() <= MAX_TILES, "{}", many.len());
    }

    #[test]
    fn linear_fills_cut_from_their_origin() {
        let p = filled_polygon(FillMethod::Horizontal, 0, 0.3, true);
        assert!(close(polygon_area(&p), 0.3));
        assert!(p.iter().all(|v| v.x <= 0.3 + 1e-6));
        let p = filled_polygon(FillMethod::Horizontal, 1, 0.3, true);
        assert!(p.iter().all(|v| v.x >= 0.7 - 1e-6));
        let p = filled_polygon(FillMethod::Vertical, 1, 0.25, true);
        assert!(p.iter().all(|v| v.y >= 0.75 - 1e-6));
        assert!(filled_polygon(FillMethod::Radial360, 0, 0.0, true).is_empty());
    }

    #[test]
    fn radial_360_angles() {
        // Half a turn covers half the square, from any origin, either way round.
        for origin in 0..4 {
            for cw in [true, false] {
                let p = filled_polygon(FillMethod::Radial360, origin, 0.5, cw);
                assert!(close(polygon_area(&p), 0.5), "origin {origin} cw {cw}");
            }
        }
        // An eighth from the top, clockwise: the triangle centre, top middle, top-right corner.
        let p = filled_polygon(FillMethod::Radial360, 2, 0.125, true);
        assert!(close(polygon_area(&p), 0.125));
        assert_eq!(p[0], Vec2::splat(0.5));
        assert!(p[1].distance(Vec2::new(0.5, 1.0)) < 1e-5);
        assert!(p.last().unwrap().distance(Vec2::ONE) < 1e-5, "{p:?}");
        // Anticlockwise from the top goes left instead.
        let p = filled_polygon(FillMethod::Radial360, 2, 0.125, false);
        assert!(p.last().unwrap().distance(Vec2::new(0.0, 1.0)) < 1e-5);
        // From the bottom, a quarter clockwise sweeps to the left edge (down → left).
        let p = filled_polygon(FillMethod::Radial360, 0, 0.25, true);
        assert!(
            p.last().unwrap().distance(Vec2::new(0.0, 0.5)) < 1e-5,
            "{p:?}"
        );
        // A wedge's area follows the angle: 30° from the right edge = tan(30°) / 8.
        let p = filled_polygon(FillMethod::Radial360, 3, 30.0 / 360.0, false);
        assert!(close(polygon_area(&p), 30f32.to_radians().tan() / 8.0));
        // The whole turn is the whole square.
        let p = filled_polygon(FillMethod::Radial360, 1, 1.0, true);
        assert!(close(polygon_area(&p), 1.0));
    }

    #[test]
    fn radial_90_and_180_turn_about_a_corner_and_an_edge() {
        // Bottom-left corner, clockwise: the hand starts up and turns right.
        let p = filled_polygon(FillMethod::Radial90, 0, 0.5, true);
        assert_eq!(p[0], Vec2::ZERO);
        assert!(p[1].distance(Vec2::new(0.0, 1.0)) < 1e-5);
        assert!(p.last().unwrap().distance(Vec2::ONE) < 1e-5);
        assert!(close(polygon_area(&p), 0.5));
        // Anticlockwise starts along the bottom edge.
        let p = filled_polygon(FillMethod::Radial90, 0, 0.25, false);
        assert!(p[1].distance(Vec2::new(1.0, 0.0)) < 1e-5);
        assert!(close(polygon_area(&p), 22.5f32.to_radians().tan() * 0.5));
        // Top-right corner, the whole quarter = the whole square.
        assert!(close(
            polygon_area(&filled_polygon(FillMethod::Radial90, 2, 1.0, true)),
            1.0
        ));
        // Bottom edge middle, clockwise from the left: half covers the left half.
        let p = filled_polygon(FillMethod::Radial180, 0, 0.5, true);
        assert!(close(polygon_area(&p), 0.5));
        assert!(p.iter().all(|v| v.x <= 0.5 + 1e-5), "{p:?}");
        let p = filled_polygon(FillMethod::Radial180, 3, 1.0, false);
        assert!(close(polygon_area(&p), 1.0));
    }

    #[test]
    fn polygons_clip_to_masks_with_their_uvs() {
        let square: Vec<(Vec2, Vec2)> = CORNERS.iter().map(|c| (*c * 10.0, *c)).collect();
        let cut = clip_polygon(
            &square,
            Rect::new(Vec2::new(5.0, -1.0), Vec2::new(20.0, 20.0)),
        );
        let pts: Vec<Vec2> = cut.iter().map(|p| p.0).collect();
        assert!(close(polygon_area(&pts), 50.0));
        assert!(cut.iter().all(|(p, uv)| (*p / 10.0).distance(*uv) < 1e-5));
    }

    #[test]
    fn preserve_aspect_shrinks_towards_the_pivot() {
        let r = Rect::new(Vec2::ZERO, Vec2::new(200.0, 100.0));
        let square = preserve_aspect(r, Vec2::splat(0.5), 1.0);
        assert_eq!(
            square,
            Rect::new(Vec2::new(50.0, 0.0), Vec2::new(150.0, 100.0))
        );
        let left = preserve_aspect(r, Vec2::ZERO, 1.0);
        assert_eq!(left.min.x, 0.0);
        let wide = preserve_aspect(r, Vec2::new(0.5, 1.0), 4.0);
        assert_eq!(
            wide,
            Rect::new(Vec2::new(0.0, 50.0), Vec2::new(200.0, 100.0))
        );
    }
}
