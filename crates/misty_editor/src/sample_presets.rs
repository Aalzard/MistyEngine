//! Sample presets in `SampleProject/Assets/Presets`: Warm Street Lamp (a point light with a
//! halo), Stormy Sky (only a Post-process Volume's Volumetric Clouds and Fog), Bouncy (a Sphere
//! Collider's surface, keeping the collider's size) and Pixel Art (texture import settings for
//! crisp sprites). Regenerate with `cargo test -p misty_editor -- --ignored write_sample_presets`;
//! the other tests check the committed files still match and apply cleanly.

use std::path::PathBuf;

use misty_engine::preset::{self, Preset};
use misty_engine::scene::{
    CloudPreset, Light, LightType, PhysicsSurface, PostProcessVolume, Shadows, SphereCollider,
};
use misty_engine::sprites::{FilterMode, TextureImporter, TextureType};

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Untick every field of `p` except `keep`.
fn only(mut p: Preset, keep: &[&str]) -> Preset {
    for f in p.fields() {
        if !keep.contains(&f.as_str()) {
            p.set_included(&f, false);
        }
    }
    p
}

fn street_lamp() -> Preset {
    let mut l = Light {
        kind: LightType::Point,
        color: [1.0, 0.72, 0.42],
        intensity: 2.5,
        range: 12.0,
        shadows: Shadows::Soft,
        ..Default::default()
    };
    l.halo.enabled = true;
    l.halo.size = 0.6;
    l.halo.intensity = 1.2;
    l.halo.softness = 0.6;
    Preset::of_value("Light", &l)
}

fn stormy_sky() -> Preset {
    let mut v = PostProcessVolume::default();
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Stormy.apply(c);
    c.wind_speed = 25.0;
    c.ambient = 0.6;
    let f = &mut v.fog;
    f.active = true;
    f.attenuation_distance = 1500.0;
    f.max_fog_distance = 2500.0;
    f.albedo = [0.62, 0.66, 0.72];
    f.noise_intensity = 0.4;
    only(
        Preset::of_value("Post-process Volume", &v),
        &["clouds", "fog"],
    )
}

fn bouncy() -> Preset {
    let s = SphereCollider {
        surface: PhysicsSurface {
            friction: 0.15,
            bounciness: 0.9,
        },
        ..Default::default()
    };
    only(Preset::of_value("Sphere Collider", &s), &["surface"])
}

fn pixel_art() -> Preset {
    let t = TextureImporter {
        texture_type: TextureType::Sprite,
        pixels_per_unit: 16.0,
        filter_mode: FilterMode::Nearest,
        generate_mip_maps: false,
        ..Default::default()
    };
    let mut p = Preset::of_value(preset::TEXTURE_IMPORTER, &t);
    p.set_included("sprites", false);
    p
}

/// File stem → preset.
fn presets() -> Vec<(&'static str, Preset)> {
    vec![
        ("Warm Street Lamp", street_lamp()),
        ("Stormy Sky", stormy_sky()),
        ("Bouncy", bouncy()),
        ("Pixel Art", pixel_art()),
    ]
}

fn path(stem: &str) -> PathBuf {
    project()
        .join(preset::FOLDER)
        .join(format!("{stem}.preset"))
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_sample_presets() {
    for (stem, p) in presets() {
        p.save(&path(stem)).unwrap();
    }
}

#[test]
fn sample_presets_match_their_generator() {
    for (stem, p) in presets() {
        let on_disk = Preset::load(&path(stem)).unwrap();
        assert_eq!(on_disk, p, "{stem}");
        on_disk.check().unwrap();
    }
}

#[test]
fn sample_presets_apply_as_described() {
    use misty_engine::hecs::World;
    let mut w = World::new();
    let mut lit = PostProcessVolume::default();
    lit.bloom.active = true;
    let sphere = SphereCollider {
        radius: 2.0,
        ..Default::default()
    };
    let e = w.spawn((Light::default(), lit, sphere));
    street_lamp().apply(&mut w, e, &[]).unwrap();
    stormy_sky().apply(&mut w, e, &[]).unwrap();
    bouncy().apply(&mut w, e, &[]).unwrap();
    assert!(w.get::<&Light>(e).unwrap().halo.enabled);
    let v = w.get::<&PostProcessVolume>(e).unwrap();
    // Only clouds and fog change: the volume's own bloom stays on.
    assert!(v.clouds.active && v.fog.active && v.bloom.active);
    let s = w.get::<&SphereCollider>(e).unwrap();
    assert_eq!((s.radius, s.surface.bounciness), (2.0, 0.9));
    let t = pixel_art()
        .apply_value(&TextureImporter::default())
        .unwrap();
    assert_eq!(t.filter_mode, FilterMode::Nearest);
}
