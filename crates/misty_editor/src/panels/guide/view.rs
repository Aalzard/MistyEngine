//! The guide reader: the chapter list and search on the side, the chapter rendered from its
//! Markdown on the right. It shows the chapters in the editor's language: Arabic right to left
//! (shaped through `i18n::shape`), any other language in English. It needs no editor state, so
//! the Hub's Learn page shows it too.

use std::collections::HashMap;

use egui::{Align, Color32, Layout, RichText, TextWrapMode, Ui};

use super::content::{self, CHAPTERS};
use super::markdown::{self, Block, NoteKind, Span};
use crate::i18n::{self, Language, shape, tr};

/// What the reader asks of its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuideAction {
    /// "Open example" on a guide chapter (its id).
    OpenExample(&'static str),
}

/// Reader state: the chapter shown, the search, and the pictures loaded so far.
#[derive(Default)]
pub struct GuideView {
    pub chapter: usize,
    pub search: String,
    /// Bring this block of the chapter into view when it is next drawn.
    scroll_to: Option<usize>,
    /// Scroll back to the top (a new chapter).
    to_top: bool,
    /// Parsed chapters by (chapter, language).
    parsed: HashMap<(usize, Language), Vec<Block>>,
    textures: HashMap<&'static str, egui::TextureHandle>,
}

/// The language the chapters are read in: Arabic, else English.
fn reading_language() -> Language {
    match i18n::language() {
        Language::Arabic => Language::Arabic,
        _ => Language::English,
    }
}

impl GuideView {
    /// Show chapter `id` (a file name such as `05-guide-first-person`); false if unknown.
    pub fn open(&mut self, id: &str) -> bool {
        match CHAPTERS.iter().position(|c| c.id == id) {
            Some(i) => {
                self.go(i, None);
                true
            }
            None => false,
        }
    }

    fn go(&mut self, chapter: usize, block: Option<usize>) {
        self.chapter = chapter.min(CHAPTERS.len() - 1);
        self.scroll_to = block;
        self.to_top = block.is_none();
    }

    fn blocks(&mut self, chapter: usize, lang: Language) -> &[Block] {
        self.parsed
            .entry((chapter, lang))
            .or_insert_with(|| markdown::parse(CHAPTERS[chapter].text(lang)))
    }

    /// A chapter's title: its first heading.
    fn title(&mut self, chapter: usize, lang: Language) -> String {
        self.blocks(chapter, lang)
            .iter()
            .find_map(|b| match b {
                Block::Heading { text, .. } => Some(markdown::plain(text)),
                _ => None,
            })
            .unwrap_or_else(|| CHAPTERS[chapter].id.to_owned())
    }

    /// Follow a link of the current chapter: another chapter (`02-performance.md`, with an
    /// optional `#anchor`) or a heading of this one (`#anchor`). Web links are ignored.
    fn follow(&mut self, target: &str, lang: Language) {
        let (file, anchor) = target.split_once('#').unwrap_or((target, ""));
        let chapter = if file.is_empty() {
            Some(self.chapter)
        } else {
            let id = file
                .rsplit('/')
                .next()
                .unwrap_or(file)
                .trim_end_matches(".md");
            CHAPTERS.iter().position(|c| c.id == id)
        };
        let Some(chapter) = chapter else {
            return;
        };
        let block = (!anchor.is_empty())
            .then(|| {
                self.blocks(chapter, lang).iter().position(|b| {
                    matches!(b, Block::Heading { text, .. } if markdown::anchor(text) == anchor)
                })
            })
            .flatten();
        self.go(chapter, block);
    }
}

/// Blocks of every chapter (in `lang`) whose text contains `query`: (chapter, block, snippet).
pub fn search(view: &mut GuideView, query: &str, lang: Language) -> Vec<(usize, usize, String)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for c in 0..CHAPTERS.len() {
        for (i, b) in view.blocks(c, lang).iter().enumerate() {
            let text = block_text(b);
            if let Some(at) = text.to_lowercase().find(&q) {
                out.push((c, i, snippet(&text, at)));
            }
        }
    }
    out
}

fn block_text(b: &Block) -> String {
    match b {
        Block::Heading { text, .. } | Block::Paragraph(text) => markdown::plain(text),
        Block::List { items, .. } => items
            .iter()
            .map(|(_, t)| markdown::plain(t))
            .collect::<Vec<_>>()
            .join(" "),
        Block::Table { header, rows } => header
            .iter()
            .chain(rows.iter().flatten())
            .map(|c| markdown::plain(c))
            .collect::<Vec<_>>()
            .join(" "),
        Block::Code { text, .. } => text.clone(),
        Block::Image { alt, .. } => alt.clone(),
        Block::Note { blocks, .. } => blocks.iter().map(block_text).collect::<Vec<_>>().join(" "),
    }
}

/// About 70 characters around byte offset `at` of `text` (in the lowercased text, which has
/// the same character count for the scripts the guide uses).
fn snippet(text: &str, at: usize) -> String {
    let lower = text.to_lowercase();
    let char_at = lower[..at.min(lower.len())].chars().count();
    let chars: Vec<char> = text.chars().collect();
    let start = char_at.saturating_sub(25);
    let end = (start + 70).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    if start > 0 {
        s.insert(0, '…');
    }
    if end < chars.len() {
        s.push('…');
    }
    s
}

/// Draw the reader filling `ui`. `examples`: show "Open example" on the guide chapters.
pub fn show(ui: &mut Ui, view: &mut GuideView, examples: bool) -> Option<GuideAction> {
    let lang = reading_language();
    let rtl = lang.is_rtl();
    let mut action = None;
    let side = if rtl {
        egui::Panel::right("guide_chapters")
    } else {
        egui::Panel::left("guide_chapters")
    };
    side.resizable(true)
        .default_size(210.0)
        .show(ui, |ui| sidebar(ui, view, lang));
    egui::CentralPanel::default().show(ui, |ui| {
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("guide_page", view.chapter))
            .auto_shrink(false);
        if std::mem::take(&mut view.to_top) {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(860.0));
            ui.add_space(6.0);
            let id = CHAPTERS[view.chapter].id;
            if examples && content::example(id).is_some() {
                let layout = if rtl {
                    Layout::right_to_left(Align::Center)
                } else {
                    Layout::left_to_right(Align::Center)
                };
                // One row (a horizontal layout straight in the scroll area would centre the
                // button in its whole height).
                ui.horizontal(|ui| {
                    ui.with_layout(layout, |ui| {
                        let b = ui
                            .button(format!("▶ {}", tr!("Open example")))
                            .on_hover_text(tr!("Open this guide's scene and Script Graph"));
                        if b.clicked() {
                            action = Some(GuideAction::OpenExample(id));
                        }
                    });
                });
                ui.add_space(4.0);
            }
            let blocks = view.blocks(view.chapter, lang).to_vec();
            let mut page = Page {
                view,
                rtl,
                link: None,
            };
            for (i, b) in blocks.iter().enumerate() {
                let top = ui.cursor().top();
                page.block(ui, b, 0);
                if page.view.scroll_to == Some(i) {
                    page.view.scroll_to = None;
                    let r = egui::Rect::from_min_max(
                        egui::pos2(ui.min_rect().left(), top),
                        egui::pos2(ui.min_rect().right(), ui.cursor().top()),
                    );
                    ui.scroll_to_rect(r, Some(Align::TOP));
                }
            }
            ui.add_space(24.0);
            pager(ui, page.view, lang);
            if let Some(target) = page.link.take() {
                view.follow(&target, lang);
            }
            ui.add_space(16.0);
        });
    });
    action
}

fn sidebar(ui: &mut Ui, view: &mut GuideView, lang: Language) {
    let rtl = lang.is_rtl();
    ui.add_space(6.0);
    ui.add(
        egui::TextEdit::singleline(&mut view.search)
            .hint_text(format!("🔍 {}", tr!("Search the guide")))
            .desired_width(f32::INFINITY),
    );
    ui.add_space(6.0);
    egui::ScrollArea::vertical()
        .id_salt("guide_side")
        .auto_shrink(false)
        .show(ui, |ui| {
            let query = view.search.clone();
            if !query.trim().is_empty() {
                let hits = search(view, &query, lang);
                if hits.is_empty() {
                    ui.weak(tr!("No matches."));
                }
                for (c, b, text) in hits.into_iter().take(60) {
                    let title = view.title(c, lang);
                    let label = format!("{}\n{}", i18n::display(&title), i18n::display(&text));
                    let r = ui.add(
                        egui::Button::selectable(false, RichText::new(label).small())
                            .wrap_mode(TextWrapMode::Wrap),
                    );
                    if r.clicked() {
                        view.go(c, Some(b));
                    }
                }
                return;
            }
            for c in 0..CHAPTERS.len() {
                let title = view.title(c, lang);
                let layout = if rtl {
                    Layout::top_down(Align::Max)
                } else {
                    Layout::top_down(Align::Min)
                };
                let selected = view.chapter == c;
                let r = ui
                    .with_layout(layout, |ui| {
                        ui.add(
                            egui::Button::selectable(selected, i18n::display(&title).into_owned())
                                .wrap_mode(TextWrapMode::Wrap),
                        )
                    })
                    .inner;
                if r.clicked() && !selected {
                    view.go(c, None);
                }
            }
        });
}

/// "◀ Previous" / "Next ▶" at the end of a chapter.
fn pager(ui: &mut Ui, view: &mut GuideView, lang: Language) {
    ui.separator();
    let prev = view.chapter.checked_sub(1);
    let next = (view.chapter + 1 < CHAPTERS.len()).then_some(view.chapter + 1);
    let prev_title = prev.map(|c| view.title(c, lang));
    let next_title = next.map(|c| view.title(c, lang));
    ui.horizontal(|ui| {
        if let (Some(c), Some(t)) = (prev, &prev_title)
            && ui.button(format!("◀ {}", i18n::display(t))).clicked()
        {
            view.go(c, None);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let (Some(c), Some(t)) = (next, &next_title)
                && ui.button(format!("{} ▶", i18n::display(t))).clicked()
            {
                view.go(c, None);
            }
        });
    });
}

/// Drawing one chapter.
struct Page<'a> {
    view: &'a mut GuideView,
    rtl: bool,
    /// A link clicked this frame.
    link: Option<String>,
}

fn note_style(kind: NoteKind) -> (Color32, &'static str, &'static str) {
    match kind {
        NoteKind::Note => (Color32::from_rgb(90, 150, 230), "ℹ", "Note"),
        NoteKind::Tip => (Color32::from_rgb(110, 196, 120), "💡", "Tip"),
        NoteKind::Warning => (Color32::from_rgb(230, 180, 60), "⚠", "Warning"),
        NoteKind::Programmers => (Color32::from_rgb(180, 140, 230), "⌨", "For programmers"),
    }
}

impl Page<'_> {
    fn block(&mut self, ui: &mut Ui, b: &Block, depth: usize) {
        match b {
            Block::Heading { level, text } => {
                let size = match level {
                    1 => 24.0,
                    2 => 19.0,
                    _ => 16.0,
                };
                ui.add_space(if *level == 1 { 4.0 } else { 12.0 });
                let plain = markdown::plain(text);
                let rich = RichText::new(self.line(&plain)).size(size).strong();
                self.aligned(ui, |ui| {
                    ui.add(egui::Label::new(rich).wrap_mode(self.wrap()));
                });
                ui.add_space(4.0);
            }
            Block::Paragraph(text) => {
                self.text(ui, text, None);
                ui.add_space(6.0);
            }
            Block::List { ordered, items } => {
                for (n, (d, item)) in items.iter().enumerate() {
                    let mark = if *ordered {
                        format!("{}.", n + 1)
                    } else if d % 2 == 1 {
                        "◦".to_owned()
                    } else {
                        "•".to_owned()
                    };
                    let indent = 18.0 * (*d + depth) as f32;
                    self.text(ui, item, Some((mark, indent)));
                    ui.add_space(2.0);
                }
                ui.add_space(4.0);
            }
            Block::Code { text, .. } => {
                egui::Frame::new()
                    .fill(ui.visuals().code_bg_color)
                    .corner_radius(4.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::horizontal()
                            .id_salt(ui.next_auto_id())
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(text).monospace())
                                        .wrap_mode(TextWrapMode::Extend),
                                );
                            });
                    });
                ui.add_space(6.0);
            }
            Block::Image { alt, path } => {
                self.image(ui, alt, path);
                ui.add_space(8.0);
            }
            Block::Table { header, rows } => {
                self.table(ui, header, rows);
                ui.add_space(8.0);
            }
            Block::Note { kind, blocks } => {
                let (color, icon, title) = note_style(*kind);
                egui::Frame::new()
                    .fill(color.gamma_multiply(0.12))
                    .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.6)))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let head = format!("{icon} {}", tr(title));
                        self.aligned(ui, |ui| {
                            ui.label(RichText::new(head).strong().color(color));
                        });
                        for b in blocks {
                            self.block(ui, b, depth);
                        }
                    });
                ui.add_space(8.0);
            }
        }
    }

    fn wrap(&self) -> TextWrapMode {
        if self.rtl {
            TextWrapMode::Extend
        } else {
            TextWrapMode::Wrap
        }
    }

    /// One line of text for drawing: shaped right to left in Arabic.
    fn line(&self, text: &str) -> String {
        if self.rtl {
            shape::visual(text, shape::Base::Rtl).into_owned()
        } else {
            text.to_owned()
        }
    }

    /// Lay `add` out from the reading side.
    fn aligned<R>(&self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
        let layout = if self.rtl {
            Layout::top_down(Align::Max)
        } else {
            Layout::top_down(Align::Min)
        };
        ui.with_layout(layout, add).inner
    }

    /// A paragraph or list item (`mark`: its bullet and indent).
    fn text(&mut self, ui: &mut Ui, text: &str, mark: Option<(String, f32)>) {
        let spans = markdown::inline(text);
        if self.rtl {
            self.text_rtl(ui, &spans, mark);
            return;
        }
        ui.horizontal(|ui| {
            if let Some((m, indent)) = &mark {
                ui.add_space(*indent);
                ui.add_sized([16.0, 0.0], egui::Label::new(m.as_str()));
            }
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for s in &spans {
                        self.span(ui, s);
                    }
                });
            });
        });
    }

    fn span(&mut self, ui: &mut Ui, s: &Span) {
        match s {
            Span::Text(t) => {
                ui.label(t.as_str());
            }
            Span::Bold(t) => {
                ui.label(RichText::new(t).strong());
            }
            Span::Italic(t) => {
                ui.label(RichText::new(t).italics());
            }
            Span::Code(t) => {
                ui.label(
                    RichText::new(t)
                        .monospace()
                        .background_color(ui.visuals().code_bg_color),
                );
            }
            Span::Link { text, target } => {
                if ui.link(text.as_str()).clicked() {
                    self.link = Some(target.clone());
                }
            }
        }
    }

    /// Right-to-left text: shaped and broken to the width, without inline styles; its links
    /// follow as buttons.
    fn text_rtl(&mut self, ui: &mut Ui, spans: &[Span], mark: Option<(String, f32)>) {
        let plain: String = spans
            .iter()
            .map(|s| match s {
                Span::Text(t) | Span::Bold(t) | Span::Italic(t) | Span::Code(t) => t.as_str(),
                Span::Link { text, .. } => text.as_str(),
            })
            .collect();
        let indent = mark.as_ref().map_or(0.0, |(_, i)| *i + 18.0);
        let width = (ui.available_width() - indent - 8.0).max(80.0);
        let shaped = i18n::wrap_rtl(ui, &plain, width);
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.add_space(indent - if mark.is_some() { 18.0 } else { 0.0 });
            if let Some((m, _)) = &mark {
                ui.add_sized([16.0, 0.0], egui::Label::new(m.as_str()));
            }
            ui.with_layout(Layout::top_down(Align::Max), |ui| {
                ui.add(egui::Label::new(shaped).wrap_mode(TextWrapMode::Extend));
                for s in spans {
                    if let Span::Link { text, target } = s
                        && ui.link(i18n::display(text)).clicked()
                    {
                        self.link = Some(target.clone());
                    }
                }
            });
        });
    }

    fn cell(&mut self, ui: &mut Ui, text: &str, width: f32, strong: bool) {
        let plain = markdown::plain(text);
        if self.rtl {
            let shaped = i18n::wrap_rtl(ui, &plain, width);
            let mut rich = RichText::new(shaped);
            if strong {
                rich = rich.strong();
            }
            ui.with_layout(Layout::top_down(Align::Max), |ui| {
                ui.add(egui::Label::new(rich).wrap_mode(TextWrapMode::Extend));
            });
        } else {
            ui.set_max_width(width);
            let mut rich = RichText::new(plain);
            if strong {
                rich = rich.strong();
            }
            ui.add(egui::Label::new(rich).wrap_mode(TextWrapMode::Wrap));
        }
    }

    fn table(&mut self, ui: &mut Ui, header: &[String], rows: &[Vec<String>]) {
        let cols = header.len().max(1);
        let width = ((ui.available_width() - 12.0 * cols as f32) / cols as f32).max(70.0);
        let order: Vec<usize> = if self.rtl {
            (0..cols).rev().collect()
        } else {
            (0..cols).collect()
        };
        egui::Frame::new()
            .stroke(egui::Stroke::new(
                1.0,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ))
            .corner_radius(4.0)
            .inner_margin(6.0)
            .show(ui, |ui| {
                egui::Grid::new(ui.next_auto_id())
                    .striped(true)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        for &c in &order {
                            self.cell(ui, header.get(c).map_or("", |s| s), width, true);
                        }
                        ui.end_row();
                        for row in rows {
                            for &c in &order {
                                self.cell(ui, row.get(c).map_or("", |s| s), width, false);
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    fn image(&mut self, ui: &mut Ui, alt: &str, path: &str) {
        let name = path.rsplit('/').next().unwrap_or(path);
        let tex = match content::IMAGES.iter().find(|(n, _)| *n == name) {
            Some((n, bytes)) => match self.view.textures.get(n) {
                Some(t) => Some(t.clone()),
                None => {
                    let t = load(ui.ctx(), n, bytes);
                    if let Some(t) = &t {
                        self.view.textures.insert(n, t.clone());
                    }
                    t
                }
            },
            None => None,
        };
        self.aligned(ui, |ui| match tex {
            Some(t) => {
                let size = t.size_vec2();
                let w = size.x.min(ui.available_width());
                let h = size.y * w / size.x.max(1.0);
                ui.add(egui::Image::new((t.id(), egui::vec2(w, h))).corner_radius(4.0));
                ui.label(RichText::new(i18n::display(alt)).small().weak());
            }
            None => {
                ui.weak(format!("[{}]", i18n::display(alt)));
            }
        });
    }
}

fn load(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let img = misty_engine::image::load_from_memory(bytes)
        .ok()?
        .into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(format!("guide/{name}"), color, egui::TextureOptions::LINEAR))
}
