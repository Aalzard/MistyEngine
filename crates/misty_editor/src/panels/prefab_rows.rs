//! Prefab details of Hierarchy rows: blue names for instances, red ones for instances whose
//! prefab is missing, a "+" badge on objects an instance added, the ▸ arrow on instance roots
//! that opens Prefab Mode, and the row menu's Prefab submenu.

use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, FontId, Rect, Sense, Ui, pos2, vec2};
use misty_engine::hecs::Entity;
use misty_engine::scene::prefab::Override;

use crate::actions::Action;
use crate::state::EditorState;
use crate::theme;

type AddedCache = HashMap<(Entity, String), (std::time::Instant, HashSet<Entity>)>;

thread_local! {
    /// Added objects per instance root, recent results only.
    static ADDED: std::cell::RefCell<AddedCache> = std::cell::RefCell::new(HashMap::new());
}

/// How a row relates to a prefab.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowPrefab {
    /// Part of an instance (root or child).
    pub instance: bool,
    /// Its instance's prefab file is missing.
    pub missing: bool,
    /// The row is an instance root.
    pub root: bool,
    /// An object the instance added (not in the prefab).
    pub added: bool,
}

/// Per-frame facts about the instances shown in the Hierarchy (built as rows ask).
#[derive(Default)]
pub struct Rows {
    /// Asset exists, by path.
    exists: HashMap<String, bool>,
    /// For each outermost instance root: the instance objects the prefab doesn't have.
    added: HashMap<Entity, HashSet<Entity>>,
}

impl Rows {
    pub fn info(&mut self, s: &mut EditorState, e: Entity) -> RowPrefab {
        let roots = s.scene.prefab_roots(e);
        let Some(&outer) = roots.last() else {
            return RowPrefab::default();
        };
        let mut missing = false;
        for r in &roots {
            if let Some(path) = s.scene.prefab_link(*r) {
                missing |= !self.exists(s, &path);
            }
        }
        let added = !missing && e != outer && self.added_in(s, outer).contains(&e);
        RowPrefab {
            instance: true,
            missing,
            root: roots.first() == Some(&e),
            added,
        }
    }

    fn exists(&mut self, s: &mut EditorState, path: &str) -> bool {
        if let Some(x) = self.exists.get(path) {
            return *x;
        }
        let x = s.assets.prefab(path).is_ok();
        self.exists.insert(path.to_owned(), x);
        x
    }

    /// Tops of the subtrees the instance at `root` added (worked out at most twice a second
    /// per instance: comparing a big instance with its prefab every frame would cost).
    fn added_in(&mut self, s: &mut EditorState, root: Entity) -> &HashSet<Entity> {
        if !self.added.contains_key(&root) {
            let key = (root, s.scene.name.clone());
            let cached = ADDED.with(|c| {
                c.borrow()
                    .get(&key)
                    .filter(|(t, _)| t.elapsed().as_secs_f32() < 0.5)
                    .map(|(_, set)| set.clone())
            });
            let set = cached.unwrap_or_else(|| {
                let mut set = HashSet::new();
                if let Some(items) = crate::prefabs::overrides_of(s, root) {
                    for o in items {
                        if let (Override::AddedObject { .. }, Some(e)) = (o.key, o.entity) {
                            set.insert(e);
                        }
                    }
                }
                ADDED.with(|c| {
                    let mut c = c.borrow_mut();
                    c.retain(|_, (t, _)| t.elapsed().as_secs_f32() < 5.0);
                    c.insert(key, (std::time::Instant::now(), set.clone()));
                });
                set
            });
            self.added.insert(root, set);
        }
        &self.added[&root]
    }
}

/// Name colour of an instance row (selected rows are drawn white by the caller).
pub fn name_color(info: RowPrefab, active: bool) -> Option<Color32> {
    if !info.instance {
        return None;
    }
    Some(match (info.missing, active) {
        (true, true) => theme::MISSING_PREFAB,
        (true, false) => theme::MISSING_PREFAB.gamma_multiply(0.6),
        (false, true) => theme::PREFAB,
        (false, false) => theme::PREFAB_DIM,
    })
}

/// Badges drawn on a row: "+" on the icon of an added object, the ▸ arrow at the right of an
/// instance root (opens Prefab Mode). `icon_x` is where the icon starts.
pub fn badges(
    ui: &mut Ui,
    s: &mut EditorState,
    e: Entity,
    info: RowPrefab,
    rect: Rect,
    icon_x: f32,
) {
    if info.added {
        let c = pos2(icon_x + 1.0, rect.top() + 5.0);
        ui.painter().circle_filled(c, 4.5, theme::OVERRIDE);
        ui.painter().text(
            c,
            Align2::CENTER_CENTER,
            "+",
            FontId::proportional(10.0),
            Color32::WHITE,
        );
    }
    if info.missing && info.root {
        ui.painter().text(
            pos2(rect.right() - 6.0, rect.center().y),
            Align2::RIGHT_CENTER,
            "Missing Prefab",
            FontId::proportional(11.0),
            theme::MISSING_PREFAB,
        );
        return;
    }
    if !info.root {
        return;
    }
    let Some(path) = s.scene.prefab_link(e) else {
        return;
    };
    if crate::prefabs::read_only(&path) {
        return;
    }
    let arrow = Rect::from_min_size(
        pos2(rect.right() - 18.0, rect.top()),
        vec2(16.0, rect.height()),
    );
    let resp = ui
        .interact(arrow, egui::Id::new(("prefab_open", e)), Sense::click())
        .on_hover_text(format!(
            "Open {} in Prefab Mode",
            crate::prefabs::display_name(&path)
        ));
    let color = if resp.hovered() {
        Color32::WHITE
    } else {
        theme::PREFAB
    };
    ui.painter().text(
        arrow.center(),
        Align2::CENTER_CENTER,
        "›",
        FontId::proportional(16.0),
        color,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Open Prefab"));
    if resp.clicked() {
        s.push(Action::OpenPrefab {
            path,
            from: Some(e),
        });
    }
}

/// The row menu's Prefab submenu for an object that is part of an instance.
pub fn menu(ui: &mut Ui, s: &mut EditorState, e: Entity) {
    let roots = s.scene.prefab_roots(e);
    let (Some(&near), Some(&outer)) = (roots.first(), roots.last()) else {
        return;
    };
    let Some(path) = s.scene.prefab_link(near) else {
        return;
    };
    let name = crate::prefabs::display_name(&path);
    let missing = s.assets.prefab(&path).is_err();
    let editable = !missing && !crate::prefabs::read_only(&path);
    ui.separator();
    ui.menu_button("Prefab", |ui| {
        let mut act = |ui: &mut Ui, label: &str, enabled: bool, action: Action| {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                s.push(action);
                ui.close();
            }
        };
        act(
            ui,
            &format!("Open {name}"),
            editable,
            Action::OpenPrefab {
                path: path.clone(),
                from: Some(near),
            },
        );
        act(
            ui,
            "Select Asset",
            !missing,
            Action::SelectPrefabAsset(near),
        );
        ui.separator();
        // Only an outermost instance can be unpacked (a nested one belongs to its prefab).
        act(
            ui,
            "Unpack Prefab",
            outer == near,
            Action::UnpackPrefab(near),
        );
        act(
            ui,
            "Unpack Prefab Completely",
            outer == near,
            Action::UnpackPrefabCompletely(near),
        );
        // An object the instance added: apply it to the prefab or remove it.
        if let Some(items) = crate::prefabs::overrides_of(s, outer)
            && let Some(item) = items
                .into_iter()
                .find(|o| matches!(o.key, Override::AddedObject { .. }) && o.entity == Some(e))
        {
            let outer_path = s.scene.prefab_link(outer).unwrap_or_default();
            let keys = vec![item.key];
            ui.separator();
            let label = format!(
                "Apply Added GameObject to Prefab '{}'",
                crate::prefabs::display_name(&outer_path)
            );
            if ui
                .add_enabled(
                    !crate::prefabs::read_only(&outer_path),
                    egui::Button::new(label),
                )
                .clicked()
            {
                s.push(Action::ApplyPrefabOverrides {
                    root: outer,
                    keys: keys.clone(),
                });
                ui.close();
            }
            if ui.button("Revert Added GameObject").clicked() {
                s.push(Action::RevertPrefabOverrides { root: outer, keys });
                ui.close();
            }
        }
    });
}
