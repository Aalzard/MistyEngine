//! Project window folder tree: Favorites header, the Assets folders with ▼/▶ foldouts and
//! connection lines, then Packages. Clicking a row opens the folder; the triangle folds it.

use std::path::PathBuf;

use egui::{Align2, Color32, FontId, Id, Rect, Sense, Ui, pos2, vec2};

use super::project::{asset_relative, drop_target};
use super::project_menus::{self, Requests};
use crate::branding::icons::{self, Icon};
use crate::project::{FolderNode, ProjectBrowser};
use crate::theme;
use crate::tree_lines::{self, Guide};

const ROW_H: f32 = 22.0;
const INDENT: f32 = 16.0;
/// Triangle column width, then the 15 px folder icon and the gap before the name.
const ARROW_W: f32 = 14.0;
const ICON_W: f32 = 15.0;
const ICON_GAP: f32 = 5.0;

/// Left edge of the triangle column at `depth`.
fn column_left(rect: Rect, depth: usize) -> f32 {
    rect.left() + 6.0 + depth as f32 * INDENT
}

/// The whole tree. `open` receives a clicked folder.
pub fn show(
    ui: &mut Ui,
    browser: &mut ProjectBrowser,
    open: &mut Option<PathBuf>,
    req: &mut Requests,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    header(ui, "★  Favorites");

    // Visible folders depth-first (closed folders hide their children), then Packages.
    // The trees are moved out for the rows and put back below (copying them every frame cost
    // an allocation per folder); the rows only read and change other browser fields.
    let empty = FolderNode {
        path: PathBuf::new(),
        name: String::new(),
        children: Vec::new(),
    };
    let tree = std::mem::replace(&mut browser.tree, empty);
    let mut rows: Vec<(&FolderNode, usize)> = Vec::new();
    visit(&tree, 0, &browser.expanded, &mut rows);
    let packages_root = FolderNode {
        path: browser.packages_dir(),
        name: "Packages".into(),
        children: std::mem::take(&mut browser.packages),
    };
    let first_package = rows.len();
    visit(&packages_root, 0, &browser.expanded, &mut rows);
    let depths: Vec<Option<usize>> = rows.iter().map(|(_, d)| Some(*d)).collect();
    let guides = tree_lines::guides(&depths);

    for (i, (node, depth)) in rows.iter().enumerate() {
        // The Packages root and registry packages can't be renamed, moved into or deleted.
        let locked = i >= first_package && (*depth == 0 || browser.is_read_only(&node.path));
        folder_row(
            ui,
            browser,
            node,
            *depth,
            (i, &guides[i], locked),
            open,
            req,
        );
    }
    browser.tree = tree;
    browser.packages = packages_root.children;
}

fn visit<'a>(
    node: &'a FolderNode,
    depth: usize,
    expanded: &std::collections::HashSet<PathBuf>,
    out: &mut Vec<(&'a FolderNode, usize)>,
) {
    out.push((node, depth));
    if expanded.contains(&node.path) {
        for c in &node.children {
            visit(c, depth + 1, expanded, out);
        }
    }
}

/// A dim section title row.
fn header(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(12.5),
        theme::TEXT_DIM,
    );
}

/// Folder icon and name after the triangle column.
fn paint_label(ui: &Ui, rect: Rect, depth: usize, text: &str, color: Color32, bold: bool) {
    let x = column_left(rect, depth) + ARROW_W + 2.0;
    let slot = Rect::from_min_size(pos2(x, rect.top()), vec2(ICON_W, ROW_H));
    icons::paint(ui, slot.shrink2(vec2(0.0, 3.0)), Icon::Folder, color);
    let passes: &[f32] = if bold { &[0.0, 0.45] } else { &[0.0] };
    for dx in passes {
        ui.painter().text(
            pos2(x + ICON_W + ICON_GAP + dx, rect.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(13.0),
            color,
        );
    }
}

/// One folder row: background, lines, triangle, icon + name (or the rename field), and the
/// same click / drag / drop / context menu behaviour as before.
fn folder_row(
    ui: &mut Ui,
    browser: &mut ProjectBrowser,
    node: &FolderNode,
    depth: usize,
    (index, guide, locked): (usize, &Guide, bool),
    open: &mut Option<PathBuf>,
    req: &mut Requests,
) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
    let selected = node.path == browser.current;
    let parent = !node.children.is_empty();
    tree_lines::paint_background(ui.painter(), rect, index, parent);
    if selected {
        ui.painter().rect_filled(rect, 0.0, theme::SELECTION);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, tree_lines::HOVER);
    }

    let left = column_left(rect, depth);
    let column = |d: usize| column_left(rect, d) + ARROW_W * 0.5;
    let tick_end = if parent {
        left + 2.0
    } else {
        left + ARROW_W + 1.0
    };
    tree_lines::paint_guide(ui.painter(), rect, guide, column, tick_end, 3.5);

    if parent {
        let arrow_rect = Rect::from_min_size(pos2(left, rect.top()), vec2(ARROW_W, ROW_H));
        let id = Id::new(("project_tree_arrow", &node.path));
        let arrow = ui.interact(arrow_rect, id, Sense::click());
        let is_open = browser.expanded.contains(&node.path);
        let color = if arrow.hovered() {
            tree_lines::ARROW_HOVER
        } else {
            tree_lines::ARROW
        };
        tree_lines::paint_arrow(ui.painter(), arrow_rect.center(), is_open, color);
        if arrow.clicked() {
            if is_open {
                browser.expanded.remove(&node.path);
            } else {
                browser.expanded.insert(node.path.clone());
            }
        }
    }

    let renaming = browser
        .renaming
        .as_ref()
        .is_some_and(|(p, _)| *p == node.path);
    let color = match (selected, parent) {
        (true, _) => Color32::WHITE,
        (false, true) => tree_lines::PARENT_TEXT,
        (false, false) => theme::TEXT,
    };
    let label = if renaming { "" } else { node.name.as_str() };
    paint_label(ui, rect, depth, label, color, parent);
    if renaming {
        let r = rect.with_min_x(left + ARROW_W + 2.0 + ICON_W + 2.0);
        project_menus::rename_field(ui, r, browser, req);
    }

    if resp.clicked() {
        *open = Some(node.path.clone());
    }
    if locked {
        return;
    }
    drop_target(&resp, &browser.root, &node.path, req);
    if depth > 0 {
        if let Some(rel) = asset_relative(&node.path) {
            resp.dnd_set_drag_payload(super::AssetPayload(rel));
        }
        let path = node.path.clone();
        resp.context_menu(|ui| project_menus::item_menu(ui, &path, asset_relative(&path), req));
    }
}
