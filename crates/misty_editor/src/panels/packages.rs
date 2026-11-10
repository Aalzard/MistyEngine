//! Packages window (Window ▸ Package Manager…): built-in modules, packages in the project and
//! the registry's packages on the left; the selected package's details, dependencies, samples
//! and Install / Update / Remove on the right. The ＋ menu adds packages from disk, a zip or a
//! git URL.

use std::path::Path;

use egui::{RichText, Ui};

use crate::packages::manifest::{PackageJson, Source};
use crate::packages::registry::RegistryPackage;
use crate::packages::resolve::Origin;
use crate::packages::semver::{Range, Version};
use crate::packages::{Filter, PackagesWindow, RegistryState, builtin, install};
use crate::state::EditorState;
use crate::theme;
use crate::widgets::{panel_toolbar, search_field};

pub const ICON: &str = "📦";

/// One row of the list.
#[derive(Clone)]
struct Row {
    name: String,
    title: String,
    version: String,
    /// "update available" and the like.
    note: Option<String>,
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let project = state.project.root.clone();
    let w = &mut state.packages;
    w.ensure_loaded(&project);
    w.poll();
    let mut refresh_project = false;

    panel_toolbar(ui, |ui| {
        ui.menu_button("＋ ⏷", |ui| {
            add_menu(ui, w, &project, &mut refresh_project)
        });
        egui::ComboBox::from_id_salt("packages_filter")
            .selected_text(format!("Packages: {}", w.filter.label()))
            .show_ui(ui, |ui| {
                for f in Filter::ALL {
                    ui.selectable_value(&mut w.filter, f, f.label());
                }
            });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("⟳").on_hover_text("Refresh").clicked() {
                w.loaded_for = None;
                w.ensure_loaded(&project);
                refresh_project = true;
            }
            search_field(ui, &mut w.search, 150.0);
        });
    });
    if let Some(url) = w.git_url.as_mut() {
        let mut go = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.label("Git URL");
            let r = ui.add(
                egui::TextEdit::singleline(url)
                    .hint_text("https://host/repo.git#v1.0.0")
                    .desired_width(300.0),
            );
            go = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            go |= ui.button("Add").clicked();
            cancel = ui.button("Cancel").clicked();
        });
        if go && !url.trim().is_empty() {
            let url = url.clone();
            w.git_url = None;
            w.install_git(&project, &url);
            refresh_project = true;
        } else if cancel {
            w.git_url = None;
        }
    }
    if let Some((err, msg)) = &w.status {
        let color = if *err { theme::WARN } else { theme::TEXT_DIM };
        ui.label(RichText::new(msg).color(color));
    }
    for c in &w.resolution.conflicts {
        ui.label(RichText::new(format!("⚠ Conflict: {}", c.message())).color(theme::WARN));
    }
    for (dir, e) in &w.broken {
        ui.label(
            RichText::new(format!(
                "⚠ {}: {e}",
                dir.file_name().unwrap_or_default().to_string_lossy()
            ))
            .color(theme::WARN),
        );
    }

    let rows = rows(w);
    egui::Panel::left("packages_list")
        .resizable(true)
        .default_size(250.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    if w.filter == Filter::Registry {
                        match &w.registry {
                            RegistryState::NotConfigured => {
                                ui.weak("No registry configured.");
                                ui.weak("Set one in Project Settings ▸ Package Manager.");
                            }
                            RegistryState::Loading(_) => {
                                ui.weak("Loading the registry…");
                            }
                            RegistryState::Failed(e) => {
                                ui.label(
                                    RichText::new(format!("Registry: {e}")).color(theme::WARN),
                                );
                            }
                            RegistryState::Loaded(_) => {}
                        }
                    }
                    if rows.is_empty() && w.filter == Filter::InProject {
                        ui.weak("No packages in this project. Use ＋ to add one.");
                    }
                    for r in &rows {
                        let sel = w.selected.as_deref() == Some(r.name.as_str());
                        let resp = ui
                            .horizontal(|ui| {
                                let resp = ui.selectable_label(sel, &r.title);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.weak(&r.version);
                                        if let Some(n) = &r.note {
                                            ui.label(RichText::new(n).color(theme::PREFAB));
                                        }
                                    },
                                );
                                resp
                            })
                            .inner;
                        if resp.clicked() {
                            w.selected = Some(r.name.clone());
                        }
                    }
                });
        });
    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                refresh_project |= details(ui, w, &project);
            });
    });
    if refresh_project {
        state.project.refresh();
        if let Some((false, m)) = &state.packages.status {
            state.console.info(m.clone());
        } else if let Some((true, m)) = &state.packages.status {
            state.console.warn(m.clone());
        }
    }
}

fn add_menu(ui: &mut Ui, w: &mut PackagesWindow, project: &Path, refresh: &mut bool) {
    if ui.button("Add package from disk…").clicked() {
        ui.close();
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Folder with a package.json")
            .pick_folder()
        {
            w.install_folder(project, &dir);
            *refresh = true;
        }
    }
    if ui.button("Add package from zip…").clicked() {
        ui.close();
        if let Some(zip) = rfd::FileDialog::new()
            .add_filter("Zip archive", &["zip"])
            .pick_file()
        {
            w.install_zip(project, &zip);
            *refresh = true;
        }
    }
    let git = w.git_available();
    let r = ui.add_enabled(git, egui::Button::new("Add package from git URL…"));
    let r = if git {
        r
    } else {
        r.on_disabled_hover_text(
            "git was not found on PATH: install git to add packages from git URLs",
        )
    };
    if r.clicked() {
        w.git_url = Some(String::new());
        ui.close();
    }
    if !git {
        ui.weak("git not found on PATH");
    }
}

fn matches(search: &str, name: &str, title: &str) -> bool {
    let s = search.to_lowercase();
    s.is_empty() || name.to_lowercase().contains(&s) || title.to_lowercase().contains(&s)
}

fn rows(w: &PackagesWindow) -> Vec<Row> {
    let mut out = Vec::new();
    match w.filter {
        Filter::InProject => {
            for p in &w.installed {
                let update = registry_package(w, &p.info.name)
                    .and_then(RegistryPackage::latest)
                    .filter(|v| *v > p.info.version())
                    .map(|v| format!("↑ {v}"));
                out.push(Row {
                    name: p.info.name.clone(),
                    title: p.info.title().to_owned(),
                    version: p.info.version.clone(),
                    note: update,
                });
            }
        }
        Filter::BuiltIn => {
            for p in builtin::all() {
                out.push(Row {
                    name: p.name.clone(),
                    title: p.title().to_owned(),
                    version: p.version.clone(),
                    note: None,
                });
            }
        }
        Filter::Registry => {
            if let Some(index) = w.registry.index() {
                for p in &index.packages {
                    let installed = w.installed.iter().any(|i| i.info.name == p.name);
                    out.push(Row {
                        name: p.name.clone(),
                        title: if p.display_name.is_empty() {
                            p.name.clone()
                        } else {
                            p.display_name.clone()
                        },
                        version: p.latest().map(|v| v.to_string()).unwrap_or_default(),
                        note: installed.then(|| "✔".to_owned()),
                    });
                }
            }
        }
    }
    out.retain(|r| matches(&w.search, &r.name, &r.title));
    out
}

fn registry_package<'a>(w: &'a PackagesWindow, name: &str) -> Option<&'a RegistryPackage> {
    w.registry.index().and_then(|i| i.package(name))
}

/// The right side; returns true when the project's files changed.
fn details(ui: &mut Ui, w: &mut PackagesWindow, project: &Path) -> bool {
    let Some(name) = w.selected.clone() else {
        ui.weak("Select a package to see its details.");
        return false;
    };
    let installed = w.installed.iter().find(|p| p.info.name == name).cloned();
    let builtin = builtin::all().into_iter().find(|p| p.name == name);
    let reg = registry_package(w, &name).cloned();
    let info: PackageJson = match (&installed, &builtin, &reg) {
        (Some(p), _, _) => p.info.clone(),
        (None, Some(b), _) => b.clone(),
        (None, None, Some(r)) => PackageJson {
            name: r.name.clone(),
            display_name: r.display_name.clone(),
            version: r.latest().map(|v| v.to_string()).unwrap_or_default(),
            description: r.description.clone(),
            author: crate::packages::manifest::Author::Name(r.author.clone()),
            license: r.license.clone(),
            dependencies: r
                .latest()
                .and_then(|v| {
                    r.versions
                        .get(&v.to_string())
                        .map(|x| x.dependencies.clone())
                })
                .unwrap_or_default(),
            samples: Vec::new(),
        },
        _ => {
            ui.weak(format!("{name} is not available."));
            return false;
        }
    };
    let mut changed = false;
    ui.heading(info.title());
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Version {}", info.version)).strong());
        let source = if builtin.is_some() {
            "Built-in".to_owned()
        } else if installed.is_some() {
            match w.manifest.source(&name) {
                Some(Source::Git(url, _)) => format!("Git: {url}"),
                Some(Source::Local(_)) => "Local (Packages folder)".into(),
                Some(Source::Registry(r)) => format!("Registry ({r})"),
                None if w.is_registry_package(&name) => "Registry (dependency)".into(),
                None => "In Packages folder (not in the manifest)".into(),
            }
        } else {
            "Registry".into()
        };
        ui.weak(source);
    });
    ui.weak(&info.name);
    if !info.author.name().is_empty() {
        ui.label(format!("By {}", info.author.name()));
    }
    if !info.license.is_empty() {
        ui.weak(format!("License: {}", info.license));
    }
    ui.add_space(4.0);
    ui.label(&info.description);
    ui.add_space(6.0);

    if !info.dependencies.is_empty() {
        ui.label(RichText::new("Dependencies").strong());
        for (dep, range) in &info.dependencies {
            let resolved = w.resolution.packages.get(dep);
            let ok = resolved
                .is_some_and(|r| Range::parse(range).is_ok_and(|rg| rg.matches(&r.version)));
            let status = match resolved {
                Some(r) if ok => format!("✔ {}", r.version),
                Some(r) => format!("✖ {} doesn't match", r.version),
                None => "✖ missing".into(),
            };
            let color = if ok { theme::TEXT } else { theme::WARN };
            ui.label(RichText::new(format!("{dep} {range}   {status}")).color(color));
        }
        ui.add_space(6.0);
    }

    // Packages that use this one.
    let users: Vec<String> = w
        .installed
        .iter()
        .filter(|p| p.info.dependencies.contains_key(&name))
        .map(|p| p.info.title().to_owned())
        .collect();
    if !users.is_empty() {
        ui.weak(format!("Used by: {}", users.join(", ")));
    }

    if let Some(pkg) = &installed {
        let samples = install::samples(pkg);
        if !samples.is_empty() {
            ui.label(RichText::new("Samples").strong());
            for s in &samples {
                ui.horizontal(|ui| {
                    ui.label(&s.display_name);
                    if !s.description.is_empty() {
                        ui.weak(&s.description);
                    }
                    let done = install::sample_dest(project, &pkg.info, s).is_dir();
                    let label = if done { "Reimport" } else { "Import Sample" };
                    if ui.button(label).clicked() {
                        w.import_sample(project, &name, s);
                        changed = true;
                    }
                });
            }
            ui.add_space(6.0);
        }
    }

    ui.horizontal(|ui| {
        if builtin.is_some() {
            ui.add_enabled(false, egui::Button::new("Remove"))
                .on_disabled_hover_text("Built-in packages can't be removed");
            return;
        }
        if let Some(r) = &reg {
            let versions = r.sorted_versions();
            let have = installed.as_ref().map(|p| p.info.version());
            match (&have, versions.first()) {
                (None, Some(latest)) if ui.button(format!("Install {latest}")).clicked() => {
                    w.install_registry(project, &name, latest);
                    changed = true;
                }
                (Some(h), Some(latest))
                    if latest > h && ui.button(format!("Update to {latest}")).clicked() =>
                {
                    update(w, project, &name, latest);
                    changed = true;
                }
                _ => {}
            }
        }
        if installed.is_some() && ui.button("Remove").clicked() {
            w.remove(project, &name);
            changed = true;
        }
    });
    if installed
        .as_ref()
        .is_some_and(|_| w.is_registry_package(&name))
    {
        ui.weak("Registry packages are read-only in the Project window.");
    }
    if installed.is_some()
        && let Some(origin) = w.resolution.packages.get(&name).map(|r| r.origin.clone())
        && origin == Origin::Registry
        && reg.is_none()
    {
        ui.weak("Its registry is not loaded: updates can't be checked.");
    }
    changed
}

fn update(w: &mut PackagesWindow, project: &Path, name: &str, to: &Version) {
    w.install_registry(project, name, to);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;
    use crate::project::ProjectBrowser;

    fn project(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_pkg_ui_{tag}_{n}"));
        let pkg = root.join("Packages/com.test.tools");
        std::fs::create_dir_all(pkg.join("Samples~/Demo")).unwrap();
        std::fs::create_dir_all(pkg.join("Runtime")).unwrap();
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r#"{"name":"com.test.tools","displayName":"Test Tools","version":"1.2.0",
                "description":"Handy tools","author":"Tester","license":"MIT",
                "dependencies":{"com.misty.modules.physics":"*"}}"#,
        )
        .unwrap();
        std::fs::write(pkg.join("Runtime/T.lua"), "return {}").unwrap();
        std::fs::write(pkg.join("Samples~/Demo/a.txt"), "a").unwrap();
        std::fs::write(
            root.join("Packages/manifest.json"),
            r#"{"dependencies":{"com.test.tools":"file:com.test.tools"}}"#,
        )
        .unwrap();
        root
    }

    fn window(s: EditorState) -> Harness<'static, EditorState> {
        let mut h = Harness::builder()
            .with_size([800.0, 600.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        h
    }

    #[test]
    fn package_manager_lists_shows_details_imports_and_removes() {
        let root = project("window");
        let mut h = window(EditorState::new(ProjectBrowser::open(root.clone())));
        h.get_by_label("Test Tools").click();
        h.run();
        h.get_by_label("Version 1.2.0");
        h.get_by_label("Local (Packages folder)");
        h.get_by_label("Handy tools");
        h.get_by_label_contains("com.misty.modules.physics *");
        h.get_by_label("Import Sample").click();
        h.run();
        assert!(
            root.join("Assets/Samples/Test Tools/1.2.0/Demo/a.txt")
                .is_file()
        );
        h.get_by_label("Reimport");
        // The lock file records the resolution.
        let lock = crate::packages::manifest::Lock::load(&root).unwrap();
        assert_eq!(lock.dependencies["com.test.tools"].version, "1.2.0");
        assert_eq!(
            lock.dependencies["com.misty.modules.physics"].source,
            "builtin"
        );

        // Built-in modules: listed, not removable.
        h.state_mut().packages.filter = Filter::BuiltIn;
        h.run();
        h.get_by_label("Physics").click();
        h.run();
        assert!(h.get_by_label("Remove").accesskit_node().is_disabled());

        // Registry: none configured.
        h.state_mut().packages.filter = Filter::Registry;
        h.run();
        h.get_by_label("No registry configured.");

        // The ＋ menu offers disk, zip and git.
        h.get_by_label("＋ ⏷").click();
        h.run();
        h.get_by_label("Add package from disk…");
        h.get_by_label("Add package from zip…");
        let git = h.get_by_label("Add package from git URL…");
        assert_eq!(
            git.accesskit_node().is_disabled(),
            !crate::packages::install::git_available()
        );
        h.key_press(egui::Key::Escape);
        h.run();

        h.state_mut().packages.filter = Filter::InProject;
        h.state_mut().packages.selected = Some("com.test.tools".into());
        h.run();
        h.get_by_label("Remove").click();
        h.run();
        assert!(!root.join("Packages/com.test.tools").exists());
        h.get_by_label("No packages in this project. Use ＋ to add one.");
    }

    #[test]
    fn project_window_shows_packages_under_their_root() {
        let root = project("tree");
        let mut b = ProjectBrowser::open(root.clone());
        assert_eq!(b.packages.len(), 1);
        assert_eq!(b.packages[0].name, "Test Tools");
        assert_eq!(b.packages[0].children.len(), 2, "Runtime and Samples~");
        let runtime = root.join("Packages/com.test.tools/Runtime");
        b.open_folder(runtime.clone());
        assert!(b.entries.iter().any(|e| e.name == "T.lua"));
        let crumbs: Vec<String> = b.breadcrumb().into_iter().map(|(n, _)| n).collect();
        assert_eq!(crumbs, ["Packages", "com.test.tools", "Runtime"]);
        assert!(b.expanded.contains(&root.join("Packages")));
        assert_eq!(
            crate::panels::project::asset_relative(&runtime.join("T.lua")).as_deref(),
            Some("Packages/com.test.tools/Runtime/T.lua")
        );
        // Registry packages are read-only.
        assert!(!b.is_read_only(&runtime));
        let mut lock = crate::packages::manifest::Lock::default();
        lock.dependencies.insert(
            "com.test.tools".into(),
            crate::packages::manifest::LockEntry {
                version: "1.2.0".into(),
                source: "registry".into(),
                ..Default::default()
            },
        );
        lock.save(&root).unwrap();
        b.refresh();
        assert!(b.is_read_only(&runtime));
    }

    #[test]
    fn registry_url_page_in_project_settings() {
        let root = project("settings");
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        s.project_settings_window.category = crate::project_settings::Category::PackageManager;
        let mut h = Harness::builder().with_size([700.0, 400.0]).build_ui_state(
            |ui, s: &mut EditorState| crate::project_settings::contents(ui, s),
            s,
        );
        h.run();
        h.get_by_label("No registry configured.");
        let index = root.join("index.json");
        std::fs::write(
            &index,
            r#"{"packages":[{"name":"com.r.x","versions":{"1.0.0":{"url":"x.zip"}}}]}"#,
        )
        .unwrap();
        let url = format!(
            "file:///{}",
            index
                .to_string_lossy()
                .replace('\\', "/")
                .trim_start_matches('/')
        );
        h.state_mut().packages.set_registry(&root, &url);
        // The index is read on a thread: wait for it by the clock (under load 200 short
        // frames ran out while it still showed "Loading the registry index…").
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while std::time::Instant::now() < deadline {
            h.run();
            if !matches!(h.state().packages.registry, RegistryState::Loading(_)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.get_by_label("1 packages in the registry.");
        let m = crate::packages::manifest::Manifest::load(&root).unwrap();
        assert_eq!(m.registry, url);
    }
}
