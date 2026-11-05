//! Inspector editors for tilemaps and 2D lights: Grid, Tilemap, Tilemap Renderer, Tilemap
//! Collider 2D, Light 2D and Shadow Caster 2D, plus the `.tile` and `.palette` asset editors
//! (Rule Tile rules as clickable 3×3 neighbour grids).

use egui::{Color32, Sense, Stroke, Ui, vec2};
use glam::Vec2;
use misty_engine::hecs::Entity;
use misty_engine::light2d::{Light2D, Light2DType, ShadowCaster2D};
use misty_engine::scene::SceneComponent;
use misty_engine::tilemap::tile::{
    self, ColliderType, Neighbor, RuleOutput, RuleTransform, TileAsset, TileKind, TilingRule,
};
use misty_engine::tilemap::{
    Grid, GridLayout, Tilemap, TilemapCollider2D, TilemapRenderMode, TilemapRenderer,
};

use super::editors::{color_rgb, combo, grid};
use super::sprite_editors::{sprite_choices, sprite_slot};
use crate::state::EditorState;

/// Names of the components handled here.
pub const NAMES: [&str; 6] = [
    "Grid",
    "Tilemap",
    "Tilemap Renderer",
    "Tilemap Collider 2D",
    "Light 2D",
    "Shadow Caster 2D",
];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Grid" => "#",
        "Tilemap" | "Tilemap Renderer" => "▦",
        "Tilemap Collider 2D" => "⬠",
        "Light 2D" => "💡",
        "Shadow Caster 2D" => "◩",
        _ => return None,
    })
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Tilemap Renderer" => w.get::<&TilemapRenderer>(e).ok().map(|c| c.enabled),
        "Tilemap Collider 2D" => w
            .get::<&TilemapCollider2D>(e)
            .ok()
            .map(|c| c.common.enabled),
        "Light 2D" => w.get::<&Light2D>(e).ok().map(|c| c.enabled),
        "Shadow Caster 2D" => w.get::<&ShadowCaster2D>(e).ok().map(|c| c.enabled),
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &state.scene.world;
    let _ = match name {
        "Tilemap Renderer" => w.get::<&mut TilemapRenderer>(e).map(|mut c| c.enabled = on),
        "Tilemap Collider 2D" => w
            .get::<&mut TilemapCollider2D>(e)
            .map(|mut c| c.common.enabled = on),
        "Light 2D" => w.get::<&mut Light2D>(e).map(|mut c| c.enabled = on),
        "Shadow Caster 2D" => w.get::<&mut ShadowCaster2D>(e).map(|mut c| c.enabled = on),
        _ => return,
    };
}

fn edit_copy<T: SceneComponent>(
    state: &mut EditorState,
    e: Entity,
    f: impl FnOnce(&mut T) -> bool,
) -> bool {
    let Ok(mut copy) = state.scene.world.get::<&T>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let changed = f(&mut copy);
    if changed {
        let _ = state.scene.world.insert_one(e, copy);
    }
    changed
}

fn number(ui: &mut Ui, label: &str, v: &mut f32, speed: f64, min: f32, max: f32) -> bool {
    ui.label(label);
    let ch = ui
        .add(
            egui::DragValue::new(v)
                .speed(speed)
                .range(min..=max)
                .max_decimals(3),
        )
        .changed();
    ui.end_row();
    ch
}

fn vec2_row(ui: &mut Ui, label: &str, v: &mut Vec2, speed: f64) -> bool {
    ui.label(label);
    let ch = ui
        .horizontal(|ui| {
            let x = ui.add(egui::DragValue::new(&mut v.x).speed(speed).prefix("X "));
            let y = ui.add(egui::DragValue::new(&mut v.y).speed(speed).prefix("Y "));
            x.changed() || y.changed()
        })
        .inner;
    ui.end_row();
    ch
}

fn color_rgba(ui: &mut Ui, c: &mut [f32; 4]) -> bool {
    let mut srgba = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed = crate::panels::inspector::card::wide(ui, |ui| {
        ui.color_edit_button_srgba_unmultiplied(&mut srgba)
    })
    .changed();
    if changed {
        *c = srgba.map(|v| v as f32 / 255.0);
    }
    changed
}

fn sorting_layer(ui: &mut Ui, id: &str, layers: &[String], value: &mut String) -> bool {
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.clone())
        .show_ui(ui, |ui| {
            for l in layers {
                ch |= ui.selectable_value(value, l.clone(), l).changed();
            }
        });
    ch
}

/// Draw the editor for component `name`; true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Grid" => edit_copy::<Grid>(state, e, |g| {
            grid(ui, "grid_component", |ui| {
                let mut ch = vec2_row(ui, "Cell Size", &mut g.cell_size, 0.01);
                g.cell_size = g.cell_size.max(Vec2::splat(0.001));
                ch |= vec2_row(ui, "Cell Gap", &mut g.cell_gap, 0.01);
                ui.label("Cell Layout");
                ch |= combo(
                    ui,
                    "grid_layout",
                    &mut g.layout,
                    &GridLayout::ALL,
                    GridLayout::label,
                );
                ui.end_row();
                ch
            })
        }),
        "Tilemap" => tilemap(ui, state, e),
        "Tilemap Renderer" => {
            let layers = state.project_settings.sorting_layers.clone();
            edit_copy::<TilemapRenderer>(state, e, |r| {
                grid(ui, "tilemap_renderer", |ui| {
                    let mut ch = false;
                    ui.label("Mode");
                    ch |= combo(
                        ui,
                        "tilemap_mode",
                        &mut r.mode,
                        &[TilemapRenderMode::Chunk, TilemapRenderMode::Individual],
                        |m| match m {
                            TilemapRenderMode::Chunk => "Chunk",
                            TilemapRenderMode::Individual => "Individual",
                        },
                    );
                    ui.end_row();
                    ui.label("Material");
                    ui.weak("◈ Sprite-Lit-Default");
                    ui.end_row();
                    ui.label("Sorting Layer");
                    ch |= sorting_layer(ui, "tilemap_layer", &layers, &mut r.sorting_layer);
                    ui.end_row();
                    ui.label("Order in Layer");
                    ch |= ui
                        .add(egui::DragValue::new(&mut r.order_in_layer))
                        .changed();
                    ui.end_row();
                    ch
                })
            })
        }
        "Tilemap Collider 2D" => edit_copy::<TilemapCollider2D>(state, e, |c| {
            grid(ui, "tilemap_collider", |ui| {
                let m = &mut c.common.material;
                let mut ch = number(ui, "Friction", &mut m.friction, 0.01, 0.0, 1e6);
                ch |= number(ui, "Bounciness", &mut m.bounciness, 0.01, 0.0, 1.0);
                ui.label("Is Trigger");
                ch |= ui.checkbox(&mut c.common.is_trigger, "").changed();
                ui.end_row();
                ch |= vec2_row(ui, "Offset", &mut c.common.offset, 0.01);
                ui.label("Used By Composite");
                ch |= ui
                    .checkbox(&mut c.used_by_composite, "")
                    .on_hover_text("Merge neighbouring tiles into as few boxes as possible")
                    .changed();
                ui.end_row();
                ch
            })
        }),
        "Light 2D" => {
            let layers = state.project_settings.sorting_layers.clone();
            edit_copy::<Light2D>(state, e, |l| light_2d(ui, l, &layers))
        }
        "Shadow Caster 2D" => edit_copy::<ShadowCaster2D>(state, e, |c| {
            grid(ui, "shadow_caster", |ui| {
                ui.label("Casting Source");
                ui.weak("Sprite rect / Tilemap cells");
                ui.end_row();
                ui.label("Self Shadows");
                let ch = ui.checkbox(&mut c.self_shadows, "").changed();
                ui.end_row();
                ch
            })
        }),
        _ => false,
    }
}

fn tilemap(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let count = state
        .scene
        .world
        .get::<&Tilemap>(e)
        .map(|m| m.len())
        .unwrap_or(0);
    let mut clear = false;
    ui.horizontal(|ui| {
        if ui.button("Open Tile Palette").clicked() {
            state.tile_palette.target = Some(e);
            state.push(crate::actions::Action::FocusTab(
                crate::layout::Tab::TilePalette,
            ));
        }
        clear = ui
            .add_enabled(count > 0, egui::Button::new("Clear All Tiles"))
            .clicked();
    });
    if clear {
        state.record("Clear All Tiles");
        if let Ok(mut m) = state.scene.world.get::<&mut Tilemap>(e) {
            m.clear();
        }
    }
    let ch = edit_copy::<Tilemap>(state, e, |m| {
        grid(ui, "tilemap_component", |ui| {
            ui.label("Tiles");
            ui.weak(format!("{count}"));
            ui.end_row();
            ui.label("Color");
            let mut ch = color_rgba(ui, &mut m.color);
            ui.end_row();
            ch |= vec2_row(ui, "Tile Anchor", &mut m.tile_anchor, 0.01);
            ch |= number(
                ui,
                "Animation Frame Rate",
                &mut m.animation_frame_rate,
                0.01,
                0.0,
                100.0,
            );
            ch
        })
    });
    ch || clear
}

fn light_2d(ui: &mut Ui, l: &mut Light2D, layers: &[String]) -> bool {
    let mut ch = grid(ui, "light_2d", |ui| {
        let mut ch = false;
        ui.label("Light Type");
        ch |= combo(
            ui,
            "light2d_type",
            &mut l.kind,
            &Light2DType::ALL,
            Light2DType::label,
        );
        ui.end_row();
        ui.label("Color");
        ch |= color_rgb(ui, &mut l.color);
        ui.end_row();
        ch |= number(ui, "Intensity", &mut l.intensity, 0.01, 0.0, 100.0);
        if l.kind != Light2DType::Global {
            ch |= number(ui, "Radius Inner", &mut l.inner_radius, 0.01, 0.0, 1e4);
            ch |= number(ui, "Radius Outer", &mut l.outer_radius, 0.01, 0.0, 1e4);
            l.inner_radius = l.inner_radius.min(l.outer_radius);
            if l.kind == Light2DType::Spot {
                ch |= number(ui, "Inner Spot Angle", &mut l.inner_angle, 0.5, 0.0, 360.0);
                ch |= number(ui, "Outer Spot Angle", &mut l.outer_angle, 0.5, 0.0, 360.0);
                l.inner_angle = l.inner_angle.min(l.outer_angle);
            }
            ui.label("Falloff Strength");
            ch |= ui
                .add(egui::Slider::new(&mut l.falloff_strength, 0.0..=1.0))
                .changed();
            ui.end_row();
        }
        ch
    });
    // Target Sorting Layers: Everything or a chosen list.
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), "Target Sorting Layers"))
        .id_salt("light2d_layers")
        .default_open(false)
        .show(ui, |ui| {
            let mut all = l.target_sorting_layers.is_none();
            if ui.checkbox(&mut all, "Everything").changed() {
                l.target_sorting_layers = if all { None } else { Some(layers.to_vec()) };
                ch = true;
            }
            if let Some(list) = &mut l.target_sorting_layers {
                for name in layers {
                    let mut on = list.contains(name);
                    if ui.checkbox(&mut on, name).changed() {
                        if on {
                            list.push(name.clone());
                        } else {
                            list.retain(|n| n != name);
                        }
                        ch = true;
                    }
                }
            }
        });
    if l.kind != Light2DType::Global {
        egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), "Shadows"))
            .id_salt("light2d_shadows")
            .default_open(l.shadows_enabled)
            .show(ui, |ui| {
                ch |= grid(ui, "light2d_shadow_grid", |ui| {
                    ui.label("Enabled");
                    let mut ch = ui.checkbox(&mut l.shadows_enabled, "").changed();
                    ui.end_row();
                    ui.label("Strength");
                    ch |= ui
                        .add(egui::Slider::new(&mut l.shadow_strength, 0.0..=1.0))
                        .changed();
                    ui.end_row();
                    ui.label("Softness");
                    ch |= ui
                        .add(egui::Slider::new(&mut l.shadow_softness, 0.0..=1.0))
                        .changed();
                    ui.end_row();
                    ch
                });
            });
    }
    ch
}

// ---- assets -------------------------------------------------------------------------------------

/// Editor of a `.tile` asset (saved on every change).
pub fn tile_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let abs = state.assets.absolute(rel);
    let Ok(mut t) = TileAsset::load(&abs) else {
        ui.colored_label(Color32::LIGHT_RED, "Could not read this tile asset.");
        return;
    };
    let name = std::path::Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("▦").size(22.0));
        ui.label(crate::theme::bold(
            ui.ctx(),
            format!("{name} ({})", t.kind.label()),
        ));
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    let choices = sprite_choices(state);
    let mut ch = grid(ui, "tile_asset", |ui| {
        let mut ch = false;
        ui.label("Type");
        ch |= combo(
            ui,
            "tile_kind",
            &mut t.kind,
            &TileKind::ALL,
            TileKind::label,
        );
        ui.end_row();
        if t.kind != TileKind::AnimatedTile {
            ui.label(if t.kind == TileKind::RuleTile {
                "Default Sprite"
            } else {
                "Sprite"
            });
            ch |= sprite_slot(ui, "tile_sprite", &mut t.sprite, &choices);
            ui.end_row();
        }
        ui.label("Color");
        ch |= color_rgba(ui, &mut t.color);
        ui.end_row();
        ui.label(if t.kind == TileKind::RuleTile {
            "Default Collider"
        } else {
            "Collider Type"
        });
        ch |= combo(
            ui,
            "tile_collider",
            &mut t.collider,
            &ColliderType::ALL,
            ColliderType::label,
        );
        ui.end_row();
        if t.kind == TileKind::AnimatedTile {
            ch |= number(ui, "Speed (fps)", &mut t.speed, 0.05, 0.0, 120.0);
            ch |= number(ui, "Start Time", &mut t.start_time, 0.01, 0.0, 1e4);
        }
        ch
    });
    match t.kind {
        TileKind::AnimatedTile => {
            ch |= sprite_list(ui, "anim_frames", "Frames", &mut t.frames, &choices)
        }
        TileKind::RuleTile => ch |= rules(ui, &mut t.rules, &choices),
        TileKind::Tile => {}
    }
    if ch {
        match t.save(&abs) {
            Ok(()) => tile::invalidate(rel),
            Err(e) => state.console.warn(format!("Could not save {rel}: {e}")),
        }
    }
}

fn sprite_list(
    ui: &mut Ui,
    id: &str,
    title: &str,
    list: &mut Vec<String>,
    choices: &[String],
) -> bool {
    let mut ch = false;
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), title))
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            let mut remove = None;
            for (i, s) in list.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{i}"));
                    let mut v = Some(s.clone());
                    if sprite_slot(ui, &format!("{id}_{i}"), &mut v, choices) {
                        match v {
                            Some(v) => *s = v,
                            None => remove = Some(i),
                        }
                        ch = true;
                    }
                    if ui.small_button("✖").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                list.remove(i);
                ch = true;
            }
            let mut new = None;
            ui.horizontal(|ui| {
                ui.label("+");
                if sprite_slot(ui, &format!("{id}_new"), &mut new, choices)
                    && let Some(s) = new
                {
                    list.push(s);
                    ch = true;
                }
            });
        });
    ch
}

/// One clickable 3×3 neighbour grid (centre = the tile itself).
fn neighbor_grid(ui: &mut Ui, n: &mut [Neighbor; 8]) -> bool {
    let size = 18.0;
    let (rect, _) = ui.allocate_exact_size(vec2(size * 3.0, size * 3.0), Sense::hover());
    let mut ch = false;
    let mut i = 0;
    for row in 0..3 {
        for col in 0..3 {
            let r = egui::Rect::from_min_size(
                rect.min + vec2(col as f32 * size, row as f32 * size),
                vec2(size, size),
            )
            .shrink(1.0);
            let painter = ui.painter();
            painter.rect_filled(r, 2.0, Color32::from_gray(45));
            if row == 1 && col == 1 {
                painter.rect_filled(r.shrink(4.0), 1.0, Color32::from_gray(120));
                continue;
            }
            let idx = i;
            i += 1;
            let resp = ui.interact(r, ui.id().with(("ngrid", idx)), Sense::click());
            match n[idx] {
                Neighbor::This => {
                    // Arrow pointing at the neighbour.
                    let c = r.center();
                    let d = vec2(col as f32 - 1.0, row as f32 - 1.0).normalized() * 5.0;
                    painter.arrow(
                        c - d,
                        d * 2.0,
                        Stroke::new(2.0, Color32::from_rgb(80, 220, 100)),
                    );
                }
                Neighbor::NotThis => {
                    let s = Stroke::new(2.0, Color32::from_rgb(235, 80, 70));
                    let r2 = r.shrink(4.0);
                    painter.line_segment([r2.left_top(), r2.right_bottom()], s);
                    painter.line_segment([r2.right_top(), r2.left_bottom()], s);
                }
                Neighbor::DontCare => {}
            }
            if resp.clicked() {
                n[idx] = n[idx].cycle();
                ch = true;
            }
            if resp.secondary_clicked() {
                n[idx] = Neighbor::DontCare;
                ch = true;
            }
        }
    }
    ch
}

fn rules(ui: &mut Ui, rules: &mut Vec<TilingRule>, choices: &[String]) -> bool {
    let mut ch = false;
    ui.add_space(4.0);
    ui.label(crate::theme::bold(ui.ctx(), "Tiling Rules"));
    ui.weak("Click a neighbour: green arrow = this tile, red cross = not this tile.");
    let mut remove = None;
    let mut swap = None;
    let n = rules.len();
    for (i, rule) in rules.iter_mut().enumerate() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ch |= neighbor_grid(ui, &mut rule.neighbors);
                ui.vertical(|ui| {
                    let mut first = rule.sprites.first().cloned();
                    ui.horizontal(|ui| {
                        ui.label("Sprite");
                        if sprite_slot(ui, &format!("rule_sprite_{i}"), &mut first, choices) {
                            match (first, rule.sprites.is_empty()) {
                                (Some(s), true) => rule.sprites.push(s),
                                (Some(s), false) => rule.sprites[0] = s,
                                (None, _) => rule.sprites.clear(),
                            }
                            ch = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Output");
                        ch |= combo(
                            ui,
                            &format!("rule_out_{i}"),
                            &mut rule.output,
                            &RuleOutput::ALL,
                            |o| match o {
                                RuleOutput::Single => "Single",
                                RuleOutput::Random => "Random",
                                RuleOutput::Animation => "Animation",
                            },
                        );
                        ui.label("Transform");
                        ch |= combo(
                            ui,
                            &format!("rule_tr_{i}"),
                            &mut rule.transform,
                            &RuleTransform::ALL,
                            RuleTransform::label,
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Collider");
                        ch |= combo(
                            ui,
                            &format!("rule_col_{i}"),
                            &mut rule.collider,
                            &ColliderType::ALL,
                            ColliderType::label,
                        );
                        if rule.output == RuleOutput::Animation {
                            ui.label("Speed");
                            ch |= ui
                                .add(
                                    egui::DragValue::new(&mut rule.animation_speed)
                                        .speed(0.05)
                                        .range(0.0..=120.0),
                                )
                                .changed();
                        }
                    });
                    if rule.output != RuleOutput::Single {
                        ch |= sprite_list(
                            ui,
                            &format!("rule_sprites_{i}"),
                            "Sprites",
                            &mut rule.sprites,
                            choices,
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui.small_button("✖").on_hover_text("Remove rule").clicked() {
                        remove = Some(i);
                    }
                    if i + 1 < n && ui.small_button("⏷").clicked() {
                        swap = Some((i, i + 1));
                    }
                    if i > 0 && ui.small_button("⏶").clicked() {
                        swap = Some((i - 1, i));
                    }
                });
            });
        });
    }
    if let Some(i) = remove {
        rules.remove(i);
        ch = true;
    }
    if let Some((a, b)) = swap {
        rules.swap(a, b);
        ch = true;
    }
    if ui.button("+ Add Rule").clicked() {
        rules.push(TilingRule::default());
        ch = true;
    }
    ch
}

/// A `.palette` asset: its tiles, and a button to open it in the Tile Palette window.
pub fn palette_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let abs = state.assets.absolute(rel);
    let p = misty_engine::tilemap::palette::TilePalette::load(&abs).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("🎨").size(22.0));
        let name = std::path::Path::new(rel)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        ui.label(crate::theme::bold(
            ui.ctx(),
            format!("{name} (Tile Palette)"),
        ));
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    ui.label(format!("{} tiles", p.tiles.len()));
    if ui.button("Open in Tile Palette").clicked() {
        let root = state.project.root.clone();
        state.tile_palette.open(&root, rel);
        state.push(crate::actions::Action::FocusTab(
            crate::layout::Tab::TilePalette,
        ));
    }
}
