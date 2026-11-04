//! Presets in the editor: apply and save component presets (one undo step for every target),
//! default presets from Project Settings ▸ Preset Manager, and the picker window's state.
//! The UI lives in `panels/inspector/preset_*.rs`; the file format in `misty_engine::preset`.

use misty_engine::hecs::Entity;
use misty_engine::preset::{self, Preset};
use misty_engine::scene::registry;
use serde_json::Value;

use crate::actions::Action;
use crate::state::EditorState;

/// Preset button and `.preset` files in the Project window.
pub const PRESET_ICON: &str = "🎚";

/// What the preset picker applies to.
#[derive(Clone, Debug, PartialEq)]
pub enum PickTarget {
    /// A component on the Inspector's objects.
    Component { name: String, targets: Vec<Entity> },
    /// A material asset (applied and saved right away).
    Material(String),
    /// A texture's import settings (applied to the unapplied edits; Apply writes them).
    Texture(String),
}

impl PickTarget {
    /// The preset target type it lists presets of.
    pub fn kind(&self) -> &str {
        match self {
            PickTarget::Component { name, .. } => name,
            PickTarget::Material(_) => preset::MATERIAL,
            PickTarget::Texture(_) => preset::TEXTURE_IMPORTER,
        }
    }
}

/// What the picker's None restores.
#[derive(Clone, Debug)]
pub enum Backup {
    Component(Vec<(Entity, Value)>),
    /// The asset's settings struct as JSON.
    Asset(Value),
}

/// The open preset picker window.
pub struct PresetPicker {
    pub target: PickTarget,
    /// Screen position of the button that opened it.
    pub anchor: egui::Pos2,
    /// Values when the picker opened: None puts them back.
    pub backup: Backup,
    /// Preset applied from this picker (highlighted in the list).
    pub chosen: Option<String>,
    /// Name typed for "Save current to…".
    pub save_name: String,
}

/// Apply a preset command. Returns the action back if it is not one.
pub fn apply(s: &mut EditorState, action: Action) -> Option<Action> {
    match action {
        Action::ApplyPreset { entities, preset } => apply_preset(s, &entities, &preset),
        Action::RestoreComponentValues { name, values } => restore(s, &name, values),
        Action::SavePreset { entity, name, file } => {
            if let Some(p) = Preset::capture(&s.scene.world, entity, &name) {
                save_new(s, p, &file);
            }
        }
        other => return Some(other),
    }
    None
}

/// Display name of a preset path ("Warm Street Lamp").
pub fn preset_name(rel: &str) -> String {
    crate::assets::AssetDatabase::material_name(Some(rel))
}

/// The objects in `entities` that have component `name`.
fn with_component(s: &EditorState, entities: &[Entity], name: &str) -> Vec<Entity> {
    let Some(info) = registry::find(name) else {
        return Vec::new();
    };
    entities
        .iter()
        .copied()
        .filter(|e| s.scene.contains(*e) && (info.has)(&s.scene.world, *e))
        .collect()
}

fn apply_preset(s: &mut EditorState, entities: &[Entity], rel: &str) {
    let Some(p) = s.assets.presets.get(rel).cloned() else {
        s.console.warn(format!("Missing preset: {rel}"));
        return;
    };
    let targets = with_component(s, entities, &p.target);
    if targets.is_empty() {
        s.console
            .warn(format!("No selected object has a {}.", p.target));
        return;
    }
    s.record(&format!("Apply Preset {}", preset_name(rel)));
    for e in targets {
        if let Err(err) = p.apply(&mut s.scene.world, e, &[]) {
            s.console.warn(format!("Preset {rel}: {err}"));
        }
    }
}

fn restore(s: &mut EditorState, name: &str, values: Vec<(Entity, Value)>) {
    let Some(info) = registry::find(name) else {
        return;
    };
    s.record(&format!("Revert {name} Preset"));
    for (e, v) in values {
        if s.scene.contains(e) && (info.has)(&s.scene.world, e) {
            let _ = (info.from_json)(&mut s.scene.world, e, v);
        }
    }
}

/// Values of component `name` on each target (the picker's None backup).
pub fn snapshot(s: &EditorState, name: &str, targets: &[Entity]) -> Vec<(Entity, Value)> {
    let Some(info) = registry::find(name) else {
        return Vec::new();
    };
    targets
        .iter()
        .filter_map(|e| (info.to_json)(&s.scene.world, *e).map(|v| (*e, v)))
        .collect()
}

/// A free `Assets/Presets/<file>.preset` path (numbered when taken).
pub fn new_preset_path(s: &EditorState, file: &str) -> String {
    let dir = s.assets.absolute(preset::FOLDER);
    let stem = file.trim();
    let stem = if stem.is_empty() { "New Preset" } else { stem };
    let path = crate::asset_ops::unique_path(&dir, stem, "preset");
    s.assets.relative(&path)
}

/// Save `p` as a new preset asset; returns its path.
pub fn save_new(s: &mut EditorState, p: Preset, file: &str) -> Option<String> {
    let rel = new_preset_path(s, file);
    match s.assets.save_preset(&rel, p) {
        Ok(()) => {
            s.project.refresh();
            s.console.info(format!("Saved preset {rel}"));
            Some(rel)
        }
        Err(e) => {
            s.console.warn(format!("Could not save {rel}: {e}"));
            None
        }
    }
}

/// "Light Preset", the suggested name for a new preset.
pub fn suggested_name(kind: &str) -> String {
    format!("{kind} Preset")
}

/// The Preset Manager's default preset of `target`, if it exists and matches.
pub fn default_for(s: &EditorState, target: &str) -> Option<Preset> {
    let rel = s.project_settings.default_presets.get(target)?;
    s.assets
        .presets
        .get(rel)
        .filter(|p| p.target == target)
        .cloned()
}

/// Apply the default preset of component `name` to `e` (on add, create and Reset).
/// The type a Create menu item chose (a Light's kind) is kept. True when one was applied.
pub fn apply_default(s: &mut EditorState, e: Entity, name: &str) -> bool {
    let Some(p) = default_for(s, name) else {
        return false;
    };
    match p.apply(&mut s.scene.world, e, preset::identity_fields(name)) {
        Ok(()) => true,
        Err(err) => {
            s.console.warn(format!("Default preset of {name}: {err}"));
            false
        }
    }
}

/// Default presets for every component of a newly created object (not its Transform, which
/// the Create command places).
pub fn apply_defaults(s: &mut EditorState, e: Entity) {
    for name in s.scene.component_names(e) {
        if name != "Transform" {
            apply_default(s, e, &name);
        }
    }
}

/// Default Material preset on a material made with Assets ▸ Create ▸ Material.
pub fn apply_default_material(s: &mut EditorState, rel: &str) {
    let (Some(p), Some(m)) = (
        default_for(s, preset::MATERIAL),
        s.assets.materials.get(rel).cloned(),
    ) else {
        return;
    };
    match p.apply_value(&m) {
        Ok(m) => {
            if let Err(e) = s.assets.update_material(rel, m) {
                s.console.warn(format!("Could not save {rel}: {e}"));
            }
        }
        Err(e) => s.console.warn(format!("Default Material preset: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use misty_engine::scene::{Light, LightType, SphereCollider};

    use super::*;
    use crate::project::ProjectBrowser;

    fn state(tag: &str) -> EditorState {
        let root = std::env::temp_dir().join(format!("misty_presets_{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        EditorState::new(ProjectBrowser::open(root))
    }

    fn run(s: &mut EditorState, a: Action) {
        assert!(crate::commands::apply(s, a).is_none());
    }

    fn lamp(s: &mut EditorState) -> String {
        let src = s.scene.spawn("Lamp", None);
        s.scene.add(
            src,
            Light {
                intensity: 4.0,
                color: [1.0, 0.6, 0.3],
                ..Default::default()
            },
        );
        run(
            s,
            Action::SavePreset {
                entity: src,
                name: "Light".into(),
                file: "Lamp".into(),
            },
        );
        "Assets/Presets/Lamp.preset".to_owned()
    }

    #[test]
    fn save_and_apply_to_every_target_in_one_undo_step() {
        let mut s = state("apply");
        let rel = lamp(&mut s);
        assert!(s.assets.absolute(&rel).exists());
        assert_eq!(s.assets.presets_for("Light"), [rel.clone()]);
        // A second save gets a numbered name.
        assert_eq!(new_preset_path(&s, "Lamp"), "Assets/Presets/Lamp 1.preset");

        let (a, b, c) = (
            s.scene.spawn("A", None),
            s.scene.spawn("B", None),
            s.scene.spawn("C", None),
        );
        s.scene.add(a, Light::directional());
        s.scene.add(b, Light::default());
        s.scene.add(c, SphereCollider::default());
        run(
            &mut s,
            Action::ApplyPreset {
                entities: vec![a, b, c],
                preset: rel,
            },
        );
        for e in [a, b] {
            let l = s.scene.world.get::<&Light>(e).unwrap();
            assert_eq!((l.intensity, l.kind), (4.0, LightType::Point));
        }
        assert!(s.scene.world.get::<&Light>(c).is_err());
        run(&mut s, Action::Undo);
        assert_eq!(
            s.scene.world.get::<&Light>(a).unwrap().kind,
            LightType::Directional
        );
        assert_eq!(s.scene.world.get::<&Light>(b).unwrap().intensity, 1.0);
    }

    #[test]
    fn restore_puts_the_picker_backup_back() {
        let mut s = state("restore");
        let rel = lamp(&mut s);
        let e = s.scene.spawn("E", None);
        s.scene.add(e, Light::default());
        let backup = snapshot(&s, "Light", &[e]);
        run(
            &mut s,
            Action::ApplyPreset {
                entities: vec![e],
                preset: rel,
            },
        );
        run(
            &mut s,
            Action::RestoreComponentValues {
                name: "Light".into(),
                values: backup,
            },
        );
        assert_eq!(s.scene.world.get::<&Light>(e).unwrap().intensity, 1.0);
    }

    #[test]
    fn default_preset_applies_on_add_create_and_reset() {
        let mut s = state("defaults");
        let rel = lamp(&mut s);
        s.project_settings
            .default_presets
            .insert("Light".into(), rel);

        // Add Component.
        let e = s.scene.spawn("E", None);
        s.select(e);
        run(&mut s, Action::AddComponent("Light"));
        assert_eq!(s.scene.world.get::<&Light>(e).unwrap().intensity, 4.0);

        // Create ▸ Directional Light keeps its type.
        run(
            &mut s,
            Action::Create {
                what: crate::actions::Create::Light(LightType::Directional),
                parent: None,
            },
        );
        let sun = s.selection.active().unwrap();
        let l = (*s.scene.world.get::<&Light>(sun).unwrap()).clone();
        assert_eq!((l.kind, l.intensity), (LightType::Directional, 4.0));

        // Reset goes back to the default preset, not the built-in defaults.
        s.scene.world.get::<&mut Light>(e).unwrap().intensity = 9.0;
        run(
            &mut s,
            Action::ResetComponent {
                entities: vec![e],
                name: "Light".into(),
            },
        );
        assert_eq!(s.scene.world.get::<&Light>(e).unwrap().intensity, 4.0);

        // A missing or mismatched preset is ignored.
        s.project_settings
            .default_presets
            .insert("Camera".into(), "Assets/Presets/Lamp.preset".into());
        assert!(default_for(&s, "Camera").is_none());
    }

    #[test]
    fn default_material_preset_on_create() {
        let mut s = state("material");
        let mut m = misty_engine::assets::Material::default();
        m.metallic = 0.8;
        let rel = save_new(&mut s, Preset::of_value(preset::MATERIAL, &m), "Shiny").unwrap();
        s.project_settings
            .default_presets
            .insert(preset::MATERIAL.into(), rel);
        let dir = s.assets.absolute("Assets");
        let mat = s.assets.create_material(&dir).unwrap();
        apply_default_material(&mut s, &mat);
        assert_eq!(s.assets.materials[&mat].metallic, 0.8);
    }
}
