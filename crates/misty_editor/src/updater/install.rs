//! Installing a downloaded package in place: checksum check, extraction to a staging folder and
//! the file swap with rollback. The running `.exe` can't be overwritten on Windows but can be
//! renamed, so every replaced `.exe` becomes `<name>.old.exe` (deleted on the next start) and
//! every other replaced file is moved to a backup folder until the swap has succeeded. On Linux
//! the package is a `.tar.gz` (executable bits kept) and the running binary, which has no
//! extension, goes to the backup folder like any other file (Linux allows renaming and removing
//! a running executable).

use std::path::{Path, PathBuf};

use super::Phase;
use super::http::{self, Source};
use super::release::Update;

/// Replaced files wait here until the swap succeeds (removed afterwards / on the next start).
pub const BACKUP_DIR: &str = ".misty-update-backup";
/// The editor executable every package for this platform must contain.
pub const PACKAGE_EXE: &str = if cfg!(windows) {
    "MistyEngine.exe"
} else {
    "MistyEngine"
};
/// Existing files in this top-level folder are kept (the user may have edited the examples).
const KEEP_EXISTING: &str = "Examples";

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The hash in a checksum file: `<hex>` or `<hex>  <file name>` (sha256sum / certutil style).
pub fn parse_checksum(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.len() == 64 && w.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
}

/// Refuse a package whose hash doesn't match the release's checksum file.
pub fn verify(actual: &str, expected: &str) -> Result<(), String> {
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(
            "The download is damaged (its SHA-256 checksum doesn't match the release). \
             Nothing was changed."
                .into(),
        )
    }
}

/// Whether a package name is a gzipped tarball (the Linux package) rather than a zip.
pub fn is_tar_gz(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".tar.gz") || n.ends_with(".tgz")
}

/// Unpack a `.zip` or `.tar.gz` package into `dest`; returns the package root (a single
/// top-level folder is skipped).
pub fn extract(package: &Path, dest: &Path) -> Result<PathBuf, String> {
    let name = package.file_name().unwrap_or_default().to_string_lossy();
    if is_tar_gz(&name) {
        extract_tar_gz(package, dest)?;
    } else {
        extract_zip(package, dest)?;
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(dest)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    match entries.as_slice() {
        [only] if only.is_dir() => Ok(only.clone()),
        _ => Ok(dest.to_path_buf()),
    }
}

fn extract_zip(zip: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("bad package: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("bad package: {e}"))?;
        // `enclosed_name` rejects absolute paths and `..` (no writing outside `dest`).
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| format!("unsafe path in package: {}", entry.name()))?;
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        std::io::copy(&mut entry, &mut f).map_err(|e| format!("bad package: {e}"))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(())
}

/// Unpack a gzipped tarball: regular files and folders only (no links), no absolute or `..`
/// paths; file modes are kept on Unix (the executables stay executable).
fn extract_tar_gz(tar_gz: &Path, dest: &Path) -> Result<(), String> {
    use std::path::Component;
    let file = std::fs::File::open(tar_gz).map_err(|e| format!("{}: {e}", tar_gz.display()))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let bad = |e: std::io::Error| format!("bad package: {e}");
    for entry in archive.entries().map_err(bad)? {
        let mut entry = entry.map_err(bad)?;
        let rel = entry.path().map_err(bad)?.into_owned();
        let unsafe_path = rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir));
        if unsafe_path {
            return Err(format!("unsafe path in package: {}", rel.display()));
        }
        match entry.header().entry_type() {
            tar::EntryType::Regular | tar::EntryType::Directory => {}
            // GNU long-name and PAX records are read by the tar crate itself.
            tar::EntryType::XGlobalHeader => continue,
            other => {
                return Err(format!(
                    "unsupported entry in package ({other:?}): {}",
                    rel.display()
                ));
            }
        }
        if !entry.unpack_in(dest).map_err(bad)? {
            return Err(format!("unsafe path in package: {}", rel.display()));
        }
    }
    Ok(())
}

/// Whether files can be created in `dir` (e.g. not under Program Files without admin rights).
pub fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".misty-write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// `Foo.exe` → `Foo.old.exe`.
pub fn old_exe_path(exe: &Path) -> PathBuf {
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    exe.with_file_name(format!("{stem}.old.exe"))
}

/// Delete what an earlier update left behind (`*.old.exe`, the backup folder). Returns true
/// when nothing is left (an old exe may still be running for a moment after a restart).
pub fn cleanup_old(dir: &Path) -> bool {
    let mut clean = true;
    let backup = dir.join(BACKUP_DIR);
    if backup.exists() && std::fs::remove_dir_all(&backup).is_err() {
        clean = false;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".old.exe") && std::fs::remove_file(e.path()).is_err() {
                clean = false;
            }
        }
    }
    clean
}

/// One undoable step of the swap.
enum Step {
    /// An existing file moved aside (`from` → `to`).
    Moved { from: PathBuf, to: PathBuf },
    /// A new file placed.
    Placed(PathBuf),
    /// A folder created.
    Dir(PathBuf),
}

/// Swap the package's files into `install_dir`; returns the exe to relaunch.
/// `running_exe` names the exe in use (its name is preferred if the package has it).
pub fn apply(package: &Path, install_dir: &Path, running_exe: &Path) -> Result<PathBuf, String> {
    apply_inner(package, install_dir, running_exe, None)
}

/// [`apply`] with a simulated failure before file `fail_at` (rollback tests).
pub fn apply_inner(
    package: &Path,
    install_dir: &Path,
    running_exe: &Path,
    fail_at: Option<usize>,
) -> Result<PathBuf, String> {
    let mut files = Vec::new();
    list_files(package, Path::new(""), &mut files).map_err(|e| e.to_string())?;
    let exe = pick_exe(&files, running_exe)?;
    // The exe goes last, so a failure on any other file leaves the old program untouched.
    files.sort_by_key(|f| (f == &exe, f.clone()));
    let backup = install_dir.join(BACKUP_DIR);
    let _ = std::fs::remove_dir_all(&backup);
    let mut journal = Vec::new();
    let result = swap(package, install_dir, &backup, &files, fail_at, &mut journal);
    if let Err(e) = result {
        rollback(journal);
        let _ = std::fs::remove_dir_all(&backup);
        return Err(format!(
            "{e} The update was rolled back; nothing was changed."
        ));
    }
    let _ = std::fs::remove_dir_all(&backup);
    Ok(install_dir.join(exe))
}

fn swap(
    package: &Path,
    install_dir: &Path,
    backup: &Path,
    files: &[PathBuf],
    fail_at: Option<usize>,
    journal: &mut Vec<Step>,
) -> Result<(), String> {
    let err = |p: &Path, e: std::io::Error| format!("Could not update {}: {e}.", p.display());
    for (i, rel) in files.iter().enumerate() {
        if fail_at == Some(i) {
            return Err("Simulated failure.".into());
        }
        let dest = install_dir.join(rel);
        let keep = rel
            .components()
            .next()
            .is_some_and(|c| c.as_os_str().eq_ignore_ascii_case(KEEP_EXISTING));
        if keep && dest.exists() {
            continue;
        }
        if dest.is_dir() {
            return Err(format!("{} is a folder in the install.", dest.display()));
        }
        if dest.exists() {
            let is_exe = rel
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("exe"));
            let aside = if is_exe {
                old_exe_path(&dest)
            } else {
                backup.join(rel)
            };
            if aside.exists() {
                std::fs::remove_file(&aside).map_err(|e| err(&aside, e))?;
            }
            make_dirs(aside.parent(), journal).map_err(|e| err(&aside, e))?;
            std::fs::rename(&dest, &aside).map_err(|e| err(&dest, e))?;
            journal.push(Step::Moved {
                from: dest.clone(),
                to: aside,
            });
        }
        make_dirs(dest.parent(), journal).map_err(|e| err(&dest, e))?;
        place(&package.join(rel), &dest).map_err(|e| err(&dest, e))?;
        journal.push(Step::Placed(dest));
    }
    Ok(())
}

/// Undo the swap, newest step first.
fn rollback(journal: Vec<Step>) {
    for step in journal.into_iter().rev() {
        match step {
            Step::Placed(p) => {
                let _ = std::fs::remove_file(p);
            }
            Step::Moved { from, to } => {
                let _ = std::fs::rename(to, from);
            }
            Step::Dir(d) => {
                let _ = std::fs::remove_dir(d);
            }
        }
    }
}

/// Create `dir` and missing parents, journaling each new folder.
fn make_dirs(dir: Option<&Path>, journal: &mut Vec<Step>) -> std::io::Result<()> {
    let Some(dir) = dir else { return Ok(()) };
    let missing: Vec<&Path> = dir.ancestors().take_while(|a| !a.exists()).collect();
    for d in missing.into_iter().rev() {
        std::fs::create_dir(d)?;
        journal.push(Step::Dir(d.to_path_buf()));
    }
    Ok(())
}

/// Move a staged file into place (copy when the staging folder is on another drive).
fn place(src: &Path, dest: &Path) -> std::io::Result<()> {
    if std::fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dest)?;
    Ok(())
}

fn list_files(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(root.join(rel))? {
        let e = e?;
        let r = rel.join(e.file_name());
        if e.file_type()?.is_dir() {
            list_files(root, &r, out)?;
        } else {
            out.push(r);
        }
    }
    Ok(())
}

/// The package's top-level exe named like the running one, else `MistyEngine.exe`.
fn pick_exe(files: &[PathBuf], running_exe: &Path) -> Result<PathBuf, String> {
    let top = |name: &std::ffi::OsStr| {
        files
            .iter()
            .find(|f| f.parent() == Some(Path::new("")) && f.as_os_str().eq_ignore_ascii_case(name))
            .cloned()
    };
    running_exe
        .file_name()
        .and_then(top)
        .or_else(|| top(PACKAGE_EXE.as_ref()))
        .ok_or_else(|| format!("The package has no {PACKAGE_EXE}; nothing was changed."))
}

/// Everything after the user clicked Update: in-place install or a reason to download by hand.
pub enum Outcome {
    /// Installed; relaunch this exe.
    Installed(PathBuf),
    /// Can't install in place (read-only folder, no package or checksum in the release).
    Manual(String),
}

/// Download, verify, extract and swap. `work` is a scratch folder (removed afterwards).
pub fn run(
    src: &Source,
    update: &Update,
    install_dir: &Path,
    running_exe: &Path,
    work: &Path,
    mut phase: impl FnMut(Phase),
) -> Result<Outcome, String> {
    if !writable(install_dir) {
        return Ok(Outcome::Manual(format!(
            "Misty Engine is installed in a folder it can't change ({}). \
             Download the new version and install it yourself.",
            install_dir.display()
        )));
    }
    let (Some(zip), Some(sum_url)) = (&update.zip, &update.checksum_url) else {
        let why = match super::release::Platform::HOST {
            Some(p) => format!(
                "This release has no {} package with a checksum file to install automatically.",
                p.label()
            ),
            None => "Automatic updates are not available for this system.".into(),
        };
        return Ok(Outcome::Manual(why));
    };
    let expected = parse_checksum(&http::fetch_text(src, sum_url)?)
        .ok_or("The release's checksum file has no SHA-256 hash.")?;
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    let result = (|| {
        let zip_path = work.join(if is_tar_gz(&zip.name) {
            "package.tar.gz"
        } else {
            "package.zip"
        });
        let actual = http::download(src, &zip.url, &zip_path, |got, total| {
            phase(Phase::Downloading {
                received: got,
                total: total.or((zip.size > 0).then_some(zip.size)),
            })
        })?;
        phase(Phase::Verifying);
        verify(&actual, &expected)?;
        phase(Phase::Installing);
        let root = extract(&zip_path, &work.join("files"))?;
        apply(&root, install_dir, running_exe).map(Outcome::Installed)
    })();
    let _ = std::fs::remove_dir_all(work);
    result
}
