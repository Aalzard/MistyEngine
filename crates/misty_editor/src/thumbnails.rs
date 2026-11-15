//! Texture previews for the Project window (HDR images tonemapped), decoded once per file
//! version, and the material previews (rendered by `material_preview`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Longest side of a preview.
const SIZE: u32 = 96;
/// A preview no panel asked for in this long is dropped with its texture (a long session
/// browsing a big project kept every one it ever showed, and those of deleted files).
const KEEP: Duration = Duration::from_secs(120);
/// Decoding time the previews may take per frame (about one frame at 60 Hz) before the rest
/// wait for the next frames: opening a folder of 4K images decoded them all in one frame.
const DECODE_BUDGET: Duration = Duration::from_millis(12);

struct Thumb {
    modified: Option<SystemTime>,
    /// None = the file could not be decoded (the icon is shown instead).
    texture: Option<egui::TextureHandle>,
    used: Instant,
}

#[derive(Default)]
pub struct Thumbnails {
    cache: HashMap<PathBuf, Thumb>,
    /// When the stale previews were last looked for.
    swept: Option<Instant>,
    /// The frame (egui pass) of the current budget and the decoding time spent in it. Per
    /// frame, not per 16 ms of wall time: a decode that ran over the budget held every frame of
    /// the next 16 ms back, each asking for another (round 73).
    window: Option<(u64, Duration)>,
    /// Material thumbnails and the Inspector's preview sphere.
    pub materials: crate::material_preview::MaterialPreviews,
    /// The Terrain Inspector's palette previews (grass tufts, trees, detail meshes, prefabs).
    pub terrain: crate::terrain_previews::TerrainPreviews,
}

impl Thumbnails {
    /// Preview of an image file (decoded on first request and when the file changes).
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        // The background file watch's stamp: a `stat` per visible thumbnail per frame on the
        // main thread froze the editor while the project's disk was busy.
        let modified = misty_engine::file_watch::cached_modified(path);
        let now = Instant::now();
        self.sweep(now);
        if let Some(t) = self.cache.get_mut(path) {
            t.used = now;
            if t.modified == modified {
                return t.texture.clone();
            }
        }
        // Over this frame's decoding budget: next frame (the first one always decodes).
        let pass = ctx.cumulative_pass_nr();
        let spent = match self.window {
            Some((p, spent)) if p == pass => spent,
            _ => Duration::ZERO,
        };
        if spent >= DECODE_BUDGET {
            ctx.request_repaint();
            self.window = Some((pass, spent));
            return self.cache.get(path).and_then(|t| t.texture.clone());
        }
        let tex = decode(path).map(|img| {
            ctx.load_texture(
                format!("thumb:{}", path.display()),
                img,
                egui::TextureOptions::LINEAR,
            )
        });
        self.window = Some((pass, spent + now.elapsed()));
        self.cache.insert(
            path.to_path_buf(),
            Thumb {
                modified,
                texture: tex.clone(),
                used: now,
            },
        );
        tex
    }

    /// Use up this frame's decoding budget (tests).
    #[cfg(test)]
    fn spend_budget(&mut self, ctx: &egui::Context) {
        self.window = Some((ctx.cumulative_pass_nr(), DECODE_BUDGET));
    }

    /// Drop the previews nobody asked for in [`KEEP`] (looked for every few seconds).
    fn sweep(&mut self, now: Instant) {
        if self
            .swept
            .is_some_and(|t| now.duration_since(t) < Duration::from_secs(5))
        {
            return;
        }
        self.swept = Some(now);
        self.cache
            .retain(|_, t| now.saturating_duration_since(t.used) < KEEP);
    }
}

fn decode(path: &Path) -> Option<egui::ColorImage> {
    if misty_engine::hdri::is_hdri(&path.to_string_lossy()) {
        // HDR images: tonemapped (auto exposure) so the sky reads at a glance.
        let img = misty_engine::hdri::HdrImage::load(path).ok()?;
        let (w, h, rgba) = misty_engine::hdri::preview::rgba8(&img, SIZE);
        return Some(egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            &rgba,
        ));
    }
    let img = misty_engine::image::open(path).ok()?;
    let small = img.thumbnail(SIZE, SIZE).into_rgba8();
    let size = [small.width() as usize, small.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        small.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Previews not asked for in a while are dropped (with their textures); ones still shown
    /// stay. The cache used to keep every preview ever shown.
    #[test]
    fn texture_previews_not_shown_for_a_while_are_dropped() {
        let dir = std::env::temp_dir().join(format!("misty_tex_thumbs_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<PathBuf> = (0..3).map(|i| dir.join(format!("T{i}.png"))).collect();
        for p in &paths {
            misty_engine::image::RgbaImage::from_pixel(
                8,
                8,
                misty_engine::image::Rgba([200, 100, 50, 255]),
            )
            .save(p)
            .unwrap();
        }
        let ctx = egui::Context::default();
        let mut t = Thumbnails::default();
        for p in &paths {
            assert!(t.get(&ctx, p).is_some());
            // A new budget window for each (the test is not about the budget).
            t.window = None;
        }
        assert_eq!(t.cache.len(), 3);
        // Two of them were last shown long ago; the third is shown again now.
        let Some(long_ago) = Instant::now().checked_sub(KEEP + Duration::from_secs(1)) else {
            return;
        };
        for p in &paths[..2] {
            t.cache.get_mut(p).unwrap().used = long_ago;
        }
        t.swept = None;
        assert!(t.get(&ctx, &paths[2]).is_some());
        assert_eq!(t.cache.len(), 1);
        assert!(t.cache.contains_key(&paths[2]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Round 73: the decoding budget was "per 16 ms of wall time", not per frame. A decode that
    /// ran over it (a loaded machine) held every frame of the next 16 ms back, each asking for
    /// another frame: the material picker test ran out of the harness's frames that way. The
    /// next frame decodes, however soon it comes.
    #[test]
    fn a_frame_over_the_decoding_budget_holds_back_only_itself() {
        let dir = std::env::temp_dir().join(format!("misty_tex_budget_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("T.png");
        misty_engine::image::RgbaImage::from_pixel(
            8,
            8,
            misty_engine::image::Rgba([20, 100, 50, 255]),
        )
        .save(&p)
        .unwrap();
        let ctx = egui::Context::default();
        let mut t = Thumbnails::default();
        // This frame's budget is used up: the preview waits for the next frame.
        let _ = ctx.run_ui(Default::default(), |ui| {
            t.spend_budget(ui.ctx());
            assert!(t.get(ui.ctx(), &p).is_none(), "over the budget: next frame");
        });
        // The next frame decodes it.
        let _ = ctx.run_ui(Default::default(), |ui| {
            assert!(t.get(ui.ctx(), &p).is_some(), "decoded in the next frame");
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
