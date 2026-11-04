//! Windows: the Misty Engine icon and version info in misty.exe.

include!("build_icon.rs");

fn main() {
    embed_windows_icon(
        std::path::Path::new("assets/branding/misty.ico"),
        "Misty Engine",
    );
}
