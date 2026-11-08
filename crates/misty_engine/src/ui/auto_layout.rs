//! Auto layout: min / preferred / flexible sizes of UI objects, Horizontal / Vertical / Grid
//! Layout Group placement and Content Size Fitter sizing. Rects are canvas units, y up; layout
//! offsets run from the left and from the top like the familiar layout groups.

use glam::Vec2;
use hecs::Entity;

use super::layout::Rect;
use super::text::{builtin_font, text_size};
use crate::scene::{
    ContentSizeFitter, FitMode, GridAxis, GridConstraint, GridCorner, GridLayoutGroup,
    HorizontalLayoutGroup, LayoutElement, LinearLayout, RectTransform, Scene, UiText,
    VerticalLayoutGroup,
};

/// Layout sizes of an object along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sizes {
    pub min: f32,
    pub preferred: f32,
    pub flexible: f32,
}

/// A layout group on an object.
enum Group {
    Linear { l: LinearLayout, vertical: bool },
    Grid(GridLayoutGroup),
}

fn group(scene: &Scene, e: Entity) -> Option<Group> {
    let w = &scene.world;
    if let Ok(h) = w.get::<&HorizontalLayoutGroup>(e)
        && h.0.enabled
    {
        return Some(Group::Linear {
            l: h.0.clone(),
            vertical: false,
        });
    }
    if let Ok(v) = w.get::<&VerticalLayoutGroup>(e)
        && v.0.enabled
    {
        return Some(Group::Linear {
            l: v.0.clone(),
            vertical: true,
        });
    }
    w.get::<&GridLayoutGroup>(e)
        .ok()
        .filter(|g| g.enabled)
        .map(|g| Group::Grid((*g).clone()))
}

/// Does `e` have an enabled layout group (it places its children)?
pub fn has_group(scene: &Scene, e: Entity) -> bool {
    group(scene, e).is_some()
}

/// Children a layout group places: active, with a Rect Transform, not Ignore Layout.
fn layout_children(scene: &Scene, e: Entity) -> Vec<Entity> {
    scene
        .children(e)
        .into_iter()
        .filter(|c| {
            scene.game_object(*c).active
                && scene.world.get::<&RectTransform>(*c).is_ok()
                && !scene
                    .world
                    .get::<&LayoutElement>(*c)
                    .is_ok_and(|le| le.enabled && le.ignore_layout)
        })
        .collect()
}

/// Min / preferred / flexible size of `e` along `axis` (0 = width, 1 = height). `width` is the
/// width `e` will have, so wrapped text and layout groups can work out their height.
pub fn sizes(scene: &Scene, e: Entity, axis: usize, width: Option<f32>) -> Sizes {
    let mut s = Sizes::default();
    if let Some(g) = group(scene, e) {
        s = group_sizes(scene, e, &g, axis, width);
    } else if let Ok(t) = scene.world.get::<&UiText>(e)
        && t.enabled
    {
        let wrap = if t.wrap { width } else { None };
        let size = text_size(
            builtin_font(),
            &t.text,
            t.scaled_font_size(),
            t.line_spacing,
            wrap,
        );
        s.preferred = size[axis];
    }
    // A Layout Element overrides whatever it sets.
    if let Ok(le) = scene.world.get::<&LayoutElement>(e)
        && le.enabled
    {
        let (min, pref, flex) = if axis == 0 {
            (le.min_width, le.preferred_width, le.flexible_width)
        } else {
            (le.min_height, le.preferred_height, le.flexible_height)
        };
        s.min = min.unwrap_or(s.min);
        s.preferred = pref.unwrap_or(s.preferred);
        s.flexible = flex.unwrap_or(s.flexible);
    }
    s.preferred = s.preferred.max(s.min);
    s
}

fn group_sizes(scene: &Scene, e: Entity, g: &Group, axis: usize, width: Option<f32>) -> Sizes {
    let children = layout_children(scene, e);
    match g {
        Group::Linear { l, vertical } => {
            // Heights depend on the widths the children get (wrapped text).
            let widths = width.filter(|_| axis == 1).map(|w| {
                linear_place(scene, l, *vertical, &children, 0, w, None)
                    .into_iter()
                    .map(|(_, s)| s)
                    .collect::<Vec<_>>()
            });
            linear_totals(scene, l, *vertical, &children, axis, widths.as_deref())
        }
        Group::Grid(g) => grid_sizes(g, children.len(), axis, width),
    }
}

/// The size an object sets itself along `axis` (its Size Delta).
fn sizes_or_own(scene: &Scene, e: Entity, axis: usize) -> f32 {
    scene
        .world
        .get::<&RectTransform>(e)
        .map_or(0.0, |rt| rt.size_delta[axis])
        .max(0.0)
}

/// Sizes of one child as a linear group sees it.
fn child_sizes(
    scene: &Scene,
    l: &LinearLayout,
    child: Entity,
    axis: usize,
    width: Option<f32>,
) -> Sizes {
    let (control, expand) = if axis == 0 {
        (l.control_child_width, l.child_force_expand_width)
    } else {
        (l.control_child_height, l.child_force_expand_height)
    };
    let mut s = if control {
        sizes(scene, child, axis, width)
    } else {
        let own = sizes_or_own(scene, child, axis);
        Sizes {
            min: own,
            preferred: own,
            flexible: 0.0,
        }
    };
    if expand {
        s.flexible = s.flexible.max(1.0);
    }
    s
}

fn padding_along(l: &LinearLayout, axis: usize) -> f32 {
    if axis == 0 {
        l.padding.horizontal()
    } else {
        l.padding.vertical()
    }
}

/// Total sizes of a linear group's children along `axis`.
fn linear_totals(
    scene: &Scene,
    l: &LinearLayout,
    vertical: bool,
    children: &[Entity],
    axis: usize,
    widths: Option<&[f32]>,
) -> Sizes {
    let pad = padding_along(l, axis);
    let across = vertical != (axis == 1);
    let mut t = Sizes {
        min: pad,
        preferred: pad,
        flexible: 0.0,
    };
    for (i, c) in children.iter().enumerate() {
        let s = child_sizes(scene, l, *c, axis, widths.map(|w| w[i]));
        if across {
            t.min = t.min.max(s.min + pad);
            t.preferred = t.preferred.max(s.preferred + pad);
            t.flexible = t.flexible.max(s.flexible);
        } else {
            t.min += s.min + l.spacing;
            t.preferred += s.preferred + l.spacing;
            t.flexible += s.flexible;
        }
    }
    if !across && !children.is_empty() {
        t.min -= l.spacing;
        t.preferred -= l.spacing;
    }
    t.preferred = t.preferred.max(t.min);
    t
}

/// Offset of a block of `required` size (padding excluded) inside `available`, by alignment.
fn start_offset(
    l_pad: (f32, f32),
    align: crate::scene::TextAnchor,
    axis: usize,
    available: f32,
    required: f32,
) -> f32 {
    let (col, row) = align.cell();
    let a = if axis == 0 { col } else { row } as f32 * 0.5;
    let surplus = available - (required + l_pad.0 + l_pad.1);
    l_pad.0 + surplus * a
}

fn pads(l: &LinearLayout, axis: usize) -> (f32, f32) {
    if axis == 0 {
        (l.padding.left, l.padding.right)
    } else {
        (l.padding.top, l.padding.bottom)
    }
}

/// (offset from the start edge, size) of each child along `axis` in a rect of `size`.
fn linear_place(
    scene: &Scene,
    l: &LinearLayout,
    vertical: bool,
    children: &[Entity],
    axis: usize,
    size: f32,
    widths: Option<&[f32]>,
) -> Vec<(f32, f32)> {
    let control = if axis == 0 {
        l.control_child_width
    } else {
        l.control_child_height
    };
    let (col, row) = l.child_alignment.cell();
    let align = if axis == 0 { col } else { row } as f32 * 0.5;
    let own = |c: Entity| sizes_or_own(scene, c, axis);
    let across = vertical != (axis == 1);
    let sizes: Vec<Sizes> = children
        .iter()
        .enumerate()
        .map(|(i, c)| child_sizes(scene, l, *c, axis, widths.map(|w| w[i])))
        .collect();
    if across {
        let inner = size - padding_along(l, axis);
        return children
            .iter()
            .zip(&sizes)
            .map(|(c, s)| {
                let max = if s.flexible > 0.0 { size } else { s.preferred };
                let required = inner.clamp(s.min, max.max(s.min));
                let start = start_offset(pads(l, axis), l.child_alignment, axis, size, required);
                if control {
                    (start, required)
                } else {
                    let sz = own(*c);
                    (start + (required - sz) * align, sz)
                }
            })
            .collect();
    }
    let total = linear_totals(scene, l, vertical, children, axis, widths);
    let mut pos = pads(l, axis).0;
    let mut flex_mul = 0.0;
    let surplus = size - total.preferred;
    if surplus > 0.0 {
        if total.flexible == 0.0 {
            pos = start_offset(
                pads(l, axis),
                l.child_alignment,
                axis,
                size,
                total.preferred - padding_along(l, axis),
            );
        } else {
            flex_mul = surplus / total.flexible;
        }
    }
    let lerp = if total.min != total.preferred {
        ((size - total.min) / (total.preferred - total.min)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut order: Vec<usize> = (0..children.len()).collect();
    if l.reverse_arrangement {
        order.reverse();
    }
    let mut out = vec![(0.0, 0.0); children.len()];
    for i in order {
        let s = sizes[i];
        let cell = s.min + (s.preferred - s.min) * lerp + s.flexible * flex_mul;
        out[i] = if control {
            (pos, cell)
        } else {
            let sz = own(children[i]);
            (pos + (cell - sz) * align, sz)
        };
        pos += cell + l.spacing;
    }
    out
}

/// Columns and rows a grid of `n` children uses inside `size`.
fn grid_counts(g: &GridLayoutGroup, n: usize, size: Vec2) -> (usize, usize) {
    let n = n.max(1);
    let fit = |axis: usize, pad: f32| {
        let step = (g.cell_size[axis] + g.spacing[axis]).max(1e-3);
        ((size[axis] - pad + g.spacing[axis] + 0.001) / step)
            .floor()
            .max(1.0) as usize
    };
    let count = g.constraint_count.max(1) as usize;
    let by_rows = match g.constraint {
        GridConstraint::FixedColumnCount => false,
        GridConstraint::FixedRowCount => true,
        GridConstraint::Flexible => g.start_axis == GridAxis::Vertical,
    };
    if by_rows {
        let rows = match g.constraint {
            GridConstraint::FixedRowCount => count,
            _ => fit(1, g.padding.vertical()),
        }
        .clamp(1, n);
        (n.div_ceil(rows), rows)
    } else {
        let cols = match g.constraint {
            GridConstraint::FixedColumnCount => count,
            _ => fit(0, g.padding.horizontal()),
        }
        .clamp(1, n);
        (cols, n.div_ceil(cols))
    }
}

fn grid_sizes(g: &GridLayoutGroup, n: usize, axis: usize, width: Option<f32>) -> Sizes {
    let span = |count: usize, axis: usize| {
        let pad = if axis == 0 {
            g.padding.horizontal()
        } else {
            g.padding.vertical()
        };
        pad + (g.cell_size[axis] + g.spacing[axis]) * count as f32 - g.spacing[axis]
    };
    let count = g.constraint_count.max(1) as usize;
    let (min, pref) = if axis == 0 {
        match g.constraint {
            GridConstraint::FixedColumnCount => (span(count, 0), span(count, 0)),
            GridConstraint::FixedRowCount => {
                let c = n.div_ceil(count).max(1);
                (span(c, 0), span(c, 0))
            }
            GridConstraint::Flexible => (span(1, 0), span((n as f32).sqrt().ceil() as usize, 0)),
        }
    } else {
        let rows = match g.constraint {
            GridConstraint::FixedColumnCount => n.div_ceil(count),
            GridConstraint::FixedRowCount => count,
            GridConstraint::Flexible => {
                let w = width.unwrap_or_else(|| span((n as f32).sqrt().ceil() as usize, 0));
                grid_counts(g, n, Vec2::new(w, f32::MAX / 4.0)).1
            }
        };
        let r = span(rows.max(1), 1);
        (r, r)
    };
    Sizes {
        min,
        preferred: pref,
        flexible: 0.0,
    }
}

/// Grid cell rects (canvas units, y up) of `n` children inside `rect`.
fn grid_place(g: &GridLayoutGroup, n: usize, rect: Rect) -> Vec<Rect> {
    let size = rect.size();
    let (cols, rows) = grid_counts(g, n, size);
    let required = Vec2::new(
        cols as f32 * g.cell_size.x + (cols as f32 - 1.0) * g.spacing.x,
        rows as f32 * g.cell_size.y + (rows as f32 - 1.0) * g.spacing.y,
    );
    let (col, row) = g.child_alignment.cell();
    let start = Vec2::new(
        g.padding.left + (size.x - required.x - g.padding.horizontal()) * col as f32 * 0.5,
        g.padding.top + (size.y - required.y - g.padding.vertical()) * row as f32 * 0.5,
    );
    let flip_x = matches!(
        g.start_corner,
        GridCorner::UpperRight | GridCorner::LowerRight
    );
    let flip_y = matches!(
        g.start_corner,
        GridCorner::LowerLeft | GridCorner::LowerRight
    );
    (0..n)
        .map(|i| {
            let (mut x, mut y) = if g.start_axis == GridAxis::Horizontal {
                (i % cols, i / cols)
            } else {
                (i / rows, i % rows)
            };
            if flip_x {
                x = cols - 1 - x;
            }
            if flip_y {
                y = rows - 1 - y;
            }
            let off = start + Vec2::new(x as f32, y as f32) * (g.cell_size + g.spacing);
            let left = rect.min.x + off.x;
            let top = rect.max.y - off.y;
            Rect::new(
                Vec2::new(left, top - g.cell_size.y),
                Vec2::new(left + g.cell_size.x, top),
            )
        })
        .collect()
}

/// Rects of the children `e`'s layout group places inside `rect` (`None` = no group).
pub fn arrange(scene: &Scene, e: Entity, rect: Rect) -> Option<Vec<(Entity, Rect)>> {
    let g = group(scene, e)?;
    let children = layout_children(scene, e);
    let rects = match g {
        Group::Grid(g) => grid_place(&g, children.len(), rect),
        Group::Linear { l, vertical } => {
            let size = rect.size();
            let xs = linear_place(scene, &l, vertical, &children, 0, size.x, None);
            let widths: Vec<f32> = xs.iter().map(|x| x.1).collect();
            let ys = linear_place(scene, &l, vertical, &children, 1, size.y, Some(&widths));
            xs.iter()
                .zip(&ys)
                .map(|((x, w), (y, h))| {
                    let top = rect.max.y - y;
                    Rect::new(
                        Vec2::new(rect.min.x + x, top - h),
                        Vec2::new(rect.min.x + x + w, top),
                    )
                })
                .collect()
        }
    };
    Some(children.into_iter().zip(rects).collect())
}

/// `rect` resized by `e`'s Content Size Fitter (around its pivot), if it has one.
pub fn fit(scene: &Scene, e: Entity, rect: Rect) -> Rect {
    let Some(f) = scene
        .world
        .get::<&ContentSizeFitter>(e)
        .ok()
        .filter(|f| f.enabled)
        .map(|f| (*f).clone())
    else {
        return rect;
    };
    let pivot = scene
        .world
        .get::<&RectTransform>(e)
        .map_or(Vec2::splat(0.5), |rt| rt.pivot);
    let mut size = rect.size();
    for (axis, mode) in [(0, f.horizontal_fit), (1, f.vertical_fit)] {
        let width = (axis == 1).then_some(size.x);
        let s = sizes(scene, e, axis, width);
        match mode {
            FitMode::Unconstrained => {}
            FitMode::MinSize => size[axis] = s.min,
            FitMode::PreferredSize => size[axis] = s.preferred,
        }
    }
    let anchor = rect.min + rect.size() * pivot;
    let min = anchor - size * pivot;
    Rect::new(min, min + size)
}

#[cfg(test)]
#[path = "auto_layout_tests.rs"]
mod tests;
