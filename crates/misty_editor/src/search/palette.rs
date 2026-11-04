//! The Ctrl+K palette: a floating box near the top of the window with grouped results.
//! Up/Down move, Enter runs the default action, Alt+Enter (or right-click) lists the others,
//! Esc steps back / closes.

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, Color32, FontId, Key, Modifiers, Sense, Ui, vec2};

use crate::state::EditorState;
use crate::theme;

use super::dispatch::{self, After, Command};
use super::{Hit, Provider, Target};

const WIDTH: f32 = 560.0;
const ROW_H: f32 = 24.0;
const LIST_H: f32 = 380.0;
const HINT: &str = "Search objects, assets, menus, settings…";

/// Keys the palette takes before any panel sees them.
#[derive(Default)]
struct Keys {
    up: bool,
    down: bool,
    enter: bool,
    alt_enter: bool,
    esc: bool,
}

fn take_keys(ctx: &egui::Context) -> Keys {
    ctx.input_mut(|i| Keys {
        // Alt+Enter first: plain Enter would also match it (extra Alt is ignored).
        alt_enter: i.consume_key(Modifiers::ALT, Key::Enter),
        enter: i.consume_key(Modifiers::NONE, Key::Enter),
        up: i.consume_key(Modifiers::NONE, Key::ArrowUp),
        down: i.consume_key(Modifiers::NONE, Key::ArrowDown),
        esc: i.consume_key(Modifiers::NONE, Key::Escape),
    })
}

/// Draw the palette if open. Call before the panels so it gets the keyboard first.
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    if !s.search.open {
        return;
    }
    if s.search.index.tick(super::asset_index::CHECKS_PER_FRAME) || s.search.index.is_building() {
        ctx.request_repaint();
    }
    super::update(s);
    let keys = take_keys(ctx);
    let moved = keys.up || keys.down;
    if handle_keys(s, &keys) {
        return;
    }
    let id = egui::Id::new("search_palette");
    let width = WIDTH.min(ctx.content_rect().width() - 32.0);
    let frame = egui::Frame::popup(&ctx.global_style())
        .inner_margin(10)
        .corner_radius(6);
    let area = egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, vec2(0.0, 72.0));
    let mut clicked: Option<(usize, Command)> = None;
    let modal = egui::Modal::new(id)
        .area(area)
        .frame(frame)
        .backdrop_color(Color32::from_black_alpha(60))
        .show(ctx, |ui| {
            ui.set_width(width);
            text_box(ui, s);
            ui.add_space(6.0);
            clicked = results(ui, s, moved);
            footer(ui);
        });
    if let Some((i, cmd)) = clicked {
        activate(s, i, cmd);
    } else if modal.backdrop_response.clicked() {
        super::close(s);
    }
}

/// Keyboard navigation; returns true when the palette closed.
fn handle_keys(s: &mut EditorState, k: &Keys) -> bool {
    let p = &mut s.search;
    if let Some(i) = p.actions_of {
        let n = p.hits.get(i).map_or(0, |h| dispatch::commands(h).len());
        step(&mut p.action_selected, n, k);
        if k.esc {
            p.actions_of = None;
        } else if (k.enter || k.alt_enter)
            && let Some(cmd) = p
                .hits
                .get(i)
                .and_then(|h| dispatch::commands(h).get(p.action_selected).copied())
        {
            return activate(s, i, cmd);
        }
        return false;
    }
    step(&mut p.selected, p.hits.len(), k);
    if k.esc {
        super::close(s);
        return true;
    }
    let i = p.selected;
    if k.alt_enter && i < p.hits.len() {
        p.actions_of = Some(i);
        p.action_selected = 0;
    } else if k.enter
        && let Some(cmd) = p.selected_hit().map(|h| dispatch::commands(h)[0])
    {
        return activate(s, i, cmd);
    }
    false
}

/// Up/Down within `n` rows (wrapping).
fn step(sel: &mut usize, n: usize, k: &Keys) {
    if n == 0 {
        *sel = 0;
    } else if k.down {
        *sel = (*sel + 1) % n;
    } else if k.up {
        *sel = (*sel + n - 1) % n;
    }
}

/// Run a command on hit `i`; returns true when the palette closed.
fn activate(s: &mut EditorState, i: usize, cmd: Command) -> bool {
    let Some(hit) = s.search.hits.get(i).cloned() else {
        return false;
    };
    let text = s.search.query.clone();
    match dispatch::run(s, &hit, cmd) {
        After::Close => {
            s.search.remember(&text);
            super::close(s);
            true
        }
        After::Search(t) => {
            s.search.set_query(&t);
            false
        }
    }
}

fn text_box(ui: &mut Ui, s: &mut EditorState) {
    let p = &mut s.search;
    let resp = ui.add(
        egui::TextEdit::singleline(&mut p.query)
            .hint_text(HINT)
            .font(FontId::proportional(16.0))
            .margin(vec2(8.0, 6.0))
            .desired_width(f32::INFINITY),
    );
    // The box keeps the keyboard while the palette is open.
    if p.focus_text || !resp.has_focus() {
        resp.request_focus();
        p.focus_text = false;
    }
    if resp.changed() {
        p.selected = 0;
        p.actions_of = None;
    }
}

/// Grouped result rows (or the actions of one row). Returns a clicked row and command.
fn results(ui: &mut Ui, s: &mut EditorState, moved: bool) -> Option<(usize, Command)> {
    let p = &mut s.search;
    if let Some(i) = p.actions_of
        && let Some(hit) = p.hits.get(i)
    {
        return actions_list(ui, hit, i, p.action_selected);
    }
    if p.hits.is_empty() {
        empty_state(ui, p.query.trim().is_empty(), p.index.is_building());
        return None;
    }
    let mut clicked = None;
    let mut open_actions = None;
    egui::ScrollArea::vertical()
        .max_height(LIST_H)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let mut last = None;
            for (i, hit) in p.hits.iter().enumerate() {
                if last != Some(hit.provider) {
                    group_header(ui, hit.provider);
                    last = Some(hit.provider);
                }
                let selected = i == p.selected;
                let resp = row(ui, hit, selected);
                if selected && moved {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() {
                    clicked = Some((i, dispatch::commands(hit)[0]));
                } else if resp.secondary_clicked() {
                    open_actions = Some(i);
                } else if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                    // Mouse movement moves the highlight (keyboard keeps working from there).
                    p.selected = i;
                }
            }
        });
    if let Some(i) = open_actions {
        p.selected = i;
        p.actions_of = Some(i);
        p.action_selected = 0;
    }
    clicked
}

fn actions_list(ui: &mut Ui, hit: &Hit, i: usize, sel: usize) -> Option<(usize, Command)> {
    group_header(ui, hit.provider);
    ui.label(egui::RichText::new(format!("{}  {}", hit.icon, hit.label)).strong());
    ui.add_space(2.0);
    let mut clicked = None;
    for (n, cmd) in dispatch::commands(hit).into_iter().enumerate() {
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        paint_row_bg(ui, rect, n == sel, resp.hovered());
        let color = if n == sel {
            Color32::WHITE
        } else {
            theme::TEXT
        };
        ui.painter().text(
            rect.left_center() + vec2(30.0, 0.0),
            Align2::LEFT_CENTER,
            cmd.label(),
            FontId::proportional(14.0),
            color,
        );
        if resp.clicked() {
            clicked = Some((i, cmd));
        }
    }
    clicked
}

fn group_header(ui: &mut Ui, provider: Provider) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!("{}  {}", provider.icon(), provider.title()))
            .size(11.0)
            .color(theme::TEXT_DIM),
    );
}

fn paint_row_bg(ui: &Ui, rect: egui::Rect, selected: bool, hovered: bool) {
    if selected {
        ui.painter().rect_filled(rect, 3.0, theme::SELECTION);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, 3.0, Color32::from_white_alpha(10));
    }
}

/// Label with matched characters highlighted.
fn label_job(hit: &Hit, selected: bool, max_width: f32) -> LayoutJob {
    let font = FontId::proportional(14.0);
    let base = if selected {
        Color32::WHITE
    } else {
        theme::TEXT
    };
    let accent = if selected {
        Color32::from_rgb(255, 214, 110)
    } else {
        Color32::from_rgb(110, 170, 255)
    };
    let mut job = LayoutJob::default();
    let mut matched = hit.indices.iter().peekable();
    for (n, c) in hit.label.chars().enumerate() {
        let hit_char = matched.next_if_eq(&&n).is_some();
        let color = if hit_char { accent } else { base };
        let mut buf = [0; 4];
        job.append(
            c.encode_utf8(&mut buf),
            0.0,
            TextFormat::simple(font.clone(), color),
        );
    }
    job.wrap = TextWrapping::truncate_at_width(max_width);
    job
}

fn row(ui: &mut Ui, hit: &Hit, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    paint_row_bg(ui, rect, selected, resp.hovered());
    let painter = ui.painter();
    let color = if hit.provider == Provider::Project
        && matches!(&hit.target, Target::Asset { rel, .. } if rel.ends_with(".prefab"))
    {
        theme::PREFAB
    } else {
        theme::TEXT
    };
    painter.text(
        rect.left_center() + vec2(14.0, 0.0),
        Align2::CENTER_CENTER,
        hit.icon,
        FontId::proportional(13.0),
        color,
    );
    let x = rect.left() + 30.0;
    let shortcut_w = if hit.shortcut.is_empty() { 0.0 } else { 90.0 };
    let label_room = (rect.right() - x - shortcut_w - 8.0).max(40.0);
    let galley = ui.fonts_mut(|f| f.layout_job(label_job(hit, selected, label_room * 0.7)));
    let label_w = galley.size().x;
    painter.galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        Color32::WHITE,
    );
    let dim = if selected {
        Color32::from_gray(190)
    } else {
        theme::TEXT_DIM
    };
    if !hit.detail.is_empty() {
        let mut job =
            LayoutJob::simple_singleline(hit.detail.clone(), FontId::proportional(12.0), dim);
        job.wrap = TextWrapping::truncate_at_width((label_room - label_w - 12.0).max(0.0));
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let pos = egui::pos2(x + label_w + 12.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, dim);
    }
    if !hit.shortcut.is_empty() {
        ui.painter().text(
            rect.right_center() - vec2(8.0, 0.0),
            Align2::RIGHT_CENTER,
            hit.shortcut,
            FontId::proportional(12.0),
            dim,
        );
    }
    resp
}

fn empty_state(ui: &mut Ui, blank: bool, indexing: bool) {
    let text = if blank {
        "Type to search. Prefixes: h: Hierarchy   p: Project   m: Menu   s: Settings   \
         Filters: t:Light  tag:Player  layer:UI   Calculator: =2*3"
    } else if indexing {
        "Indexing assets…"
    } else {
        "No results"
    };
    ui.add_space(6.0);
    ui.add(egui::Label::new(egui::RichText::new(text).color(theme::TEXT_DIM)).wrap());
    ui.add_space(6.0);
}

fn footer(ui: &mut Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.label(
        egui::RichText::new("↑↓ move    Enter open    Alt+Enter more actions    Esc close")
            .size(11.0)
            .color(theme::TEXT_DIM),
    );
}
