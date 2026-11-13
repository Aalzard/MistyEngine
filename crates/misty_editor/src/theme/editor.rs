//! Preferences ▸ Appearance (theme, fonts, text size) and the Theme editor window.
//!
//! Everything applies live: [`Appearance::sync`] runs every frame and re-applies the palette
//! when the chosen theme or one of its colours changed. Custom themes are saved per user
//! (see [`super::custom`]); editing a preset's colour first copies it to a custom theme.

use egui::{Color32, RichText};

use super::custom::{self, Choice, CustomTheme, ThemeStore};
use super::fonts::{self, FaceRef, FontEntry, FontList, FontPrefs};
use super::presets::Preset;
use super::{Group, Palette, TOKENS};
use crate::i18n::tr;

/// Which font the inline picker is choosing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSlot {
    Ui,
    Mono,
}

/// The editor's appearance settings and the Theme editor's state.
pub struct Appearance {
    /// Where the choice is saved (`hub.json`); `None` = not saved (tests).
    pub config: Option<std::path::PathBuf>,
    pub choice: Choice,
    pub store: ThemeStore,
    pub fonts: FontPrefs,
    pub font_list: FontList,
    /// The Theme editor window is open.
    pub editor_open: bool,
    /// The inline font picker that is open in Preferences.
    pub picking: Option<FontSlot>,
    font_search: String,
    /// Show every font when picking the code font (not only monospace ones).
    all_mono: bool,
    /// Renaming a custom theme: the text being typed.
    rename: Option<String>,
    /// The last import / export / save result shown in the Theme editor.
    pub status: Option<String>,
    /// The palette last handed to egui.
    applied: Option<Palette>,
    /// A custom theme changed and is not written yet (saved once the mouse is up).
    dirty: bool,
    /// The note about a missing font, until this time (seconds).
    note: Option<(String, f64)>,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            config: None,
            choice: Choice::default(),
            store: ThemeStore::default(),
            fonts: FontPrefs::new(),
            font_list: FontList::default(),
            editor_open: false,
            picking: None,
            font_search: String::new(),
            all_mono: false,
            rename: None,
            status: None,
            applied: None,
            dirty: false,
            note: None,
        }
    }
}

impl Appearance {
    /// The saved theme choice, custom themes and fonts. `MISTY_THEME_EDITOR=1` opens the Theme
    /// editor (screenshots).
    pub fn load() -> Self {
        Self {
            config: crate::hub::config::config_path(),
            choice: custom::saved_choice(),
            store: ThemeStore::load(custom::themes_dir()),
            fonts: FontPrefs::load(),
            editor_open: std::env::var_os("MISTY_THEME_EDITOR").is_some(),
            ..Default::default()
        }
    }

    /// Dark, with the custom themes of `store`, saving the choice in `config` (tests).
    #[cfg(test)]
    pub fn with_store(store: ThemeStore, config: std::path::PathBuf) -> Self {
        Self {
            store,
            config: Some(config),
            ..Default::default()
        }
    }

    /// The palette of the chosen theme (Dark when a chosen custom theme is gone).
    pub fn palette(&self) -> Palette {
        self.store
            .palette(&self.choice)
            .unwrap_or_else(|| Preset::Dark.palette())
    }

    /// The chosen theme's name.
    pub fn choice_label(&self) -> String {
        match &self.choice {
            Choice::Preset(p) => p.label(),
            Choice::Custom(name) => name.clone(),
        }
    }

    /// Once per frame: apply a changed palette, save an edited theme once the mouse is up,
    /// pick up the note about a missing font and draw it.
    pub fn sync(&mut self, ctx: &egui::Context, console: &mut crate::console::Console) {
        let p = self.palette();
        if self.applied != Some(p) {
            super::apply_palette(ctx, &p);
            self.applied = Some(p);
        }
        if self.dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.dirty = false;
            if let Choice::Custom(name) = &self.choice
                && let Some(t) = self.store.get(name).cloned()
                && let Err(e) = self.store.save(t)
            {
                console.warn(format!("Theme not saved: {e}"));
            }
        }
        if let Some(missing) = fonts::take_fallback_note() {
            let text = tr!(
                "Font not found: {font}. Using the default font.",
                font = missing
            );
            console.warn(text.clone());
            self.note = Some((text, ctx.input(|i| i.time) + 8.0));
        }
        self.show_note(ctx);
    }

    /// The small note about a missing font (bottom right, a few seconds, or until closed).
    fn show_note(&mut self, ctx: &egui::Context) {
        let Some((text, until)) = &self.note else {
            return;
        };
        let now = ctx.input(|i| i.time);
        if now > *until {
            self.note = None;
            return;
        }
        let mut close = false;
        egui::Area::new(egui::Id::new("font_fallback_note"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -32.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(text.as_str()).small().color(super::warn()));
                        close = ui.small_button("✕").on_hover_text(tr!("Close")).clicked();
                    });
                });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        if close {
            self.note = None;
        }
    }

    /// Choose a theme and save the choice (the Hub's light / dark follows it).
    pub fn choose(&mut self, choice: Choice) -> Result<(), String> {
        self.choice = choice;
        self.rename = None;
        custom::save_choice(self.config.as_deref(), &self.choice, &self.palette())
    }

    /// The chosen custom theme to edit; a preset is first copied to a new custom theme.
    fn editable(&mut self) -> Result<&mut CustomTheme, String> {
        if let Choice::Preset(p) = self.choice {
            let name = self.store.unique_name(&format!("{} Custom", p.label()));
            self.store.save(CustomTheme::from_preset(name.clone(), p))?;
            self.choose(Choice::Custom(name))?;
        }
        let Choice::Custom(name) = &self.choice else {
            unreachable!("a preset was copied above");
        };
        let name = name.clone();
        if self.store.get(&name).is_none() {
            self.store
                .save(CustomTheme::from_preset(name.clone(), Preset::Dark))?;
        }
        Ok(self
            .store
            .themes
            .iter_mut()
            .find(|t| t.name == name)
            .expect("saved above"))
    }

    /// Set one colour of the chosen theme (live; written to disk once the mouse is up).
    pub fn set_color(&mut self, key: &str, color: Color32) -> Result<(), String> {
        let t = self.editable()?;
        if let Some(slot) = t.palette.get_mut(key)
            && *slot != color
        {
            *slot = color;
            self.dirty = true;
        }
        Ok(())
    }

    /// Copy the chosen theme to a new custom theme and choose it.
    pub fn duplicate(&mut self) -> Result<String, String> {
        let (base, palette) = match &self.choice {
            Choice::Preset(p) => (*p, p.palette()),
            Choice::Custom(n) => self
                .store
                .get(n)
                .map(|t| (t.base, t.palette))
                .unwrap_or((Preset::Dark, Preset::Dark.palette())),
        };
        let name = self
            .store
            .unique_name(&format!("{} Copy", self.choice_label()));
        self.store.save(CustomTheme {
            name: name.clone(),
            base,
            palette,
        })?;
        self.choose(Choice::Custom(name.clone()))?;
        Ok(name)
    }

    /// Rename the chosen custom theme.
    pub fn rename_current(&mut self, to: &str) -> Result<(), String> {
        let Choice::Custom(from) = self.choice.clone() else {
            return Ok(());
        };
        let name = self.store.rename(&from, to)?;
        self.choose(Choice::Custom(name))
    }

    /// Delete the chosen custom theme and go back to its base preset.
    pub fn delete_current(&mut self) -> Result<(), String> {
        let Choice::Custom(name) = self.choice.clone() else {
            return Ok(());
        };
        let base = self.store.get(&name).map_or(Preset::Dark, |t| t.base);
        self.store.delete(&name)?;
        self.choose(Choice::Preset(base))
    }

    /// Give the chosen custom theme a preset's colours.
    pub fn reset_to(&mut self, preset: Preset) -> Result<(), String> {
        if let Choice::Preset(_) = self.choice {
            return self.choose(Choice::Preset(preset));
        }
        let t = self.editable()?;
        t.base = preset;
        t.palette = preset.palette();
        let t = t.clone();
        self.store.save(t)
    }

    /// Import a `.mistytheme` file as a new custom theme (renamed if the name is taken) and
    /// choose it.
    pub fn import(&mut self, path: &std::path::Path) -> Result<String, String> {
        let mut t = CustomTheme::import(path)?;
        t.name = self.store.unique_name(&t.name);
        let name = t.name.clone();
        self.store.save(t)?;
        self.choose(Choice::Custom(name.clone()))?;
        Ok(name)
    }

    /// Export the chosen theme (a preset exports as a custom theme of its colours).
    pub fn export(&self, path: &std::path::Path) -> Result<(), String> {
        let t = match &self.choice {
            Choice::Preset(p) => CustomTheme::from_preset(p.label(), *p),
            Choice::Custom(n) => self
                .store
                .get(n)
                .cloned()
                .ok_or_else(|| format!("no theme named {n}"))?,
        };
        t.export(path)
    }

    /// Change a font choice: load it live and save it.
    pub fn set_font(
        &mut self,
        ctx: &egui::Context,
        slot: FontSlot,
        face: Option<FaceRef>,
    ) -> Result<(), String> {
        match slot {
            FontSlot::Ui => self.fonts.ui = face,
            FontSlot::Mono => self.fonts.mono = face,
        }
        fonts::apply(ctx, &self.fonts);
        self.fonts.save(self.config.as_deref())
    }

    /// Change the text size scale (live) and save it.
    pub fn set_font_scale(&mut self, ctx: &egui::Context, scale: f32) -> Result<(), String> {
        self.fonts.scale = fonts::clamp_scale(scale);
        fonts::apply_scale(ctx, self.fonts.scale);
        self.fonts.save(self.config.as_deref())
    }
}

/// The theme dropdown: presets, then custom themes. The choice picked this frame, if any.
fn theme_combo(ui: &mut egui::Ui, a: &Appearance, id: &str) -> Option<Choice> {
    let mut picked = None;
    let label = ui.label(tr!("Theme"));
    egui::ComboBox::from_id_salt(id)
        .selected_text(a.choice_label())
        .show_ui(ui, |ui| {
            for p in Preset::ALL {
                let c = Choice::Preset(p);
                if ui.selectable_label(a.choice == c, p.label()).clicked() {
                    picked = Some(c);
                }
            }
            if !a.store.themes.is_empty() {
                ui.separator();
            }
            for t in &a.store.themes {
                let c = Choice::Custom(t.name.clone());
                if ui.selectable_label(a.choice == c, &t.name).clicked() {
                    picked = Some(c);
                }
            }
        })
        .response
        .labelled_by(label.id);
    picked
}

fn font_label(a: &Appearance, slot: FontSlot) -> String {
    let face = match slot {
        FontSlot::Ui => a.fonts.ui.as_ref(),
        FontSlot::Mono => a.fonts.mono.as_ref(),
    };
    let Some(face) = face else {
        return tr!("Default");
    };
    let listed = a
        .font_list
        .fonts
        .as_deref()
        .and_then(|l| l.iter().find(|e| e.face == *face))
        .map(FontEntry::label);
    listed.unwrap_or_else(|| {
        face.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

/// Preferences ▸ Appearance: theme, Theme editor button, UI and code fonts, text size.
pub fn appearance_section(
    ui: &mut egui::Ui,
    a: &mut Appearance,
    console: &mut crate::console::Console,
) {
    ui.strong(tr!("Appearance"));
    ui.horizontal(|ui| {
        if let Some(c) = theme_combo(ui, a, "pref_theme")
            && let Err(e) = a.choose(c)
        {
            console.warn(format!("Theme not saved: {e}"));
        }
        if ui
            .button(tr!("Theme Editor…"))
            .on_hover_text(tr!(
                "Change the theme's colours, save and share custom themes"
            ))
            .clicked()
        {
            a.editor_open = true;
        }
    });
    for (slot, label) in [
        (FontSlot::Ui, tr!("UI Font")),
        (FontSlot::Mono, tr!("Code Font")),
    ] {
        ui.horizontal(|ui| {
            let l = ui.label(label);
            let open = a.picking == Some(slot);
            let r = ui
                .selectable_label(open, font_label(a, slot))
                .labelled_by(l.id)
                .on_hover_text(tr!("Choose from the fonts installed on this computer"));
            if r.clicked() {
                a.picking = if open { None } else { Some(slot) };
                a.font_search.clear();
            }
            let chosen = match slot {
                FontSlot::Ui => a.fonts.ui.is_some(),
                FontSlot::Mono => a.fonts.mono.is_some(),
            };
            if chosen
                && ui.small_button(tr!("Default")).clicked()
                && let Err(e) = a.set_font(ui.ctx(), slot, None)
            {
                console.warn(format!("Font not saved: {e}"));
            }
        });
        if a.picking == Some(slot) {
            font_picker(ui, a, slot, console);
        }
    }
    ui.horizontal(|ui| {
        let l = ui.label(tr!("Text Size"));
        let mut pct = (a.fonts.scale * 100.0).round();
        let r = ui
            .add(
                egui::Slider::new(
                    &mut pct,
                    fonts::MIN_SCALE * 100.0..=fonts::MAX_SCALE * 100.0,
                )
                .step_by(5.0)
                .suffix("%"),
            )
            .labelled_by(l.id);
        if r.changed()
            && let Err(e) = a.set_font_scale(ui.ctx(), pct / 100.0)
        {
            console.warn(format!("Text size not saved: {e}"));
        }
    });
}

/// The inline font list: a search box, the installed fonts and a preview line.
fn font_picker(
    ui: &mut egui::Ui,
    a: &mut Appearance,
    slot: FontSlot,
    console: &mut crate::console::Console,
) {
    a.font_list.request();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            let l = ui.label(tr!("Search"));
            ui.add(egui::TextEdit::singleline(&mut a.font_search).desired_width(180.0))
                .labelled_by(l.id);
            if slot == FontSlot::Mono {
                ui.checkbox(&mut a.all_mono, tr!("All fonts"))
                    .on_hover_text(tr!("Also list fonts that are not monospace"));
            }
        });
        let current = match slot {
            FontSlot::Ui => a.fonts.ui.clone(),
            FontSlot::Mono => a.fonts.mono.clone(),
        };
        let mut pick: Option<FaceRef> = None;
        let q = a.font_search.trim().to_lowercase();
        let all_mono = a.all_mono;
        match a.font_list.get() {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(tr!("Reading installed fonts…")).color(super::text_dim()),
                    );
                });
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            Some(list) => {
                let shown: Vec<&FontEntry> = list
                    .iter()
                    .filter(|e| slot == FontSlot::Ui || all_mono || e.monospace)
                    .filter(|e| q.is_empty() || e.label().to_lowercase().contains(&q))
                    .collect();
                if shown.is_empty() {
                    ui.label(RichText::new(tr!("No fonts match.")).color(super::text_dim()));
                }
                egui::ScrollArea::vertical()
                    .id_salt(("font_list", slot as u8))
                    .max_height(180.0)
                    .auto_shrink([false, true])
                    .show_rows(ui, 18.0, shown.len(), |ui, rows| {
                        for e in &shown[rows] {
                            let on = current.as_ref() == Some(&e.face);
                            if ui.selectable_label(on, e.label()).clicked() {
                                pick = Some(e.face.clone());
                            }
                        }
                    });
            }
        }
        // The preview: the chosen font (applied live) with Latin, Turkish and Arabic text.
        let family = match slot {
            FontSlot::Ui => egui::FontFamily::Proportional,
            FontSlot::Mono => egui::FontFamily::Monospace,
        };
        ui.label(
            RichText::new("The quick brown fox 0123 — ğüşıöç İ — مرحبا بالعالم")
                .family(family)
                .size(15.0),
        );
        if let Some(face) = pick
            && let Err(e) = a.set_font(ui.ctx(), slot, Some(face))
        {
            console.warn(format!("Font not saved: {e}"));
        }
    });
}

/// The Theme editor window (Preferences ▸ Appearance ▸ Theme Editor…).
pub fn show_window(ctx: &egui::Context, a: &mut Appearance, console: &mut crate::console::Console) {
    let mut open = a.editor_open;
    egui::Window::new(tr!("Theme Editor"))
        .id(egui::Id::new("theme_editor_window"))
        .open(&mut open)
        .default_size([420.0, 560.0])
        .collapsible(false)
        .show(ctx, |ui| editor_ui(ui, a, console));
    a.editor_open = open;
}

/// The Theme editor's contents.
pub fn editor_ui(ui: &mut egui::Ui, a: &mut Appearance, console: &mut crate::console::Console) {
    let mut result: Result<(), String> = Ok(());
    ui.horizontal(|ui| {
        if let Some(c) = theme_combo(ui, a, "theme_editor_theme") {
            result = a.choose(c);
        }
    });
    let custom = matches!(a.choice, Choice::Custom(_));
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(tr!("Duplicate"))
            .on_hover_text(tr!("Copy this theme to a new custom theme"))
            .clicked()
        {
            result = a.duplicate().map(|_| ());
        }
        if ui
            .add_enabled(custom, egui::Button::new(tr!("Rename")))
            .clicked()
        {
            a.rename = Some(a.choice_label());
        }
        if ui
            .add_enabled(custom, egui::Button::new(tr!("Delete")))
            .on_hover_text(tr!("Delete this custom theme"))
            .clicked()
        {
            result = a.delete_current();
        }
        ui.menu_button(tr!("Reset to Preset"), |ui| {
            for p in Preset::ALL {
                if ui.button(p.label()).clicked() {
                    result = a.reset_to(p);
                    ui.close();
                }
            }
        });
        if ui
            .button(tr!("Import…"))
            .on_hover_text(tr!("Add a theme from a .mistytheme file"))
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Misty theme", &[custom::EXTENSION])
                .pick_file()
        {
            result = a.import(&path).map(|name| {
                a.status = Some(tr!("Imported {name}", name = name));
            });
        }
        if ui
            .button(tr!("Export…"))
            .on_hover_text(tr!("Save this theme as a .mistytheme file to share"))
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Misty theme", &[custom::EXTENSION])
                .set_file_name(format!(
                    "{}.{}",
                    custom::clean_name(&a.choice_label()),
                    custom::EXTENSION
                ))
                .save_file()
        {
            result = a.export(&path).map(|()| {
                a.status = Some(tr!("Exported to {file}", file = path.display()));
            });
        }
    });
    if let Some(text) = &mut a.rename {
        let mut done = None;
        ui.horizontal(|ui| {
            let l = ui.label(tr!("Name"));
            let r = ui.text_edit_singleline(text).labelled_by(l.id);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            if ui.button(tr!("OK")).clicked() {
                done = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                done = Some(false);
            }
        });
        match done {
            Some(true) => {
                let to = a.rename.take().unwrap_or_default();
                result = a.rename_current(&to);
            }
            Some(false) => a.rename = None,
            None => {}
        }
    }
    if let Err(e) = result {
        console.warn(format!("Theme: {e}"));
        a.status = Some(e);
    }
    if let Some(s) = &a.status {
        ui.label(RichText::new(s.as_str()).small().color(super::text_dim()));
    }
    if !custom {
        ui.label(
            RichText::new(tr!(
                "Built-in theme: changing a colour saves a copy as a custom theme."
            ))
            .small()
            .color(super::text_dim()),
        );
    }
    let palette = a.palette();
    let low = custom::low_contrast(&palette);
    for (fg, bg, ratio) in &low {
        ui.label(
            RichText::new(tr!(
                "⚠ Low contrast: {text} on {background} ({ratio}:1)",
                text = tr_dyn(fg),
                background = tr_dyn(bg),
                ratio = format!("{ratio:.1}")
            ))
            .color(super::warn()),
        );
    }
    preview(ui, &palette);
    ui.separator();
    let mut edits: Vec<(&'static str, Color32)> = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt("theme_tokens")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for group in Group::ALL {
                egui::CollapsingHeader::new(tr_dyn(group.label()))
                    .id_salt(("theme_group", group.label()))
                    .default_open(matches!(
                        group,
                        Group::Panels | Group::Text | Group::Accents
                    ))
                    .show(ui, |ui| {
                        egui::Grid::new(("theme_grid", group.label()))
                            .num_columns(3)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                for t in TOKENS.iter().filter(|t| t.group == group) {
                                    let Some(mut c) = palette.get(t.key) else {
                                        continue;
                                    };
                                    let l = ui.label(tr_dyn(t.label));
                                    let r = egui::color_picker::color_edit_button_srgba(
                                        ui,
                                        &mut c,
                                        egui::color_picker::Alpha::Opaque,
                                    )
                                    .labelled_by(l.id);
                                    if r.changed() {
                                        edits.push((t.key, c));
                                    }
                                    let mut hex = custom::to_hex(c);
                                    let h = ui.add(
                                        egui::TextEdit::singleline(&mut hex)
                                            .desired_width(72.0)
                                            .font(egui::TextStyle::Monospace),
                                    );
                                    if h.changed()
                                        && let Some(parsed) = custom::from_hex(&hex)
                                    {
                                        edits.push((t.key, parsed));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            }
        });
    for (key, c) in edits {
        if let Err(e) = a.set_color(key, c) {
            console.warn(format!("Theme: {e}"));
        }
    }
}

/// A translated token, group or contrast-pair name (they are listed in the locale files).
fn tr_dyn(english: &str) -> String {
    crate::i18n::tr(english)
}

/// A small sample of the theme: a panel with text, a field, buttons, a selected row, status
/// colours and a graph node.
fn preview(ui: &mut egui::Ui, p: &Palette) {
    egui::Frame::new()
        .fill(p.panel_bg)
        .corner_radius(4.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(tr!("Text")).color(p.text));
                    ui.label(RichText::new(tr!("Weak text")).color(p.text_dim));
                    egui::Frame::new()
                        .fill(p.selection)
                        .corner_radius(2.0)
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.label(RichText::new(tr!("Selection")).color(p.selection_text));
                        });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr!("Warning")).color(p.warn));
                        ui.label(RichText::new(tr!("Error")).color(p.error));
                        ui.label(RichText::new(tr!("Success")).color(p.success));
                    });
                });
                ui.add_space(12.0);
                // A graph node on its canvas.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(130.0, 70.0), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 4.0, p.graph_canvas);
                let node = rect.shrink2(egui::vec2(16.0, 10.0));
                painter.rect_filled(node, 4.0, p.graph_node);
                let header = egui::Rect::from_min_size(node.min, egui::vec2(node.width(), 16.0));
                painter.rect_filled(
                    header,
                    egui::CornerRadius {
                        nw: 4,
                        ne: 4,
                        sw: 0,
                        se: 0,
                    },
                    p.graph_header,
                );
                painter.rect_stroke(
                    node,
                    4.0,
                    egui::Stroke::new(1.5, p.graph_select),
                    egui::StrokeKind::Outside,
                );
                painter.text(
                    header.left_center() + egui::vec2(6.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    tr!("Node"),
                    egui::FontId::proportional(11.0),
                    p.text,
                );
            });
        });
}
