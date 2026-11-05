//! Rich text: parsing (nesting, unknown tags, escapes), layout metrics (wrapping, spacing,
//! ellipsis, truncate, pages, auto-size), link hit testing, right-to-left order, distance field
//! glyph quality.

use glam::Vec2;

use super::fonts::{FontId, FontSource};
use super::layout::{DecoKind, LayoutParams, layout};
use super::parse::{HAlign, OBJECT, Script, SizeSpec, SpriteKey, parse, parse_color};
use crate::scene::{TextAnchor, TextOverflow, UiText};

/// Every glyph `px` wide; ascent 0.8 px, descent -0.2 px, line height 1.2 px.
struct Mono;

impl FontSource for Mono {
    fn lookup(&self, c: char) -> (FontId, u16) {
        (0, c as u32 as u16)
    }
    fn advance(&self, _: FontId, _: u16, px: f32) -> f32 {
        px
    }
    fn kern(&self, _: FontId, _: u16, _: u16, _: f32) -> f32 {
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

fn params(w: f32, h: f32) -> LayoutParams {
    LayoutParams {
        px: 10.0,
        size: Vec2::new(w, h),
        ..Default::default()
    }
}

fn text_of(rich: &super::RichText) -> String {
    rich.plain()
}

#[test]
fn tags_nest_and_close_their_own_kind() {
    let r = parse("a<b>b<i>c</i>d</b>e");
    assert_eq!(text_of(&r), "abcde");
    let st: Vec<(bool, bool)> = r
        .chars
        .iter()
        .map(|c| (c.style.bold, c.style.italic))
        .collect();
    assert_eq!(
        st,
        [
            (false, false),
            (true, false),
            (true, true),
            (true, false),
            (false, false)
        ]
    );
    let r = parse("<color=red>R<color=#00FF0080>G</color>R</color>x");
    assert_eq!(r.chars[0].style.color, Some([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(r.chars[1].style.color.unwrap()[3], 128.0 / 255.0);
    assert_eq!(r.chars[2].style.color, Some([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(r.chars[3].style.color, None);
    // A closing tag with nothing open is consumed; other kinds stay open.
    let r = parse("</b><u>x</i>y");
    assert_eq!(text_of(&r), "xy");
    assert!(r.chars[1].style.underline);
}

#[test]
fn unknown_and_malformed_tags_stay_as_text() {
    for s in [
        "<foo>x</foo>",
        "a < b",
        "<color=nope>c",
        "<size=big>s",
        "1 <2> 3",
        "<b",
        "< b>",
    ] {
        assert_eq!(text_of(&parse(s)), s, "{s}");
    }
    assert_eq!(text_of(&parse("a<<b>b")), "a<b");
}

#[test]
fn escapes_show_markup_literally() {
    assert_eq!(text_of(&parse("<noparse><b>x</b></noparse>")), "<b>x</b>");
    assert!(!parse("<noparse><b>x</noparse>").chars[3].style.bold);
    assert_eq!(text_of(&parse("&lt;b&gt; &amp;")), "<b> &");
    assert!(super::parse::has_markup("<b>x</b>"));
    assert!(!super::parse::has_markup("x < y and 5 > 3"));
    assert!(!super::parse::has_markup("plain"));
}

#[test]
fn values_sizes_sprites_links_and_breaks() {
    assert_eq!(parse_color("#F00"), Some([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(parse_color("#0000FF80").unwrap()[3], 128.0 / 255.0);
    assert_eq!(parse_color("#12345"), None);
    let r = parse("<size=20>a</size><size=50%>b<size=+4>c</size></size>");
    assert_eq!(r.chars[0].style.size, Some(SizeSpec::Abs(20.0)));
    assert_eq!(r.chars[1].style.size, Some(SizeSpec::Rel(0.5)));
    assert_eq!(r.chars[2].style.size, Some(SizeSpec::Add(4.0)));
    assert_eq!(SizeSpec::Rel(0.5).resolve(30.0), 15.0);
    let r = parse("x<sprite name=\"Coin\" tint=1><sprite=2>");
    assert_eq!(r.chars[1].ch, OBJECT);
    assert_eq!(r.sprites[0].key, SpriteKey::Name("Coin".into()));
    assert!(r.sprites[0].tint);
    assert_eq!(r.sprites[1].key, SpriteKey::Index(2));
    let r = parse("<link=\"shop\">Buy</link> now<br>next<sup>2</sup><sub>i</sub>");
    assert_eq!(r.links, ["shop"]);
    assert_eq!(r.chars[0].style.link, Some(0));
    assert_eq!(r.chars[3].style.link, None);
    assert!(text_of(&r).contains('\n'));
    let sup = r.chars.iter().find(|c| c.ch == '2').unwrap();
    assert_eq!(sup.style.script, Script::Super);
    let r = parse("<align=right>R</align><alpha=#80>a<mark=#FFFF00>m</mark>");
    assert_eq!(r.chars[0].style.align, Some(HAlign::Right));
    assert_eq!(r.chars[1].style.alpha, Some(128.0 / 255.0));
    assert_eq!(
        r.chars[2].style.mark.unwrap()[3],
        0.5,
        "mark without alpha is translucent"
    );
}

#[test]
fn wraps_at_spaces_with_spacing() {
    let r = parse("aa bb cc");
    let l = layout(&r, &Mono, &params(55.0, 100.0));
    assert_eq!(l.lines, 2);
    // "aa bb" on the first line, "cc" on the second at x 0.
    let ys: Vec<f32> = l.glyphs.iter().map(|g| g.pos.y).collect();
    assert_eq!(ys, [8.0, 8.0, 8.0, 8.0, 20.0, 20.0]);
    assert_eq!(l.glyphs[4].pos.x, 0.0);
    // Character spacing 10 (0.1 em) widens every advance by 1 px.
    let mut p = params(200.0, 100.0);
    p.char_spacing = 10.0;
    p.word_spacing = 50.0;
    let l = layout(&parse("ab c"), &Mono, &p);
    assert_eq!(l.glyphs[1].pos.x, 11.0);
    // 'c' after "ab " = 11 + 11 + (11 + 5) for the space.
    assert_eq!(l.glyphs[2].pos.x, 38.0);
}

#[test]
fn alignment_per_line_and_sizes() {
    let mut p = params(100.0, 50.0);
    p.align = TextAnchor::UpperLeft;
    let l = layout(&parse("ab\n<align=right>cd</align>"), &Mono, &p);
    assert_eq!(l.glyphs[0].pos.x, 0.0);
    assert_eq!(l.glyphs[2].pos.x, 80.0);
    let l = layout(&parse("a<size=20>b</size>"), &Mono, &params(100.0, 50.0));
    // The bigger glyph raises the line's ascent: both share a baseline at 16.
    assert_eq!(l.glyphs[0].pos.y, 16.0);
    assert_eq!(l.glyphs[1].px, 20.0);
    assert_eq!(l.glyphs[1].pos.x, 10.0);
}

#[test]
fn ellipsis_truncate_and_pages() {
    let text = parse("one two three four five six");
    let mut p = params(50.0, 25.0);
    // Two lines fit (8 + 12 + 2 = 22 px).
    p.overflow = TextOverflow::Truncate;
    let l = layout(&text, &Mono, &p);
    assert_eq!(l.lines, 2);
    assert!(l.overflowed);
    p.overflow = TextOverflow::Ellipsis;
    let l = layout(&text, &Mono, &p);
    assert_eq!(l.lines, 2);
    let last = l.glyphs.last().unwrap();
    assert_eq!(last.glyph, '\u{2026}' as u16, "ends with an ellipsis");
    assert!(last.pos.x + 10.0 <= 50.0);
    p.overflow = TextOverflow::Page;
    p.page = 2;
    let l = layout(&text, &Mono, &p);
    assert_eq!(l.pages, 3);
    assert_eq!(l.glyphs[0].glyph, 't' as u16, "page 2 starts at \"three\"");
    p.page = 99;
    assert_eq!(
        layout(&text, &Mono, &p).glyphs[0].glyph,
        'f' as u16,
        "last page"
    );
    // Unwrapped long line with ellipsis is cut at the rect's width.
    let mut p = params(45.0, 20.0);
    p.wrap = false;
    p.overflow = TextOverflow::Ellipsis;
    let l = layout(&parse("abcdefgh"), &Mono, &p);
    assert_eq!(l.glyphs.len(), 4, "abc + …");
}

#[test]
fn auto_size_picks_the_largest_size_that_fits() {
    let mut p = params(100.0, 20.0);
    p.auto_size = Some((4.0, 40.0));
    p.wrap = false;
    let l = layout(&parse("abcde"), &Mono, &p);
    // Width: 5 glyphs × px ≤ 100 → 20; height px ≤ 20 → 20.
    assert!((l.px - 20.0).abs() < 0.3, "{}", l.px);
    // Wrapped text gets smaller until every line fits.
    let mut p = params(60.0, 30.0);
    p.auto_size = Some((4.0, 40.0));
    let l = layout(&parse("aaa bbb ccc"), &Mono, &p);
    assert!(l.px > 4.0 && l.px < 20.0, "{}", l.px);
    assert!(l.block.y <= 30.5 && l.block.x <= 60.5, "{:?}", l.block);
    // Nothing fits: the minimum.
    let mut p = params(10.0, 5.0);
    p.auto_size = Some((8.0, 40.0));
    assert_eq!(layout(&parse("zzzzzz"), &Mono, &p).px, 8.0);
}

#[test]
fn links_are_hit_tested_per_line() {
    let r = parse("go <link=\"a\">here</link> or <link=b>there now</link>");
    let l = layout(&r, &Mono, &params(80.0, 100.0));
    assert_eq!(l.link_at(Vec2::new(35.0, 5.0)), Some("a"));
    assert_eq!(l.link_at(Vec2::new(5.0, 5.0)), None);
    // "there now" wraps: both lines belong to link b.
    let b: Vec<_> = l.links.iter().filter(|x| x.id == "b").collect();
    assert_eq!(b.len(), 2, "{:?}", l.links);
    let second = b[1].rect.center();
    assert_eq!(l.link_at(second), Some("b"));
}

#[test]
fn decorations_merge_per_word() {
    let l = layout(
        &parse("<u>ab</u> <s>c</s> <mark=#FF000080>d</mark>"),
        &Mono,
        &params(200.0, 50.0),
    );
    let unders: Vec<_> = l
        .decos
        .iter()
        .filter(|d| d.kind == DecoKind::Underline)
        .collect();
    assert_eq!(unders.len(), 1);
    assert_eq!(unders[0].max.x - unders[0].min.x, 20.0);
    assert!(l.decos.iter().any(|d| d.kind == DecoKind::Strike));
    assert!(
        l.decos
            .iter()
            .any(|d| d.kind == DecoKind::Mark && d.color[3] < 0.6)
    );
}

#[test]
fn right_to_left_runs_are_reversed_for_display() {
    // Hebrew alef, bet, gimel in logical order, between Latin letters.
    let r = parse("a \u{5d0}\u{5d1}\u{5d2} b");
    let l = layout(&r, &Mono, &params(200.0, 50.0));
    let order: Vec<u16> = l.glyphs.iter().map(|g| g.glyph).collect();
    let (a, b) = ('a' as u16, 'b' as u16);
    assert_eq!(order[0], a);
    // Spaces draw no glyph: a, gimel, bet, alef, b.
    assert_eq!(&order[1..4], &[0x5d2, 0x5d1, 0x5d0], "{order:x?}");
    assert_eq!(*order.last().unwrap(), b);
    let xs: Vec<f32> = l.glyphs.iter().map(|g| g.pos.x).collect();
    assert!(
        xs.windows(2).all(|w| w[0] < w[1]),
        "drawn left to right: {xs:?}"
    );
}

#[test]
fn arabic_is_shaped_when_a_font_has_it() {
    let set = super::fonts::FontSet::new(None);
    let (font, glyph) = set.lookup('\u{628}');
    if glyph == 0 {
        eprintln!("no Arabic-capable font on this machine; shaping not checked");
        return;
    }
    // Beh alone vs. beh joined in a word: shaping picks different (contextual) glyphs.
    let alone = set.shape(font, "\u{628}", true, 20.0).unwrap();
    let word = set
        .shape(font, "\u{628}\u{628}\u{628}", true, 20.0)
        .unwrap();
    assert_eq!(word.len(), 3);
    assert!(word.iter().any(|g| g.glyph != alone[0].glyph), "{word:?}");
    // Through the layout: one glyph per letter, the logically first letter on the right.
    let r = parse("\u{628}\u{644}\u{62f}");
    let mut p = params(300.0, 60.0);
    p.px = 20.0;
    let l = layout(&r, &set, &p);
    assert_eq!(l.glyphs.len(), 3);
    assert!(l.glyphs[0].pos.x < l.glyphs[2].pos.x);
}

#[test]
fn needs_rich_only_for_rich_features() {
    let t = UiText::default();
    assert!(!super::needs_rich(&t, "Score: 10"));
    assert!(super::needs_rich(&t, "<b>Score</b>"));
    assert!(super::needs_rich(&t, "\u{5e9}\u{5dc}\u{5d5}\u{5dd}"));
    let off = UiText {
        rich_text: false,
        ..Default::default()
    };
    assert!(!super::needs_rich(&off, "<b>Score</b>"));
    let mut fx = UiText::default();
    fx.effects.outline_width = 1.0;
    assert!(super::needs_rich(&fx, "plain"));
}

#[test]
fn distance_field_glyphs_match_the_outline() {
    use super::sdf::{BASE, SdfAtlas, glyph_field};
    let f = super::fonts::font(super::fonts::BUILTIN);
    let g = f.glyph('O').unwrap();
    let (w, h, off, field) = glyph_field(&f, g).unwrap();
    // Compare the field's inside (> 0.5) with the coverage rendered directly at BASE, sampling
    // the field (bilinear) at each coverage pixel's centre.
    let (cw, ch, coff, cov) = f.coverage(g, BASE).unwrap();
    let at = |x: f32, y: f32| -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (tx, ty) = (x - x0, y - y0);
        let v = |xi: f32, yi: f32| {
            if xi < 0.0 || yi < 0.0 || xi >= w as f32 || yi >= h as f32 {
                0.0
            } else {
                field[(yi as u32 * w + xi as u32) as usize] as f32
            }
        };
        let top = v(x0, y0) * (1.0 - tx) + v(x0 + 1.0, y0) * tx;
        let bottom = v(x0, y0 + 1.0) * (1.0 - tx) + v(x0 + 1.0, y0 + 1.0) * tx;
        top * (1.0 - ty) + bottom * ty
    };
    let (mut both, mut either) = (0, 0);
    for y in 0..ch {
        for x in 0..cw {
            let c = cov[(y * cw + x) as usize] >= 0.5;
            // Pixel centres: (x + 0.5 + coff) in pen space = (fx + 0.5 + off) in the field.
            let inside = at(x as f32 + coff[0] - off.x, y as f32 + coff[1] - off.y) > 127.5;
            both += (c && inside) as u32;
            either += (c || inside) as u32;
        }
    }
    let iou = both as f32 / either.max(1) as f32;
    assert!(iou > 0.9, "field matches coverage: IoU {iou}");
    // Padding around the glyph fades to "far outside"; the ring of the O is inside, its hole
    // outside.
    assert!(field[0] < 10, "corner {}", field[0]);
    let mid = field[((h / 2) * w + w / 2) as usize];
    assert!(mid < 128, "hole of the O is outside: {mid}");
    let mut atlas = SdfAtlas::new(256);
    let e = atlas.glyph(&f, g).unwrap().unwrap();
    assert_eq!(e.size, Vec2::new(w as f32, h as f32));
    assert_eq!(atlas.glyph(&f, f.glyph(' ').unwrap_or(3)).unwrap(), None);
    assert!(super::sdf::threshold(2.0, 20.0) < 0.5 && super::sdf::threshold(0.0, 20.0) == 0.5);
}
