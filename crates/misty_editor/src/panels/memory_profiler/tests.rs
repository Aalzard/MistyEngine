//! Memory Profiler window: capture without a GPU, view switching, table sorting, the tree map
//! layout and the snapshot comparison.

use egui::{Rect, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::memory_snapshot::{MemCategory, MemItem, MemorySnapshot};

use super::treemap::squarify;
use super::*;
use crate::layout::{self, Tab};
use crate::project::ProjectBrowser;

fn state() -> EditorState {
    let root = std::env::temp_dir().join(format!("misty_memprof_{}", std::process::id()));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    EditorState::new(ProjectBrowser::open(root))
}

fn snap(label: &str, items: &[(MemCategory, &str, u64, u32)]) -> MemorySnapshot {
    MemorySnapshot {
        label: label.into(),
        items: items
            .iter()
            .map(|(c, n, b, r)| MemItem::new(*c, "T", *n, *b).with_refs(*r))
            .collect(),
        working_set: Some(200 << 20),
    }
}

#[test]
fn squarified_rects_fill_the_area_in_proportion() {
    let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(600.0, 400.0));
    let sizes = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
    let rects = squarify(&sizes, area);
    let total: f64 = sizes.iter().sum();
    let mut covered = 0.0;
    for (s, r) in sizes.iter().zip(&rects) {
        let a = (r.width() * r.height()) as f64;
        let expected = s / total * 240_000.0;
        assert!((a - expected).abs() < 1.0, "{s}: {a} vs {expected}");
        assert!(area.expand(0.01).contains_rect(*r));
        covered += a;
        // Squarified: no sliver worse than 1:4 for this input.
        let aspect = (r.width() / r.height()).max(r.height() / r.width());
        assert!(aspect < 4.0, "{r:?}");
    }
    assert!((covered - 240_000.0).abs() < 2.0);
    // No overlaps.
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            let o = rects[i].intersect(rects[j]);
            assert!(o.width() <= 0.01 || o.height() <= 0.01, "{i} {j}");
        }
    }
    assert!(squarify(&[0.0, 0.0], area).iter().all(|r| !r.is_positive()));
}

#[test]
fn table_sorts_by_each_column_and_filters() {
    let s = snap(
        "A",
        &[
            (MemCategory::Textures, "grass.png", 4000, 2),
            (MemCategory::Meshes, "tree", 9000, 1),
            (MemCategory::Audio, "beep.wav", 100, 5),
        ],
    );
    assert_eq!(sorted(&s, Column::Size, false, ""), vec![1, 0, 2]);
    assert_eq!(sorted(&s, Column::Size, true, ""), vec![2, 0, 1]);
    assert_eq!(sorted(&s, Column::Name, true, ""), vec![2, 0, 1]);
    assert_eq!(sorted(&s, Column::Refs, false, ""), vec![2, 0, 1]);
    assert_eq!(sorted(&s, Column::Category, true, ""), vec![0, 1, 2]);
    assert_eq!(sorted(&s, Column::Size, false, "GRASS"), vec![0]);
    // GPU before CPU (audio is system memory).
    assert_eq!(sorted(&s, Column::Location, true, "")[2], 2);
}

fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1000.0, 560.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s, None), s);
    h.run();
    h
}

#[test]
fn capture_compare_and_switch_views() {
    let mut h = window(state());
    h.get_by_label("Press Capture to take a snapshot of the editor's memory.");
    h.get_by_label("⏺ Capture").click();
    h.run();
    {
        let w = &h.state().memory_profiler;
        assert_eq!(w.snapshots.len(), 1);
        let first = &w.snapshots[0];
        // Without a GPU: the scene's components.
        assert!(first.category_total(MemCategory::SceneObjects) > 0);
        assert_eq!(first.label, "Snapshot 1");
    }
    h.get_by_label("Scene Objects");
    // A second snapshot after adding objects compares against the first.
    for i in 0..50 {
        h.state_mut().scene.spawn(&format!("Extra {i}"), None);
    }
    h.get_by_label("⏺ Capture").click();
    h.run();
    assert_eq!(h.state().memory_profiler.baseline, Some(0));
    h.get_by_label("Compare").click();
    h.run();
    h.get_by_label("Largest changes");
    let w = &h.state().memory_profiler;
    let rows = misty_engine::memory_snapshot::diff(&w.snapshots[0], &w.snapshots[1]);
    let go = rows.iter().find(|r| r.name == "GameObject").unwrap();
    assert_eq!(
        go.delta(),
        50 * std::mem::size_of::<misty_engine::scene::GameObject>() as i64
    );
    h.get_by_label("Table").click();
    h.run();
    h.get_by_label("Size ⏷");
    h.get_by_label("Tree Map").click();
    h.run();
    assert_eq!(h.state().memory_profiler.view, View::TreeMap);
}

#[test]
fn memory_profiler_opens_beside_the_project_window() {
    let mut dock = layout::default_layout();
    layout::focus_or_open(&mut dock, Tab::MemoryProfiler);
    let mp = dock.find_tab(&Tab::MemoryProfiler).expect("opened");
    assert_eq!(mp.node, dock.find_tab(&Tab::Project).unwrap().node);
    assert_eq!(Tab::MemoryProfiler.title(), "Memory Profiler");
}
