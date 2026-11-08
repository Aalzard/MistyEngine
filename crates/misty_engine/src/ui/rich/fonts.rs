//! Fonts for rich text: the built-in font, project font files (`.ttf` / `.otf`) and operating
//! system fonts used as a fallback for characters the chosen fonts lack (Arabic, Hebrew, …),
//! plus complex-script shaping with rustybuzz.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use ab_glyph::{Font, FontArc, GlyphId, PxScale, point};

/// A loaded font, process-wide (glyph atlases key glyphs by it).
pub type FontId = u32;

/// The built-in font.
pub const BUILTIN: FontId = 0;

/// Glyph lookups and metrics the layout needs (a fake in tests).
pub trait FontSource {
    /// The font and glyph that draw `c` (falling back through the fonts; a missing glyph comes
    /// back as glyph 0 of the first font).
    fn lookup(&self, c: char) -> (FontId, u16);
    fn advance(&self, font: FontId, glyph: u16, px: f32) -> f32;
    fn kern(&self, font: FontId, a: u16, b: u16, px: f32) -> f32;
    /// Baseline to the top of the tallest glyphs (positive) of the main font.
    fn ascent(&self, px: f32) -> f32;
    /// Baseline to the bottom (negative).
    fn descent(&self, px: f32) -> f32;
    fn line_height(&self, px: f32) -> f32;
    /// Shape a run of one font (`rtl` = right to left): glyphs in visual order with the byte
    /// offset of the character each belongs to. `None` = no shaper (use per-character glyphs).
    fn shape(&self, font: FontId, text: &str, rtl: bool, px: f32) -> Option<Vec<Shaped>> {
        let _ = (font, text, rtl, px);
        None
    }
}

/// One shaped glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaped {
    pub glyph: u16,
    /// Byte offset in the shaped text of the character (cluster) it belongs to.
    pub cluster: usize,
    pub advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

/// A font file in memory.
pub struct LoadedFont {
    pub id: FontId,
    pub name: String,
    data: Arc<Vec<u8>>,
    index: u32,
    font: FontArc,
    /// Font units → pixels at 1 px em size.
    units: f32,
    /// Its font asset settings (atlas mode, sampling size, fallbacks).
    pub settings: super::font_asset::FontImporter,
}

impl LoadedFont {
    fn new(id: FontId, name: String, data: Vec<u8>, index: u32) -> Result<Self, String> {
        let fv = ab_glyph::FontVec::try_from_vec_and_index(data.clone(), index)
            .map_err(|e| format!("{name}: {e}"))?;
        let font = FontArc::new(fv);
        let data = Arc::new(data);
        let units = 1.0 / font.units_per_em().unwrap_or(1000.0);
        Ok(Self {
            id,
            name,
            data,
            index,
            font,
            units,
            settings: Default::default(),
        })
    }

    pub fn glyph(&self, c: char) -> Option<u16> {
        let g = self.font.glyph_id(c);
        (g.0 != 0).then_some(g.0)
    }

    pub fn advance(&self, glyph: u16, px: f32) -> f32 {
        self.font.h_advance_unscaled(GlyphId(glyph)) * self.units * px
    }

    pub fn kern(&self, a: u16, b: u16, px: f32) -> f32 {
        self.font.kern_unscaled(GlyphId(a), GlyphId(b)) * self.units * px
    }

    pub fn ascent(&self, px: f32) -> f32 {
        self.font.ascent_unscaled() * self.units * px
    }

    pub fn descent(&self, px: f32) -> f32 {
        self.font.descent_unscaled() * self.units * px
    }

    pub fn line_height(&self, px: f32) -> f32 {
        (self.font.height_unscaled() + self.font.line_gap_unscaled()) * self.units * px
    }

    /// Coverage bitmap of a glyph at em size `px`: (w, h, offset of the top-left from the pen
    /// on the baseline, coverage 0–1). `None` for blank glyphs.
    pub fn coverage(&self, glyph: u16, px: f32) -> Option<(u32, u32, [f32; 2], Vec<f32>)> {
        let scale = px * self.units * self.font.height_unscaled();
        let g = GlyphId(glyph).with_scale_and_position(PxScale::from(scale), point(0.0, 0.0));
        let outlined = self.font.outline_glyph(g)?;
        let b = outlined.px_bounds();
        let (w, h) = (b.width() as u32, b.height() as u32);
        if w == 0 || h == 0 {
            return None;
        }
        let mut data = vec![0.0f32; (w * h) as usize];
        outlined.draw(|x, y, cov| {
            if let Some(p) = data.get_mut((y * w + x) as usize) {
                *p = cov.clamp(0.0, 1.0);
            }
        });
        Some((w, h, [b.min.x, b.min.y], data))
    }

    /// Shape `text` with rustybuzz.
    pub fn shape(&self, text: &str, rtl: bool, px: f32) -> Option<Vec<Shaped>> {
        let face = rustybuzz::Face::from_slice(&self.data, self.index)?;
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.set_direction(if rtl {
            rustybuzz::Direction::RightToLeft
        } else {
            rustybuzz::Direction::LeftToRight
        });
        buf.guess_segment_properties();
        let out = rustybuzz::shape(&face, &[], buf);
        let s = px * self.units;
        Some(
            out.glyph_infos()
                .iter()
                .zip(out.glyph_positions())
                .map(|(i, p)| Shaped {
                    glyph: i.glyph_id as u16,
                    cluster: i.cluster as usize,
                    advance: p.x_advance as f32 * s,
                    x_offset: p.x_offset as f32 * s,
                    y_offset: p.y_offset as f32 * s,
                })
                .collect(),
        )
    }
}

struct Library {
    fonts: Vec<Arc<LoadedFont>>,
    by_path: HashMap<String, Option<FontId>>,
    /// Operating system fallback fonts (loaded on first need).
    os: Option<Vec<FontId>>,
}

fn library() -> &'static Mutex<Library> {
    static LIB: OnceLock<Mutex<Library>> = OnceLock::new();
    LIB.get_or_init(|| {
        let builtin = LoadedFont::new(
            BUILTIN,
            "Ubuntu Light (built-in)".into(),
            epaint_default_fonts::UBUNTU_LIGHT.to_vec(),
            0,
        )
        .expect("the built-in font parses");
        Mutex::new(Library {
            fonts: vec![Arc::new(builtin)],
            by_path: HashMap::new(),
            os: None,
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, Library> {
    library().lock().unwrap_or_else(|e| e.into_inner())
}

/// A font by id.
pub fn font(id: FontId) -> Arc<LoadedFont> {
    let lib = lock();
    lib.fonts
        .get(id as usize)
        .cloned()
        .unwrap_or_else(|| lib.fonts[0].clone())
}

/// File extensions of font assets.
pub fn is_font(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".ttf") || p.ends_with(".otf") || p.ends_with(".ttc")
}

/// Load a project font (`Assets/Fonts/Title.ttf`, read through the virtual file system) or an
/// absolute path; `None` (logged once by the caller) when it can't be read.
pub fn load(path: &str) -> Result<FontId, String> {
    if let Some(known) = lock().by_path.get(path).copied() {
        return known.ok_or_else(|| format!("{path}: unreadable font"));
    }
    let abs = if std::path::Path::new(path).is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        crate::assets::absolute(path)
    };
    let result = crate::vfs::read(&abs)
        .map_err(|e| format!("{path}: {e}"))
        .and_then(|data| {
            let settings = super::font_asset::FontImporter::load(&abs);
            let mut lib = lock();
            let id = lib.fonts.len() as FontId;
            let mut f = LoadedFont::new(id, path.to_owned(), data, 0)?;
            f.settings = settings;
            lib.fonts.push(Arc::new(f));
            Ok(id)
        });
    lock()
        .by_path
        .insert(path.to_owned(), result.as_ref().ok().copied());
    result
}

/// Forget a loaded font so its next use reads the file and its settings again (the editor
/// applied new import settings). Glyphs already made keep the old id and are not reused.
pub fn reload(path: &str) {
    lock().by_path.remove(path);
}

/// System fonts with wide script coverage (Arabic, Hebrew, Greek, Cyrillic, …), tried in order.
fn os_candidates() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if cfg!(windows) {
        let dir = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into())
            .join("Fonts");
        for f in ["segoeui.ttf", "arial.ttf", "tahoma.ttf"] {
            v.push(dir.join(f));
        }
    } else if cfg!(target_os = "macos") {
        for f in [
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial Unicode.ttf",
        ] {
            v.push(f.into());
        }
    } else {
        for f in [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
        ] {
            v.push(f.into());
        }
    }
    v
}

/// The operating system fallback fonts (loaded on first call; empty when none exist).
pub fn os_fallbacks() -> Vec<FontId> {
    if let Some(os) = &lock().os {
        return os.clone();
    }
    let mut ids = Vec::new();
    for p in os_candidates() {
        let Ok(data) = std::fs::read(&p) else {
            continue;
        };
        let mut lib = lock();
        let id = lib.fonts.len() as FontId;
        if let Ok(f) = LoadedFont::new(id, p.to_string_lossy().into_owned(), data, 0) {
            lib.fonts.push(Arc::new(f));
            ids.push(id);
        }
        if ids.len() >= 2 {
            break;
        }
    }
    lock().os = Some(ids.clone());
    ids
}

/// The fonts one text uses: its own font (or the built-in one), then the built-in font, then
/// the system fallbacks.
pub struct FontSet {
    fonts: Vec<Arc<LoadedFont>>,
    /// Fallbacks were added (loaded lazily when a character is missing).
    with_os: std::cell::Cell<bool>,
    extra: std::cell::RefCell<Vec<Arc<LoadedFont>>>,
}

impl FontSet {
    /// Fonts for `primary` (a project font; None = the built-in font): it, its font asset's
    /// fallback fonts, then the built-in font.
    pub fn new(primary: Option<FontId>) -> Self {
        let mut fonts: Vec<Arc<LoadedFont>> = Vec::new();
        if let Some(p) = primary.filter(|p| *p != BUILTIN) {
            let main = font(p);
            for path in &main.settings.fallback_fonts {
                if let Ok(id) = load(path)
                    && id != p
                    && id != BUILTIN
                    && !fonts.iter().any(|f| f.id == id)
                {
                    fonts.push(font(id));
                }
            }
            fonts.insert(0, main);
        }
        fonts.push(font(BUILTIN));
        Self {
            fonts,
            with_os: std::cell::Cell::new(false),
            extra: Default::default(),
        }
    }

    pub fn main(&self) -> &LoadedFont {
        &self.fonts[0]
    }

    fn by_id(&self, id: FontId) -> Arc<LoadedFont> {
        self.fonts
            .iter()
            .chain(self.extra.borrow().iter())
            .find(|f| f.id == id)
            .cloned()
            .unwrap_or_else(|| font(id))
    }
}

impl FontSource for FontSet {
    fn lookup(&self, c: char) -> (FontId, u16) {
        for f in &self.fonts {
            if let Some(g) = f.glyph(c) {
                return (f.id, g);
            }
        }
        if c.is_control() || c == super::parse::OBJECT {
            return (self.fonts[0].id, 0);
        }
        if !self.with_os.get() {
            self.with_os.set(true);
            *self.extra.borrow_mut() = os_fallbacks().into_iter().map(font).collect();
        }
        for f in self.extra.borrow().iter() {
            if let Some(g) = f.glyph(c) {
                return (f.id, g);
            }
        }
        (self.fonts[0].id, 0)
    }

    fn advance(&self, font: FontId, glyph: u16, px: f32) -> f32 {
        self.by_id(font).advance(glyph, px)
    }

    fn kern(&self, font: FontId, a: u16, b: u16, px: f32) -> f32 {
        self.by_id(font).kern(a, b, px)
    }

    fn ascent(&self, px: f32) -> f32 {
        self.main().ascent(px)
    }

    fn descent(&self, px: f32) -> f32 {
        self.main().descent(px)
    }

    fn line_height(&self, px: f32) -> f32 {
        self.main().line_height(px)
    }

    fn shape(&self, font: FontId, text: &str, rtl: bool, px: f32) -> Option<Vec<Shaped>> {
        self.by_id(font).shape(text, rtl, px)
    }
}
