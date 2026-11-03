//! Turn a laid-out UI into textured triangles, batched by texture, ready for the GPU.

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use super::input::{UiInteraction, tint};
use super::text::{AtlasFull, GlyphAtlas, UiFont, layout_text};
use super::tree::{UiLayout, UiNode};
use crate::scene::{Scene, UiButton, UiImage, UiText};

/// One UI vertex. Colour is sRGB + alpha; `mask` = 1 samples glyph coverage instead of colour.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct UiVertex {
    /// Clip space (x right, y up).
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub mask: f32,
}

/// What a batch samples.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum UiTexture {
    /// Plain colour (1×1 white texture).
    White,
    /// The glyph atlas.
    Glyphs,
    /// A project texture ("Assets/…/Panel.png").
    Image(String),
}

/// Consecutive vertices drawn with one texture.
#[derive(Clone, Debug, PartialEq)]
pub struct UiBatch {
    pub texture: UiTexture,
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiDrawList {
    pub vertices: Vec<UiVertex>,
    pub batches: Vec<UiBatch>,
}

/// A rectangle to draw: screen pixels (y down), uv corners, colour, texture.
struct Quad {
    min: Vec2,
    max: Vec2,
    uv: [Vec2; 2],
    color: [f32; 4],
    texture: UiTexture,
    mask: f32,
}

impl UiDrawList {
    fn quad(&mut self, screen: Vec2, q: Quad) {
        let clip = |p: Vec2| [p.x / screen.x * 2.0 - 1.0, 1.0 - p.y / screen.y * 2.0];
        let v = |p: Vec2, t: Vec2| UiVertex {
            pos: clip(p),
            uv: t.to_array(),
            color: q.color,
            mask: q.mask,
        };
        let (a, b) = (q.min, q.max);
        let (ta, tb) = (q.uv[0], q.uv[1]);
        let tex = q.texture;
        let corners = [
            v(a, ta),
            v(Vec2::new(b.x, a.y), Vec2::new(tb.x, ta.y)),
            v(b, tb),
            v(a, ta),
            v(b, tb),
            v(Vec2::new(a.x, b.y), Vec2::new(ta.x, tb.y)),
        ];
        let start = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&corners);
        match self.batches.last_mut() {
            Some(last) if last.texture == tex => last.count += 6,
            _ => self.batches.push(UiBatch {
                texture: tex,
                start,
                count: 6,
            }),
        }
    }
}

fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| a[i] * b[i])
}

/// Build the draw list for `layout`. Buttons tint their Image by `interaction` state.
pub fn build(
    layout: &UiLayout,
    scene: &Scene,
    interaction: Option<&UiInteraction>,
    font: &UiFont,
    atlas: &mut GlyphAtlas,
) -> UiDrawList {
    match try_build(layout, scene, interaction, font, atlas) {
        Ok(list) => list,
        Err(AtlasFull) => {
            // Start over with an empty atlas; if even that overflows, skip the extra text.
            atlas.clear();
            try_build(layout, scene, interaction, font, atlas).unwrap_or_default()
        }
    }
}

fn try_build(
    layout: &UiLayout,
    scene: &Scene,
    interaction: Option<&UiInteraction>,
    font: &UiFont,
    atlas: &mut GlyphAtlas,
) -> Result<UiDrawList, AtlasFull> {
    let mut list = UiDrawList::default();
    for node in &layout.nodes {
        image(&mut list, layout.screen, node, scene, interaction);
        text(&mut list, layout.screen, node, scene, font, atlas)?;
    }
    Ok(list)
}

fn image(
    list: &mut UiDrawList,
    screen: Vec2,
    n: &UiNode,
    scene: &Scene,
    interaction: Option<&UiInteraction>,
) {
    let Ok(img) = scene.world.get::<&UiImage>(n.entity) else {
        return;
    };
    if !img.enabled {
        return;
    }
    let mut color = img.color;
    if let Ok(b) = scene.world.get::<&UiButton>(n.entity) {
        let state = UiInteraction::state(interaction, scene, n.entity);
        color = mul(color, tint(&b, state));
    }
    let tex = img
        .texture
        .clone()
        .map_or(UiTexture::White, UiTexture::Image);
    let q = Quad {
        min: n.screen.min,
        max: n.screen.max,
        uv: [Vec2::ZERO, Vec2::ONE],
        color,
        texture: tex,
        mask: 0.0,
    };
    list.quad(screen, q);
}

fn text(
    list: &mut UiDrawList,
    screen: Vec2,
    n: &UiNode,
    scene: &Scene,
    font: &UiFont,
    atlas: &mut GlyphAtlas,
) -> Result<(), AtlasFull> {
    let Ok(t) = scene.world.get::<&UiText>(n.entity) else {
        return Ok(());
    };
    if !t.enabled || t.text.is_empty() {
        return Ok(());
    }
    // Rasterise at the on-screen pixel size so text stays crisp at every scale.
    let px = (t.font_size * n.scale).round().clamp(1.0, 512.0) as u32;
    let size = n.screen.size();
    let placed = layout_text(
        font,
        &t.text,
        px as f32,
        t.line_spacing,
        size,
        t.wrap,
        t.alignment,
    );
    for p in placed {
        let Some(g) = atlas.glyph(font, p.ch, px)? else {
            continue;
        };
        let min = (n.screen.min + p.pos).round() + g.offset;
        let q = Quad {
            min,
            max: min + g.size,
            uv: [g.uv_min, g.uv_max],
            color: t.color,
            texture: UiTexture::Glyphs,
            mask: 1.0,
        };
        list.quad(screen, q);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Canvas, CanvasScaler, RectTransform, UiScaleMode};
    use crate::ui::tree::layout as lay_out;

    #[test]
    fn image_quads_and_text_batches() {
        let mut s = Scene::new("UI");
        let canvas = s.spawn("Canvas", None);
        s.add(canvas, Canvas::default());
        s.add(
            canvas,
            CanvasScaler {
                mode: UiScaleMode::ConstantPixelSize,
                ..Default::default()
            },
        );
        let img = s.spawn("Image", Some(canvas));
        s.add(img, RectTransform::stretched());
        s.add(
            img,
            UiImage {
                color: [1.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        let label = s.spawn("Text", Some(canvas));
        s.add(label, RectTransform::stretched());
        s.add(
            label,
            UiText {
                text: "Hi Hi".into(),
                ..Default::default()
            },
        );
        let l = lay_out(&s, Vec2::new(200.0, 100.0));
        let mut atlas = GlyphAtlas::new(256);
        let list = build(&l, &s, None, &UiFont::default(), &mut atlas);
        assert_eq!(list.batches.len(), 2);
        assert_eq!(list.batches[0].texture, UiTexture::White);
        assert_eq!(list.batches[0].count, 6);
        // Full-screen quad spans clip space.
        assert_eq!(list.vertices[0].pos, [-1.0, 1.0]);
        assert_eq!(list.vertices[2].pos, [1.0, -1.0]);
        assert_eq!(list.vertices[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(list.batches[1].texture, UiTexture::Glyphs);
        assert_eq!(list.batches[1].count, 4 * 6, "four visible letters");
        assert!(list.vertices[6..].iter().all(|v| v.mask == 1.0));
    }

    #[test]
    fn button_tints_its_image() {
        let mut s = Scene::new("UI");
        let canvas = s.spawn("Canvas", None);
        s.add(canvas, Canvas::default());
        let b = s.spawn("Button", Some(canvas));
        s.add(b, RectTransform::default());
        s.add(b, UiImage::default());
        s.add(
            b,
            UiButton {
                interactable: false,
                ..Default::default()
            },
        );
        let l = lay_out(&s, Vec2::new(100.0, 100.0));
        let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
        let c = list.vertices[0].color;
        assert!(c[0] < 0.8 && c[3] == 0.5, "disabled colour: {c:?}");
    }
}
