//! Build ▸ Asset Bundles: write each planned bundle (a pak archive with the group's
//! compression), hash it, and save the catalog next to them.
//!
//! ```text
//! Builds/Bundles/<platform>/catalog.json
//! Builds/Bundles/<platform>/<local bundles>
//! Builds/Bundles/<platform>/Remote/<remote bundles>   upload these to the group's URL
//! ```

use std::path::{Path, PathBuf};

use super::catalog::{self, CATALOG_FILE, Catalog, Duplicate, REMOTE_DIR};
use super::fetch::sha256_hex;
use super::settings::AssetGroupSettings;
use crate::pak::PakWriter;

/// What a bundle build produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BundleBuild {
    pub out_dir: PathBuf,
    pub catalog: Catalog,
    /// Missing references and shared files, as messages.
    pub warnings: Vec<String>,
    pub duplicates: Vec<Duplicate>,
}

/// `<project>/Builds/Bundles/<platform>`.
pub fn build_dir(project: &Path, platform: &str) -> PathBuf {
    project.join(catalog::BUILD_DIR).join(platform)
}

/// Build every included group's bundles for `platform` into `out_dir` (old bundles there are
/// removed first).
pub fn build_bundles(
    project: &Path,
    settings: &AssetGroupSettings,
    platform: &str,
    out_dir: &Path,
) -> Result<BundleBuild, String> {
    let plan = catalog::plan(project, settings, platform);
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let remote_dir = out_dir.join(REMOTE_DIR);
    for dir in [out_dir, remote_dir.as_path()] {
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        for f in std::fs::read_dir(dir).map_err(|e| io(dir, e))?.flatten() {
            let p = f.path();
            let stale = p.extension().is_some_and(|x| x == "bundle")
                || p.file_name().is_some_and(|n| n == CATALOG_FILE);
            if p.is_file() && stale {
                std::fs::remove_file(&p).map_err(|e| io(&p, e))?;
            }
        }
    }
    let mut catalog = plan.catalog.clone();
    for b in &mut catalog.bundles {
        let dir = if b.remote { &remote_dir } else { out_dir };
        let path = dir.join(&b.name);
        let file = std::fs::File::create(&path).map_err(|e| io(&path, e))?;
        let mut pak =
            PakWriter::with_compression(std::io::BufWriter::new(file), b.compression.pak())
                .map_err(|e| io(&path, e))?;
        for rel in &b.assets {
            let bytes = crate::vfs::read(project.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
            pak.add(rel, &bytes).map_err(|e| io(&path, e))?;
        }
        pak.finish().map_err(|e| io(&path, e))?;
        let bytes = std::fs::read(&path).map_err(|e| io(&path, e))?;
        b.hash = sha256_hex(&bytes);
        b.size = bytes.len() as u64;
    }
    let cat_path = out_dir.join(CATALOG_FILE);
    std::fs::write(&cat_path, catalog.to_json()).map_err(|e| io(&cat_path, e))?;

    let mut warnings: Vec<String> = plan
        .missing
        .iter()
        .map(|(m, from)| format!("{m} is missing (used by {from})"))
        .collect();
    let duplicates = plan.duplicates();
    for d in &duplicates {
        warnings.push(format!(
            "{} is packed into {} bundles: {}",
            d.path,
            d.bundles.len(),
            d.bundles.join(", ")
        ));
    }
    Ok(BundleBuild {
        out_dir: out_dir.to_path_buf(),
        catalog,
        warnings,
        duplicates,
    })
}

/// Copy the catalog and the local bundles of a bundle build into `dest` (a player's
/// `<data>/Bundles`). Returns the number of files copied.
pub fn copy_local(built: &Path, dest: &Path) -> Result<usize, String> {
    let catalog = Catalog::load(&built.join(CATALOG_FILE))?;
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::create_dir_all(dest).map_err(|e| io(dest, e))?;
    let mut n = 0;
    for b in catalog.bundles.iter().filter(|b| !b.remote) {
        let from = built.join(&b.name);
        std::fs::copy(&from, dest.join(&b.name)).map_err(|e| io(&from, e))?;
        n += 1;
    }
    std::fs::copy(built.join(CATALOG_FILE), dest.join(CATALOG_FILE))
        .map_err(|e| io(&built.join(CATALOG_FILE), e))?;
    Ok(n + 1)
}
