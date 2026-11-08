use std::borrow::Cow;
use std::collections::HashSet;

use misty_engine::profiler::{
    Category, Counter, CounterGroup, FrameData, GpuPass, HierarchyTree, Sample, ThreadKind,
    ThreadTrack,
};
use misty_engine::render::RenderStats;

use super::details::{gpu_views, lanes, memory_rows, module_rows, rendering_rows, visible_rows};
use super::{Module, ProfilerWindow, Source};
use crate::layout::{self, Tab};

fn sample(name: &'static str, depth: u16, start: f32, ms: f32) -> Sample {
    Sample {
        name: Cow::Borrowed(name),
        category: Category::Scripts,
        depth,
        start_ms: start,
        duration_ms: ms,
    }
}

fn frame() -> FrameData {
    let mut f = FrameData::new(
        7,
        vec![
            sample("Scripts.Update", 0, 0.0, 3.0),
            sample("Mover.Update()", 1, 0.0, 1.0),
            sample("Mover.Update()", 1, 1.0, 1.0),
            sample("Render", 0, 3.0, 2.0),
        ],
        8.0,
    );
    f.render = RenderStats {
        batches: 12,
        saved_by_batching: 300,
        ..Default::default()
    };
    f
}

#[test]
fn hierarchy_rows_follow_expansion() {
    let tree = HierarchyTree::build(&frame());
    let mut expanded = HashSet::new();
    let rows = visible_rows(&tree, &expanded);
    let names: Vec<&str> = rows
        .iter()
        .map(|r| tree.nodes[r.node].name.as_str())
        .collect();
    assert_eq!(names, ["Scripts.Update", "Render"], "longest first");
    expanded.insert("Scripts.Update".to_owned());
    let rows = visible_rows(&tree, &expanded);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].depth, 1);
    assert_eq!(rows[1].path, "Scripts.Update/Mover.Update()");
    assert_eq!(tree.nodes[rows[1].node].calls, 2);
}

#[test]
fn counters_list_rendering_and_memory_values() {
    let f = frame();
    let rendering = rendering_rows(&f);
    assert!(rendering.contains(&("Batches (draw calls)", "12".to_owned())));
    assert!(rendering.contains(&("Saved by batching", "300".to_owned())));
    assert_eq!(memory_rows(&f)[0].0, "GameObjects");
}

#[test]
fn profiler_opens_beside_the_project_window() {
    let mut dock = layout::default_layout();
    layout::focus_or_open(&mut dock, Tab::Profiler);
    let profiler = dock.find_tab(&Tab::Profiler).expect("opened");
    assert_eq!(profiler.node, dock.find_tab(&Tab::Project).unwrap().node);
}

#[test]
fn stats_box_shows_batching_and_screen() {
    let render = RenderStats {
        batches: 40,
        saved_by_batching: 4960,
        triangles: 2_500_000,
        ..Default::default()
    };
    let lines = crate::panels::game_stats_lines(1.0 / 50.0, &render, Some(3.2), [1280, 720], None);
    let text = lines.join("\n");
    assert!(text.contains("50.0 FPS"), "{text}");
    assert!(
        text.contains("Batches: 40   Saved by batching: 4960"),
        "{text}"
    );
    assert!(text.contains("Tris: 2500.0k"), "{text}");
    assert!(text.contains("Screen: 1280x720"));
    assert!(text.contains("GPU: 3.2ms"));
}

fn pass(view: &'static str, name: &'static str, start: f32, ms: f32) -> GpuPass {
    GpuPass {
        view: view.into(),
        name: name.into(),
        start_ms: start,
        ms,
    }
}

/// A frame with GPU passes of two renders, a worker thread and module counters.
fn rich_frame() -> FrameData {
    let mut f = frame();
    f.samples.push(sample("Render.Submit", 1, 5.0, 0.5));
    f.gpu_passes = vec![
        pass("Scene", "Shadow maps (cascades, local)", 0.0, 1.0),
        pass("Scene", "Main: opaque meshes", 1.0, 2.0),
        pass("Game", "Shadow maps (cascades, local)", 0.0, 0.5),
        pass("Game", "Bloom, tonemap, finish", 0.5, 0.25),
    ];
    f.threads = vec![ThreadTrack {
        name: "Worker 2".into(),
        kind: ThreadKind::Worker,
        samples: vec![sample("Render.ShadowList", 0, -1.0, 2.0)],
    }];
    f.counters = vec![0.0; Counter::COUNT];
    f.counters[Counter::PhysicsBodies.index()] = 42.0;
    f.counters[Counter::AudioDspLoad.index()] = 1.5;
    f.counters[Counter::UiElements.index()] = 17.0;
    f
}

#[test]
fn gpu_hierarchy_groups_passes_by_render() {
    let f = rich_frame();
    let views = gpu_views(&f);
    assert_eq!(views.len(), 2);
    assert_eq!(views[0].0, "Scene");
    assert_eq!(views[0].1.len(), 2);
    assert_eq!(views[1].0, "Game");
    assert_eq!(f.gpu_total_ms(), Some(3.75));
}

#[test]
fn timeline_has_a_lane_per_thread_and_render() {
    let f = rich_frame();
    let lanes = lanes(&f);
    let names: Vec<&str> = lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Main Thread",
            "Render (GPU) · Scene",
            "Render (GPU) · Game",
            "Worker 2"
        ]
    );
    assert_eq!(lanes[0].rows(), 2, "main thread nesting");
    // The first render's GPU work starts after its submit (5.0 + 0.5 ms).
    assert_eq!(lanes[1].samples[0].start_ms, 5.5);
    // A worker scope that began in an earlier frame is clipped to the frame's start.
    let w = &lanes[3].samples[0];
    assert_eq!((w.start_ms, w.ms), (0.0, 1.0));
}

#[test]
fn module_counters_show_times_then_values() {
    let mut f = rich_frame();
    f.samples.push(sample("Physics.Step", 0, 6.0, 0.75));
    let physics = module_rows(&f, CounterGroup::Physics);
    assert_eq!(physics[0], ("Physics time", "0.750 ms".to_owned()));
    assert!(physics.contains(&("Rigidbodies", "42".to_owned())));
    let audio = module_rows(&f, CounterGroup::Audio);
    assert!(audio.contains(&("DSP load (% of one core)", "1.50 %".to_owned())));
    let ui = module_rows(&f, CounterGroup::Ui);
    assert!(ui.contains(&("Elements", "17".to_owned())));
}

#[test]
fn modules_are_found_by_short_names() {
    assert_eq!(Module::named("GPU"), Some(Module::Gpu));
    assert_eq!(Module::named("cpu usage"), Some(Module::Cpu));
    assert_eq!(Module::named("UI"), Some(Module::Ui));
    assert_eq!(Module::named("Physics"), Some(Module::Physics));
    assert_eq!(Module::named("nope"), None);
}

#[test]
fn saved_frames_load_back_as_a_file_source() {
    let dir = std::env::temp_dir().join(format!("misty_profiler_panel_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("capture.mprof");
    let frames = [rich_frame(), rich_frame()];
    misty_engine::profiler::capture::save(&path, frames.iter()).unwrap();
    let mut w = ProfilerWindow::default();
    w.load(path.clone()).unwrap();
    assert!(matches!(w.source, Source::File { .. }));
    assert_eq!(w.source.with_frames(|f| f.len()), 2);
    let again = dir.join("again.mprof");
    assert_eq!(w.save(&again).unwrap(), 2);
    let back = misty_engine::profiler::capture::load(&again).unwrap();
    assert_eq!(back[1].gpu_passes.len(), 4);
    assert_eq!(back[0].counter(Counter::PhysicsBodies), 42.0);
    let _ = std::fs::remove_dir_all(dir);
}
