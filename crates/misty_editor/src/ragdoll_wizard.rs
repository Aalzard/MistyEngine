//! GameObject ▸ 3D Object ▸ Ragdoll…: the Ragdoll Wizard window. Pick the character, check the
//! bones (auto-filled from its humanoid Avatar or bone names), set the total mass, strength and
//! facing, and Create: colliders, rigidbodies, Character Joints and a Ragdoll component (one undo
//! step). The work is `misty_engine::physics::ragdoll::create`.

use std::cell::RefCell;

use misty_engine::hecs::Entity;
use misty_engine::physics::ragdoll::{
    self, LABELS, REQUIRED, RagdollBones, RagdollOptions, auto_fill,
};

use crate::state::EditorState;

#[derive(Clone, Default)]
struct Wizard {
    root: Option<Entity>,
    bones: RagdollBones,
    options: RagdollOptions,
    /// Last problem (shown in red).
    error: Option<String>,
}

thread_local! {
    static WIZARD: RefCell<Option<Wizard>> = const { RefCell::new(None) };
}

/// The top object of `e`'s hierarchy that looks like a character (an Animator or a Skinned
/// Mesh Renderer below it), else `e` itself.
fn character_root(state: &EditorState, e: Entity) -> Entity {
    let s = &state.scene;
    let mut cur = Some(e);
    let mut best = e;
    while let Some(c) = cur {
        if s.world.get::<&misty_engine::scene::Animator>(c).is_ok() {
            best = c;
        }
        cur = s.parent(c);
    }
    best
}

/// Open the wizard for `root` (or the selected object's character).
pub fn open(state: &mut EditorState, root: Option<Entity>) {
    let root = root
        .or_else(|| state.selection.active())
        .map(|e| character_root(state, e));
    let bones = root.map(|r| auto_fill(&state.scene, r)).unwrap_or_default();
    WIZARD.with(|w| {
        *w.borrow_mut() = Some(Wizard {
            root,
            bones,
            options: RagdollOptions::default(),
            error: None,
        })
    });
}

#[cfg(test)]
pub fn is_open() -> bool {
    WIZARD.with(|w| w.borrow().is_some())
}

#[cfg(test)]
pub fn close() {
    WIZARD.with(|w| *w.borrow_mut() = None);
}

/// Objects below `root` (bone choices), with their paths.
fn descendants(state: &EditorState, root: Entity) -> Vec<(Entity, String)> {
    let s = &state.scene;
    s.walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| *e != root && s.is_ancestor(root, *e))
        .map(|e| (e, s.name(e)))
        .collect()
}

/// Run the wizard with its current settings (one undo step). Returns the error, if any.
fn create(state: &mut EditorState, w: &Wizard) -> Result<usize, String> {
    let root = w.root.ok_or("Pick the character first.")?;
    let missing = w.bones.missing(&state.scene);
    if !missing.is_empty() {
        return Err(format!("Missing bones: {}", missing.join(", ")));
    }
    state.record("Create Ragdoll");
    let report = ragdoll::create(&mut state.scene, root, &w.bones, &w.options)?;
    Ok(report.bodies.len())
}

/// Draw the window while open.
pub fn window(ctx: &egui::Context, state: &mut EditorState) {
    let Some(mut w) = WIZARD.with(|w| w.borrow().clone()) else {
        return;
    };
    let mut open = true;
    let mut done = false;
    egui::Window::new("Ragdoll Wizard")
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.weak(
                "Bodies, colliders and Character Joints on the character's bones. \
                 The bones fill in from the Animator's Avatar or the bone names.",
            );
            ui.horizontal(|ui| {
                ui.label("Character");
                let name = w
                    .root
                    .filter(|r| state.scene.contains(*r))
                    .map_or("None".to_owned(), |r| state.scene.name(r));
                ui.strong(name);
                if ui.button("Use Selection").clicked()
                    && let Some(e) = state.selection.active()
                {
                    let r = character_root(state, e);
                    w.root = Some(r);
                    w.bones = auto_fill(&state.scene, r);
                }
                if ui.button("Auto-fill").clicked()
                    && let Some(r) = w.root
                {
                    w.bones = auto_fill(&state.scene, r);
                }
            });
            ui.separator();
            let choices = w
                .root
                .filter(|r| state.scene.contains(*r))
                .map(|r| descendants(state, r))
                .unwrap_or_default();
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    egui::Grid::new("ragdoll_bones")
                        .num_columns(2)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            for (slot, label) in LABELS.iter().enumerate() {
                                if slot < REQUIRED {
                                    ui.label(*label);
                                } else {
                                    ui.label(format!("{label} (optional)"));
                                }
                                let mut v = w.bones.get(slot);
                                let name = v
                                    .and_then(|e| choices.iter().find(|(c, _)| *c == e))
                                    .map_or("None".to_owned(), |(_, n)| n.clone());
                                egui::ComboBox::from_id_salt(("ragdoll_slot", slot))
                                    .selected_text(name)
                                    .width(180.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut v, None, "None");
                                        for (e, n) in &choices {
                                            ui.selectable_value(&mut v, Some(*e), n);
                                        }
                                    });
                                w.bones.set(slot, v);
                                ui.end_row();
                            }
                        });
                });
            ui.separator();
            egui::Grid::new("ragdoll_options")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Total Mass");
                    ui.add(
                        egui::DragValue::new(&mut w.options.total_mass)
                            .speed(0.5)
                            .range(0.1..=10000.0)
                            .suffix(" kg"),
                    );
                    ui.end_row();
                    ui.label("Strength");
                    ui.add(
                        egui::DragValue::new(&mut w.options.strength)
                            .speed(5.0)
                            .range(0.0..=1e7),
                    )
                    .on_hover_text("Break force of the joints (0 = unbreakable)");
                    ui.end_row();
                    ui.label("Flip Forward");
                    ui.checkbox(&mut w.options.flip_forward, "")
                        .on_hover_text("The character faces -Z");
                    ui.end_row();
                });
            if let Some(err) = &w.error {
                ui.colored_label(crate::theme::AXIS_X, err);
            }
            ui.horizontal(|ui| {
                if ui.button("Create").clicked() {
                    match create(state, &w) {
                        Ok(n) => {
                            state.console.info(format!("Ragdoll created: {n} bodies."));
                            done = true;
                        }
                        Err(e) => w.error = Some(e),
                    }
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
        });
    WIZARD.with(|cell| {
        *cell.borrow_mut() = (open && !done).then_some(w);
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::scene::{CharacterJoint, Ragdoll, Rigidbody, Transform};

    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    /// A bones-only humanoid like the engine's test skeleton.
    fn character(s: &mut EditorState) -> misty_engine::hecs::Entity {
        let scene = &mut s.scene;
        let root = scene.spawn("Hero", None);
        scene.add(root, misty_engine::scene::Animator::default());
        let bone = |sc: &mut misty_engine::scene::Scene, n: &str, p, at: glam::Vec3| {
            let e = sc.spawn(n, Some(p));
            sc.set_transform(e, Transform::from_position(at));
            e
        };
        let hips = bone(scene, "Hips", root, glam::Vec3::new(0.0, 1.0, 0.0));
        let spine = bone(scene, "Spine", hips, glam::Vec3::new(0.0, 0.2, 0.0));
        let chest = bone(scene, "Chest", spine, glam::Vec3::new(0.0, 0.2, 0.0));
        let neck = bone(scene, "Neck", chest, glam::Vec3::new(0.0, 0.2, 0.0));
        bone(scene, "Head", neck, glam::Vec3::new(0.0, 0.1, 0.0));
        for (side, x) in [("Left", -1.0), ("Right", 1.0)] {
            let a = bone(
                scene,
                &format!("{side}UpperArm"),
                chest,
                glam::Vec3::new(0.2 * x, 0.15, 0.0),
            );
            let b = bone(
                scene,
                &format!("{side}LowerArm"),
                a,
                glam::Vec3::new(0.0, -0.28, 0.0),
            );
            bone(
                scene,
                &format!("{side}Hand"),
                b,
                glam::Vec3::new(0.0, -0.26, 0.0),
            );
            let t = bone(
                scene,
                &format!("{side}UpperLeg"),
                hips,
                glam::Vec3::new(0.1 * x, -0.05, 0.0),
            );
            let c = bone(
                scene,
                &format!("{side}LowerLeg"),
                t,
                glam::Vec3::new(0.0, -0.45, 0.0),
            );
            bone(
                scene,
                &format!("{side}Foot"),
                c,
                glam::Vec3::new(0.0, -0.42, 0.0),
            );
        }
        root
    }

    #[test]
    fn the_wizard_fills_the_bones_and_creates_the_ragdoll() {
        let mut s = EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_ragdoll_wizard_test"),
        ));
        let root = character(&mut s);
        let hips = s.scene.find_by_name("Hips").unwrap();
        s.selection.set(hips);
        super::open(&mut s, None);
        let mut h = Harness::builder()
            .with_size([520.0, 900.0])
            .build_ui_state(|ui, s: &mut EditorState| super::window(ui.ctx(), s), s);
        h.run();
        for label in [
            "Pelvis",
            "Left Knee",
            "Middle Spine",
            "Head",
            "Total Mass",
            "Strength",
        ] {
            h.get_by_label(label);
        }
        // Filled from the names; the character is the Animator's object above the selection.
        h.get_by_label("Hero");
        let knee = super::WIZARD.with(|w| w.borrow().as_ref().and_then(|w| w.bones.get(2)));
        assert_eq!(
            knee.map(|k| h.state().scene.name(k)).as_deref(),
            Some("LeftLowerLeg")
        );
        h.get_by_label("Create").click();
        h.run();
        let s = h.state();
        assert!(!super::is_open(), "closes after creating");
        assert!(s.scene.world.get::<&Ragdoll>(root).is_ok());
        let knee = s.scene.find_by_name("LeftLowerLeg").unwrap();
        assert!(s.scene.world.get::<&Rigidbody>(knee).is_ok());
        assert!(s.scene.world.get::<&CharacterJoint>(knee).is_ok());
        assert_eq!(s.undo.undo_label(), Some("Create Ragdoll"));
    }

    #[test]
    fn missing_bones_are_reported_not_created() {
        let mut s = EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_ragdoll_wizard_test2"),
        ));
        let root = character(&mut s);
        super::open(&mut s, Some(root));
        super::WIZARD.with(|w| {
            if let Some(w) = w.borrow_mut().as_mut() {
                w.bones.set(12, None);
            }
        });
        let mut h = Harness::builder()
            .with_size([520.0, 900.0])
            .build_ui_state(|ui, s: &mut EditorState| super::window(ui.ctx(), s), s);
        h.run();
        h.get_by_label("Create").click();
        h.run();
        h.get_by_label("Missing bones: Head");
        assert!(super::is_open());
        assert!(h.state().scene.world.get::<&Ragdoll>(root).is_err());
        super::close();
    }
}
