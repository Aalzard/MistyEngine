use super::*;

fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty_profiles_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    root
}

#[test]
fn profiles_save_and_load_with_every_override() {
    let root = project("round_trip");
    let legacy = BuildSettings {
        target: BuildTarget::LinuxArm64,
        development: true,
        ..Default::default()
    };
    let mut set = BuildProfiles::load(&root, &legacy);
    assert!(set.warnings.is_empty(), "{:?}", set.warnings);
    let labels: Vec<&str> = set.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(labels, ["Windows", "Linux", "macOS", "Dedicated Server"]);
    // Platform profiles without files start from the old Build Settings options.
    let linux = set.find("linux").unwrap();
    assert_eq!(linux.target, BuildTarget::LinuxArm64);
    assert!(linux.development);
    assert!(set.find("server").unwrap().is_server());

    // A custom profile with every override.
    let i = set
        .add_custom(&root, "Demo", None, ProfilePlatform::Linux)
        .unwrap();
    {
        let p = &mut set.profiles[i];
        p.override_scenes = true;
        p.scenes = vec![BuildScene::new("Assets/Scenes/Demo.scene")];
        p.defines = vec![
            "DEMO".into(),
            " STEAM ".into(),
            "DEMO".into(),
            "bad name!".into(),
        ];
        p.texture_compression = Some(TextureCompression::HighQuality);
        p.max_texture_size = Some(500);
        p.quality_level = Some("Low".into());
        p.development = true;
        p.compression = false;
        p.create_zip = true;
        p.output_folder = "Builds/Demo".into();
        p.sanitize();
        p.save(&root).unwrap();
    }
    assert!(
        root.join("Assets/Settings/Build Profiles/Demo.buildprofile")
            .is_file()
    );
    let back = BuildProfiles::load(&root, &legacy);
    let demo = back.find("demo").unwrap();
    assert_eq!(demo, &set.profiles[i]);
    assert!(demo.custom);
    assert_eq!(demo.defines, ["DEMO", "STEAM", "badname"]);
    assert_eq!(demo.max_texture_size, Some(512), "snapped to a size choice");
    assert_eq!(demo.title(), "Demo (Linux)");
    assert_eq!(
        demo.output_path(&root),
        Some(root.join("Builds/Demo")),
        "relative to the project"
    );

    // Platform profiles are saved too.
    let mut win = back.find("Windows").unwrap().clone();
    win.defines = vec!["WIN_ONLY".into()];
    win.save(&root).unwrap();
    assert!(
        root.join("ProjectSettings/BuildProfiles/Windows.buildprofile")
            .is_file()
    );
    let again = BuildProfiles::load(&root, &legacy);
    assert_eq!(again.find("windows").unwrap().defines, ["WIN_ONLY"]);

    // Duplicate, rename and delete.
    let mut set = again;
    let demo = set.find("Demo").cloned();
    let d = set
        .add_custom(&root, "Demo", demo.as_ref(), ProfilePlatform::Windows)
        .unwrap();
    assert_eq!(set.profiles[d].name, "Demo 1");
    assert_eq!(set.profiles[d].platform, ProfilePlatform::Linux);
    let renamed = set.rename(&root, d, "Steam Deck").unwrap();
    assert_eq!(renamed, "Steam Deck");
    assert!(
        root.join("Assets/Settings/Build Profiles/Steam Deck.buildprofile")
            .is_file()
    );
    assert!(
        !root
            .join("Assets/Settings/Build Profiles/Demo 1.buildprofile")
            .exists()
    );
    assert!(
        set.rename(&root, 0, "Nope").is_err(),
        "platform profiles keep their name"
    );
    let names = |s: &BuildProfiles| -> Vec<String> {
        s.profiles
            .iter()
            .filter(|p| p.custom)
            .map(|p| p.name.clone())
            .collect()
    };
    assert_eq!(
        names(&BuildProfiles::load(&root, &legacy)),
        ["Demo", "Steam Deck"]
    );
    let at = set.index_of("Steam Deck").unwrap();
    set.delete(&root, at).unwrap();
    assert_eq!(names(&BuildProfiles::load(&root, &legacy)), ["Demo"]);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn applying_a_profile_overrides_the_shared_settings() {
    let shared = BuildSettings {
        scenes: vec![BuildScene::new("Assets/Main.scene")],
        steam_app_id: "480".into(),
        ..Default::default()
    };
    let mut p = BuildProfile::for_platform(ProfilePlatform::DedicatedServer);
    p.development = true;
    p.compression = false;
    let b = p.apply(&shared);
    assert_eq!(b.scenes, shared.scenes, "the shared list");
    assert!(p.platform.targets().contains(&b.target));
    assert!(b.development && !b.compression);
    assert_eq!(b.steam_app_id, "480");
    assert_eq!(b.active_profile, "Dedicated Server");
    p.override_scenes = true;
    p.scenes = vec![BuildScene::new("Assets/Server.scene")];
    assert_eq!(p.apply(&shared).scenes[0].path, "Assets/Server.scene");
    let defines = p.build_defines();
    assert!(defines.contains(&"MISTY_SERVER".to_owned()));
    assert!(defines.contains(&"DEVELOPMENT_BUILD".to_owned()));

    let q = crate::quality::QualitySettings::default();
    p.quality_level = Some(q.levels[0].name.clone());
    assert_eq!(p.quality_index(&q), Some(0));
    p.quality_level = Some("Nope".into());
    assert_eq!(p.quality_index(&q), None);
}

#[test]
fn texture_overrides_rewrite_the_platform_settings_and_keep_the_guid() {
    let o = TextureOverride {
        compression: Some(TextureCompression::HighQuality),
        max_size: Some(256),
    };
    let meta = r#"{ "guid": "0123456789abcdef0123456789abcdef", "max_size": 1024 }"#;
    let text = o.apply_meta(Some(meta), "Windows").unwrap();
    assert!(text.contains("0123456789abcdef0123456789abcdef"), "{text}");
    let imp: TextureImporter = serde_json::from_str(&text).unwrap();
    assert_eq!(
        imp.compression_for("Windows"),
        TextureCompression::HighQuality
    );
    assert_eq!(imp.max_size_for("Windows"), 256);
    assert_eq!(imp.max_size_for("Linux"), 1024, "other platforms unchanged");

    // A sprite (uncompressed) stays uncompressed; a smaller Max Size stays.
    let sprite = TextureImporter {
        max_size: 128,
        ..TextureImporter::sprite()
    };
    let s = o.apply(&sprite, "Windows");
    assert_eq!(s.compression_for("Windows"), TextureCompression::None);
    assert_eq!(s.max_size_for("Windows"), 128);
    // "None" turns compression off everywhere.
    let off = TextureOverride {
        compression: Some(TextureCompression::None),
        max_size: None,
    };
    let t = off.apply(&TextureImporter::default(), "Linux");
    assert_eq!(t.compression_for("Linux"), TextureCompression::None);
    // No meta yet: one is made.
    assert!(
        o.apply_meta(None, "Windows")
            .unwrap()
            .contains("HighQuality")
    );
}

#[test]
fn lua_blocks_follow_the_defines_and_keep_line_numbers() {
    let src = "local a = 1\n--#if DEMO\nlocal m = 3\n--#elif !STEAM\nlocal m = 10\n--#else\nlocal m = 99\n--#endif\nreturn m\n";
    fn lines(s: &str) -> Vec<&str> {
        s.lines().map(str::trim).collect()
    }
    let demo = preprocess_lua(src, &["DEMO".into()]);
    assert_eq!(demo.lines().count(), src.lines().count());
    assert!(lines(&demo).contains(&"local m = 3"));
    assert!(!demo.contains("m = 10") && !demo.contains("m = 99"));
    let plain = preprocess_lua(src, &[]);
    assert!(plain.contains("m = 10") && !plain.contains("m = 3\n"));
    let steam = preprocess_lua(src, &["STEAM".into()]);
    assert!(steam.contains("m = 99") && !steam.contains("m = 10"));
    // Nested blocks inside a skipped block stay skipped.
    let nested = "--#if A\n--#if B\nx\n--#else\ny\n--#endif\n--#endif\nz\n";
    assert_eq!(preprocess_lua(nested, &["B".into()]).trim(), "z");
    assert!(preprocess_lua(nested, &["A".into()]).contains('y'));
    assert!(eval_condition("A && !B || C", &["C".into()]));
    assert!(!eval_condition("A && !B", &["A".into(), "B".into()]));
    assert_eq!(preprocess_lua("no directives", &[]), "no directives");
}

#[test]
fn servers_strip_what_is_only_seen_or_heard() {
    assert!(server_strips("Assets/Wall.png"));
    assert!(server_strips("Assets/Wall.png.meta"));
    assert!(server_strips("Assets/Audio/Shot.wav"));
    assert!(server_strips("Assets/Sky.hdr"));
    assert!(!server_strips("Assets/Models/Level.glb"));
    assert!(!server_strips("Assets/Scenes/Net.scene"));
    assert!(!server_strips("Assets/Scripts/Server.lua"));
}
