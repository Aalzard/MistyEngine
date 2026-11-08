//! Keyboard and gamepad navigation between selectables: which selectable a move up / down /
//! left / right goes to (each selectable's Navigation mode: None, Horizontal, Vertical,
//! Automatic or Explicit, optionally wrapping), and how a held stick or d-pad repeats moves.
//!
//! Automatic navigation scores every other interactable selectable by how far it lies in the
//! direction of the move, measured from the edge of the current one: `dot / distance²`, so
//! nearer and better-aligned candidates win. Positions are compared in image pixels, so
//! navigation also crosses canvases.

use glam::Vec2;
use hecs::Entity;

use super::input::{interactable, selectable};
use super::tree::UiLayout;
use crate::scene::{Navigation, NavigationMode, Scene};

/// A navigation move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NavDir {
    Up,
    Down,
    Left,
    Right,
}

impl NavDir {
    pub const ALL: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    /// Direction in image pixels (y down).
    pub fn vec(self) -> Vec2 {
        match self {
            Self::Up => Vec2::new(0.0, -1.0),
            Self::Down => Vec2::new(0.0, 1.0),
            Self::Left => Vec2::new(-1.0, 0.0),
            Self::Right => Vec2::new(1.0, 0.0),
        }
    }

    fn horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    /// The move a key name (script key names) stands for.
    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            _ => return None,
        })
    }

    /// The move a stick or d-pad vector (y up) points at, if it is past the dead zone: the
    /// larger axis wins.
    pub fn from_vector(v: Vec2, dead_zone: f32) -> Option<Self> {
        if v.length() < dead_zone {
            return None;
        }
        Some(if v.x.abs() > v.y.abs() {
            if v.x > 0.0 { Self::Right } else { Self::Left }
        } else if v.y > 0.0 {
            Self::Up
        } else {
            Self::Down
        })
    }
}

/// Navigation settings of a selectable (`None` when `e` isn't one).
pub fn navigation_of(scene: &Scene, e: Entity) -> Option<Navigation> {
    selectable(scene, e).map(|s| s.navigation)
}

/// Selectables navigation can reach: laid out, interactable and not set to None.
fn candidates(scene: &Scene, l: &UiLayout) -> Vec<(Entity, Vec2, Vec2)> {
    l.nodes
        .iter()
        .filter(|n| {
            navigation_of(scene, n.entity).is_some_and(|nav| nav.mode != NavigationMode::None)
                && n.interactable
                && interactable(scene, n.entity)
        })
        .filter_map(|n| {
            let (centre, half) = bounds_in_image(l, n.entity)?;
            Some((n.entity, centre, half))
        })
        .collect()
}

/// Centre and half size (image pixels) of a laid-out object.
fn bounds_in_image(l: &UiLayout, e: Entity) -> Option<(Vec2, Vec2)> {
    let n = l.node(e)?;
    let pts: Vec<Vec2> = n
        .corners()
        .iter()
        .filter_map(|c| l.to_target(n.canvas, *c))
        .collect();
    if pts.len() < 4 {
        return None;
    }
    let min = pts.iter().fold(Vec2::splat(f32::MAX), |a, p| a.min(*p));
    let max = pts.iter().fold(Vec2::splat(f32::MIN), |a, p| a.max(*p));
    Some(((min + max) * 0.5, (max - min) * 0.5))
}

/// The selectable a move in `dir` from `cur` selects (`None` = stay).
pub fn find_selectable(scene: &Scene, l: &UiLayout, cur: Entity, dir: NavDir) -> Option<Entity> {
    let nav = navigation_of(scene, cur)?;
    match nav.mode {
        NavigationMode::None => return None,
        NavigationMode::Explicit => {
            let target = match dir {
                NavDir::Up => nav.select_on_up,
                NavDir::Down => nav.select_on_down,
                NavDir::Left => nav.select_on_left,
                NavDir::Right => nav.select_on_right,
            };
            return target.filter(|t| scene.contains(*t) && scene.active_in_hierarchy(*t));
        }
        NavigationMode::Horizontal if !dir.horizontal() => return None,
        NavigationMode::Vertical if dir.horizontal() => return None,
        _ => {}
    }
    let (centre, half) = bounds_in_image(l, cur)?;
    let d = dir.vec();
    // Measure from the edge of the current rect facing the move.
    let from = centre + d * half;
    let others: Vec<(Entity, Vec2, Vec2)> = candidates(scene, l)
        .into_iter()
        .filter(|c| c.0 != cur)
        .collect();
    let mut best = None;
    let mut best_score = 0.0;
    for (e, pos, _) in &others {
        let v = *pos - from;
        let dot = v.dot(d);
        if dot <= 0.0 {
            continue;
        }
        let score = dot / v.length_squared().max(1e-3);
        if score > best_score {
            best_score = score;
            best = Some(*e);
        }
    }
    let wraps = nav.wrap_around
        && matches!(
            nav.mode,
            NavigationMode::Horizontal | NavigationMode::Vertical
        );
    if best.is_none() && wraps {
        // Come back in at the far end: the candidate furthest the other way, nearest the line.
        best = others
            .iter()
            .map(|(e, pos, _)| {
                let v = *pos - centre;
                (*e, -v.dot(d), (v - d * v.dot(d)).length())
            })
            .filter(|(_, back, _)| *back > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.2.total_cmp(&a.2)))
            .map(|(e, _, _)| e);
    }
    best
}

/// Every navigation link of the laid-out selectables (from, to, move), for the Scene view's
/// Visualize Navigation.
pub fn links(scene: &Scene, l: &UiLayout) -> Vec<(Entity, Entity, NavDir)> {
    let mut out = Vec::new();
    for n in &l.nodes {
        if navigation_of(scene, n.entity).is_none() {
            continue;
        }
        for dir in NavDir::ALL {
            if let Some(to) = find_selectable(scene, l, n.entity, dir) {
                out.push((n.entity, to, dir));
            }
        }
    }
    out
}

/// Turns a held Navigate vector into moves: one at once, then after `delay` seconds one every
/// `rate` seconds while the same direction is held.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveRepeat {
    held: Option<NavDir>,
    /// Seconds until the next repeat.
    wait: f32,
}

/// Navigate vectors shorter than this don't move.
pub const MOVE_DEAD_ZONE: f32 = 0.6;

impl MoveRepeat {
    /// This frame's move for the vector `v` (y up), `dt` seconds after the last frame.
    pub fn step(&mut self, v: Vec2, dt: f32, delay: f32, rate: f32) -> Option<NavDir> {
        let Some(dir) = NavDir::from_vector(v, MOVE_DEAD_ZONE) else {
            self.held = None;
            return None;
        };
        if self.held != Some(dir) {
            self.held = Some(dir);
            self.wait = delay.max(0.0);
            return Some(dir);
        }
        self.wait -= dt;
        if self.wait <= 0.0 {
            self.wait += rate.max(0.01);
            return Some(dir);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{
        Canvas, CanvasGroup, CanvasScaler, RectTransform, UiButton, UiImage, UiScaleMode,
    };
    use crate::ui::tree::layout;

    /// A 3×2 grid of buttons (100×40, 20 apart) on an 800×600 canvas at scale 1.
    fn grid() -> (Scene, Vec<Entity>) {
        let mut s = Scene::new("Nav");
        let canvas = s.spawn("Canvas", None);
        s.add(canvas, Canvas::default());
        s.add(
            canvas,
            CanvasScaler {
                mode: UiScaleMode::ConstantPixelSize,
                ..Default::default()
            },
        );
        let mut buttons = Vec::new();
        for row in 0..2 {
            for col in 0..3 {
                let b = s.spawn(&format!("B{row}{col}"), Some(canvas));
                let mut rt = RectTransform::sized(100.0, 40.0);
                rt.anchored_position = Vec2::new(col as f32 * 120.0 - 120.0, -(row as f32) * 60.0);
                s.add(b, rt);
                s.add(b, UiImage::default());
                s.add(b, UiButton::default());
                buttons.push(b);
            }
        }
        (s, buttons)
    }

    fn nav(s: &mut Scene, e: Entity, f: impl FnOnce(&mut Navigation)) {
        f(&mut s.world.get::<&mut UiButton>(e).unwrap().navigation);
    }

    #[test]
    fn automatic_navigation_picks_the_neighbour_in_each_direction() {
        let (s, b) = grid();
        let l = layout(&s, Vec2::new(800.0, 600.0));
        let go = |from: usize, d: NavDir| find_selectable(&s, &l, b[from], d);
        // Top row: B00 B01 B02, bottom row: B10 B11 B12.
        assert_eq!(go(0, NavDir::Right), Some(b[1]));
        assert_eq!(go(1, NavDir::Right), Some(b[2]));
        assert_eq!(go(2, NavDir::Right), None, "nothing further right");
        assert_eq!(go(1, NavDir::Down), Some(b[4]));
        assert_eq!(go(4, NavDir::Up), Some(b[1]));
        assert_eq!(go(5, NavDir::Left), Some(b[4]));
        assert_eq!(go(0, NavDir::Up), None);
        // A full walk right then down visits in reading order.
        let mut order = vec![b[0]];
        let mut cur = b[0];
        while let Some(n) = find_selectable(&s, &l, cur, NavDir::Right) {
            order.push(n);
            cur = n;
        }
        assert_eq!(order, b[..3]);
    }

    #[test]
    fn modes_restrict_wrap_and_explicit_targets_override() {
        let (mut s, b) = grid();
        nav(&mut s, b[0], |n| n.mode = NavigationMode::Horizontal);
        nav(&mut s, b[2], |n| {
            n.mode = NavigationMode::Horizontal;
            n.wrap_around = true;
        });
        nav(&mut s, b[3], |n| {
            n.mode = NavigationMode::Explicit;
            n.select_on_right = Some(b[5]);
        });
        nav(&mut s, b[4], |n| n.mode = NavigationMode::None);
        let l = layout(&s, Vec2::new(800.0, 600.0));
        let go = |from: usize, d: NavDir| find_selectable(&s, &l, b[from], d);
        assert_eq!(go(0, NavDir::Down), None, "horizontal ignores down");
        assert_eq!(go(0, NavDir::Right), Some(b[1]));
        assert_eq!(go(2, NavDir::Right), Some(b[0]), "wraps to the row's start");
        assert_eq!(go(3, NavDir::Right), Some(b[5]), "explicit");
        assert_eq!(go(3, NavDir::Up), None, "explicit without a target stays");
        // B11 is None and so unreachable: down from B01 goes to a diagonal neighbour instead.
        let down = go(1, NavDir::Down);
        assert!(down == Some(b[3]) || down == Some(b[5]), "{down:?}");
        assert_eq!(
            go(4, NavDir::Left),
            None,
            "a None selectable doesn't navigate"
        );
    }

    #[test]
    fn disabled_and_group_blocked_selectables_are_skipped() {
        let (mut s, b) = grid();
        let blocked = CanvasGroup {
            interactable: false,
            ..Default::default()
        };
        let right = |s: &Scene| {
            let l = layout(s, Vec2::new(800.0, 600.0));
            find_selectable(s, &l, b[0], NavDir::Right)
        };
        // B01 disabled: the nearest one to the right is now B11, down and right.
        s.world.get::<&mut UiButton>(b[1]).unwrap().interactable = false;
        assert_eq!(right(&s), Some(b[4]));
        // B11 behind a Canvas Group with Interactable off: B02, along the row.
        s.add(b[4], blocked.clone());
        assert_eq!(right(&s), Some(b[2]));
        // B02 too: the last one, B12.
        s.add(b[2], blocked);
        assert_eq!(right(&s), Some(b[5]));
    }

    #[test]
    fn held_moves_repeat_after_a_delay() {
        let mut r = MoveRepeat::default();
        let right = Vec2::new(1.0, 0.0);
        let mut moves = Vec::new();
        // 1.2 s held at 50 frames a second: the first move, then after 0.5 s every 0.1 s.
        for _ in 0..60 {
            moves.push(r.step(right, 0.02, 0.5, 0.1));
        }
        let n = moves.iter().filter(|m| m.is_some()).count();
        assert_eq!(moves[0], Some(NavDir::Right));
        assert!((7..=9).contains(&n), "{n} moves");
        // Letting go and pressing again moves at once; small tilts don't move.
        assert_eq!(r.step(Vec2::ZERO, 0.02, 0.5, 0.1), None);
        assert_eq!(
            r.step(Vec2::new(0.0, -0.9), 0.02, 0.5, 0.1),
            Some(NavDir::Down)
        );
        assert_eq!(r.step(Vec2::new(0.3, 0.0), 0.02, 0.5, 0.1), None);
    }
}
