//! Built-in sprites made in code (GameObject ▸ 2D Object ▸ Sprites ▸ …): white shapes one unit
//! across, referenced as `Builtin/<Name>.png` (no file on disk).

use image::{Rgba, RgbaImage};

use super::import::{TextureImporter, TextureType};

pub const PREFIX: &str = "Builtin/";

/// Shape names, in menu order.
pub const SHAPES: [&str; 3] = ["Square", "Circle", "Triangle"];

/// Side length in pixels (also the pixels per unit, so each shape is 1×1 unit).
const SIZE: u32 = 128;

/// Reference of a built-in shape ("Builtin/Circle.png").
pub fn path(shape: &str) -> String {
    format!("{PREFIX}{shape}.png")
}

pub fn is_builtin(rel: &str) -> bool {
    rel.starts_with(PREFIX)
}

fn shape_of(rel: &str) -> Option<&'static str> {
    let name = rel.strip_prefix(PREFIX)?.strip_suffix(".png")?;
    SHAPES.into_iter().find(|s| *s == name)
}

/// Import settings of a built-in sprite (`None` for unknown names).
pub fn importer(rel: &str) -> Option<TextureImporter> {
    shape_of(rel)?;
    Some(TextureImporter {
        texture_type: TextureType::Sprite,
        pixels_per_unit: SIZE as f32,
        ..TextureImporter::sprite()
    })
}

/// Coverage of a pixel by a shape, 4×4 supersampled for smooth edges.
fn coverage(inside: impl Fn(f32, f32) -> bool, x: u32, y: u32) -> u8 {
    let mut n = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            // -1..1 with y up.
            let u = (x as f32 + (sx as f32 + 0.5) / 4.0) / SIZE as f32 * 2.0 - 1.0;
            let v = 1.0 - (y as f32 + (sy as f32 + 0.5) / 4.0) / SIZE as f32 * 2.0;
            n += inside(u, v) as u32;
        }
    }
    (n * 255 / 16) as u8
}

/// The image of a built-in sprite (`None` for unknown names).
pub fn image(rel: &str) -> Option<RgbaImage> {
    let inside: fn(f32, f32) -> bool = match shape_of(rel)? {
        "Square" => |_, _| true,
        "Circle" => |u, v| u * u + v * v <= 1.0,
        // Apex at the top centre, base along the bottom edge.
        _ => |u, v| v >= -1.0 && u.abs() <= (1.0 - v) * 0.5,
    };
    Some(RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        Rgba([255, 255, 255, coverage(inside, x, y)])
    }))
}

/// Pixel size of a built-in sprite image.
pub fn size(rel: &str) -> Option<[u32; 2]> {
    shape_of(rel).map(|_| [SIZE, SIZE])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_are_one_unit_and_shaped() {
        for s in SHAPES {
            let p = path(s);
            assert!(is_builtin(&p));
            let img = image(&p).unwrap();
            assert_eq!(img.dimensions(), (SIZE, SIZE));
            assert_eq!(importer(&p).unwrap().pixels_per_unit, SIZE as f32);
        }
        let circle = image(&path("Circle")).unwrap();
        assert_eq!(circle.get_pixel(64, 64)[3], 255);
        assert_eq!(
            circle.get_pixel(0, 0)[3],
            0,
            "corners are outside the circle"
        );
        let tri = image(&path("Triangle")).unwrap();
        assert_eq!(tri.get_pixel(64, 120)[3], 255, "base is filled");
        assert_eq!(tri.get_pixel(4, 4)[3], 0, "top corners are empty");
        assert!(image("Builtin/Nope.png").is_none() && importer("Assets/a.png").is_none());
    }
}
