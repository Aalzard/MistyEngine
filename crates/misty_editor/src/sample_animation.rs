//! The AnimationDemo sample in `SampleProject`: a bobbing, spinning cube and a blinking light,
//! with their `.anim` clips. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_animation_sample`; the other test checks the
//! committed files still match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::animation::{AnimationClip, Property, TangentMode};
use misty_engine::scene::{
    Animation, Light, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef, Scripts, Transform,
};

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

const BOB: &str = "Assets/Animations/Bob.anim";
const BLINK: &str = "Assets/Animations/Blink.anim";
const SCENE: &str = "Assets/Scenes/AnimationDemo.scene";

/// "Bobber" floats up and down; its child "Cube" turns once per loop (linear, so it never eases).
fn bob() -> AnimationClip {
    let mut c = AnimationClip {
        length: 2.0,
        ..Default::default()
    };
    for (t, y) in [(0.0, 0.6), (1.0, 1.6), (2.0, 0.6)] {
        c.set_key("", Property::Position(1), t, y);
    }
    for (t, deg) in [(0.0, 0.0), (2.0, 360.0)] {
        c.set_key("Cube", Property::Rotation(1), t, deg);
    }
    let spin = c.curves.len() - 1;
    for i in 0..2 {
        c.curves[spin].keys.set_tangent(i, TangentMode::Linear);
    }
    c
}

/// The light switches on and off (Constant keys) and changes colour each time.
fn blink() -> AnimationClip {
    let mut c = AnimationClip {
        length: 1.0,
        ..Default::default()
    };
    let keys = [(0.0, 3.0, [1.0, 0.6, 0.2]), (0.5, 0.2, [0.3, 0.5, 1.0])];
    for (t, intensity, rgb) in keys {
        c.set_key("", Property::LightIntensity, t, intensity);
        for (ch, v) in rgb.into_iter().enumerate() {
            c.set_key("", Property::LightColor(ch as u8), t, v);
        }
    }
    for curve in &mut c.curves {
        for i in 0..curve.keys.len() {
            curve.keys.set_tangent(i, TangentMode::Constant);
        }
    }
    c
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("AnimationDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.5, -7.0))
                .with_euler(Vec3::new(12.0, 0.0, 0.0)),
        );
    }
    let mesh =
        |s: &mut Scene, name: &str, parent, p: PrimitiveMesh, pos: Vec3, mat: Option<&str>| {
            let e = s.spawn(name, parent);
            s.set_transform(e, Transform::from_position(pos));
            s.add(e, MeshFilter::primitive(p));
            s.add(
                e,
                MeshRenderer {
                    material: mat.map(Into::into),
                    ..Default::default()
                },
            );
            e
        };
    mesh(
        &mut s,
        "Floor",
        None,
        PrimitiveMesh::Plane,
        Vec3::ZERO,
        None,
    );

    let bobber = s.spawn("Bobber", None);
    s.set_transform(bobber, Transform::from_position(Vec3::new(-1.5, 0.6, 0.0)));
    let red = Some("Assets/Materials/Red.mat");
    mesh(
        &mut s,
        "Cube",
        Some(bobber),
        PrimitiveMesh::Cube,
        Vec3::ZERO,
        red,
    );
    s.add(
        bobber,
        Animation {
            clip: Some(BOB.into()),
            clips: vec![BOB.into()],
            ..Default::default()
        },
    );
    s.add(
        bobber,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/AnimationControl.lua".into(),
                ..Default::default()
            }],
        },
    );

    let lamp = s.spawn("Blinking Light", None);
    s.set_transform(lamp, Transform::from_position(Vec3::new(1.8, 1.2, -0.5)));
    s.add(
        lamp,
        Light {
            intensity: 3.0,
            range: 6.0,
            color: [1.0, 0.6, 0.2],
            ..Default::default()
        },
    );
    s.add(
        lamp,
        Animation {
            clip: Some(BLINK.into()),
            clips: vec![BLINK.into()],
            ..Default::default()
        },
    );
    mesh(
        &mut s,
        "Pedestal",
        None,
        PrimitiveMesh::Cylinder,
        Vec3::new(1.8, 0.0, 0.5),
        None,
    );
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_animation_sample() {
    let root = project();
    bob().save(&root.join(BOB)).unwrap();
    blink().save(&root.join(BLINK)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn animation_sample_matches_its_generator_and_plays() {
    let root = project();
    crate::test_util::assert_close(&AnimationClip::load(&root.join(BOB)).unwrap(), &bob(), BOB);
    crate::test_util::assert_close(
        &AnimationClip::load(&root.join(BLINK)).unwrap(),
        &blink(),
        BLINK,
    );
    let (mut scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Play it for a second: the cube bobs and the light blinks.
    let mut rt = misty_engine::runtime::Runtime::start_with_audio(
        &mut scene,
        root.clone(),
        &Default::default(),
        misty_engine::audio::Audio::null(root).0,
    );
    let mut time = misty_engine::runtime::Time::default();
    let bobber = scene.find_by_name("Bobber").unwrap();
    let lamp = scene.find_by_name("Blinking Light").unwrap();
    for _ in 0..45 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Default::default());
    }
    let errors: Vec<_> = rt.take_logs();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(scene.transform(bobber).position.y > 1.0, "rising");
    assert!(
        scene.world.get::<&Light>(lamp).unwrap().intensity < 1.0,
        "off"
    );
}
