//! A small `.zip` writer (deflate) for shipping a build to itch.io and similar stores.
//! Executables get Unix "rwx" permissions so Linux and macOS players can start them.

use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::DeflateEncoder;

struct Entry {
    name: String,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
    executable: bool,
}

/// Zip every file under `dir` (paths inside the zip start with `prefix/`), skipping files for
/// which `skip(relative path)` is true.
pub fn zip_dir(
    dir: &Path,
    out: &Path,
    prefix: &str,
    executable: impl Fn(&str) -> bool,
    skip: impl Fn(&str) -> bool,
) -> Result<u64, String> {
    let mut files = Vec::new();
    walk(dir, dir, &mut files).map_err(|e| e.to_string())?;
    files.sort();
    let mut data = Vec::new();
    let mut entries = Vec::new();
    let (time, date) = dos_now();
    for rel in files.iter().filter(|r| !skip(r)) {
        let bytes = std::fs::read(dir.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&bytes).map_err(|e| e.to_string())?;
        let packed = enc.finish().map_err(|e| e.to_string())?;
        let too_big = |n: usize| u32::try_from(n).map_err(|_| "zip: file over 4 GB".to_owned());
        let name = format!("{prefix}/{rel}");
        let entry = Entry {
            crc: crc32fast::hash(&bytes),
            compressed: too_big(packed.len())?,
            size: too_big(bytes.len())?,
            offset: too_big(data.len())?,
            executable: executable(rel),
            name,
        };
        // Local file header.
        data.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        header_fields(&mut data, &entry, time, date);
        data.extend_from_slice(entry.name.as_bytes());
        data.extend_from_slice(&packed);
        entries.push(entry);
    }
    let dir_start = data.len() as u32;
    for e in &entries {
        data.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        data.extend_from_slice(&(3u16 << 8 | 20).to_le_bytes()); // made by: Unix, zip 2.0
        header_fields(&mut data, e, time, date);
        data.extend_from_slice(&[0; 4]); // comment length, disk number
        data.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        let mode: u32 = if e.executable { 0o100755 } else { 0o100644 };
        data.extend_from_slice(&(mode << 16).to_le_bytes());
        data.extend_from_slice(&e.offset.to_le_bytes());
        data.extend_from_slice(e.name.as_bytes());
    }
    let dir_len = data.len() as u32 - dir_start;
    data.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]); // disk numbers
    let n = entries.len() as u16;
    data.extend_from_slice(&n.to_le_bytes());
    data.extend_from_slice(&n.to_le_bytes());
    data.extend_from_slice(&dir_len.to_le_bytes());
    data.extend_from_slice(&dir_start.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    std::fs::write(out, &data).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(data.len() as u64)
}

/// Version, flags, method, time, date, CRC, sizes, name length, extra length.
fn header_fields(out: &mut Vec<u8>, e: &Entry, time: u16, date: u16) {
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&(1u16 << 11).to_le_bytes()); // UTF-8 names
    out.extend_from_slice(&8u16.to_le_bytes()); // deflate
    out.extend_from_slice(&time.to_le_bytes());
    out.extend_from_slice(&date.to_le_bytes());
    out.extend_from_slice(&e.crc.to_le_bytes());
    out.extend_from_slice(&e.compressed.to_le_bytes());
    out.extend_from_slice(&e.size.to_le_bytes());
    out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            walk(root, &p, out)?;
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Current UTC time as MS-DOS time and date fields.
fn dos_now() -> (u16, u16) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let time = ((rem / 3600) << 11) | ((rem % 3600 / 60) << 5) | ((rem % 60) / 2);
    let date = ((year - 1980).clamp(0, 127) << 9) | (month << 5) | day;
    (time as u16, date as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_readable_archive() {
        let root = std::env::temp_dir().join(format!("misty_zip_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Game/Game_Data")).unwrap();
        std::fs::write(root.join("Game/Game"), b"exe").unwrap();
        std::fs::write(root.join("Game/Game_Data/data.pak"), b"x".repeat(1000)).unwrap();
        std::fs::write(root.join("Game/steam_appid.txt"), b"480").unwrap();
        let out = root.join("Game.zip");
        zip_dir(
            &root.join("Game"),
            &out,
            "Game",
            |r| r == "Game",
            |r| r == "steam_appid.txt",
        )
        .unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        let end = &bytes[bytes.len() - 22..];
        assert_eq!(&end[..4], b"PK\x05\x06");
        assert_eq!(
            u16::from_le_bytes([end[10], end[11]]),
            2,
            "steam_appid.txt skipped"
        );
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Game/Game_Data/data.pak"));
        assert!(bytes.len() < 600, "deflated");
        let (_, date) = dos_now();
        assert!(date >> 9 >= 45, "year 2025 or later");
    }
}
