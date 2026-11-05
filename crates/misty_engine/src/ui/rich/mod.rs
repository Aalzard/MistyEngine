//! Rich text for UI Text: markup ([`parse`]), fonts with fallback and shaping ([`fonts`]),
//! layout with wrapping / overflow / auto-size ([`layout`]) and signed distance field glyphs
//! ([`sdf`]) that draw crisply at any size with outlines and shadows.
//!
//! A text only takes this path when it uses something the plain path can't draw (markup,
//! effects, a font asset, spacing, an overflow mode, auto-size or right-to-left script), so
//! existing texts render exactly as before.

pub mod fonts;
pub mod layout;
pub mod parse;
pub mod sdf;

#[cfg(test)]
mod tests;

use glam::Vec2;
use hecs::Entity;

use crate::scene::{Scene, TextOverflow, UiText};
pub use layout::{LayoutParams, RichLayout};
pub use parse::{RichText, parse};

/// Does `t` (showing `shown`) need the rich text path?
pub fn needs_rich(t: &UiText, shown: &str) -> bool {
    (t.rich_text && parse::has_markup(shown))
        || t.font.is_some()
        || t.effects.any()
        || t.auto_size
        || t.overflow != TextOverflow::Overflow
        || t.character_spacing != 0.0
        || t.word_spacing != 0.0
        || shown.chars().any(layout::is_rtl_char)
}

/// The fonts of a text (its font asset when it loads, then the built-in and system fonts).
pub fn font_set(t: &UiText) -> fonts::FontSet {
    let primary = t.font.as_deref().and_then(|f| fonts::load(f).ok());
    fonts::FontSet::new(primary)
}

/// The sprite reference (`texture#name`) an inline `<sprite>` tag names.
pub fn sprite_reference(asset: Option<&str>, tag: &parse::SpriteTag) -> Option<String> {
    let tex = asset?;
    let name = match &tag.key {
        parse::SpriteKey::Name(n) => n.clone(),
        parse::SpriteKey::Index(i) => crate::sprites::importer(tex)
            .sprites
            .get(*i)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| i.to_string()),
    };
    Some(format!("{tex}#{name}"))
}

/// Parse (when Rich Text is on) and lay out `shown` for a rect of `size` screen pixels at
/// `scale` screen pixels per canvas unit.
pub fn layout_ui_text(
    t: &UiText,
    shown: &str,
    scale: f32,
    size: Vec2,
    fonts: &fonts::FontSet,
) -> (RichText, RichLayout) {
    let rich = if t.rich_text {
        parse(shown)
    } else {
        RichText {
            chars: shown
                .chars()
                .map(|ch| parse::RichChar {
                    ch,
                    style: Default::default(),
                    sprite: None,
                })
                .collect(),
            ..Default::default()
        }
    };
    let sprite_aspects = rich
        .sprites
        .iter()
        .map(|tag| {
            sprite_reference(t.sprite_asset.as_deref(), tag)
                .and_then(|r| crate::sprites::resolve(&r))
                .map(|s| s.rect[2] / s.rect[3].max(1.0))
                .unwrap_or(1.0)
        })
        .collect();
    let p = LayoutParams {
        px: (t.font_size * scale).max(0.5),
        scale,
        size,
        align: t.alignment,
        wrap: t.wrap,
        line_spacing: t.line_spacing,
        char_spacing: t.character_spacing,
        word_spacing: t.word_spacing,
        overflow: t.overflow,
        page: t.page,
        auto_size: t.auto_size.then_some((t.font_size_min, t.font_size_max)),
        color: t.color,
        sprite_aspects,
    };
    let l = layout::layout(&rich, fonts, &p);
    (rich, l)
}

/// The link id under screen point `p` on text object `e` laid out in `ui` (None when the text
/// has no link there).
pub fn link_at(scene: &Scene, ui: &crate::ui::UiLayout, e: Entity, p: Vec2) -> Option<String> {
    let node = ui.node(e)?;
    let t = scene.world.get::<&UiText>(e).ok()?;
    if !t.enabled || !t.rich_text || !t.text.contains("<link") {
        return None;
    }
    let fonts = font_set(&t);
    let (_, l) = layout_ui_text(&t, &t.text, node.scale, node.screen.size(), &fonts);
    l.link_at(p - node.screen.min).map(str::to_owned)
}
