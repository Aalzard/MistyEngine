//! Text: the built-in font, line breaking/alignment, and a glyph atlas rasterised on demand.

use std::collections::HashMap;

use ab_glyph::{Font, FontRef, Glyph, point};
use glam::Vec2;

use crate::scene::TextAnchor;

/// Font measurements at a pixel size (em size, like a font size setting).
pub trait GlyphMetrics {
    fn advance(&self, c: char, px: f32) -> f32;
    fn kern(&self, a: char, b: char, px: f32) -> f32;
    /// Baseline to top of the tallest glyphs (positive).
    fn ascent(&self, px: f32) -> f32;
    /// Baseline to bottom of the lowest glyphs (negative).
    fn descent(&self, px: f32) -> f32;
    /// Baseline-to-baseline distance at line spacing 1.
    fn line_height(&self, px: f32) -> f32;
}

/// The built-in UI font (Ubuntu Light, shipped with the editor's UI library).
pub struct UiFont {
    font: FontRef<'static>,
    /// Font units → pixels at 1 px em size.
    units: f32,
}

impl Default for UiFont {
    fn default() -> Self {
        let font = FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)
            .expect("the bundled font parses");
        let units = 1.0 / font.units_per_em().unwrap_or(1000.0);
        Self { font, units }
    }
}

impl GlyphMetrics for UiFont {
    fn advance(&self, c: char, px: f32) -> f32 {
        self.font.h_advance_unscaled(self.font.glyph_id(c)) * self.units * px
    }
    fn kern(&self, a: char, b: char, px: f32) -> f32 {
        let (a, b) = (self.font.glyph_id(a), self.font.glyph_id(b));
        self.font.kern_unscaled(a, b) * self.units * px
    }
    fn ascent(&self, px: f32) -> f32 {
        self.font.ascent_unscaled() * self.units * px
    }
    fn descent(&self, px: f32) -> f32 {
        self.font.descent_unscaled() * self.units * px
    }
    fn line_height(&self, px: f32) -> f32 {
        (self.font.height_unscaled() + self.font.line_gap_unscaled()) * self.units * px
    }
}

impl UiFont {
    /// Coverage bitmap of `c` at `px`: (width, height, offset of its top-left from the pen
    /// position on the baseline, coverage bytes). `None` for blank glyphs (space).
    pub fn rasterize(&self, c: char, px: f32) -> Option<(u32, u32, Vec2, Vec<u8>)> {
        let scale = px * self.units * self.font.height_unscaled();
        let glyph: Glyph = self
            .font
            .glyph_id(c)
            .with_scale_and_position(scale, point(0.0, 0.0));
        let outlined = self.font.outline_glyph(glyph)?;
        let b = outlined.px_bounds();
        let (w, h) = (b.width() as u32, b.height() as u32);
        if w == 0 || h == 0 {
            return None;
        }
        let mut data = vec![0u8; (w * h) as usize];
        outlined.draw(|x, y, cov| {
            if let Some(p) = data.get_mut((y * w + x) as usize) {
                *p = (cov.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        Some((w, h, Vec2::new(b.min.x, b.min.y), data))
    }
}

/// A character placed by [`layout_text`]: pen position on the baseline, pixels from the
/// rect's top-left corner (y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedChar {
    pub ch: char,
    pub pos: Vec2,
}

fn width_of(m: &dyn GlyphMetrics, s: &str, px: f32) -> f32 {
    let mut w = 0.0;
    let mut prev = None;
    for c in s.chars() {
        if let Some(p) = prev {
            w += m.kern(p, c, px);
        }
        w += m.advance(c, px);
        prev = Some(c);
    }
    w
}

/// Break `text` into lines: explicit newlines, and at spaces when `wrap_width` is set.
/// A single word wider than the rect stays on its own line.
pub fn break_lines(
    m: &dyn GlyphMetrics,
    text: &str,
    px: f32,
    wrap_width: Option<f32>,
) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let Some(max) = wrap_width else {
            lines.push(para.to_owned());
            continue;
        };
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && width_of(m, &candidate, px) > max + 0.01 {
                lines.push(std::mem::take(&mut line));
                line = word.to_owned();
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

/// Place every visible character of `text` inside a `size` box (pixels).
pub fn layout_text(
    m: &dyn GlyphMetrics,
    text: &str,
    px: f32,
    line_spacing: f32,
    size: Vec2,
    wrap: bool,
    align: TextAnchor,
) -> Vec<PlacedChar> {
    let lines = break_lines(m, text, px, wrap.then_some(size.x));
    let (col, row) = align.cell();
    let (hf, vf) = (col as f32 * 0.5, row as f32 * 0.5);
    let line_h = m.line_height(px) * line_spacing;
    let block_h = m.ascent(px) - m.descent(px) + line_h * (lines.len() as f32 - 1.0);
    let top = (size.y - block_h) * vf;
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let baseline = top + m.ascent(px) + line_h * i as f32;
        let mut x = (size.x - width_of(m, line, px)) * hf;
        let mut prev = None;
        for c in line.chars() {
            if let Some(p) = prev {
                x += m.kern(p, c, px);
            }
            if !c.is_whitespace() {
                out.push(PlacedChar {
                    ch: c,
                    pos: Vec2::new(x, baseline),
                });
            }
            x += m.advance(c, px);
            prev = Some(c);
        }
    }
    out
}

/// The built-in font, parsed once (layout and input fields measure text with it).
pub fn builtin_font() -> &'static UiFont {
    static FONT: std::sync::OnceLock<UiFont> = std::sync::OnceLock::new();
    FONT.get_or_init(UiFont::default)
}

/// Size of the text block (widest line × lines), wrapped at `wrap_width` when set.
pub fn text_size(
    m: &dyn GlyphMetrics,
    text: &str,
    px: f32,
    line_spacing: f32,
    wrap_width: Option<f32>,
) -> Vec2 {
    let lines = break_lines(m, text, px, wrap_width);
    let w = lines.iter().map(|l| width_of(m, l, px)).fold(0.0, f32::max);
    let line_h = m.line_height(px) * line_spacing;
    let h = m.ascent(px) - m.descent(px) + line_h * (lines.len() as f32 - 1.0);
    Vec2::new(w, h)
}

/// Single-line text in a `size` box: x of each caret position (before char 0 … after the last),
/// plus the line's top and bottom, all relative to the box's top-left (pixels, y down).
pub fn caret_stops(
    m: &dyn GlyphMetrics,
    text: &str,
    px: f32,
    size: Vec2,
    align: TextAnchor,
) -> (Vec<f32>, f32, f32) {
    let (col, row) = align.cell();
    let line_w = width_of(m, text, px);
    let mut x = (size.x - line_w) * col as f32 * 0.5;
    let h = m.ascent(px) - m.descent(px);
    let top = (size.y - h) * row as f32 * 0.5;
    let mut stops = vec![x];
    let mut prev = None;
    for c in text.chars() {
        if let Some(p) = prev {
            x += m.kern(p, c, px);
        }
        x += m.advance(c, px);
        stops.push(x);
        prev = Some(c);
    }
    (stops, top, top + h)
}

/// Where a glyph image sits in the atlas and relative to the pen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphEntry {
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    /// Top-left of the bitmap relative to the pen position on the baseline.
    pub offset: Vec2,
    pub size: Vec2,
}

/// Single-channel coverage atlas filled shelf by shelf as new (char, size) pairs appear.
pub struct GlyphAtlas {
    pub size: u32,
    pub pixels: Vec<u8>,
    /// Bumped whenever `pixels` changes (the GPU copy re-uploads).
    pub version: u64,
    entries: HashMap<(char, u32), Option<GlyphEntry>>,
    cursor: (u32, u32),
    shelf_h: u32,
}

/// The atlas has no room left; clear it and try again.
#[derive(Debug, PartialEq, Eq)]
pub struct AtlasFull;

impl GlyphAtlas {
    pub fn new(size: u32) -> Self {
        Self {
            size,
            pixels: vec![0; (size * size) as usize],
            version: 1,
            entries: HashMap::new(),
            cursor: (1, 1),
            shelf_h: 0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self {
            version: self.version + 1,
            ..Self::new(self.size)
        };
    }

    /// The glyph for `c` at integer pixel size `px`, rasterising it on first use.
    /// `Ok(None)` = nothing to draw (blank glyph).
    pub fn glyph(
        &mut self,
        font: &UiFont,
        c: char,
        px: u32,
    ) -> Result<Option<GlyphEntry>, AtlasFull> {
        if let Some(e) = self.entries.get(&(c, px)) {
            return Ok(*e);
        }
        let entry = match font.rasterize(c, px as f32) {
            None => None,
            Some((w, h, offset, data)) => Some(self.insert(w, h, offset, &data)?),
        };
        self.entries.insert((c, px), entry);
        Ok(entry)
    }

    fn insert(
        &mut self,
        w: u32,
        h: u32,
        offset: Vec2,
        data: &[u8],
    ) -> Result<GlyphEntry, AtlasFull> {
        const PAD: u32 = 1;
        if w + 2 * PAD > self.size {
            return Err(AtlasFull);
        }
        if self.cursor.0 + w + PAD > self.size {
            self.cursor = (PAD, self.cursor.1 + self.shelf_h + PAD);
            self.shelf_h = 0;
        }
        if self.cursor.1 + h + PAD > self.size {
            return Err(AtlasFull);
        }
        let (x0, y0) = self.cursor;
        for y in 0..h {
            let row = ((y0 + y) * self.size + x0) as usize;
            let src = (y * w) as usize;
            self.pixels[row..row + w as usize].copy_from_slice(&data[src..src + w as usize]);
        }
        self.cursor.0 += w + PAD;
        self.shelf_h = self.shelf_h.max(h);
        self.version += 1;
        let s = self.size as f32;
        Ok(GlyphEntry {
            uv_min: Vec2::new(x0 as f32 / s, y0 as f32 / s),
            uv_max: Vec2::new((x0 + w) as f32 / s, (y0 + h) as f32 / s),
            offset,
            size: Vec2::new(w as f32, h as f32),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every glyph 10 px wide at 10 px, ascent 8, descent -2, line height 12.
    struct Mono;
    impl GlyphMetrics for Mono {
        fn advance(&self, _: char, px: f32) -> f32 {
            px
        }
        fn kern(&self, _: char, _: char, _: f32) -> f32 {
            0.0
        }
        fn ascent(&self, px: f32) -> f32 {
            px * 0.8
        }
        fn descent(&self, px: f32) -> f32 {
            -px * 0.2
        }
        fn line_height(&self, px: f32) -> f32 {
            px * 1.2
        }
    }

    #[test]
    fn wraps_at_spaces_and_newlines() {
        let lines = break_lines(&Mono, "aa bb cc\ndd", 10.0, Some(55.0));
        assert_eq!(lines, ["aa bb", "cc", "dd"]);
        let lines = break_lines(&Mono, "averylongword x", 10.0, Some(30.0));
        assert_eq!(lines, ["averylongword", "x"]);
        assert_eq!(break_lines(&Mono, "aa bb", 10.0, None), ["aa bb"]);
    }

    #[test]
    fn alignment_places_lines_in_the_box() {
        let size = Vec2::new(100.0, 50.0);
        let ul = layout_text(&Mono, "ab", 10.0, 1.0, size, true, TextAnchor::UpperLeft);
        assert_eq!(ul[0].pos, Vec2::new(0.0, 8.0));
        assert_eq!(ul[1].pos, Vec2::new(10.0, 8.0));
        let mc = layout_text(&Mono, "ab", 10.0, 1.0, size, true, TextAnchor::MiddleCenter);
        // 20 px wide line centred; one 10 px tall line centred.
        assert_eq!(mc[0].pos, Vec2::new(40.0, 20.0 + 8.0));
        let lr = layout_text(
            &Mono,
            "ab\nc",
            10.0,
            1.0,
            size,
            true,
            TextAnchor::LowerRight,
        );
        assert_eq!(lr.len(), 3);
        assert_eq!(lr[2].pos, Vec2::new(90.0, 50.0 - 2.0));
        assert_eq!(lr[0].pos.y, 50.0 - 2.0 - 12.0);
    }

    #[test]
    fn spaces_advance_but_are_not_drawn() {
        let p = layout_text(
            &Mono,
            "a b",
            10.0,
            1.0,
            Vec2::new(100.0, 20.0),
            false,
            TextAnchor::UpperLeft,
        );
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].pos.x, 20.0);
    }

    #[test]
    fn bundled_font_rasterises_into_the_atlas() {
        let font = UiFont::default();
        assert!(font.advance('W', 20.0) > font.advance('i', 20.0));
        assert!(font.ascent(20.0) > 0.0 && font.descent(20.0) < 0.0);
        let mut atlas = GlyphAtlas::new(64);
        let a = atlas.glyph(&font, 'A', 20).unwrap().expect("A has ink");
        assert!(a.size.x > 4.0 && a.offset.y < 0.0, "{a:?}");
        assert!(atlas.pixels.iter().any(|p| *p > 200));
        assert_eq!(atlas.glyph(&font, ' ', 20), Ok(None));
        let v = atlas.version;
        assert_eq!(atlas.glyph(&font, 'A', 20).unwrap(), Some(a), "cached");
        assert_eq!(atlas.version, v);
        // A tiny atlas fills up.
        let mut full = None;
        for c in 'a'..='z' {
            if let Err(e) = atlas.glyph(&font, c, 20) {
                full = Some(e);
                break;
            }
        }
        assert_eq!(full, Some(AtlasFull));
    }
}
