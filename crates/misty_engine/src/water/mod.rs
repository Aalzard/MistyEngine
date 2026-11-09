//! Water system: Water Surface components (ocean / lake / river), the wave model shared with
//! the GPU, height queries (`Water.GetHeight`) and buoyancy (Floater).
//!
//! Public API for other systems (e.g. a world generator placing lakes and rivers):
//! [`spawn_water`] creates a surface object, [`height_at`] samples the water under a point and
//! [`WaterSurface`] / [`Floater`] are the components.

pub mod buoyancy;
pub mod settings;
pub mod waves;

use glam::{Mat4, Vec2, Vec3};
use hecs::Entity;

pub use crate::scene::water::{Floater, WaterKind, WaterSurface};
pub use settings::{SpectrumKind, WaterPreset, WaterSettings};
pub use waves::WaveState;

use crate::scene::{Scene, Transform};

/// Animation time of the waves (seconds; follows `Time.time` while playing).
pub fn time() -> f32 {
    (crate::clock::now() % 100_000.0) as f32
}

/// Renderer input for one water surface (and everything a height query needs).
#[derive(Clone, Debug, PartialEq)]
pub struct WaterDraw {
    pub kind: WaterKind,
    /// The object's world matrix: lakes and rivers are its local XZ rectangle; for the ocean only
    /// the height matters.
    pub model: Mat4,
    /// Lake / river extent in local metres.
    pub size: [f32; 2],
    pub settings: WaterSettings,
    pub time: f32,
}

impl WaterDraw {
    pub fn new(surface: &WaterSurface, model: Mat4, time: f32) -> Self {
        Self {
            kind: surface.kind,
            model,
            size: surface.size,
            settings: surface.settings,
            time,
        }
    }

    /// Rest height of the surface.
    pub fn level(&self) -> f32 {
        self.model.w_axis.y
    }

    /// River surface velocity (world XZ, m/s); zero for oceans and lakes.
    pub fn flow(&self) -> Vec2 {
        if self.kind != WaterKind::River {
            return Vec2::ZERO;
        }
        let f = self.model.z_axis.truncate();
        Vec2::new(f.x, f.z).normalize_or(Vec2::Y) * self.settings.flow_speed
    }

    fn flow_dir(&self) -> Option<Vec2> {
        (self.kind == WaterKind::River).then(|| self.flow().normalize_or(Vec2::Y))
    }

    /// Is a world XZ point over this surface?
    pub fn contains(&self, p: Vec2) -> bool {
        if self.kind == WaterKind::Ocean {
            return true;
        }
        let local = self
            .model
            .inverse()
            .transform_point3(Vec3::new(p.x, self.level(), p.y));
        local.x.abs() <= self.size[0] * 0.5 && local.z.abs() <= self.size[1] * 0.5
    }

    /// The wave model at this draw's time.
    pub fn waves(&self) -> WaveState {
        WaveState::new(&self.settings, self.flow_dir(), self.time)
    }

    /// World height of the water surface above a world XZ point.
    pub fn height(&self, p: Vec2) -> f32 {
        self.level() + self.waves().height(p, self.flow())
    }
}

/// Enabled water surfaces of active objects, in Hierarchy order.
pub fn draws(scene: &Scene) -> Vec<WaterDraw> {
    draws_where(scene, |_| true)
}

/// [`draws`] of the objects `keep` accepts (a camera's culling mask).
pub fn draws_where(scene: &Scene, keep: impl Fn(Entity) -> bool) -> Vec<WaterDraw> {
    if !scene.has_any::<WaterSurface>() {
        return Vec::new();
    }
    let t = time();
    scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.active_in_hierarchy(*e) && keep(*e))
        .filter_map(|(e, _)| {
            let w = scene.world.get::<&WaterSurface>(e).ok()?;
            w.enabled
                .then(|| WaterDraw::new(&w, scene.world_matrix(e), t))
        })
        .collect()
}

/// Height of the water under a world point (the highest surface covering it), `None` when
/// there is no water there.
pub fn height_at(scene: &Scene, x: f32, z: f32) -> Option<f32> {
    let p = Vec2::new(x, z);
    draws(scene)
        .iter()
        .filter(|d| d.contains(p))
        .map(|d| d.height(p))
        .max_by(f32::total_cmp)
}

/// Create a water surface object: an ocean at `position.y`, or a lake / river rectangle of
/// `size` (local X × Z metres) centred at `position`. Settings come from the kind's preset.
pub fn spawn_water(
    scene: &mut Scene,
    name: &str,
    kind: WaterKind,
    position: Vec3,
    size: Vec2,
) -> Entity {
    let e = scene.spawn(name, None);
    scene.set_transform(e, Transform::from_position(position));
    let mut surface = WaterSurface::new(kind);
    if kind != WaterKind::Ocean {
        surface.size = size.to_array();
    }
    scene.add(e, surface);
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_queries_cover_their_surfaces() {
        let mut s = Scene::new("t");
        assert_eq!(height_at(&s, 0.0, 0.0), None);
        let lake = spawn_water(
            &mut s,
            "Lake",
            WaterKind::Lake,
            Vec3::new(10.0, 5.0, 0.0),
            Vec2::new(8.0, 4.0),
        );
        let h = height_at(&s, 10.0, 0.0).expect("inside the lake");
        assert!((h - 5.0).abs() < 1.0, "{h}");
        assert_eq!(height_at(&s, 20.0, 0.0), None, "outside the lake");
        spawn_water(&mut s, "Ocean", WaterKind::Ocean, Vec3::ZERO, Vec2::ZERO);
        let sea = height_at(&s, 200.0, -50.0).expect("the ocean is everywhere");
        assert!(sea.abs() < 4.0);
        assert!(
            height_at(&s, 10.0, 0.0).unwrap() > 3.0,
            "highest surface wins"
        );
        // Disabled surfaces are ignored.
        s.world.get::<&mut WaterSurface>(lake).unwrap().enabled = false;
        assert!(height_at(&s, 10.0, 0.0).unwrap() < 4.0);
    }
}
