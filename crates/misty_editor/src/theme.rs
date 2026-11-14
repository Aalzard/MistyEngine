//! The editor's look: a palette of colour tokens (the Dark preset is the familiar grey-panel /
//! blue-selection look), the egui visuals and dock style built from it, and the UI fonts.
//! No outlines: areas and widgets are told apart by soft shade steps (bars darker than panel
//! bodies, fields darker, buttons lighter); only focus and selection use the accent colour.
//!
//! Panels read colours through the token functions ([`text`], [`panel_bg`] …), which return the
//! active theme's colour (Edit ▸ Preferences ▸ Appearance). Fixed greys drawn by panels go
//! through [`shade`], which maps a colour made for the Dark theme onto the active theme (the
//! identity under Dark). Presets live in [`presets`], custom themes and `.mistytheme` files in
//! [`custom`], the font choice in [`fonts`] and the Theme editor in [`editor`].

pub mod custom;
pub mod editor;
pub mod fonts;
#[cfg(test)]
mod panel_tests;
pub mod presets;
#[cfg(test)]
mod tests;

use std::cell::RefCell;

use egui::{Color32, CornerRadius, Stroke, Visuals};

/// Scene gizmo axis colours (drawn over the 3D view, the same in every theme).
pub const AXIS_X: Color32 = Color32::from_rgb(219, 62, 29);
pub const AXIS_Y: Color32 = Color32::from_rgb(139, 220, 0);
pub const AXIS_Z: Color32 = Color32::from_rgb(58, 122, 237);

/// Where a token is shown in the Theme editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Panels,
    Text,
    Accents,
    Status,
    Graphs,
    Code,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::Panels,
        Group::Text,
        Group::Accents,
        Group::Status,
        Group::Graphs,
        Group::Code,
    ];

    /// English name (translate with `tr!` where shown).
    pub fn label(self) -> &'static str {
        match self {
            Group::Panels => "Panels",
            Group::Text => "Text",
            Group::Accents => "Accents",
            Group::Status => "Status",
            Group::Graphs => "Graphs",
            Group::Code => "Code",
        }
    }
}

/// One colour token: its key in `.mistytheme` files, its English label and editor group.
#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub key: &'static str,
    pub label: &'static str,
    pub group: Group,
}

macro_rules! palette {
    ($( $(#[$m:meta])* $field:ident : $group:ident, $label:literal; )*) => {
        /// Every colour the editor UI draws with. `Copy`, so a theme is a plain value.
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Palette {
            $( $(#[$m])* pub $field: Color32, )*
        }

        /// The tokens in Theme-editor order.
        pub const TOKENS: &[Token] = &[
            $( Token { key: stringify!($field), label: $label, group: Group::$group }, )*
        ];

        impl Palette {
            /// The colour of the token `key` (see [`TOKENS`]).
            pub fn get(&self, key: &str) -> Option<Color32> {
                match key {
                    $( stringify!($field) => Some(self.$field), )*
                    _ => None,
                }
            }

            /// The colour of the token `key`, to edit it.
            pub fn get_mut(&mut self, key: &str) -> Option<&mut Color32> {
                match key {
                    $( stringify!($field) => Some(&mut self.$field), )*
                    _ => None,
                }
            }
        }

        $(
            $(#[$m])*
            // Every token has one, also those only drawn through the palette today.
            #[allow(dead_code)]
            #[inline]
            pub fn $field() -> Color32 {
                CURRENT.with(|c| c.borrow().$field)
            }
        )*
    };
}

palette! {
    /// Tab strips, title and menu bars: the darkest chrome.
    tab_bar_bg: Panels, "Background";
    /// Panel bodies.
    panel_bg: Panels, "Panel";
    tab_active: Panels, "Active tab";
    tab_inactive: Panels, "Inactive tab";
    /// Panel toolbars (the Hierarchy / Project create and search strips, bottom bars).
    toolbar_bg: Panels, "Toolbar";
    /// The Project window's folder tree, a soft step off the file area.
    sidebar_bg: Panels, "Panel alt";
    /// Popups and menus: a step lighter than panels instead of an outline.
    popup_bg: Panels, "Popup";
    /// Inspector component cards: body and header bar (a soft step lighter than the panel).
    card_bg: Panels, "Card";
    card_header: Panels, "Card header";
    /// Text fields and recessed areas.
    field_bg: Panels, "Field";
    /// Number fields and dropdowns inside Inspector property rows.
    input_bg: Panels, "Input";
    input_hover: Panels, "Input hover";
    button_bg: Panels, "Button";
    hover_bg: Panels, "Hover";
    /// Dock separators and the gaps between docked panels: a soft step darker than the panels.
    border: Panels, "Border";
    /// Soft divider lines (separators, panel splits): close to the panel colour.
    divider: Panels, "Divider";
    text: Text, "Text";
    text_dim: Text, "Weak text";
    /// Bold titles and the active tab's name.
    text_strong: Text, "Strong text";
    /// Text drawn on the selection colour.
    selection_text: Text, "Selected text";
    /// Links, drop guides and the accent icon tint.
    accent: Accents, "Accent";
    selection: Accents, "Selection";
    /// A button while it is pressed.
    pressed: Accents, "Pressed";
    /// Names of prefab instances in the Hierarchy (and the prefab icon).
    prefab: Accents, "Prefab";
    prefab_dim: Accents, "Prefab (dim)";
    /// The bar beside overridden values in the Inspector, and added objects' badge.
    overridden: Accents, "Override";
    /// The tab strip tint while playing.
    play_tint: Accents, "Play mode tint";
    /// Warnings and problems that need the user's attention.
    warn: Status, "Warning";
    error: Status, "Error";
    success: Status, "Success";
    /// Instances whose prefab file is gone ("Missing Prefab").
    missing_prefab: Status, "Missing prefab";
    /// Script / Shader / Effect Graph and Animator canvases.
    graph_canvas: Graphs, "Graph background";
    graph_node: Graphs, "Node body";
    graph_header: Graphs, "Node header";
    graph_select: Graphs, "Node selection";
    code_bg: Code, "Code background";
    code_text: Code, "Code text";
    code_line_no: Code, "Line numbers";
    code_current_line: Code, "Current line";
    code_selection: Code, "Code selection";
}

thread_local! {
    /// The active theme. Per thread: the UI runs on one thread, and tests that draw under
    /// another theme don't change the colours other tests see.
    static CURRENT: RefCell<Palette> = const { RefCell::new(presets::DARK) };
}

/// The active palette.
pub fn current() -> Palette {
    CURRENT.with(|c| *c.borrow())
}

/// Make `p` the active palette (without touching egui's visuals; see [`apply_palette`]).
pub fn set_current(p: Palette) {
    CURRENT.with(|c| *c.borrow_mut() = p);
}

/// The relative luminance (WCAG) of an sRGB colour, 0 … 1.
pub fn luminance(c: Color32) -> f32 {
    fn lin(v: u8) -> f32 {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// The WCAG contrast ratio of two colours (1 … 21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

impl Palette {
    /// Dark panels (egui's dark visuals, light icon tints).
    pub fn is_dark(&self) -> bool {
        luminance(self.panel_bg) < 0.25
    }

    /// The fixed Dark-theme shades that [`shade`](Self::shade) maps through: (grey level in
    /// the Dark preset, this palette's colour for it). Sorted by level.
    fn anchors(&self) -> [(f32, Color32); 9] {
        [
            (40.0, self.tab_bar_bg),
            (42.0, self.field_bg),
            (56.0, self.panel_bg),
            (63.0, self.card_bg),
            (78.0, self.button_bg),
            (92.0, self.hover_bg),
            (130.0, self.text_dim),
            (210.0, self.text),
            (232.0, self.text_strong),
        ]
    }

    /// Map a colour made for the Dark theme onto this palette: its brightness is placed
    /// between the palette's matching shades (backgrounds, buttons, weak and strong text) and
    /// its tint is kept. Under the Dark preset this returns `c` unchanged; under Light a dark
    /// canvas becomes a light one, light text becomes dark and bright accents get darker.
    pub fn shade(&self, c: Color32) -> Color32 {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        if a == 0 {
            return c;
        }
        let (r, g, b) = (r as f32, g as f32, b as f32);
        let l = (r + g + b) / 3.0;
        let anchors = self.anchors();
        let rgb = |c: Color32| [c.r() as f32, c.g() as f32, c.b() as f32];
        let base: [f32; 3] = if l <= anchors[0].0 {
            // Darker than every anchor: as far again from the text as the darkest shade.
            let (a0, c0) = anchors[0];
            let (at, ct) = anchors[7];
            let t = (a0 - l) / (at - a0);
            let (c0, ct) = (rgb(c0), rgb(ct));
            [0, 1, 2].map(|i| c0[i] + (c0[i] - ct[i]) * t)
        } else if l >= anchors[8].0 {
            // Brighter than every anchor: further on from weak text past strong text.
            let (an, cn) = anchors[8];
            let (aw, cw) = anchors[6];
            let t = (l - an) / (an - aw);
            let (cn, cw) = (rgb(cn), rgb(cw));
            [0, 1, 2].map(|i| cn[i] + (cn[i] - cw[i]) * t)
        } else {
            let i = anchors.windows(2).position(|w| l <= w[1].0).unwrap_or(7);
            let ((la, ca), (lb, cb)) = (anchors[i], anchors[i + 1]);
            let t = (l - la) / (lb - la);
            let (ca, cb) = (rgb(ca), rgb(cb));
            [0, 1, 2].map(|k| ca[k] + (cb[k] - ca[k]) * t)
        };
        let out = [base[0] + r - l, base[1] + g - l, base[2] + b - l]
            .map(|v| v.round().clamp(0.0, 255.0) as u8);
        Color32::from_rgba_unmultiplied(out[0], out[1], out[2], a)
    }
}

/// [`Palette::shade`] with the active palette: wrap fixed UI colours made for the Dark theme.
pub fn shade(c: Color32) -> Color32 {
    CURRENT.with(|p| p.borrow().shade(c))
}

/// A colour drawn beside a token (a shader node body next to the graph node token): the
/// token's colour in the active theme, offset as `dark` was from it under Dark. Exactly `dark`
/// under the Dark preset; follows the token when a theme changes it.
pub fn near(token: fn(&Palette) -> Color32, dark: Color32) -> Color32 {
    let (now, was) = CURRENT.with(|c| (token(&c.borrow()), token(&presets::DARK)));
    let ch = |n: u8, w: u8, d: u8| (n as i32 + d as i32 - w as i32).clamp(0, 255) as u8;
    Color32::from_rgba_unmultiplied(
        ch(now.r(), was.r(), dark.r()),
        ch(now.g(), was.g(), dark.g()),
        ch(now.b(), was.b(), dark.b()),
        dark.a(),
    )
}

/// `c` as text on the active theme's panels: unchanged when it already reads (always under
/// Dark), otherwise darkened or lightened until it reaches 3:1 against the panel colour.
pub fn readable(c: Color32) -> Color32 {
    let p = current();
    let target = if p.is_dark() {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let mut out = c;
    for i in 1..=10 {
        if contrast(out, p.panel_bg) >= 3.0 {
            break;
        }
        out = c.lerp_to_gamma(target, i as f32 * 0.1);
    }
    out
}

/// The built-in Arabic font's name in the font definitions.
pub(crate) const ARABIC_FONT: &str = "arabic";

/// Font family of bold titles (falls back to the regular font when no bold face was found).
pub(crate) const BOLD_FAMILY: &str = "bold";

/// The bold font family, or the regular one when the bold face is not loaded.
pub fn bold_family(ctx: &egui::Context) -> egui::FontFamily {
    let bold = egui::FontFamily::Name(BOLD_FAMILY.into());
    if ctx.fonts(|f| f.definitions().families.contains_key(&bold)) {
        bold
    } else {
        egui::FontFamily::Proportional
    }
}

/// Bold title text (component names, sub-section headers).
pub fn bold(ctx: &egui::Context, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .family(bold_family(ctx))
        .color(text_strong())
}

/// The UI fonts: the user's choice (Preferences ▸ Appearance) or the OS UI font, with symbol
/// fonts as glyph fallback and the built-in Arabic font for Arabic letters.
pub fn load_fonts(ctx: &egui::Context) {
    let (defs, _) = fonts::definitions(&fonts::FontPrefs::load());
    ctx.set_fonts(defs);
}

/// Startup look: the saved theme (or `MISTY_THEME=<preset or custom name>`), fonts and text
/// size.
pub fn apply(ctx: &egui::Context) {
    fonts::apply(ctx, &fonts::FontPrefs::load());
    let palette = custom::startup_palette();
    apply_palette(ctx, &palette);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 3.0);
        s.spacing.button_padding = egui::vec2(6.0, 2.0);
        s.spacing.interact_size.y = 18.0;
    });
}

/// Make `p` the active theme: the token colours and egui's visuals (applies live).
pub fn apply_palette(ctx: &egui::Context, p: &Palette) {
    set_current(*p);
    ctx.set_visuals(visuals(p));
}

/// egui's visuals for a palette.
pub fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.is_dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.override_text_color = Some(p.text);
    v.panel_fill = p.panel_bg;
    v.window_fill = p.popup_bg;
    v.extreme_bg_color = p.field_bg;
    v.faint_bg_color = p.shade(Color32::from_rgb(62, 62, 62));
    v.window_stroke = Stroke::NONE;
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0, p.selection_text);
    v.hyperlink_color = p.accent;
    if !p.is_dark() {
        // A soft shadow under popups stands in for the outline on light panels.
        v.window_stroke = Stroke::new(1.0, p.border);
    }

    let r = CornerRadius::same(3);
    v.widgets.noninteractive.bg_fill = p.panel_bg;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.divider);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.bg_fill = p.button_bg;
    v.widgets.inactive.weak_bg_fill = p.button_bg;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.bg_fill = p.hover_bg;
    v.widgets.hovered.weak_bg_fill = p.hover_bg;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.bg_fill = p.pressed;
    v.widgets.active.weak_bg_fill = p.pressed;
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.active.corner_radius = r;
    let open = p.shade(Color32::from_rgb(70, 70, 70));
    v.widgets.open.bg_fill = open;
    v.widgets.open.weak_bg_fill = open;
    v.widgets.open.bg_stroke = Stroke::NONE;
    v.widgets.open.corner_radius = r;
    v
}

/// Dock style: dark tab strip, lighter active tab, thin separators.
pub fn dock_style(ctx: &egui::Context) -> egui_dock::Style {
    let p = current();
    let mut s = egui_dock::Style::from_egui(&ctx.global_style());
    s.main_surface_border_stroke = Stroke::NONE;
    s.separator.width = 2.0;
    s.separator.color_idle = p.border;
    s.separator.color_hovered = p.shade(Color32::from_rgb(80, 80, 80));
    s.separator.color_dragged = p.selection;
    s.tab_bar.bg_fill = p.tab_bar_bg;
    s.tab_bar.height = 22.0;
    s.tab_bar.hline_color = p.tab_bar_bg;
    s.tab.active.bg_fill = p.tab_active;
    s.tab.active.text_color = p.shade(Color32::WHITE);
    s.tab.focused.bg_fill = p.tab_active;
    s.tab.focused.text_color = p.shade(Color32::WHITE);
    s.tab.inactive.bg_fill = p.tab_inactive;
    s.tab.inactive.text_color = p.text_dim;
    s.tab.hovered.bg_fill = p.shade(Color32::from_rgb(52, 52, 52));
    s.tab.hovered.text_color = p.text;
    s.tab.tab_body.bg_fill = p.panel_bg;
    s.tab.tab_body.stroke = Stroke::NONE;
    for t in [
        &mut s.tab.active,
        &mut s.tab.inactive,
        &mut s.tab.focused,
        &mut s.tab.hovered,
        &mut s.tab.inactive_with_kb_focus,
        &mut s.tab.active_with_kb_focus,
        &mut s.tab.focused_with_kb_focus,
    ] {
        t.outline_color = Color32::TRANSPARENT;
    }
    s.tab.hline_below_active_tab_name = false;
    s.buttons.add_tab_border_color = Color32::TRANSPARENT;
    s.buttons.close_all_tabs_border_color = Color32::TRANSPARENT;
    s.buttons.collapse_tabs_border_color = Color32::TRANSPARENT;
    s.buttons.minimize_window_border_color = Color32::TRANSPARENT;
    drop_overlay(&mut s.overlay, p.shade(Color32::from_rgb(96, 160, 230)));
    s
}

/// The guide shown while dragging a tab: soft accent drop targets without the grey border
/// boxes, a rounded tint over the panel under the mouse and a dimmed rest.
fn drop_overlay(o: &mut egui_dock::OverlayStyle, accent: Color32) {
    let tint = |a: u8| Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), a);
    o.button_color = accent;
    o.button_border_stroke = Stroke::NONE;
    o.selection_color = tint(70);
    o.max_button_size = 64.0;
    o.button_spacing = 12.0;
    o.surface_fade_opacity = 0.35;
    o.hovered_leaf_highlight = egui_dock::LeafHighlighting {
        color: tint(18),
        corner_radius: CornerRadius::same(8),
        stroke: Stroke::NONE,
        expansion: -4.0,
    };
}
