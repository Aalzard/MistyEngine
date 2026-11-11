//! Console panel tests: only the rows in view are drawn, a frame costs the same however much
//! was logged, and the counters, search, Collapse and selection still work once old lines are
//! dropped. `console_flood_report` prints the numbers of docs/progress.md.

use std::time::Instant;

use egui::{Event, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::show;
use crate::console::{DEFAULT_CAPACITY, LogEntry, LogLevel};
use crate::project::ProjectBrowser;
use crate::state::EditorState;

fn state(tag: &str) -> EditorState {
    let root = std::env::temp_dir().join(format!("misty_console_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    EditorState::new(ProjectBrowser::open(root))
}

fn cleanup(h: Harness<'static, EditorState>) {
    let _ = std::fs::remove_dir_all(&h.state().project.root);
}

fn harness(s: EditorState) -> Harness<'static, EditorState> {
    Harness::builder()
        .with_size([900.0, 400.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s)
}

fn line(i: usize) -> String {
    format!("Player position 12.5, 3.0, 7.25 (frame {i})")
}

fn flooded(tag: &str, lines: usize) -> Harness<'static, EditorState> {
    let mut s = state(tag);
    for i in 0..lines {
        s.console.info(line(i));
    }
    let mut h = harness(s);
    h.run();
    h
}

/// Rows of the list on screen (their text labels).
fn rows_drawn(h: &Harness<'static, EditorState>, text: &str) -> usize {
    h.query_all_by_label_contains(text).count()
}

fn click_at(h: &mut Harness<'static, EditorState>, pos: Pos2) {
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    h.run();
}

fn held_bytes(s: &EditorState) -> usize {
    let entries = s.console.entries();
    entries.capacity() * std::mem::size_of::<LogEntry>()
        + entries
            .iter()
            .map(|e| e.message.capacity() + e.time.capacity())
            .sum::<usize>()
}

fn median(mut t: Vec<f64>) -> f64 {
    t.sort_by(f64::total_cmp);
    t[t.len() / 2]
}

/// Median time of one frame of the panel in the harness, in milliseconds.
fn median_step_ms(h: &mut Harness<'static, EditorState>, steps: usize) -> f64 {
    for _ in 0..3 {
        h.step();
    }
    median(
        (0..steps)
            .map(|_| {
                let t0 = Instant::now();
                h.step();
                t0.elapsed().as_secs_f64() * 1e3
            })
            .collect(),
    )
}

/// The same without the harness's accessibility tree: layout and tessellation only.
fn median_raw_ms(s: &mut EditorState, steps: usize) -> f64 {
    let ctx = egui::Context::default();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(900.0, 400.0),
        )),
        ..Default::default()
    };
    let mut t = Vec::new();
    for i in 0..steps + 3 {
        let t0 = Instant::now();
        let out = ctx.run_ui(input(), |ui| show(ui, s));
        let _ = ctx.tessellate(out.shapes, out.pixels_per_point);
        if i >= 3 {
            t.push(t0.elapsed().as_secs_f64() * 1e3);
        }
    }
    median(t)
}

#[test]
fn only_the_rows_in_view_are_drawn() {
    let few = flooded("view_few", 500);
    let many = flooded("view_many", 60_000);
    let (a, b) = (rows_drawn(&few, "(frame "), rows_drawn(&many, "(frame "));
    assert!((3..40).contains(&a), "{a} rows drawn of 500");
    assert_eq!(a, b, "as many rows drawn of 60 000 lines as of 500");
    // The list follows the newest line.
    few.get_by_label_contains("(frame 499)");
    many.get_by_label_contains("(frame 59999)");
    assert_eq!(many.state().console.entries().len(), DEFAULT_CAPACITY);
    cleanup(few);
    cleanup(many);
}

#[test]
fn a_frame_costs_the_same_however_much_was_logged() {
    let mut small = flooded("cost_small", 1_000);
    let mut big = flooded("cost_big", 200_000);
    // The smaller of two tries each: other work on the machine only ever adds time.
    let ms =
        |h: &mut Harness<'static, EditorState>| median_step_ms(h, 20).min(median_step_ms(h, 20));
    let (a, b) = (ms(&mut small), ms(&mut big));
    // Before the rows were virtual, 200 000 lines cost hundreds of times more than 1 000.
    assert!(
        b < a * 3.0 + 1.0,
        "{b:.3} ms a frame after 200 000 lines, {a:.3} ms after 1 000"
    );
    // The rows are not gone through again when nothing changed.
    let built = big.state().console.row_rebuilds();
    big.run_steps(10);
    assert_eq!(big.state().console.row_rebuilds(), built);
    cleanup(small);
    cleanup(big);
}

#[test]
fn a_script_printing_every_frame_does_not_slow_the_panel() {
    let mut h = harness(state("stream"));
    h.run();
    let built = h.state().console.row_rebuilds();
    let frames = 15_000;
    let mut times = Vec::with_capacity(frames);
    for f in 0..frames {
        let c = &mut h.state_mut().console;
        c.info(line(f));
        c.warn(format!("slow frame {f}"));
        let t0 = Instant::now();
        h.step();
        times.push(t0.elapsed().as_secs_f64() * 1e3);
    }
    let c = &h.state().console;
    assert_eq!(c.entries().len(), DEFAULT_CAPACITY);
    assert_eq!(c.total(), 2 * frames);
    assert_eq!(c.row_rebuilds(), built, "rows kept up line by line");
    let early = median(times[200..1_200].to_vec());
    let late = median(times[frames - 1_000..].to_vec());
    assert!(
        late < early * 3.0 + 1.0,
        "{late:.3} ms a frame at the end, {early:.3} ms at the start"
    );
    h.get_by_label_contains(&format!("slow frame {}", frames - 1));
    cleanup(h);
}

#[test]
fn counters_search_and_collapse_after_lines_were_dropped() {
    let mut s = state("counts");
    for i in 0..25_000 {
        s.console.info(line(i));
        if i % 100 == 0 {
            s.console.warn("Texture has no mipmaps");
        }
        if i % 2_500 == 0 {
            s.console.log(
                LogLevel::Error,
                format!("Assets/Scripts/Enemy.lua:7: boom {i}"),
            );
        }
    }
    let mut h = harness(s);
    h.run();
    // The toolbar counts everything logged, not just what is kept.
    h.get_by_label("ℹ 25000");
    h.get_by_label("⚠ 250");
    h.get_by_label("⛔ 10");
    assert_eq!(h.state().console.entries().len(), DEFAULT_CAPACITY);
    // Search (any case) lists the errors still kept, with their script link.
    h.state_mut().console.search = "BOOM".into();
    h.run();
    let kept_errors = h.state().console.built_rows().len();
    assert!((3..=4).contains(&kept_errors), "{kept_errors}");
    assert_eq!(rows_drawn(&h, "boom"), kept_errors);
    assert_eq!(rows_drawn(&h, "(frame "), 0);
    assert_eq!(h.query_all_by_label("Enemy.lua:7").count(), kept_errors);
    h.get_by_label_contains("boom 22500");
    // Level toggles.
    h.state_mut().console.search.clear();
    h.state_mut().console.show_info = false;
    h.state_mut().console.show_errors = false;
    h.run();
    let kept_warnings = h.state().console.built_rows().len();
    assert!((99..=100).contains(&kept_warnings), "{kept_warnings}");
    // Collapse: one row for the identical warnings, with their number.
    h.state_mut().console.collapse = true;
    h.run();
    assert_eq!(rows_drawn(&h, "Texture has no mipmaps"), 1);
    h.get_by_label(&kept_warnings.to_string());
    // Clear empties the list and the counters.
    h.get_by_label("Clear").click();
    h.run();
    h.get_by_label("ℹ 0");
    h.get_by_label("⚠ 0");
    assert_eq!(rows_drawn(&h, "Texture"), 0);
    cleanup(h);
}

#[test]
fn clicking_a_row_selects_it_and_shows_the_whole_message() {
    let mut s = state("select");
    for i in 0..300 {
        s.console.info(line(i));
    }
    s.console.log(
        LogLevel::Error,
        "Assets/Scripts/Player.lua:12: attempt to index a nil value\nstack traceback:\n  in Update",
    );
    s.console.info("last line");
    let mut h = harness(s);
    h.run();
    assert_eq!(rows_drawn(&h, "stack traceback"), 0, "first lines only");
    let at = h.get_by_label_contains("attempt to index").rect().center();
    click_at(&mut h, at);
    assert_eq!(h.state().console.selected, Some(300));
    // The detail pane has the whole message, to select and copy.
    h.get_by_label_contains("stack traceback");
    // Lines logged later, or dropped earlier ones, leave the selection on its line.
    for i in 0..DEFAULT_CAPACITY - 400 {
        h.state_mut().console.info(line(i));
    }
    h.run();
    assert_eq!(h.state().console.selected, Some(300));
    h.get_by_label_contains("stack traceback");
    // Once its line is dropped there is nothing selected.
    for i in 0..1_000 {
        h.state_mut().console.info(line(i));
    }
    h.run();
    assert_eq!(h.state().console.selected, None);
    assert_eq!(rows_drawn(&h, "stack traceback"), 0);
    cleanup(h);
}

/// `cargo test -p misty_editor --release console_flood_report -- --ignored --nocapture`
#[test]
#[ignore]
fn console_flood_report() {
    for n in [1_000usize, 10_000, 50_000, 200_000] {
        let mut s = state("flood");
        for i in 0..n {
            s.console.info(line(i));
        }
        let bytes = held_bytes(&s);
        let raw = median_raw_ms(&mut s, 5);
        let mut h = harness(s);
        let ms = median_step_ms(&mut h, 5);
        println!(
            "logged {n}: held {} entries, {:.2} MB; draw {ms:.3} ms/frame (harness), {raw:.3} ms (no a11y)",
            h.state().console.entries().len(),
            bytes as f64 / 1e6
        );
        cleanup(h);
    }
    // Collapse on, distinct messages.
    let mut s = state("collapse");
    s.console.collapse = true;
    for i in 0..10_000 {
        s.console.info(line(i));
    }
    println!(
        "collapse on, 10000 distinct: {:.3} ms (no a11y)",
        median_raw_ms(&mut s, 3)
    );
    let _ = std::fs::remove_dir_all(&s.project.root);
    // A script that prints twice every frame.
    for (frames, collapse) in [(3_000usize, false), (30_000, false), (30_000, true)] {
        let mut s = state("stream_report");
        s.console.collapse = collapse;
        let mut h = harness(s);
        let mut times = Vec::with_capacity(frames);
        for f in 0..frames {
            h.state_mut().console.info(line(f));
            h.state_mut().console.warn(format!("slow frame {f}"));
            let t0 = Instant::now();
            h.step();
            times.push(t0.elapsed().as_secs_f64() * 1e3);
        }
        let avg = |r: std::ops::Range<usize>| times[r.clone()].iter().sum::<f64>() / r.len() as f64;
        println!(
            "stream of 2 lines/frame, collapse {collapse}: frames 100..200 {:.3} ms, frames {}..{} {:.3} ms, held {:.2} MB",
            avg(100..200),
            frames - 100,
            frames,
            avg(frames - 100..frames),
            held_bytes(h.state()) as f64 / 1e6
        );
        cleanup(h);
    }
}
