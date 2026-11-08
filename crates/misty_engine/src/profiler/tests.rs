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

#[test]
fn main_samples_start_at_the_frame_start() {
    let _serial = test_serial();
    set_enabled(true);
    end_frame();
    busy(0.3);
    {
        profile_scope!("Late", Category::Others);
    }
    end_frame();
    let frame = last_frame();
    set_enabled(false);
    assert!(frame.samples[0].start_ms >= 0.25, "{:?}", frame.samples[0]);
    assert!(frame.cpu_ms >= frame.samples[0].start_ms);
}

#[test]
fn worker_threads_get_their_own_timeline_rows() {
    let _serial = test_serial();
    set_enabled(true);
    end_frame();
    std::thread::Builder::new()
        .name("misty background 3".into())
        .spawn(|| {
            profile_scope!("MeshSdf.Bake", Category::Rendering);
            {
                profile_scope!("Bricks");
                busy(0.2);
            }
        })
        .unwrap()
        .join()
        .unwrap();
    std::thread::spawn(|| {
        name_thread("Audio Thread");
        profile_scope!("Audio.Mix", Category::Audio);
    })
    .join()
    .unwrap();
    {
        profile_scope!("MainWork", Category::Others);
    }
    end_frame();
    let frame = last_frame();
    set_enabled(false);
    assert_eq!(frame.samples.len(), 1, "only the main thread's scope");
    let bg = frame
        .threads
        .iter()
        .find(|t| t.kind == ThreadKind::Background)
        .expect("background row");
    assert_eq!(bg.name, "Background 3");
    assert_eq!(bg.samples.len(), 2);
    assert_eq!(bg.samples[1].depth, 1);
    assert!(bg.samples[1].duration_ms >= 0.2);
    assert!(bg.samples[0].start_ms >= 0.0);
    let audio = frame
        .threads
        .iter()
        .find(|t| t.kind == ThreadKind::Audio)
        .expect("audio row");
    assert_eq!(audio.name, "Audio Thread");
    assert_eq!(audio.samples[0].name, "Audio.Mix");
    // Rows are sorted by kind: audio before background.
    let kinds: Vec<ThreadKind> = frame.threads.iter().map(|t| t.kind).collect();
    let mut sorted = kinds.clone();
    sorted.sort();
    assert_eq!(kinds, sorted);
}

#[test]
fn rayon_workers_are_named_by_index() {
    use rayon::prelude::*;
    let _serial = test_serial();
    set_enabled(true);
    end_frame();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    pool.install(|| {
        (0..4).into_par_iter().with_max_len(1).for_each(|_| {
            profile_scope!("Chunk", Category::Rendering);
            busy(0.05);
        })
    });
    end_frame();
    let frame = last_frame();
    set_enabled(false);
    let chunks: Vec<&ThreadTrack> = frame
        .threads
        .iter()
        .filter(|t| t.samples.iter().any(|s| s.name == "Chunk"))
        .collect();
    let total: usize = chunks
        .iter()
        .map(|t| t.samples.iter().filter(|s| s.name == "Chunk").count())
        .sum();
    // `install` may run some chunks on this (main) thread; those are Main Thread scopes.
    let on_main = frame.samples.iter().filter(|s| s.name == "Chunk").count();
    assert_eq!(total + on_main, 4);
    assert!(
        chunks
            .iter()
            .all(|t| t.kind == ThreadKind::Worker && t.name.starts_with("Worker "))
    );
}

#[test]
fn counters_are_per_frame_and_off_when_not_recording() {
    let _serial = test_serial();
    set_enabled(false);
    set_counter(Counter::PhysicsBodies, 9.0);
    set_enabled(true);
    end_frame();
    set_counter(Counter::PhysicsBodies, 12.0);
    add_counter(Counter::AudioMixMs, 0.25);
    std::thread::spawn(|| add_counter(Counter::AudioMixMs, 0.5))
        .join()
        .unwrap();
    end_frame();
    let frame = last_frame();
    end_frame();
    let next = last_frame();
    set_enabled(false);
    assert_eq!(frame.counter(Counter::PhysicsBodies), 12.0);
    assert_eq!(frame.counter(Counter::AudioMixMs), 0.75);
    assert_eq!(
        next.counter(Counter::PhysicsBodies),
        0.0,
        "reset each frame"
    );
    assert_eq!(Counter::PhysicsJoints.group(), CounterGroup::Physics);
    assert_eq!(Counter::AudioDspLoad.group(), CounterGroup::Audio);
    assert_eq!(Counter::UiBatches.group(), CounterGroup::Ui);
    assert_eq!(Counter::ALL.len(), Counter::COUNT);
    for (i, c) in Counter::ALL.iter().enumerate() {
        assert_eq!(c.index(), i);
    }
}

#[test]
fn gpu_passes_reach_their_frame_and_group() {
    let _serial = test_serial();
    set_enabled(true);
    let index = frame_index();
    let pass = |name: &'static str, ms: f32| GpuPass {
        view: "Game".into(),
        name: name.into(),
        start_ms: 0.0,
        ms,
    };
    add_gpu_passes(index, vec![pass("Shadow maps (cascades, local)", 1.0)]);
    end_frame();
    add_gpu_passes(
        index,
        vec![
            pass("Main: opaque meshes", 2.0),
            pass("Bloom, tonemap, finish", 0.5),
            pass("Grass shading", 0.25),
        ],
    );
    let frame = with_frames(|f| f.iter().find(|f| f.index == index).cloned()).unwrap();
    set_enabled(false);
    assert_eq!(frame.gpu_passes.len(), 4);
    let groups = frame.gpu_groups();
    assert_eq!(groups[GpuGroup::Shadows.index()], 1.0);
    assert_eq!(groups[GpuGroup::Opaque.index()], 2.0);
    assert_eq!(groups[GpuGroup::PostProcess.index()], 0.5);
    assert_eq!(groups[GpuGroup::Vegetation.index()], 0.25);
    assert_eq!(frame.gpu_total_ms(), Some(3.75));
}

#[test]
fn gpu_groups_cover_the_renderer_labels() {
    for (label, group) in [
        ("Depth prepass", GpuGroup::Depth),
        ("Hi-Z + culling (second phase)", GpuGroup::Depth),
        ("Virtual shadow map pages", GpuGroup::Shadows),
        ("Virtual geometry: shadow culling", GpuGroup::Shadows),
        ("Main: virtual geometry", GpuGroup::Opaque),
        ("Main pass (rest)", GpuGroup::Opaque),
        ("Main: grass lit", GpuGroup::Vegetation),
        ("Main: trees", GpuGroup::Vegetation),
        ("GTAO", GpuGroup::Lighting),
        ("Froxel fog", GpuGroup::Lighting),
        ("Clouds", GpuGroup::Lighting),
        ("Decals", GpuGroup::Transparent),
        ("Water simulation", GpuGroup::Transparent),
        ("TAA", GpuGroup::PostProcess),
        ("Depth of field", GpuGroup::PostProcess),
    ] {
        assert_eq!(GpuGroup::of(label), group, "{label}");
    }
}

#[test]
fn saved_captures_load_back() {
    let _serial = test_serial();
    set_enabled(true);
    end_frame();
    {
        profile_scope!("Saved", Category::Physics);
    }
    set_counter(Counter::UiElements, 33.0);
    end_frame();
    let frame = last_frame();
    set_enabled(false);
    let dir = std::env::temp_dir();
    let path = dir.join(format!("misty_profile_save_{}.mprof", std::process::id()));
    capture::save(&path, [&frame, &frame]).unwrap();
    let loaded = capture::load(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].index, frame.index);
    assert_eq!(loaded[0].samples, frame.samples);
    assert_eq!(loaded[0].counter(Counter::UiElements), 33.0);
    // Older captures (no counters, threads or GPU passes) still load; junk lines are skipped.
    let old = "{\"index\":3,\"cpu_ms\":16.0,\"gpu_ms\":null,\
               \"categories\":[0,0,0,0,0,0,0,16],\"samples\":[]}";
    let path2 = dir.join(format!("misty_profile_old_{}.mprof", std::process::id()));
    std::fs::write(&path2, format!("{old}\nnot json\n")).unwrap();
    let loaded = capture::load(&path2).unwrap();
    std::fs::write(&path2, "garbage\n").unwrap();
    let bad = capture::load(&path2);
    let _ = std::fs::remove_file(&path2);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].counter(Counter::UiElements), 0.0);
    assert!(bad.is_err());
}

#[test]
fn a_player_streams_frames_to_the_editor_listener() {
    let _serial = test_serial();
    let listener = remote::Listener::bind("127.0.0.1:0").unwrap();
    remote::connect(&listener.addr.to_string()).unwrap();
    assert!(enabled() && remote::is_connected());
    for i in 0..12 {
        {
            profile_scope!(format!("Frame{i}"), Category::Scripts);
        }
        end_frame();
    }
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while listener.with_frames(|f| f.len()) < 8 && Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    remote::disconnect();
    set_enabled(false);
    let got = listener.with_frames(|f| f.iter().cloned().collect::<Vec<_>>());
    assert!(got.len() >= 8, "received {}", got.len());
    assert!(got[0].samples[0].name.starts_with("Frame"));
}
