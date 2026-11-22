//! Timings of whole-scene operations on the sample project's big scenes (round 72's measure;
//! run with `MISTY_BENCH_PROJECT=<a copy of SampleProject>` and `--ignored --nocapture`).

use std::path::PathBuf;
use std::time::Instant;

use super::Scene;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

#[test]
#[ignore = "bench: needs MISTY_BENCH_PROJECT"]
fn whole_scene_operations_bench() {
    let Some(root) = std::env::var_os("MISTY_BENCH_PROJECT").map(PathBuf::from) else {
        return;
    };
    let scenes = std::env::var("MISTY_BENCH_SCENES")
        .unwrap_or_else(|_| "NoonValley,MeadowDemo,StressDemo".to_owned());
    for name in scenes.split(',') {
        let path = root.join(format!("Assets/Scenes/{name}.scene"));
        let (mut load, mut clone, mut json, mut drop_ms) = (vec![], vec![], vec![], vec![]);
        for _ in 0..7 {
            let t = Instant::now();
            let (scene, _) = Scene::load(&path).expect("scene loads");
            load.push(t.elapsed().as_secs_f64() * 1000.0);
            let t = Instant::now();
            let copy = scene.clone();
            clone.push(t.elapsed().as_secs_f64() * 1000.0);
            let t = Instant::now();
            let text = scene.scene_to_json(scene.active_scene());
            json.push(t.elapsed().as_secs_f64() * 1000.0);
            assert!(!text.is_empty());
            let t = Instant::now();
            drop(copy);
            drop_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        eprintln!(
            "{name}: load {:.2} ms, clone {:.2} ms, scene_to_json {:.2} ms, drop {:.2} ms (medians of 7)",
            median(load),
            median(clone),
            median(json),
            median(drop_ms)
        );
    }
}

/// One operation in a loop, for `perf stat` (MISTY_BENCH_OP = load | clone | save).
#[test]
#[ignore = "bench: needs MISTY_BENCH_PROJECT"]
fn one_scene_operation_loop() {
    let Some(root) = std::env::var_os("MISTY_BENCH_PROJECT").map(PathBuf::from) else {
        return;
    };
    let name = std::env::var("MISTY_BENCH_SCENES").unwrap_or_else(|_| "StressDemo".to_owned());
    let op = std::env::var("MISTY_BENCH_OP").unwrap_or_else(|_| "load".to_owned());
    let path = root.join(format!("Assets/Scenes/{name}.scene"));
    let (scene, _) = Scene::load(&path).expect("scene loads");
    let mut times = vec![];
    for _ in 0..20 {
        let t = Instant::now();
        match op.as_str() {
            "load" => drop(Scene::load(&path).expect("scene loads")),
            "clone" => drop(scene.clone()),
            _ => assert!(!scene.scene_to_json(scene.active_scene()).is_empty()),
        }
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    eprintln!("{name} {op}: {:.2} ms (median of 20)", median(times));
}

/// Play-mode frames (scripts, physics, audio, particles; no rendering) on a sample scene, for
/// profiling the per-frame CPU cost (MISTY_BENCH_SCENES = one scene, MISTY_BENCH_FRAMES).
#[test]
#[ignore = "bench: needs MISTY_BENCH_PROJECT"]
fn play_frames_loop() {
    let Some(root) = std::env::var_os("MISTY_BENCH_PROJECT").map(PathBuf::from) else {
        return;
    };
    let name = std::env::var("MISTY_BENCH_SCENES").unwrap_or_else(|_| "StressDemo".to_owned());
    let frames: usize = std::env::var("MISTY_BENCH_FRAMES")
        .ok()
        .and_then(|f| f.parse().ok())
        .unwrap_or(600);
    let path = root.join(format!("Assets/Scenes/{name}.scene"));
    let _db = crate::asset_db::open(&root);
    let (mut scene, _) = Scene::load(&path).expect("scene loads");
    let settings = crate::settings::ProjectSettings::load(&root).unwrap_or_default();
    let (audio, _) = crate::audio::Audio::null(root.clone());
    let t = Instant::now();
    let mut rt = crate::runtime::Runtime::start_with_audio(&mut scene, root, &settings, audio);
    let start_ms = t.elapsed().as_secs_f64() * 1000.0;
    let mut time = crate::runtime::Time::new(&settings.time);
    let mut times = vec![];
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        let t = Instant::now();
        rt.update(&mut scene, &time, crate::scripting::Input::default());
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    eprintln!(
        "{name}: start {start_ms:.1} ms, {} objects, frame median {:.3} ms, p90 {:.3} ms, max {:.3} ms",
        scene.len(),
        median(times),
        sorted[sorted.len() * 9 / 10],
        sorted[sorted.len() - 1]
    );
}
