use std::borrow::Cow;
use std::collections::HashSet;

use misty_engine::profiler::{Category, FrameData, HierarchyTree, Sample};
use misty_engine::render::RenderStats;

use super::details::{memory_rows, rendering_rows, visible_rows};
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
fn profiler_opens_beside_the_scene_view() {
    let mut dock = layout::default_layout();
    layout::focus_or_open(&mut dock, Tab::Profiler);
    let profiler = dock.find_tab(&Tab::Profiler).expect("opened");
    assert_eq!(profiler.node, dock.find_tab(&Tab::Scene).unwrap().node);
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
