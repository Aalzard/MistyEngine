//! The editor's icon set: the designer's icons as alpha masks, tinted with the theme's text
//! colour (or the accent colour when active), so black and white source art look the same in
//! dark and light themes.
//!
//! Each icon is decoded once; textures are made per pixel size (points × pixels-per-point)
//! with a proper downscale, so icons stay crisp at any display scale. Textures are cached in
//! the egui context.

use std::collections::HashMap;
use std::sync::OnceLock;

use egui::{Color32, Context, Rect, Response, Sense, TextureHandle, TextureId, Ui, Vec2};
use misty_engine::image::{GrayImage, Luma, imageops};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Hand,
    Move,
    Rotate,
    Scale,
    Grid,
    Filter,
    Folder,
    Camera,
    CharacterController,
    /// The Post-process Volume (the designer's "SkyVolume").
    SkyVolume,
    /// The Directional Light.
    SunLight,
    // Terrain tools (Terrain Inspector tool bar) and palette / field actions.
    TerrainRaise,
    TerrainSetHeight,
    TerrainSmooth,
    TerrainPaint,
    TerrainHoles,
    TerrainGrass,
    TerrainTree,
    TerrainPlace,
    TerrainSettings,
    Brush,
    Layers,
    Add,
    Duplicate,
    Remove,
    Reset,
    Fill,
    Clear,
    Locate,
}

impl Icon {
    pub const ALL: [Icon; 29] = [
        Icon::Hand,
        Icon::Move,
        Icon::Rotate,
        Icon::Scale,
        Icon::Grid,
        Icon::Filter,
        Icon::Folder,
        Icon::Camera,
        Icon::CharacterController,
        Icon::SkyVolume,
        Icon::SunLight,
        Icon::TerrainRaise,
        Icon::TerrainSetHeight,
        Icon::TerrainSmooth,
        Icon::TerrainPaint,
        Icon::TerrainHoles,
        Icon::TerrainGrass,
        Icon::TerrainTree,
        Icon::TerrainPlace,
        Icon::TerrainSettings,
        Icon::Brush,
        Icon::Layers,
        Icon::Add,
        Icon::Duplicate,
        Icon::Remove,
        Icon::Reset,
        Icon::Fill,
        Icon::Clear,
        Icon::Locate,
    ];

    fn png(self) -> &'static [u8] {
        match self {
            Icon::Hand => include_bytes!("../../assets/icons/hand.png"),
            Icon::Move => include_bytes!("../../assets/icons/move.png"),
            Icon::Rotate => include_bytes!("../../assets/icons/rotate.png"),
            Icon::Scale => include_bytes!("../../assets/icons/scale.png"),
            Icon::Grid => include_bytes!("../../assets/icons/grid.png"),
            Icon::Filter => include_bytes!("../../assets/icons/filter.png"),
            Icon::Folder => include_bytes!("../../assets/icons/folder.png"),
            Icon::Camera => include_bytes!("../../assets/icons/camera.png"),
            Icon::CharacterController => {
                include_bytes!("../../assets/icons/character_controller.png")
            }
            Icon::SkyVolume => include_bytes!("../../assets/icons/sky_volume.png"),
            Icon::SunLight => include_bytes!("../../assets/icons/sun_light.png"),
            Icon::TerrainRaise => include_bytes!("../../assets/icons/terrain_raise.png"),
            Icon::TerrainSetHeight => include_bytes!("../../assets/icons/terrain_set_height.png"),
            Icon::TerrainSmooth => include_bytes!("../../assets/icons/terrain_smooth.png"),
            Icon::TerrainPaint => include_bytes!("../../assets/icons/terrain_paint.png"),
            Icon::TerrainHoles => include_bytes!("../../assets/icons/terrain_holes.png"),
            Icon::TerrainGrass => include_bytes!("../../assets/icons/terrain_grass.png"),
            Icon::TerrainTree => include_bytes!("../../assets/icons/terrain_tree.png"),
            Icon::TerrainPlace => include_bytes!("../../assets/icons/terrain_place.png"),
            Icon::TerrainSettings => include_bytes!("../../assets/icons/terrain_settings.png"),
            Icon::Brush => include_bytes!("../../assets/icons/brush.png"),
            Icon::Layers => include_bytes!("../../assets/icons/layers.png"),
            Icon::Add => include_bytes!("../../assets/icons/add.png"),
            Icon::Duplicate => include_bytes!("../../assets/icons/duplicate.png"),
            Icon::Remove => include_bytes!("../../assets/icons/remove.png"),
            Icon::Reset => include_bytes!("../../assets/icons/reset.png"),
            Icon::Fill => include_bytes!("../../assets/icons/fill.png"),
            Icon::Clear => include_bytes!("../../assets/icons/clear.png"),
            Icon::Locate => include_bytes!("../../assets/icons/locate.png"),
        }
    }

    /// Share of the drawing box the art fills: the component icons carry their own margin,
    /// the tool icons are drawn edge to edge, so those are drawn a little smaller.
    fn fill(self) -> f32 {
        match self {
            Icon::Camera | Icon::CharacterController | Icon::SkyVolume | Icon::SunLight => 1.0,
            _ => 0.85,
        }
    }

    /// The decoded mask (alpha only), shared by every size.
    fn mask(self) -> &'static GrayImage {
        static MASKS: [OnceLock<GrayImage>; Icon::ALL.len()] =
            [const { OnceLock::new() }; Icon::ALL.len()];
        MASKS[self as usize].get_or_init(|| {
            let rgba = misty_engine::image::load_from_memory(self.png())
                .expect("embedded icon")
                .into_rgba8();
            GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
                Luma([rgba.get_pixel(x, y)[3]])
            })
        })
    }
}

/// Textures per (icon, pixel size), kept in the context's temporary data.
#[derive(Clone, Default)]
struct Cache(HashMap<(Icon, u32), TextureHandle>);

/// Pixel size of `icon` drawn in a box `points` wide.
fn pixels(ctx: &Context, icon: Icon, points: f32) -> u32 {
    pixels_at(ctx.pixels_per_point(), icon, points)
}

fn pixels_at(pixels_per_point: f32, icon: Icon, points: f32) -> u32 {
    ((points * icon.fill() * pixels_per_point).round() as u32).clamp(4, 256)
}

/// The icon's texture for a box `points` wide (white; tint it when painting).
pub fn texture(ctx: &Context, icon: Icon, points: f32) -> TextureId {
    let px = pixels(ctx, icon, points);
    let id = egui::Id::new("misty_icon_cache");
    let found = ctx.data(|d| {
        d.get_temp::<Cache>(id)
            .and_then(|c| c.0.get(&(icon, px)).map(TextureHandle::id))
    });
    if let Some(t) = found {
        return t;
    }
    let handle = ctx.load_texture(
        format!("icon {icon:?} {px}"),
        rasterize(icon, px),
        egui::TextureOptions::LINEAR,
    );
    let tex = handle.id();
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(id)
            .0
            .insert((icon, px), handle)
    });
    tex
}

/// The mask scaled to `px` square: white, with the mask as coverage.
fn rasterize(icon: Icon, px: u32) -> egui::ColorImage {
    let scaled = imageops::resize(icon.mask(), px, px, imageops::FilterType::Lanczos3);
    let pixels = scaled
        .pixels()
        .map(|p| Color32::from_white_alpha(p[0]))
        .collect();
    egui::ColorImage::new([px as usize, px as usize], pixels)
}

/// Paint `icon` centred in `rect` (in a square box of `rect`'s smaller side).
pub fn paint(ui: &Ui, rect: Rect, icon: Icon, tint: Color32) {
    paint_with(ui.painter(), rect, icon, tint);
}

/// [`paint`] with a given painter (clipped overlays).
pub fn paint_with(painter: &egui::Painter, rect: Rect, icon: Icon, tint: Color32) {
    let ctx = painter.ctx();
    let side = rect.width().min(rect.height());
    // Exactly the texture's pixel size, on whole pixels, so it maps 1:1.
    let ppp = ctx.pixels_per_point();
    let px = pixels(ctx, icon, side) as f32 / ppp;
    let drawn = Rect::from_center_size(rect.center(), Vec2::splat(px));
    let min = (drawn.min.to_vec2() * ppp).round() / ppp;
    let drawn = Rect::from_min_size(min.to_pos2(), drawn.size());
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.image(texture(ctx, icon, side), drawn, uv, tint);
}

/// The icon as an inline widget `points` square.
pub fn image(ui: &mut Ui, icon: Icon, points: f32, tint: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(points), Sense::hover());
    paint(ui, rect, icon, tint);
    resp
}

/// Accent colour for active / selected icons, derived from the theme.
pub fn accent(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals.hyperlink_color.lerp_to_gamma(Color32::WHITE, 0.3)
    } else {
        visuals.hyperlink_color
    }
}

/// A toolbar button showing `icon` (pressed look and accent tint while `selected`).
pub fn button(ui: &mut Ui, icon: Icon, selected: bool, size: Vec2, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    paint_button(ui, rect, &resp, icon, selected);
    resp.on_hover_text(tip)
}

/// [`button`] at a fixed rect (overlays).
pub fn button_at(ui: &mut Ui, rect: Rect, icon: Icon, selected: bool, tip: &str) -> Response {
    let resp = ui.allocate_rect(rect, Sense::click());
    paint_button(ui, rect, &resp, icon, selected);
    resp.on_hover_text(tip)
}

fn paint_button(ui: &Ui, rect: Rect, resp: &Response, icon: Icon, selected: bool) {
    let v = ui.visuals();
    let enabled = ui.is_enabled();
    let hovered = resp.hovered() && enabled;
    let fill = if selected {
        Some(v.widgets.hovered.weak_bg_fill)
    } else if hovered {
        Some(v.widgets.inactive.weak_bg_fill)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    let tint = match (selected, enabled) {
        (true, _) => accent(v),
        (false, true) if hovered => v.strong_text_color(),
        (false, true) => v.text_color(),
        (false, false) => v.weak_text_color(),
    };
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            enabled,
            selected,
            format!("{icon:?}"),
        )
    });
    paint(ui, rect.shrink(rect.height() * 0.12), icon, tint);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_decodes_to_a_mask() {
        for (i, icon) in Icon::ALL.into_iter().enumerate() {
            assert_eq!(icon as usize, i);
            let mask = icon.mask();
            assert!(mask.width() >= 64, "{icon:?}");
            // Coverage somewhere and clear somewhere: a shape, not a filled square.
            assert!(mask.pixels().any(|p| p[0] > 200), "{icon:?}");
            assert!(mask.pixels().any(|p| p[0] == 0), "{icon:?}");
        }
    }

    #[test]
    fn textures_follow_the_pixel_size_and_are_reused() {
        let ctx = Context::default();
        let a = texture(&ctx, Icon::Camera, 16.0);
        assert_eq!(texture(&ctx, Icon::Camera, 16.0), a);
        assert_ne!(
            texture(&ctx, Icon::Camera, 24.0),
            a,
            "one texture per pixel size"
        );
        // HiDPI: twice the pixels for the same points; tool icons fill 85 % of their box.
        assert_eq!(pixels_at(2.0, Icon::Camera, 16.0), 32);
        assert_eq!(pixels_at(1.0, Icon::Move, 20.0), 17);
        let img = rasterize(Icon::Move, 32);
        assert_eq!(img.size, [32, 32]);
        // White with coverage in alpha (premultiplied: r == a).
        assert!(img.pixels.iter().all(|c| c.r() == c.a()));
    }

    #[test]
    fn tints_follow_the_theme() {
        let dark = egui::Visuals::dark();
        let light = egui::Visuals::light();
        // The accent stands out from the text colour in both themes.
        for v in [&dark, &light] {
            assert_ne!(accent(v), v.text_color());
        }
        assert!(accent(&dark).r() > accent(&light).r());
    }
}
