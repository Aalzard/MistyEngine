//! The text area of one document: gutter (line numbers, error markers, fold arrows), highlighted
//! text, carets and selections, squiggles, the autocomplete popup and the minimap. Mouse and
//! keyboard input become editing commands here; file-level commands go back to `ui`.

use std::sync::Arc;

use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{Color32, FontId, Galley, Key, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use super::Document;
use super::completion::{self, Item};
use super::keys::Cmd;
use super::lexer::TokenKind;
use super::prefs::Settings;
use super::text::{Pos, Selection, is_word};

/// Per-document view state.
#[derive(Default)]
pub struct ViewState {
    /// Bring the primary caret into view next frame (`center`: in the middle).
    pub scroll_to_caret: bool,
    pub center: bool,
    /// Editor version the syntax-check debounce last saw.
    pub seen_version: u64,
    /// Scroll by this many pixels next frame (Ctrl+Up/Down).
    pub scroll_delta: f32,
    /// Jump the vertical scroll to this offset next frame (minimap).
    pub scroll_to_y: Option<f32>,
    /// Vertical scroll offset and viewport height last frame (minimap).
    pub scroll_y: f32,
    pub viewport_h: f32,
    /// Rows that fit in the view (Page Up/Down) and the row height, last frame.
    pub page_rows: usize,
    pub row_h: f32,
    /// A mouse drag is selecting: whether it's a column (box) selection, or whole lines (gutter).
    drag: Option<Drag>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    Text,
    Box(Pos),
    Lines(usize),
}

/// The autocomplete popup.
pub struct Completion {
    pub ctx: completion::Context,
    pub items: Vec<Item>,
    pub selected: usize,
}

/// What the view needs from the window and gives back.
pub struct ViewCtx<'a> {
    pub settings: &'a Settings,
    pub chord: &'a mut Option<Key>,
    pub completion: &'a mut Option<Completion>,
    /// Find matches to highlight.
    pub matches: &'a [(Pos, Pos)],
    /// Text last copied as whole lines (pasting it inserts lines above the caret).
    pub line_clipboard: &'a mut Option<String>,
    pub request_focus: bool,
}

#[derive(Default)]
pub struct ViewOutput {
    /// Commands for the window (save, find, play…).
    pub cmds: Vec<Cmd>,
    pub focused: bool,
    /// Status-bar message (chords).
    pub message: Option<String>,
    /// Text to put on the clipboard.
    pub copy: Option<String>,
}

pub mod colors {
    use egui::Color32;

    pub const BG: Color32 = Color32::from_rgb(30, 30, 32);
    pub const GUTTER: Color32 = Color32::from_rgb(30, 30, 32);
    pub const LINE_NO: Color32 = Color32::from_rgb(110, 118, 129);
    pub const LINE_NO_ACTIVE: Color32 = Color32::from_rgb(205, 210, 215);
    pub const CURRENT_LINE: Color32 = Color32::from_rgb(40, 40, 44);
    pub const SELECTION: Color32 = Color32::from_rgb(38, 79, 120);
    pub const SELECTION_INACTIVE: Color32 = Color32::from_rgb(58, 61, 65);
    pub const FIND: Color32 = Color32::from_rgba_premultiplied(120, 90, 20, 140);
    pub const OCCURRENCE: Color32 = Color32::from_rgba_premultiplied(40, 40, 40, 60);
    pub const CARET: Color32 = Color32::from_rgb(220, 220, 220);
    pub const BRACKET_BOX: Color32 = Color32::from_rgb(130, 130, 130);
    pub const ERROR: Color32 = Color32::from_rgb(240, 70, 60);
    pub const GUIDE: Color32 = Color32::from_rgb(55, 55, 58);
    pub const TEXT: Color32 = Color32::from_rgb(212, 212, 212);
}

/// Text colour of a token kind.
pub fn token_color(kind: TokenKind) -> Color32 {
    match kind {
        TokenKind::Keyword => Color32::from_rgb(197, 134, 192),
        TokenKind::Literal | TokenKind::SelfKw => Color32::from_rgb(86, 156, 214),
        TokenKind::Number => Color32::from_rgb(181, 206, 168),
        TokenKind::String => Color32::from_rgb(214, 157, 133),
        TokenKind::Comment => Color32::from_rgb(106, 153, 85),
        TokenKind::EngineGlobal => Color32::from_rgb(78, 201, 176),
        TokenKind::Builtin => Color32::from_rgb(86, 186, 214),
        TokenKind::Function => Color32::from_rgb(220, 220, 170),
        TokenKind::Identifier => Color32::from_rgb(156, 220, 254),
        TokenKind::Bracket => Color32::from_rgb(255, 215, 0),
        TokenKind::Operator | TokenKind::Punct => colors::TEXT,
    }
}

/// Layout of the visible rows for this frame.
struct Rows {
    /// Visible (not folded away) lines.
    lines: Vec<usize>,
    /// Top of each row in content coordinates, plus the total height at the end.
    tops: Vec<f32>,
    row_h: f32,
    wrap: Option<f32>,
}

impl Rows {
    fn index_at_y(&self, y: f32) -> usize {
        match self.tops.binary_search_by(|t| t.total_cmp(&y)) {
            Ok(i) => i.min(self.lines.len().saturating_sub(1)),
            Err(i) => i.saturating_sub(1).min(self.lines.len().saturating_sub(1)),
        }
    }

    /// Row index of `line`, or of the nearest visible line above it.
    fn index_of(&self, line: usize) -> usize {
        match self.lines.binary_search(&line) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    fn total(&self) -> f32 {
        self.tops.last().copied().unwrap_or(0.0)
    }
}

/// Fonts and galleys for one frame.
struct Painter<'a> {
    font: FontId,
    painter: egui::Painter,
    tokens: &'a [Vec<super::lexer::Token>],
    lines: &'a [String],
}

impl Painter<'_> {
    fn galley(&self, line: usize, wrap: Option<f32>) -> Arc<Galley> {
        let text = &self.lines[line];
        let mut job = LayoutJob {
            text: text.clone(),
            ..Default::default()
        };
        if let Some(w) = wrap {
            job.wrap.max_width = w;
        }
        let fmt = |c: Color32| TextFormat::simple(self.font.clone(), c);
        let byte = |col: usize| super::text::byte_of(text, col);
        let mut last = 0;
        for t in self.tokens.get(line).map(Vec::as_slice).unwrap_or(&[]) {
            let (s, e) = (byte(t.start), byte(t.end));
            if s > last {
                job.sections.push(section(last..s, fmt(colors::TEXT)));
            }
            job.sections.push(section(s..e, fmt(token_color(t.kind))));
            last = e;
        }
        if last < text.len() || job.sections.is_empty() {
            job.sections
                .push(section(last..text.len(), fmt(colors::TEXT)));
        }
        self.painter.layout_job(job)
    }
}

fn section(range: std::ops::Range<usize>, format: TextFormat) -> egui::text::LayoutSection {
    egui::text::LayoutSection {
        leading_space: 0.0,
        byte_range: range.start.into()..range.end.into(),
        format,
    }
}

/// Rectangles covering columns `a..b` of a laid-out line (several when it wraps).
fn range_rects(g: &Galley, a: usize, b: usize, row_h: f32) -> Vec<Rect> {
    let mut out = Vec::new();
    let mut start = 0;
    for row in &g.rows {
        let n = row.char_count_excluding_newline().0;
        let (ra, rb) = (a.max(start), b.min(start + n));
        if ra <= rb && (ra < rb || a == b || (a < start + n + 1 && b > start + n)) {
            let x0 = row.x_offset((ra - start).into());
            let x1 = if b > start + n {
                row.x_offset((rb - start).into()) + 4.0
            } else {
                row.x_offset((rb - start).into())
            };
            let y = row.pos.y;
            out.push(Rect::from_min_max(
                Pos2::new(x0, y),
                Pos2::new(x1, y + row.height().max(row_h)),
            ));
        }
        start += n;
    }
    out
}

/// Draw the document and handle its input.
pub fn show(ui: &mut Ui, doc: &mut Document, mut cx: ViewCtx<'_>) -> ViewOutput {
    let mut out = ViewOutput::default();
    let settings = cx.settings;
    let font = FontId::monospace(settings.font_size);
    let (row_h, char_w) = ui.fonts_mut(|f| (f.row_height(&font), f.glyph_width(&font, 'M')));
    let area = ui.available_rect_before_wrap();
    let minimap_w = if settings.minimap { 90.0 } else { 0.0 };
    let text_area = Rect::from_min_max(area.min, Pos2::new(area.max.x - minimap_w, area.max.y));
    let id = ui.id().with("code_text");
    let has_focus = ui.memory(|m| m.has_focus(id));

    // Keyboard first, so this frame draws the result.
    if has_focus {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
        let before = (doc.editor.version, doc.editor.sels.clone());
        super::input::keyboard(ui, doc, &mut cx, &mut out);
        if (doc.editor.version, doc.editor.sels.clone()) != before {
            doc.view.scroll_to_caret = true;
        }
    }

    let analysis = doc.analysis();
    let lines_vec = doc.editor.buf.lines().to_vec();
    let painter_src = Painter {
        font: font.clone(),
        painter: ui.painter().clone(),
        tokens: &analysis.tokens,
        lines: &lines_vec,
    };
    let digits = lines_vec.len().to_string().len().max(3) as f32;
    let fold_w = row_h * 0.9;
    let marker_w = 12.0;
    let numbers_w = if settings.line_numbers {
        digits * char_w + 10.0
    } else {
        4.0
    };
    let gutter_w = marker_w + numbers_w + fold_w;
    let text_x = gutter_w + 6.0;
    let wrap = settings
        .word_wrap
        .then(|| (text_area.width() - text_x - 16.0).max(char_w * 10.0));

    // Row geometry.
    let visible = doc.visible_lines();
    let mut tops = Vec::with_capacity(visible.len() + 1);
    let mut y = 0.0;
    let mut wrapped: std::collections::HashMap<usize, Arc<Galley>> = Default::default();
    for &l in &visible {
        tops.push(y);
        y += match wrap {
            Some(w) => {
                let g = painter_src.galley(l, Some(w));
                let h = g.size().y.max(row_h);
                wrapped.insert(l, g);
                h
            }
            None => row_h,
        };
    }
    tops.push(y);
    let rows = Rows {
        lines: visible,
        tops,
        row_h,
        wrap,
    };
    let max_chars = lines_vec
        .iter()
        .map(|l| {
            l.chars()
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let content_w = match wrap {
        Some(_) => text_area.width() - 4.0,
        None => text_x + max_chars as f32 * char_w + 60.0,
    };
    let folded = doc.folded_ranges();

    let mut scroll = egui::ScrollArea::both()
        .id_salt(("code_scroll", doc.path.as_str()))
        .auto_shrink(false);
    if let Some(y) = doc.view.scroll_to_y.take() {
        scroll = scroll.vertical_scroll_offset(y.max(0.0));
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_area));
    child.painter().rect_filled(text_area, 0.0, colors::BG);
    scroll.show_viewport(&mut child, |ui, viewport| {
        let origin = ui.max_rect().min;
        let content = Rect::from_min_size(
            origin,
            Vec2::new(
                content_w,
                rows.total() + (viewport.height() - row_h * 2.0).max(row_h),
            ),
        );
        let resp = ui.interact(content, id, Sense::click_and_drag());
        ui.allocate_rect(content, Sense::hover());
        if cx.request_focus && !resp.has_focus() {
            resp.request_focus();
        }
        out.focused = resp.has_focus() || has_focus;
        if doc.view.scroll_delta != 0.0 {
            ui.scroll_with_delta(Vec2::new(0.0, -doc.view.scroll_delta));
            doc.view.scroll_delta = 0.0;
        }
        doc.view.page_rows = ((viewport.height() / row_h) as usize)
            .saturating_sub(1)
            .max(1);
        doc.view.row_h = row_h;
        doc.view.scroll_y = viewport.min.y;
        doc.view.viewport_h = viewport.height();

        let text_origin = |row: usize| Pos2::new(origin.x + text_x, origin.y + rows.tops[row]);
        let galley_for = |line: usize| -> Arc<Galley> {
            wrapped
                .get(&line)
                .cloned()
                .unwrap_or_else(|| painter_src.galley(line, rows.wrap))
        };
        let pos_at = |p: Pos2| -> Pos {
            let row = rows.index_at_y(p.y - origin.y);
            let line = rows.lines.get(row).copied().unwrap_or(0);
            if p.y - origin.y > rows.total() {
                return Pos::new(line, lines_vec[line].chars().count());
            }
            let g = galley_for(line);
            let local = p - text_origin(row);
            Pos::new(line, g.cursor_from_pos(local).index.0)
        };

        mouse(ui, &resp, doc, &rows, gutter_w, &pos_at, &mut out);

        // Visible rows.
        let first = rows.index_at_y(viewport.min.y);
        let last = rows.index_at_y(viewport.max.y);
        let painter = ui.painter_at(
            viewport
                .translate(origin.to_vec2())
                .intersect(ui.clip_rect()),
        );
        let sel_color = if out.focused {
            colors::SELECTION
        } else {
            colors::SELECTION_INACTIVE
        };
        let primary = doc.editor.primary();
        let word = (primary.is_empty())
            .then(|| doc.editor.buf.word_at(primary.head))
            .flatten()
            .map(|(a, b)| doc.editor.buf.slice(a, b))
            .filter(|w| w.chars().any(|c| !c.is_ascii_digit()));
        let bracket = doc.editor.matching_bracket(&analysis.tokens);
        let full_w = viewport.width().max(content_w);
        for row in first..=last.min(rows.lines.len().saturating_sub(1)) {
            let line = rows.lines[row];
            let g = galley_for(line);
            let o = text_origin(row);
            let row_rect = Rect::from_min_size(
                Pos2::new(origin.x, o.y),
                Vec2::new(full_w, rows.tops[row + 1] - rows.tops[row]),
            );
            if primary.is_empty() && primary.head.line == line {
                painter.rect_filled(row_rect, 0.0, colors::CURRENT_LINE);
            }
            // Indent guides.
            let indent = lines_vec[line].len() - lines_vec[line].trim_start().len();
            let tab = settings.tab_size.max(1);
            let mut col = tab;
            while col < indent || (lines_vec[line].trim().is_empty() && col < indent) {
                let x = o.x + col as f32 * char_w - char_w * tab as f32 + 1.0;
                painter.vline(x, row_rect.y_range(), Stroke::new(1.0, colors::GUIDE));
                col += tab;
            }
            let fill = |a: usize, b: usize, color: Color32| {
                for r in range_rects(&g, a, b, row_h) {
                    painter.rect_filled(r.translate(o.to_vec2()), 2.0, color);
                }
            };
            if let Some(w) = &word {
                let chars: Vec<char> = lines_vec[line].chars().collect();
                let wl = w.chars().count();
                let mut i = 0;
                while i + wl <= chars.len() {
                    let hit = chars[i..i + wl].iter().copied().eq(w.chars())
                        && (i == 0 || !is_word(chars[i - 1]))
                        && (i + wl == chars.len() || !is_word(chars[i + wl]));
                    if hit {
                        fill(i, i + wl, colors::OCCURRENCE);
                        i += wl;
                    } else {
                        i += 1;
                    }
                }
            }
            for (a, b) in cx.matches.iter().filter(|m| m.0.line == line) {
                fill(a.col, b.col, colors::FIND);
            }
            for s in &doc.editor.sels {
                let (a, b) = (s.start(), s.end());
                if s.is_empty() || line < a.line || line > b.line {
                    continue;
                }
                let ca = if line == a.line { a.col } else { 0 };
                let len = lines_vec[line].chars().count();
                let cb = if line == b.line { b.col } else { len + 1 };
                fill(ca, cb.min(len + 1), sel_color);
            }
            if let Some((p, q)) = bracket {
                for b in [p, q] {
                    if b.line == line {
                        for r in range_rects(&g, b.col, b.col + 1, row_h) {
                            painter.rect_stroke(
                                r.translate(o.to_vec2()),
                                1.0,
                                Stroke::new(1.0, colors::BRACKET_BOX),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                }
            }
            painter.galley(o, g.clone(), colors::TEXT);
            if let Some(d) = doc.diagnostic.as_ref().filter(|d| d.line == line) {
                for r in range_rects(&g, d.start, d.end.max(d.start + 1), row_h) {
                    squiggle(&painter, r.translate(o.to_vec2()));
                }
            }
            if let Some(&(_, end)) = folded.iter().find(|f| f.0 == line) {
                let x = o.x + g.size().x + char_w;
                let r = Rect::from_min_size(
                    Pos2::new(x, o.y + 1.0),
                    Vec2::new(char_w * 3.0, row_h - 2.0),
                );
                painter.rect_stroke(
                    r,
                    2.0,
                    Stroke::new(1.0, colors::LINE_NO),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    "…",
                    font.clone(),
                    colors::LINE_NO,
                );
                let hidden = end - line;
                let r_resp = ui.interact(r, id.with(("fold", line)), Sense::click());
                if r_resp
                    .on_hover_text(format!("{hidden} hidden lines"))
                    .clicked()
                {
                    doc.toggle_fold(line);
                }
            }
            // Carets (blinking while focused).
            let blink_on = !out.focused
                || (ui.input(|i| i.time) * 1.6).fract() < 0.6
                || ui.input(|i| i.pointer.any_down());
            for s in &doc.editor.sels {
                if s.head.line == line && (blink_on || !out.focused) && out.focused {
                    let r = g.pos_from_cursor(CCursor::new(s.head.col));
                    let x = o.x + r.min.x;
                    painter.vline(
                        x,
                        egui::Rangef::new(o.y + r.min.y, o.y + r.min.y + row_h),
                        Stroke::new(2.0, colors::CARET),
                    );
                }
            }
        }
        if out.focused {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(300));
        }

        // Gutter, pinned to the left edge.
        let gx = origin.x + viewport.min.x;
        let gutter = Rect::from_min_max(
            Pos2::new(gx, origin.y + viewport.min.y),
            Pos2::new(gx + gutter_w, origin.y + viewport.max.y),
        );
        painter.rect_filled(gutter, 0.0, colors::GUTTER);
        let fold_starts: Vec<usize> = analysis.structure.folds.iter().map(|f| f.0).collect();
        let gutter_hovered = ui.rect_contains_pointer(gutter);
        let caret_lines: Vec<usize> = doc.editor.sels.iter().map(|s| s.head.line).collect();
        for row in first..=last.min(rows.lines.len().saturating_sub(1)) {
            let line = rows.lines[row];
            let y = origin.y + rows.tops[row];
            if settings.line_numbers {
                let color = if caret_lines.contains(&line) {
                    colors::LINE_NO_ACTIVE
                } else {
                    colors::LINE_NO
                };
                painter.text(
                    Pos2::new(gx + marker_w + numbers_w - 6.0, y),
                    egui::Align2::RIGHT_TOP,
                    (line + 1).to_string(),
                    font.clone(),
                    color,
                );
            }
            if let Some(d) = doc.diagnostic.as_ref().filter(|d| d.line == line) {
                let c = Pos2::new(gx + marker_w * 0.5 + 1.0, y + row_h * 0.5);
                painter.circle_filled(c, 4.0, colors::ERROR);
                let r = Rect::from_center_size(c, Vec2::splat(row_h));
                ui.interact(r, id.with(("err", line)), Sense::hover())
                    .on_hover_text(&d.message);
            }
            if fold_starts.contains(&line) {
                let is_folded = folded.iter().any(|f| f.0 == line);
                if gutter_hovered || is_folded {
                    let c = Pos2::new(gx + marker_w + numbers_w + fold_w * 0.5, y + row_h * 0.5);
                    let r = Rect::from_center_size(c, Vec2::splat(fold_w));
                    let icon = if is_folded { "⏵" } else { "⏷" };
                    painter.text(
                        c,
                        egui::Align2::CENTER_CENTER,
                        icon,
                        FontId::proportional(row_h * 0.7),
                        colors::LINE_NO,
                    );
                    if ui
                        .interact(r, id.with(("fold_btn", line)), Sense::click())
                        .clicked()
                    {
                        doc.toggle_fold(line);
                    }
                }
            }
        }

        // Keep the caret in view after keyboard moves.
        if doc.view.scroll_to_caret {
            doc.view.scroll_to_caret = false;
            let head = doc.editor.caret();
            let row = rows.index_of(head.line);
            let g = galley_for(rows.lines.get(row).copied().unwrap_or(0));
            let r = g.pos_from_cursor(CCursor::new(head.col));
            let o = text_origin(row);
            let caret = Rect::from_min_size(o + r.min.to_vec2(), Vec2::new(char_w, row_h))
                .expand2(Vec2::new(char_w * 4.0, row_h));
            // The gutter covers the left part of the view.
            let align = doc.view.center.then_some(egui::Align::Center);
            doc.view.center = false;
            ui.scroll_to_rect(caret.with_min_x(caret.min.x - gutter_w), align);
        }

        // Autocomplete popup under the caret.
        if let Some(c) = cx.completion.as_ref() {
            let head = doc.editor.caret();
            let row = rows.index_of(head.line);
            let g = galley_for(head.line);
            let r = g.pos_from_cursor(CCursor::new(c.ctx.start));
            let at = text_origin(row) + r.min.to_vec2() + Vec2::new(0.0, row_h);
            completion_popup(ui, c, at, &font);
        }

        // Quick info: API docs and syntax errors under the mouse.
        if let Some(p) = resp.hover_pos()
            && p.x > gx + gutter_w
            && !ui.input(|i| i.pointer.any_down())
        {
            let pos = pos_at(p);
            let tip = doc
                .diagnostic
                .as_ref()
                .filter(|d| d.line == pos.line && pos.col >= d.start && pos.col <= d.end)
                .map(|d| d.message.clone())
                .or_else(|| quick_info(doc, pos));
            if let Some(tip) = tip {
                resp.clone().on_hover_ui_at_pointer(|ui| {
                    ui.label(egui::RichText::new(tip).monospace());
                });
            }
        }
    });

    if settings.minimap {
        let map = Rect::from_min_max(Pos2::new(text_area.max.x, area.min.y), area.max);
        minimap(ui, doc, &analysis.tokens, map, &rows);
    }
    ui.advance_cursor_after_rect(area);
    out
}

/// Red wavy underline along the bottom of `r`.
fn squiggle(painter: &egui::Painter, r: Rect) {
    let y = r.bottom() - 1.5;
    let mut pts = Vec::new();
    let mut x = r.left();
    let mut up = true;
    while x <= r.right() + 0.1 {
        pts.push(Pos2::new(x, if up { y - 1.5 } else { y + 0.5 }));
        x += 2.0;
        up = !up;
    }
    painter.add(egui::Shape::line(pts, Stroke::new(1.0, colors::ERROR)));
}

/// Signature and description of the API name at `pos`.
fn quick_info(doc: &mut Document, pos: Pos) -> Option<String> {
    let (a, b) = doc.editor.buf.word_at(pos)?;
    let word = doc.editor.buf.slice(a, b);
    let before: String = doc.editor.buf.line(a.line).chars().take(a.col).collect();
    let owner = before
        .strip_suffix('.')
        .or_else(|| before.strip_suffix(':'))
        .map(|h| {
            h.chars()
                .rev()
                .take_while(|c| is_word(*c))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        });
    let entry = match owner.as_deref() {
        Some(o) if super::api::is_global(o) => super::api::lookup(Some(o), &word),
        Some("transform") => super::api::lookup(Some("#Transform"), &word),
        Some("gameObject") => super::api::lookup(Some("#GameObject"), &word),
        Some(_) => None,
        None => super::api::lookup(None, &word),
    }?;
    let full = match owner {
        Some(o) => format!("{o}.{}", entry.signature),
        None => entry.signature.to_owned(),
    };
    Some(format!("{full}\n{}", entry.doc))
}

fn mouse(
    ui: &Ui,
    resp: &egui::Response,
    doc: &mut Document,
    rows: &Rows,
    gutter_w: f32,
    pos_at: &dyn Fn(Pos2) -> Pos,
    out: &mut ViewOutput,
) {
    let (pressed, down, mods, pointer) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers,
            i.pointer.interact_pos(),
        )
    });
    let Some(p) = pointer else { return };
    let gutter_x = ui.clip_rect().left() + gutter_w;
    let ed = &mut doc.editor;
    if pressed && resp.hovered() {
        resp.request_focus();
        out.focused = true;
        let pos = pos_at(p);
        if p.x < gutter_x {
            // Line numbers select whole lines.
            if p.x < gutter_x - rows.row_h {
                let sel = ed.line_selection(pos.line);
                ed.set_selection(sel);
                doc.view.drag = Some(Drag::Lines(pos.line));
            }
            return;
        }
        if mods.alt && mods.shift {
            let anchor = ed.box_anchor.unwrap_or(ed.caret());
            ed.box_select(anchor, pos);
            doc.view.drag = Some(Drag::Box(anchor));
        } else if mods.alt {
            ed.add_selection(Selection::caret(pos));
            doc.view.drag = Some(Drag::Text);
        } else if mods.shift {
            let anchor = ed.primary().anchor;
            ed.set_selection(Selection::new(anchor, pos));
            doc.view.drag = Some(Drag::Text);
        } else if mods.command {
            // Ctrl+click: go to definition.
            ed.set_caret(pos);
            out.cmds.push(Cmd::GoToDefinition);
        } else {
            ed.set_caret(pos);
            doc.view.drag = Some(Drag::Text);
        }
    } else if down && let Some(drag) = doc.view.drag {
        let pos = pos_at(p);
        match drag {
            Drag::Text => {
                let i = ed.primary;
                ed.sels[i].head = pos;
                ed.sels[i].goal = None;
            }
            Drag::Box(anchor) => ed.box_select(anchor, pos),
            Drag::Lines(first) => {
                let (a, b) = (first.min(pos.line), first.max(pos.line));
                let end = ed.line_selection(b).end();
                let sel = if pos.line >= first {
                    Selection::new(Pos::new(a, 0), end)
                } else {
                    Selection::new(ed.line_selection(first).end(), Pos::new(a, 0))
                };
                ed.set_selection(sel);
            }
        }
        // Drag past the edge scrolls.
        let clip = ui.clip_rect();
        if !clip.contains(p) {
            let r = Rect::from_center_size(p, Vec2::splat(rows.row_h));
            ui.scroll_to_rect(r, None);
        }
    } else if !down && doc.view.drag.take().is_some() {
        ed.normalize();
    }
    if resp.triple_clicked() {
        let pos = pos_at(p);
        let sel = ed.line_selection(pos.line);
        ed.set_selection(sel);
    } else if resp.double_clicked() && p.x >= gutter_x {
        let sel = ed.select_word_at(pos_at(p));
        if mods.alt {
            ed.add_selection(sel);
        } else {
            ed.set_selection(sel);
        }
    }
}

fn completion_popup(ui: &Ui, c: &Completion, at: Pos2, font: &FontId) {
    egui::Area::new(ui.id().with("completion"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(260.0);
                let start = c.selected.saturating_sub(8);
                for (i, item) in c.items.iter().enumerate().skip(start).take(10) {
                    let text = format!("{} {}", item.kind.icon(), item.label);
                    let rich =
                        egui::RichText::new(text)
                            .font(font.clone())
                            .color(if i == c.selected {
                                Color32::WHITE
                            } else {
                                colors::TEXT
                            });
                    let bg = if i == c.selected {
                        colors::SELECTION
                    } else {
                        Color32::TRANSPARENT
                    };
                    egui::Frame::new().fill(bg).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(rich);
                    });
                }
                if let Some(item) = c.items.get(c.selected)
                    && !(item.signature.is_empty() && item.doc.is_empty())
                {
                    ui.separator();
                    if !item.signature.is_empty() {
                        ui.label(egui::RichText::new(&item.signature).monospace().strong());
                    }
                    if !item.doc.is_empty() {
                        ui.label(&item.doc);
                    }
                }
            });
        });
}

/// Small overview of the whole file on the right; click or drag to scroll there.
fn minimap(
    ui: &mut Ui,
    doc: &mut Document,
    tokens: &[Vec<super::lexer::Token>],
    rect: Rect,
    rows: &Rows,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(26, 26, 28));
    let n = rows.lines.len().max(1);
    let line_h = (rect.height() / n as f32).clamp(0.5, 3.0);
    let cw = 1.2;
    for (row, &l) in rows.lines.iter().enumerate() {
        let y = rect.top() + row as f32 * line_h;
        if y > rect.bottom() {
            break;
        }
        for t in tokens.get(l).map(Vec::as_slice).unwrap_or(&[]) {
            let x0 = rect.left() + 4.0 + t.start as f32 * cw;
            let x1 = (rect.left() + 4.0 + t.end as f32 * cw).min(rect.right() - 2.0);
            if x0 < x1 {
                let c = token_color(t.kind).gamma_multiply(0.6);
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + line_h * 0.8)),
                    0.0,
                    c,
                );
            }
        }
    }
    // Viewport indicator.
    let total = rows.total().max(1.0);
    let top = rect.top() + doc.view.scroll_y / total * n as f32 * line_h;
    let h = doc.view.viewport_h / total * n as f32 * line_h;
    let view = Rect::from_min_size(
        Pos2::new(rect.left(), top),
        Vec2::new(rect.width(), h.max(4.0)),
    );
    painter.rect_filled(view, 0.0, Color32::from_white_alpha(18));
    let resp = ui.interact(rect, ui.id().with("minimap"), Sense::click_and_drag());
    if (resp.clicked() || resp.dragged())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let row = ((p.y - rect.top()) / line_h) as usize;
        let y = rows.tops.get(row.min(n - 1)).copied().unwrap_or(0.0);
        doc.view.scroll_to_y = Some(y - doc.view.viewport_h * 0.5);
    }
}
