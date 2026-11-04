//! Hierarchy folder colours: Alt+right-click a folder row for a pastel colour wheel (hue
//! around the ring, lighter towards the centre) and a None reset. Undoable; stored in the
//! folder's `HierarchyFolder` component.

use egui::{Color32, Id, Pos2, Rect, Sense, Shape, Ui, epaint::Mesh, pos2, vec2};
use misty_engine::hecs::Entity;
use misty_engine::scene::HierarchyFolder;

use crate::state::EditorState;

const WHEEL: f32 = 132.0;
/// Inner hole of the ring (fraction of the radius) where the current colour shows.
const HOLE: f32 = 0.38;
/// Pastel range: fixed soft saturation, lightness from the rim (deeper) to the hole (paler).
const SATURATION: f32 = 0.58;
const LIGHT_RIM: f32 = 0.72;
const LIGHT_HOLE: f32 = 0.88;

/// The pastel at `hue` (0..1, around the ring) and `t` (0 = rim, 1 = inner edge), as sRGB 0..1.
pub fn pastel(hue: f32, t: f32) -> [f32; 3] {
    let l = LIGHT_RIM + (LIGHT_HOLE - LIGHT_RIM) * t.clamp(0.0, 1.0);
    hsl_to_rgb(hue.rem_euclid(1.0), SATURATION, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let k = |n: f32| (n + h * 12.0) % 12.0;
    let f = |n: f32| l - c * 0.5 * (k(n) - 3.0).min(9.0 - k(n)).clamp(-1.0, 1.0);
    [f(0.0), f(8.0), f(4.0)]
}

pub fn to_color32(c: [f32; 3]) -> Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(b(c[0]), b(c[1]), b(c[2]))
}

/// The folder's colour, if it is a folder with one.
pub fn color_of(state: &EditorState, e: Entity) -> Option<Color32> {
    let f = state.scene.world.get::<&HierarchyFolder>(e).ok()?;
    f.color.map(to_color32)
}

/// Row background for a coloured folder: a soft wash so the text stays readable.
pub fn row_tint(c: Color32) -> Color32 {
    c.gamma_multiply(0.22)
}

/// Set (or clear) a folder's colour as one undo step.
pub fn set_color(state: &mut EditorState, e: Entity, color: Option<[f32; 3]>) {
    let current = state
        .scene
        .world
        .get::<&HierarchyFolder>(e)
        .ok()
        .map(|f| f.color);
    if current.is_none() || current == Some(color) {
        return;
    }
    state.record(&format!("Folder Color {}", state.scene.name(e)));
    if let Ok(mut f) = state.scene.world.get::<&mut HierarchyFolder>(e) {
        f.color = color;
    }
}

/// The open picker (if any), drawn above everything; closes on a pick, Escape or a click outside.
pub fn show_picker(ui: &Ui, state: &mut EditorState) {
    let Some((e, at)) = state.hierarchy.folder_picker else {
        return;
    };
    if !state.scene.contains(e) {
        state.hierarchy.folder_picker = None;
        return;
    }
    let current = color_of(state, e);
    let mut pick: Option<Option<[f32; 3]>> = None;
    let area = egui::Area::new(Id::new("hier_folder_picker"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label("Folder Color");
                if let Some(c) = wheel(ui, current) {
                    pick = Some(Some(c));
                }
                if ui.button("None").clicked() {
                    pick = Some(None);
                }
            });
        });
    let outside = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .press_origin()
                .is_some_and(|p| !area.response.rect.contains(p))
    });
    if let Some(color) = pick {
        set_color(state, e, color);
        state.hierarchy.folder_picker = None;
    } else if outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.hierarchy.folder_picker = None;
    }
}

/// Pastel ring: returns the colour clicked.
fn wheel(ui: &mut Ui, current: Option<Color32>) -> Option<[f32; 3]> {
    let (rect, resp) = ui.allocate_exact_size(vec2(WHEEL, WHEEL), Sense::click());
    let c = rect.center();
    let r = WHEEL * 0.5 - 2.0;
    ui.painter().add(Shape::mesh(ring_mesh(c, r)));
    // Current colour (or the panel colour for None) in the hole.
    let hole = current.unwrap_or(ui.visuals().panel_fill);
    ui.painter().circle_filled(c, r * HOLE - 3.0, hole);
    let hit = resp.interact_pointer_pos().or(resp.hover_pos())?;
    let (hue, t) = at(c, r, hit)?;
    ui.painter()
        .circle_stroke(hit, 4.0, egui::Stroke::new(1.5, Color32::from_gray(40)));
    resp.clicked().then(|| pastel(hue, t))
}

/// Hue and rim-to-hole position under `p`, if it is on the ring.
fn at(c: Pos2, r: f32, p: Pos2) -> Option<(f32, f32)> {
    let d = p - c;
    let len = d.length();
    if len > r || len < r * HOLE {
        return None;
    }
    let hue = d.y.atan2(d.x) / std::f32::consts::TAU;
    Some((hue.rem_euclid(1.0), (r - len) / (r * (1.0 - HOLE))))
}

/// Colour-per-vertex ring: 72 hue steps, 4 bands from rim to hole.
fn ring_mesh(c: Pos2, r: f32) -> Mesh {
    const STEPS: usize = 72;
    const BANDS: usize = 4;
    let mut mesh = Mesh::default();
    for band in 0..=BANDS {
        let t = band as f32 / BANDS as f32;
        let radius = r - t * r * (1.0 - HOLE);
        for i in 0..=STEPS {
            let hue = i as f32 / STEPS as f32;
            let a = hue * std::f32::consts::TAU;
            let p = c + vec2(a.cos(), a.sin()) * radius;
            mesh.colored_vertex(p, to_color32(pastel(hue, t)));
        }
    }
    let row = (STEPS + 1) as u32;
    for band in 0..BANDS as u32 {
        for i in 0..STEPS as u32 {
            let (a, b) = (band * row + i, band * row + i + 1);
            let (c2, d) = (a + row, b + row);
            mesh.add_triangle(a, b, d);
            mesh.add_triangle(a, d, c2);
        }
    }
    mesh
}

/// Where the picker opens for a row: just right of the pointer, below the row.
pub fn picker_pos(row: Rect, pointer: Option<Pos2>) -> Pos2 {
    pos2(
        pointer.map_or(row.left() + 40.0, |p| p.x),
        row.bottom() + 2.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;

    #[test]
    fn wheel_colours_stay_pastel() {
        for i in 0..12 {
            for t in [0.0, 0.5, 1.0] {
                let [r, g, b] = pastel(i as f32 / 12.0, t);
                let (max, min) = (r.max(g).max(b), r.min(g).min(b));
                assert!(min > 0.45 && max <= 1.0, "soft, never dark: {r} {g} {b}");
                assert!(max - min < 0.5, "never saturated: {r} {g} {b}");
            }
        }
        // The hole edge is paler than the rim.
        let sum = |c: [f32; 3]| c.iter().sum::<f32>();
        assert!(sum(pastel(0.3, 1.0)) > sum(pastel(0.3, 0.0)));
        assert!(at(pos2(0.0, 0.0), 10.0, pos2(1.0, 0.0)).is_none(), "hole");
    }

    #[test]
    fn colour_is_stored_and_undoable() {
        let mut s = EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_folder_color_test"),
        ));
        crate::commands::apply(
            &mut s,
            crate::actions::Action::Create {
                what: crate::actions::Create::Folder,
                parent: None,
            },
        );
        let folder = s.scene.find_by_name("Folder").unwrap();
        let pink = pastel(0.95, 0.5);
        set_color(&mut s, folder, Some(pink));
        assert_eq!(color_of(&s, folder), Some(to_color32(pink)));
        s.undo_step();
        let folder = s.scene.find_by_name("Folder").unwrap();
        let stored = s.scene.world.get::<&HierarchyFolder>(folder).unwrap().color;
        assert_eq!(stored, None, "undo restores the plain folder");
        // Plain objects are not folders: nothing to colour.
        let cam = s.scene.find_by_name("Main Camera").unwrap();
        set_color(&mut s, cam, Some(pink));
        assert!(s.scene.world.get::<&HierarchyFolder>(cam).is_err());
    }
}
