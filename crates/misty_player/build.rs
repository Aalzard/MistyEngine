//! Windows: the Misty Engine icon and version info in misty_player.exe (game builds replace
//! the icon with the project's own, see the editor's `build_pipeline::icon`).

include!("../misty_editor/build_icon.rs");

fn main() {
    embed_windows_icon(
        std::path::Path::new("../misty_editor/assets/branding/misty.ico"),
        "Misty Engine Player",
    );
}
