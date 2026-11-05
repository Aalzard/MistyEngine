//! Effect Graph tests: every block and operator compiles to WGSL that naga validates, graphs
//! round-trip through JSON, the spawn system's rates and bursts, structure errors land on the
//! right node, property data and instance controls.

use super::blocks::{Attribute, BlockKind, Composition, ContextType, OrientMode, ShapeMode};
use super::codegen::{CpuExpr, SpawnBlock, compile, property_data, ramp_data};
use super::graph::{
    BlendMode, ContextKind, EffectGraph, OutputKind, OutputSettings, PropertyValue,
};
use super::operators::{AttributeRead, OperatorKind, RandomMode};
use super::spawn::{self, SpawnState};
use super::types::{Curve, Gradient, Value, ValueType};
use super::{EffectInstance, VisualEffect};

fn ctx(g: &EffectGraph, t: ContextType) -> u32 {
    g.context_of(t).expect("context").id
}

fn compiles(g: &EffectGraph, what: &str) {
    if let Err(e) = compile(g) {
        panic!("{what}: {e:?}");
    }
}

#[test]
fn default_graph_compiles() {
    let g = EffectGraph::new_default();
    let c = compile(&g).unwrap();
    assert_eq!(c.capacity, 4096);
    assert_eq!(c.outputs.len(), 1);
    assert_eq!(c.spawn.len(), 1);
    for entry in ["cs_reset", "cs_emit", "cs_args", "cs_update", "cs_finish"] {
        assert!(c.sim_source.contains(&format!("fn {entry}(")), "{entry}");
    }
}

/// Each block, in every context that accepts it (and every setting variant), validates.
#[test]
fn every_block_validates_in_every_context() {
    let mut variants = BlockKind::catalog();
    for a in Attribute::ALL {
        for c in Composition::ALL {
            variants.push(BlockKind::SetAttribute {
                attribute: a,
                random: true,
                composition: c,
            });
        }
    }
    for m in [ShapeMode::Volume, ShapeMode::Surface] {
        variants.push(BlockKind::PositionSphere { mode: m });
        variants.push(BlockKind::PositionBox { mode: m });
        variants.push(BlockKind::PositionCone { mode: m });
        variants.push(BlockKind::PositionCircle { mode: m });
    }
    variants.push(BlockKind::CollideSphere { inverted: true });
    for m in OrientMode::ALL {
        variants.push(BlockKind::Orient { mode: m });
    }
    let mut count = 0;
    for kind in variants {
        for t in ContextType::ALL {
            if !kind.allowed_in(t) {
                continue;
            }
            let mut g = EffectGraph::new_default();
            let c = ctx(&g, t);
            g.add_block(c, kind.clone()).expect("allowed");
            compiles(&g, &format!("{} in {}", kind.title(), t.label()));
            count += 1;
        }
    }
    assert!(count > 40, "{count} combinations");
}

/// Each operator output wired into a block input of its type (in Initialize, Update and Output).
#[test]
fn every_operator_validates_wired_into_a_block() {
    let mut kinds = OperatorKind::catalog();
    kinds.push(OperatorKind::Random {
        mode: RandomMode::PerParticle,
    });
    for kind in kinds {
        for t in [
            ContextType::Initialize,
            ContextType::Update,
            ContextType::Output,
        ] {
            let mut g = EffectGraph::new_default();
            let op = g.add_operator(kind.clone(), [-300.0, 0.0]);
            let outs = g.outputs(g.operator(op).unwrap());
            for out in outs {
                let ty = g.types().output(op, out.name).unwrap();
                let attr = match ty {
                    ValueType::Float => Attribute::Size,
                    ValueType::Vec3 if t == ContextType::Output => Attribute::Angle,
                    ValueType::Vec3 => Attribute::Velocity,
                    ValueType::Color => Attribute::Color,
                    _ => unreachable!("operators output numbers"),
                };
                let block = g
                    .add_block(
                        ctx(&g, t),
                        BlockKind::SetAttribute {
                            attribute: attr,
                            random: false,
                            composition: Composition::Overwrite,
                        },
                    )
                    .unwrap();
                g.connect(op, out.name, block, attr.label())
                    .unwrap_or_else(|e| panic!("{}: {e}", kind.title()));
            }
            compiles(&g, &format!("{} in {}", kind.title(), t.label()));
        }
    }
}

#[test]
fn outputs_of_every_kind_and_blend_validate() {
    for kind in OutputKind::ALL {
        for blend in BlendMode::ALL {
            let mut g = EffectGraph::new_default();
            let out = ctx(&g, ContextType::Output);
            g.context_mut(out).unwrap().kind = ContextKind::Output(OutputSettings {
                kind,
                blend,
                flipbook: [4, 2],
                ..Default::default()
            });
            let second = g.add_context(ContextType::Output, [400.0, 640.0]);
            g.add_block(second, BlockKind::SizeOverLife);
            let c = compile(&g).unwrap_or_else(|e| panic!("{kind:?} {blend:?}: {e:?}"));
            assert_eq!(c.outputs.len(), 2);
            assert!(
                c.sim_source.contains("vfx_indirect[12u]"),
                "second draw args"
            );
        }
    }
}

#[test]
fn graph_round_trips_through_json() {
    let mut g = EffectGraph::new_default();
    let rate = g.add_property("Rate", PropertyValue::Float(250.0));
    g.add_property("Tint", PropertyValue::Color([2.0, 0.5, 0.1, 1.0]));
    g.add_property("Fade", PropertyValue::Gradient(Gradient::default()));
    g.add_property(
        "Sprite",
        PropertyValue::Texture(Some("Assets/Textures/Dot.png".into())),
    );
    let p = g.add_operator(OperatorKind::Property { name: rate }, [-300.0, 0.0]);
    let spawn = g.context_of(ContextType::Spawn).unwrap().blocks[0].id;
    g.connect(p, "Out", spawn, "Rate").unwrap();
    let noise = g.add_operator(OperatorKind::CurlNoise, [-300.0, 200.0]);
    let pos = g.add_operator(
        OperatorKind::GetAttribute {
            attribute: AttributeRead::Position,
        },
        [-500.0, 200.0],
    );
    g.connect(pos, "Out", noise, "Coordinate").unwrap();
    let upd = ctx(&g, ContextType::Update);
    let b = g
        .add_block(
            upd,
            BlockKind::SetAttribute {
                attribute: Attribute::Velocity,
                random: false,
                composition: Composition::Add,
            },
        )
        .unwrap();
    g.connect(noise, "Out", b, "Velocity").unwrap();
    g.block_mut(b)
        .unwrap()
        .values
        .insert("Velocity".into(), Value::Vec3([1.0, 2.0, 3.0]));
    let text = g.to_json();
    let back = EffectGraph::from_json(&text).unwrap();
    assert_eq!(back, g);
    assert!(text.contains("\"type\": \"Initialize\""));
    assert!(text.contains("\"capacity\": 4096"));
    compiles(&back, "round-tripped graph");
}

fn rate_block(rate: f32) -> SpawnBlock {
    SpawnBlock {
        kind: BlockKind::ConstantRate,
        inputs: vec![("Rate", CpuExpr::Const(rate))],
    }
}

fn props() -> spawn::Props<'static> {
    spawn::Props {
        defaults: &[],
        overrides: &[],
    }
}

#[test]
fn constant_rate_emits_rate_times_time() {
    let blocks = [rate_block(100.0)];
    let mut st = SpawnState::default();
    let mut total = 0;
    for _ in 0..120 {
        total += spawn::step(&blocks, &mut st, &props(), 1.0 / 60.0);
    }
    assert!((199..=200).contains(&total), "{total} over 2 s at 100/s");
    // Fractions carry over: 0.5 per step gives one every other step.
    let blocks = [rate_block(30.0)];
    let mut st = SpawnState::default();
    let counts: Vec<u32> = (0..6)
        .map(|_| spawn::step(&blocks, &mut st, &props(), 1.0 / 60.0))
        .collect();
    assert_eq!(counts.iter().sum::<u32>(), 3);
}

#[test]
fn bursts_fire_at_their_times() {
    let single = SpawnBlock {
        kind: BlockKind::SingleBurst,
        inputs: vec![
            ("Count", CpuExpr::Const(50.0)),
            ("Delay", CpuExpr::Const(0.5)),
        ],
    };
    let mut st = SpawnState::default();
    let counts: Vec<u32> = (0..10)
        .map(|_| spawn::step(std::slice::from_ref(&single), &mut st, &props(), 0.1))
        .collect();
    assert_eq!(counts.iter().sum::<u32>(), 50);
    assert_eq!(counts.iter().filter(|c| **c == 50).count(), 1, "{counts:?}");

    let periodic = SpawnBlock {
        kind: BlockKind::PeriodicBurst,
        inputs: vec![
            ("Count", CpuExpr::Const(10.0)),
            ("Period", CpuExpr::Const(0.25)),
            ("Delay", CpuExpr::Const(0.0)),
        ],
    };
    let mut st = SpawnState::default();
    let mut total = 0;
    for _ in 0..60 {
        total += spawn::step(
            std::slice::from_ref(&periodic),
            &mut st,
            &props(),
            1.0 / 60.0,
        );
    }
    assert_eq!(total, 40, "bursts at 0, 0.25, 0.5, 0.75");
    // One big step covers several bursts at once.
    let mut st = SpawnState::default();
    assert_eq!(spawn::step(&[periodic], &mut st, &props(), 1.0), 40);
}

#[test]
fn spawn_inputs_read_properties_and_overrides() {
    let mut g = EffectGraph::new_default();
    let rate = g.add_property("Rate", PropertyValue::Float(10.0));
    let p = g.add_operator(OperatorKind::Property { name: rate.clone() }, [-300.0, 0.0]);
    let two = g.add_operator(OperatorKind::Float, [-300.0, 100.0]);
    g.operator_mut(two)
        .unwrap()
        .values
        .insert("Value".into(), Value::Float(2.0));
    let mul = g.add_operator(OperatorKind::Multiply, [-150.0, 0.0]);
    g.connect(p, "Out", mul, "A").unwrap();
    g.connect(two, "Out", mul, "B").unwrap();
    let spawn = g.context_of(ContextType::Spawn).unwrap().blocks[0].id;
    g.connect(mul, "Out", spawn, "Rate").unwrap();
    let c = compile(&g).unwrap();
    let mut st = SpawnState::default();
    let defaults = spawn::Props {
        defaults: &c.properties,
        overrides: &[],
    };
    assert_eq!(spawn::step(&c.spawn, &mut st, &defaults, 1.0), 20);
    let overrides = [(rate, PropertyValue::Float(50.0))];
    let over = spawn::Props {
        defaults: &c.properties,
        overrides: &overrides,
    };
    assert_eq!(spawn::step(&c.spawn, &mut st, &over, 1.0), 100);
}

#[test]
fn structure_errors_land_on_their_nodes() {
    let mut g = EffectGraph::default();
    let errors = compile(&g).unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("Spawn")));
    assert!(errors.iter().any(|e| e.message.contains("Initialize")));
    assert!(errors.iter().any(|e| e.message.contains("Output")));

    g = EffectGraph::new_default();
    let extra = g.add_context(ContextType::Spawn, [500.0, 0.0]);
    let errors = compile(&g).unwrap_err();
    assert_eq!(errors[0].node, Some(extra));

    // A Spawn block fed by a per-particle operator: refused when linking, an error when forced.
    g = EffectGraph::new_default();
    let spawn = g.context_of(ContextType::Spawn).unwrap().blocks[0].id;
    let noise = g.add_operator(OperatorKind::Noise, [-300.0, 0.0]);
    assert!(g.connect(noise, "Out", spawn, "Rate").is_err());
    g.links.push(super::graph::Link {
        from: noise,
        output: "Out".into(),
        to: spawn,
        input: "Rate".into(),
    });
    let errors = compile(&g).unwrap_err();
    assert_eq!(errors[0].node, Some(spawn));

    // A Property node whose property was removed.
    g = EffectGraph::new_default();
    let p = g.add_operator(
        OperatorKind::Property {
            name: "Gone".into(),
        },
        [0.0, 0.0],
    );
    let init = ctx(&g, ContextType::Initialize);
    let b = g
        .add_block(
            init,
            BlockKind::SetAttribute {
                attribute: Attribute::Size,
                random: false,
                composition: Composition::Overwrite,
            },
        )
        .unwrap();
    g.links.push(super::graph::Link {
        from: p,
        output: "Out".into(),
        to: b,
        input: "Size".into(),
    });
    let errors = compile(&g).unwrap_err();
    assert_eq!(errors[0].node, Some(p), "{errors:?}");

    // Blocks only go where they belong; ramps don't feed numbers.
    g = EffectGraph::new_default();
    assert!(
        g.add_block(ctx(&g, ContextType::Spawn), BlockKind::Gravity)
            .is_none()
    );
    let sg = g.add_operator(OperatorKind::SampleGradient, [0.0, 0.0]);
    let len = g.add_operator(OperatorKind::Length, [0.0, 0.0]);
    assert!(
        g.connect(sg, "Out", len, "Input").is_ok(),
        "colour truncates to vector"
    );
    let grad = g.add_property("G", PropertyValue::Gradient(Gradient::default()));
    let gp = g.add_operator(OperatorKind::Property { name: grad }, [0.0, 0.0]);
    assert!(g.connect(gp, "Out", len, "Input").is_err());
    assert!(g.connect(gp, "Out", sg, "Gradient").is_ok());
    // Loops are refused.
    let a = g.add_operator(OperatorKind::Add, [0.0, 0.0]);
    let m = g.add_operator(OperatorKind::Multiply, [0.0, 0.0]);
    g.connect(a, "Out", m, "A").unwrap();
    assert!(g.connect(m, "Out", a, "A").is_err());
}

#[test]
fn removing_nodes_drops_their_links() {
    let mut g = EffectGraph::new_default();
    let op = g.add_operator(OperatorKind::Float, [0.0, 0.0]);
    let upd = ctx(&g, ContextType::Update);
    let drag = g.context(upd).unwrap().blocks[0].id;
    g.connect(op, "Out", drag, "Drag").unwrap();
    assert_eq!(g.links.len(), 1);
    g.remove(upd);
    assert!(g.links.is_empty());
    assert!(g.block(drag).is_none());
}

#[test]
fn property_data_applies_overrides_in_linear_colour() {
    let mut g = EffectGraph::new_default();
    g.add_property("Speed", PropertyValue::Float(3.0));
    g.add_property("Tint", PropertyValue::Color([0.5, 0.5, 0.5, 1.0]));
    g.add_property("Fade", PropertyValue::Gradient(Gradient::default()));
    let fade = g.add_operator(
        OperatorKind::Property {
            name: "Fade".into(),
        },
        [0.0, 0.0],
    );
    let out = ctx(&g, ContextType::Output);
    let col = g.context(out).unwrap().blocks[0].id;
    g.connect(fade, "Out", col, "Gradient").unwrap();
    let c = compile(&g).unwrap();
    let data = property_data(&c, &[]);
    assert_eq!(data[c.slot("Speed").unwrap()][0], 3.0);
    let tint = data[c.slot("Tint").unwrap()];
    assert!((tint[0] - 0.5f32.powf(2.2)).abs() < 1e-5);
    let over = property_data(&c, &[("Speed".into(), PropertyValue::Float(7.0))]);
    assert_eq!(over[c.slot("Speed").unwrap()][0], 7.0);
    // A wrongly typed override is ignored.
    let bad = property_data(&c, &[("Speed".into(), PropertyValue::Vector3([1.0; 3]))]);
    assert_eq!(bad[c.slot("Speed").unwrap()][0], 3.0);
    // The gradient property's ramp follows its override.
    let red = Gradient {
        color_keys: vec![super::types::ColorKey {
            time: 0.0,
            color: [1.0, 0.0, 0.0],
        }],
        alpha_keys: vec![super::types::AlphaKey {
            time: 0.0,
            alpha: 1.0,
        }],
    };
    let ramps = ramp_data(&c, &[("Fade".into(), PropertyValue::Gradient(red))]);
    let row = c
        .ramps
        .iter()
        .position(|r| matches!(r, super::codegen::RampSource::Property(n) if n == "Fade"))
        .unwrap();
    let px = ramps[row * super::codegen::RAMP_SAMPLES + 10];
    assert_eq!(px[0], 1.0);
    assert_eq!(px[1], 0.0);
}

#[test]
fn curves_evaluate_piecewise_linear() {
    let c = Curve {
        keys: vec![[0.0, 0.0], [0.5, 2.0], [1.0, 1.0]],
    };
    assert_eq!(c.evaluate(-1.0), 0.0);
    assert_eq!(c.evaluate(0.25), 1.0);
    assert_eq!(c.evaluate(0.75), 1.5);
    assert_eq!(c.evaluate(2.0), 1.0);
}

#[test]
fn instance_play_stop_reinit() {
    let c = std::sync::Arc::new(compile(&EffectGraph::new_default()).unwrap());
    let comp = VisualEffect {
        reseed_on_play: false,
        seed: 7,
        ..Default::default()
    };
    let mut i = EffectInstance::with_effect(c, &comp);
    assert!(!i.playing);
    i.play(&comp);
    i.step(&comp, 0.5);
    assert_eq!(i.spawn_count, 10, "20 per second for half a second");
    assert_eq!(i.serial, 1);
    i.stop();
    i.step(&comp, 0.5);
    assert_eq!(i.spawn_count, 0, "stopped effects don't spawn");
    let reset = i.reset_gen;
    i.reinit(&comp);
    assert!(i.playing);
    assert_eq!(i.reset_gen, reset + 1);
    assert_eq!(i.seed, 7, "no reseeding");
    let d = i.draw(glam::Mat4::IDENTITY, &comp).unwrap();
    assert_eq!(d.seed, 7);
    assert_eq!(d.textures, vec![None]);
}

#[test]
fn working_copies_replace_the_file_until_cleared() {
    let dir = std::env::temp_dir().join(format!("misty_vfx_lib_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Lib.effect");
    EffectGraph::new_default().save(&file).unwrap();
    let rel = file.to_string_lossy().replace('\\', "/");
    let (g0, c0) = super::library::get(&rel);
    assert_eq!(c0.unwrap().capacity, 4096);
    let mut edited = EffectGraph::new_default();
    for c in &mut edited.contexts {
        if let ContextKind::Initialize(s) = &mut c.kind {
            s.capacity = 100;
        }
    }
    super::library::set_working_copy(&rel, &edited);
    let (g1, c1) = super::library::get(&rel);
    assert!(g1 > g0);
    assert_eq!(c1.unwrap().capacity, 100);
    // Same content again: no new generation (instances keep their particles).
    super::library::set_working_copy(&rel, &edited);
    assert_eq!(super::library::generation(&rel), g1);
    super::library::clear_working_copy(&rel);
    assert_eq!(super::library::get(&rel).1.unwrap().capacity, 4096);
    let _ = std::fs::remove_dir_all(&dir);
}
