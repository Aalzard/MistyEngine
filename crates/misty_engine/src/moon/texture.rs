//! The moon's image: the built-in one ([`super::texture_gen`]) or a project image set in
//! Lighting ▸ Environment ▸ Moon ▸ Texture.
//!
//! **Your own texture** (for example one made with an image model): any PNG / JPG in the
//! project.
//! - 2:1 (e.g. 2048 × 1024): an equirectangular map of the whole moon, selenographic longitude
//!   −180°..180° left to right, the near side (longitude 0) in the middle, north up. NASA's
//!   LRO colour maps (CGI Moon Kit) are in this layout.
//! - Square: a photo of the near side, the disc touching all four edges, north up.
//!
//! It is the albedo only: draw the full, evenly lit moon, without a shadow or a phase. The sky
//! lights it from the sun so one image shows every phase. The built-in image also carries a
//! height map in its alpha (craters cast shading near the terminator); project images are
//! lit as a smooth sphere.

use std::sync::{Arc, OnceLock};

use image::RgbaImage;

/// The built-in texture (luminance = albedo, alpha = height).
pub const BUILT_IN_PNG: &[u8] = include_bytes!("assets/moon.png");

/// How the image maps onto the moon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    /// 2:1, whole sphere, near side in the middle.
    Equirectangular,
    /// Square photo of the near side.
    Disc,
}

/// A decoded moon image with its mip chain (sRGB colour, linear alpha).
#[derive(Clone, Debug)]
pub struct MoonImage {
    pub mips: Arc<Vec<RgbaImage>>,
    pub projection: Projection,
    /// The alpha channel is a height map (the built-in image).
    pub has_height: bool,
}

impl MoonImage {
    pub fn width(&self) -> u32 {
        self.mips[0].width()
    }

    /// Texels across the moon's diameter at the full size.
    pub fn texels_across(&self) -> f32 {
        match self.projection {
            Projection::Equirectangular => self.width() as f32 * 0.5,
            Projection::Disc => self.width() as f32,
        }
    }

    pub fn bytes(&self) -> u64 {
        self.mips.iter().map(|m| m.as_raw().len() as u64).sum()
    }
}

fn mips(img: RgbaImage) -> Arc<Vec<RgbaImage>> {
    Arc::new(crate::render::texture::mip_chain(img, true))
}

/// The built-in moon (decoded once per process).
pub fn built_in() -> MoonImage {
    static IMAGE: OnceLock<MoonImage> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            let la = image::load_from_memory(BUILT_IN_PNG)
                .expect("built-in moon texture")
                .into_luma_alpha8();
            let rgba = RgbaImage::from_fn(la.width(), la.height(), |x, y| {
                let p = la.get_pixel(x, y);
                image::Rgba([p[0], p[0], p[0], p[1]])
            });
            MoonImage {
                mips: mips(rgba),
                projection: Projection::Equirectangular,
                has_height: true,
            }
        })
        .clone()
}

/// A project image (`rel` = "Assets/…"): square = near-side photo, otherwise equirectangular.
pub fn load(rel: &str) -> Result<MoonImage, String> {
    let img = crate::render::texture::load_image(&crate::assets::absolute(rel))?;
    let projection = if img.width() == img.height() {
        Projection::Disc
    } else {
        Projection::Equirectangular
    };
    Ok(MoonImage {
        mips: mips(img),
        projection,
        has_height: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_is_equirectangular_with_height() {
        let m = built_in();
        assert_eq!(m.projection, Projection::Equirectangular);
        assert_eq!((m.width(), m.mips[0].height()), (2048, 1024));
        assert!(m.has_height);
        assert_eq!(m.texels_across(), 1024.0);
        assert_eq!(m.mips.last().unwrap().dimensions(), (1, 1));
    }

    #[test]
    fn project_images_pick_their_projection_by_shape() {
        let dir = std::env::temp_dir().join(format!("misty_moon_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let square = dir.join("disc.png");
        RgbaImage::from_pixel(64, 64, image::Rgba([200, 200, 200, 255]))
            .save(&square)
            .unwrap();
        let wide = dir.join("map.png");
        RgbaImage::from_pixel(128, 64, image::Rgba([90, 90, 90, 255]))
            .save(&wide)
            .unwrap();
        let disc = load(square.to_str().unwrap()).unwrap();
        assert_eq!(disc.projection, Projection::Disc);
        assert!(!disc.has_height);
        assert_eq!(disc.texels_across(), 64.0);
        let map = load(wide.to_str().unwrap()).unwrap();
        assert_eq!(map.projection, Projection::Equirectangular);
        assert!(load("Assets/does/not/exist.png").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
