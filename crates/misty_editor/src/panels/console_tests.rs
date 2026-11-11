//! Console panel tests: BASELINE measurement (before the bounded store).

use std::time::Instant;

use egui_kittest::Harness;

use super::show;
use crate::project::ProjectBrowser;
use crate::state::EditorState;

fn state(tag: &str) -> EditorState {
    let root = std::env::temp_dir().join(format!("misty_console_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    EditorState::new(ProjectBrowser::open(root))
}

fn harness(s: EditorState) -> Harness<'static, EditorState> {
    Harness::builder()
        .with_size([900.0, 400.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s)
}

fn line(i: usize) -> String {
    format!("Player position 12.5, 3.0, 7.25 (frame {i})")
}

fn held_bytes(s: &EditorState) -> usize {
    let c = &s.console;
    c.entries.capacity() * std::mem::size_of::<crate::console::LogEntry>()
        + c.entries
            .iter()
            .map(|e| e.message.capacity() + e.time.capacity())
            .sum::<usize>()
}

fn median_step_ms(h: &mut Harness<'static, EditorState>, steps: usize) -> f64 {
    for _ in 0..3 {
        h.step();
    }
    let mut t: Vec<f64> = (0..steps)
        .map(|_| {
            let t0 = Instant::now();
            h.step();
            t0.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    t.sort_by(f64::total_cmp);
    t[t.len() / 2]
}

/// The same panel without the harness's accessibility tree.
fn median_raw_ms(s: &mut EditorState, steps: usize) -> f64 {
    let ctx = egui::Context::default();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
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
    t.sort_by(f64::total_cmp);
    t[t.len() / 2]
}

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
            h.state().console.entries.len(),
            bytes as f64 / 1e6
        );
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
    // A script that prints twice every frame.
    let mut h = harness(state("stream"));
    let frames = 3_000;
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
        "stream of 2 lines/frame: frames 100..200 {:.3} ms, frames {}..{} {:.3} ms, held {:.2} MB",
        avg(100..200),
        frames - 100,
        frames,
        avg(frames - 100..frames),
        held_bytes(h.state()) as f64 / 1e6
    );
}
