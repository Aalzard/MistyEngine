//! Misty Engine branding: the official logo (window icon, About window, splash, toolbar mark)
//! and the editor's icon set ([`icons`]).
//!
//! The source art lives in `assets/branding/` and `assets/icons/` (downscaled copies of the
//! designer's files) and is embedded into the binary.
//!
//! **For other windows (e.g. the Misty Hub):** call [`logo_texture`] with the `egui::Context`
//! to get the logo as a cached texture, and draw it with `egui::Image::new(&texture)` or
//! `ui.image(&texture)` at any size; [`small_logo_texture`] is the version drawn below ~48 px.
//! [`window_icon`] is the eframe viewport icon.

pub mod about;
pub mod icons;
pub mod splash;

use egui::{Context, TextureHandle, TextureOptions};
use misty_engine::image;

/// The official logo (faceted flame with its drop shadow), 512×512.
pub const LOGO_PNG: &[u8] = include_bytes!("../../assets/branding/logo.png");
/// The logo cropped tighter for icons (more of the square is flame), 256×256.
pub const LOGO_ICON_PNG: &[u8] = include_bytes!("../../assets/branding/logo_icon.png");
/// The designer's small logo, cropped square; reads better at 32 px and below.
pub const LOGO_SMALL_PNG: &[u8] = include_bytes!("../../assets/branding/logo_small.png");
/// Multi-size Windows icon made from the logos (see `tests::ico_matches_the_logos`).
#[cfg(test)]
const ICO: &[u8] = include_bytes!("../../assets/branding/misty.ico");

/// Product name shown in the title bar, About window and exe properties.
pub const PRODUCT: &str = "Misty Engine";

/// The editor's version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The logo as a texture, decoded and uploaded once per context.
pub fn logo_texture(ctx: &Context) -> TextureHandle {
    cached(ctx, "misty_logo", LOGO_PNG)
}

/// The small logo (for marks of ~16–48 px, such as the main toolbar).
pub fn small_logo_texture(ctx: &Context) -> TextureHandle {
    cached(ctx, "misty_logo_small", LOGO_SMALL_PNG)
}

/// The window / taskbar icon (eframe's `ViewportBuilder::with_icon`).
pub fn window_icon() -> egui::IconData {
    let img = image::load_from_memory(LOGO_ICON_PNG)
        .expect("embedded logo")
        .resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
        .into_rgba8();
    egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    }
}

fn cached(ctx: &Context, name: &'static str, png: &[u8]) -> TextureHandle {
    let id = egui::Id::new(name);
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return t;
    }
    let img = image::load_from_memory(png)
        .expect("embedded logo")
        .into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let t = ctx.load_texture(name, color, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// The exe icon: the small logo for 16–32 px, the icon crop above (PNG-compressed at 256).
#[cfg(test)]
fn ico_bytes() -> Vec<u8> {
    use crate::build_pipeline::icon;
    let mut images = icon::images_sized(LOGO_SMALL_PNG, &[16, 24, 32]).unwrap();
    images.extend(icon::images_sized(LOGO_ICON_PNG, &[48, 64, 128, 256]).unwrap());
    icon::ico_file(&images)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regenerate `assets/branding/misty.ico` after changing a logo:
    /// `cargo test -p misty_editor -- --ignored write_misty_ico`.
    #[test]
    #[ignore]
    fn write_misty_ico() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/branding/misty.ico");
        std::fs::write(path, ico_bytes()).unwrap();
    }

    #[test]
    fn ico_matches_the_logos() {
        assert!(ICO == ico_bytes().as_slice(), "run write_misty_ico");
        // Seven sizes: 16, 24, 32, 48, 64, 128 and 256 (stored as 0).
        assert_eq!(&ICO[..6], &[0, 0, 1, 0, 7, 0]);
        let sides: Vec<u8> = (0..7).map(|i| ICO[6 + 16 * i]).collect();
        assert_eq!(sides, [16, 24, 32, 48, 64, 128, 0]);
    }

    #[test]
    fn logos_decode_and_textures_are_cached() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (64, 64));
        // The flame is opaque around the middle, the corners are clear.
        let alpha = |x: usize, y: usize| icon.rgba[(y * 64 + x) * 4 + 3];
        assert!((24..40).any(|y| (24..40).any(|x| alpha(x, y) > 240)));
        assert_eq!(icon.rgba[3], 0);
        let ctx = Context::default();
        let a = logo_texture(&ctx);
        assert_eq!(a.size(), [512, 512]);
        assert_eq!(logo_texture(&ctx).id(), a.id());
        assert_ne!(small_logo_texture(&ctx).id(), a.id());
    }
}
