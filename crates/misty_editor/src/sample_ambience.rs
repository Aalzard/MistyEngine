//! Ambient sound for the outdoor sample scenes (NoonValley, MeadowDemo): an "Ambience" object
//! with `AmbienceZones.lua`, an open-ground loop (2D) and a forest loop (3D, Linear rolloff)
//! that the script moves to the closest point of the nearest forest area (box footprints given
//! by the scene's generator), so the birds come from the woods, swell as you walk up to them and
//! surround you inside, while the meadow loop gives way. The loops are the sample's recordings
//! under `Assets/Audio/Ambience` (prepared by `tools/sample_sounds.py`).

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    AudioRolloff, AudioSource, Scene, ScriptRef, ScriptValue, Scripts, Transform,
};

pub const SCRIPT: &str = "Assets/Scripts/AmbienceZones.lua";
pub const MEADOW: &str = "Assets/Audio/Ambience/meadow_ambience_loop.ogg";
pub const FOREST: &str = "Assets/Audio/Ambience/forest_birds_loop.ogg";
pub const BREEZE: &str = "Assets/Audio/Ambience/breeze_grass_loop.ogg";

/// Names of the rig's objects (the script's Open Source / Zone Source / Zone Prefix).
pub const ROOT: &str = "Ambience";
const OPEN: &str = "Meadow Ambience";
const ZONE: &str = "Forest Birds";
const AREA: &str = "Forest Area";

/// A forest area: a box footprint centred at `centre` (world x, z), `size` metres across
/// (its own x, z), turned `yaw` degrees about the vertical.
#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub centre: Vec2,
    pub size: Vec2,
    pub yaw: f32,
}

impl Area {
    /// The area along an edge from `a` to `b` (world x, z), reaching `depth` metres to the
    /// right of it (looking from `a` to `b`).
    pub fn beside(a: Vec2, b: Vec2, depth: f32) -> Self {
        let along = b - a;
        let right = Vec2::new(along.y, -along.x).normalize();
        Self {
            centre: (a + b) * 0.5 + right * depth * 0.5,
            size: Vec2::new(depth, along.length()),
            // The box's +Z runs along the edge (glam's Y rotation: +Z turns towards +X).
            yaw: along.x.atan2(along.y).to_degrees(),
        }
    }

    /// Distance from `p` (world x, z) to the footprint (0 inside).
    pub fn distance(&self, p: Vec2) -> f32 {
        let (s, c) = self.yaw.to_radians().sin_cos();
        // The box's right (+X) and forward (+Z) axes on the ground.
        let (right, forward) = (Vec2::new(c, -s), Vec2::new(s, c));
        let d = p - self.centre;
        let local = Vec2::new(d.dot(right), d.dot(forward));
        let out = (local.abs() - self.size * 0.5).max(Vec2::ZERO);
        out.length()
    }
}

/// Distance from `p` to the nearest of `areas`.
pub fn distance(areas: &[Area], p: Vec2) -> f32 {
    areas
        .iter()
        .map(|a| a.distance(p))
        .fold(f32::INFINITY, f32::min)
}

/// A looping clip that starts with Play.
fn looping(clip: &str, volume: f32) -> AudioSource {
    AudioSource {
        clip: Some(clip.into()),
        looping: true,
        play_on_awake: true,
        volume,
        ..Default::default()
    }
}

/// The rig: `open` everywhere, the forest birds from `areas`, `extra` 2D loops (clip, volume)
/// on top. `y` is a height near the ground for the objects (the script keeps the forest loop
/// at the listener's height).
pub fn add(
    s: &mut Scene,
    y: f32,
    open: (&str, f32),
    areas: &[Area],
    extra: &[(&str, &str, f32)],
) -> Entity {
    let root = s.spawn(ROOT, None);
    s.set_transform(root, Transform::from_position(Vec3::new(0.0, y, 0.0)));
    let fields = [
        ("openSource", ScriptValue::Text(OPEN.into())),
        ("zoneSource", ScriptValue::Text(ZONE.into())),
        ("zonePrefix", ScriptValue::Text(AREA.into())),
        ("openVolume", ScriptValue::Number(open.1 as f64)),
    ];
    s.add(
        root,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                fields: fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
                ..Default::default()
            }],
        },
    );
    let meadow = s.spawn(OPEN, Some(root));
    s.add(meadow, looping(open.0, open.1));
    let birds = s.spawn(ZONE, Some(root));
    s.add(
        birds,
        AudioSource {
            spatial_blend: 1.0,
            rolloff: AudioRolloff::Linear,
            // Full within 25 m of the trees, gone 160 m out; heard over a wide arc.
            min_distance: 25.0,
            max_distance: 160.0,
            spread: 120.0,
            doppler_level: 0.0,
            ..looping(FOREST, 0.8)
        },
    );
    for (name, clip, volume) in extra {
        let e = s.spawn(name, Some(root));
        s.add(e, looping(clip, *volume));
    }
    for (i, a) in areas.iter().enumerate() {
        let e = s.spawn(&format!("{AREA} {}", i + 1), Some(root));
        let t = Transform {
            position: Vec3::new(a.centre.x, 0.0, a.centre.y),
            scale: Vec3::new(a.size.x, 1.0, a.size.y),
            ..Default::default()
        }
        .with_euler(Vec3::new(0.0, a.yaw, 0.0));
        s.set_transform(e, t);
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
    }

    #[test]
    fn ambience_clips_and_script_are_in_the_sample_project() {
        let root = project();
        for rel in [SCRIPT, MEADOW, FOREST, BREEZE] {
            assert!(root.join(rel).is_file(), "{rel}");
            assert!(root.join(format!("{rel}.meta")).is_file(), "{rel}.meta");
        }
    }

    #[test]
    fn areas_measure_distance_like_the_script() {
        let a = Area::beside(Vec2::new(0.0, 0.0), Vec2::new(0.0, 100.0), 50.0);
        // Along +Z, reaching 50 m to the right (+X).
        assert!((a.centre - Vec2::new(25.0, 50.0)).length() < 1e-4, "{a:?}");
        assert!(a.distance(Vec2::new(10.0, 50.0)) == 0.0);
        assert!((a.distance(Vec2::new(-20.0, 50.0)) - 20.0).abs() < 1e-4);
        assert!((a.distance(Vec2::new(25.0, 130.0)) - 30.0).abs() < 1e-4);
        // A turned area: the script finds the same distances from the object's axes.
        let b = Area::beside(Vec2::new(0.0, 0.0), Vec2::new(60.0, 80.0), 40.0);
        let mut s = Scene::new("t");
        add(&mut s, 0.0, (MEADOW, 0.6), &[b], &[]);
        let e = s.find_by_name(&format!("{AREA} 1")).unwrap();
        let m = s.world_matrix(e);
        let (right, forward) = (
            m.x_axis.truncate().normalize(),
            m.z_axis.truncate().normalize(),
        );
        let lua = format!(
            r#"
            local A = AmbienceZones
            local c = {{ x = {cx}, z = {cz} }}
            local r = {{ x = {rx}, z = {rz} }}
            local f = {{ x = {fx}, z = {fz} }}
            for _, p in ipairs({{ {{0, 0}}, {{-30, 10}}, {{100, 0}}, {{30, 40}}, {{0, 200}} }}) do
                local _, _, d = A.closestPoint({{ x = p[1], z = p[2] }}, c, r, f, {hx}, {hz})
                local want = WANT[p[1] .. "," .. p[2]]
                assert(math.abs(d - want) < 1e-3, p[1] .. "," .. p[2] .. ": " .. d .. " vs " .. want)
            end
            -- Inside a zone the open loop drops to Open In Zone and the zone loop turns 2D.
            local open, spatial = A.blend(A, 0)
            assert(math.abs(open - A.openVolume * A.openInZone) < 1e-6 and spatial == 0)
            open, spatial = A.blend(A, 1000)
            assert(math.abs(open - A.openVolume) < 1e-6 and spatial == 1)
            local o1 = A.blend(A, 10)
            local o2 = A.blend(A, 30)
            assert(o1 < o2, "the open loop rises walking out")
            "#,
            cx = b.centre.x,
            cz = b.centre.y,
            rx = right.x,
            rz = right.z,
            fx = forward.x,
            fz = forward.z,
            hx = b.size.x * 0.5,
            hz = b.size.y * 0.5,
        );
        let want: Vec<String> = [
            (0.0, 0.0),
            (-30.0, 10.0),
            (100.0, 0.0),
            (30.0, 40.0),
            (0.0, 200.0),
        ]
        .iter()
        .map(|&(x, z)| format!("[\"{x},{z}\"] = {}", b.distance(Vec2::new(x, z))))
        .collect();
        let chunk = format!("WANT = {{ {} }}\n{lua}", want.join(", "));
        let src = std::fs::read_to_string(project().join(SCRIPT)).unwrap();
        misty_engine::scripting::test_scripts(&[("AmbienceZones", &src)], &chunk)
            .unwrap_or_else(|e| panic!("{e}"));
    }
}
