//! TIFF files of every kind asset packs (and broken downloads) bring: decoding must give an
//! image or an error, never panic, hang or run out of memory.

use super::texture::{decode, tiff_alpha};

/// One IFD entry: tag, field type (3 = SHORT, 4 = LONG) and its values.
type Entry = (u16, u16, Vec<u32>);

/// A little-endian TIFF: `ifds` (each its entries; the first IFD's data follows the header),
/// `data` appended after the IFDs; entries may name `DATA` + n as an offset into it.
const DATA: u32 = 0x4000_0000;

fn tiff(ifds: &[Vec<Entry>], data: &[u8]) -> Vec<u8> {
    let mut out = vec![b'I', b'I', 42, 0];
    out.extend(8u32.to_le_bytes());
    // Lay out: IFDs one after another, each followed by its out-of-line values, then data.
    let mut sizes = Vec::new();
    for ifd in ifds {
        let extra: usize = ifd
            .iter()
            .map(|(_, kind, v)| {
                let size = if *kind == 3 { 2 } else { 4 } * v.len();
                if size > 4 { size } else { 0 }
            })
            .sum();
        sizes.push(2 + ifd.len() * 12 + 4 + extra);
    }
    let data_at = 8 + sizes.iter().sum::<usize>() as u32;
    let mut at = 8usize;
    for (k, ifd) in ifds.iter().enumerate() {
        let mut values = Vec::new();
        let values_at = at + 2 + ifd.len() * 12 + 4;
        out.extend((ifd.len() as u16).to_le_bytes());
        let fix = |v: u32| {
            if v >= DATA { v - DATA + data_at } else { v }
        };
        for (tag, kind, v) in ifd {
            out.extend(tag.to_le_bytes());
            out.extend(kind.to_le_bytes());
            out.extend((v.len() as u32).to_le_bytes());
            let mut bytes = Vec::new();
            for x in v {
                let x = fix(*x);
                if *kind == 3 {
                    bytes.extend((x as u16).to_le_bytes());
                } else {
                    bytes.extend(x.to_le_bytes());
                }
            }
            if bytes.len() <= 4 {
                bytes.resize(4, 0);
                out.extend(bytes);
            } else {
                out.extend(((values_at + values.len()) as u32).to_le_bytes());
                values.extend(bytes);
            }
        }
        let next = if k + 1 < ifds.len() {
            (at + sizes[k]) as u32
        } else {
            0
        };
        out.extend(next.to_le_bytes());
        out.extend(values);
        at += sizes[k];
        assert_eq!(out.len(), at);
    }
    out.extend_from_slice(data);
    out
}

/// The usual tags of a one-strip image; `extra` adds or replaces tags.
fn image_ifd(
    w: u32,
    h: u32,
    bits: &[u32],
    photometric: u32,
    bytes: u32,
    extra: &[Entry],
) -> Vec<Entry> {
    let mut e: Vec<Entry> = vec![
        (256, 4, vec![w]),
        (257, 4, vec![h]),
        (258, 3, bits.to_vec()),
        (259, 3, vec![1]),
        (262, 3, vec![photometric]),
        (273, 4, vec![DATA]),
        (277, 3, vec![bits.len() as u32]),
        (278, 4, vec![h]),
        (279, 4, vec![bytes]),
        (284, 3, vec![1]),
    ];
    for x in extra {
        e.retain(|y| y.0 != x.0);
        e.push(x.clone());
    }
    e.sort_by_key(|x| x.0);
    e
}

/// Decode on a thread with a deadline: `Some(result)`, or `None` when it panicked.
fn try_decode(name: &str, bytes: Vec<u8>) -> Option<Result<image::RgbaImage, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let r = decode(&bytes)
            .map(|i| i.into_rgba8())
            .map_err(|e| e.to_string());
        let _ = tx.send(r);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(20)) {
        Ok(r) => {
            let _ = handle.join();
            Some(r)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("{name}: decoding hangs"),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            let _ = handle.join();
            None
        }
    }
}

#[test]
fn hostile_tiffs_decode_or_fail_cleanly() {
    let px = |n: usize| (0..n).map(|i| (i * 37 % 251) as u8).collect::<Vec<u8>>();
    let rgb8 = image_ifd(4, 4, &[8, 8, 8], 2, 48, &[]);
    let mut cases: Vec<(&str, Vec<u8>)> = vec![
        ("rgb8", tiff(std::slice::from_ref(&rgb8), &px(48))),
        // Float RGB (HDR paint exports).
        (
            "float32",
            tiff(
                &[image_ifd(
                    2,
                    2,
                    &[32, 32, 32],
                    2,
                    48,
                    &[(339, 3, vec![3, 3, 3])],
                )],
                &[0.5f32, 1.0, 0.25]
                    .repeat(4)
                    .iter()
                    .flat_map(|f| f.to_le_bytes())
                    .collect::<Vec<u8>>(),
            ),
        ),
        // CMYK print files.
        (
            "cmyk",
            tiff(&[image_ifd(4, 4, &[8, 8, 8, 8], 5, 64, &[])], &px(64)),
        ),
        // Palette images.
        (
            "palette",
            tiff(
                &[image_ifd(
                    4,
                    4,
                    &[8],
                    3,
                    16,
                    &[(320, 3, (0..768).map(|i| (i * 85) as u32 & 0xffff).collect())],
                )],
                &px(16),
            ),
        ),
        // Two pages (the first is used).
        ("multipage", tiff(&[rgb8.clone(), rgb8.clone()], &px(48))),
        // Tiles instead of strips.
        (
            "tiled",
            tiff(
                &[{
                    let mut e = image_ifd(16, 16, &[8, 8, 8], 2, 768, &[]);
                    e.retain(|x| ![273, 278, 279].contains(&x.0));
                    e.extend([
                        (322, 3, vec![16]),
                        (323, 3, vec![16]),
                        (324, 4, vec![DATA]),
                        (325, 4, vec![768]),
                    ]);
                    e.sort_by_key(|x| x.0);
                    e
                }],
                &px(768),
            ),
        ),
        // PackBits: one literal run of 48 bytes.
        (
            "packbits",
            tiff(
                &[image_ifd(4, 4, &[8, 8, 8], 2, 49, &[(259, 3, vec![32773])])],
                &{
                    let mut d = vec![47u8];
                    d.extend(px(48));
                    d
                },
            ),
        ),
        // Compression schemes the decoder lacks (JPEG-in-TIFF, old JPEG).
        (
            "jpeg-compressed",
            tiff(
                &[image_ifd(4, 4, &[8, 8, 8], 2, 48, &[(259, 3, vec![6])])],
                &px(48),
            ),
        ),
        // Grey with a Photoshop alpha, and 16-bit RGB with one.
        (
            "grey+alpha",
            tiff(
                &[image_ifd(4, 4, &[8, 8], 1, 32, &[(338, 3, vec![0])])],
                &px(32),
            ),
        ),
        (
            "rgba16 unspecified",
            tiff(
                &[image_ifd(
                    2,
                    2,
                    &[16, 16, 16, 16],
                    2,
                    32,
                    &[(338, 3, vec![0])],
                )],
                &px(32),
            ),
        ),
        // Photoshop with two alpha channels: RGB + 2 unspecified extra samples.
        (
            "rgb + 2 extra",
            tiff(
                &[image_ifd(
                    2,
                    2,
                    &[8, 8, 8, 8, 8],
                    2,
                    20,
                    &[(338, 3, vec![0, 0])],
                )],
                &px(20),
            ),
        ),
        (
            "rgba associated",
            tiff(
                &[image_ifd(2, 2, &[8, 8, 8, 8], 2, 16, &[(338, 3, vec![1])])],
                &px(16),
            ),
        ),
        // Bilevel and 4-bit grey.
        ("1-bit", tiff(&[image_ifd(8, 2, &[1], 1, 2, &[])], &px(2))),
        ("4-bit", tiff(&[image_ifd(4, 2, &[4], 1, 4, &[])], &px(4))),
    ];
    // Broken files: every one must fail (or decode) without panicking or hanging.
    let broken: Vec<(&str, Vec<u8>)> = vec![
        // 65535 × 65535 RGBA8 (16 GB) with 48 bytes of data.
        (
            "huge",
            tiff(
                &[image_ifd(65535, 65535, &[8, 8, 8, 8], 2, 48, &[])],
                &px(48),
            ),
        ),
        (
            "huge strip",
            tiff(
                &[image_ifd(
                    65535,
                    65535,
                    &[8, 8, 8],
                    2,
                    u32::MAX,
                    &[(278, 4, vec![65535])],
                )],
                &px(48),
            ),
        ),
        // 4 billion pixels wide, one tall.
        (
            "wide",
            tiff(&[image_ifd(u32::MAX, 1, &[8], 1, 48, &[])], &px(48)),
        ),
        (
            "zero size",
            tiff(&[image_ifd(0, 0, &[8, 8, 8], 2, 0, &[])], &[]),
        ),
        ("strip past the end", {
            let mut b = tiff(std::slice::from_ref(&rgb8), &px(48));
            b.truncate(b.len() - 20);
            b
        }),
        (
            "strip offset far out",
            tiff(
                &[image_ifd(
                    4,
                    4,
                    &[8, 8, 8],
                    2,
                    48,
                    &[(273, 4, vec![0x7fff_ffff])],
                )],
                &px(48),
            ),
        ),
        ("header only", vec![b'I', b'I', 42, 0]),
        (
            "ifd past the end",
            vec![b'I', b'I', 42, 0, 0xff, 0xff, 0xff, 0x7f],
        ),
        ("ifd loop", {
            let mut b = tiff(std::slice::from_ref(&rgb8), &px(48));
            // Next-IFD offset of the only IFD points back at itself.
            let n = rgb8.len();
            let at = 8 + 2 + n * 12;
            b[at..at + 4].copy_from_slice(&8u32.to_le_bytes());
            b
        }),
        ("65535 entries", {
            let mut b = vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 0xff, 0xff];
            b.extend(px(600));
            b
        }),
        (
            "bits 0",
            tiff(&[image_ifd(4, 4, &[0, 0, 0], 2, 48, &[])], &px(48)),
        ),
        (
            "bits 64",
            tiff(&[image_ifd(4, 4, &[64, 64, 64], 2, 48, &[])], &px(48)),
        ),
        (
            "samples 0",
            tiff(
                &[image_ifd(4, 4, &[8, 8, 8], 2, 48, &[(277, 3, vec![0])])],
                &px(48),
            ),
        ),
        (
            "samples 200",
            tiff(
                &[image_ifd(4, 4, &[8, 8, 8], 2, 48, &[(277, 3, vec![200])])],
                &px(48),
            ),
        ),
        (
            "packbits overrun",
            tiff(
                &[image_ifd(4, 4, &[8, 8, 8], 2, 3, &[(259, 3, vec![32773])])],
                &[0x81, 7, 0x81],
            ),
        ),
        (
            "lzw garbage",
            tiff(
                &[image_ifd(4, 4, &[8, 8, 8], 2, 48, &[(259, 3, vec![5])])],
                &[0xff; 48],
            ),
        ),
        (
            "deflate garbage",
            tiff(
                &[image_ifd(4, 4, &[8, 8, 8], 2, 48, &[(259, 3, vec![8])])],
                &[0x78; 48],
            ),
        ),
        (
            "tile 0",
            tiff(
                &[{
                    let mut e = image_ifd(16, 16, &[8, 8, 8], 2, 768, &[]);
                    e.retain(|x| ![273, 278, 279].contains(&x.0));
                    e.extend([
                        (322, 3, vec![0]),
                        (323, 3, vec![0]),
                        (324, 4, vec![DATA]),
                        (325, 4, vec![768]),
                    ]);
                    e.sort_by_key(|x| x.0);
                    e
                }],
                &px(768),
            ),
        ),
        (
            "palette without a colour map",
            tiff(&[image_ifd(4, 4, &[8], 3, 16, &[])], &px(16)),
        ),
        (
            "extra samples 3 values",
            tiff(
                &[image_ifd(
                    4,
                    4,
                    &[8, 8, 8, 8],
                    2,
                    64,
                    &[(338, 3, vec![0, 0, 0])],
                )],
                &px(64),
            ),
        ),
    ];
    cases.extend(broken);
    // Every prefix of a good file too.
    let good = tiff(
        &[image_ifd(4, 4, &[8, 8, 8, 8], 2, 64, &[(338, 3, vec![0])])],
        &px(64),
    );
    let mut report = Vec::new();
    for (name, bytes) in &cases {
        let r = try_decode(name, bytes.clone());
        report.push(format!(
            "{name}: {:?}",
            r.as_ref().map(|r| r.as_ref().map(|i| i.dimensions()))
        ));
        assert!(r.is_some(), "{name}: decoding panicked");
    }
    for len in 0..good.len() {
        let r = try_decode("prefix", good[..len].to_vec());
        assert!(r.is_some(), "prefix of {len} bytes: decoding panicked");
        let _ = tiff_alpha(&good[..len]);
    }
    println!("{}", report.join("\n"));
    // The good ones decode.
    for name in [
        "rgb8",
        "multipage",
        "tiled",
        "packbits",
        "grey+alpha",
        "rgba16 unspecified",
    ] {
        let (_, bytes) = cases.iter().find(|c| c.0 == name).unwrap();
        assert!(decode(bytes).is_ok(), "{name}: {:?}", decode(bytes).err());
    }
}

/// Associated (premultiplied) alpha, ExtraSamples 1 (GIMP, some exporters): the colours come
/// back un-premultiplied, or cut-out edges and soft alpha draw too dark.
#[test]
fn tiff_associated_alpha_is_unpremultiplied() {
    let pixels = [
        50u8, 25, 0, 128, // half covered: (100, 50, 0) at half alpha
        200, 100, 50, 255, // opaque: as it is
        0, 0, 0, 0, // clear
        10, 20, 30, 64,
    ];
    for extra in [1u32, 2] {
        let bytes = tiff(
            &[image_ifd(
                2,
                2,
                &[8, 8, 8, 8],
                2,
                16,
                &[(338, 3, vec![extra])],
            )],
            &pixels,
        );
        let img = decode(&bytes).unwrap().into_rgba8();
        let got: Vec<[u8; 4]> = img.pixels().map(|p| p.0).collect();
        if extra == 2 {
            assert_eq!(img.as_raw().as_slice(), &pixels, "unassociated alpha stays");
            continue;
        }
        assert_eq!(got[0], [100, 50, 0, 128]);
        assert_eq!(got[1], [200, 100, 50, 255]);
        assert_eq!(got[2], [0, 0, 0, 0]);
        assert_eq!(got[3], [40, 80, 120, 64]);
    }
    // 16 bits a channel too.
    let deep: Vec<u8> = [0x4000u16, 0x2000, 0, 0x8000]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let bytes = tiff(
        &[image_ifd(
            1,
            1,
            &[16, 16, 16, 16],
            2,
            8,
            &[(338, 3, vec![1])],
        )],
        &deep,
    );
    let img = decode(&bytes).unwrap().into_rgba16();
    let p = img.get_pixel(0, 0).0;
    assert!(
        (i32::from(p[0]) - 0x7fff).abs() <= 2 && (i32::from(p[1]) - 0x3fff).abs() <= 2,
        "{p:?}"
    );
}

/// Photoshop saves a second alpha channel (or a spot colour) as another unspecified extra
/// sample: RGB + 2. The first extra channel is the alpha, the second is dropped.
#[test]
fn tiff_two_extra_channels_keep_the_first_as_alpha() {
    let pixels: Vec<u8> = vec![
        10, 20, 30, 0, 99, //
        40, 50, 60, 128, 99, //
        70, 80, 90, 255, 99, //
        1, 2, 3, 77, 99,
    ];
    let bytes = tiff(
        &[image_ifd(
            2,
            2,
            &[8, 8, 8, 8, 8],
            2,
            20,
            &[(338, 3, vec![0, 0])],
        )],
        &pixels,
    );
    let img = decode(&bytes).unwrap().into_rgba8();
    let got: Vec<[u8; 4]> = img.pixels().map(|p| p.0).collect();
    assert_eq!(
        got,
        [
            [10, 20, 30, 0],
            [40, 50, 60, 128],
            [70, 80, 90, 255],
            [1, 2, 3, 77]
        ]
    );
}

/// Greyscale TIFFs with an alpha channel (masks, gloss maps saved in Grayscale mode with
/// transparency) load: the image crate refuses grey + extra samples.
#[test]
fn tiff_grey_with_alpha_loads() {
    for extra in [0u32, 2] {
        let bytes = tiff(
            &[image_ifd(2, 1, &[8, 8], 1, 4, &[(338, 3, vec![extra])])],
            &[10, 0, 200, 255],
        );
        let img = decode(&bytes).unwrap().into_rgba8();
        assert_eq!(
            img.get_pixel(0, 0).0,
            [10, 10, 10, 0],
            "ExtraSamples {extra}"
        );
        // A grey leaf mask with clear texels is a cut-out; its size reads without a decode.
        assert!(crate::models::named_textures::bytes_have_cutouts(&bytes));
        let path = std::env::temp_dir().join(format!(
            "misty_grey_alpha_{extra}_{}.tif",
            std::process::id()
        ));
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(super::texture::image_size(&path), Some([2, 1]));
        std::fs::remove_file(path).ok();
        assert_eq!(img.get_pixel(1, 0).0, [200, 200, 200, 255]);
    }
    let deep: Vec<u8> = [0x1000u16, 0xffff]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let bytes = tiff(
        &[image_ifd(1, 1, &[16, 16], 1, 4, &[(338, 3, vec![2])])],
        &deep,
    );
    assert_eq!(
        decode(&bytes).unwrap().into_rgba8().get_pixel(0, 0).0,
        [16, 16, 16, 255]
    );
    // White is zero: inverted when it loads (the decoder may refuse it), never a panic.
    let bytes = tiff(
        &[image_ifd(1, 1, &[8, 8], 0, 2, &[(338, 3, vec![2])])],
        &[10, 255],
    );
    if let Ok(img) = decode(&bytes) {
        assert_eq!(img.into_rgba8().get_pixel(0, 0).0, [245, 245, 245, 255]);
    }
}

/// Timing of a pack-sized TIFF (4K RGBA with a Photoshop alpha): decode, the cut-out check
/// and the 2K converted copy extraction writes. Run by hand (`--ignored --nocapture`).
#[test]
#[ignore]
fn big_tiff_timing() {
    let (w, h) = (4096u32, 4096u32);
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for (i, p) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p.copy_from_slice(&[
            (i % 251) as u8,
            (i % 241) as u8,
            (i % 239) as u8,
            (i % 256) as u8,
        ]);
    }
    let bytes = tiff(
        &[image_ifd(
            w,
            h,
            &[8, 8, 8, 8],
            2,
            w * h * 4,
            &[(338, 3, vec![0])],
        )],
        &pixels,
    );
    let t = std::time::Instant::now();
    let img = decode(&bytes).unwrap().into_rgba8();
    let decoded = t.elapsed();
    let t = std::time::Instant::now();
    let cut = crate::models::named_textures::bytes_have_cutouts(&bytes);
    let cutouts = t.elapsed();
    let t = std::time::Instant::now();
    let small = super::texture::fit(img, 2048);
    let fitted = t.elapsed();
    println!(
        "4K TIFF: decode {decoded:?}, cut-out check {cutouts:?} ({cut}), 2K fit {fitted:?}, {}",
        small.width()
    );
}
