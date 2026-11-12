use glam::{Mat4, Vec3};

use super::component::{Coverage, TimelineEntry, TimelineMode, Weather, WeatherTimeline};
use super::lightning::{self, Bolt, Lightning, thunder_delay};
use super::state::{VolumeState, accumulate, area_weight};
use super::*;
use crate::scene::{Scene, Transform};

fn approx(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}

#[test]
fn presets_parse_label_and_round_trip() {
    for p in WeatherPreset::ALL {
        assert_eq!(WeatherPreset::parse(p.key()), Some(p), "{p:?}");
        assert_eq!(WeatherPreset::parse(p.label()), Some(p), "{p:?}");
        assert_eq!(WeatherPreset::matching(&p.params()), Some(p), "{p:?}");
    }
    assert_eq!(
        WeatherPreset::parse("heavy_rain"),
        Some(WeatherPreset::HeavyRain)
    );
    assert_eq!(
        WeatherPreset::parse("Storm"),
        Some(WeatherPreset::Thunderstorm)
    );
    assert_eq!(WeatherPreset::parse("nope"), None);
    // Presets keep the wind direction.
    let mut p = WeatherPreset::Rain.params();
    p.wind_direction = 200.0;
    WeatherPreset::Blizzard.apply(&mut p);
    assert_eq!(p.wind_direction, 200.0);
    assert_eq!(WeatherPreset::matching(&p), Some(WeatherPreset::Blizzard));
    p.rain_density = 3.0;
    assert_eq!(WeatherPreset::matching(&p), None);
    // Old files and partial values load with defaults.
    let w: Weather = serde_json::from_str("{}").unwrap();
    assert_eq!(w, Weather::default());
    let q: WeatherParams = serde_json::from_str(r#"{ "rain_density": 5.0 }"#).unwrap();
    assert_eq!(q.rain_density, 5.0);
    assert_eq!(q.visibility, WeatherParams::default().visibility);
}

#[test]
fn transitions_interpolate_every_value() {
    let a = WeatherPreset::Clear.params();
    let b = WeatherPreset::Downpour.params();
    assert_eq!(a.lerp(&b, 0.0), a);
    assert_eq!(a.lerp(&b, 1.0), b);
    let m = a.lerp(&b, 0.5);
    assert!(approx(m.rain_density, b.rain_density * 0.5, 1e-4));
    assert!(approx(m.rain_intensity, 0.5, 1e-4));
    // Clear has no rain: the drops keep the downpour's look while they thin out.
    assert_eq!(m.drop_size, b.drop_size);
    assert_eq!(m.rain_speed, b.rain_speed);
    // Visibility blends by extinction: halfway is the harmonic mean, much closer to the fog.
    let v = 1.0 / (0.5 / a.visibility + 0.5 / b.visibility);
    assert!(approx(m.visibility, v, 1.0), "{} vs {v}", m.visibility);
    assert!(m.visibility < 1000.0);
    // Wind direction takes the short way round.
    let mut w1 = a;
    let mut w2 = a;
    w1.wind_direction = 350.0;
    w2.wind_direction = 10.0;
    assert!(
        approx(w1.lerp(&w2, 0.5).wind_direction, 0.0, 1e-3)
            || approx(w1.lerp(&w2, 0.5).wind_direction, 360.0, 1e-3)
    );
    // Rain → snow passes through sleet: both falling halfway.
    let s = WeatherPreset::Rain
        .params()
        .lerp(&WeatherPreset::Snow.params(), 0.5);
    assert!(s.rain_density > 0.0 && s.snow_density > 0.0);
    // Monotonic along the way.
    let mut last = -1.0;
    for i in 0..=10 {
        let d = a.lerp(&b, i as f32 / 10.0).rain_density;
        assert!(d >= last);
        last = d;
    }
}

fn global(w: Weather) -> Vec<(hecs::Entity, Weather, Mat4)> {
    let mut world = hecs::World::new();
    let e = world.spawn(());
    vec![(e, w, Mat4::IDENTITY)]
}

#[test]
fn timeline_steps_through_its_entries_with_transitions() {
    let w = Weather {
        timeline: WeatherTimeline {
            mode: TimelineMode::Sequence,
            entries: vec![
                TimelineEntry::new(WeatherPreset::Clear, 10.0, 0.0),
                TimelineEntry::new(WeatherPreset::HeavyRain, 5.0, 4.0),
                TimelineEntry::new(WeatherPreset::Snow, 5.0, 2.0),
            ],
            looping: true,
            time_scale: 1.0,
        },
        ..Default::default()
    };
    let vols = global(w.clone());
    let e = vols[0].0;
    let mut world = WeatherWorld::default();
    world.update(&vols, 0.0, Vec3::ZERO);
    assert_eq!(
        world.volume(e).unwrap().params,
        WeatherPreset::Clear.params()
    );
    // Hold Clear for 10 s.
    for _ in 0..99 {
        world.update(&vols, 0.1, Vec3::ZERO);
    }
    assert_eq!(world.volume(e).unwrap().timeline.index, 0);
    let changed: Vec<_> = world.take_events();
    assert!(changed.is_empty(), "{changed:?}");
    world.update(&vols, 0.15, Vec3::ZERO);
    let st = world.volume(e).unwrap();
    assert_eq!(st.timeline.index, 1);
    assert!(
        world
            .take_events()
            .iter()
            .any(|e| matches!(e, WeatherEvent::Changed { state, transition } if state == "HeavyRain" && *transition == 4.0))
    );
    // Halfway through the 4 s blend: between the two (smoothstep(0.5) = 0.5).
    for _ in 0..20 {
        world.update(&vols, 0.1, Vec3::ZERO);
    }
    let p = world.volume(e).unwrap().params;
    let heavy = WeatherPreset::HeavyRain.params();
    assert!(
        approx(p.rain_density, heavy.rain_density * 0.5, 0.5),
        "{}",
        p.rain_density
    );
    // Blend done, then the 5 s hold, then Snow, then back to Clear (looping).
    for _ in 0..20 {
        world.update(&vols, 0.1, Vec3::ZERO);
    }
    assert_eq!(world.volume(e).unwrap().params, heavy);
    for _ in 0..55 {
        world.update(&vols, 0.1, Vec3::ZERO);
    }
    assert_eq!(world.volume(e).unwrap().timeline.index, 2);
    for _ in 0..80 {
        world.update(&vols, 0.1, Vec3::ZERO);
    }
    assert_eq!(world.volume(e).unwrap().timeline.index, 0, "looped");
    // Not looping: stays on the last entry.
    let mut w2 = w.clone();
    w2.timeline.looping = false;
    let vols2 = global(w2);
    let mut world2 = WeatherWorld::default();
    for _ in 0..1000 {
        world2.update(&vols2, 0.1, Vec3::ZERO);
    }
    let e2 = vols2[0].0;
    assert_eq!(world2.volume(e2).unwrap().timeline.index, 2);
    assert_eq!(
        world2.volume(e2).unwrap().params,
        WeatherPreset::Snow.params()
    );
}

#[test]
fn random_timeline_follows_weights_and_never_repeats() {
    let mut w = Weather {
        timeline: WeatherTimeline {
            mode: TimelineMode::Random,
            entries: vec![
                TimelineEntry {
                    weight: 1.0,
                    ..TimelineEntry::new(WeatherPreset::Clear, 1.0, 0.0)
                },
                TimelineEntry {
                    weight: 3.0,
                    ..TimelineEntry::new(WeatherPreset::Rain, 1.0, 0.0)
                },
                TimelineEntry {
                    weight: 0.0,
                    ..TimelineEntry::new(WeatherPreset::Blizzard, 1.0, 0.0)
                },
            ],
            looping: true,
            time_scale: 1.0,
        },
        ..Default::default()
    };
    for e in &mut w.timeline.entries {
        e.max_duration = 2.0;
    }
    let vols = global(w);
    let e = vols[0].0;
    let mut world = WeatherWorld::default();
    let mut last = 0;
    let mut counts = [0usize; 3];
    for _ in 0..4000 {
        world.update(&vols, 0.25, Vec3::ZERO);
        let i = world.volume(e).unwrap().timeline.index;
        if i != last {
            counts[i] += 1;
            last = i;
        }
    }
    assert_eq!(counts[2], 0, "zero weight never picked");
    assert!(counts[0] > 50 && counts[1] > 50, "{counts:?}");
    // Holds are random within min..max: the switches happen every 1–2 s (4000 × 0.25 s).
    let switches = counts[0] + counts[1];
    assert!((450..=1050).contains(&switches), "{switches}");
}

#[test]
fn area_volumes_fade_at_their_edges_and_blend_by_priority() {
    let m = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
    let size = [20.0, 10.0, 20.0];
    // Box: inside, inside the blend band, outside.
    assert_eq!(
        area_weight(
            Coverage::Box,
            &m,
            size,
            0.0,
            4.0,
            Vec3::new(100.0, 0.0, 0.0)
        ),
        1.0
    );
    let edge = area_weight(
        Coverage::Box,
        &m,
        size,
        0.0,
        4.0,
        Vec3::new(108.0, 0.0, 0.0),
    );
    assert!(edge > 0.0 && edge < 1.0, "{edge}");
    assert_eq!(
        area_weight(
            Coverage::Box,
            &m,
            size,
            0.0,
            4.0,
            Vec3::new(111.0, 0.0, 0.0)
        ),
        0.0
    );
    // Scale and rotation count.
    let scaled = Mat4::from_scale_rotation_translation(
        Vec3::splat(2.0),
        glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        Vec3::ZERO,
    );
    assert_eq!(
        area_weight(
            Coverage::Box,
            &scaled,
            [10.0, 2.0, 2.0],
            0.0,
            0.0,
            Vec3::new(0.0, 0.0, 9.0)
        ),
        1.0
    );
    assert_eq!(
        area_weight(
            Coverage::Box,
            &scaled,
            [10.0, 2.0, 2.0],
            0.0,
            0.0,
            Vec3::new(9.0, 0.0, 0.0)
        ),
        0.0
    );
    // Sphere.
    assert_eq!(
        area_weight(
            Coverage::Sphere,
            &m,
            size,
            10.0,
            2.0,
            Vec3::new(100.0, 5.0, 0.0)
        ),
        1.0
    );
    assert_eq!(
        area_weight(
            Coverage::Sphere,
            &m,
            size,
            10.0,
            2.0,
            Vec3::new(100.0, 11.0, 0.0)
        ),
        0.0
    );

    // A global drizzle with a storm cell (higher priority) over x = 100.
    let mut world_e = hecs::World::new();
    let (g, a) = (world_e.spawn(()), world_e.spawn(()));
    let drizzle = Weather::with_preset(WeatherPreset::Drizzle);
    let storm = Weather {
        coverage: Coverage::Box,
        size,
        blend_distance: 4.0,
        priority: 1.0,
        ..Weather::with_preset(WeatherPreset::Downpour)
    };
    let vols = vec![(g, drizzle, Mat4::IDENTITY), (a, storm, m)];
    let mut world = WeatherWorld::default();
    world.update(&vols, 0.1, Vec3::ZERO);
    assert!(approx(
        world.current.rain_density,
        WeatherPreset::Drizzle.params().rain_density,
        1e-3
    ));
    world.update(&vols, 0.1, Vec3::new(100.0, 0.0, 0.0));
    assert!(approx(
        world.current.rain_density,
        WeatherPreset::Downpour.params().rain_density,
        1e-3
    ));
    assert_eq!(world.dominant, Some(a));
    world.update(&vols, 0.1, Vec3::new(108.0, 0.0, 0.0));
    let r = world.current.rain_density;
    assert!(r > 10.0 && r < 50.0, "{r}");
    // The renderer gets the storm as a region and the drizzle as the global amount.
    let f = frame::build(&world).unwrap();
    assert_eq!(f.regions.len(), 1);
    assert!(approx(
        f.global.rain,
        WeatherPreset::Drizzle.params().rain_density,
        1e-3
    ));
    assert!(approx(
        f.amounts_at(Vec3::new(100.0, 0.0, 0.0)).rain,
        50.0,
        1e-3
    ));
    assert!(approx(
        f.amounts_at(Vec3::new(-50.0, 0.0, 0.0)).rain,
        f.global.rain,
        1e-3
    ));
    assert_eq!(f.max_rain, 50.0);
    // Lower priority than the global: the global wins inside the area too.
    let mut low = vols.clone();
    low[1].1.priority = -1.0;
    low.sort_by(|a, b| a.1.priority.total_cmp(&b.1.priority));
    let mut w2 = WeatherWorld::default();
    w2.update(&low, 0.1, Vec3::new(100.0, 0.0, 0.0));
    assert!(approx(
        w2.current.rain_density,
        WeatherPreset::Drizzle.params().rain_density,
        1e-3
    ));
}

#[test]
fn surfaces_get_wet_dry_and_snow_builds_and_melts() {
    let set = SurfaceSettings {
        wet_time: 10.0,
        dry_time: 20.0,
        snow_time: 10.0,
        melt_time: 20.0,
        ..Default::default()
    };
    let mut s = VolumeState::default();
    let rain = WeatherPreset::Downpour.params();
    for _ in 0..50 {
        accumulate(&mut s, &rain, &set, 0.1);
    }
    assert!(approx(s.wetness, 0.5, 0.01), "{}", s.wetness);
    for _ in 0..60 {
        accumulate(&mut s, &rain, &set, 0.1);
    }
    assert_eq!(s.wetness, 1.0);
    let puddles = s.puddles;
    assert!(
        puddles > 0.3 && puddles < 1.0,
        "puddles fill slower: {puddles}"
    );
    // Dries after the rain: 20 s from fully wet; puddles linger.
    let clear = WeatherPreset::Clear.params();
    for _ in 0..100 {
        accumulate(&mut s, &clear, &set, 0.1);
    }
    assert!(approx(s.wetness, 0.5, 0.01), "{}", s.wetness);
    assert!(
        s.puddles > 0.0 && puddles - s.puddles < 0.45,
        "puddles dry slower: {}",
        s.puddles
    );
    for _ in 0..400 {
        accumulate(&mut s, &clear, &set, 0.1);
    }
    assert_eq!(s.wetness, 0.0);
    assert_eq!(s.puddles, 0.0);
    // Light rain wets slower than a downpour.
    let mut light = VolumeState::default();
    accumulate(&mut light, &WeatherPreset::Drizzle.params(), &set, 1.0);
    let mut hard = VolumeState::default();
    accumulate(&mut hard, &rain, &set, 1.0);
    assert!(light.wetness < hard.wetness);
    // Snow builds up, then melts (and wets the ground as it does).
    let snow = WeatherPreset::Blizzard.params();
    for _ in 0..50 {
        accumulate(&mut s, &snow, &set, 0.1);
    }
    assert!(approx(s.snow_cover, 0.5, 0.01), "{}", s.snow_cover);
    for _ in 0..100 {
        accumulate(&mut s, &snow, &set, 0.1);
    }
    assert_eq!(s.snow_cover, 1.0);
    assert_eq!(s.wetness, 0.0);
    for _ in 0..50 {
        accumulate(&mut s, &clear, &set, 0.1);
    }
    assert!(approx(s.snow_cover, 0.75, 0.01), "{}", s.snow_cover);
    assert!(s.wetness > 0.1);
    // Rain melts it three times as fast.
    let mut r = VolumeState::default();
    r.snow_cover = 1.0;
    accumulate(&mut r, &rain, &set, 1.0);
    assert!(
        approx(r.snow_cover, 1.0 - 3.0 / 20.0, 0.02),
        "{}",
        r.snow_cover
    );
    // Max snow caps it; turning surfaces off clears them.
    let capped = SurfaceSettings {
        max_snow: 0.4,
        ..set
    };
    for _ in 0..200 {
        accumulate(&mut s, &snow, &capped, 0.1);
    }
    assert_eq!(s.snow_cover, 0.4);
    let off = SurfaceSettings {
        wetness: false,
        snow_cover: false,
        ..set
    };
    accumulate(&mut s, &snow, &off, 0.1);
    assert_eq!((s.wetness, s.snow_cover), (0.0, 0.0));
}

#[test]
fn thunder_arrives_after_the_flash_by_distance() {
    assert!(approx(thunder_delay(343.0), 1.0, 1e-6));
    assert!(approx(thunder_delay(3430.0), 10.0, 1e-5));
    let mut l = Lightning::default();
    l.seed(7);
    let cam = Vec3::new(5.0, 2.0, 5.0);
    let (_, d, _) = l.strike_at(Vec3::new(5.0, 2.0, 1720.0), 1715.0, 1.0, true, cam);
    assert_eq!(d, 1715.0);
    assert!(l.strikes[0].bolt.is_some(), "near strikes have a bolt");
    let (b, _) = l.flash(cam);
    assert!(b > 0.0);
    // 5 s of 1/60 s steps: thunder after exactly 1715 / 343 = 5 s.
    let mut heard_at = None;
    for i in 1..=400 {
        let ev = l.update(1.0 / 60.0, 0.0, 400.0, 600.0, 1.0, cam, true);
        if !ev.thunder.is_empty() && heard_at.is_none() {
            heard_at = Some(i as f32 / 60.0);
        }
    }
    let t = heard_at.expect("thunder");
    assert!(approx(t, 5.0, 1.0 / 30.0), "{t}");
    // The flash is long over.
    assert!(l.strikes.is_empty());
    // Far strikes have no bolt.
    l.strike_at(Vec3::ZERO, lightning::BOLT_RANGE + 1.0, 1.0, true, cam);
    assert!(l.strikes[0].bolt.is_none());
}

#[test]
fn lightning_rate_and_bolts() {
    let mut l = Lightning::default();
    l.seed(3);
    let mut strikes = 0;
    // 10 minutes at 6 per minute.
    for _ in 0..36000 {
        strikes += l
            .update(1.0 / 60.0, 6.0, 500.0, 5000.0, 1.0, Vec3::ZERO, true)
            .strikes
            .len();
    }
    assert!((40..=85).contains(&strikes), "{strikes}");
    // Same seed, same bolt; it reaches from the cloud to the ground and branches.
    let a = Bolt::generate(9, Vec3::new(0.0, 900.0, 0.0), Vec3::ZERO);
    let b = Bolt::generate(9, Vec3::new(0.0, 900.0, 0.0), Vec3::ZERO);
    assert_eq!(a, b);
    assert!(a.segments.len() > 128 && a.segments.len() <= lightning::MAX_SEGMENTS);
    let main: Vec<_> = a.segments.iter().filter(|s| s.brightness == 1.0).collect();
    assert_eq!(main.first().unwrap().a, Vec3::new(0.0, 900.0, 0.0));
    assert_eq!(main.last().unwrap().b, Vec3::ZERO);
    assert!(a.segments.iter().any(|s| s.brightness < 1.0), "branches");
}

#[test]
fn script_layer_overrides_and_releases() {
    let vols = global(Weather::with_preset(WeatherPreset::Snow));
    let mut world = WeatherWorld::default();
    world.update(&vols, 0.1, Vec3::ZERO);
    assert_eq!(
        world.current.snow_density,
        WeatherPreset::Snow.params().snow_density
    );
    world.set_state(WeatherPreset::HeavyRain.params(), 2.0, "HeavyRain");
    assert!(world.scripted());
    for _ in 0..10 {
        world.update(&vols, 0.1, Vec3::ZERO);
    }
    // Halfway (1 s of 2).
    let r = world.current.rain_density;
    assert!(
        approx(r, WeatherPreset::HeavyRain.params().rain_density * 0.5, 0.5),
        "{r}"
    );
    for _ in 0..20 {
        world.update(&vols, 0.1, Vec3::ZERO);
    }
    assert!(approx(
        world.current.rain_density,
        WeatherPreset::HeavyRain.params().rain_density,
        1e-3
    ));
    assert!(approx(world.current.snow_density, 0.0, 1e-4));
    // Changing one value keeps the rest.
    world.modify(0.0, |p| p.wind_speed = 20.0);
    world.update(&vols, 0.1, Vec3::ZERO);
    assert!(approx(world.current.wind_speed, 20.0, 1e-3));
    assert!(world.current.rain_density > 30.0);
    // Release: back to the volume's snow over 1 s.
    world.release(1.0);
    for _ in 0..12 {
        world.update(&vols, 0.1, Vec3::ZERO);
    }
    assert!(!world.scripted());
    assert_eq!(
        world.current.snow_density,
        WeatherPreset::Snow.params().snow_density
    );
    // Without any volume, scripts still make weather (the implicit clear base).
    let mut bare = WeatherWorld::default();
    bare.set_state(WeatherPreset::Rain.params(), 0.0, "Rain");
    bare.update(&[], 0.1, Vec3::ZERO);
    assert_eq!(
        bare.current.rain_density,
        WeatherPreset::Rain.params().rain_density
    );
    assert!(frame::build(&bare).is_some());
    let ev = bare.take_events();
    assert!(matches!(&ev[0], WeatherEvent::Changed { state, .. } if state == "Rain"));
}

#[test]
fn wind_drifts_and_gusts_change_the_slant() {
    let mut w = Weather::with_preset(WeatherPreset::Thunderstorm);
    w.state.wind_direction = 90.0;
    let vols = global(w);
    let mut world = WeatherWorld::default();
    let mut speeds = Vec::new();
    for _ in 0..600 {
        world.update(&vols, 0.1, Vec3::ZERO);
        speeds.push(world.wind.length());
        // Blows towards +X (90° clockwise from +Z), give or take the turbulence.
        assert!(world.wind.x > 0.0);
    }
    let (lo, hi) = speeds
        .iter()
        .fold((f32::MAX, 0.0f32), |(a, b), &s| (a.min(s), b.max(s)));
    assert!(hi - lo > 2.0, "gusts vary the wind: {lo}..{hi}");
    // The drift accumulates (wrapped).
    assert!(world.wind_offset.x > 0.0 && world.wind_offset.x < state::WRAP);
}

#[test]
fn scene_update_uses_volumes_and_the_frame_follows() {
    let mut s = Scene::default_3d("W");
    assert!(frame::scene_frame(&s).is_none());
    let e = s.spawn("Weather", None);
    s.add(e, Weather::with_preset(WeatherPreset::HeavySnow));
    // Not running yet: nothing drawn.
    assert!(frame::scene_frame(&s).is_none());
    update_scene(&mut s, 0.1, Vec3::ZERO);
    let f = frame::scene_frame(&s).expect("live");
    assert_eq!(f.max_snow, WeatherPreset::HeavySnow.params().snow_density);
    assert!(f.max_rain == 0.0);
    // A disabled volume falls back to clear.
    s.world.get::<&mut Weather>(e).unwrap().enabled = false;
    update_scene(&mut s, 0.1, Vec3::ZERO);
    assert_eq!(s.weather.current.snow_density, 0.0);
    // An area volume away from the camera: still a region (it can be seen from outside).
    s.world.get::<&mut Weather>(e).unwrap().enabled = true;
    s.world.get::<&mut Weather>(e).unwrap().coverage = Coverage::Sphere;
    s.set_transform(e, Transform::from_position(Vec3::new(60.0, 0.0, 0.0)));
    update_scene(&mut s, 0.1, Vec3::ZERO);
    let f = frame::scene_frame(&s).unwrap();
    assert_eq!(f.regions.len(), 1);
    assert_eq!(s.weather.current.snow_density, 0.0);
    // Drive Clouds turns overcast clouds on in a scene without them.
    let mut post = crate::render::PostSettings::default();
    s.weather
        .set_state(WeatherPreset::Thunderstorm.params(), 0.0, "Thunderstorm");
    update_scene(&mut s, 0.1, Vec3::ZERO);
    frame::adjust_post(&mut post, &s);
    assert!(post.clouds.is_on() && post.clouds.coverage > 0.8);
    s.weather.stop();
    assert!(frame::scene_frame(&s).is_none());
}

#[test]
fn generated_wind_and_thunder_are_sane() {
    let w = audio::wind(2.0, 1);
    assert!(w.len() > (audio::RATE as f32 * 1.2) as usize);
    let peak = w
        .iter()
        .fold(0.0f32, |m, (a, b)| m.max(a.abs()).max(b.abs()));
    assert!(peak > 0.5 && peak <= 0.71, "{peak}");
    let near = audio::thunder(true, 2);
    let far = audio::thunder(false, 2);
    // A near strike cracks at once; far thunder rolls in slowly.
    let early = |v: &[(f32, f32)]| {
        v[..audio::RATE as usize / 10]
            .iter()
            .map(|s| s.0.abs())
            .sum::<f32>()
    };
    assert!(early(&near) > early(&far) * 2.0);
}

#[test]
fn rain_volume_follows_intensity_and_drops_under_cover() {
    use audio::rain_volumes;
    let under = super::component::UNDER_COVER;
    // Clear: silent.
    assert_eq!(rain_volumes(0.0, 0.0, under), (0.0, 0.0));
    assert_eq!(rain_volumes(f32::NAN, 0.0, under), (0.0, 0.0));
    // Louder as the rain gets heavier (equal-power sum), the storm loop taking over.
    let power = |(a, b): (f32, f32)| (a * a + b * b).sqrt();
    let mut last = (0.0, 0.0);
    for i in 1..=20 {
        let r = i as f32 / 20.0;
        let v = rain_volumes(r, 0.0, under);
        assert!(power(v) > power(last), "louder at {r}: {v:?} vs {last:?}");
        assert!(v.1 >= last.1, "more storm at {r}");
        last = v;
    }
    let (steady, storm) = rain_volumes(0.3, 0.0, under);
    assert!(steady > 0.5 && storm == 0.0, "light rain: steady only");
    let (steady, storm) = rain_volumes(1.0, 0.0, under);
    assert!(storm > 0.99 && steady < 1e-3, "downpour: storm only");
    // Under a roof: quieter by Under Cover, half covered in between.
    for r in [0.2, 0.6, 1.0] {
        let open = power(rain_volumes(r, 0.0, under));
        let half = power(rain_volumes(r, 0.5, under));
        let roof = power(rain_volumes(r, 1.0, under));
        assert!(roof < half && half < open, "{r}");
        assert!(approx(roof, open * under, 1e-5), "{r}");
        // Rain drums on a roof or a car: still clearly heard under one.
        assert!(roof >= open * 0.75, "{r}: a roof keeps most of the rain");
    }
    // Wind under cover keeps more than the rain.
    let mut w = WeatherWorld::default();
    w.current = WeatherPreset::Blizzard.params();
    w.wind = Vec3::new(20.0, 0.0, 0.0);
    let (_, _, open) = audio::loop_volumes(&w, 0.0);
    let (_, _, roof) = audio::loop_volumes(&w, 1.0);
    assert!(open > 0.8 && roof < open && roof > open * under);
}

#[test]
fn weather_sounds_crossfade_ease_and_stop() {
    let (mut a, log) = crate::audio::Audio::null(std::env::temp_dir());
    let mut d = audio::Driver::default();
    let mut w = WeatherWorld::default();
    w.live = true;
    // Clear: nothing plays.
    w.current = WeatherPreset::Clear.params();
    d.update(&mut a, &mut w, 0.1, 0.0);
    assert!(log.lock().unwrap().played.is_empty());
    // Steady rain fades in (eased), playing the bundled steady loop only.
    w.current = WeatherPreset::Rain.params();
    d.update(&mut a, &mut w, 0.1, 0.0);
    let first = d.levels().0[0];
    for _ in 0..50 {
        d.update(&mut a, &mut w, 0.1, 0.0);
    }
    let settled = d.levels().0;
    assert!(
        first > 0.0 && first < settled[0] * 0.3,
        "{first} {settled:?}"
    );
    let played = log.lock().unwrap().played.clone();
    assert!(
        played.iter().any(|p| p.ends_with("rain_steady_v2.ogg")),
        "{played:?}"
    );
    // A downpour crossfades into the storm loop.
    w.current = WeatherPreset::Downpour.params();
    for _ in 0..50 {
        d.update(&mut a, &mut w, 0.1, 0.0);
    }
    let storm = d.levels().0;
    assert!(storm[1] > storm[0] && storm[1] > settled[0], "{storm:?}");
    assert!(
        log.lock()
            .unwrap()
            .played
            .iter()
            .any(|p| p.ends_with("rain_storm_v2.ogg"))
    );
    // Under a roof: a little quieter (rain drums on it); with Sheltered Areas off the cover is
    // ignored.
    for _ in 0..50 {
        d.update(&mut a, &mut w, 0.1, 1.0);
    }
    let (covered, cover) = d.levels();
    assert!(cover > 0.99 && covered[1] < storm[1] * 0.9, "{covered:?}");
    w.effects.occlusion = false;
    for _ in 0..50 {
        d.update(&mut a, &mut w, 0.1, 1.0);
    }
    assert!(d.levels().1 < 0.01);
    // Sound off: everything stops.
    w.audio.enabled = false;
    d.update(&mut a, &mut w, 0.1, 0.0);
    assert!(log.lock().unwrap().voices.is_empty());
    assert_eq!(d.levels().0, [0.0; 3]);
}

#[test]
fn bundled_rain_loops_decode_seamlessly() {
    use crate::audio::stream::{Pull, StreamInput, StreamReader};
    for bytes in [audio::RAIN_STEADY_OGG, audio::RAIN_STORM_OGG] {
        let bytes: std::sync::Arc<[u8]> = bytes.into();
        // Played looping (Compressed In Memory): the second pass repeats the first exactly,
        // with nothing added or lost at the seam.
        let mut r = StreamReader::open(StreamInput::Memory(bytes.clone()), true, false, 0).unwrap();
        assert_eq!(r.rate, 48_000);
        let len = 30 * 48_000;
        let want = len + 4_800;
        let mut got = Vec::with_capacity(want);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while got.len() < want {
            match r.pop() {
                Pull::Frame(f) => got.push(f),
                Pull::End => panic!("a looping stream ended after {} frames", got.len()),
                Pull::Underrun => {
                    assert!(std::time::Instant::now() < deadline, "decoding stalled");
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
        for k in 0..4_800 {
            let (a, b) = (got[k], got[len + k]);
            assert!(
                (a.left - b.left).abs() < 1e-6 && (a.right - b.right).abs() < 1e-6,
                "frame {k} of the second pass differs: {a:?} vs {b:?}"
            );
        }
        // No click at the seam: the step over it is like the steps around it.
        let step = |i: usize| (got[i + 1].left - got[i].left).abs();
        let around: f32 = (len - 200..len + 200).map(step).sum::<f32>() / 400.0;
        assert!(
            step(len - 1) < around * 8.0 + 1e-3,
            "seam step {}",
            step(len - 1)
        );
        // Decoded whole (Decompress On Load): the same length.
        let data = kira::sound::static_sound::StaticSoundData::from_cursor(std::io::Cursor::new(
            bytes.to_vec(),
        ))
        .unwrap();
        assert_eq!(data.frames.len(), len);
    }
}

#[test]
fn old_weather_audio_loads_and_new_fields_stay_out_of_files() {
    // Saved before Storm Loop and Under Cover existed.
    let old = r#"{"enabled":true,"output":"","rain_clip":"","thunder_clips":[],"volume":0.8,"wind_clip":""}"#;
    let a: WeatherAudio = serde_json::from_str(old).unwrap();
    assert_eq!(a, WeatherAudio::default());
    // Re-saved with the same keys (no new ones at their defaults).
    let keys =
        |v: serde_json::Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    assert_eq!(
        keys(serde_json::to_value(&a).unwrap()),
        keys(serde_json::from_str(old).unwrap())
    );
    // Set, they are written.
    let set = WeatherAudio {
        storm_clip: "Assets/Audio/MyStorm.ogg".into(),
        under_cover: 0.5,
        ..Default::default()
    };
    let v = serde_json::to_value(&set).unwrap();
    assert_eq!(v["storm_clip"], "Assets/Audio/MyStorm.ogg");
    assert_eq!(v["under_cover"], 0.5);
    assert_eq!(serde_json::from_value::<WeatherAudio>(v).unwrap(), set);
}
