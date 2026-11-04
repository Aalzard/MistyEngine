//! Steam upload scripts: SteamPipe `app_build` / `depot_build` templates for the build folder and
//! a README that walks through uploading with `steamcmd`. No Steamworks SDK is needed to make
//! them; uploading needs the free SteamCMD tool and a Steamworks partner account.

use std::path::Path;

/// By Steamworks convention the first depot of an app is App ID + 1.
pub fn depot_id(app_id: &str) -> String {
    app_id
        .parse::<u64>()
        .map_or_else(|_| app_id.to_owned(), |id| (id + 1).to_string())
}

/// `app_build_<app>.vdf`.
pub fn app_build_vdf(app_id: &str, description: &str, content: &Path) -> String {
    let depot = depot_id(app_id);
    format!(
        r#""AppBuild"
{{
	"AppID" "{app_id}"
	"Desc" "{desc}"
	// Build logs and caches go here (next to this file).
	"BuildOutput" "output/"
	// The build folder (the one with the game executable).
	"ContentRoot" "{root}"
	// Set a branch name (e.g. "beta") to make the build live there right away; never "default".
	"SetLive" ""
	// 1 = dry run: report what would be uploaded without uploading.
	"Preview" "0"
	"Depots"
	{{
		"{depot}" "depot_build_{depot}.vdf"
	}}
}}
"#,
        desc = escape(description),
        root = escape(&content.to_string_lossy()),
    )
}

/// `depot_build_<depot>.vdf`: everything in the build folder except the Steam files.
pub fn depot_build_vdf(app_id: &str, content: &Path) -> String {
    format!(
        r#""DepotBuild"
{{
	"DepotID" "{depot}"
	"ContentRoot" "{root}"
	"FileMapping"
	{{
		"LocalPath" "*"
		"DepotPath" "."
		"Recursive" "1"
	}}
	// Development-only files never ship.
	"FileExclusion" "steam_appid.txt"
	"FileExclusion" "steam\*"
	"FileExclusion" "Player.log"
}}
"#,
        depot = depot_id(app_id),
        root = escape(&content.to_string_lossy()),
    )
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// README_STEAM.md for the build's `steam/` folder.
pub fn readme(app_id: &str, product: &str, exe: &str) -> String {
    let depot = depot_id(app_id);
    format!(
        r#"# Uploading {product} to Steam

These files were made by Misty's Build window. They describe *what* to upload; the free
**SteamCMD** tool does the upload. Misty doesn't talk to Steam itself.

## Once

1. Join Steamworks (partner.steamgames.com) and create your app. Its **App ID** is {app_id}.
2. In the app's Steamworks settings, open *SteamPipe ▸ Depots* and check that depot
   **{depot}** exists (Steam creates App ID + 1 by default).
3. In *Installation ▸ General*, add a launch option: executable `{exe}`, operating system
   matching this build. Publish the change.
4. Download SteamCMD (developer.valvesoftware.com/wiki/SteamCMD) and unzip it anywhere.

## Every release

1. Make the build in Misty (File ▸ Build Settings ▸ Build). Turn **Development Build off** for
   releases.
2. Run (replace the paths and account name):

   ```
   steamcmd +login <your_steam_account> +run_app_build "<this folder>/app_build_{app_id}.vdf" +quit
   ```

   Steam Guard asks for a code the first time.
3. In Steamworks, open *SteamPipe ▸ Builds*, find the new build, and set it live on a branch
   (try a private `beta` branch first, then `default`).

## Testing with Steam on your PC

Development builds get a `steam_appid.txt` next to the executable so the game knows its App ID
when started outside the Steam client. Release builds don't, and Steam starts them.

## Notes

- `depot_build_{depot}.vdf` uploads everything in the build folder except this `steam/` folder,
  `steam_appid.txt` and `Player.log`.
- Achievements, overlay and other Steamworks API features need the Steamworks SDK, which Misty
  doesn't include yet. Uploading and selling a game works without it.
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_point_at_the_build_folder() {
        let dir = Path::new(r"D:\Builds\My Game");
        let app = app_build_vdf("480", "v1.0 \"beta\"", dir);
        assert!(app.contains(r#""AppID" "480""#));
        assert!(app.contains(r#""481" "depot_build_481.vdf""#));
        assert!(app.contains(r#""ContentRoot" "D:\\Builds\\My Game""#));
        assert!(app.contains(r#"v1.0 \"beta\""#));
        let depot = depot_build_vdf("480", dir);
        assert!(depot.contains(r#""DepotID" "481""#));
        assert!(depot.contains("steam_appid.txt"));
        assert!(readme("480", "Ball", "Ball.exe").contains("+run_app_build"));
        assert_eq!(depot_id("x"), "x");
    }
}
