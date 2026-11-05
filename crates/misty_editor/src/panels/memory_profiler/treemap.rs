//! Squarified tree map layout (Bruls, Huizing, van Wijk): rows of rectangles along the
//! shorter side, a row grows while it makes its worst aspect ratio better.

use egui::{Rect, pos2};

/// Rectangles for `sizes` (largest first gives the best result) filling `area`, in input
/// order. Zero sizes get empty rectangles.
pub fn squarify(sizes: &[f64], area: Rect) -> Vec<Rect> {
    let mut out = vec![Rect::NOTHING; sizes.len()];
    let total: f64 = sizes.iter().filter(|s| **s > 0.0).sum();
    if total <= 0.0 || area.width() <= 0.0 || area.height() <= 0.0 {
        return out;
    }
    let scale = (area.width() as f64 * area.height() as f64) / total;
    let order: Vec<usize> = (0..sizes.len()).filter(|i| sizes[*i] > 0.0).collect();
    let mut free = area;
    let mut row: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < order.len() {
        let side = free.width().min(free.height()) as f64;
        let area_of = |k: usize| sizes[k] * scale;
        let mut candidate = row.clone();
        candidate.push(order[i]);
        if row.is_empty() || worst(&candidate, side, area_of) <= worst(&row, side, area_of) {
            row = candidate;
            i += 1;
        } else {
            free = lay_row(&row, free, area_of, &mut out);
            row.clear();
        }
    }
    if !row.is_empty() {
        lay_row(&row, free, |k| sizes[k] * scale, &mut out);
    }
    out
}

/// Worst aspect ratio of a row laid along a side of length `side`.
fn worst(row: &[usize], side: f64, area: impl Fn(usize) -> f64) -> f64 {
    let sum: f64 = row.iter().map(|k| area(*k)).sum();
    if sum <= 0.0 || side <= 0.0 {
        return f64::INFINITY;
    }
    let (min, max) = row.iter().fold((f64::INFINITY, 0.0f64), |(lo, hi), k| {
        let a = area(*k);
        (lo.min(a), hi.max(a))
    });
    let s2 = side * side;
    (s2 * max / (sum * sum)).max(sum * sum / (s2 * min))
}

/// Place `row` along the shorter side of `free`; returns the space left.
fn lay_row(row: &[usize], free: Rect, area: impl Fn(usize) -> f64, out: &mut [Rect]) -> Rect {
    let sum: f64 = row.iter().map(|k| area(*k)).sum();
    if free.width() >= free.height() {
        // Column on the left.
        let w = (sum / free.height() as f64) as f32;
        let mut y = free.top();
        for k in row {
            let h = (area(*k) / w as f64) as f32;
            out[*k] = Rect::from_min_max(pos2(free.left(), y), pos2(free.left() + w, y + h));
            y += h;
        }
        Rect::from_min_max(pos2(free.left() + w, free.top()), free.max)
    } else {
        // Row along the top.
        let h = (sum / free.width() as f64) as f32;
        let mut x = free.left();
        for k in row {
            let w = (area(*k) / h as f64) as f32;
            out[*k] = Rect::from_min_max(pos2(x, free.top()), pos2(x + w, free.top() + h));
            x += w;
        }
        Rect::from_min_max(pos2(free.left(), free.top() + h), free.max)
    }
}
