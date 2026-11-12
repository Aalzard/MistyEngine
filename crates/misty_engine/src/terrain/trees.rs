//! Terrain trees (the Paint Trees tab): a list of tree prototypes (`.tree` assets) and the
//! placed instances, stored in the terrain data asset like the reference engine's tree
//! instances. An instance keeps its position across the terrain (0..1, so it follows resizes),
//! its prototype, rotation, scale and a colour variation; its height always comes from the
//! heightmap, so trees stay on the ground when the terrain is sculpted.
//!
//! Instances are stored as base64 (20 bytes each) to keep big forests small on disk.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::TerrainData;
use super::noise;

/// Most prototypes a terrain holds (the index is stored in 16 bits; the UI lists them).
pub const MAX_PROTOTYPES: usize = 64;
/// Bytes per stored instance.
const INSTANCE_BYTES: usize = 20;
/// Highest a tree can stand above the terrain (bounds for culling and shadow fitting).
pub const MAX_TREE_HEIGHT: f32 = 45.0;

/// A tree the terrain can place.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreePrototype {
    /// Project-relative `.tree` asset.
    pub tree: String,
}

/// One placed tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeInstance {
    /// Position across the terrain: 0..1 along x and z.
    pub position: Vec2,
    pub prototype: u16,
    /// Turn about the vertical axis (radians).
    pub rotation: f32,
    pub height_scale: f32,
    pub width_scale: f32,
    /// Colour variation 0..1 (each tree a little warmer or cooler, lighter or darker).
    pub color: f32,
}

impl Default for TreeInstance {
    fn default() -> Self {
        Self {
            position: Vec2::splat(0.5),
            prototype: 0,
            rotation: 0.0,
            height_scale: 1.0,
            width_scale: 1.0,
            color: 0.5,
        }
    }
}

/// Trees of a terrain and how they are drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeSettings {
    pub prototypes: Vec<TreePrototype>,
    #[serde(serialize_with = "save_instances", deserialize_with = "load_instances")]
    pub instances: Vec<TreeInstance>,
    /// Trees further than this from the camera are not drawn (metres).
    pub draw_distance: f32,
    /// Full-detail meshes up to here, the lighter mesh beyond (metres, × the quality level's
    /// LOD bias).
    pub lod_distance: f32,
    /// Meshes up to here, billboards (impostors) beyond (metres, × the LOD bias).
    pub billboard_start: f32,
    /// Cross-fade band at each switch (lighter mesh, impostor, draw distance) as a share of its
    /// distance, just before it (0 = switch at once). Terrains saved before it existed get the
    /// default; it is written only when changed.
    #[serde(
        default = "default_lod_fade",
        skip_serializing_if = "is_default_lod_fade"
    )]
    pub lod_fade: f32,
    pub cast_shadows: bool,
    /// 0.1..1: detail of the trees in the sun's shadow cascades (1 = each cascade's level of
    /// detail by its texel size; lower = lighter meshes and impostors in finer cascades: a
    /// dense forest's shadows cost much less).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub shadow_detail: f32,
}

fn one() -> f32 {
    1.0
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

/// Tree Rendering ▸ LOD Fade of new (and older) terrains: a tenth of each switch distance.
pub const DEFAULT_LOD_FADE: f32 = 0.1;

fn default_lod_fade() -> f32 {
    DEFAULT_LOD_FADE
}

fn is_default_lod_fade(v: &f32) -> bool {
    *v == DEFAULT_LOD_FADE
}

impl Default for TreeSettings {
    fn default() -> Self {
        Self {
            prototypes: Vec::new(),
            instances: Vec::new(),
            draw_distance: 2000.0,
            lod_distance: 30.0,
            billboard_start: 110.0,
            lod_fade: DEFAULT_LOD_FADE,
            cast_shadows: true,
            shadow_detail: 1.0,
        }
    }
}

fn save_instances<S: Serializer>(v: &[TreeInstance], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&super::file::encode(&encode_instances(v)))
}

fn load_instances<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<TreeInstance>, D::Error> {
    let text = String::deserialize(d)?;
    let bytes = super::file::decode(&text).map_err(serde::de::Error::custom)?;
    Ok(decode_instances(&bytes))
}

/// 16-bit fixed point of `v` over 0..`max`.
fn fixed(v: f32, max: f32) -> u16 {
    (v / max * 65535.0).round().clamp(0.0, 65535.0) as u16
}

fn encode_instances(v: &[TreeInstance]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * INSTANCE_BYTES);
    for t in v {
        out.extend(t.position.x.to_le_bytes());
        out.extend(t.position.y.to_le_bytes());
        out.extend(t.prototype.to_le_bytes());
        let turn = t.rotation.rem_euclid(std::f32::consts::TAU);
        out.extend(fixed(turn, std::f32::consts::TAU).to_le_bytes());
        out.extend(fixed(t.height_scale, 8.0).to_le_bytes());
        out.extend(fixed(t.width_scale, 8.0).to_le_bytes());
        out.push((t.color.clamp(0.0, 1.0) * 255.0).round() as u8);
        out.extend([0u8; 3]);
    }
    out
}

fn decode_instances(bytes: &[u8]) -> Vec<TreeInstance> {
    bytes
        .as_chunks::<INSTANCE_BYTES>()
        .0
        .iter()
        .map(|b| {
            let f = |i: usize| f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
            let u = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as f32 / 65535.0;
            TreeInstance {
                position: Vec2::new(f(0), f(4)),
                prototype: u16::from_le_bytes([b[8], b[9]]),
                rotation: u(10) * std::f32::consts::TAU,
                height_scale: u(12) * 8.0,
                width_scale: u(14) * 8.0,
                color: b[16] as f32 / 255.0,
            }
        })
        .collect()
}

impl TreeSettings {
    /// Nothing placed and every setting at its default.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Drop instances of missing prototypes and fix values after loading.
    pub fn sanitize(&mut self) {
        self.prototypes.truncate(MAX_PROTOTYPES);
        let n = self.prototypes.len() as u16;
        self.instances
            .retain(|t| t.prototype < n && t.position.is_finite());
        for t in &mut self.instances {
            t.position = t.position.clamp(Vec2::ZERO, Vec2::ONE);
        }
        let fix =
            |v: &mut f32, lo: f32, hi: f32| *v = if v.is_finite() { v.clamp(lo, hi) } else { lo };
        fix(&mut self.draw_distance, 10.0, 20_000.0);
        fix(&mut self.lod_distance, 1.0, 1000.0);
        fix(&mut self.billboard_start, 2.0, 5000.0);
        self.lod_fade = if self.lod_fade.is_finite() {
            self.lod_fade.clamp(0.0, 0.5)
        } else {
            DEFAULT_LOD_FADE
        };
        self.shadow_detail = if self.shadow_detail.is_finite() {
            self.shadow_detail.clamp(0.1, 1.0)
        } else {
            1.0
        };
    }

    /// Remove prototype `index` and the trees placed with it (later indices move down).
    pub fn remove_prototype(&mut self, index: usize) {
        if index >= self.prototypes.len() {
            return;
        }
        self.prototypes.remove(index);
        self.instances.retain(|t| t.prototype as usize != index);
        for t in &mut self.instances {
            if t.prototype as usize > index {
                t.prototype -= 1;
            }
        }
    }
}

/// Where a placed tree stands in terrain-local metres (its foot on the heightmap).
pub fn local_position(d: &TerrainData, t: &TreeInstance) -> Vec3 {
    let x = t.position.x * d.size.x;
    let z = t.position.y * d.size.z;
    Vec3::new(x, d.sample_height(x, z), z)
}

/// A tree's trunk for physics: terrain-local foot, radius and height (metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trunk {
    pub foot: Vec3,
    pub radius: f32,
    pub height: f32,
}

/// The trunks of every placed tree (the recipes are read once per prototype; trees whose
/// asset can't be read get none).
pub fn trunks(d: &TerrainData) -> Vec<Trunk> {
    let recipes: Vec<Option<crate::trees::TreeAsset>> = d
        .trees
        .prototypes
        .iter()
        .map(|p| crate::trees::TreeAsset::load(&crate::assets::absolute(&p.tree)).ok())
        .collect();
    d.trees
        .instances
        .iter()
        .filter_map(|t| {
            let r = recipes.get(t.prototype as usize)?.as_ref()?;
            Some(Trunk {
                foot: local_position(d, t),
                radius: (r.trunk_radius * t.width_scale).max(0.05),
                height: (r.height * r.crown_base.max(0.15) * t.height_scale).max(1.0),
            })
        })
        .collect()
}

/// Slope in degrees at a local point.
pub fn slope_at(d: &TerrainData, x: f32, z: f32) -> f32 {
    let s = d.spacing().max_element().max(0.1);
    let dx = (d.sample_height(x + s, z) - d.sample_height(x - s, z)) / (2.0 * s);
    let dz = (d.sample_height(x, z + s) - d.sample_height(x, z - s)) / (2.0 * s);
    (dx * dx + dz * dz).sqrt().atan().to_degrees()
}

/// How new trees are placed (Mass Place, the paint brush).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Height and width scale ranges (each tree random within).
    pub height: [f32; 2],
    pub width: [f32; 2],
    /// Steepest ground trees grow on (degrees).
    pub max_slope: f32,
    /// Closest two trees stand (metres, trunk to trunk).
    pub spacing: f32,
    /// Random seed.
    pub seed: u32,
    /// Terrain heights trees grow at (metres above the terrain's base).
    pub altitude: [f32; 2],
    /// Gentlest ground trees grow on (degrees; above 0 keeps them off flats).
    pub min_slope: f32,
    /// Each tree turned at random about its trunk (else all face the same way).
    pub random_rotation: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            height: [0.85, 1.15],
            width: [0.9, 1.1],
            max_slope: 30.0,
            spacing: 6.0,
            seed: 1,
            altitude: [0.0, 10_000.0],
            min_slope: 0.0,
            random_rotation: true,
        }
    }
}

impl Placement {
    /// Slope and height at terrain-local `(x, z)` allow a tree (and it is not over a hole).
    pub fn ground_allows(&self, d: &TerrainData, x: f32, z: f32) -> bool {
        let slope = slope_at(d, x, z);
        let h = d.sample_height(x, z);
        slope <= self.max_slope
            && slope >= self.min_slope
            && (self.altitude[0]..=self.altitude[1]).contains(&h)
            && !d.hole_at(x, z)
    }
}

fn rand(i: u32, salt: i32, seed: u32) -> f32 {
    noise::hash(i as i32, salt, seed)
}

/// A new instance at terrain-local `(x, z)` with random rotation, scale and colour.
pub fn new_instance(
    d: &TerrainData,
    x: f32,
    z: f32,
    prototype: u16,
    p: &Placement,
    n: u32,
) -> TreeInstance {
    let r = |salt| rand(n, salt, p.seed);
    let h = p.height[0] + (p.height[1] - p.height[0]) * r(11);
    let w = p.width[0] + (p.width[1] - p.width[0]) * r(12);
    TreeInstance {
        position: Vec2::new(x / d.size.x, z / d.size.z),
        prototype,
        rotation: if p.random_rotation {
            r(13) * std::f32::consts::TAU
        } else {
            0.0
        },
        height_scale: h,
        // Wider trees are a little shorter and the other way round (a stand of individuals).
        width_scale: w * (h / ((p.height[0] + p.height[1]) * 0.5)).powf(0.3),
        color: r(14),
    }
}

/// Is there room for a tree at local `(x, z)`: ground not too steep, no tree within spacing.
pub fn room_at(d: &TerrainData, x: f32, z: f32, p: &Placement, placed: &[TreeInstance]) -> bool {
    if x < 0.0 || z < 0.0 || x > d.size.x || z > d.size.z || !p.ground_allows(d, x, z) {
        return false;
    }
    let spacing2 = p.spacing * p.spacing;
    placed.iter().all(|t| {
        let q = Vec2::new(t.position.x * d.size.x, t.position.y * d.size.z);
        q.distance_squared(Vec2::new(x, z)) >= spacing2
    })
}

/// Place up to `count` trees of the `prototypes` at random over the whole terrain (Mass Place
/// Trees). Returns how many were placed.
pub fn mass_place(d: &mut TerrainData, count: usize, prototypes: &[u16], p: &Placement) -> usize {
    if prototypes.is_empty() || count == 0 {
        return 0;
    }
    let mut placed = 0;
    let base = d.trees.instances.len() as u32;
    // Spatial grid of the trees so far, so big counts stay fast.
    let cell = p.spacing.max(1.0);
    let mut grid: std::collections::HashMap<(i32, i32), Vec<Vec2>> = Default::default();
    let key = |q: Vec2| ((q.x / cell).floor() as i32, (q.y / cell).floor() as i32);
    for t in &d.trees.instances {
        let q = Vec2::new(t.position.x * d.size.x, t.position.y * d.size.z);
        grid.entry(key(q)).or_default().push(q);
    }
    let attempts = count * 8;
    for i in 0..attempts as u32 {
        if placed >= count {
            break;
        }
        let n = base + i;
        let x = rand(n, 1, p.seed) * d.size.x;
        let z = rand(n, 2, p.seed) * d.size.z;
        if !p.ground_allows(d, x, z) {
            continue;
        }
        let q = Vec2::new(x, z);
        let (kx, kz) = key(q);
        let crowded = (-1..=1).any(|dx| {
            (-1..=1).any(|dz| {
                grid.get(&(kx + dx, kz + dz))
                    .is_some_and(|v| v.iter().any(|o| o.distance(q) < p.spacing))
            })
        });
        if crowded {
            continue;
        }
        let proto =
            prototypes[(rand(n, 3, p.seed) * prototypes.len() as f32) as usize % prototypes.len()];
        d.trees.instances.push(new_instance(d, x, z, proto, p, n));
        grid.entry((kx, kz)).or_default().push(q);
        placed += 1;
    }
    placed
}

/// Paint trees inside a circle (local centre, radius): about `density` trees per 100 m² of
/// free ground, one stroke step at a time. With `erase`, remove the trees of the prototype in
/// the circle instead (all prototypes when None).
pub fn paint(
    d: &mut TerrainData,
    center: Vec2,
    radius: f32,
    density: f32,
    prototype: Option<u16>,
    p: &Placement,
    erase: bool,
) {
    let inside = |t: &TreeInstance| {
        Vec2::new(t.position.x * d.size.x, t.position.y * d.size.z).distance(center) <= radius
    };
    if erase {
        let before = d.trees.instances.len();
        let keep: Vec<TreeInstance> = d
            .trees
            .instances
            .iter()
            .copied()
            .filter(|t| !inside(t) || prototype.is_some_and(|p| p != t.prototype))
            .collect();
        if keep.len() != before {
            d.trees.instances = keep;
        }
        return;
    }
    let Some(proto) = prototype else {
        return;
    };
    let area = std::f32::consts::PI * radius * radius;
    let want = (area / 100.0 * density).round().max(1.0) as usize;
    let have = d.trees.instances.iter().filter(|t| inside(t)).count();
    if have >= want {
        return;
    }
    let base = d.trees.instances.len() as u32 * 31 + (center.x * 7.0 + center.y * 13.0) as u32;
    for i in 0..((want - have) * 4) as u32 {
        let n = base.wrapping_add(i);
        let a = rand(n, 5, p.seed) * std::f32::consts::TAU;
        let r = rand(n, 6, p.seed).sqrt() * radius;
        let q = center + Vec2::new(a.cos(), a.sin()) * r;
        if room_at(d, q.x, q.y, p, &d.trees.instances) {
            let t = new_instance(d, q.x, q.y, proto, p, n);
            d.trees.instances.push(t);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_round_trip_through_the_file() {
        let mut d = TerrainData::default();
        d.trees.prototypes.push(TreePrototype {
            tree: "Assets/Trees/Oak.tree".into(),
        });
        d.trees.instances.push(TreeInstance {
            position: Vec2::new(0.25, 0.75),
            prototype: 0,
            rotation: 1.5,
            height_scale: 1.2,
            width_scale: 0.9,
            color: 0.4,
        });
        let back = TerrainData::from_file_str(&d.to_file_string()).unwrap();
        let (a, b) = (d.trees.instances[0], back.trees.instances[0]);
        assert_eq!(back.trees.prototypes, d.trees.prototypes);
        assert_eq!(a.position, b.position);
        assert!((a.rotation - b.rotation).abs() < 1e-3);
        assert!((a.height_scale - b.height_scale).abs() < 1e-3);
        assert!((a.width_scale - b.width_scale).abs() < 1e-3);
        assert!((a.color - b.color).abs() < 1e-2);
    }

    #[test]
    fn mass_place_respects_spacing_and_slope() {
        let mut d = TerrainData::new(65, Vec3::new(200.0, 50.0, 200.0), 64);
        d.trees.prototypes.push(TreePrototype::default());
        let p = Placement {
            spacing: 10.0,
            ..Default::default()
        };
        let n = mass_place(&mut d, 100, &[0], &p);
        assert!(n > 50, "{n}");
        let pts: Vec<Vec2> = d
            .trees
            .instances
            .iter()
            .map(|t| t.position * 200.0)
            .collect();
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                assert!(a.distance(*b) >= 10.0 - 1e-3);
            }
        }
        // A wall of a slope: nothing grows on it.
        let mut steep = TerrainData::new(65, Vec3::new(200.0, 500.0, 200.0), 64);
        for iz in 0..65 {
            for ix in 0..65 {
                steep.set_height(ix, iz, ix as f32 / 64.0);
            }
        }
        steep.trees.prototypes.push(TreePrototype::default());
        assert_eq!(mass_place(&mut steep, 50, &[0], &p), 0);
    }

    /// A ramp from 0 to 50 m along x: height limits keep trees in their band, the brush
    /// plants within it and erases only the picked tree.
    #[test]
    fn height_limits_rotation_and_erase() {
        let mut d = TerrainData::new(65, Vec3::new(200.0, 50.0, 200.0), 64);
        for iz in 0..65 {
            for ix in 0..65 {
                d.set_height(ix, iz, ix as f32 / 64.0);
            }
        }
        d.trees.prototypes = vec![TreePrototype::default(); 2];
        let p = Placement {
            spacing: 5.0,
            altitude: [10.0, 20.0],
            random_rotation: false,
            ..Default::default()
        };
        let placed = mass_place(&mut d, 200, &[0], &p);
        assert!(placed > 20, "{placed}");
        for t in &d.trees.instances {
            let h = local_position(&d, t).y;
            assert!((10.0 - 0.01..=20.01).contains(&h), "tree at {h} m");
            assert_eq!(t.rotation, 0.0, "no random turn");
        }
        // Paint the other tree in the band, then erase only the first one there.
        let centre = Vec2::new(60.0, 100.0);
        for _ in 0..80 {
            paint(&mut d, centre, 30.0, 4.0, Some(1), &p, false);
        }
        let inside = |d: &TerrainData, proto: u16| {
            (d.trees.instances.iter())
                .filter(|t| t.prototype == proto)
                .filter(|t| (t.position * 200.0).distance(centre) <= 30.0)
                .count()
        };
        assert!(inside(&d, 1) > 3, "painted");
        assert!(inside(&d, 0) > 0);
        paint(&mut d, centre, 30.0, 4.0, Some(0), &p, true);
        assert_eq!(inside(&d, 0), 0, "erased");
        assert!(inside(&d, 1) > 3, "the other tree stays");
    }

    #[test]
    fn removing_a_prototype_drops_its_trees() {
        let mut s = TreeSettings {
            prototypes: vec![TreePrototype::default(); 3],
            ..Default::default()
        };
        for proto in [0, 1, 2, 1] {
            s.instances.push(TreeInstance {
                prototype: proto,
                ..Default::default()
            });
        }
        s.remove_prototype(1);
        assert_eq!(s.prototypes.len(), 2);
        let protos: Vec<u16> = s.instances.iter().map(|t| t.prototype).collect();
        assert_eq!(protos, vec![0, 1]);
    }

    #[test]
    fn lod_fade_loads_from_old_files_and_round_trips() {
        // Saved before LOD Fade existed: the default band.
        let old: TreeSettings =
            serde_json::from_str(r#"{"draw_distance":500.0,"lod_distance":40.0}"#).unwrap();
        assert_eq!(old.lod_fade, DEFAULT_LOD_FADE);
        // The default is not written, so unchanged terrains save as before.
        let text = serde_json::to_string(&TreeSettings::default()).unwrap();
        assert!(!text.contains("lod_fade"), "{text}");
        for v in [0.0, 0.25] {
            let s = TreeSettings {
                lod_fade: v,
                ..Default::default()
            };
            let text = serde_json::to_string(&s).unwrap();
            let back: TreeSettings = serde_json::from_str(&text).unwrap();
            assert_eq!(back.lod_fade, v, "{text}");
        }
        let mut bad = TreeSettings {
            lod_fade: f32::NAN,
            ..Default::default()
        };
        bad.sanitize();
        assert_eq!(bad.lod_fade, DEFAULT_LOD_FADE);
        bad.lod_fade = 3.0;
        bad.sanitize();
        assert_eq!(bad.lod_fade, 0.5);
    }
}
