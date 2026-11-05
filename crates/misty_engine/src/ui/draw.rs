//! Turn a laid-out UI into textured triangles, batched by texture, ready for the GPU. Quads are
//! clipped on the CPU to their Mask / Rect Mask 2D rect (UI quads are axis-aligned, so this is
//! exact and needs no stencil).

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use hecs::Entity;

use super::driven::{Visuals, display_text, visuals};
use super::events::{UiInteraction, text_shift};
use super::layout::Rect;
use super::rich::sdf::{self, SdfAtlas, SdfFull};
use super::text::{AtlasFull, GlyphAtlas, UiFont, caret_stops, layout_text};
use super::tree::{UiLayout, UiNode};
use crate::scene::{Scene, UiImage, UiInputField, UiRawImage, UiText};

/// One UI vertex. Colour is sRGB + alpha; `mask` = 1 samples glyph coverage instead of colour,
/// 2 a glyph distance field drawn with `sdf` = (edge threshold, extra softness).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct UiVertex {
    /// Clip space (x right, y up).
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub mask: f32,
    pub sdf: [f32; 2],
}

/// What a batch samples.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum UiTexture {
    /// Plain colour (1×1 white texture).
    White,
    /// The glyph atlas.
    Glyphs,
    /// The distance field glyph atlas (rich text).
    SdfGlyphs,
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
    /// Distance field threshold and softness (mask 2).
    sdf: [f32; 2],
    /// Horizontal shift of the top and bottom edges (italic).
    skew: [f32; 2],
    /// Per-corner colours (top-left, top-right, bottom-left, bottom-right) instead of `color`.
    corners: Option<[[f32; 4]; 4]>,
}

impl Quad {
    fn new(min: Vec2, max: Vec2, uv: [Vec2; 2], color: [f32; 4], texture: UiTexture) -> Self {
        Self {
            min,
            max,
            uv,
            color,
            texture,
            mask: 0.0,
            sdf: [0.5, 0.0],
            skew: [0.0; 2],
            corners: None,
        }
    }
}

/// Cut a quad to `clip`, shrinking its uvs to match; `None` when nothing is left.
fn clip_quad(mut q: Quad, clip: Option<Rect>) -> Option<Quad> {
    let Some(c) = clip else { return Some(q) };
    let r = Rect::new(q.min, q.max).intersect(&c);
    if r.is_empty() {
        return None;
    }
    let size = (q.max - q.min).max(Vec2::splat(1e-6));
    let (u0, u1) = (q.uv[0], q.uv[1]);
    let uv = |p: Vec2| u0 + (u1 - u0) * ((p - q.min) / size);
    q.uv = [uv(r.min), uv(r.max)];
    q.min = r.min;
    q.max = r.max;
    Some(q)
}

impl UiDrawList {
    fn quad(&mut self, screen: Vec2, q: Quad, clip: Option<Rect>) {
        let Some(q) = clip_quad(q, clip) else { return };
        let clip = |p: Vec2| [p.x / screen.x * 2.0 - 1.0, 1.0 - p.y / screen.y * 2.0];
        let cc = q.corners.unwrap_or([q.color; 4]);
        let v = |p: Vec2, t: Vec2, c: [f32; 4], dx: f32| UiVertex {
            pos: clip(p + Vec2::new(dx, 0.0)),
            uv: t.to_array(),
            color: c,
            mask: q.mask,
            sdf: q.sdf,
        };
        let (a, b) = (q.min, q.max);
        let (ta, tb) = (q.uv[0], q.uv[1]);
        let (top, bottom) = (q.skew[0], q.skew[1]);
        let tex = q.texture;
        let corners = [
            v(a, ta, cc[0], top),
            v(Vec2::new(b.x, a.y), Vec2::new(tb.x, ta.y), cc[1], top),
            v(b, tb, cc[3], bottom),
            v(a, ta, cc[0], top),
            v(b, tb, cc[3], bottom),
            v(Vec2::new(a.x, b.y), Vec2::new(ta.x, tb.y), cc[2], bottom),
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

    /// A textured rectangle (screen pixels, y down) with the texture's full area.
    pub fn image(
        &mut self,
        screen: Vec2,
        min: Vec2,
        max: Vec2,
        texture: UiTexture,
        color: [f32; 4],
    ) {
        let q = Quad::new(min, max, [Vec2::ZERO, Vec2::ONE], color, texture);
        self.quad(screen, q, None);
    }

    fn solid(&mut self, screen: Vec2, min: Vec2, max: Vec2, color: [f32; 4], clip: Option<Rect>) {
        let q = Quad::new(min, max, [Vec2::ZERO, Vec2::ONE], color, UiTexture::White);
        self.quad(screen, q, clip);
    }
}

fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| a[i] * b[i])
}

/// Per-frame drawing context.
struct Ctx<'a> {
    scene: &'a Scene,
    screen: Vec2,
    interaction: Option<&'a UiInteraction>,
    tints: HashMap<Entity, [f32; 4]>,
    visuals: Visuals,
    /// Text component → (its Input Field, the field's settings), for caret drawing.
    fields: HashMap<Entity, (Entity, UiInputField)>,
}

/// One of the glyph atlases ran out of room.
#[derive(Debug)]
enum Full {
    Glyphs,
    Sdf,
}

impl From<AtlasFull> for Full {
    fn from(_: AtlasFull) -> Self {
        Full::Glyphs
    }
}

impl From<SdfFull> for Full {
    fn from(_: SdfFull) -> Self {
        Full::Sdf
    }
}

/// Build the draw list for `layout`. Selectables tint their target graphic by `interaction`.
/// Rich text glyphs go into a temporary distance field atlas (see [`build_with`]).
pub fn build(
    layout: &UiLayout,
    scene: &Scene,
    interaction: Option<&UiInteraction>,
    font: &UiFont,
    atlas: &mut GlyphAtlas,
) -> UiDrawList {
    build_with(
        layout,
        scene,
        interaction,
        font,
        atlas,
        &mut SdfAtlas::new(512),
    )
}

/// [`build`] with the distance field atlas rich text uses.
pub fn build_with(
    layout: &UiLayout,
    scene: &Scene,
    interaction: Option<&UiInteraction>,
    font: &UiFont,
    atlas: &mut GlyphAtlas,
    sdf_atlas: &mut SdfAtlas,
) -> UiDrawList {
    let fields = scene
        .world
        .query::<(Entity, &UiInputField)>()
        .iter()
        .filter_map(|(e, f)| Some((f.text_component?, (e, f.clone()))))
        .collect();
    let ctx = Ctx {
        scene,
        screen: layout.screen,
        interaction,
        tints: UiInteraction::tints(interaction, scene, layout),
        visuals: visuals(scene),
        fields,
    };
    match ctx.try_build(layout, font, atlas, sdf_atlas) {
        Ok(list) => list,
        Err(full) => {
            // Start over with an empty atlas; if even that overflows, skip the extra text.
            match full {
                Full::Glyphs => atlas.clear(),
                Full::Sdf => sdf_atlas.clear(),
            }
            ctx.try_build(layout, font, atlas, sdf_atlas)
                .unwrap_or_default()
        }
    }
}

impl Ctx<'_> {
    fn try_build(
        &self,
        layout: &UiLayout,
        font: &UiFont,
        atlas: &mut GlyphAtlas,
        sdf_atlas: &mut SdfAtlas,
    ) -> Result<UiDrawList, Full> {
        let mut list = UiDrawList::default();
        for node in &layout.nodes {
            if self.visuals.hidden.contains(&node.entity) {
                continue;
            }
            self.image(&mut list, node);
            self.raw_image(&mut list, node);
            self.text(&mut list, node, font, atlas, sdf_atlas)?;
        }
        Ok(list)
    }

    fn tint(&self, e: Entity, color: [f32; 4]) -> [f32; 4] {
        self.tints.get(&e).map_or(color, |t| mul(color, *t))
    }

    fn image(&self, list: &mut UiDrawList, n: &UiNode) {
        let Ok(img) = self.scene.world.get::<&UiImage>(n.entity) else {
            return;
        };
        if !img.enabled {
            return;
        }
        let sprite = img.sprite.as_deref().and_then(crate::sprites::resolve);
        let (tex, uv) = match sprite {
            // The sprite's rect of its texture (uv v runs up, screen y down).
            Some(s) => {
                let (lo, hi) = s.uv_rect();
                let uv = [Vec2::new(lo.x, 1.0 - hi.y), Vec2::new(hi.x, 1.0 - lo.y)];
                (UiTexture::Image(s.texture), uv)
            }
            None => (
                img.texture
                    .clone()
                    .map_or(UiTexture::White, UiTexture::Image),
                [Vec2::ZERO, Vec2::ONE],
            ),
        };
        let color = self.tint(n.entity, img.color);
        let q = Quad::new(n.screen.min, n.screen.max, uv, color, tex);
        list.quad(self.screen, q, n.clip);
    }

    fn raw_image(&self, list: &mut UiDrawList, n: &UiNode) {
        let Ok(img) = self.scene.world.get::<&UiRawImage>(n.entity) else {
            return;
        };
        if !img.enabled {
            return;
        }
        let tex = img
            .texture
            .clone()
            .map_or(UiTexture::White, UiTexture::Image);
        // UV y runs up like the uv rect; screen y runs down.
        let (lo, hi) = (img.uv_min(), img.uv_max());
        let uv = [Vec2::new(lo.x, 1.0 - hi.y), Vec2::new(hi.x, 1.0 - lo.y)];
        let color = self.tint(n.entity, img.color);
        let q = Quad::new(n.screen.min, n.screen.max, uv, color, tex);
        list.quad(self.screen, q, n.clip);
    }

    fn text(
        &self,
        list: &mut UiDrawList,
        n: &UiNode,
        font: &UiFont,
        atlas: &mut GlyphAtlas,
        sdf_atlas: &mut SdfAtlas,
    ) -> Result<(), Full> {
        let Ok(t) = self.scene.world.get::<&UiText>(n.entity) else {
            return Ok(());
        };
        if !t.enabled {
            return Ok(());
        }
        let shown = self.visuals.text.get(&n.entity).unwrap_or(&t.text);
        if !self.fields.contains_key(&n.entity) && super::rich::needs_rich(&t, shown) {
            return self.rich_text(list, n, &t, shown, sdf_atlas);
        }
        // Rasterise at the on-screen pixel size so text stays crisp at every scale.
        let px = (t.font_size * n.scale).round().clamp(1.0, 512.0) as u32;
        let size = n.screen.size();
        let color = self.tint(n.entity, t.color);
        // Input Field text: one line, clipped to its rect, scrolled to keep the caret in view.
        let field = self.fields.get(&n.entity);
        let mut clip = n.clip;
        let mut shift = 0.0;
        if let Some((fe, f)) = field {
            clip = Some(clip.map_or(n.screen, |c| c.intersect(&n.screen)));
            let shown_f = display_text(f, &f.text);
            let (stops, top, bottom) = caret_stops(font, &shown_f, px as f32, size, t.alignment);
            if let Some(ed) = self
                .interaction
                .and_then(|i| i.editing.as_ref())
                .filter(|ed| ed.field == *fe)
            {
                shift = text_shift(&stops, size.x, ed.caret);
                self.caret(list, n, f, &stops, (top, bottom), shift, clip);
            }
        }
        if shown.is_empty() {
            return Ok(());
        }
        let wrap = t.wrap && field.is_none();
        let placed = layout_text(
            font,
            shown,
            px as f32,
            t.line_spacing,
            size,
            wrap,
            t.alignment,
        );
        for p in placed {
            let Some(g) = atlas.glyph(font, p.ch, px)? else {
                continue;
            };
            let min = (n.screen.min + p.pos - Vec2::new(shift, 0.0)).round() + g.offset;
            let mut q = Quad::new(
                min,
                min + g.size,
                [g.uv_min, g.uv_max],
                color,
                UiTexture::Glyphs,
            );
            q.mask = 1.0;
            list.quad(self.screen, q, clip);
        }
        Ok(())
    }

    /// A text through the rich text path: marks, shadow, outline, face (distance field glyphs,
    /// optional gradient), inline sprites, then underlines and strikethroughs.
    fn rich_text(
        &self,
        list: &mut UiDrawList,
        n: &UiNode,
        t: &UiText,
        shown: &str,
        sdf_atlas: &mut SdfAtlas,
    ) -> Result<(), Full> {
        use super::rich::layout::DecoKind;
        let fonts = super::rich::font_set(t);
        let (rich, lay) = super::rich::layout_ui_text(t, shown, n.scale, n.screen.size(), &fonts);
        let o = n.screen.min;
        let clip = n.clip;
        let tint = self.tints.get(&n.entity).copied().unwrap_or([1.0; 4]);
        let fx = &t.effects;
        for d in lay.decos.iter().filter(|d| d.kind == DecoKind::Mark) {
            list.solid(self.screen, o + d.min, o + d.max, mul(d.color, tint), clip);
        }
        // Glyph geometry once: (atlas entry, quad min, max, italic skew, bold growth).
        let mut quads = Vec::with_capacity(lay.glyphs.len());
        for g in &lay.glyphs {
            let f = super::rich::fonts::font(g.font);
            let Some(e) = sdf_atlas.glyph(&f, g.glyph)? else {
                continue;
            };
            let s = g.px / sdf::BASE;
            let min = o + g.pos + e.offset * s;
            let max = min + e.size * s;
            let skew = if g.italic {
                let base = o.y + g.pos.y;
                [(base - min.y) * 0.2, (base - max.y) * 0.2]
            } else {
                [0.0; 2]
            };
            let grow = if g.bold { g.px * 0.04 } else { 0.0 };
            quads.push((e, min, max, skew, grow, g));
        }
        let glyph_quad = |e: &sdf::SdfGlyph, min: Vec2, max: Vec2, color, skew, th, soft| {
            let mut q = Quad::new(min, max, [e.uv_min, e.uv_max], color, UiTexture::SdfGlyphs);
            q.mask = 2.0;
            q.sdf = [th, soft];
            q.skew = skew;
            q
        };
        if fx.shadow {
            let off = Vec2::new(fx.shadow_offset[0], -fx.shadow_offset[1]) * n.scale;
            for (e, min, max, skew, grow, g) in &quads {
                let mut c = mul(fx.shadow_color, tint);
                c[3] *= g.color[3];
                let outline = fx.outline_width * n.scale;
                let th = sdf::threshold(grow + outline, g.px);
                let soft = sdf::softness(fx.shadow_softness.clamp(0.0, 1.0) * g.px * 0.3, g.px);
                let q = glyph_quad(e, *min + off, *max + off, c, *skew, th, soft);
                list.quad(self.screen, q, clip);
            }
        }
        if fx.outline_width > 0.0 {
            for (e, min, max, skew, grow, g) in &quads {
                let mut c = mul(fx.outline_color, tint);
                c[3] *= g.color[3];
                let th = sdf::threshold(grow + fx.outline_width * n.scale, g.px);
                let q = glyph_quad(e, *min, *max, c, *skew, th, 0.0);
                list.quad(self.screen, q, clip);
            }
        }
        for (e, min, max, skew, grow, g) in &quads {
            let c = mul(g.color, tint);
            let mut q = glyph_quad(e, *min, *max, c, *skew, sdf::threshold(*grow, g.px), 0.0);
            if fx.gradient {
                q.corners = Some(fx.gradient_colors.map(|k| mul(k, c)));
            }
            list.quad(self.screen, q, clip);
        }
        for sp in &lay.sprites {
            let Some(tag) = rich.sprites.get(sp.sprite as usize) else {
                continue;
            };
            let sprite = super::rich::sprite_reference(t.sprite_asset.as_deref(), tag)
                .and_then(|r| crate::sprites::resolve(&r));
            let Some(sprite) = sprite else { continue };
            let (lo, hi) = sprite.uv_rect();
            let uv = [Vec2::new(lo.x, 1.0 - hi.y), Vec2::new(hi.x, 1.0 - lo.y)];
            let mut c = match (tag.color, tag.tint) {
                (Some(c), _) => c,
                (None, true) => sp.text_color,
                (None, false) => [1.0, 1.0, 1.0, 1.0],
            };
            c[3] *= sp.text_color[3];
            let tex = UiTexture::Image(sprite.texture.clone());
            let q = Quad::new(o + sp.min, o + sp.max, uv, mul(c, tint), tex);
            list.quad(self.screen, q, clip);
        }
        for d in lay.decos.iter().filter(|d| d.kind != DecoKind::Mark) {
            list.solid(self.screen, o + d.min, o + d.max, mul(d.color, tint), clip);
        }
        Ok(())
    }

    /// Selection highlight and blinking caret of the field being edited.
    #[allow(clippy::too_many_arguments)]
    fn caret(
        &self,
        list: &mut UiDrawList,
        n: &UiNode,
        f: &UiInputField,
        stops: &[f32],
        (top, bottom): (f32, f32),
        shift: f32,
        clip: Option<Rect>,
    ) {
        let Some(i) = self.interaction else { return };
        let Some(ed) = i.editing.as_ref() else { return };
        let x = |c: usize| n.screen.min.x + stops.get(c).copied().unwrap_or(0.0) - shift;
        let (y0, y1) = (n.screen.min.y + top, n.screen.min.y + bottom);
        let (a, b) = ed.selection();
        if a != b {
            let (min, max) = (Vec2::new(x(a), y0), Vec2::new(x(b), y1));
            list.solid(self.screen, min, max, f.selection_color, clip);
        } else if i.caret_visible(f.caret_blink_rate) {
            let w = (f.caret_width * n.scale).max(1.0);
            let cx = x(ed.caret).round();
            let color = self
                .scene
                .world
                .get::<&UiText>(n.entity)
                .map_or([0.2, 0.2, 0.2, 1.0], |t| t.color);
            list.solid(
                self.screen,
                Vec2::new(cx, y0),
                Vec2::new(cx + w, y1),
                [color[0], color[1], color[2], 1.0],
                clip,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Canvas, CanvasScaler, RectTransform, UiButton, UiMask, UiScaleMode};
    use crate::ui::tree::layout as lay_out;

    fn canvas(s: &mut Scene) -> Entity {
        let canvas = s.spawn("Canvas", None);
        s.add(canvas, Canvas::default());
        s.add(
            canvas,
            CanvasScaler {
                mode: UiScaleMode::ConstantPixelSize,
                ..Default::default()
            },
        );
        canvas
    }

    #[test]
    fn image_quads_and_text_batches() {
        let mut s = Scene::new("UI");
        let canvas = canvas(&mut s);
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

    #[test]
    fn masks_clip_children_and_can_hide_their_graphic() {
        let mut s = Scene::new("UI");
        let canvas = canvas(&mut s);
        // 100×100 mask in the middle of a 200×200 screen, child twice as wide, shifted right.
        let mask = s.spawn("Mask", Some(canvas));
        s.add(mask, RectTransform::sized(100.0, 100.0));
        s.add(mask, UiImage::default());
        s.add(mask, UiMask::default());
        let child = s.spawn("Child", Some(mask));
        let mut rt = RectTransform::sized(200.0, 50.0);
        rt.anchored_position.x = 100.0;
        s.add(child, rt);
        s.add(child, UiImage::default());
        let l = lay_out(&s, Vec2::new(200.0, 200.0));
        assert_eq!(
            l.node(child).unwrap().clip,
            Some(l.node(mask).unwrap().screen)
        );
        let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
        assert_eq!(list.vertices.len(), 12);
        // Child quad cut to x 50..150 (clip space -0.5..0.5) with uvs 0.25..0.5 of the way.
        let child_v = &list.vertices[6..];
        assert_eq!(child_v[0].pos[0], -0.0);
        assert_eq!(child_v[1].pos[0], 0.5);
        assert_eq!(child_v[0].uv[0], 0.0);
        assert!((child_v[1].uv[0] - 0.25).abs() < 1e-6);
        // Hidden mask graphic: only the child remains; raycasts are clipped too.
        s.world.get::<&mut UiMask>(mask).unwrap().show_mask_graphic = false;
        let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
        assert_eq!(list.vertices.len(), 6);
        let p = Vec2::new(180.0, 100.0);
        assert_eq!(
            crate::ui::input::raycast(&l, &s, p),
            None,
            "outside the mask"
        );
        assert_eq!(
            crate::ui::input::raycast(&l, &s, Vec2::new(140.0, 100.0)),
            Some(child)
        );
    }
}
