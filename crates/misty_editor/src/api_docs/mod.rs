//! The Lua API reference: generated from the engine's binding definitions (`scripting/*.rs`,
//! read at build time) and the Code Editor's autocomplete tables, as Markdown and one static
//! HTML page. `misty --api-docs <out>` writes it; Help ▸ Scripting Reference writes it to the
//! project's `Library/ApiDocs` and opens the page. The copy in `docs/api/` is checked by a test.

mod render;
mod scan;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::panels::code_editor::api as autocomplete;
pub use scan::Kind;

/// The binding sources, embedded so a released editor can write the reference too.
macro_rules! sources {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../../../misty_engine/src/scripting/", $name)))),*]
    };
}

pub const SOURCES: &[(&str, &str)] = sources!(
    "accessibility_api.rs",
    "animator_api.rs",
    "api.rs",
    "app_api.rs",
    "assets_api.rs",
    "attributes.rs",
    "audio_api.rs",
    "camera_api.rs",
    "cursor_api.rs",
    "input_actions_api.rs",
    "input_api.rs",
    "light_probes_api.rs",
    "localization_api.rs",
    "material_api.rs",
    "mod.rs",
    "navigation_api.rs",
    "network_api.rs",
    "override_api.rs",
    "particles_api.rs",
    "physics2d_api.rs",
    "physics_api.rs",
    "render_settings_api.rs",
    "rigging_api.rs",
    "save_api.rs",
    "save_host.rs",
    "scene_api.rs",
    "skinning_api.rs",
    "spline_api.rs",
    "syntax.rs",
    "terrain_api.rs",
    "tilemap_api.rs",
    "timeline_api.rs",
    "ui_api.rs",
    "ui_dispatch.rs",
    "ui_widgets_api.rs",
    "vcam_api.rs",
    "vehicle_api.rs",
    "vfx_api.rs",
    "video_api.rs",
    "water_api.rs",
    "weather_api.rs",
    "world_partition_api.rs",
);

/// The generated files, relative to the output folder.
pub const MARKDOWN: &str = "lua-api.md";
pub const HTML: &str = "index.html";

/// One documented member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocEntry {
    pub name: String,
    pub kind: Kind,
    /// `Input.GetKey(name)`, `transform.position`.
    pub signature: String,
    /// The autocomplete table's one-liner (what the Code Editor shows on hover).
    pub summary: String,
    /// The comment above the binding in the engine source.
    pub notes: String,
    /// `input_api.rs`, or empty for members known only from the autocomplete table.
    pub source: String,
    pub settable: bool,
}

/// A global table, an object type, or the globals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// `""` = globals, `Input`, `#Transform`.
    pub owner: String,
    pub title: String,
    pub description: String,
    pub entries: Vec<DocEntry>,
}

impl Section {
    /// Anchor used in both outputs (`input`, `type-transform`).
    pub fn anchor(&self) -> String {
        if self.owner.is_empty() {
            return "globals".into();
        }
        let base = self.owner.trim_start_matches('#').to_lowercase();
        if self.owner.starts_with('#') {
            format!("type-{base}")
        } else {
            base
        }
    }
}

/// Every section, in reference order: globals, global tables A–Z, object types A–Z.
pub fn sections() -> Vec<Section> {
    let mut found: BTreeMap<(String, String), scan::Found> = BTreeMap::new();
    for (file, text) in SOURCES {
        for f in scan::scan_file(file, text) {
            found.entry((f.owner.clone(), f.name.clone())).or_insert(f);
        }
    }
    let mut by_owner: BTreeMap<String, BTreeMap<String, DocEntry>> = BTreeMap::new();
    let mut descriptions: BTreeMap<String, String> = BTreeMap::new();
    // The autocomplete table: signatures and one-line docs.
    for e in autocomplete::entries() {
        if e.owner.is_empty() && !e.is_function() {
            descriptions.insert(e.name.to_owned(), e.doc.to_owned());
            by_owner.entry(e.name.to_owned()).or_default();
            continue;
        }
        let f = found.remove(&(e.owner.to_owned(), e.name.to_owned()));
        let kind = if e.is_function() {
            Kind::Function
        } else {
            Kind::Property
        };
        let entry = DocEntry {
            name: e.name.to_owned(),
            kind,
            signature: qualified(e.owner, e.signature, kind),
            summary: e.doc.to_owned(),
            notes: f.as_ref().map(|f| f.doc.clone()).unwrap_or_default(),
            source: f.as_ref().map(|f| f.source.clone()).unwrap_or_default(),
            settable: f.as_ref().is_some_and(|f| f.settable),
        };
        by_owner
            .entry(e.owner.to_owned())
            .or_default()
            .insert(e.name.to_owned(), entry);
    }
    // Bindings the table doesn't list: signature from the closure, doc from the comment.
    for ((owner, name), f) in found {
        let sig = match (f.kind, &f.params) {
            (Kind::Function, Some(p)) => format!("{name}({})", p.join(", ")),
            (Kind::Function, None) => format!("{name}(...)"),
            (Kind::Property, _) => name.clone(),
        };
        let entry = DocEntry {
            name: name.clone(),
            kind: f.kind,
            signature: qualified(&owner, &sig, f.kind),
            summary: String::new(),
            notes: f.doc,
            source: f.source,
            settable: f.settable,
        };
        by_owner.entry(owner).or_default().insert(name, entry);
    }
    let mut out: Vec<Section> = by_owner
        .into_iter()
        .filter(|(owner, entries)| !entries.is_empty() || !owner.is_empty())
        .map(|(owner, entries)| {
            let mut entries: Vec<DocEntry> = entries.into_values().collect();
            // Properties first, then functions, each A–Z.
            entries.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
            let title = match owner.as_str() {
                "" => "Globals".to_owned(),
                o if o.starts_with('#') => format!("{} (object)", &o[1..]),
                o => o.to_owned(),
            };
            let description = descriptions.get(&owner).cloned().unwrap_or_default();
            Section {
                owner,
                title,
                description,
                entries,
            }
        })
        .filter(|s| !s.entries.is_empty())
        .collect();
    out.sort_by_key(|s| {
        let rank = match s.owner.as_str() {
            "" => 0,
            o if o.starts_with('#') => 2,
            _ => 1,
        };
        (rank, s.owner.trim_start_matches('#').to_lowercase())
    });
    out
}

/// `Input.GetKey(name)` for tables, `obj:Method(a)` / `obj.field` for object types.
fn qualified(owner: &str, sig: &str, kind: Kind) -> String {
    match owner {
        "" => sig.to_owned(),
        o if o.starts_with('#') => {
            let var = lower_first(&o[1..]);
            let sep = if kind == Kind::Function { ":" } else { "." };
            format!("{var}{sep}{sig}")
        }
        o => format!("{o}.{sig}"),
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The Markdown reference.
#[cfg(test)]
pub fn markdown() -> String {
    render::markdown(&sections())
}

/// The single-page HTML reference.
#[cfg(test)]
pub fn html() -> String {
    render::html(&sections())
}

/// Write both files into `out` (created if needed); returns their paths.
pub fn write(out: &Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let secs = sections();
    let files = [
        (out.join(MARKDOWN), render::markdown(&secs)),
        (out.join(HTML), render::html(&secs)),
    ];
    let mut written = Vec::new();
    for (path, text) in files {
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// `misty --api-docs <out>`.
pub fn cli(out: &Path) -> i32 {
    match write(out) {
        Ok(paths) => {
            for p in paths {
                println!("Wrote {}", p.display());
            }
            0
        }
        Err(e) => {
            eprintln!("misty --api-docs: {e}");
            1
        }
    }
}

/// Help ▸ Scripting Reference: write the reference into the project's Library and open it.
pub fn open_reference(s: &mut crate::state::EditorState) {
    let out = s.project.root.join("Library/ApiDocs");
    match write(&out) {
        Ok(_) => {
            let page = out.join(HTML);
            if let Err(e) = crate::assets::open_external(&page) {
                s.console
                    .warn(format!("Could not open {}: {e}", page.display()));
            } else {
                s.console
                    .info(format!("Scripting Reference: {}", page.display()));
            }
        }
        Err(e) => s
            .console
            .warn(format!("Scripting Reference not written: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/api")
    }

    /// The committed `docs/api/` must match the bindings. When this fails, run
    /// `misty --api-docs docs/api` (or `cargo run -p misty_editor -- --api-docs docs/api`) and
    /// commit the result.
    #[test]
    fn committed_api_docs_are_up_to_date() {
        for (file, now) in [(MARKDOWN, markdown()), (HTML, html())] {
            let path = docs_dir().join(file);
            let saved = std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n");
            assert!(
                saved == now,
                "docs/api/{file} is stale: regenerate it with `misty --api-docs docs/api` \
                 (cargo run --release -p misty_editor -- --api-docs docs/api) and commit it"
            );
        }
    }

    #[test]
    fn every_binding_source_is_read() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../misty_engine/src/scripting");
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".rs") && !name.ends_with("tests.rs") {
                assert!(
                    SOURCES.iter().any(|(n, _)| *n == name),
                    "add scripting/{name} to api_docs::SOURCES"
                );
            }
        }
    }

    #[test]
    fn reference_has_signatures_and_source_comments() {
        let secs = sections();
        assert_eq!(secs[0].title, "Globals");
        let find = |owner: &str, name: &str| {
            secs.iter()
                .find(|s| s.owner == owner)
                .and_then(|s| s.entries.iter().find(|e| e.name == name))
                .unwrap_or_else(|| panic!("{owner}.{name}"))
                .clone()
        };
        let key = find("Input", "GetKey");
        assert_eq!(key.signature, "Input.GetKey(name)");
        assert!(key.summary.starts_with("True while the key is held"));
        assert_eq!(key.source, "api.rs");
        let rotate = find("#Transform", "Rotate");
        assert_eq!(rotate.signature, "transform:Rotate(eulerDegrees)");
        assert_eq!(
            find("#Transform", "position").signature,
            "transform.position"
        );
        // A binding the autocomplete table doesn't know: signature from the closure.
        let child = find("#Transform", "GetChild");
        assert_eq!(child.signature, "transform:GetChild(i)");
        // A comment above a binding becomes its notes.
        let res = find("Screen", "SetResolution");
        assert!(res.notes.contains("Called with a dot"), "{res:?}");
        // The accessibility settings are documented from their source comments.
        let scale = find("Accessibility", "textScale");
        assert!(!scale.notes.is_empty() || !scale.summary.is_empty());
        let md = markdown();
        assert!(md.contains("## Input"));
        assert!(md.contains("`Input.GetKey(name)`"));
        let page = html();
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("id=\"input\""));
        assert!(page.contains("Input.GetKey(name)"));
    }
}
