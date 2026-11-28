//! `ScreenCapture.CaptureScreenshot(filename, superSize)` and `Application.persistentDataPath`.
//!
//! A script asks; whoever draws the game (the player after presenting, the editor's Game
//! view) saves the next finished frame as a PNG. Relative file names land in the game's data
//! folder: the project folder in the editor (like the familiar engine), the per-user folder
//! next to PlayerPrefs and the saves in a built game (the game's own folder may be read-only).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Largest `superSize` accepted (a 4K window × 8 is already a 1 GB image).
pub const MAX_SUPER_SIZE: u32 = 8;

/// A screenshot asked for by a script.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Where to save it (absolute).
    pub path: PathBuf,
    /// Image size as a multiple of the window size (1 = as shown).
    pub super_size: u32,
}

static REQUESTS: Mutex<Vec<Request>> = Mutex::new(Vec::new());

/// The folder relative names resolve against (`Application.persistentDataPath`).
pub fn data_folder(root: &Path) -> PathBuf {
    crate::save::data_folder(root)
}

/// `name` as a path: kept when absolute, else under [`data_folder`]. A missing extension
/// becomes `.png`.
pub fn resolve(root: &Path, name: &str) -> PathBuf {
    let mut p = PathBuf::from(name);
    if p.extension().is_none() {
        p.set_extension("png");
    }
    if p.is_absolute() {
        p
    } else {
        data_folder(root).join(p)
    }
}

/// Ask for the next frame to be saved as `path` (an empty name is ignored).
pub fn request(root: &Path, name: &str, super_size: i64) {
    if name.trim().is_empty() {
        return;
    }
    let super_size = super_size.clamp(1, i64::from(MAX_SUPER_SIZE)) as u32;
    REQUESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Request {
            path: resolve(root, name),
            super_size,
        });
}

/// Whether a screenshot is waiting (so the renderer only reads the frame back when needed).
pub fn pending() -> bool {
    !REQUESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
}

/// The screenshots asked for since the last call.
pub fn take() -> Vec<Request> {
    std::mem::take(&mut *REQUESTS.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Save one frame (`width`×`height` RGBA) for `req`. A `superSize` above 1 scales the frame
/// up (smoothly; the frame is not drawn again at the larger size).
pub fn write(req: &Request, width: u32, height: u32, rgba: Vec<u8>) -> Result<(), String> {
    let img = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "screenshot: wrong pixel count".to_owned())?;
    let img = if req.super_size > 1 {
        image::imageops::resize(
            &img,
            width * req.super_size,
            height * req.super_size,
            image::imageops::FilterType::CatmullRom,
        )
    } else {
        img
    };
    if let Some(dir) = req.path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    image::DynamicImage::ImageRgba8(img)
        .to_rgb8()
        .save(&req.path)
        .map_err(|e| format!("{}: {e}", req.path.display()))
}

/// Save the frame for every waiting request; returns the errors (for the Console).
pub fn write_all(width: u32, height: u32, rgba: &[u8]) -> Vec<String> {
    take()
        .iter()
        .filter_map(|r| write(r, width, height, rgba.to_vec()).err())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_under_the_data_folder_with_png_added() {
        let root = Path::new("/tmp/proj");
        let p = resolve(root, "shots/one");
        assert!(p.ends_with("shots/one.png"), "{}", p.display());
        let abs = std::env::temp_dir().join("x.jpg");
        assert_eq!(resolve(root, abs.to_str().unwrap()), abs);
    }

    #[test]
    fn a_request_is_written_at_super_size() {
        let dir = std::env::temp_dir().join(format!("misty_capture_{}", std::process::id()));
        let req = Request {
            path: dir.join("a/shot.png"),
            super_size: 2,
        };
        let px = vec![255u8; 4 * 3 * 2];
        write(&req, 3, 2, px).unwrap();
        let img = image::open(&req.path).unwrap();
        assert_eq!((img.width(), img.height()), (6, 4));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
