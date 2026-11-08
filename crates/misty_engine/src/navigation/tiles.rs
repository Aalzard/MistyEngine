//! Tiles: the bake input split by tile (triangles touching each tile plus its border) and the
//! whole voxel → polygon pipeline for one tile. A tile's input hash decides whether it needs
//! building again (partial rebakes, carving).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use glam::{Vec2, Vec3Swizzles};

use super::contours::{self, Simplify};
use super::geometry::{AreaVolume, BuildInput};
use super::heightfield::{self, CarveShape, Config, Grid, NONE};
use super::navmesh::{BuildSettings, Tile};
use super::polymesh::{self, Detail, Frame};
use super::regions;

/// Bake input indexed by tile, ready to build any tile (on any thread).
pub struct TileBuilder {
    pub settings: BuildSettings,
    cfg: Config,
    input: BuildInput,
    /// Solid object of each triangle (`NONE` = open surface).
    solid_of: Vec<u32>,
    tile_tris: HashMap<(i32, i32), Vec<u32>>,
    /// Tiles whose own area (not just the border) has geometry.
    cores: std::collections::HashSet<(i32, i32)>,
}

/// Voxel settings for an agent at a cell size.
fn config(s: &BuildSettings) -> Config {
    let a = s.agent;
    Config {
        cell: s.cell_size,
        climb: a.step_height.max(0.0),
        height: a.height.max(0.01),
        walkable_cos: a.max_slope.clamp(0.0, 89.9).to_radians().cos(),
        radius: a.radius.max(0.0),
        min_region_area: s.min_region_area.max(0.0),
    }
}

impl TileBuilder {
    pub fn new(input: BuildInput, settings: BuildSettings) -> Self {
        let cfg = config(&settings);
        let geo = &input.geometry;
        let mut solid_of = vec![NONE; geo.triangles.len()];
        for (k, r) in geo.solids.iter().enumerate() {
            solid_of[r.clone()].fill(k as u32);
        }
        let pad = cfg.border() as f32 * cfg.cell;
        let mut tile_tris: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let mut cores = std::collections::HashSet::new();
        if settings.tile_size > 0 {
            for (i, t) in geo.triangles.iter().enumerate() {
                let (tlo, thi) = (t[0].min(t[1]).min(t[2]).xz(), t[0].max(t[1]).max(t[2]).xz());
                let (clo, chi) = match input.clip {
                    Some((lo, hi)) => (tlo.max(lo.xz()), thi.min(hi.xz())),
                    None => (tlo, thi),
                };
                if clo.cmple(chi).all() {
                    let (x0, z0) = settings.tile_at(clo);
                    let (x1, z1) = settings.tile_at(chi);
                    for z in z0..=z1 {
                        for x in x0..=x1 {
                            cores.insert((x, z));
                        }
                    }
                }
                let lo = tlo - pad;
                let hi = thi + pad;
                if let Some((clo, chi)) = input.clip
                    && (hi.cmplt(clo.xz() - pad).any() || lo.cmpgt(chi.xz() + pad).any())
                {
                    continue;
                }
                let (x0, z0) = settings.tile_at(lo);
                let (x1, z1) = settings.tile_at(hi);
                for z in z0..=z1 {
                    for x in x0..=x1 {
                        tile_tris.entry((x, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        Self {
            settings,
            cfg,
            input,
            solid_of,
            tile_tris,
            cores,
        }
    }

    /// Every tile with geometry in it (sorted).
    pub fn tiles(&self) -> Vec<(i32, i32)> {
        let mut v: Vec<_> = self.cores.iter().copied().collect();
        v.sort_by_key(|&(x, z)| (z, x));
        v
    }

    /// Tiles whose core comes within the agent radius of x/z box lo..hi.
    pub fn tiles_touching(&self, lo: Vec2, hi: Vec2) -> Vec<(i32, i32)> {
        let pad = self.cfg.radius + self.cfg.cell * 2.0;
        let (x0, z0) = self.settings.tile_at(lo - pad);
        let (x1, z1) = self.settings.tile_at(hi + pad);
        let mut v = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                if self.cores.contains(&(x, z)) {
                    v.push((x, z));
                }
            }
        }
        v
    }

    /// x/z bounds of tile (x, z) with its border.
    fn padded_bounds(&self, x: i32, z: i32) -> (Vec2, Vec2) {
        let (lo, hi) = self.settings.tile_bounds(x, z);
        let pad = self.cfg.border() as f32 * self.cfg.cell + self.cfg.radius;
        (lo - pad, hi + pad)
    }

    fn volumes_in(&self, x: i32, z: i32) -> Vec<&AreaVolume> {
        let (lo, hi) = self.padded_bounds(x, z);
        self.input
            .volumes
            .iter()
            .filter(|v| !(v.hi.xz().cmplt(lo).any() || v.lo.xz().cmpgt(hi).any()))
            .collect()
    }

    fn carves_in<'a>(&self, x: i32, z: i32, carves: &'a [CarveShape]) -> Vec<&'a CarveShape> {
        let (lo, hi) = self.padded_bounds(x, z);
        carves
            .iter()
            .filter(|c| {
                let (clo, chi) = c.bounds(0.0);
                !(chi.cmplt(lo).any() || clo.cmpgt(hi).any())
            })
            .collect()
    }

    /// Hash of everything tile (x, z) is built from.
    pub fn hash(&self, x: i32, z: i32, carves: &[CarveShape]) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let f = |v: f32, h: &mut std::collections::hash_map::DefaultHasher| v.to_bits().hash(h);
        (x, z, self.settings.tile_size, self.settings.agent_type).hash(&mut h);
        for v in [
            self.cfg.cell,
            self.cfg.climb,
            self.cfg.height,
            self.cfg.walkable_cos,
            self.cfg.radius,
            self.cfg.min_region_area,
            self.settings.origin.x,
            self.settings.origin.y,
        ] {
            f(v, &mut h);
        }
        let geo = &self.input.geometry;
        for &i in self
            .tile_tris
            .get(&(x, z))
            .map_or(&[][..], |v| v.as_slice())
        {
            for p in geo.triangles[i as usize] {
                for c in p.to_array() {
                    f(c, &mut h);
                }
            }
            geo.area(i as usize).hash(&mut h);
            self.solid_of[i as usize].hash(&mut h);
        }
        for v in self.volumes_in(x, z) {
            for c in v
                .to_local
                .to_cols_array()
                .into_iter()
                .chain(v.half.to_array())
            {
                f(c, &mut h);
            }
            v.area.hash(&mut h);
        }
        for c in self.carves_in(x, z, carves) {
            for v in [
                c.center.x, c.center.y, c.axis.x, c.axis.y, c.half.x, c.half.y, c.radius, c.y_min,
                c.y_max,
            ] {
                f(v, &mut h);
            }
            c.capsule.hash(&mut h);
        }
        if let Some((lo, hi)) = self.input.clip {
            for c in lo.to_array().into_iter().chain(hi.to_array()) {
                f(c, &mut h);
            }
        }
        h.finish()
    }

    /// Build tile (x, z) with obstacles `carves` cut out.
    pub fn build(&self, x: i32, z: i32, carves: &[CarveShape]) -> Tile {
        let hash = self.hash(x, z, carves);
        let cfg = &self.cfg;
        let ts = self.settings.tile_size as i64;
        let border = cfg.border();
        let offset = (x as i64 * ts - border as i64, z as i64 * ts - border as i64);
        let origin = Vec2::new(
            self.settings.origin.x + offset.0 as f32 * cfg.cell,
            self.settings.origin.y + offset.1 as f32 * cfg.cell,
        );
        let grid = Grid {
            origin,
            width: self.settings.tile_size + 2 * border,
            depth: self.settings.tile_size + 2 * border,
            border,
        };
        let tris = self
            .tile_tris
            .get(&(x, z))
            .map_or(&[][..], |v| v.as_slice());
        let mut field = heightfield::rasterize(
            &self.input.geometry,
            tris,
            &self.solid_of,
            grid,
            cfg,
            self.input.clip,
        );
        heightfield::erode(&mut field, cfg.radius);
        let volumes: Vec<AreaVolume> = self.volumes_in(x, z).into_iter().cloned().collect();
        heightfield::mark_volumes(&mut field, &volumes);
        let carves: Vec<CarveShape> = self.carves_in(x, z, carves).into_iter().copied().collect();
        heightfield::carve(&mut field, &carves, cfg);
        heightfield::drop_small_regions(&mut field, cfg.min_region_area);
        let regions = regions::build(&field);
        let simplify = Simplify {
            max_error: 1.3,
            max_edge_len: (12.0 / cfg.cell).round() as i32,
        };
        let contours = contours::build(&field, &regions, simplify);
        let frame = Frame {
            origin: self.settings.origin,
            offset,
            cell: cfg.cell,
        };
        let detail = Detail {
            sample_cells: 3,
            max_error: (cfg.cell * 0.5).max(0.05),
        };
        let mut polys = polymesh::build(&contours, &field, frame, detail);
        polys.truncate(u16::MAX as usize);
        Tile::new(x, z, polys, hash)
    }
}

/// Steps of one tile build, for tests: (live cells, regions, contours).
#[cfg(test)]
pub(super) fn debug_tile(
    b: &TileBuilder,
    x: i32,
    z: i32,
) -> (usize, usize, Vec<contours::Contour>) {
    let cfg = &b.cfg;
    let ts = b.settings.tile_size as i64;
    let border = cfg.border();
    let offset = (x as i64 * ts - border as i64, z as i64 * ts - border as i64);
    let grid = Grid {
        origin: Vec2::new(
            b.settings.origin.x + offset.0 as f32 * cfg.cell,
            b.settings.origin.y + offset.1 as f32 * cfg.cell,
        ),
        width: b.settings.tile_size + 2 * border,
        depth: b.settings.tile_size + 2 * border,
        border,
    };
    let tris = b.tile_tris.get(&(x, z)).map_or(&[][..], |v| v.as_slice());
    let mut field = heightfield::rasterize(&b.input.geometry, tris, &b.solid_of, grid, cfg, None);
    heightfield::erode(&mut field, cfg.radius);
    let live = field
        .cells
        .iter()
        .filter(|c| c.area != heightfield::NULL_AREA && field.in_core(c))
        .count();
    let regions = regions::build(&field);
    let simplify = Simplify {
        max_error: 1.3,
        max_edge_len: 0,
    };
    let contours = contours::build(&field, &regions, simplify);
    (live, regions.count() - 1, contours)
}

/// A world point of tile corner units (tests).
#[cfg(test)]
pub(super) fn corner_world(b: &TileBuilder, x: i32, z: i32, v: &contours::Vert) -> glam::Vec3 {
    let ts = b.settings.tile_size as i64;
    let border = b.cfg.border() as i64;
    let frame = Frame {
        origin: b.settings.origin,
        offset: (x as i64 * ts - border, z as i64 * ts - border),
        cell: b.cfg.cell,
    };
    let p = frame.world(v.x, v.z);
    glam::Vec3::new(p.x, v.y, p.y)
}
