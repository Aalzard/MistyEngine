//! Tree look shared by the Hierarchy and the Project folder tree: row stripes, the soft
//! "folder" tint of parent rows, filled foldout triangles and file-explorer connection lines
//! (a vertical line down from each open parent, ├ ticks into its children, └ on the last one).

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

/// Every other row: a barely visible lift over the panel colour.
const STRIPE: Color32 = Color32::from_rgba_premultiplied(5, 5, 5, 5);
/// Rows of objects/folders with children: a little stronger, so parents read like folders.
const PARENT_FILL: Color32 = Color32::from_rgba_premultiplied(16, 18, 22, 24);
/// Pointer over a row: an overlay, so it lifts stripes and parent tints alike.
pub const HOVER: Color32 = Color32::from_rgba_premultiplied(18, 18, 18, 18);
/// Connection lines: soft grey, never white.
const LINE: Color32 = Color32::from_rgba_premultiplied(48, 48, 48, 48);
/// Foldout triangles.
pub const ARROW: Color32 = Color32::from_rgb(150, 150, 150);
pub const ARROW_HOVER: Color32 = Color32::from_rgb(215, 215, 215);
/// Names of parent rows: a touch brighter than plain text.
pub const PARENT_TEXT: Color32 = Color32::from_rgb(232, 232, 232);

/// Connection lines of one row.
#[derive(Clone, Debug, PartialEq)]
pub struct Guide {
    pub depth: usize,
    /// `more[k - 1]`: at level `k` (1..=depth) a sibling follows below, so the line goes on.
    /// The last entry is this row's own: ├ when true, └ when false.
    pub more: Vec<bool>,
    /// The next row is this row's first child: a stub runs down from the triangle.
    pub opens: bool,
}

impl Guide {
    /// The row's own connector, for tests and debugging.
    #[cfg(test)]
    pub fn connector(&self) -> Option<char> {
        self.more.last().map(|m| if *m { '├' } else { '└' })
    }
}

/// Guides for rows listed depth-first. `None` is a break (a scene header): lines never cross it.
pub fn guides(depths: &[Option<usize>]) -> Vec<Guide> {
    let mut out = vec![
        Guide {
            depth: 0,
            more: Vec::new(),
            opens: false
        };
        depths.len()
    ];
    // Walking up from the bottom: `next[k]` = a row at depth k comes before any shallower one.
    let mut next: Vec<bool> = Vec::new();
    let mut below: Option<usize> = None;
    for (i, d) in depths.iter().enumerate().rev() {
        let Some(d) = *d else {
            next.clear();
            below = None;
            continue;
        };
        if next.len() <= d {
            next.resize(d + 1, false);
        }
        out[i] = Guide {
            depth: d,
            more: (1..=d).map(|k| next[k]).collect(),
            opens: below == Some(d + 1),
        };
        next[d] = true;
        next.truncate(d + 1);
        below = Some(d);
    }
    out
}

/// Stripe or parent tint behind a row (draw before hover/selection fills).
pub fn paint_background(painter: &Painter, rect: Rect, index: usize, parent: bool) {
    if parent {
        painter.rect_filled(rect, 0.0, PARENT_FILL);
    } else if index % 2 == 1 {
        painter.rect_filled(rect, 0.0, STRIPE);
    }
}

/// Connection lines for one row. `column(d)` is the x of the triangle centre at depth `d`;
/// `tick_end` is where the horizontal tick stops; `arrow_half` is the triangle's half height.
pub fn paint_guide(
    painter: &Painter,
    rect: Rect,
    g: &Guide,
    column: impl Fn(usize) -> f32,
    tick_end: f32,
    arrow_half: f32,
) {
    let stroke = Stroke::new(1.0, LINE);
    let x_at = |d: usize| painter.round_to_pixel_center(column(d));
    let mid = painter.round_to_pixel_center(rect.center().y);
    let (top, bottom) = (rect.top(), rect.bottom());
    for (k, more) in (1..=g.depth).zip(&g.more) {
        let x = x_at(k - 1);
        if k < g.depth {
            if *more {
                painter.vline(x, top..=bottom, stroke);
            }
            continue;
        }
        // Own level: ├ (line goes on) or └ (ends at the tick).
        painter.vline(x, top..=if *more { bottom } else { mid + 0.5 }, stroke);
        painter.hline(x..=tick_end, mid, stroke);
    }
    if g.opens {
        let x = x_at(g.depth);
        painter.vline(x, rect.center().y + arrow_half + 2.0..=bottom, stroke);
    }
}

/// Filled foldout triangle: ▼ open, ▶ closed.
pub fn paint_arrow(painter: &Painter, center: Pos2, open: bool, color: Color32) {
    let (c, s) = (center, 3.5);
    let points = if open {
        vec![
            pos2(c.x - s, c.y - s * 0.55),
            pos2(c.x + s, c.y - s * 0.55),
            pos2(c.x, c.y + s * 0.75),
        ]
    } else {
        vec![
            pos2(c.x - s * 0.55, c.y - s),
            pos2(c.x + s * 0.75, c.y),
            pos2(c.x - s * 0.55, c.y + s),
        ]
    };
    painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A
    /// ├ B
    /// │ └ C
    /// └ D
    /// E
    #[test]
    fn connectors_follow_siblings() {
        let g = guides(&[Some(0), Some(1), Some(2), Some(1), Some(0)]);
        let c: Vec<_> = g.iter().map(Guide::connector).collect();
        assert_eq!(c, [None, Some('├'), Some('└'), Some('└'), None]);
        assert_eq!(
            g[2].more,
            [true, false],
            "A's line passes C's row down to D"
        );
        assert!(g[0].opens && g[1].opens && !g[2].opens && !g[3].opens);
    }

    /// Scene headers (None) cut the lines: the last child before a header gets └.
    #[test]
    fn breaks_end_lines() {
        let g = guides(&[None, Some(0), Some(1), None, Some(1), Some(0)]);
        assert_eq!(g[2].connector(), Some('└'));
        assert_eq!(g[4].connector(), Some('└'));
        assert!(g[1].opens && !g[5].opens);
    }

    /// Deeper levels: a line keeps running at level 1 while level 2 ends.
    #[test]
    fn nested_last_children() {
        // A, ├B, │├C, │└D, └E, └F (F under E)
        let g = guides(&[Some(0), Some(1), Some(2), Some(2), Some(1), Some(2)]);
        let c: Vec<_> = g.iter().map(Guide::connector).collect();
        assert_eq!(
            c,
            [None, Some('├'), Some('├'), Some('└'), Some('└'), Some('└')]
        );
        assert_eq!(g[3].more, [true, false]);
        assert_eq!(g[5].more, [false, false]);
    }
}
