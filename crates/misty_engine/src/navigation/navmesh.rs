//! The baked polygon nav mesh: convex planar polygons with portal links, point queries and the
//! `.navmesh` file format (JSON, one polygon per line).

use std::path::Path;

use glam::{Vec2, Vec3, Vec3Swizzles};
use serde::{Deserialize, Serialize};

use super::AgentSettings;

const VERSION: u32 = 1;

/// A shared edge to a neighbouring polygon (`a`..`b`, on this polygon's surface).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavLink {
    pub to: u32,
    pub a: Vec3,
    pub b: Vec3,
}

/// One convex walkable polygon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavPoly {
    pub vertices: Vec<Vec3>,
    pub links: Vec<NavLink>,
}

/// Result of a nearest-point query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NearestHit {
    pub poly: u32,
    pub position: Vec3,
    pub distance: f32,
}

#[derive(Serialize, Deserialize)]
struct Header {
    version: u32,
    agent: AgentSettings,
    cell_size: f32,
}

#[derive(Deserialize)]
struct File {
    version: u32,
    agent: AgentSettings,
    cell_size: f32,
    polygons: Vec<NavPoly>,
}

/// Polygons bucketed by their x/z bounds for point queries.
#[derive(Debug, Default)]
struct Buckets {
    origin: Vec2,
    size: f32,
    dims: (usize, usize),
    cells: Vec<Vec<u32>>,
}

impl Buckets {
    fn new(bounds: &[(Vec3, Vec3)]) -> Self {
        let Some(lo) = bounds.iter().map(|b| b.0.xz()).reduce(Vec2::min) else {
            return Self::default();
        };
        let hi = bounds.iter().map(|b| b.1.xz()).fold(lo, Vec2::max);
        let ext = (hi - lo).max(Vec2::ONE);
        let size = (ext.max_element() / 128.0).max(2.0);
        let dims = ((ext.x / size) as usize + 1, (ext.y / size) as usize + 1);
        let mut b = Self {
            origin: lo,
            size,
            dims,
            cells: vec![Vec::new(); dims.0 * dims.1],
        };
        for (i, (plo, phi)) in bounds.iter().enumerate() {
            let (x0, z0) = b.index(plo.xz());
            let (x1, z1) = b.index(phi.xz());
            for z in z0..=z1 {
                for x in x0..=x1 {
                    b.cells[z * dims.0 + x].push(i as u32);
                }
            }
        }
        b
    }

    fn index(&self, p: Vec2) -> (usize, usize) {
        let q = ((p - self.origin) / self.size).floor();
        (
            (q.x.max(0.0) as usize).min(self.dims.0.saturating_sub(1)),
            (q.y.max(0.0) as usize).min(self.dims.1.saturating_sub(1)),
        )
    }

    /// Polygons whose buckets overlap the square `p ± r` (may repeat).
    fn around(&self, p: Vec2, r: f32) -> impl Iterator<Item = u32> + '_ {
        let (x0, z0) = self.index(p - r);
        let (x1, z1) = self.index(p + r);
        let empty = self.cells.is_empty();
        (z0..=z1)
            .flat_map(move |z| (x0..=x1).map(move |x| (x, z)))
            .filter(move |_| !empty)
            .flat_map(move |(x, z)| self.cells[z * self.dims.0 + x].iter().copied())
    }
}

/// A baked nav mesh with lookup data.
#[derive(Debug, Default)]
pub struct NavMesh {
    /// Agent the mesh was baked for.
    pub agent: AgentSettings,
    pub cell_size: f32,
    pub polys: Vec<NavPoly>,
    /// Plane per polygon: `n · p = d` with n.y > 0.
    planes: Vec<(Vec3, f32)>,
    bounds: Vec<(Vec3, Vec3)>,
    buckets: Buckets,
}

/// 2D cross product of (b - a) × (c - a) on x/z.
pub(super) fn cross2(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    (b - a).perp_dot(c - a)
}

/// Closest point to `p` on segment `a`..`b` (x/z), as a parameter 0..1.
fn segment_t(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    let d = b - a;
    let len2 = d.length_squared();
    if len2 < 1e-12 {
        0.0
    } else {
        ((p - a).dot(d) / len2).clamp(0.0, 1.0)
    }
}

impl NavMesh {
    pub fn new(agent: AgentSettings, cell_size: f32, polys: Vec<NavPoly>) -> Self {
        let planes = polys.iter().map(|p| plane(&p.vertices)).collect();
        let bounds: Vec<(Vec3, Vec3)> = polys
            .iter()
            .map(|p| {
                let first = p.vertices.first().copied().unwrap_or_default();
                p.vertices
                    .iter()
                    .fold((first, first), |(lo, hi), v| (lo.min(*v), hi.max(*v)))
            })
            .collect();
        let buckets = Buckets::new(&bounds);
        Self {
            agent,
            cell_size,
            polys,
            planes,
            bounds,
            buckets,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.polys.is_empty()
    }

    /// Surface height of polygon `poly` above x/z.
    pub fn height(&self, poly: u32, x: f32, z: f32) -> f32 {
        let (n, d) = self.planes[poly as usize];
        (d - n.x * x - n.z * z) / n.y
    }

    /// Whether x/z of `p` lies inside polygon `poly` (edges included).
    pub fn contains_xz(&self, poly: u32, p: Vec3) -> bool {
        let v = &self.polys[poly as usize].vertices;
        let q = p.xz();
        let (mut pos, mut neg) = (false, false);
        for i in 0..v.len() {
            let c = cross2(v[i].xz(), v[(i + 1) % v.len()].xz(), q);
            pos |= c > 1e-6;
            neg |= c < -1e-6;
        }
        !(pos && neg)
    }

    /// Closest point on polygon `poly`'s surface to `p`.
    pub fn closest_point(&self, poly: u32, p: Vec3) -> Vec3 {
        if self.contains_xz(poly, p) {
            return Vec3::new(p.x, self.height(poly, p.x, p.z), p.z);
        }
        let v = &self.polys[poly as usize].vertices;
        let q = p.xz();
        let mut best = (f32::MAX, v[0]);
        for i in 0..v.len() {
            let (a, b) = (v[i], v[(i + 1) % v.len()]);
            let t = segment_t(a.xz(), b.xz(), q);
            let c = a.lerp(b, t);
            let d = c.distance_squared(p);
            if d < best.0 {
                best = (d, c);
            }
        }
        best.1
    }

    /// Centre of polygon `poly` (vertex average).
    pub fn center(&self, poly: u32) -> Vec3 {
        let v = &self.polys[poly as usize].vertices;
        v.iter().copied().sum::<Vec3>() / v.len().max(1) as f32
    }

    /// Nearest point on the mesh within `max_distance` of `p` (`NavMesh.SamplePosition`).
    pub fn nearest(&self, p: Vec3, max_distance: f32) -> Option<NearestHit> {
        let r = max_distance.min(1e4);
        let mut best: Option<NearestHit> = None;
        for i in self.buckets.around(p.xz(), r) {
            let (lo, hi) = self.bounds[i as usize];
            let gap = (lo - p).max(p - hi).max(Vec3::ZERO);
            if gap.length() > r || best.is_some_and(|b| gap.length() >= b.distance) {
                continue;
            }
            let c = self.closest_point(i, p);
            let d = c.distance(p);
            if d <= r && best.is_none_or(|b| d < b.distance) {
                best = Some(NearestHit {
                    poly: i,
                    position: c,
                    distance: d,
                });
            }
        }
        best
    }

    /// Polygon under `p` (x/z inside, surface within `max_dy` vertically), closest in height.
    pub fn locate(&self, p: Vec3, max_dy: f32) -> Option<u32> {
        self.buckets
            .around(p.xz(), 0.0)
            .filter(|&i| self.contains_xz(i, p))
            .map(|i| (i, (self.height(i, p.x, p.z) - p.y).abs()))
            .filter(|(_, dy)| *dy <= max_dy)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Move from `from` (on polygon `poly`) towards x/z of `to`, staying on the mesh: the
    /// polygon under the target among the nearby connected ones, else the closest point on them
    /// (slides along edges). Returns the new polygon and surface position.
    pub fn move_along(&self, poly: u32, from: Vec3, to: Vec3) -> (u32, Vec3) {
        let reach = from.xz().distance(to.xz()) + self.cell_size * 2.0;
        // Polygons connected to `poly` whose bounds come within reach.
        let mut near = vec![poly];
        let mut i = 0;
        while i < near.len() && near.len() < 256 {
            for l in &self.polys[near[i] as usize].links {
                if near.contains(&l.to) {
                    continue;
                }
                let (lo, hi) = self.bounds[l.to as usize];
                let q = to.xz();
                let gap = (lo.xz() - q).max(q - hi.xz()).max(Vec2::ZERO).length();
                if gap <= reach {
                    near.push(l.to);
                }
            }
            i += 1;
        }
        let probe = Vec3::new(to.x, from.y, to.z);
        let inside = near
            .iter()
            .copied()
            .filter(|&p| self.contains_xz(p, probe))
            .min_by(|&a, &b| {
                let da = (self.height(a, to.x, to.z) - from.y).abs();
                let db = (self.height(b, to.x, to.z) - from.y).abs();
                da.total_cmp(&db)
            });
        if let Some(p) = inside {
            return (p, Vec3::new(to.x, self.height(p, to.x, to.z), to.z));
        }
        near.iter()
            .map(|&p| (p, self.closest_point(p, probe)))
            .min_by(|a, b| {
                let da = a.1.xz().distance_squared(to.xz());
                let db = b.1.xz().distance_squared(to.xz());
                da.total_cmp(&db)
            })
            .unwrap_or((poly, from))
    }

    /// Fan triangles of every polygon (for drawing).
    pub fn triangles(&self) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        self.polys.iter().flat_map(|p| {
            (1..p.vertices.len().saturating_sub(1))
                .map(move |i| [p.vertices[0], p.vertices[i], p.vertices[i + 1]])
        })
    }

    /// Total walkable area in m² (x/z).
    pub fn area(&self) -> f32 {
        self.triangles()
            .map(|t| cross2(t[0].xz(), t[1].xz(), t[2].xz()).abs() * 0.5)
            .sum()
    }

    // ---- file ---------------------------------------------------------------------------

    /// `.navmesh` text: a header line, then one polygon per line.
    pub fn to_json(&self) -> String {
        let header = serde_json::to_string(&Header {
            version: VERSION,
            agent: self.agent,
            cell_size: self.cell_size,
        })
        .expect("header serialises");
        let polys: Vec<String> = self
            .polys
            .iter()
            .map(|p| serde_json::to_string(p).expect("polygon serialises"))
            .collect();
        // `{"version":…,"cell_size":…}` + `,"polygons":[…]}`
        let head = header.trim_end_matches('}');
        format!("{head},\"polygons\":[\n{}\n]}}\n", polys.join(",\n"))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let f: File = serde_json::from_str(text).map_err(|e| format!("invalid nav mesh: {e}"))?;
        if f.version > VERSION {
            return Err(format!(
                "nav mesh was saved by a newer version (format {})",
                f.version
            ));
        }
        let n = f.polygons.len() as u32;
        let bad = f.polygons.iter().any(|p| {
            p.vertices.len() < 3
                || p.links.iter().any(|l| l.to >= n)
                || plane(&p.vertices).0.y <= 1e-4
        });
        if bad {
            return Err("invalid nav mesh: broken polygon data".into());
        }
        Ok(Self::new(f.agent, f.cell_size, f.polygons))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_json()).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text)
    }
}

/// Plane through a polygon (Newell's normal, facing up).
fn plane(v: &[Vec3]) -> (Vec3, f32) {
    let mut n = Vec3::ZERO;
    for i in 0..v.len() {
        let (a, b) = (v[i], v[(i + 1) % v.len()]);
        n += Vec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    let mut n = n.normalize_or_zero();
    if n.y < 0.0 {
        n = -n;
    }
    let c = v.iter().copied().sum::<Vec3>() / v.len().max(1) as f32;
    (n, n.dot(c))
}
