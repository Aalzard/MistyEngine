//! Wind Zone: wind for the terrain grass. The zone's forward axis (+Z) is where the wind blows;
//! its values multiply each grass layer's own wind settings (1 = unchanged). The first enabled
//! zone of active objects is used.

use serde::{Deserialize, Serialize};

use super::registry::SceneComponent;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindZone {
    pub enabled: bool,
    /// × the grass wind strength (how far blades bend).
    pub main: f32,
    /// × the fast flutter of single blades.
    pub turbulence: f32,
    /// × the strength of the gust waves rolling across the field.
    pub pulse_magnitude: f32,
    /// × the speed the gust waves travel at.
    pub pulse_frequency: f32,
}

impl Default for WindZone {
    fn default() -> Self {
        Self {
            enabled: true,
            main: 1.0,
            turbulence: 1.0,
            pulse_magnitude: 1.0,
            pulse_frequency: 1.0,
        }
    }
}

impl SceneComponent for WindZone {
    const NAME: &'static str = "Wind Zone";
    const CATEGORY: &'static str = "Miscellaneous";
}

/// The scene's wind for the renderer: the first enabled Wind Zone in Hierarchy order.
pub fn scene_wind(scene: &super::Scene) -> Option<crate::render::Wind> {
    let mut zones = scene.active_with::<WindZone>();
    scene.sort_hierarchy_order(&mut zones);
    zones.into_iter().find_map(|e| {
        let z = scene.world.get::<&WindZone>(e).ok()?;
        if !z.enabled {
            return None;
        }
        let (_, rot, _) = scene.world_matrix(e).to_scale_rotation_translation();
        Some(crate::render::Wind {
            direction: rot * glam::Vec3::Z,
            main: z.main.max(0.0),
            turbulence: z.turbulence.max(0.0),
            pulse_magnitude: z.pulse_magnitude.max(0.0),
            pulse_frequency: z.pulse_frequency.max(0.0),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Scene, Transform};

    #[test]
    fn first_enabled_zone_sets_the_wind() {
        let mut s = Scene::default_3d("Wind");
        assert!(scene_wind(&s).is_none());
        let off = s.spawn("Off", None);
        s.add(
            off,
            WindZone {
                enabled: false,
                ..Default::default()
            },
        );
        let on = s.spawn("Wind", None);
        s.set_transform(
            on,
            Transform::default().with_euler(glam::Vec3::new(0.0, 90.0, 0.0)),
        );
        s.add(
            on,
            WindZone {
                main: 2.0,
                ..Default::default()
            },
        );
        let w = scene_wind(&s).expect("the enabled zone");
        assert!(
            (w.direction - glam::Vec3::X).length() < 1e-4,
            "{:?}",
            w.direction
        );
        assert_eq!(w.main, 2.0);
        let back: WindZone = serde_json::from_str("{}").unwrap();
        assert_eq!(back, WindZone::default());
    }
}
