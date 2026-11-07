//! Lay out a build folder around a compiled player:
//!
//! ```text
//! <Out>/<Product>.exe            the player, renamed (icon embedded on Windows)
//! <Out>/<Product>_Data/data.pak  scenes in the build + what they use + Resources + settings
//! <Out>/<Product>_Data/icon.png  the icon (window icon on Linux, desktop files)
//! <Out>/steam_appid.txt          development builds with a Steam App ID
//! <Out>/steam/                   SteamPipe upload scripts (with a Steam App ID)
//! <Out>.zip / <Product>-<ver>-<platform>.zip   when "Create .zip" is on
//! ```
//!
//! macOS builds are an app bundle: `<Product>.app/Contents/MacOS/<Product>` with the data folder
//! in `Contents/Resources`.

use std::path::{Path, PathBuf};

use misty_engine::asset_deps;
use misty_engine::build::{
    self, BUILD_INFO_FILE, BuildInfo, BuildSettings, BuildTarget, PAK_FILE, TargetOs,
};
use misty_engine::pak::PakWriter;
use misty_engine::settings::ProjectSettings;

use super::{Msg, steam, zip};

/// Everything a build needs, copied from the editor when the build starts.
#[derive(Clone, Debug)]
pub struct PackageRequest {
    pub project: PathBuf,
    pub out: PathBuf,
    pub build: BuildSettings,
    pub settings: ProjectSettings,
}

impl PackageRequest {
    pub fn product(&self) -> String {
        self.settings
            .player_or(&self.build)
            .product_name_or(&self.project)
    }
}

/// What a finished build produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Built {
    /// The executable to run.
    pub exe: PathBuf,
    pub zip: Option<PathBuf>,
    pub files: usize,
    pub pak_bytes: u64,
}

/// Refuse output folders inside the project's Assets / ProjectSettings (or the project itself).
pub fn check_output(project: &Path, out: &Path) -> Result<(), String> {
    let out = std::path::absolute(out).map_err(|e| e.to_string())?;
    let root = std::path::absolute(project).map_err(|e| e.to_string())?;
    let inside = ["Assets", "ProjectSettings", "Library"]
        .iter()
        .any(|d| out.starts_with(root.join(d)));
    if out == root || inside {
        return Err(
            "Choose a build folder outside the project (or in a Builds folder next to Assets)."
                .into(),
        );
    }
    Ok(())
}

/// Write the build folder for `player` (a compiled `misty_player` for `req.build.target`).
pub fn package(
    req: &PackageRequest,
    player: &Path,
    send: &mut dyn FnMut(Msg),
) -> Result<Built, String> {
    check_output(&req.project, &req.out)?;
    let target = req.build.target;
    let player_settings = req.settings.player_or(&req.build);
    let product = req.product();
    let stem = build::file_stem(&product);
    let out = &req.out;
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::create_dir_all(out).map_err(|e| io(out, e))?;

    // Executable and data folder locations.
    let (exe, data_dir) = if target.os() == TargetOs::MacOs {
        let contents = out.join(format!("{stem}.app")).join("Contents");
        (
            contents.join("MacOS").join(&stem),
            contents.join("Resources").join(build::data_folder(&stem)),
        )
    } else {
        (
            out.join(format!("{stem}{}", target.exe_suffix())),
            out.join(build::data_folder(&stem)),
        )
    };
    if data_dir.exists() {
        std::fs::remove_dir_all(&data_dir).map_err(|e| io(&data_dir, e))?;
    }
    std::fs::create_dir_all(&data_dir).map_err(|e| io(&data_dir, e))?;
    if let Some(dir) = exe.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    std::fs::copy(player, &exe).map_err(|e| io(&exe, e))?;
    send(Msg::Log(format!("Player: {}", exe.display())));

    // Assets: the build scenes and everything they reference.
    send(Msg::Progress(0.75, "Collecting assets".into()));
    let mut roots: Vec<String> = req.build.enabled().map(|s| s.path.clone()).collect();
    if roots.is_empty() {
        return Err("no scenes in Build Settings: add one with \"Add Open Scenes\"".into());
    }
    roots.extend(player_settings.icon.clone());
    roots.extend(player_settings.splash.image.clone());
    let deps = asset_deps::collect(&req.project, &roots);
    for (missing, from) in &deps.missing {
        let from = if from.is_empty() {
            "the build list"
        } else {
            from
        };
        send(Msg::Warning(format!(
            "{missing} is missing (used by {from})"
        )));
    }
    send(Msg::Log(format!(
        "{} asset files from {} scenes",
        deps.files.len(),
        req.build.enabled().count()
    )));

    // The pak: assets, then settings.
    let pak_path = data_dir.join(PAK_FILE);
    let file = std::fs::File::create(&pak_path).map_err(|e| io(&pak_path, e))?;
    let mut pak = PakWriter::new(std::io::BufWriter::new(file), req.build.compression)
        .map_err(|e| io(&pak_path, e))?;
    let total = deps.files.len().max(1);
    for (i, rel) in deps.files.iter().enumerate() {
        let bytes =
            misty_engine::vfs::read(req.project.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        pak.add(rel, &bytes).map_err(|e| io(&pak_path, e))?;
        if i % 16 == 0 {
            let f = 0.76 + 0.14 * i as f32 / total as f32;
            send(Msg::Progress(f, format!("Packing {rel}")));
        }
    }
    // Virtual geometry cluster builds of the packed models (made now if the cache is missing),
    // so players load them instead of building at runtime.
    for rel in deps.files.iter().filter(|r| misty_engine::models::is_model(r)) {
        match misty_engine::models::virtual_build::bake(rel) {
            Ok(Some(path)) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                let bytes = misty_engine::vfs::read(&path).map_err(|e| io(&path, e))?;
                let key = format!(
                    "{}/{}",
                    misty_engine::models::virtual_build::LIBRARY_DIR,
                    name.unwrap_or_default()
                );
                pak.add(&key, &bytes).map_err(|e| io(&pak_path, e))?;
                send(Msg::Log(format!("Virtual geometry: {key}")));
            }
            Ok(None) => {}
            Err(e) => send(Msg::Warning(format!("virtual geometry of {rel}: {e}"))),
        }
    }
    let mut settings = req.settings.clone();
    // The player reads its settings from here, so migrate old projects' window options.
    settings.player = Some(player_settings.clone());
    let info = BuildInfo {
        development: req.build.development,
        target: target.triple().into(),
        version: player_settings.version.clone(),
    };
    // GUID → path of the packed assets, so the player resolves references like the editor.
    let guids: std::collections::BTreeMap<String, String> =
        misty_engine::asset_db::GuidDatabase::read(&req.project)
            .to_map()
            .into_iter()
            .filter(|(_, p)| deps.files.contains(p))
            .collect();
    for (rel, text) in [
        (misty_engine::settings::SETTINGS_FILE, json(&settings)),
        (build::BUILD_SETTINGS_FILE, json(&req.build)),
        (BUILD_INFO_FILE, json(&info)),
        (misty_engine::asset_db::MAP_FILE, json(&guids)),
    ] {
        pak.add(rel, text?.as_bytes())
            .map_err(|e| io(&pak_path, e))?;
    }
    pak.finish().map_err(|e| io(&pak_path, e))?;
    let pak_bytes = std::fs::metadata(&pak_path).map_or(0, |m| m.len());
    send(Msg::Log(format!(
        "{PAK_FILE}: {:.1} MB{}",
        pak_bytes as f64 / 1e6,
        if req.build.compression { " (LZ4)" } else { "" }
    )));

    // Asset bundles: build the addressable groups and ship the local bundles + catalog.
    bundles(req, &data_dir, send)?;

    // Icon: next to the data, and inside the .exe on Windows.
    send(Msg::Progress(0.92, "Icon".into()));
    if let Some(icon) = &player_settings.icon {
        match misty_engine::vfs::read(req.project.join(icon)) {
            Ok(bytes) => icon_files(&bytes, &data_dir, &exe, target, send),
            Err(e) => send(Msg::Warning(format!("icon {icon}: {e}"))),
        }
    }
    if target.os() == TargetOs::MacOs {
        let plist = info_plist(
            &product,
            &stem,
            &player_settings.version,
            &player_settings.company_name,
        );
        let path = exe
            .parent()
            .and_then(Path::parent)
            .map(|c| c.join("Info.plist"));
        if let Some(p) = path {
            std::fs::write(&p, plist).map_err(|e| io(&p, e))?;
        }
    }

    // Steam.
    let app_id = req.build.steam_app_id.trim();
    let appid_txt = out.join("steam_appid.txt");
    if !app_id.is_empty() {
        if req.build.development {
            std::fs::write(&appid_txt, app_id).map_err(|e| io(&appid_txt, e))?;
        } else if appid_txt.exists() {
            // A release must not ship the development file.
            std::fs::remove_file(&appid_txt).map_err(|e| io(&appid_txt, e))?;
        }
        write_steam(out, app_id, &product, &player_settings.version, &exe)?;
        send(Msg::Log(
            "Steam upload scripts: steam/README_STEAM.md".into(),
        ));
    }

    // Zip next to the build folder.
    let zip = if req.build.create_zip {
        send(Msg::Progress(0.95, "Zipping".into()));
        let name = format!(
            "{stem}-{}-{}.zip",
            build::file_stem(&player_settings.version),
            target.slug()
        );
        let path = out.parent().unwrap_or(out).join(name);
        let exe_rel = exe
            .strip_prefix(out)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let skip = |r: &str| r == "steam_appid.txt" || r == "Player.log" || r.starts_with("steam/");
        let size = zip::zip_dir(out, &path, &stem, |r| r == exe_rel, skip)?;
        send(Msg::Log(format!(
            "{}: {:.1} MB",
            path.display(),
            size as f64 / 1e6
        )));
        Some(path)
    } else {
        None
    };
    Ok(Built {
        exe,
        zip,
        files: deps.files.len(),
        pak_bytes,
    })
}

/// JSON text of a settings value (no BOM, trailing newline).
/// Build the project's asset bundles for the target (when it has addressables) and copy the
/// catalog and local bundles into `<data>/Bundles`.
fn bundles(req: &PackageRequest, data_dir: &Path, send: &mut dyn FnMut(Msg)) -> Result<(), String> {
    use misty_engine::asset_groups::{AssetGroupSettings, build, catalog};
    let settings = AssetGroupSettings::load(&req.project)?;
    if settings.entries().next().is_none() {
        return Ok(());
    }
    send(Msg::Progress(0.91, "Asset bundles".into()));
    let platform = req.build.target.slug();
    let out = build::build_dir(&req.project, platform);
    let built = build::build_bundles(&req.project, &settings, platform, &out)?;
    for w in &built.warnings {
        send(Msg::Warning(format!("Asset bundles: {w}")));
    }
    let n = build::copy_local(&out, &data_dir.join(catalog::PLAYER_DIR))?;
    let remote = built.catalog.bundles.iter().filter(|b| b.remote).count();
    send(Msg::Log(format!(
        "Asset bundles: {} local files copied{}",
        n,
        if remote > 0 {
            format!(
                "; upload {remote} remote bundles from {}/{}",
                out.display(),
                catalog::REMOTE_DIR
            )
        } else {
            String::new()
        }
    )));
    Ok(())
}

fn json<T: serde::Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string_pretty(v)
        .map(|t| t + "\n")
        .map_err(|e| e.to_string())
}

fn icon_files(
    png: &[u8],
    data_dir: &Path,
    exe: &Path,
    target: BuildTarget,
    send: &mut dyn FnMut(Msg),
) {
    let path = data_dir.join("icon.png");
    if let Err(e) = std::fs::write(&path, png) {
        send(Msg::Warning(format!("{}: {e}", path.display())));
    }
    if target.os() != TargetOs::Windows {
        return;
    }
    match super::icon::images(png) {
        Ok(images) => {
            let ico = data_dir.join("icon.ico");
            if let Err(e) = std::fs::write(&ico, super::icon::ico_file(&images)) {
                send(Msg::Warning(format!("{}: {e}", ico.display())));
            }
            match super::icon::embed(exe, &images) {
                Ok(()) => send(Msg::Log("Icon embedded in the executable".into())),
                Err(e) => send(Msg::Warning(e)),
            }
        }
        Err(e) => send(Msg::Warning(e)),
    }
}

fn write_steam(
    out: &Path,
    app_id: &str,
    product: &str,
    version: &str,
    exe: &Path,
) -> Result<(), String> {
    let dir = out.join("steam");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let content = std::path::absolute(out).map_err(|e| e.to_string())?;
    let exe_rel = exe
        .strip_prefix(out)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let depot = steam::depot_id(app_id);
    let files = [
        (
            format!("app_build_{app_id}.vdf"),
            steam::app_build_vdf(app_id, &format!("{product} {version}"), &content),
        ),
        (
            format!("depot_build_{depot}.vdf"),
            steam::depot_build_vdf(app_id, &content),
        ),
        (
            "README_STEAM.md".to_owned(),
            steam::readme(app_id, product, &exe_rel),
        ),
    ];
    for (name, text) in files {
        let p = dir.join(name);
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

fn info_plist(product: &str, exe: &str, version: &str, company: &str) -> String {
    let id: String = format!("{company}.{product}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>{product}</string>
	<key>CFBundleExecutable</key><string>{exe}</string>
	<key>CFBundleIdentifier</key><string>com.{id}</string>
	<key>CFBundleShortVersionString</key><string>{version}</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use misty_engine::build::BuildScene;
    use misty_engine::pak::Pak;

    use super::*;

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("misty_pkg_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |rel: &str, text: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        w("Assets/Scenes/A.scene", r#"{ "m": "Assets/Red.mat" }"#);
        w("Assets/Red.mat", "{}");
        w("Assets/Unused.mat", "{}");
        root
    }

    fn request(root: &Path, target: BuildTarget) -> PackageRequest {
        PackageRequest {
            project: root.to_path_buf(),
            out: root.with_extension("out"),
            build: BuildSettings {
                scenes: vec![BuildScene::new("Assets/Scenes/A.scene")],
                product_name: "My: Game".into(),
                target,
                development: true,
                create_zip: true,
                steam_app_id: "480".into(),
                ..Default::default()
            },
            settings: ProjectSettings::default(),
        }
    }

    #[test]
    fn lays_out_exe_data_pak_steam_and_zip() {
        let root = project("layout");
        let req = request(&root, BuildTarget::WindowsX64);
        let _ = std::fs::remove_dir_all(&req.out);
        let player = root.join("player.bin");
        std::fs::write(&player, "exe").unwrap();
        let mut log = Vec::new();
        let built = package(&req, &player, &mut |m| log.push(m)).unwrap();
        let out = &req.out;
        assert_eq!(built.exe, out.join("My_ Game.exe"));
        assert!(built.exe.is_file());
        let pak = Pak::open(&out.join("My_ Game_Data/data.pak")).unwrap();
        let paths: Vec<&str> = pak.paths().collect();
        assert_eq!(
            paths,
            [
                "AssetGuids.json",
                "Assets/Red.mat",
                "Assets/Scenes/A.scene",
                "ProjectSettings/BuildInfo.json",
                "ProjectSettings/BuildSettings.json",
                "ProjectSettings/ProjectSettings.json",
            ]
        );
        let info: BuildInfo = serde_json::from_slice(&pak.read(BUILD_INFO_FILE).unwrap()).unwrap();
        assert!(info.development);
        // Old projects' window options are migrated into the packed Player settings.
        let s: ProjectSettings =
            serde_json::from_slice(&pak.read("ProjectSettings/ProjectSettings.json").unwrap())
                .unwrap();
        assert_eq!(s.player.unwrap().product_name, "My: Game");
        assert_eq!(
            std::fs::read_to_string(out.join("steam_appid.txt")).unwrap(),
            "480"
        );
        assert!(out.join("steam/app_build_480.vdf").is_file());
        assert!(out.join("steam/depot_build_481.vdf").is_file());
        assert!(out.join("steam/README_STEAM.md").is_file());
        assert_eq!(
            built.zip.as_deref(),
            Some(
                out.parent()
                    .unwrap()
                    .join("My_ Game-1.0-windows-x64.zip")
                    .as_path()
            )
        );

        // A release build drops steam_appid.txt.
        let mut release = req.clone();
        release.build.development = false;
        release.build.create_zip = false;
        package(&release, &player, &mut |_| {}).unwrap();
        assert!(!out.join("steam_appid.txt").exists());

        // Never into the project.
        let mut inside = req.clone();
        inside.out = root.join("Assets/Build");
        assert!(package(&inside, &player, &mut |_| {}).is_err());
    }

    #[test]
    fn mac_builds_are_app_bundles() {
        let root = project("mac");
        let mut req = request(&root, BuildTarget::MacArm64);
        req.out = root.with_extension("mac");
        req.build.create_zip = false;
        req.build.steam_app_id.clear();
        let _ = std::fs::remove_dir_all(&req.out);
        let player = root.join("player.bin");
        std::fs::write(&player, "exe").unwrap();
        let built = package(&req, &player, &mut |_| {}).unwrap();
        let contents = req.out.join("My_ Game.app/Contents");
        assert_eq!(built.exe, contents.join("MacOS/My_ Game"));
        assert!(contents.join("Resources/My_ Game_Data/data.pak").is_file());
        assert!(contents.join("Info.plist").is_file());
    }
}
