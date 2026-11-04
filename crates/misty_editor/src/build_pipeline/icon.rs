//! Game icons: a PNG from Assets becomes a multi-size Windows icon, written as an `.ico` file
//! and embedded into the built `.exe` as its icon resource (on a Windows machine).

use misty_engine::image::{self, RgbaImage, imageops::FilterType};

/// Sizes in the icon (Explorer, taskbar, Alt+Tab, large views).
pub const SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

/// One image of an icon, encoded the way `.ico` files and icon resources store it.
pub struct IconImage {
    pub size: u32,
    /// BMP data (small sizes) or a PNG file (256).
    pub data: Vec<u8>,
}

/// Square images of every size, from any PNG/JPG (non-square images are centred on a
/// transparent square first).
pub fn images(source: &[u8]) -> Result<Vec<IconImage>, String> {
    images_sized(source, &SIZES)
}

/// [`images`] with a chosen list of sizes (the editor's own icon has more of them).
pub fn images_sized(source: &[u8], sizes: &[u32]) -> Result<Vec<IconImage>, String> {
    let img = image::load_from_memory(source)
        .map_err(|e| format!("icon: {e}"))?
        .into_rgba8();
    let side = img.width().max(img.height());
    let mut square = RgbaImage::new(side, side);
    let (x, y) = ((side - img.width()) / 2, (side - img.height()) / 2);
    image::imageops::overlay(&mut square, &img, x as i64, y as i64);
    sizes
        .iter()
        .map(|&size| {
            let scaled = image::imageops::resize(&square, size, size, FilterType::Lanczos3);
            let data = if size >= 256 {
                png(&scaled)?
            } else {
                dib(&scaled)
            };
            Ok(IconImage { size, data })
        })
        .collect()
}

fn png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// 32-bit BGRA bitmap with the doubled height and AND mask that icon bitmaps use.
fn dib(img: &RgbaImage) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let mask_row = (w.div_ceil(32) * 4) as usize;
    let mut out = Vec::new();
    // BITMAPINFOHEADER
    for v in [40u32, w, h * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    for _ in 0..6 {
        out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB, size, resolution, colours
    }
    // Pixels, bottom row first.
    for y in (0..h).rev() {
        for x in 0..w {
            let [r, g, b, a] = img.get_pixel(x, y).0;
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    // AND mask: all zero (the alpha channel decides transparency).
    out.resize(out.len() + mask_row * h as usize, 0);
    out
}

/// ICONDIRENTRY / GRPICONDIRENTRY fields shared by both formats.
fn entry_head(img: &IconImage, out: &mut Vec<u8>) {
    let side = if img.size >= 256 { 0 } else { img.size as u8 };
    out.extend_from_slice(&[side, side, 0, 0]);
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bit count
    out.extend_from_slice(&(img.data.len() as u32).to_le_bytes());
}

fn dir_head(count: usize, out: &mut Vec<u8>) {
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&(count as u16).to_le_bytes());
}

/// A `.ico` file.
pub fn ico_file(images: &[IconImage]) -> Vec<u8> {
    let mut out = Vec::new();
    dir_head(images.len(), &mut out);
    let mut offset = 6 + 16 * images.len() as u32;
    for img in images {
        entry_head(img, &mut out);
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.data.len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img.data);
    }
    out
}

/// The RT_GROUP_ICON resource: like the file header, with resource ids instead of offsets.
pub fn group_resource(images: &[IconImage], first_id: u16) -> Vec<u8> {
    let mut out = Vec::new();
    dir_head(images.len(), &mut out);
    for (i, img) in images.iter().enumerate() {
        entry_head(img, &mut out);
        out.extend_from_slice(&(first_id + i as u16).to_le_bytes());
    }
    out
}

/// Put the icon into a Windows executable (any CPU). Needs a Windows machine to run.
#[cfg(windows)]
pub fn embed(exe: &std::path::Path, images: &[IconImage]) -> Result<(), String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn BeginUpdateResourceW(file: *const u16, delete_existing: i32) -> *mut c_void;
        fn UpdateResourceW(
            update: *mut c_void,
            kind: *const u16,
            name: *const u16,
            language: u16,
            data: *const c_void,
            size: u32,
        ) -> i32;
        fn EndUpdateResourceW(update: *mut c_void, discard: i32) -> i32;
    }
    const RT_ICON: usize = 3;
    const RT_GROUP_ICON: usize = 14;
    // Integer resource ids are passed as pointer values (MAKEINTRESOURCE).
    let id = |n: usize| n as *const u16;

    // The player's own Misty icon uses the neutral language like ours, so ours replace it
    // (its 7th image, past our 6, stays as an unused image).
    let path: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let err = || format!("icon: {}", std::io::Error::last_os_error());
    // SAFETY: plain Win32 calls with a NUL-terminated path and buffers that outlive them.
    unsafe {
        let handle = BeginUpdateResourceW(path.as_ptr(), 0);
        if handle.is_null() {
            return Err(err());
        }
        let mut ok = true;
        for (i, img) in images.iter().enumerate() {
            ok &= UpdateResourceW(
                handle,
                id(RT_ICON),
                id(i + 1),
                0,
                img.data.as_ptr().cast(),
                img.data.len() as u32,
            ) != 0;
        }
        let group = group_resource(images, 1);
        ok &= UpdateResourceW(
            handle,
            id(RT_GROUP_ICON),
            id(1),
            0,
            group.as_ptr().cast(),
            group.len() as u32,
        ) != 0;
        let failure = (!ok).then(err);
        if EndUpdateResourceW(handle, i32::from(!ok)) == 0 && ok {
            return Err(err());
        }
        failure.map_or(Ok(()), Err)
    }
}

#[cfg(not(windows))]
pub fn embed(_exe: &std::path::Path, _images: &[IconImage]) -> Result<(), String> {
    Err("embedding the .exe icon needs a Windows machine; the window icon still works".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Vec<u8> {
        let img = RgbaImage::from_fn(300, 200, |x, _| image::Rgba([x as u8, 0, 200, 255]));
        png(&img).unwrap()
    }

    #[test]
    fn icon_files_hold_every_size() {
        let imgs = images(&source()).unwrap();
        assert_eq!(imgs.iter().map(|i| i.size).collect::<Vec<_>>(), SIZES);
        // 16×16 DIB: header + pixels + mask.
        assert_eq!(imgs[0].data.len(), 40 + 16 * 16 * 4 + 4 * 16);
        assert_eq!(&imgs[5].data[1..4], b"PNG");
        let ico = ico_file(&imgs);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 6, 0]);
        // The first entry points just past the directory.
        assert_eq!(
            u32::from_le_bytes(ico[18..22].try_into().unwrap()),
            6 + 16 * 6
        );
        let group = group_resource(&imgs, 1);
        assert_eq!(group.len(), 6 + 14 * 6);
        assert!(images(b"not an image").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn embeds_into_an_executable() {
        // Any PE file accepts resources; use a copy of this test binary, which already carries
        // the Misty icon (build.rs): the game icon must replace it, not sit beside it.
        let exe = std::env::current_exe().unwrap();
        let copy = std::env::temp_dir().join(format!("misty_icon_{}.exe", std::process::id()));
        std::fs::copy(&exe, &copy).unwrap();
        assert_eq!(
            icon_group_languages(&copy),
            [0],
            "the Misty icon, neutral language"
        );
        let before = std::fs::metadata(&copy).unwrap().len();
        embed(&copy, &images(&source()).unwrap()).unwrap();
        assert_ne!(std::fs::metadata(&copy).unwrap().len(), before);
        assert_eq!(
            icon_group_languages(&copy),
            [0],
            "one icon after the game's replaced it"
        );
        let _ = std::fs::remove_file(&copy);
    }

    /// Languages of icon group 1 in `exe`.
    #[cfg(windows)]
    fn icon_group_languages(exe: &std::path::Path) -> Vec<u16> {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;
        type EnumLang =
            unsafe extern "system" fn(*mut c_void, *const u16, *const u16, u16, isize) -> i32;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn LoadLibraryExW(file: *const u16, reserved: *mut c_void, flags: u32) -> *mut c_void;
            fn EnumResourceLanguagesW(
                module: *mut c_void,
                kind: *const u16,
                name: *const u16,
                callback: EnumLang,
                param: isize,
            ) -> i32;
            fn FreeLibrary(module: *mut c_void) -> i32;
        }
        unsafe extern "system" fn push(
            _: *mut c_void,
            _: *const u16,
            _: *const u16,
            lang: u16,
            param: isize,
        ) -> i32 {
            // SAFETY: `param` is the Vec passed below, alive for the enumeration.
            unsafe { (*(param as *mut Vec<u16>)).push(lang) };
            1
        }
        const LOAD_LIBRARY_AS_DATAFILE: u32 = 2;
        // Integer resource ids are passed as pointer values (MAKEINTRESOURCE).
        let id = |n: usize| n as *const u16;
        let path: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
        let mut langs = Vec::new();
        // SAFETY: the module is loaded as data only and freed before returning.
        unsafe {
            let module = LoadLibraryExW(
                path.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_AS_DATAFILE,
            );
            assert!(!module.is_null());
            EnumResourceLanguagesW(
                module,
                id(14), // RT_GROUP_ICON
                id(1),
                push,
                &mut langs as *mut Vec<u16> as isize,
            );
            FreeLibrary(module);
        }
        langs
    }
}
