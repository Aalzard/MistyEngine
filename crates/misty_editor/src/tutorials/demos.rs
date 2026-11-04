//! `demo` actions: each does what a step asks the user to do, in code. Screenshots use them to
//! reach a step (`MISTY_TUTORIAL=<id>:<step>`) and tests to run tutorials end to end.

use glam::Vec3;
use misty_engine::scene::{
    CloudPreset, Light, PostProcessVolume, PrimitiveMesh, Shadows, Transform, registry,
};
use misty_engine::terrain::{TerrainLayer, store};
use misty_engine::world_gen::stamps::WaterMode;
use misty_engine::world_gen::{self, Generator, Preset, StampKind, WorldBuilder, WorldStamp};

use super::conditions::{sun_entity, terrain_entity, volume_entity};
use crate::actions::{Action, Create};
use crate::state::EditorState;
use crate::terrain_tools::{self, TerrainTool};

/// Run `demo`; false when it is not known.
pub fn run(s: &mut EditorState, demo: &str) -> bool {
    let (kind, arg) = demo.split_once(':').unwrap_or((demo, ""));
    match kind {
        "select" => {
            if let Some(e) = s.scene.find_by_name(arg) {
                s.select(e);
            }
        }
        "add" => {
            let Some(info) = registry::find(arg) else {
                return false;
            };
            crate::commands::apply(s, Action::AddComponent(info.name));
        }
        "create" => {
            let what = match arg {
                "Terrain" => Create::Terrain,
                "Local Volumetric Fog" => Create::FogVolume,
                "Cube" => Create::Primitive(PrimitiveMesh::Cube),
                _ => return false,
            };
            crate::commands::apply(s, Action::Create { what, parent: None });
        }
        "focus" => {
            let Some(tab) = crate::layout::Tab::every().find(|t| t.title() == arg) else {
                return false;
            };
            s.push(Action::FocusTab(tab));
        }
        "play" => crate::play::start(s),
        "ppv" => return ppv(s, arg),
        "sun" => return sun(s, arg),
        "god_rays" if arg == "wall" => {
            crate::commands::apply(
                s,
                Action::Create {
                    what: Create::Primitive(PrimitiveMesh::Cube),
                    parent: None,
                },
            );
            if let Some(e) = s.selection.active() {
                let mut t = Transform::from_position(Vec3::new(-1.0, 3.0, 5.0));
                t.scale = Vec3::new(7.0, 6.0, 0.4);
                s.scene.set_transform(e, t);
            }
        }
        "terrain" => return terrain(s, arg),
        "wb" => return wb(s, arg),
        _ => return false,
    }
    true
}

/// Change the scene's Post-process Volume (selecting its object first, like the user would).
fn ppv(s: &mut EditorState, arg: &str) -> bool {
    let Some(e) = volume_entity(s) else {
        return false;
    };
    s.select(e);
    let Ok(mut v) = s
        .scene
        .world
        .get::<&PostProcessVolume>(e)
        .map(|v| (*v).clone())
    else {
        return false;
    };
    match arg {
        "tonemapping" => v.tonemapping.active = true,
        "bloom_color" => {
            v.bloom.active = true;
            v.color_adjustments.active = true;
            v.color_adjustments.contrast = 15.0;
        }
        "volumetric" => {
            v.fog.active = true;
            v.fog.volumetric = true;
        }
        "anisotropy" => v.fog.anisotropy = 0.7,
        "heights" => v.fog.maximum_height = 15.0,
        "ambient_low" => v.fog.ambient_dimmer = 0.2,
        "sun_shafts" => v.sun_shafts.active = true,
        "clouds" => {
            v.clouds.active = true;
            CloudPreset::Sparse.apply(&mut v.clouds);
        }
        "cloud_shape" => v.clouds.shape = 0.6,
        _ => return false,
    }
    let _ = s.scene.world.insert_one(e, v);
    s.mark_edited("Change Post-process Volume");
    true
}

fn sun(s: &mut EditorState, arg: &str) -> bool {
    let Some(e) = sun_entity(s) else {
        return false;
    };
    s.select(e);
    let Ok(mut l) = s.scene.world.get::<&mut Light>(e) else {
        return false;
    };
    match arg {
        "shadows" => l.shadows = Shadows::Soft,
        "volumetric" => l.volumetric_multiplier = 2.5,
        _ => return false,
    }
    true
}

/// The first project texture whose name contains `word` (else any texture).
fn texture(s: &EditorState, word: &str) -> Option<String> {
    let all = &s.assets.textures;
    all.iter()
        .find(|t| t.to_lowercase().contains(word))
        .or_else(|| all.first())
        .cloned()
}

fn terrain(s: &mut EditorState, arg: &str) -> bool {
    let Some(e) = terrain_entity(s) else {
        return false;
    };
    s.select(e);
    let tool = match arg {
        "raise" => Some(TerrainTool::RaiseLower),
        "smooth" => Some(TerrainTool::Smooth),
        "set_height" => Some(TerrainTool::SetHeight),
        "paint" => Some(TerrainTool::Paint),
        _ => None,
    };
    if let Some(tool) = tool {
        if tool == TerrainTool::Paint {
            s.terrain_tools.layer = 1;
        }
        terrain_tools::demo_stroke(s, tool);
        return true;
    }
    let Ok(path) = s
        .scene
        .world
        .get::<&misty_engine::scene::Terrain>(e)
        .map(|t| t.terrain_data.clone())
    else {
        return false;
    };
    s.terrain_tools.tool = TerrainTool::Paint;
    if arg == "generate" {
        crate::terrain_rules::generate(s, &path);
        terrain_tools::end_frame(s, false);
        return true;
    }
    if !matches!(
        arg,
        "layers1" | "texture1" | "layers2" | "rule_slope" | "rules2"
    ) {
        return false;
    }
    let (grass, rock) = (texture(s, "grass"), texture(s, "rock"));
    let layer = |name: &str, diffuse: Option<String>| TerrainLayer {
        name: name.into(),
        diffuse,
        ..Default::default()
    };
    terrain_tools::begin_edit(s, &path, "Change Terrain Layer");
    store::modify_settings(&path, |d| {
        let (first, second) = match &mut d.layers[..] {
            [a, b, ..] => (Some(a), Some(b)),
            [a] => (Some(a), None),
            [] => (None, None),
        };
        match arg {
            "texture1" => first.into_iter().for_each(|l| l.diffuse = grass.clone()),
            "rule_slope" => second.into_iter().for_each(|l| {
                l.rules.enabled = true;
                l.rules.slope = [30.0, 90.0];
            }),
            "rules2" => first.into_iter().for_each(|l| {
                l.rules.enabled = true;
                l.rules.slope = [0.0, 30.0];
            }),
            "layers1" => d.layers.push(layer("Grass", None)),
            _ => d.layers.push(layer("Rock", rock.clone())),
        }
    });
    terrain_tools::end_frame(s, false);
    s.terrain_tools.layer = usize::from(matches!(arg, "layers2" | "rule_slope"));
    true
}

fn wb(s: &mut EditorState, arg: &str) -> bool {
    if arg == "create" {
        crate::panels::world_builder::create_builder(s, None);
        return true;
    }
    let Some(b) = world_gen::builders(&s.scene).first().copied() else {
        return false;
    };
    match arg {
        "generator" => {
            if let Ok(mut w) = s.scene.world.get::<&mut WorldBuilder>(b) {
                w.base.generator = Generator::Mountains;
            }
        }
        "mountain" => {
            crate::panels::world_builder::add_stamp(s, b, "Mountain");
        }
        "sea" => {
            if let Some(e) = crate::panels::world_builder::add_stamp(s, b, "Water")
                && let Ok(mut w) = s.scene.world.get::<&mut WorldStamp>(e)
                && let StampKind::Water(water) = &mut w.kind
            {
                water.mode = WaterMode::Sea;
            }
        }
        "zone" => {
            if let Some(e) = crate::panels::world_builder::add_stamp(s, b, "Biome Zone")
                && let Ok(mut w) = s.scene.world.get::<&mut WorldStamp>(e)
                && let StampKind::BiomeZone(z) = &mut w.kind
            {
                z.biome = Preset::Desert.path();
            }
        }
        "biome_open" => s.world_builder.biome = Some(Preset::Forest.path()),
        "generate" => crate::panels::world_builder::generate(s, b),
        _ => return false,
    }
    true
}
