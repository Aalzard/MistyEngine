//! Splines: cubic Bézier splines made of knots (position, rotation, tangents, tangent mode),
//! evaluated by distance along the curve, and the components built on them (`components`):
//! Spline Container, Spline Extrude (`extrude`), Spline Instantiate (`instantiate`) and Spline
//! Animate (`animate`).
//!
//! A spline's `t` runs from 0 to 1 by distance: `t = 0.5` is half way along its length, not half
//! way through its knots. [`SplinePath`] holds the curves and an arc-length table of one spline
//! so repeated queries are cheap; build it with [`Spline::path`].

pub mod animate;
pub mod components;
pub mod extrude;
pub mod instantiate;

use glam::{Mat3, Quat, Vec3};
use serde::{Deserialize, Serialize};

pub use components::{
    SPLINE_REFS, SplineAnimate, SplineContainer, SplineExtrude, SplineInstance, SplineInstantiate,
};

/// How a knot's tangents are set (like the reference engine's tangent modes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TangentMode {
    /// Tangents follow the neighbouring knots for a smooth curve (`tension` sets their length).
    #[default]
    AutoSmooth,
    /// Straight segments to the neighbours (a corner at the knot).
    Linear,
    /// Bézier: the in tangent is the out tangent reversed, same length.
    Mirrored,
    /// Bézier: the in tangent points opposite the out tangent but keeps its own length.
    Continuous,
    /// Bézier: both tangents are free.
    Broken,
}

impl TangentMode {
    pub const ALL: [TangentMode; 5] = [
        Self::AutoSmooth,
        Self::Linear,
        Self::Mirrored,
        Self::Continuous,
        Self::Broken,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::AutoSmooth => "Auto Smooth",
            Self::Linear => "Linear",
            Self::Mirrored => "Bezier Mirrored",
            Self::Continuous => "Bezier Continuous",
            Self::Broken => "Bezier Broken",
        }
    }

    /// The tangents are edited by hand (shown as handles).
    pub fn is_bezier(self) -> bool {
        matches!(self, Self::Mirrored | Self::Continuous | Self::Broken)
    }
}

/// Default Auto Smooth tension (tangent length as a share of the distance to the neighbour).
pub const DEFAULT_TENSION: f32 = 1.0 / 3.0;

/// One control point. Tangents are in the knot's local frame (`rotation`), `tangent_in` usually
/// pointing back (−Z) and `tangent_out` forward (+Z).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Knot {
    pub position: Vec3,
    pub rotation: Quat,
    pub tangent_in: Vec3,
    pub tangent_out: Vec3,
    pub mode: TangentMode,
    /// Auto Smooth tangent length (share of the distance to each neighbour).
    pub tension: f32,
}

impl Default for Knot {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            tangent_in: Vec3::new(0.0, 0.0, -1.0),
            tangent_out: Vec3::new(0.0, 0.0, 1.0),
            mode: TangentMode::AutoSmooth,
            tension: DEFAULT_TENSION,
        }
    }
}

impl Knot {
    pub fn at(position: Vec3) -> Self {
        Self {
            position,
            ..Default::default()
        }
    }

    /// A Bézier (Mirrored) knot at `position` whose out tangent is `out` (spline space).
    pub fn bezier(position: Vec3, out: Vec3) -> Self {
        let len = out.length();
        let rotation = look_rotation(out, Vec3::Y);
        Self {
            position,
            rotation,
            tangent_in: Vec3::new(0.0, 0.0, -len),
            tangent_out: Vec3::new(0.0, 0.0, len),
            mode: TangentMode::Mirrored,
            tension: DEFAULT_TENSION,
        }
    }

    /// Make the stored tangents agree with the mode after `tangent_out` (or `tangent_in`, when
    /// `in_changed`) was edited.
    pub fn enforce(&mut self, in_changed: bool) {
        match self.mode {
            TangentMode::Mirrored => {
                if in_changed {
                    self.tangent_out = -self.tangent_in;
                } else {
                    self.tangent_in = -self.tangent_out;
                }
            }
            TangentMode::Continuous => {
                if in_changed {
                    let len = self.tangent_out.length();
                    self.tangent_out = -self.tangent_in.normalize_or_zero() * len;
                } else {
                    let len = self.tangent_in.length();
                    self.tangent_in = -self.tangent_out.normalize_or_zero() * len;
                }
            }
            _ => {}
        }
    }

    /// Up direction of the knot (spline space).
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }
}

/// Rotation turning +Z to `forward` and +Y as close as possible to `up`.
pub fn look_rotation(forward: Vec3, up: Vec3) -> Quat {
    let f = forward.normalize_or_zero();
    if f == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    let mut u = (up - f * up.dot(f)).normalize_or_zero();
    if u == Vec3::ZERO {
        // Forward is (anti)parallel to up: pick any perpendicular.
        u = f.any_orthonormal_vector();
    }
    let r = u.cross(f);
    Quat::from_mat3(&Mat3::from_cols(r, u, f)).normalize()
}

/// A cubic Bézier segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BezierCurve {
    pub p0: Vec3,
    pub p1: Vec3,
    pub p2: Vec3,
    pub p3: Vec3,
}

impl BezierCurve {
    pub fn position(&self, u: f32) -> Vec3 {
        let v = 1.0 - u;
        self.p0 * (v * v * v)
            + self.p1 * (3.0 * v * v * u)
            + self.p2 * (3.0 * v * u * u)
            + self.p3 * (u * u * u)
    }

    /// First derivative (velocity) at `u`.
    pub fn derivative(&self, u: f32) -> Vec3 {
        let v = 1.0 - u;
        (self.p1 - self.p0) * (3.0 * v * v)
            + (self.p2 - self.p1) * (6.0 * v * u)
            + (self.p3 - self.p2) * (3.0 * u * u)
    }

    pub fn second_derivative(&self, u: f32) -> Vec3 {
        (self.p2 - self.p1 * 2.0 + self.p0) * (6.0 * (1.0 - u))
            + (self.p3 - self.p2 * 2.0 + self.p1) * (6.0 * u)
    }

    /// Direction of travel; falls back to the chord where the velocity vanishes.
    pub fn direction(&self, u: f32) -> Vec3 {
        let d = self.derivative(u);
        if d.length_squared() > 1e-12 {
            return d.normalize();
        }
        // A zero-length tangent at an end: look a little inside the curve.
        let inner = self.derivative(u.clamp(1e-3, 1.0 - 1e-3));
        if inner.length_squared() > 1e-12 {
            return inner.normalize();
        }
        (self.p3 - self.p0).normalize_or_zero()
    }
}

/// One spline: knots in order, optionally closed into a loop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spline {
    pub knots: Vec<Knot>,
    pub closed: bool,
}

impl Spline {
    pub fn new(knots: Vec<Knot>, closed: bool) -> Self {
        Self { knots, closed }
    }

    /// Number of Bézier segments.
    pub fn curve_count(&self) -> usize {
        match self.knots.len() {
            0 | 1 => 0,
            n if self.closed => n,
            n => n - 1,
        }
    }

    fn neighbours(&self, i: usize) -> (Option<usize>, Option<usize>) {
        let n = self.knots.len();
        let prev = if i > 0 {
            Some(i - 1)
        } else if self.closed && n > 2 {
            Some(n - 1)
        } else {
            None
        };
        let next = if i + 1 < n {
            Some(i + 1)
        } else if self.closed && n > 2 {
            Some(0)
        } else {
            None
        };
        (prev, next)
    }

    /// Effective (in, out) tangents of knot `i` in spline space, following its mode.
    pub fn tangents(&self, i: usize) -> (Vec3, Vec3) {
        let k = &self.knots[i];
        let (prev, next) = self.neighbours(i);
        let p = k.position;
        let prev_p = prev.map(|j| self.knots[j].position);
        let next_p = next.map(|j| self.knots[j].position);
        match k.mode {
            TangentMode::Linear => (
                prev_p.map_or(Vec3::ZERO, |q| (q - p) / 3.0),
                next_p.map_or(Vec3::ZERO, |q| (q - p) / 3.0),
            ),
            TangentMode::AutoSmooth => {
                let dir = match (prev_p, next_p) {
                    (Some(a), Some(b)) => {
                        // Average of the two unit directions: the smaller gap doesn't dominate.
                        let d = (b - p).normalize_or_zero() + (p - a).normalize_or_zero();
                        if d.length_squared() > 1e-10 {
                            d.normalize()
                        } else {
                            (b - p).normalize_or_zero()
                        }
                    }
                    (None, Some(b)) => (b - p).normalize_or_zero(),
                    (Some(a), None) => (p - a).normalize_or_zero(),
                    (None, None) => Vec3::ZERO,
                };
                let t = k.tension.max(0.0);
                (
                    -dir * prev_p.map_or(0.0, |a| a.distance(p)) * t,
                    dir * next_p.map_or(0.0, |b| b.distance(p)) * t,
                )
            }
            TangentMode::Mirrored => {
                let out = k.rotation * k.tangent_out;
                (-out, out)
            }
            TangentMode::Continuous => {
                let out = k.rotation * k.tangent_out;
                let len = k.tangent_in.length();
                (-out.normalize_or_zero() * len, out)
            }
            TangentMode::Broken => (k.rotation * k.tangent_in, k.rotation * k.tangent_out),
        }
    }

    /// Bézier segment `i` (from knot `i` to the next one).
    pub fn curve(&self, i: usize) -> BezierCurve {
        let n = self.knots.len();
        let j = (i + 1) % n;
        let (_, out_a) = self.tangents(i);
        let (in_b, _) = self.tangents(j);
        let a = self.knots[i].position;
        let b = self.knots[j].position;
        BezierCurve {
            p0: a,
            p1: a + out_a,
            p2: b + in_b,
            p3: b,
        }
    }

    /// Curves and arc-length table for fast evaluation.
    pub fn path(&self) -> SplinePath {
        SplinePath::new(self)
    }

    /// Insert a knot on segment `curve` at local parameter `u`, keeping the curve's shape for
    /// Bézier neighbours (de Casteljau split). Returns the new knot's index.
    pub fn insert_on_curve(&mut self, curve: usize, u: f32) -> usize {
        let c = self.curve(curve);
        let u = u.clamp(0.0, 1.0);
        let lerp = |a: Vec3, b: Vec3| a + (b - a) * u;
        let (q0, q1, q2) = (lerp(c.p0, c.p1), lerp(c.p1, c.p2), lerp(c.p2, c.p3));
        let (r0, r1) = (lerp(q0, q1), lerp(q1, q2));
        let p = lerp(r0, r1);
        let n = self.knots.len();
        let a = curve;
        let b = (curve + 1) % n;
        let new = Knot {
            mode: self.knots[a].mode,
            tension: self.knots[a].tension,
            ..Knot::at(p)
        };
        let mut knot = new;
        if self.knots[a].mode.is_bezier() || self.knots[b].mode.is_bezier() {
            // Split the Bézier exactly: neighbours' tangents shrink, the new knot gets r0/r1.
            let a_out = q0 - c.p0;
            let b_in = q2 - c.p3;
            set_world_out(&mut self.knots[a], a_out);
            set_world_in(&mut self.knots[b], b_in);
            let out = r1 - p;
            let rotation = look_rotation(out, self.knots[a].up());
            knot = Knot {
                position: p,
                rotation,
                tangent_in: rotation.inverse() * (r0 - p),
                tangent_out: rotation.inverse() * out,
                mode: TangentMode::Broken,
                tension: DEFAULT_TENSION,
            };
            if (r0 - p).normalize_or_zero().dot(out.normalize_or_zero()) < -0.999 {
                knot.mode = TangentMode::Continuous;
            }
        }
        self.knots.insert(curve + 1, knot);
        curve + 1
    }
}

/// Set a knot's out tangent from a spline-space vector (respecting its mode where possible).
fn set_world_out(k: &mut Knot, v: Vec3) {
    if !k.mode.is_bezier() {
        return;
    }
    k.tangent_out = k.rotation.inverse() * v;
    if k.mode == TangentMode::Mirrored {
        // Shrinking one side of a mirrored knot breaks the symmetry: keep direction only.
        k.mode = TangentMode::Continuous;
    }
    k.enforce(false);
}

fn set_world_in(k: &mut Knot, v: Vec3) {
    if !k.mode.is_bezier() {
        return;
    }
    k.tangent_in = k.rotation.inverse() * v;
    if k.mode == TangentMode::Mirrored {
        k.mode = TangentMode::Continuous;
    }
    k.enforce(true);
}

/// Arc-length samples per curve.
const SAMPLES: usize = 48;

/// A spline ready for queries: its curves, knot up vectors and cumulative arc lengths.
#[derive(Clone, Debug, Default)]
pub struct SplinePath {
    pub curves: Vec<BezierCurve>,
    /// Up vector at the start and end of each curve.
    ups: Vec<(Vec3, Vec3)>,
    /// Cumulative length at each sample of each curve (`SAMPLES + 1` per curve).
    table: Vec<[f32; SAMPLES + 1]>,
    /// Length before each curve.
    starts: Vec<f32>,
    pub length: f32,
    pub closed: bool,
    /// Lone knot of a one-knot spline.
    single: Option<Vec3>,
}

/// Where a query landed: curve index and the curve's own parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    pub curve: usize,
    pub u: f32,
}

/// Result of [`SplinePath::nearest`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nearest {
    pub t: f32,
    pub point: Vec3,
    pub distance: f32,
}

impl SplinePath {
    pub fn new(s: &Spline) -> Self {
        let count = s.curve_count();
        let mut path = SplinePath {
            closed: s.closed,
            single: (s.knots.len() == 1).then(|| s.knots[0].position),
            ..Default::default()
        };
        let mut total = 0.0;
        for i in 0..count {
            let c = s.curve(i);
            let j = (i + 1) % s.knots.len();
            path.ups.push((s.knots[i].up(), s.knots[j].up()));
            let mut row = [0.0; SAMPLES + 1];
            let mut prev = c.p0;
            let mut acc = 0.0;
            for (k, slot) in row.iter_mut().enumerate().skip(1) {
                let p = c.position(k as f32 / SAMPLES as f32);
                acc += p.distance(prev);
                prev = p;
                *slot = acc;
            }
            path.starts.push(total);
            total += acc;
            path.curves.push(c);
            path.table.push(row);
        }
        path.length = total;
        path
    }

    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
    }

    /// Curve and curve parameter at distance `d` along the spline (clamped; wraps if closed).
    pub fn at_distance(&self, d: f32) -> CurvePoint {
        if self.curves.is_empty() {
            return CurvePoint { curve: 0, u: 0.0 };
        }
        let d = if self.closed && self.length > 0.0 {
            let w = d.rem_euclid(self.length);
            // Keep the very end at the end instead of wrapping to the start.
            if d > 0.0 && w == 0.0 { self.length } else { w }
        } else {
            d.clamp(0.0, self.length)
        };
        // Last curve that starts at or before d.
        let i = match self.starts.partition_point(|s| *s <= d) {
            0 => 0,
            n => n - 1,
        };
        let local = d - self.starts[i];
        let row = &self.table[i];
        let k = row.partition_point(|v| *v < local).clamp(1, SAMPLES);
        let (a, b) = (row[k - 1], row[k]);
        let f = if b > a { (local - a) / (b - a) } else { 0.0 };
        CurvePoint {
            curve: i,
            u: ((k - 1) as f32 + f.clamp(0.0, 1.0)) / SAMPLES as f32,
        }
    }

    /// Curve point at normalised `t` (by distance).
    pub fn at(&self, t: f32) -> CurvePoint {
        self.at_distance(self.distance_of(t))
    }

    fn distance_of(&self, t: f32) -> f32 {
        if self.closed {
            t * self.length
        } else {
            t.clamp(0.0, 1.0) * self.length
        }
    }

    /// Distance along the spline of a curve point.
    pub fn distance_at(&self, p: CurvePoint) -> f32 {
        let Some(row) = self.table.get(p.curve) else {
            return 0.0;
        };
        let x = p.u.clamp(0.0, 1.0) * SAMPLES as f32;
        let k = (x.floor() as usize).min(SAMPLES - 1);
        let f = x - k as f32;
        self.starts[p.curve] + row[k] + (row[k + 1] - row[k]) * f
    }

    /// Normalised t of a curve point.
    pub fn t_of(&self, p: CurvePoint) -> f32 {
        if self.length <= 0.0 {
            return 0.0;
        }
        self.distance_at(p) / self.length
    }

    /// t at distance `d` from the start.
    pub fn t_at_distance(&self, d: f32) -> f32 {
        self.t_of(self.at_distance(d))
    }

    pub fn position(&self, t: f32) -> Vec3 {
        if let Some(p) = self.single {
            return p;
        }
        let c = self.at(t);
        self.curves
            .get(c.curve)
            .map_or(Vec3::ZERO, |b| b.position(c.u))
    }

    /// Unit direction of travel at t.
    pub fn tangent(&self, t: f32) -> Vec3 {
        let c = self.at(t);
        self.curves
            .get(c.curve)
            .map_or(Vec3::Z, |b| b.direction(c.u))
    }

    /// Velocity (unnormalised derivative of the curve) at t.
    pub fn derivative(&self, t: f32) -> Vec3 {
        let c = self.at(t);
        self.curves
            .get(c.curve)
            .map_or(Vec3::ZERO, |b| b.derivative(c.u))
    }

    /// Up vector at t: the knots' up vectors blended along the curve, made perpendicular to the
    /// tangent.
    pub fn up(&self, t: f32) -> Vec3 {
        let c = self.at(t);
        let Some((a, b)) = self.ups.get(c.curve).copied() else {
            return Vec3::Y;
        };
        let blended = a.lerp(b, smooth(c.u));
        let blended = if blended.length_squared() < 1e-8 {
            a
        } else {
            blended
        };
        let tan = self.tangent(t);
        let u = (blended - tan * blended.dot(tan)).normalize_or_zero();
        if u == Vec3::ZERO {
            tan.any_orthonormal_vector()
        } else {
            u
        }
    }

    /// Curvature (1 / radius) at t.
    pub fn curvature(&self, t: f32) -> f32 {
        let c = self.at(t);
        let Some(b) = self.curves.get(c.curve) else {
            return 0.0;
        };
        let d1 = b.derivative(c.u);
        let d2 = b.second_derivative(c.u);
        let len = d1.length();
        if len < 1e-6 {
            return 0.0;
        }
        d1.cross(d2).length() / (len * len * len)
    }

    /// Rotation at t: +Z along the tangent, +Y towards the up vector.
    pub fn rotation(&self, t: f32) -> Quat {
        look_rotation(self.tangent(t), self.up(t))
    }

    /// Closest point on the spline to `p`.
    pub fn nearest(&self, p: Vec3) -> Nearest {
        if let Some(q) = self.single {
            return Nearest {
                t: 0.0,
                point: q,
                distance: q.distance(p),
            };
        }
        let mut best = (0usize, 0.0f32, f32::INFINITY);
        // Coarse pass over the arc-length samples, then refine on the best curve.
        for (i, c) in self.curves.iter().enumerate() {
            for k in 0..=SAMPLES {
                let u = k as f32 / SAMPLES as f32;
                let d = c.position(u).distance_squared(p);
                if d < best.2 {
                    best = (i, u, d);
                }
            }
        }
        let (i, mut u, _) = best;
        let c = self.curves.get(i).copied();
        if let Some(c) = c {
            // Narrowing search around the best sample (robust where Newton overshoots).
            let mut step = 1.0 / SAMPLES as f32;
            for _ in 0..24 {
                let candidates = [u - step, u, u + step];
                u = candidates
                    .into_iter()
                    .map(|x| x.clamp(0.0, 1.0))
                    .min_by(|a, b| {
                        c.position(*a)
                            .distance_squared(p)
                            .total_cmp(&c.position(*b).distance_squared(p))
                    })
                    .unwrap_or(u);
                step *= 0.5;
            }
        }
        let point = c.map_or(Vec3::ZERO, |c| c.position(u));
        Nearest {
            t: self.t_of(CurvePoint { curve: i, u }),
            point,
            distance: point.distance(p),
        }
    }

    /// Normalised t of each knot (the start of each curve, plus the end for open splines).
    pub fn knot_ts(&self) -> Vec<f32> {
        if self.length <= 0.0 {
            return vec![0.0; self.curves.len() + usize::from(!self.closed)];
        }
        let mut ts: Vec<f32> = self.starts.iter().map(|s| s / self.length).collect();
        if !self.closed {
            ts.push(1.0);
        }
        ts
    }

    /// `per_curve` points along each curve plus the end, for drawing.
    pub fn polyline(&self, per_curve: usize) -> Vec<Vec3> {
        let mut pts = Vec::new();
        for c in &self.curves {
            for k in 0..per_curve {
                pts.push(c.position(k as f32 / per_curve as f32));
            }
        }
        if let Some(c) = self.curves.last() {
            pts.push(c.p3);
        }
        pts
    }
}

fn smooth(u: f32) -> f32 {
    u * u * (3.0 - 2.0 * u)
}

/// Small deterministic hash (FNV-1a) for generated content keys, stable across builds.
#[derive(Clone, Copy)]
pub(crate) struct Fnv(pub u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    pub fn bytes(&mut self, b: &[u8]) {
        for x in b {
            self.0 ^= *x as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    pub fn f32(&mut self, v: f32) {
        self.bytes(&v.to_bits().to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    pub fn spline(&mut self, s: &Spline) {
        self.u64(s.knots.len() as u64);
        self.u64(s.closed as u64);
        for k in &s.knots {
            self.vec3(k.position);
            for c in k.rotation.to_array() {
                self.f32(c);
            }
            self.vec3(k.tangent_in);
            self.vec3(k.tangent_out);
            self.u64(k.mode as u64);
            self.f32(k.tension);
        }
    }
    pub fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.bytes(s.as_bytes());
    }
}

/// Small deterministic random numbers (SplitMix64) for placement jitter.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
    pub fn vec3(&mut self, lo: Vec3, hi: Vec3) -> Vec3 {
        Vec3::new(
            self.range(lo.x, hi.x),
            self.range(lo.y, hi.y),
            self.range(lo.z, hi.z),
        )
    }
}

/// Run the edit-time spline systems on a scene: rebuild Spline Extrude meshes and Spline
/// Instantiate copies whose inputs changed, and keep Spline Animate objects that haven't
/// started at their start point. Cheap when nothing changed.
pub fn update(scene: &mut crate::scene::Scene) {
    extrude::update(scene);
    instantiate::update(scene);
    animate::place_unstarted(scene);
}

#[cfg(test)]
mod tests;
