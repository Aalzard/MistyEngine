//! Physics Material assets: the asset Inspector (frictions, bounciness, combine modes) and the
//! Material slot of the colliders.

use std::cell::RefCell;
use std::time::Instant;

use egui::{RichText, Ui};
use misty_engine::physics::material::{Combine, EXTENSION, PhysicsMaterial, is_physics_material};

use crate::panels::AssetPayload;
use crate::state::EditorState;

pub const ICON: &str = "◍";

thread_local! {
    /// Project Physics Materials found by walking `Assets/` (only without a GUID database),
    /// re-listed every couple of seconds.
    static LIST: RefCell<Option<(Instant, Vec<String>)>> = const { RefCell::new(None) };
}

/// Every `.physicmat` under Assets (project-relative paths, sorted). The open GUID database
/// already lists every asset, so this touches no disk: every collider's Inspector asks each
/// frame, and walking `Assets/` here froze the editor every two seconds (100 ms for 1800 files
/// on an SSD, many seconds on a busy hard disk). Without a database the folder is walked.
pub fn project_materials() -> Vec<String> {
    let listed = misty_engine::asset_db::with(|db| {
        db.paths()
            .filter(|p| is_physics_material(p))
            .map(str::to_owned)
            .collect::<Vec<String>>()
    });
    if let Some(mut out) = listed {
        out.sort();
        return out;
    }
    LIST.with(|l| {
        let mut l = l.borrow_mut();
        if let Some((t, list)) = l.as_ref()
            && t.elapsed().as_secs_f32() < 2.0
        {
            return list.clone();
        }
        let mut out = Vec::new();
        let mut stack = vec![misty_engine::assets::absolute("Assets")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if is_physics_material(&p.to_string_lossy()) {
                    out.push(misty_engine::assets::relative(&p).replace('\\', "/"));
                }
            }
        }
        out.sort();
        *l = Some((Instant::now(), out.clone()));
        out
    })
}

/// Forget the cached list (a material was just created).
pub fn refresh_list() {
    LIST.with(|l| *l.borrow_mut() = None);
}

fn short(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(&format!(".{EXTENSION}"))
        .to_owned()
}

/// Material slot: "None (Physics Material)", a project material, or one dropped on it.
pub fn slot(ui: &mut Ui, id: &str, value: &mut Option<String>) -> bool {
    let mut ch = false;
    let none = "None (Physics Material)";
    let list = project_materials();
    let r = egui::ComboBox::from_id_salt(id)
        .selected_text(match value {
            Some(p) => format!("{ICON} {}", short(p)),
            None => none.to_owned(),
        })
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, none).changed();
            for p in &list {
                ch |= ui
                    .selectable_value(value, Some(p.clone()), short(p))
                    .changed();
            }
        })
        .response;
    if let Some(p) = r.dnd_release_payload::<AssetPayload>()
        && is_physics_material(&p.0)
    {
        *value = Some(p.0.clone());
        ch = true;
    }
    ch
}

fn combine(ui: &mut Ui, label: &str, id: &str, v: &mut Combine) -> bool {
    ui.label(label);
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(v.name())
        .show_ui(ui, |ui| {
            for c in Combine::ALL {
                ch |= ui.selectable_value(v, c, c.name()).changed();
            }
        });
    ui.end_row();
    ch
}

/// Inspector of a `.physicmat` asset; changes are saved right away.
pub fn physics_material_asset(ui: &mut Ui, _state: &mut EditorState, rel: &str) {
    let path = misty_engine::assets::absolute(rel);
    let mut m = match PhysicsMaterial::load(&path) {
        Ok(m) => m,
        Err(e) => {
            ui.colored_label(crate::theme::AXIS_X, e);
            return;
        }
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(ICON).size(22.0));
        ui.label(crate::theme::bold(
            ui.ctx(),
            format!("{} (Physics Material)", short(rel)),
        ));
    });
    ui.separator();
    crate::asset_guids::path_label(ui, rel);
    let ch = super::card::grid(ui, "physmat_grid", |ui| {
        let mut ch = super::physics_editors::number(
            ui,
            "Dynamic Friction",
            &mut m.dynamic_friction,
            0.01,
            0.0,
            10.0,
        );
        ch |= super::physics_editors::number(
            ui,
            "Static Friction",
            &mut m.static_friction,
            0.01,
            0.0,
            10.0,
        );
        ch |= super::physics_editors::number(ui, "Bounciness", &mut m.bounciness, 0.01, 0.0, 1.0);
        ch |= combine(
            ui,
            "Friction Combine",
            "physmat_fc",
            &mut m.friction_combine,
        );
        ch |= combine(ui, "Bounce Combine", "physmat_bc", &mut m.bounce_combine);
        ch
    });
    ui.weak(
        "Static friction holds resting bodies; once they slide, dynamic friction applies. \
         Of the two colliders' combine modes the higher one wins: \
         Average < Minimum < Multiply < Maximum.",
    );
    if ch && let Err(e) = m.save(&path) {
        ui.colored_label(crate::theme::AXIS_X, e);
    }
}

/// Assets ▸ Create ▸ Physics Material: a new material file in `dir`.
pub fn create(dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let path = crate::asset_ops::unique_path(dir, "New Physics Material", EXTENSION);
    PhysicsMaterial::default().save(&path)?;
    refresh_list();
    Ok(path)
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    #[test]
    fn physics_material_inspector_edits_and_saves() {
        let root = std::env::temp_dir().join(format!("misty_physmat_insp_{}", std::process::id()));
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        let s = EditorState::new(ProjectBrowser::open(root.clone()));
        let path = super::create(&root.join("Assets")).unwrap();
        // (Other tests move the global project root, so use the full path.)
        let rel = path.to_string_lossy().replace('\\', "/");
        let mut h = Harness::builder().with_size([420.0, 500.0]).build_ui_state(
            move |ui, s: &mut EditorState| super::physics_material_asset(ui, s, &rel),
            s,
        );
        h.run();
        for label in [
            "Dynamic Friction",
            "Static Friction",
            "Bounciness",
            "Friction Combine",
            "Bounce Combine",
        ] {
            h.get_by_label(label);
        }
        // Drawing it leaves the file as it was.
        let m = misty_engine::physics::material::PhysicsMaterial::load(&path).unwrap();
        assert_eq!(
            m,
            misty_engine::physics::material::PhysicsMaterial::default()
        );
    }
}
