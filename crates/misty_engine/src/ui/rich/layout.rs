//! Rich text layout: sizes per character, right-to-left reordering and shaping, word wrapping
//! with character / word / line spacing, per-line alignment, overflow modes (overflow,
//! ellipsis, truncate, pages) and auto-size. Positions are screen pixels relative to the
//! rect's top-left corner (y down), pens on the baseline.

use glam::Vec2;

use super::fonts::{FontId, FontSource};
use super::parse::{HAlign, OBJECT, RichText, Script};
use crate::scene::{TextAnchor, TextOverflow};
use crate::ui::layout::Rect;

/// What the layout needs besides the text.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutParams {
    /// Base em size in screen pixels.
    pub px: f32,
    /// Canvas units → screen pixels (absolute `<size>` values, auto-size limits).
    pub scale: f32,
    /// Rect size in screen pixels.
    pub size: Vec2,
    pub align: TextAnchor,
    pub wrap: bool,
    pub line_spacing: f32,
    /// Extra space after each character, in hundredths of an em.
    pub char_spacing: f32,
    /// Extra space added to spaces, in hundredths of an em.
    pub word_spacing: f32,
    pub overflow: TextOverflow,
    /// Page shown in Page mode (1 = first).
    pub page: u32,
    /// Auto-size range in canvas units (`None` = fixed size).
    pub auto_size: Option<(f32, f32)>,
    /// sRGB colour + alpha of text without a `<color>`.
    pub color: [f32; 4],
    /// Width / height of each inline sprite (by sprite tag index).
    pub sprite_aspects: Vec<f32>,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            px: 14.0,
            scale: 1.0,
            size: Vec2::new(160.0, 30.0),
            align: TextAnchor::UpperLeft,
            wrap: true,
            line_spacing: 1.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            overflow: TextOverflow::Overflow,
            page: 1,
            auto_size: None,
            color: [0.2, 0.2, 0.2, 1.0],
            sprite_aspects: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub font: FontId,
    pub glyph: u16,
    /// Em size in screen pixels.
    pub px: f32,
    /// Pen position on the baseline.
    pub pos: Vec2,
    pub color: [f32; 4],
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedSprite {
    /// Index into [`RichText::sprites`].
    pub sprite: u16,
    pub min: Vec2,
    pub max: Vec2,
    /// The text colour where it was placed (used by `tint=1`).
    pub text_color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecoKind {
    /// `<mark>` highlight (behind the text).
    Mark,
    Underline,
    Strike,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deco {
    pub kind: DecoKind,
    pub min: Vec2,
    pub max: Vec2,
    pub color: [f32; 4],
}

/// The area of one line of a link.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkBox {
    pub id: String,
    pub rect: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichLayout {
    pub glyphs: Vec<PlacedGlyph>,
    pub sprites: Vec<PlacedSprite>,
    pub decos: Vec<Deco>,
    pub links: Vec<LinkBox>,
    /// Lines shown.
    pub lines: usize,
    /// Pages of the text (Page mode; 1 otherwise).
    pub pages: usize,
    /// Size of the shown text block.
    pub block: Vec2,
    /// Em size used (differs from the requested one with auto-size).
    pub px: f32,
    /// Some text didn't fit (cut by ellipsis/truncate/pages, or spilling out in Overflow mode).
    pub overflowed: bool,
}

impl RichLayout {
    /// The link under `p` (rect-relative pixels).
    pub fn link_at(&self, p: Vec2) -> Option<&str> {
        self.links
            .iter()
            .find(|l| l.rect.contains(p))
            .map(|l| l.id.as_str())
    }
}

/// One character (or shaped cluster) ready for line breaking.
#[derive(Clone, Debug)]
struct Unit {
    ci: usize,
    /// (font, glyph, x offset, y offset) in visual order.
    glyphs: Vec<(FontId, u16, f32, f32)>,
    advance: f32,
    /// Kerning with the previous unit (applied when both are on the same line).
    kern: f32,
    px: f32,
    shift: f32,
    ascent: f32,
    descent: f32,
    line_h: f32,
    space: bool,
    newline: bool,
    level: u8,
}

/// Is `c` from a script written right to left?
pub fn is_rtl_char(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}

/// The mirror image of a bracket-like character drawn in a right-to-left run.
pub fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        _ => c,
    }
}

/// Bidi embedding level of every character (even = left to right).
fn bidi_levels(text: &str) -> Vec<u8> {
    if !text.chars().any(is_rtl_char) {
        return vec![0; text.chars().count()];
    }
    let info = unicode_bidi::BidiInfo::new(text, None);
    text.char_indices()
        .map(|(b, _)| info.levels.get(b).map_or(0, |l| l.number()))
        .collect()
}

fn units(rich: &RichText, fonts: &dyn FontSource, p: &LayoutParams, px: f32) -> Vec<Unit> {
    let plain = rich.plain();
    let levels = bidi_levels(&plain);
    let byte_of: Vec<usize> = plain.char_indices().map(|(b, _)| b).collect();
    let em = |u: f32| u * px / 100.0;
    let mut out: Vec<Unit> = Vec::with_capacity(rich.chars.len());
    for (ci, rc) in rich.chars.iter().enumerate() {
        let st = &rc.style;
        let mut size = st.size.map_or(px, |s| {
            // Relative sizes scale the (possibly auto-sized) base; absolute ones are canvas units.
            match s {
                super::parse::SizeSpec::Abs(v) => v * p.scale,
                super::parse::SizeSpec::Rel(f) => px * f,
                super::parse::SizeSpec::Add(d) => px + d * p.scale,
            }
            .max(0.5)
        });
        let shift = match st.script {
            Script::Normal => 0.0,
            Script::Super => {
                size *= 0.6;
                -size * 0.55
            }
            Script::Sub => {
                size *= 0.6;
                size * 0.25
            }
        };
        let mut u = Unit {
            ci,
            glyphs: Vec::new(),
            advance: 0.0,
            kern: 0.0,
            px: size,
            shift,
            ascent: fonts.ascent(size) - shift,
            descent: fonts.descent(size) - shift,
            line_h: fonts.line_height(size),
            space: rc.ch == ' ' || rc.ch == '\u{3000}',
            newline: rc.ch == '\n',
            level: levels.get(ci).copied().unwrap_or(0),
        };
        if u.newline {
            out.push(u);
            continue;
        }
        if rc.ch == OBJECT {
            let aspect = rc
                .sprite
                .and_then(|s| p.sprite_aspects.get(s as usize).copied())
                .unwrap_or(1.0);
            u.advance = size * aspect + em(p.char_spacing);
            u.ascent = u.ascent.max(size * 0.85);
            out.push(u);
            continue;
        }
        // Brackets in right-to-left runs show their mirror image (UAX #9 rule L4).
        let ch = if u.level % 2 == 1 {
            mirrored(rc.ch)
        } else {
            rc.ch
        };
        let (font, glyph) = fonts.lookup(ch);
        u.glyphs.push((font, glyph, 0.0, 0.0));
        u.advance = fonts.advance(font, glyph, size) + em(p.char_spacing);
        if st.bold {
            u.advance += size * 0.02;
        }
        if u.space {
            u.advance += em(p.word_spacing);
        }
        if let Some(prev) = out.last()
            && prev.px == size
            && prev.level.is_multiple_of(2)
            && u.level.is_multiple_of(2)
            && let (Some(a), Some(b)) = (prev.glyphs.last(), u.glyphs.first())
            && a.0 == font
        {
            u.kern = fonts.kern(font, a.1, b.1, size);
        }
        out.push(u);
    }
    shape_rtl_runs(&mut out, rich, fonts, &plain, &byte_of, p);
    out
}

/// Replace per-character glyphs of right-to-left runs with shaped ones (Arabic joining forms,
/// marks), keeping one unit per character.
fn shape_rtl_runs(
    units: &mut [Unit],
    rich: &RichText,
    fonts: &dyn FontSource,
    plain: &str,
    byte_of: &[usize],
    p: &LayoutParams,
) {
    let mut i = 0;
    while i < units.len() {
        let u = &units[i];
        if u.level.is_multiple_of(2) || u.glyphs.is_empty() || !is_rtl_char(rich.chars[u.ci].ch) {
            i += 1;
            continue;
        }
        let (font, px) = (u.glyphs[0].0, u.px);
        let mut j = i + 1;
        while j < units.len() {
            let v = &units[j];
            let same = v.level % 2 == 1
                && v.px == px
                && !v.newline
                && v.glyphs.first().is_some_and(|g| g.0 == font);
            if !same {
                break;
            }
            j += 1;
        }
        let (b0, b1) = (
            byte_of[units[i].ci],
            byte_of
                .get(units[j - 1].ci + 1)
                .copied()
                .unwrap_or(plain.len()),
        );
        if let Some(shaped) = fonts.shape(font, &plain[b0..b1], true, px) {
            let spacing = p.char_spacing * px / 100.0;
            for u in &mut units[i..j] {
                u.glyphs.clear();
                u.advance = 0.0;
                u.kern = 0.0;
            }
            for g in shaped {
                // The unit whose character starts this cluster.
                let k = units[i..j]
                    .iter()
                    .rposition(|u| byte_of[u.ci] - b0 <= g.cluster)
                    .unwrap_or(0);
                let u = &mut units[i + k];
                u.glyphs
                    .push((font, g.glyph, u.advance + g.x_offset, g.y_offset));
                u.advance += g.advance;
            }
            for u in &mut units[i..j] {
                u.advance += spacing;
            }
        }
        i = j;
    }
}

/// Lines as unit ranges (logical order), by explicit breaks and, when wrapping, at spaces.
fn break_lines(units: &[Unit], wrap: Option<f32>) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut width = 0.0;
    let mut last_space: Option<usize> = None;
    let mut i = 0;
    while i < units.len() {
        let u = &units[i];
        if u.newline {
            lines.push(start..i);
            start = i + 1;
            width = 0.0;
            last_space = None;
            i += 1;
            continue;
        }
        let w = u.advance + if i > start { u.kern } else { 0.0 };
        if let Some(max) = wrap
            && !u.space
            && i > start
            && width + w > max + 0.01
            && let Some(b) = last_space.filter(|b| *b > start)
        {
            lines.push(start..b);
            start = b;
            last_space = None;
            width = units[start..i]
                .iter()
                .enumerate()
                .map(|(k, u)| u.advance + if k > 0 { u.kern } else { 0.0 })
                .sum();
            continue;
        }
        width += w;
        if u.space {
            last_space = Some(i + 1);
        }
        i += 1;
    }
    lines.push(start..units.len());
    lines
}

/// Line width without trailing spaces.
fn line_width(units: &[Unit], r: &std::ops::Range<usize>) -> f32 {
    let mut end = r.end;
    while end > r.start && units[end - 1].space {
        end -= 1;
    }
    (r.start..end)
        .map(|i| units[i].advance + if i > r.start { units[i].kern } else { 0.0 })
        .sum()
}

struct LineInfo {
    range: std::ops::Range<usize>,
    ascent: f32,
    descent: f32,
    line_h: f32,
    width: f32,
}

fn line_info(
    units: &[Unit],
    range: std::ops::Range<usize>,
    fonts: &dyn FontSource,
    px: f32,
) -> LineInfo {
    let us = &units[range.clone()];
    let visible = us.iter().filter(|u| !u.newline);
    let (mut a, mut d, mut lh) = (f32::MIN, f32::MAX, f32::MIN);
    for u in visible {
        a = a.max(u.ascent);
        d = d.min(u.descent);
        lh = lh.max(u.line_h);
    }
    if a == f32::MIN {
        // Empty line: the base size's metrics.
        a = fonts.ascent(px);
        d = fonts.descent(px);
        lh = fonts.line_height(px);
    }
    LineInfo {
        width: line_width(units, &range),
        range,
        ascent: a,
        descent: d,
        line_h: lh,
    }
}

/// Height of lines `ls` stacked: first ascent to last descent.
fn block_height(ls: &[LineInfo], spacing: f32) -> f32 {
    match (ls.first(), ls.last()) {
        (Some(f), Some(l)) => {
            f.ascent - l.descent + ls[1..].iter().map(|x| x.line_h * spacing).sum::<f32>()
        }
        _ => 0.0,
    }
}

/// Reorder a line's units for display (Unicode rule L2: reverse runs from the highest level
/// down to the lowest odd level).
fn visual_order(units: &[Unit], range: std::ops::Range<usize>) -> Vec<usize> {
    let mut order: Vec<usize> = range.collect();
    let levels: Vec<u8> = order.iter().map(|&i| units[i].level).collect();
    let (Some(&max), Some(min_odd)) = (
        levels.iter().max(),
        levels.iter().copied().filter(|l| l % 2 == 1).min(),
    ) else {
        return order;
    };
    let mut lv = levels;
    let mut level = max;
    while level >= min_odd {
        let mut i = 0;
        while i < order.len() {
            if lv[i] >= level {
                let mut j = i;
                while j < order.len() && lv[j] >= level {
                    j += 1;
                }
                order[i..j].reverse();
                lv[i..j].reverse();
                i = j;
            } else {
                i += 1;
            }
        }
        if level == 0 {
            break;
        }
        level -= 1;
    }
    order
}

/// Whether text at em size `px` fits the rect.
fn fits(lines: &[LineInfo], p: &LayoutParams) -> bool {
    let h = block_height(lines, p.line_spacing);
    h <= p.size.y + 0.5 && lines.iter().all(|l| l.width <= p.size.x + 0.5)
}

fn ellipsis_unit(fonts: &dyn FontSource, like: &Unit) -> Unit {
    let (font, glyph) = fonts.lookup('\u{2026}');
    let (glyphs, advance) = if glyph != 0 {
        (
            vec![(font, glyph, 0.0, 0.0)],
            fonts.advance(font, glyph, like.px),
        )
    } else {
        let (f, g) = fonts.lookup('.');
        let a = fonts.advance(f, g, like.px);
        ((0..3).map(|k| (f, g, a * k as f32, 0.0)).collect(), a * 3.0)
    };
    Unit {
        glyphs,
        advance,
        kern: 0.0,
        space: false,
        newline: false,
        level: 0,
        ..like.clone()
    }
}

/// Lay out parsed rich text.
pub fn layout(rich: &RichText, fonts: &dyn FontSource, p: &LayoutParams) -> RichLayout {
    let wrap = p.wrap.then_some(p.size.x);
    let mut px = p.px;
    if let Some((lo, hi)) = p.auto_size {
        // Largest size in the range that fits (binary search; the smallest when none does).
        let (mut lo, mut hi) = ((lo * p.scale).max(0.5), (hi * p.scale).max(0.5));
        if lo > hi {
            std::mem::swap(&mut lo, &mut hi);
        }
        let measure = |s: f32| {
            let us = units(rich, fonts, p, s);
            let ls: Vec<LineInfo> = break_lines(&us, wrap)
                .into_iter()
                .map(|r| line_info(&us, r, fonts, s))
                .collect();
            fits(&ls, p)
        };
        if measure(hi) {
            px = hi;
        } else {
            px = lo;
            for _ in 0..12 {
                let mid = (lo + hi) * 0.5;
                if measure(mid) {
                    lo = mid;
                    px = mid;
                } else {
                    hi = mid;
                }
                if hi - lo < 0.25 {
                    break;
                }
            }
        }
    }
    let mut us = units(rich, fonts, p, px);
    let mut lines: Vec<LineInfo> = break_lines(&us, wrap)
        .into_iter()
        .map(|r| line_info(&us, r, fonts, px))
        .collect();
    let mut out = RichLayout {
        px,
        pages: 1,
        ..Default::default()
    };
    out.overflowed = !fits(&lines, p);

    // Overflow modes: which lines show, and the ellipsis.
    let mut ellipsize: Vec<usize> = Vec::new();
    match p.overflow {
        TextOverflow::Overflow => {}
        TextOverflow::Truncate | TextOverflow::Ellipsis => {
            let mut keep = lines.len();
            while keep > 1 && block_height(&lines[..keep], p.line_spacing) > p.size.y + 0.5 {
                keep -= 1;
            }
            let cut = keep < lines.len();
            lines.truncate(keep);
            if p.overflow == TextOverflow::Ellipsis {
                if cut {
                    ellipsize.push(keep - 1);
                }
                for (i, l) in lines.iter().enumerate() {
                    if l.width > p.size.x + 0.5 && !ellipsize.contains(&i) {
                        ellipsize.push(i);
                    }
                }
            } else if !p.wrap {
                // Truncate also drops characters past the right edge of unwrapped lines.
                for l in &mut lines {
                    while l.width > p.size.x + 0.5 && l.range.end > l.range.start + 1 {
                        l.range.end -= 1;
                        l.width = line_width(&us, &l.range);
                    }
                }
            }
        }
        TextOverflow::Page => {
            let mut pages: Vec<std::ops::Range<usize>> = Vec::new();
            let mut start = 0;
            for i in 1..=lines.len() {
                if i == lines.len()
                    || block_height(&lines[start..=i], p.line_spacing) > p.size.y + 0.5
                {
                    pages.push(start..i);
                    start = i;
                }
            }
            out.pages = pages.len().max(1);
            let page = (p.page.max(1) as usize).min(pages.len()).max(1);
            if let Some(r) = pages.get(page - 1).cloned() {
                lines = lines.drain(r).collect();
            }
        }
    }
    for &li in &ellipsize {
        let max = p.size.x;
        let l = &mut lines[li];
        let Some(last) = (l.range.start..l.range.end)
            .rev()
            .find(|&i| !us[i].space)
            .map(|i| us[i].clone())
        else {
            continue;
        };
        let ell = ellipsis_unit(fonts, &last);
        while l.range.end > l.range.start && line_width(&us, &l.range) + ell.advance > max + 0.5 {
            l.range.end -= 1;
        }
        // Drop the spaces before the ellipsis, then insert it after the line.
        while l.range.end > l.range.start && us[l.range.end - 1].space {
            l.range.end -= 1;
        }
        let at = l.range.end;
        us.insert(at, ell);
        let shift = |r: &mut std::ops::Range<usize>| {
            if r.start > at {
                r.start += 1;
            }
            if r.end >= at {
                r.end += 1;
            }
        };
        for (k, other) in lines.iter_mut().enumerate() {
            if k == li {
                other.range.end = at + 1;
            } else {
                shift(&mut other.range);
            }
        }
        let l = &mut lines[li];
        l.width = line_width(&us, &l.range);
        out.overflowed = true;
    }

    // Vertical placement.
    let (col, row) = p.align.cell();
    let block_h = block_height(&lines, p.line_spacing);
    let top = (p.size.y - block_h) * row as f32 * 0.5;
    out.lines = lines.len();
    out.block = Vec2::new(lines.iter().map(|l| l.width).fold(0.0, f32::max), block_h);
    let mut baseline = top;
    for (li, l) in lines.iter().enumerate() {
        baseline += if li == 0 {
            l.ascent
        } else {
            l.line_h * p.line_spacing
        };
        let first_style = us
            .get(l.range.start)
            .and_then(|u| rich.chars.get(u.ci))
            .map(|c| c.style);
        let hf = match first_style.and_then(|s| s.align) {
            Some(HAlign::Left) => 0.0,
            Some(HAlign::Center) => 0.5,
            Some(HAlign::Right) => 1.0,
            None => col as f32 * 0.5,
        };
        let mut x = (p.size.x - l.width) * hf;
        let order = visual_order(&us, l.range.clone());
        let (line_top, line_bottom) = (baseline - l.ascent, baseline - l.descent);
        let mut prev: Option<usize> = None;
        for &ui in &order {
            let u = &us[ui];
            if u.newline {
                continue;
            }
            if prev == Some(ui.wrapping_sub(1)) {
                x += u.kern;
            }
            prev = Some(ui);
            let rc = &rich.chars[u.ci];
            let st = rc.style;
            let mut color = st.color.unwrap_or(p.color);
            if let Some(a) = st.alpha {
                color[3] *= a;
            }
            let x0 = x;
            let x1 = x + u.advance;
            let y = baseline + u.shift;
            if let Some(m) = st.mark {
                out.decos.push(Deco {
                    kind: DecoKind::Mark,
                    min: Vec2::new(x0, line_top),
                    max: Vec2::new(x1, line_bottom),
                    color: m,
                });
            }
            if let Some(s) = rc.sprite {
                let h = u.px;
                out.sprites.push(PlacedSprite {
                    sprite: s,
                    min: Vec2::new(x0, y - h * 0.85),
                    max: Vec2::new(x1 - p.char_spacing * u.px / 100.0, y + h * 0.15),
                    text_color: color,
                });
            }
            let blank = rc.ch.is_whitespace();
            for &(font, glyph, ox, oy) in u.glyphs.iter().filter(|_| !blank) {
                out.glyphs.push(PlacedGlyph {
                    font,
                    glyph,
                    px: u.px,
                    pos: Vec2::new(x + ox, y - oy),
                    color,
                    bold: st.bold,
                    italic: st.italic,
                });
            }
            let thick = (u.px * 0.06).max(1.0);
            if st.underline {
                let uy = baseline + u.px * 0.12;
                out.decos.push(Deco {
                    kind: DecoKind::Underline,
                    min: Vec2::new(x0, uy),
                    max: Vec2::new(x1, uy + thick),
                    color,
                });
            }
            if st.strike {
                let sy = y - u.px * 0.3;
                out.decos.push(Deco {
                    kind: DecoKind::Strike,
                    min: Vec2::new(x0, sy),
                    max: Vec2::new(x1, sy + thick),
                    color,
                });
            }
            if let Some(link) = st.link {
                let id = rich.links.get(link as usize).cloned().unwrap_or_default();
                let r = Rect::new(Vec2::new(x0, line_top), Vec2::new(x1, line_bottom));
                match out.links.last_mut() {
                    Some(last) if last.id == id && (last.rect.max.x - x0).abs() < 0.5 => {
                        last.rect.max.x = x1;
                    }
                    _ => out.links.push(LinkBox { id, rect: r }),
                }
            }
            x = x1;
        }
    }
    merge_decos(&mut out.decos);
    out
}

/// Join touching decorations of the same kind and colour (one underline per word, not per
/// letter, so translucent ones don't double up where letters meet).
fn merge_decos(decos: &mut Vec<Deco>) {
    let mut merged: Vec<Deco> = Vec::with_capacity(decos.len());
    for d in decos.drain(..) {
        if let Some(last) = merged.last_mut()
            && last.kind == d.kind
            && last.color == d.color
            && (last.max.x - d.min.x).abs() < 0.5
            && last.min.y == d.min.y
            && last.max.y == d.max.y
        {
            last.max.x = d.max.x;
            continue;
        }
        merged.push(d);
    }
    *decos = merged;
}
