// Shared by the misty_editor and misty_player build scripts (`include!`d): embeds the Misty
// Engine icon and version info into Windows executables through a resource script.
//
// The resource uses the neutral language so a game build's own icon (written by the editor's
// build pipeline with the neutral language too) replaces it instead of adding a second one.

/// Compile `<ico>` into the package's binaries as icon resource 1, with the version info
/// (Windows targets only). `original_filename` is the name cargo gives the exe.
///
/// The release signing checks these values (tools/release/signpath/): ProductName must stay
/// "Misty Engine" and ProductVersion the workspace version.
fn embed_windows_icon(ico: &std::path::Path, description: &str, original_filename: &str) {
    println!("cargo:rerun-if-changed={}", ico.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Ok(ico) = ico.canonicalize() else {
        println!("cargo:warning=exe icon not embedded: {} is missing", ico.display());
        return;
    };
    // rc.exe does not understand the `\\?\` prefix canonicalize adds; backslashes are escapes.
    let ico = ico.display().to_string();
    let ico = ico.trim_start_matches(r"\\?\").replace('\\', r"\\");
    let version = env!("CARGO_PKG_VERSION");
    let numbers: Vec<&str> = version.split(['.', '-']).take(3).collect();
    let numeric = format!("{},0", numbers.join(","));
    let internal_name = original_filename.trim_end_matches(".exe");
    let rc = format!(
        r#"#pragma code_page(65001)
LANGUAGE 0, 0
1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
{{
BLOCK "StringFileInfo"
{{
BLOCK "000004b0"
{{
VALUE "CompanyName", "Misty Engine"
VALUE "FileDescription", "{description}"
VALUE "ProductName", "Misty Engine"
VALUE "FileVersion", "{version}"
VALUE "ProductVersion", "{version}"
VALUE "InternalName", "{internal_name}"
VALUE "OriginalFilename", "{original_filename}"
VALUE "LegalCopyright", "Copyright (C) Misty Engine contributors. GPL-3.0-or-later with runtime exception."
}}
}}
BLOCK "VarFileInfo"
{{
VALUE "Translation", 0x0, 0x04b0
}}
}}
"#
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("misty.rc");
    std::fs::write(&out, rc).expect("write misty.rc");
    let mut res = winresource::WindowsResource::new();
    res.set_resource_file(out.to_str().unwrap());
    if let Err(e) = res.compile() {
        // A missing Windows SDK only loses the exe icon; the window icon is set at runtime.
        println!("cargo:warning=exe icon not embedded: {e}");
    }
}
