//! Lay out a build folder around a compiled player:
//!
//! ```text
//! <Out>/<Product>.exe            the player, renamed (icon embedded on Windows)
//! <Out>/<Product>_Data/data.pak  scenes in the build + what they use + Resources + settings
//! <Out>/<Product>_Data/icon.png  the icon (window icon on Linux, desktop files)
//! <Out>/steam_appid.txt          development builds with a Steam App ID
//! <Out>/steam/                   SteamPipe upload scripts (with a Steam App ID)
//! <Out>.zip / <Product>-<ver>-<platform>.zip   when "Create .zip" is on
//! <Out>/BuildReport.txt          sizes by asset type and the largest assets
//! ```
//!
//! A Build Profile ([`misty_engine::build_profile`]) decides the scenes, defines, texture
//! overrides, quality level and options. A Dedicated Server profile packs a headless player:
//! no textures, audio or video, no icon and no compressed textures.
//!
//! macOS builds are an app bundle: `<Product>.app/Contents/MacOS/<Product>` with the data folder
//! in `Contents/Resources`.

use std::path::{Path, PathBuf};

use misty_engine::asset_deps;
use misty_engine::build::{
    self, BUILD_INFO_FILE, BuildInfo, BuildSettings, BuildTarget, PAK_FILE, TargetOs,
};
use misty_engine::build_profile::{self, BuildProfile, TextureOverride};
use misty_engine::build_report::{self, BuildReport};
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
    /// The Build Profile (`build` already has its scenes, target and options); `None` builds
    /// with `build` alone.
    pub profile: Option<BuildProfile>,
}

impl PackageRequest {
    /// A build of `profile`: the shared Build Settings with the profile applied.
    pub fn with_profile(
        project: &Path,
        out: &Path,
        shared: &BuildSettings,
        settings: &ProjectSettings,
        profile: &BuildProfile,
    ) -> Self {
        Self {
            project: project.to_path_buf(),
            out: out.to_path_buf(),
            build: profile.apply(shared),
            settings: settings.clone(),
            profile: Some(profile.clone()),
        }
    }

    /// A dedicated server build.
    pub fn server(&self) -> bool {
        self.profile.as_ref().is_some_and(BuildProfile::is_server)
    }

    pub fn profile_name(&self) -> String {
        self.profile
            .as_ref()
            .map_or_else(|| self.build.target.label().to_owned(), |p| p.name.clone())
    }

    /// The scripting defines the build runs with.
    pub fn defines(&self) -> Vec<String> {
        match &self.profile {
            Some(p) => p.build_defines(),
            None => BuildProfile {
                target: self.build.target,
                development: self.build.development,
                ..Default::default()
            }
            .build_defines(),
        }
    }

    fn texture_override(&self) -> Option<TextureOverride> {
        self.profile
            .as_ref()
            .and_then(BuildProfile::texture_override)
    }

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
    /// Sizes by asset type and the largest assets.
    pub report: BuildReport,
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
    let started = std::time::Instant::now();
    let warnings = std::cell::Cell::new(0usize);
    let send = &mut |m: Msg| {
        if matches!(m, Msg::Warning(_)) {
            warnings.set(warnings.get() + 1);
        }
        send(m)
    };
    check_output(&req.project, &req.out)?;
    let target = req.build.target;
    let server = req.server();
    let mut report = BuildReport::default();
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
    // A missing build scene was only a warning: the build "succeeded" and the player failed
    // when it loaded the scene.
    if let Some(gone) = roots
        .iter()
        .find(|r| !req.project.join(r.as_str()).is_file())
    {
        return Err(format!(
            "the build scene {gone} is missing: restore it or remove it from Build Settings"
        ));
    }
    if !server {
        roots.extend(player_settings.icon.clone());
        roots.extend(player_settings.splash.image.clone());
    }
    let mut deps = asset_deps::collect(&req.project, &roots);
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
    // A dedicated server has nothing to show or play: textures, audio and video stay out.
    if server {
        let stripped: Vec<String> = deps
            .files
            .iter()
            .filter(|r| build_profile::server_strips(r))
            .cloned()
            .collect();
        for rel in &stripped {
            let bytes = std::fs::metadata(req.project.join(rel)).map_or(0, |m| m.len());
            report.strip(bytes);
            deps.files.remove(rel);
        }
        send(Msg::Log(format!(
            "Dedicated server: {} visual and audio files left out ({})",
            report.stripped,
            build_report::size_text(report.stripped_bytes)
        )));
    }
    // The profile's texture overrides go into the packed import settings, so players look up
    // the compressed blocks this build makes.
    let platform = texture_platform(target);
    let texture_override = req.texture_override().filter(|_| !server);
    let mut metas: std::collections::BTreeMap<String, String> = Default::default();
    if let Some(o) = &texture_override {
        for rel in deps
            .files
            .iter()
            .filter(|r| misty_engine::assets::is_texture(r))
        {
            let meta = format!("{rel}.meta");
            let old = misty_engine::vfs::read_to_string(req.project.join(&meta)).ok();
            match o.apply_meta(old.as_deref(), platform) {
                Ok(text) => {
                    metas.insert(meta, text);
                }
                Err(e) => send(Msg::Warning(format!("{meta}: {e}"))),
            }
        }
        send(Msg::Log(format!(
            "Texture overrides: compression {}, max size {} ({} textures)",
            o.compression.map_or("as imported", |c| c.label()),
            o.max_size.map_or("as imported".into(), |m| m.to_string()),
            metas.len()
        )));
        deps.files.extend(metas.keys().cloned());
    }

    // The pak: assets, then settings.
    let pak_path = data_dir.join(PAK_FILE);
    let file = std::fs::File::create(&pak_path).map_err(|e| io(&pak_path, e))?;
    let mut pak = PakWriter::new(std::io::BufWriter::new(file), req.build.compression)
        .map_err(|e| io(&pak_path, e))?;
    let total = deps.files.len().max(1);
    for (i, rel) in deps.files.iter().enumerate() {
        let bytes = match metas.get(rel) {
            Some(text) => text.clone().into_bytes(),
            None => {
                misty_engine::vfs::read(req.project.join(rel)).map_err(|e| format!("{rel}: {e}"))?
            }
        };
        let packed = pak.add_counted(rel, &bytes).map_err(|e| io(&pak_path, e))?;
        report.add(rel, bytes.len() as u64, packed);
        if i % 16 == 0 {
            let f = 0.76 + 0.14 * i as f32 / total as f32;
            send(Msg::Progress(f, format!("Packing {rel}")));
        }
    }
    // Virtual geometry cluster builds of the packed models (made now if the cache is missing),
    // so players load them instead of building at runtime.
    for rel in deps
        .files
        .iter()
        .filter(|r| !server && misty_engine::models::is_model(r))
    {
        match misty_engine::models::virtual_build::bake(rel) {
            Ok(Some(path)) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                let bytes = misty_engine::vfs::read(&path).map_err(|e| io(&path, e))?;
                let key = format!(
                    "{}/{}",
                    misty_engine::models::virtual_build::LIBRARY_DIR,
                    name.unwrap_or_default()
                );
                let packed = pak
                    .add_counted(&key, &bytes)
                    .map_err(|e| io(&pak_path, e))?;
                report.add(&key, bytes.len() as u64, packed);
                send(Msg::Log(format!("Misty Mesh: {key}")));
            }
            Ok(None) => {}
            Err(e) => send(Msg::Warning(format!("virtual geometry of {rel}: {e}"))),
        }
    }
    // Compressed textures (made now if the cache is missing them), at every size the quality
    // levels load them at, so players upload blocks instead of decoding images.
    let files: Vec<String> = deps.files.iter().cloned().collect();
    let (packed, saved) = if server {
        (0, 0)
    } else {
        pack_compressed_textures(req, &files, &mut pak, &mut report, send)
            .map_err(|e| io(&pak_path, e))?
    };
    if packed > 0 {
        send(Msg::Log(format!(
            "Compressed textures: {packed} ({:.1} MB of GPU memory saved)",
            saved as f64 / 1e6
        )));
    }
    let mut settings = req.settings.clone();
    // The player reads its settings from here, so migrate old projects' window options.
    settings.player = Some(player_settings.clone());
    // The profile's quality level is the one the game starts on.
    if let Some(p) = &req.profile
        && let Some(name) = &p.quality_level
    {
        match p.quality_index(&settings.quality) {
            Some(i) => {
                settings.quality.default_level = i;
                send(Msg::Log(format!("Quality level: {name}")));
            }
            None => send(Msg::Warning(format!(
                "the profile's quality level \"{name}\" doesn't exist; using the default"
            ))),
        }
    }
    let defines = req.defines();
    let info = BuildInfo {
        development: req.build.development,
        target: target.triple().into(),
        version: player_settings.version.clone(),
        server,
        profile: req
            .profile
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        defines: defines.clone(),
    };
    send(Msg::Log(format!(
        "Scripting defines: {}",
        defines.join(";")
    )));
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
        let text = text?;
        let packed = pak
            .add_counted(rel, text.as_bytes())
            .map_err(|e| io(&pak_path, e))?;
        report.add(rel, text.len() as u64, packed);
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
    if let Some(icon) = player_settings.icon.as_ref().filter(|_| !server) {
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
        let skip = |r: &str| {
            r == "steam_appid.txt"
                || r == "Player.log"
                || r == build_report::TEXT_FILE
                || r.starts_with("steam/")
        };
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
    // The report: shown by the window, saved next to the game and in the project.
    report.product = product.clone();
    report.profile = req.profile_name();
    report.target = target.label().to_owned();
    report.server = server;
    report.development = req.build.development;
    report.output = std::path::absolute(out).unwrap_or_else(|_| out.clone());
    report.time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    report.executable_bytes = std::fs::metadata(&exe).map_or(0, |m| m.len());
    report.pak_bytes = pak_bytes;
    let text_path = out.join(build_report::TEXT_FILE);
    let _ = std::fs::remove_file(&text_path);
    report.total_bytes = build_report::folder_bytes(out);
    report.seconds = started.elapsed().as_secs_f32();
    report.warnings = warnings.get();
    report.finish();
    if let Err(e) = std::fs::write(&text_path, report.to_text()) {
        send(Msg::Warning(format!("{}: {e}", text_path.display())));
    }
    let lib = BuildReport::library_path(&req.project, &report.profile);
    if let Err(e) = report.save(&lib) {
        send(Msg::Warning(format!("build report: {e}")));
    }
    send(Msg::Log(format!(
        "Build report: {} total, largest type {} ({})",
        build_report::size_text(report.total_bytes),
        report.categories.first().map_or("-", |c| c.name.as_str()),
        text_path.display()
    )));
    Ok(Built {
        exe,
        zip,
        files: deps.files.len(),
        pak_bytes,
        report,
    })
}

/// The texture import Platform name of a build target.
pub fn texture_platform(target: BuildTarget) -> &'static str {
    match target.os() {
        TargetOs::Windows => "Windows",
        TargetOs::Linux => "Linux",
        TargetOs::MacOs => "macOS",
    }
}

/// Pack the compressed blocks of the build's textures (`Library/TextureCache/<key>.mtc`, the
/// path players look them up at). Returns the entries packed and the GPU bytes they save over
/// RGBA8.
pub fn pack_compressed_textures<W: std::io::Write + std::io::Seek>(
    req: &PackageRequest,
    files: &[String],
    pak: &mut PakWriter<W>,
    report: &mut BuildReport,
    send: &mut dyn FnMut(Msg),
) -> std::io::Result<(usize, u64)> {
    use misty_engine::sprites::compress;
    let platform = texture_platform(req.build.target);
    let texture_override = req.texture_override();
    let textures: Vec<&String> = files
        .iter()
        .filter(|r| misty_engine::assets::is_texture(r))
        .collect();
    // How the packed materials sample each texture (a Default texture in a normal map slot is
    // compressed as a normal map).
    let mut uses: std::collections::HashMap<String, Vec<compress::Use>> = Default::default();
    for rel in files.iter().filter(|r| r.ends_with(".mat")) {
        let Ok(m) = misty_engine::assets::Material::load(&req.project.join(rel)) else {
            continue;
        };
        for (map, usage) in [
            (&m.albedo_map, compress::Use::Color),
            (&m.normal_map, compress::Use::Normal),
            (&m.metallic_map, compress::Use::Linear),
            (&m.occlusion_map, compress::Use::Linear),
            (&m.emission_map, compress::Use::Color),
            // Standard Lit v2 feature maps.
            (&m.features.detail.albedo_map, compress::Use::Color),
            (&m.features.detail.normal_map, compress::Use::Normal),
            (&m.features.detail.mask, compress::Use::Linear),
            (&m.features.parallax.height_map, compress::Use::Linear),
            (&m.features.clear_coat.normal_map, compress::Use::Normal),
            (
                &m.features.translucency.thickness_map,
                compress::Use::Linear,
            ),
        ] {
            if let Some(p) = map {
                uses.entry(p.clone()).or_default().push(usage);
            }
        }
    }
    let mut packed = std::collections::BTreeSet::new();
    let mut saved = 0u64;
    for (i, rel) in textures.iter().enumerate() {
        if i % 8 == 0 {
            let f = 0.90 + 0.01 * i as f32 / textures.len().max(1) as f32;
            send(Msg::Progress(f, format!("Compressing {rel}")));
        }
        let used = uses
            .get(rel.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut imp =
            misty_engine::sprites::import::TextureImporter::load(&req.project.join(rel.as_str()));
        if let Some(o) = &texture_override {
            imp = o.apply(&imp, platform);
        }
        let entries = match compress::bake_with(
            &req.project,
            rel,
            platform,
            &req.settings.quality,
            used,
            &imp,
        ) {
            Ok(b) => b.entries,
            Err(e) => {
                send(Msg::Warning(format!("texture compression of {rel}: {e}")));
                continue;
            }
        };
        for key in entries {
            if !packed.insert(key.clone()) {
                continue;
            }
            let bytes = std::fs::read(req.project.join(&key))?;
            if let Some(c) = compress::Compressed::from_file(&bytes) {
                let rgba = misty_engine::render::texture::texture_bytes(
                    misty_engine::wgpu::TextureFormat::Rgba8Unorm,
                    c.width,
                    c.height,
                    c.levels.len() as u32,
                    1,
                );
                saved += rgba.saturating_sub(c.bytes() as u64);
            }
            let stored = pak.add_counted(&key, &bytes)?;
            report.add(&key, bytes.len() as u64, stored);
        }
    }
    // The textures the renderer makes from several images (tree maps and impostors, terrain
    // layer arrays, grass and flower atlases), keyed by their packed pixels.
    send(Msg::Progress(
        0.91,
        "Compressing tree and terrain textures".into(),
    ));
    let (made, warnings) = misty_engine::render::bake_made_textures(&req.project, files, platform);
    for w in warnings {
        send(Msg::Warning(format!("texture compression: {w}")));
    }
    for key in made {
        if !packed.insert(key.clone()) {
            continue;
        }
        let bytes = std::fs::read(req.project.join(&key))?;
        if let Some(c) = compress::Compressed::from_file(&bytes) {
            let rgba = misty_engine::render::texture::texture_bytes(
                misty_engine::wgpu::TextureFormat::Rgba8Unorm,
                c.width,
                c.height,
                c.levels.len() as u32,
                1,
            );
            saved += rgba.saturating_sub(c.bytes() as u64);
        }
        let stored = pak.add_counted(&key, &bytes)?;
        report.add(&key, bytes.len() as u64, stored);
    }
    Ok((packed.len(), saved))
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
            profile: None,
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

    /// A build scene that no longer exists fails the build (the player would fail to load it).
    #[test]
    fn a_missing_build_scene_fails_the_build() {
        let root = project("missing_scene");
        let mut req = request(&root, BuildTarget::WindowsX64);
        req.out = root.with_extension("missing");
        req.build.create_zip = false;
        req.build.steam_app_id.clear();
        req.build
            .scenes
            .push(BuildScene::new("Assets/Scenes/Gone.scene"));
        let player = root.join("player.bin");
        std::fs::write(&player, "exe").unwrap();
        let err = package(&req, &player, &mut |_| {}).unwrap_err();
        assert!(err.contains("Gone.scene"), "{err}");
        let _ = std::fs::remove_dir_all(&req.out);
        let _ = std::fs::remove_dir_all(&root);
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

    /// Textures a build packs get their compressed blocks packed too, as each material samples
    /// them (a Default texture in a normal map slot as BC5), under the path players read.
    #[test]
    fn packs_compressed_textures_for_their_material_uses() {
        use misty_engine::sprites::compress::{BcFormat, Compressed};
        let root = project("bc");
        use misty_engine::image;
        let img = image::RgbaImage::from_fn(64, 64, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 4) as u8, 200, 255])
        });
        img.save(root.join("Assets/Wall.png")).unwrap();
        img.save(root.join("Assets/WallNormal.png")).unwrap();
        std::fs::write(
            root.join("Assets/Red.mat"),
            r#"{ "albedo_map": "Assets/Wall.png", "normal_map": "Assets/WallNormal.png" }"#,
        )
        .unwrap();
        let req = request(&root, BuildTarget::WindowsX64);
        let files: Vec<String> = ["Assets/Red.mat", "Assets/Wall.png", "Assets/WallNormal.png"]
            .map(str::to_owned)
            .to_vec();
        let mut pak = PakWriter::new(std::io::Cursor::new(Vec::new()), false).unwrap();
        let (packed, saved) =
            pack_compressed_textures(&req, &files, &mut pak, &mut Default::default(), &mut |_| {})
                .unwrap();
        assert_eq!(packed, 2);
        assert!(saved > 0);
        let pak = Pak::from_reader(Box::new(std::io::Cursor::new(
            pak.finish().unwrap().into_inner(),
        )))
        .unwrap();
        let mut formats: Vec<BcFormat> = pak
            .paths()
            .filter(|p| p.starts_with("Library/TextureCache/"))
            .map(|p| Compressed::from_file(&pak.read(p).unwrap()).unwrap().format)
            .collect();
        formats.sort_by_key(|f| f.label());
        assert_eq!(formats, [BcFormat::Bc1, BcFormat::Bc5]);
        std::fs::remove_dir_all(&root).ok();
    }
    /// A project with two scenes, a textured material, an audio clip and a script.
    fn profile_project(name: &str) -> PathBuf {
        let root = project(name);
        use misty_engine::image;
        let img =
            image::RgbaImage::from_fn(256, 256, |x, y| image::Rgba([x as u8, y as u8, 100, 255]));
        img.save(root.join("Assets/Wall.png")).unwrap();
        std::fs::write(
            root.join("Assets/Wall.png.meta"),
            "{\n  \"guid\": \"00112233445566778899aabbccddeeff\"\n}\n",
        )
        .unwrap();
        std::fs::write(root.join("Assets/Shot.wav"), vec![0u8; 4096]).unwrap();
        std::fs::write(
            root.join("Assets/Red.mat"),
            r#"{ "albedo_map": "Assets/Wall.png" }"#,
        )
        .unwrap();
        std::fs::write(
            root.join("Assets/Scenes/Demo.scene"),
            r#"{ "m": "Assets/Red.mat", "clip": "Assets/Shot.wav" }"#,
        )
        .unwrap();
        root
    }

    fn built_pak(out: &Path) -> Pak {
        Pak::open(&out.join("Game_Data/data.pak")).unwrap()
    }

    /// Every override of a profile reaches the build: its scene list, defines, texture
    /// compression and size, quality level and options; a report is saved.
    #[test]
    fn profile_overrides_reach_the_build_output() {
        use misty_engine::build_profile::ProfilePlatform;
        use misty_engine::sprites::compress::{BcFormat, Compressed};
        use misty_engine::sprites::import::{TextureCompression, TextureImporter};
        let root = profile_project("profile");
        let out = root.with_extension("profile_out");
        let _ = std::fs::remove_dir_all(&out);
        let shared = BuildSettings {
            scenes: vec![BuildScene::new("Assets/Scenes/A.scene")],
            product_name: "Game".into(),
            ..Default::default()
        };
        let mut settings = ProjectSettings::default();
        settings.quality.default_level = settings.quality.levels.len() - 1;
        let low = settings.quality.levels[0].name.clone();
        let mut profile = BuildProfile::for_platform(ProfilePlatform::Windows);
        profile.target = BuildTarget::WindowsX64;
        profile.name = "Demo".into();
        profile.custom = true;
        profile.override_scenes = true;
        profile.scenes = vec![BuildScene::new("Assets/Scenes/Demo.scene")];
        profile.defines = vec!["DEMO".into()];
        profile.texture_compression = Some(TextureCompression::HighQuality);
        profile.max_texture_size = Some(64);
        profile.quality_level = Some(low);
        profile.development = true;
        profile.compression = false;
        let req = PackageRequest::with_profile(&root, &out, &shared, &settings, &profile);
        let player = root.join("player.bin");
        std::fs::write(&player, "exe").unwrap();
        let built = package(&req, &player, &mut |_| {}).unwrap();
        let pak = built_pak(&out);
        let paths: Vec<&str> = pak.paths().collect();
        assert!(paths.contains(&"Assets/Scenes/Demo.scene"), "{paths:?}");
        assert!(
            !paths.contains(&"Assets/Scenes/A.scene"),
            "the shared list isn't built: {paths:?}"
        );
        let b: BuildSettings =
            serde_json::from_slice(&pak.read(build::BUILD_SETTINGS_FILE).unwrap()).unwrap();
        assert_eq!(b.scenes[0].path, "Assets/Scenes/Demo.scene");
        let info: BuildInfo = serde_json::from_slice(&pak.read(BUILD_INFO_FILE).unwrap()).unwrap();
        assert!(info.development && !info.server);
        assert_eq!(info.profile, "Demo");
        for d in ["DEMO", "DEVELOPMENT_BUILD", "MISTY_STANDALONE_WIN"] {
            assert!(info.defines.contains(&d.to_owned()), "{:?}", info.defines);
        }
        let s: ProjectSettings =
            serde_json::from_slice(&pak.read("ProjectSettings/ProjectSettings.json").unwrap())
                .unwrap();
        assert_eq!(s.quality.default_level, 0, "the profile's quality level");
        // The packed import settings carry the overrides (the GUID stays) ...
        let meta = String::from_utf8(pak.read("Assets/Wall.png.meta").unwrap()).unwrap();
        assert!(meta.contains("00112233445566778899aabbccddeeff"), "{meta}");
        let imp: TextureImporter = serde_json::from_str(&meta).unwrap();
        assert_eq!(
            imp.compression_for("Windows"),
            TextureCompression::HighQuality
        );
        assert_eq!(imp.max_size_for("Windows"), 64);
        // ... and the packed blocks match them: BC7 at 64 pixels at most.
        let blocks: Vec<Compressed> = paths
            .iter()
            .filter(|p| p.starts_with("Library/TextureCache/"))
            .map(|p| Compressed::from_file(&pak.read(p).unwrap()).unwrap())
            .collect();
        assert!(!blocks.is_empty());
        for c in &blocks {
            assert_eq!(c.format, BcFormat::Bc7);
            assert!(c.width <= 64 && c.height <= 64, "{}x{}", c.width, c.height);
        }
        // The source on disk is untouched.
        let disk = std::fs::read_to_string(root.join("Assets/Wall.png.meta")).unwrap();
        assert!(!disk.contains("HighQuality"));
        // Compress Data off.
        let e = pak.entry("Assets/Scenes/Demo.scene").unwrap();
        assert_eq!(e.stored, e.size);
        // The report.
        let r = &built.report;
        assert_eq!(r.profile, "Demo");
        assert!(r.packed_of("Compressed Textures") > 0);
        assert!(r.packed_of("Textures") > 0);
        assert!(r.largest.iter().any(|a| a.path == "Assets/Wall.png"));
        assert!(out.join("BuildReport.txt").is_file());
        let saved = BuildReport::load(&BuildReport::library_path(&root, "Demo")).unwrap();
        assert_eq!(&saved, r);
        std::fs::remove_dir_all(&out).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    /// A Dedicated Server build is marked as one and leaves textures, audio and compressed
    /// textures out.
    #[test]
    fn server_builds_strip_visuals_and_audio() {
        use misty_engine::build_profile::ProfilePlatform;
        let root = profile_project("server");
        let out = root.with_extension("server_out");
        let _ = std::fs::remove_dir_all(&out);
        let shared = BuildSettings {
            scenes: vec![BuildScene::new("Assets/Scenes/Demo.scene")],
            product_name: "Game".into(),
            ..Default::default()
        };
        let mut profile = BuildProfile::for_platform(ProfilePlatform::DedicatedServer);
        profile.target = BuildTarget::LinuxX64;
        let req = PackageRequest::with_profile(
            &root,
            &out,
            &shared,
            &ProjectSettings::default(),
            &profile,
        );
        assert!(req.server());
        let player = root.join("player.bin");
        std::fs::write(&player, "exe").unwrap();
        let built = package(&req, &player, &mut |_| {}).unwrap();
        assert_eq!(built.exe, out.join("Game"));
        let pak = built_pak(&out);
        let paths: Vec<&str> = pak.paths().collect();
        assert!(paths.contains(&"Assets/Red.mat"), "{paths:?}");
        for gone in ["Assets/Wall.png", "Assets/Wall.png.meta", "Assets/Shot.wav"] {
            assert!(!paths.contains(&gone), "{gone} in {paths:?}");
        }
        assert!(!paths.iter().any(|p| p.starts_with("Library/TextureCache/")));
        let info: BuildInfo = serde_json::from_slice(&pak.read(BUILD_INFO_FILE).unwrap()).unwrap();
        assert!(info.server);
        assert!(info.defines.contains(&"MISTY_SERVER".to_owned()));
        assert!(info.defines.contains(&"MISTY_STANDALONE_LINUX".to_owned()));
        assert_eq!(built.report.stripped, 3);
        assert!(built.report.server);
        assert!(
            built
                .report
                .to_text()
                .contains("Stripped for the server: 3 files")
        );
        std::fs::remove_dir_all(&out).ok();
        std::fs::remove_dir_all(&root).ok();
    }
}
