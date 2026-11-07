use super::*;

fn busy(ms: f32) {
    let start = Instant::now();
    while start.elapsed().as_secs_f32() * 1000.0 < ms {
        std::hint::spin_loop();
    }
}

fn last_frame() -> FrameData {
    with_frames(|f| f.back().cloned()).expect("a recorded frame")
}

#[test]
fn scopes_nest_and_merge_in_the_hierarchy() {
    let _serial = test_serial();
    set_enabled(true);
    end_frame(); // start a clean frame
    {
        profile_scope!("Scripts.Update", Category::Scripts);
        for i in 0..3 {
            profile_scope!(format!("Mover{}.Update()", i % 2));
            busy(0.2);
        }
    }
    {
        profile_scope!("Physics", Category::Physics);
        busy(0.5);
    }
    end_frame();
    let frame = last_frame();
    set_enabled(false);

    assert_eq!(frame.samples.len(), 5);
    assert_eq!(frame.samples[1].depth, 1);
    assert_eq!(
        frame.samples[1].category,
        Category::Scripts,
        "children inherit the category"
    );
    let tree = HierarchyTree::build(&frame);
    let root = &tree.nodes[0];
    assert_eq!(root.children.len(), 2);
    let scripts = tree
        .nodes
        .iter()
        .find(|n| n.name == "Scripts.Update")
        .unwrap();
    assert_eq!(scripts.children.len(), 2, "same names merge");
    let mover0 = &tree.nodes[scripts.children[0]];
    assert_eq!((mover0.name.as_str(), mover0.calls), ("Mover0.Update()", 2));
    assert!(scripts.self_ms < scripts.total_ms * 0.5);
    assert!(frame.category_ms(Category::Scripts) >= 0.5);
    assert!(frame.category_ms(Category::Physics) >= 0.5);
    // The chart's categories never add up to more than the frame.
    let sum: f32 = Category::ALL.iter().map(|c| frame.category_ms(*c)).sum();
    assert!(sum >= frame.cpu_ms - 1e-3);
}

#[test]
fn disabled_scopes_record_nothing_and_skip_their_name() {
    let _serial = test_serial();
    set_enabled(false);
    let mut evaluated = false;
    {
        profile_scope!({
            evaluated = true;
            "never"
        });
    }
    assert!(!evaluated);
    let before = with_frames(|f| f.len());
    end_frame();
    assert_eq!(with_frames(|f| f.len()), before);
}

#[test]
fn gpu_times_reach_their_frame_and_history_is_capped() {
    let _serial = test_serial();
    set_enabled(true);
    let index = frame_index();
    add_gpu_time(index, 1.5); // before the frame ends
    end_frame();
    add_gpu_time(index, 0.5); // after
    let frame = with_frames(|f| f.iter().find(|f| f.index == index).cloned()).unwrap();
    assert_eq!(frame.gpu_ms, Some(2.0));
    for _ in 0..HISTORY + 5 {
        end_frame();
    }
    assert_eq!(with_frames(|f| f.len()), HISTORY);
    set_enabled(false);
}

#[test]
fn capture_writes_one_json_line_per_frame() {
    let _serial = test_serial();
    let path = std::env::temp_dir().join("misty_profile_capture_test.jsonl");
    capture::start(&path).unwrap();
    for _ in 0..10 {
        {
            profile_scope!("Work", Category::Others);
        }
        end_frame();
    }
    capture::finish().unwrap();
    set_enabled(false);
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 10);
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v["samples"][0]["name"], "Work");
    let _ = std::fs::remove_file(path);
}
