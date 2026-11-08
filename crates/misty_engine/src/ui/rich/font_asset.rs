//! Font asset import settings (the `.meta` beside a `.ttf` / `.otf`): how the font's glyph
//! atlas is made (signed distance field or high-quality coverage), the size glyphs are sampled
//! at, the distance field's padding, and fallback fonts tried for characters the font lacks.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// How a font's glyphs are stored in the atlas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontRenderMode {
    /// Signed distance field: sharp at any size, outlines, soft shadows and bold.
    #[default]
    Sdf,
    /// Coverage, supersampled 4×4 at the sampling size: the exact look of the font's hinting-free
    /// rasterisation, best near the sampling size (outlines need SDF).
    Raster,
}

impl FontRenderMode {
    pub const ALL: [Self; 2] = [Self::Sdf, Self::Raster];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sdf => "SDF",
            Self::Raster => "Raster (Smooth)",
        }
    }
}

/// Import settings of a font asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontImporter {
    pub render_mode: FontRenderMode,
    /// Em size, in atlas pixels, glyphs are made at.
    pub sampling_size: f32,
    /// SDF: distance (atlas pixels) from the outline to the ends of the field (wider outlines
    /// and shadows need more).
    pub padding: f32,
    /// Fonts (project paths) tried in order for characters this one lacks, before the built-in
    /// and operating system fonts.
    pub fallback_fonts: Vec<String>,
}

impl Default for FontImporter {
    fn default() -> Self {
        Self {
            render_mode: FontRenderMode::Sdf,
            sampling_size: super::sdf::BASE,
            padding: super::sdf::SPREAD,
            fallback_fonts: Vec::new(),
        }
    }
}

impl FontImporter {
    pub const MIN_SAMPLING: f32 = 12.0;
    pub const MAX_SAMPLING: f32 = 128.0;

    /// Settings of the font at `path` (defaults without a readable `.meta`).
    pub fn load(path: &Path) -> Self {
        let meta = crate::asset_db::meta::meta_path(path);
        let mut s = crate::vfs::read_to_string(meta)
            .ok()
            .and_then(|t| serde_json::from_str::<Self>(&t).ok())
            .unwrap_or_default();
        s.sanitize();
        s
    }

    /// Write the settings into the font's `.meta`, keeping its GUID.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        crate::asset_db::meta::write_settings(path, self)
    }

    /// Keep values usable.
    pub fn sanitize(&mut self) {
        let def = Self::default();
        if !self.sampling_size.is_finite() {
            self.sampling_size = def.sampling_size;
        }
        self.sampling_size = self
            .sampling_size
            .clamp(Self::MIN_SAMPLING, Self::MAX_SAMPLING);
        if !self.padding.is_finite() {
            self.padding = def.padding;
        }
        self.padding = self.padding.clamp(1.0, 32.0);
        self.fallback_fonts.retain(|f| !f.trim().is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_from_the_meta_and_stay_in_range() {
        let dir = std::env::temp_dir().join("misty_font_asset_settings");
        let _ = std::fs::create_dir_all(&dir);
        let font = dir.join("F.ttf");
        std::fs::write(&font, b"not really a font").unwrap();
        let _ = std::fs::remove_file(crate::asset_db::meta::meta_path(&font));
        assert_eq!(FontImporter::load(&font), FontImporter::default());
        std::fs::write(
            crate::asset_db::meta::meta_path(&font),
            r#"{"guid":"x","render_mode":"Raster","sampling_size":4000,"fallback_fonts":["A.ttf",""]}"#,
        )
        .unwrap();
        let s = FontImporter::load(&font);
        assert_eq!(s.render_mode, FontRenderMode::Raster);
        assert_eq!(s.sampling_size, FontImporter::MAX_SAMPLING);
        assert_eq!(s.fallback_fonts, ["A.ttf"]);
    }

    /// Write a font file and its settings into a temp folder; returns its absolute path.
    fn font_file(name: &str, data: &[u8], settings: &str) -> String {
        let dir = std::env::temp_dir().join("misty_font_asset_atlas");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        std::fs::write(&path, data).unwrap();
        std::fs::write(crate::asset_db::meta::meta_path(&path), settings).unwrap();
        let p = path.to_string_lossy().into_owned();
        super::super::fonts::reload(&p);
        p
    }

    #[test]
    fn atlas_holds_distance_fields_or_smooth_coverage_at_the_sampling_size() {
        use super::super::fonts::{self, BUILTIN};
        use super::super::sdf::{self, SdfAtlas};
        let builtin = fonts::font(BUILTIN);
        let o = builtin.glyph('O').unwrap();
        // Distance field: outside far from the outline is 0, the hole in the middle of the O is
        // outside again.
        let (w, h, _, field) = sdf::glyph_field(&builtin, o).unwrap();
        let at = |x: u32, y: u32| field[(y * w + x) as usize];
        assert_eq!(at(0, 0), 0);
        assert!(at(w / 2, h / 2) < 128, "the hole of the O");
        // Inside the (light, thin) stroke the field rises above the 128 outline value.
        assert!(field.iter().any(|v| *v > 136), "inside the stroke");
        // Coverage: a glyph made twice as large is about twice the size.
        let (w1, _, _, c1) = sdf::glyph_coverage(&builtin, o, 30.0).unwrap();
        let (w2, _, _, c2) = sdf::glyph_coverage(&builtin, o, 60.0).unwrap();
        assert!((w2 as f32 / w1 as f32 - 2.0).abs() < 0.25, "{w1} → {w2}");
        for c in [&c1, &c2] {
            assert_eq!(c[0], 0, "empty border");
            assert!(c.contains(&255), "solid inside");
            assert!(c.iter().any(|v| *v > 20 && *v < 235), "smooth edges");
        }
        // A Raster font asset puts its coverage in the atlas, marked raster, at its size.
        let raster = font_file(
            "Raster.ttf",
            epaint_default_fonts::UBUNTU_LIGHT,
            r#"{"render_mode":"Raster","sampling_size":30}"#,
        );
        let f = fonts::font(fonts::load(&raster).unwrap());
        let g = f.glyph('O').unwrap();
        let mut atlas = SdfAtlas::new(256);
        let e = atlas.glyph(&f, g).unwrap().unwrap();
        assert!(e.raster && e.base == 30.0);
        let x0 = (e.uv_min.x * 256.0).round() as usize;
        let y0 = (e.uv_min.y * 256.0).round() as usize;
        let row: Vec<u8> = (0..w1 as usize)
            .map(|x| atlas.pixels[y0 * 256 + x0 + x])
            .collect();
        assert_eq!(row, c1[..w1 as usize], "the coverage, row for row");
        // An SDF font asset with its own size and padding.
        let big = font_file(
            "Big.ttf",
            epaint_default_fonts::UBUNTU_LIGHT,
            r#"{"sampling_size":64,"padding":4}"#,
        );
        let f = fonts::font(fonts::load(&big).unwrap());
        let e = atlas.glyph(&f, g).unwrap().unwrap();
        assert!(!e.raster && e.base == 64.0 && e.spread == 4.0);
        assert!((e.threshold(0.0, 32.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn fallback_fonts_fill_in_missing_characters() {
        use super::super::fonts::{self, FontSource};
        let emoji = font_file("Emoji.ttf", epaint_default_fonts::NOTO_EMOJI_REGULAR, "{}");
        let main = font_file(
            "Mono.ttf",
            epaint_default_fonts::HACK_REGULAR,
            &format!(
                r#"{{"fallback_fonts":[{}]}}"#,
                serde_json::to_string(&emoji).unwrap()
            ),
        );
        let main_id = fonts::load(&main).unwrap();
        let emoji_id = fonts::load(&emoji).unwrap();
        let set = fonts::FontSet::new(Some(main_id));
        assert_eq!(set.lookup('A').0, main_id);
        assert_eq!(set.lookup('😀').0, emoji_id, "from the fallback");
    }

    #[test]
    fn text_with_a_raster_font_asset_draws_coverage_glyphs() {
        use crate::scene::{Canvas, RectTransform, Scene, UiText};
        use crate::ui::draw::{UiTexture, build};
        use crate::ui::text::{GlyphAtlas, UiFont};
        let raster = font_file(
            "TextRaster.ttf",
            epaint_default_fonts::UBUNTU_LIGHT,
            r#"{"render_mode":"Raster"}"#,
        );
        let mut s = Scene::new("Fonts");
        let canvas = s.spawn("Canvas", None);
        s.add(canvas, Canvas::default());
        let t = s.spawn("Text", Some(canvas));
        s.add(t, RectTransform::stretched());
        s.add(
            t,
            UiText {
                text: "Hi <b>there</b>".into(),
                font: Some(raster),
                ..Default::default()
            },
        );
        let l = crate::ui::layout(&s, glam::Vec2::new(400.0, 300.0));
        let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
        let glyphs: Vec<_> = list
            .batches
            .iter()
            .filter(|b| b.texture == UiTexture::SdfGlyphs)
            .collect();
        assert!(!glyphs.is_empty());
        let b = glyphs[0];
        let verts = &list.vertices[b.start as usize..(b.start + b.count) as usize];
        assert!(
            verts.iter().all(|v| v.mask == 1.0),
            "coverage, not distances"
        );
        assert_eq!(b.count, 7 * 6, "seven visible letters");
    }
}
