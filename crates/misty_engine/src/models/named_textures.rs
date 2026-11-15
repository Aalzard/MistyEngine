//! Textures found by a material's name, for model files whose materials name none (asset
//! packs often ship FBX files with bare materials "Bark", "Leave" beside a `Textures` folder
//! of `Bark_Alb.tif`, `Bark_NM.tif`, `Leave_Gloss.tif`, …).
//!
//! A texture belongs to a material when its file name is the material's name, alone or
//! followed by `_`, `-`, `.` or a space and a suffix (case does not matter). The suffix says
//! what the map is; an unknown suffix (`Leave_Autumn`) is taken for a colour variant, the
//! first in alphabetical order when there are several.
//!
//! Also here: whether an albedo texture has cut-outs (leaf cards, fences), which FBX
//! materials do not say.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

/// The maps of one material found by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NamedTextures {
    pub albedo: Option<PathBuf>,
    pub normal: Option<PathBuf>,
    pub occlusion: Option<PathBuf>,
    /// Smoothness (white = shiny).
    pub gloss: Option<PathBuf>,
    /// Roughness (white = dull).
    pub roughness: Option<PathBuf>,
    /// Light through the surface (white = lets light through).
    pub translucency: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Albedo,
    Normal,
    Occlusion,
    Gloss,
    Roughness,
    Translucency,
    /// A map the Standard material has no automatic use for (height, masks, …).
    Other,
    /// Not a known suffix: a colour variant.
    Unknown,
}

fn kind(suffix: &str) -> Kind {
    match suffix {
        "" | "alb" | "albedo" | "basecolor" | "base_color" | "basecolour" | "base" | "diffuse"
        | "diff" | "dif" | "color" | "colour" | "col" | "d" | "bc" => Kind::Albedo,
        "nm" | "normal" | "normals" | "normalmap" | "nrm" | "norm" | "nor" | "n" => Kind::Normal,
        "ao" | "occlusion" | "occ" | "ambientocclusion" => Kind::Occlusion,
        "gloss" | "glossiness" | "smoothness" | "smooth" | "gls" => Kind::Gloss,
        "rough" | "roughness" | "rgh" | "r" => Kind::Roughness,
        "transluc" | "translucency" | "translucent" | "transmission" | "trans" | "sss" => {
            Kind::Translucency
        }
        "metal" | "metallic" | "metalness" | "spec" | "specular" | "height" | "disp"
        | "displacement" | "bump" | "mask" | "emis" | "emission" | "emissive" | "opacity"
        | "alpha" | "h" | "m" | "s" | "e" => Kind::Other,
        _ => Kind::Unknown,
    }
}

/// The folders searched, nearest first: the model's own, a `Textures` folder in it, and
/// `Textures` folders up to two levels above it.
fn folders(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.to_path_buf(), dir.join("Textures")];
    let mut up = dir.parent();
    for _ in 0..2 {
        let Some(u) = up else { break };
        out.push(u.join("Textures"));
        up = u.parent();
    }
    out
}

/// The files directly in `folder` (on disk, or in the mounted pak of a built game).
fn list(folder: &Path) -> Vec<PathBuf> {
    if crate::vfs::is_mounted() {
        return crate::vfs::files_under(folder)
            .into_iter()
            .filter(|p| p.parent() == Some(folder))
            .collect();
    }
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect()
}

/// The textures named after `material` near the model folder `dir`.
pub fn find(dir: &Path, material: &str) -> NamedTextures {
    let mut out = NamedTextures::default();
    let name = material.trim().to_ascii_lowercase();
    if name.is_empty() {
        return out;
    }
    // A colour variant only stands in when no real albedo was found anywhere.
    let mut variant: Option<PathBuf> = None;
    for folder in folders(dir) {
        let mut files: Vec<PathBuf> = list(&folder)
            .into_iter()
            .filter(|p| crate::assets::is_texture(&p.to_string_lossy()))
            .collect();
        files.sort();
        for file in files {
            let Some(stem) = file
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
            else {
                continue;
            };
            let Some(rest) = stem.strip_prefix(&name) else {
                continue;
            };
            let suffix = match rest.chars().next() {
                None => "",
                Some('_' | '-' | '.' | ' ') => &rest[1..],
                // "Barking" is not "Bark".
                Some(_) => continue,
            };
            let slot = match kind(suffix) {
                Kind::Albedo => &mut out.albedo,
                Kind::Normal => &mut out.normal,
                Kind::Occlusion => &mut out.occlusion,
                Kind::Gloss => &mut out.gloss,
                Kind::Roughness => &mut out.roughness,
                Kind::Translucency => &mut out.translucency,
                Kind::Other => continue,
                Kind::Unknown => &mut variant,
            };
            if slot.is_none() {
                *slot = Some(file);
            }
        }
    }
    if out.albedo.is_none() {
        out.albedo = variant;
    }
    out
}

/// Whether the image at `path` has cut-outs: an alpha channel with a real share of clear
/// texels (an RGBA file that is opaque everywhere has none). The answer is kept per file (and
/// its modification time): the check decodes the image once.
pub fn has_cutouts(path: &Path) -> bool {
    static KNOWN: LazyLock<Mutex<HashMap<PathBuf, (Option<SystemTime>, bool)>>> =
        LazyLock::new(Default::default);
    let stamp = crate::vfs::modified(path);
    if let Some((s, v)) = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).get(path)
        && *s == stamp
    {
        return *v;
    }
    let answer = crate::vfs::read(path).is_ok_and(|bytes| bytes_have_cutouts(&bytes));
    KNOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_path_buf(), (stamp, answer));
    answer
}

/// [`has_cutouts`] of image data.
pub fn bytes_have_cutouts(bytes: &[u8]) -> bool {
    let bytes = crate::render::texture::tiff_alpha(bytes);
    // The header says whether there is an alpha channel at all (no decode).
    let alpha = image::ImageReader::new(std::io::Cursor::new(&bytes[..]))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_decoder().ok())
        .is_some_and(|d| {
            use image::ImageDecoder;
            d.color_type().has_alpha()
        });
    if !alpha {
        return false;
    }
    let Ok(img) = image::load_from_memory(&bytes) else {
        return false;
    };
    let img = img.into_rgba8();
    // Every seventh texel is plenty; at least half a percent of them clear.
    let (mut clear, mut all) = (0u32, 0u32);
    for p in img.pixels().step_by(7) {
        all += 1;
        clear += u32::from(p[3] < 128);
    }
    clear * 200 >= all.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(alpha: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let img =
            image::RgbaImage::from_fn(16, 16, |x, y| image::Rgba([200, 150, 90, alpha(x, y)]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn textures_are_found_by_the_material_name() {
        let root = std::env::temp_dir().join(format!("misty_named_tex_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let model_dir = root.join("Pack/Model/3");
        let textures = root.join("Pack/Textures");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::create_dir_all(&textures).unwrap();
        for name in [
            "Bark_Alb.tif",
            "Bark_AO.tif",
            "Bark_Gloss.tif",
            "Bark_NM.tif",
            "Barking_Dog.png",
            "Leave_Autmn.tif",
            "Leave_Green.tif",
            "Leave_NM.tif",
            "Leave_Transluc.tif",
            "Leave_Height.png",
            "Stone.png",
            "Stone_Roughness.jpg",
            "notes.txt",
        ] {
            std::fs::write(textures.join(name), b"x").unwrap();
        }
        let bark = find(&model_dir, "Bark");
        assert_eq!(bark.albedo, Some(textures.join("Bark_Alb.tif")));
        assert_eq!(bark.normal, Some(textures.join("Bark_NM.tif")));
        assert_eq!(bark.occlusion, Some(textures.join("Bark_AO.tif")));
        assert_eq!(bark.gloss, Some(textures.join("Bark_Gloss.tif")));
        assert_eq!(bark.roughness, None);
        // No albedo suffix: the first colour variant; the height map is not one.
        let leave = find(&model_dir, "leave");
        assert_eq!(leave.albedo, Some(textures.join("Leave_Autmn.tif")));
        assert_eq!(leave.normal, Some(textures.join("Leave_NM.tif")));
        assert_eq!(
            leave.translucency,
            Some(textures.join("Leave_Transluc.tif"))
        );
        let stone = find(&model_dir, "Stone");
        assert_eq!(stone.albedo, Some(textures.join("Stone.png")));
        assert_eq!(stone.roughness, Some(textures.join("Stone_Roughness.jpg")));
        assert_eq!(find(&model_dir, "Glass"), NamedTextures::default());
        assert_eq!(find(&model_dir, ""), NamedTextures::default());
        // A texture beside the model wins over the shared folder.
        std::fs::write(model_dir.join("Bark_Albedo.png"), b"x").unwrap();
        assert_eq!(
            find(&model_dir, "Bark").albedo,
            Some(model_dir.join("Bark_Albedo.png"))
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cutouts_need_clear_texels() {
        assert!(bytes_have_cutouts(&png(|x, _| if x < 8 { 0 } else { 255 })));
        // An alpha channel that is opaque everywhere (a common export) is no cut-out.
        assert!(!bytes_have_cutouts(&png(|_, _| 255)));
        let rgb = image::RgbImage::new(8, 8);
        let mut out = std::io::Cursor::new(Vec::new());
        rgb.write_to(&mut out, image::ImageFormat::Png).unwrap();
        assert!(!bytes_have_cutouts(out.get_ref()));
        assert!(!bytes_have_cutouts(b"not an image"));
    }
}
