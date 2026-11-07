//! Platform-tolerant comparisons for the `*_matches_its_generator` tests.
//!
//! The sample files in `SampleProject` were generated on Windows. Linux's libm `sin`, `cos`,
//! `pow`… can differ from Windows' in the last bit, so a regenerated float may print or encode
//! one ulp away. These helpers compare everything exactly except floats, which must agree to
//! [`REL`] relative / [`ABS`] absolute tolerance — far below any change a person would make, far
//! above libm noise. On the platform that wrote the files they behave like plain equality.
//!
//! - [`assert_json_close`]: two JSON texts (scene, prefab, table…). The text around the numbers
//!   (keys, order, strings, formatting) must be identical; integers must be equal; floats close.
//! - [`assert_value_close`]: two `serde_json::Value`s.
//! - [`assert_close`]: two values of any `Serialize` type (materials, clips…), via their JSON.
//! - [`assert_image_close`]: two RGBA images generated from float math: same size, each channel
//!   within one step (a rounding flip), and only a few such flips.
//! - [`assert_glb_close`]: two binary glTF files: JSON chunk as above, binary chunk byte-exact
//!   except the elements of FLOAT accessors (positions, normals, weights, keyframes…).

use serde::Serialize;
use serde_json::Value;
use std::fmt::Display;

/// Relative float tolerance.
pub const REL: f64 = 1e-5;
/// Absolute float tolerance (for values near zero).
pub const ABS: f64 = 1e-6;

/// At most this many differences are listed in a failure message.
const MAX_REPORTED: usize = 12;

/// Whether two floats agree within the tolerance.
pub fn floats_close(a: f64, b: f64) -> bool {
    if a == b || (a.is_nan() && b.is_nan()) {
        return true;
    }
    if !a.is_finite() || !b.is_finite() {
        return false;
    }
    (a - b).abs() <= ABS + REL * a.abs().max(b.abs())
}

/// Asserts that two JSON texts match: identical text around the numbers, equal integers, close
/// floats. Line endings are normalised first.
#[track_caller]
pub fn assert_json_close(actual: &str, expected: &str, what: impl Display) {
    if let Err(e) = json_text_diff(actual, expected) {
        panic!("{what}: {e}");
    }
}

/// Asserts that two JSON values match: same structure, equal strings / bools / integers, close
/// floats.
#[track_caller]
pub fn assert_value_close(actual: &Value, expected: &Value, what: impl Display) {
    let diffs = value_diffs(actual, expected);
    if !diffs.is_empty() {
        panic!("{what}: {}", report(&diffs));
    }
}

/// Asserts that two values of a `Serialize` type match, compared through their JSON form.
#[track_caller]
pub fn assert_close<T: Serialize + ?Sized>(actual: &T, expected: &T, what: impl Display) {
    let a = serde_json::to_value(actual).expect("serialize actual");
    let b = serde_json::to_value(expected).expect("serialize expected");
    assert_value_close(&a, &b, what);
}

/// Asserts that two binary glTF files match: same header and chunk layout, JSON chunk as in
/// [`assert_json_close`], binary chunk byte-exact except FLOAT accessor elements, which must be
/// close.
#[track_caller]
pub fn assert_glb_close(actual: &[u8], expected: &[u8], what: impl Display) {
    if let Err(e) = glb_diff(actual, expected) {
        panic!("{what}: {e}");
    }
}

/// At most this fraction of an image's channels may be one step off.
const IMAGE_FLIPS: f64 = 0.01;

/// Asserts that two RGBA images match: same size, every channel equal or one step off (a float
/// that rounded the other way), with at most 1% of the channels off.
#[track_caller]
pub fn assert_image_close(
    actual: &misty_engine::image::RgbaImage,
    expected: &misty_engine::image::RgbaImage,
    what: impl Display,
) {
    if let Err(e) = image_diff(actual, expected) {
        panic!("{what}: {e}");
    }
}

fn image_diff(
    a: &misty_engine::image::RgbaImage,
    b: &misty_engine::image::RgbaImage,
) -> Result<(), String> {
    if a.dimensions() != b.dimensions() {
        return Err(format!("size {:?} != {:?}", a.dimensions(), b.dimensions()));
    }
    let (ra, rb) = (a.as_raw(), b.as_raw());
    let mut flips = 0;
    for (i, (x, y)) in ra.iter().zip(rb).enumerate() {
        match x.abs_diff(*y) {
            0 => {}
            1 => flips += 1,
            d => {
                let (w, px) = (a.width() as usize, i / 4);
                return Err(format!(
                    "pixel ({}, {}) channel {}: {x} != {y} (off by {d})",
                    px % w,
                    px / w,
                    i % 4
                ));
            }
        }
    }
    if flips as f64 > IMAGE_FLIPS * ra.len() as f64 {
        return Err(format!("{flips} of {} channels off by one", ra.len()));
    }
    Ok(())
}

// ---- JSON text ---------------------------------------------------------------------------------

/// The text with every number token outside strings replaced by `#`, plus those tokens.
fn split_numbers(text: &str) -> (String, Vec<&str>) {
    let bytes = text.as_bytes();
    let mut skeleton = String::with_capacity(text.len());
    let mut numbers = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    let mut start = 0; // start of the pending plain-text run
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        let prev_is_word = i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        let starts_number = (c.is_ascii_digit()
            || (c == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)))
            && !prev_is_word;
        if !starts_number {
            i += 1;
            continue;
        }
        let begin = i;
        i += 1;
        while i < bytes.len()
            && (bytes[i].is_ascii_digit() || matches!(bytes[i], b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            i += 1;
        }
        skeleton.push_str(&text[start..begin]);
        skeleton.push('#');
        numbers.push(&text[begin..i]);
        start = i;
    }
    skeleton.push_str(&text[start..]);
    (skeleton, numbers)
}

fn is_integer_token(t: &str) -> bool {
    !t.contains(['.', 'e', 'E'])
}

/// Two number tokens match: integers exactly, otherwise as close floats.
fn numbers_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if is_integer_token(a) && is_integer_token(b) {
        return false;
    }
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => floats_close(x, y),
        _ => false,
    }
}

fn json_text_diff(actual: &str, expected: &str) -> Result<(), String> {
    let actual = actual.replace("\r\n", "\n");
    let expected = expected.replace("\r\n", "\n");
    if actual == expected {
        return Ok(());
    }
    let (sa, na) = split_numbers(&actual);
    let (sb, nb) = split_numbers(&expected);
    if sa != sb {
        let line = sa
            .lines()
            .zip(sb.lines())
            .position(|(x, y)| x != y)
            .unwrap_or_else(|| sa.lines().count().min(sb.lines().count()));
        let show = |s: &str| s.lines().nth(line).unwrap_or("<end>").trim().to_owned();
        return Err(format!(
            "text differs beyond float noise at line {} (numbers shown as #):\n  actual:   {}\n  expected: {}",
            line + 1,
            show(&sa),
            show(&sb)
        ));
    }
    // Same skeleton, so the same number of tokens in the same places.
    let diffs: Vec<String> = na
        .iter()
        .zip(&nb)
        .enumerate()
        .filter(|(_, (a, b))| !numbers_match(a, b))
        .map(|(k, (a, b))| format!("number #{k}: {a} != {b}"))
        .collect();
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(report(&diffs))
    }
}

// ---- JSON values -------------------------------------------------------------------------------

fn value_diffs(a: &Value, b: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(&mut String::new(), a, b, &mut out);
    out
}

fn walk(path: &mut String, a: &Value, b: &Value, out: &mut Vec<String>) {
    let at = |p: &str| {
        if p.is_empty() {
            "$".to_owned()
        } else {
            p.to_owned()
        }
    };
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let ok = if x.is_f64() || y.is_f64() {
                floats_close(
                    x.as_f64().unwrap_or(f64::NAN),
                    y.as_f64().unwrap_or(f64::NAN),
                )
            } else {
                x == y
            };
            if !ok {
                out.push(format!("{}: {x} != {y}", at(path)));
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{}: {} items != {}", at(path), x.len(), y.len()));
                return;
            }
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                let len = path.len();
                path.push_str(&format!("[{i}]"));
                walk(path, p, q, out);
                path.truncate(len);
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().filter(|k| !y.contains_key(*k)) {
                out.push(format!("{}.{k}: only in actual", at(path)));
            }
            for k in y.keys().filter(|k| !x.contains_key(*k)) {
                out.push(format!("{}.{k}: only in expected", at(path)));
            }
            for (k, p) in x {
                if let Some(q) = y.get(k) {
                    let len = path.len();
                    path.push('.');
                    path.push_str(k);
                    walk(path, p, q, out);
                    path.truncate(len);
                }
            }
        }
        _ => {
            if a != b {
                out.push(format!("{}: {} != {}", at(path), short(a), short(b)));
            }
        }
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 80 {
        format!("{}…", s.chars().take(80).collect::<String>())
    } else {
        s
    }
}

fn report(diffs: &[String]) -> String {
    let mut s = format!("{} difference(s) beyond float tolerance", diffs.len());
    for d in diffs.iter().take(MAX_REPORTED) {
        s.push_str("\n  ");
        s.push_str(d);
    }
    if diffs.len() > MAX_REPORTED {
        s.push_str("\n  …");
    }
    s
}

// ---- GLB ---------------------------------------------------------------------------------------

const GLB_MAGIC: u32 = 0x4654_6C67; // "glTF"
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;
const FLOAT: u64 = 5126;

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| format!("truncated at byte {at}"))
}

/// A GLB chunk: its type and payload.
type Chunk<'a> = (u32, &'a [u8]);

/// The GLB version and chunks, after checking its 12-byte header.
fn glb_chunks(b: &[u8]) -> Result<(u32, Vec<Chunk<'_>>), String> {
    if u32_at(b, 0)? != GLB_MAGIC {
        return Err("not a GLB file".into());
    }
    let version = u32_at(b, 4)?;
    if u32_at(b, 8)? as usize != b.len() {
        return Err("GLB length field doesn't match the file size".into());
    }
    let mut chunks = Vec::new();
    let mut at = 12;
    while at < b.len() {
        let len = u32_at(b, at)? as usize;
        let kind = u32_at(b, at + 4)?;
        let data = b
            .get(at + 8..at + 8 + len)
            .ok_or_else(|| format!("chunk at byte {at} runs past the end"))?;
        chunks.push((kind, data));
        at += 8 + len;
    }
    Ok((version, chunks))
}

/// One flag per byte of the binary chunk: true where a FLOAT accessor element lives.
fn float_bytes(gltf: &Value, bin_len: usize) -> Result<Vec<bool>, String> {
    let mut mask = vec![false; bin_len];
    let views = gltf["bufferViews"].as_array().cloned().unwrap_or_default();
    for (i, acc) in gltf["accessors"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        if acc["componentType"].as_u64() != Some(FLOAT) {
            continue;
        }
        let Some(view) = acc["bufferView"]
            .as_u64()
            .and_then(|v| views.get(v as usize))
        else {
            continue; // sparse / zero-filled: nothing in the buffer
        };
        if view["buffer"].as_u64().unwrap_or(0) != 0 {
            return Err(format!(
                "accessor {i}: only buffer 0 (the BIN chunk) is supported"
            ));
        }
        let components = match acc["type"].as_str() {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") | Some("MAT2") => 4,
            Some("MAT3") => 9,
            Some("MAT4") => 16,
            t => return Err(format!("accessor {i}: unknown type {t:?}")),
        };
        let element = components * 4;
        let stride = view["byteStride"].as_u64().map_or(element, |s| s as usize);
        let base = view["byteOffset"].as_u64().unwrap_or(0) as usize
            + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
        for e in 0..acc["count"].as_u64().unwrap_or(0) as usize {
            let start = base + e * stride;
            let end = start + element;
            if end > bin_len {
                return Err(format!("accessor {i} runs past the binary chunk"));
            }
            mask[start..end].iter_mut().for_each(|m| *m = true);
        }
    }
    Ok(mask)
}

fn glb_diff(actual: &[u8], expected: &[u8]) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    let (va, ca) = glb_chunks(actual).map_err(|e| format!("actual: {e}"))?;
    let (vb, cb) = glb_chunks(expected).map_err(|e| format!("expected: {e}"))?;
    if va != vb {
        return Err(format!("GLB version {va} != {vb}"));
    }
    let kinds = |c: &[Chunk]| c.iter().map(|(k, _)| *k).collect::<Vec<_>>();
    if kinds(&ca) != kinds(&cb) {
        return Err(format!(
            "chunk layout {:x?} != {:x?}",
            kinds(&ca),
            kinds(&cb)
        ));
    }
    let mut gltf = Value::Null;
    for ((kind, a), (_, b)) in ca.iter().zip(&cb) {
        match *kind {
            CHUNK_JSON => {
                let text = |d: &[u8]| -> Result<String, String> {
                    std::str::from_utf8(d)
                        .map(|s| s.trim_end_matches([' ', '\0']).to_owned())
                        .map_err(|e| format!("JSON chunk: {e}"))
                };
                let (ta, tb) = (text(a)?, text(b)?);
                json_text_diff(&ta, &tb).map_err(|e| format!("JSON chunk: {e}"))?;
                gltf = serde_json::from_str(&tb).map_err(|e| format!("JSON chunk: {e}"))?;
            }
            CHUNK_BIN => {
                if a.len() != b.len() {
                    return Err(format!("BIN chunk: {} bytes != {}", a.len(), b.len()));
                }
                let mask = float_bytes(&gltf, b.len())?;
                let mut diffs = Vec::new();
                let mut i = 0;
                while i < b.len() {
                    if mask[i] {
                        // FLOAT elements are 4-byte aligned runs of f32.
                        let x = f32::from_le_bytes(a[i..i + 4].try_into().unwrap());
                        let y = f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
                        if !floats_close(x as f64, y as f64) {
                            diffs.push(format!("BIN float at byte {i}: {x} != {y}"));
                        }
                        i += 4;
                    } else {
                        if a[i] != b[i] {
                            diffs.push(format!("BIN byte {i}: {} != {}", a[i], b[i]));
                        }
                        i += 1;
                    }
                }
                if !diffs.is_empty() {
                    return Err(report(&diffs));
                }
            }
            _ => {
                if a != b {
                    return Err(format!("chunk {kind:x} differs"));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SCENE: &str = "{\n  \"name\": \"Cube 0,1\",\n  \"count\": 12,\n  \"position\": [1.0, -2.5, 0.30000001],\n  \"rotation\": [0.0, 0.70710677, 0.0, 0.70710677]\n}";

    fn json_fails(a: &str, b: &str) -> bool {
        json_text_diff(a, b).is_err()
    }

    #[test]
    fn float_noise_passes() {
        let noisy = SCENE
            .replace("0.70710677", "0.7071068")
            .replace("0.30000001", "0.3")
            .replace("\n", "\r\n");
        assert_json_close(&noisy, SCENE, "noise");
        assert!(floats_close(1e-9, -1e-9));
        assert!(floats_close(1000.0, 1000.0001));
    }

    #[test]
    fn a_real_change_fails() {
        // A float moved by a person-sized amount.
        assert!(json_fails(&SCENE.replace("-2.5", "-2.51"), SCENE));
        assert!(json_fails(&SCENE.replace("0.30000001", "0.3001"), SCENE));
        // An integer off by one.
        assert!(json_fails(&SCENE.replace("12", "13"), SCENE));
        // A string, a key, an extra item, formatting.
        assert!(json_fails(&SCENE.replace("Cube 0,1", "Cube 0,2"), SCENE));
        assert!(json_fails(&SCENE.replace("\"count\"", "\"counts\""), SCENE));
        assert!(json_fails(
            &SCENE.replace("0.30000001]", "0.30000001, 1.0]"),
            SCENE
        ));
        assert!(json_fails(&SCENE.replace("  ", "    "), SCENE));
        let result = std::panic::catch_unwind(|| {
            assert_json_close(&SCENE.replace("-2.5", "-2.6"), SCENE, "moved");
        });
        assert!(result.is_err());
    }

    #[test]
    fn values_and_typed_structs() {
        let a = json!({"c": [1.0, 2.0], "n": 3, "s": "x"});
        assert!(value_diffs(&a, &json!({"c": [1.0000001, 2.0], "n": 3, "s": "x"})).is_empty());
        assert!(!value_diffs(&a, &json!({"c": [1.01, 2.0], "n": 3, "s": "x"})).is_empty());
        assert!(!value_diffs(&a, &json!({"c": [1.0, 2.0], "n": 4, "s": "x"})).is_empty());
        assert!(!value_diffs(&a, &json!({"c": [1.0, 2.0], "n": 3, "s": "y"})).is_empty());
        assert!(!value_diffs(&a, &json!({"c": [1.0], "n": 3, "s": "x"})).is_empty());
        assert!(!value_diffs(&a, &json!({"c": [1.0, 2.0], "n": 3})).is_empty());

        let m = misty_engine::assets::Material::default();
        let mut noisy = m.clone();
        noisy.smoothness += 1e-7;
        assert_close(&noisy, &m, "material noise");
        let mut changed = m.clone();
        changed.smoothness += 0.01;
        let result = std::panic::catch_unwind(|| assert_close(&changed, &m, "material change"));
        assert!(result.is_err());
    }

    #[test]
    fn image_rounding_passes_and_real_changes_fail() {
        use misty_engine::image::{Rgba, RgbaImage};
        let base = RgbaImage::from_fn(16, 16, |x, y| Rgba([x as u8 * 10, y as u8 * 10, 128, 255]));
        let mut flip = base.clone();
        flip.put_pixel(3, 4, Rgba([31, 40, 128, 255]));
        assert_image_close(&flip, &base, "one flip");
        let mut changed = base.clone();
        changed.put_pixel(3, 4, Rgba([32, 40, 128, 255]));
        assert!(image_diff(&changed, &base).is_err(), "off by two");
        let mut brighter = base.clone();
        brighter.pixels_mut().for_each(|p| p[2] = 129);
        assert!(
            image_diff(&brighter, &base).is_err(),
            "every pixel one step brighter"
        );
        assert!(image_diff(&RgbaImage::new(16, 8), &base).is_err(), "size");
    }

    /// A one-triangle GLB: FLOAT positions plus u16 indices.
    fn glb(positions: [f32; 9], indices: [u16; 3]) -> Vec<u8> {
        let mut bin: Vec<u8> = positions.iter().flat_map(|f| f.to_le_bytes()).collect();
        bin.extend(indices.iter().flat_map(|i| i.to_le_bytes()));
        bin.extend([0, 0]);
        let json = json!({
            "asset": {"version": "2.0"},
            "buffers": [{"byteLength": bin.len()}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 36},
                {"buffer": 0, "byteOffset": 36, "byteLength": 6}
            ],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                 "max": [positions[0], 1.0, 0.0], "min": [0.0, 0.0, 0.0]},
                {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
            ]
        })
        .to_string();
        let mut json = json.into_bytes();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut out = Vec::new();
        out.extend(GLB_MAGIC.to_le_bytes());
        out.extend(2u32.to_le_bytes());
        out.extend(((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend((json.len() as u32).to_le_bytes());
        out.extend(CHUNK_JSON.to_le_bytes());
        out.extend(json);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(CHUNK_BIN.to_le_bytes());
        out.extend(bin);
        out
    }

    #[test]
    fn glb_noise_passes_and_real_changes_fail() {
        let p = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let base = glb(p, [0, 1, 2]);
        assert_glb_close(&base, &base, "same");
        let mut noisy = p;
        noisy[0] = f32::from_bits(1.0f32.to_bits() + 1); // one ulp, also in "max"
        noisy[4] = f32::from_bits(1.0f32.to_bits() - 1);
        assert_glb_close(&glb(noisy, [0, 1, 2]), &base, "one ulp");
        let mut moved = p;
        moved[4] = 1.01;
        assert!(glb_diff(&glb(moved, [0, 1, 2]), &base).is_err());
        assert!(glb_diff(&glb(p, [0, 2, 1]), &base).is_err(), "index order");
    }
}
