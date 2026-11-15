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
/// For a model in the project's `Assets` folder, nothing above that folder is searched (the
/// project root and its neighbours are not the project's assets).
fn folders(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.to_path_buf(), dir.join("Textures")];
    let mut up = dir.parent();
    for _ in 0..2 {
        let Some(u) = up else { break };
        out.push(u.join("Textures"));
        up = u.parent();
    }
    if let Some(assets) = crate::assets::project_root().map(|r| r.join("Assets"))
        && dir.starts_with(&assets)
    {
        out.retain(|f| f.starts_with(&assets));
    }
    out
}

/// The kind of map a colour-variant suffix ends in (`red_ao` → occlusion): such a file is
/// another material's map (`Leaf_Red_AO` belongs to `Leaf_Red`), not a variant of this one.
fn last_kind(suffix: &str) -> Kind {
    suffix
        .rsplit(['_', '-', '.', ' '])
        .next()
        .map_or(Kind::Unknown, kind)
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
    let found = find_exact(dir, material);
    // Blender names the copies of a material "Bark.001", "Bark.002": the textures are named
    // after the original.
    if found == NamedTextures::default()
        && let Some((base, n)) = material.trim().rsplit_once('.')
        && n.len() == 3
        && n.bytes().all(|b| b.is_ascii_digit())
    {
        return find_exact(dir, base);
    }
    found
}

fn find_exact(dir: &Path, material: &str) -> NamedTextures {
    let mut out = NamedTextures::default();
    let name = material.trim().to_lowercase();
    if name.is_empty() {
        return out;
    }
    // A colour variant only stands in when no real albedo was found anywhere.
    let mut variant: Option<PathBuf> = None;
    for folder in folders(dir) {
        // (Lower-case stem, file), alphabetical whatever the case.
        let mut files: Vec<(String, PathBuf)> = list(&folder)
            .into_iter()
            .filter(|p| crate::assets::is_texture(&p.to_string_lossy()))
            .filter_map(|p| Some((p.file_stem()?.to_string_lossy().to_lowercase(), p)))
            .collect();
        files.sort();
        for (stem, file) in files {
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
                Kind::Unknown if !matches!(last_kind(suffix), Kind::Albedo | Kind::Unknown) => {
                    continue;
                }
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
    /// Per file: its modification time when checked, and the answer.
    type Known = HashMap<PathBuf, (Option<SystemTime>, bool)>;
    static KNOWN: LazyLock<Mutex<Known>> = LazyLock::new(Default::default);
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
    let extra = crate::render::texture::tiff_extra_alpha(bytes);
    let bytes = crate::render::texture::tiff_alpha(bytes);
    // The header says whether there is an alpha channel at all (no decode).
    let alpha = extra
        || image::ImageReader::new(std::io::Cursor::new(&bytes[..]))
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
    let Ok(img) = crate::render::texture::decode(&bytes) else {
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

    /// Names that fooled the lookup: another material's maps taken for a colour variant
    /// (`Leaf_Red_AO` for `Leaf`), upper case sorting first, non-ASCII names, Blender's
    /// `.001` copies, a name with a path in it, and folders outside the project's `Assets`.
    #[test]
    fn named_textures_edge_cases() {
        let root = std::env::temp_dir().join(format!("misty_named_edge_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("Project");
        let dir = project.join("Assets/Pack");
        let textures = dir.join("Textures");
        std::fs::create_dir_all(&textures).unwrap();
        let _scope = crate::assets::scoped_root(&project);
        for name in [
            "Leaf_Red_AO.png",
            "Leaf_Red_NM.png",
            "Leaf_NM.png",
            "leaf_yellow.png",
            "Leaf_Zed.png",
            "Écorce_Alb.png",
            "Bark_Alb.png",
            "Secret.png",
        ] {
            std::fs::write(textures.join(name), b"x").unwrap();
        }
        // Not a colour variant: the ambient occlusion and normal map of a "Leaf_Red".
        let leaf = find(&dir, "Leaf");
        assert_eq!(leaf.normal, Some(textures.join("Leaf_NM.png")));
        assert_eq!(
            leaf.albedo,
            Some(textures.join("leaf_yellow.png")),
            "the first colour variant, whatever its case"
        );
        assert_eq!(
            find(&dir, "écorce").albedo,
            Some(textures.join("Écorce_Alb.png"))
        );
        assert_eq!(find(&dir, "ÉCORCE").albedo, Some(textures.join("Écorce_Alb.png")));
        // Blender names copies of a material "Bark.001".
        assert_eq!(find(&dir, "Bark.001").albedo, Some(textures.join("Bark_Alb.png")));
        // A material name is a name, not a path.
        assert_eq!(find(&dir, "../Textures/Secret"), NamedTextures::default());
        assert_eq!(find(&dir, "Textures/Secret"), NamedTextures::default());
        // Folders above the project's Assets are not searched (they are not in the project:
        // the textures were used from outside it, or copied in from a neighbour project).
        let outside = root.join("Textures");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("Stone_Alb.png"), b"x").unwrap();
        std::fs::create_dir_all(project.join("Textures")).unwrap();
        std::fs::write(project.join("Textures/Moss.png"), b"x").unwrap();
        let top = project.join("Assets");
        assert_eq!(find(&top, "Stone"), NamedTextures::default());
        assert_eq!(find(&top, "Moss"), NamedTextures::default());
        assert_eq!(find(&dir, "Bark").albedo, Some(textures.join("Bark_Alb.png")));
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
