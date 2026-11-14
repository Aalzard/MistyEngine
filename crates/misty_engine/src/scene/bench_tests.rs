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
