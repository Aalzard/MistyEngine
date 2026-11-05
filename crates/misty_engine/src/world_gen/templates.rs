//! World Builder templates: one-click starting points (Desert, Forest, Mountains, …), each a
//! complete setup of base settings, erosion, stamps, biome zones and the default biome, built
//! from the biome presets. Stamp positions are fractions of the terrain's width / length and
//! sizes scale with it, so a template fits whatever terrain size the builder has.

use glam::{Mat4, Quat, Vec2, Vec3};

use super::biome::Preset;
use super::generate::{PREVIEW_RESOLUTION, Request, Stages};
use super::stamps::{
    BiomeZone, BlendMode, Flatten, Mountain, MountainShape, River, RockArea, Water, WaterMode,
};
use super::{
    BaseSettings, ErosionSettings, Generator, PlacedStamp, Placement, StampKind, WorldBuilder,
    WorldStamp,
};

/// A one-click World Builder preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Template {
    Desert,
    Forest,
    Mountains,
    Plains,
    Islands,
    Canyon,
    Tundra,
    Volcanic,
}

/// A stamp a template places (terrain-local metres; yaw in degrees).
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateStamp {
    pub name: &'static str,
    pub position: Vec3,
    pub yaw: f32,
    pub stamp: WorldStamp,
}

/// Everything a template sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub settings: WorldBuilder,
    pub stamps: Vec<TemplateStamp>,
}

/// Reference terrain width the template sizes are written for (metres).
const REFERENCE: f32 = 600.0;

impl Template {
    pub const ALL: [Self; 8] = [
        Self::Desert,
        Self::Forest,
        Self::Mountains,
        Self::Plains,
        Self::Islands,
        Self::Canyon,
        Self::Tundra,
        Self::Volcanic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Desert => "Desert",
            Self::Forest => "Forest",
            Self::Mountains => "Mountains",
            Self::Plains => "Plains",
            Self::Islands => "Islands",
            Self::Canyon => "Canyon",
            Self::Tundra => "Tundra",
            Self::Volcanic => "Volcanic",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Desert => "Wind-shaped dunes with red-rock mesas and badlands.",
            Self::Forest => "Rolling wooded hills, a winding river and a meadow clearing.",
            Self::Mountains => "High eroded peaks with snow, cliffs and a river valley.",
            Self::Plains => "Gentle grassland, woods, a lazy river and a flat village site.",
            Self::Islands => "An island in the sea with beaches, hills and a forested heart.",
            Self::Canyon => "Mesas cut by deep channels and a river gorge.",
            Self::Tundra => "Snowfields, a frozen lake and a mountain ridge.",
            Self::Volcanic => "A volcano rising from the sea over ash plains and basalt.",
        }
    }

    /// Default biome of the template.
    pub fn biome(self) -> Preset {
        match self {
            Self::Desert => Preset::Desert,
            Self::Forest => Preset::Forest,
            Self::Mountains => Preset::Alpine,
            Self::Plains => Preset::Grassland,
            Self::Islands => Preset::Coast,
            Self::Canyon => Preset::RockyBadlands,
            Self::Tundra => Preset::Tundra,
            Self::Volcanic => Preset::Volcanic,
        }
    }

    /// The template applied to `current`: its terrain asset, width / length, resolutions,
    /// scatter storage and Live in Scene view setting are kept; everything else is replaced.
    pub fn setup(self, current: &WorldBuilder) -> Setup {
        let (w, l) = (current.size[0], current.size[2]);
        let k = w.min(l) / REFERENCE;
        let (height, base, erosion, breakup) = self.landform();
        let settings = WorldBuilder {
            terrain_data: current.terrain_data.clone(),
            size: [w, height, l],
            heightmap_resolution: current.heightmap_resolution,
            alphamap_resolution: current.alphamap_resolution,
            base: BaseSettings {
                // Feature sizes follow the terrain size.
                scale: base.scale * k.max(0.05),
                seed: base.seed,
                ..base
            },
            erosion,
            default_biome: self.biome().path(),
            biome_breakup: breakup,
            live_preview: current.live_preview,
            scatter_storage: current.scatter_storage,
        };
        let at = |u: f32, v: f32, y: f32| Vec3::new(u * w, y * height, v * l);
        let stamps = self
            .stamps(k, height)
            .into_iter()
            .map(|(name, (u, v, y), yaw, stamp)| TemplateStamp {
                name,
                position: at(u, v, y),
                yaw,
                stamp,
            })
            .collect();
        Setup { settings, stamps }
    }

    /// Maximum height (m), base settings, erosion and biome breakup.
    fn landform(self) -> (f32, BaseSettings, ErosionSettings, f32) {
        let base =
            |generator, height, scale, noise_amount, octaves, ridge_sharpness, seed| BaseSettings {
                generator,
                height,
                scale,
                noise_amount,
                octaves,
                ridge_sharpness,
                seed,
            };
        let erosion = |hydraulic_iterations: u32, hs: f32, thermal_iterations: u32, talus, ts| {
            ErosionSettings {
                hydraulic: hydraulic_iterations > 0,
                hydraulic_iterations: hydraulic_iterations.max(1),
                hydraulic_strength: hs,
                thermal: thermal_iterations > 0,
                thermal_iterations: thermal_iterations.max(1),
                talus_angle: talus,
                thermal_strength: ts,
            }
        };
        use Generator as G;
        match self {
            Self::Desert => (
                120.0,
                base(G::Dunes, 0.4, 260.0, 0.35, 5, 2.0, 311),
                erosion(0, 0.5, 15, 32.0, 0.4),
                16.0,
            ),
            Self::Forest => (
                100.0,
                base(G::RollingHills, 0.5, 260.0, 0.45, 6, 2.0, 1207),
                erosion(4, 0.5, 20, 40.0, 0.4),
                12.0,
            ),
            Self::Mountains => (
                260.0,
                base(G::Mountains, 0.9, 320.0, 0.5, 8, 2.6, 4242),
                erosion(8, 0.6, 25, 42.0, 0.4),
                14.0,
            ),
            Self::Plains => (
                60.0,
                base(G::RollingHills, 0.3, 450.0, 0.25, 5, 2.0, 77),
                erosion(2, 0.3, 10, 40.0, 0.3),
                18.0,
            ),
            Self::Islands => (
                140.0,
                base(G::Islands, 0.5, 220.0, 0.5, 7, 2.2, 2024),
                erosion(4, 0.5, 20, 40.0, 0.4),
                14.0,
            ),
            Self::Canyon => (
                180.0,
                base(G::Canyons, 0.75, 280.0, 0.45, 6, 2.0, 919),
                erosion(3, 0.5, 15, 55.0, 0.4),
                12.0,
            ),
            Self::Tundra => (
                90.0,
                base(G::RollingHills, 0.35, 300.0, 0.4, 6, 2.0, 1312),
                erosion(2, 0.4, 30, 35.0, 0.4),
                14.0,
            ),
            Self::Volcanic => (
                220.0,
                base(G::Islands, 0.35, 240.0, 0.55, 7, 2.4, 666),
                erosion(3, 0.5, 20, 38.0, 0.4),
                12.0,
            ),
        }
    }

    /// Stamps: name, (u, v, height fraction), yaw (degrees), stamp. `k` scales radii and
    /// river points (terrain width / 600 m); `height` is the terrain's maximum height.
    #[allow(clippy::type_complexity)]
    fn stamps(self, k: f32, height: f32) -> Vec<(&'static str, (f32, f32, f32), f32, WorldStamp)> {
        let stamp = |radius: f32, falloff, kind| WorldStamp {
            enabled: true,
            radius: radius * k,
            falloff,
            kind,
        };
        let mountain = |shape, h: f32, blend, roughness| {
            StampKind::Mountain(Mountain {
                shape,
                height: h,
                blend,
                roughness,
            })
        };
        let zone = |p: Preset| {
            StampKind::BiomeZone(BiomeZone {
                biome: p.path(),
                polygon: Vec::new(),
            })
        };
        let river = |points: &[[f32; 2]], width: f32, depth: f32, bank: f32| WorldStamp {
            enabled: true,
            radius: 0.0,
            falloff: 0.0,
            kind: StampKind::River(River {
                points: points.iter().map(|p| [p[0] * k, p[1] * k]).collect(),
                width: width * k.max(0.3),
                depth,
                bank: bank * k.max(0.3),
                water: true,
            }),
        };
        let water = |mode| StampKind::Water(Water { mode });
        use MountainShape as M;
        match self {
            Self::Desert => vec![
                (
                    "Mesa",
                    (0.27, 0.7, 0.0),
                    0.0,
                    stamp(
                        60.0,
                        0.25,
                        mountain(M::Mesa, 0.42 * height, BlendMode::Max, 0.4),
                    ),
                ),
                (
                    "Mesa 2",
                    (0.73, 0.3, 0.0),
                    0.0,
                    stamp(
                        45.0,
                        0.25,
                        mountain(M::Mesa, 0.36 * height, BlendMode::Max, 0.4),
                    ),
                ),
                (
                    "Butte",
                    (0.62, 0.78, 0.0),
                    0.0,
                    stamp(
                        28.0,
                        0.2,
                        mountain(M::Mesa, 0.5 * height, BlendMode::Max, 0.3),
                    ),
                ),
                (
                    "Badlands",
                    (0.3, 0.68, 0.0),
                    0.0,
                    stamp(110.0, 0.6, zone(Preset::RockyBadlands)),
                ),
            ],
            Self::Forest => vec![
                (
                    "River",
                    (0.5, 0.5, 0.0),
                    0.0,
                    river(
                        &[
                            [-300.0, -60.0],
                            [-150.0, 20.0],
                            [0.0, -20.0],
                            [150.0, 50.0],
                            [300.0, 10.0],
                        ],
                        9.0,
                        3.0,
                        14.0,
                    ),
                ),
                (
                    "Meadow",
                    (0.3, 0.27, 0.0),
                    0.0,
                    stamp(75.0, 0.6, zone(Preset::Grassland)),
                ),
                (
                    "Rocky Hill",
                    (0.75, 0.75, 0.0),
                    0.0,
                    stamp(55.0, 0.5, StampKind::RockArea(RockArea { strength: 0.7 })),
                ),
            ],
            Self::Mountains => vec![
                (
                    "Summit Ridge",
                    (0.55, 0.65, 0.0),
                    30.0,
                    stamp(
                        170.0,
                        0.6,
                        mountain(M::Ridge, 0.25 * height, BlendMode::Add, 0.4),
                    ),
                ),
                (
                    "Valley River",
                    (0.5, 0.5, 0.0),
                    0.0,
                    river(
                        &[
                            [-300.0, -140.0],
                            [-150.0, -80.0],
                            [0.0, -120.0],
                            [150.0, -60.0],
                            [300.0, -100.0],
                        ],
                        10.0,
                        6.0,
                        30.0,
                    ),
                ),
                (
                    "Crags",
                    (0.55, 0.65, 0.0),
                    0.0,
                    stamp(80.0, 0.5, StampKind::RockArea(RockArea { strength: 0.8 })),
                ),
            ],
            Self::Plains => vec![
                (
                    "Woods",
                    (0.75, 0.7, 0.0),
                    0.0,
                    stamp(95.0, 0.6, zone(Preset::Forest)),
                ),
                (
                    "Copse",
                    (0.2, 0.25, 0.0),
                    0.0,
                    stamp(55.0, 0.6, zone(Preset::Forest)),
                ),
                (
                    "River",
                    (0.5, 0.5, 0.0),
                    0.0,
                    river(
                        &[
                            [-300.0, 80.0],
                            [-100.0, 10.0],
                            [100.0, 50.0],
                            [300.0, -30.0],
                        ],
                        7.0,
                        2.0,
                        10.0,
                    ),
                ),
                (
                    "Village Site",
                    (0.4, 0.62, 0.0),
                    0.0,
                    stamp(40.0, 0.45, StampKind::Flatten(Flatten { sample: true })),
                ),
            ],
            Self::Islands => vec![
                (
                    "Hill",
                    (0.45, 0.55, 0.0),
                    0.0,
                    stamp(
                        90.0,
                        0.6,
                        mountain(M::Cone, 0.25 * height, BlendMode::Add, 0.4),
                    ),
                ),
                (
                    "Island Forest",
                    (0.5, 0.5, 0.0),
                    0.0,
                    stamp(120.0, 0.6, zone(Preset::Forest)),
                ),
                (
                    "Sea",
                    (0.5, 0.5, 0.065),
                    0.0,
                    stamp(40.0, 0.0, water(WaterMode::Sea)),
                ),
            ],
            Self::Canyon => vec![
                (
                    "Gorge",
                    (0.5, 0.5, 0.0),
                    0.0,
                    river(
                        &[
                            [-300.0, 40.0],
                            [-140.0, -30.0],
                            [0.0, 30.0],
                            [140.0, -20.0],
                            [300.0, 30.0],
                        ],
                        12.0,
                        8.0,
                        20.0,
                    ),
                ),
                (
                    "Sand Flats",
                    (0.2, 0.8, 0.0),
                    0.0,
                    stamp(110.0, 0.6, zone(Preset::Desert)),
                ),
            ],
            Self::Tundra => vec![
                (
                    "Ridge",
                    (0.5, 0.88, 0.0),
                    0.0,
                    stamp(
                        200.0,
                        0.6,
                        mountain(M::Ridge, 0.45 * height, BlendMode::Add, 0.5),
                    ),
                ),
                // A basin dug below the lowest ground, so the lake stays inside it.
                (
                    "Lake Basin",
                    (0.4, 0.35, 0.0),
                    0.0,
                    stamp(80.0, 0.5, StampKind::Flatten(Flatten { sample: false })),
                ),
                (
                    "Lake",
                    (0.4, 0.35, 0.025),
                    0.0,
                    stamp(30.0, 0.0, water(WaterMode::Lake)),
                ),
                (
                    "Snowy Peaks",
                    (0.5, 0.88, 0.0),
                    0.0,
                    stamp(120.0, 0.5, zone(Preset::SnowyMountains)),
                ),
            ],
            Self::Volcanic => vec![
                (
                    "Volcano",
                    (0.5, 0.5, 0.0),
                    0.0,
                    stamp(
                        190.0,
                        0.6,
                        mountain(M::Volcano, 0.8 * height, BlendMode::Replace, 0.3),
                    ),
                ),
                (
                    "Lava Rock",
                    (0.5, 0.5, 0.0),
                    0.0,
                    stamp(140.0, 0.5, StampKind::RockArea(RockArea { strength: 0.6 })),
                ),
                (
                    "Green Shore",
                    (0.2, 0.22, 0.0),
                    0.0,
                    stamp(90.0, 0.6, zone(Preset::Coast)),
                ),
                (
                    "Sea",
                    (0.5, 0.5, 0.05),
                    0.0,
                    stamp(40.0, 0.0, water(WaterMode::Sea)),
                ),
            ],
        }
    }

    /// Biome presets the template uses (default first).
    pub fn presets(self) -> Vec<Preset> {
        let mut out = vec![self.biome()];
        for s in self.setup(&WorldBuilder::default()).stamps {
            if let StampKind::BiomeZone(z) = &s.stamp.kind
                && let Some(p) = Preset::ALL.into_iter().find(|p| p.path() == z.biome)
                && !out.contains(&p)
            {
                out.push(p);
            }
        }
        out
    }

    /// A standalone generation request for the template on a `size` terrain at `res` (tests and
    /// thumbnails), the same as the World Builder builds from the scene.
    pub fn request(self, size: [f32; 3], res: usize) -> Request {
        let current = WorldBuilder {
            size,
            heightmap_resolution: res,
            alphamap_resolution: (res - 1).clamp(16, 1024),
            ..Default::default()
        };
        let setup = self.setup(&current);
        let settings = setup.settings;
        let mut paths = vec![settings.default_biome.clone()];
        let mut stamps = Vec::new();
        let mut zone_biome = Vec::new();
        for (i, t) in setup.stamps.into_iter().enumerate() {
            let b = match &t.stamp.kind {
                StampKind::BiomeZone(z) => {
                    paths.iter().position(|p| *p == z.biome).unwrap_or_else(|| {
                        paths.push(z.biome.clone());
                        paths.len() - 1
                    })
                }
                _ => 0,
            };
            zone_biome.push(b);
            let m = Mat4::from_rotation_translation(
                Quat::from_rotation_y(t.yaw.to_radians()),
                t.position,
            );
            stamps.push(PlacedStamp {
                stamp: t.stamp,
                at: Placement::from_matrix(&m, Vec3::ZERO),
                seed: settings.base.seed.wrapping_add(i as u32 * 7919),
            });
        }
        let biomes = paths
            .iter()
            .map(|p| {
                Preset::ALL
                    .into_iter()
                    .find(|x| x.path() == *p)
                    .map(|x| x.biome())
                    .unwrap_or_default()
            })
            .collect();
        Request {
            settings,
            stamps,
            biomes,
            zone_biome,
            stages: Stages::All,
            current: None,
            preview: false,
            preview_resolution: PREVIEW_RESOLUTION,
        }
    }
}

impl Template {
    /// A top-down picture of the template's world (Biomes preview, hillshaded, water in blue)
    /// on a 600 m terrain: the picker's thumbnails.
    pub fn thumbnail(self, size: u32) -> image::RgbaImage {
        use super::preview::{self, PreviewMode};
        let req = self.request([REFERENCE, 0.0, REFERENCE], 257);
        let out = super::generate::run(&req, &super::Progress::default()).expect("not cancelled");
        let colors: Vec<[f32; 3]> = out
            .data
            .layers
            .iter()
            .map(|l| preview::layer_color(l, &|_| None))
            .collect();
        let img = preview::render(
            &out.data,
            Some(&out.wet),
            PreviewMode::Biomes,
            size as usize,
            &|i| colors.get(i).copied().unwrap_or([0.5; 3]),
        );
        image::RgbaImage::from_raw(size, size, img.rgba.into_iter().flatten().collect())
            .expect("size² pixels")
    }
}

/// Terrain-local x/z of a template stamp (helper for placing it under a terrain corner).
pub fn local_xz(t: &TemplateStamp) -> Vec2 {
    Vec2::new(t.position.x, t.position.z)
}
