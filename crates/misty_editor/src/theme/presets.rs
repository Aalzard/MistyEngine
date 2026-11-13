//! The built-in themes. Dark is the editor's original look, value for value.

use egui::Color32;

use super::Palette;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const fn gray(v: u8) -> Color32 {
    Color32::from_rgb(v, v, v)
}

pub const DARK: Palette = Palette {
    tab_bar_bg: gray(40),
    panel_bg: gray(56),
    tab_active: gray(60),
    tab_inactive: gray(45),
    toolbar_bg: gray(49),
    sidebar_bg: gray(51),
    popup_bg: gray(62),
    card_bg: gray(63),
    card_header: gray(71),
    field_bg: gray(42),
    input_bg: gray(47),
    input_hover: gray(55),
    button_bg: gray(78),
    hover_bg: gray(92),
    border: gray(44),
    divider: gray(48),
    text: gray(210),
    text_dim: gray(130),
    text_strong: gray(232),
    selection_text: Color32::WHITE,
    accent: rgb(76, 126, 255),
    selection: rgb(44, 93, 135),
    pressed: rgb(70, 96, 124),
    prefab: rgb(123, 174, 250),
    prefab_dim: rgb(78, 108, 155),
    overridden: rgb(64, 150, 255),
    play_tint: rgb(70, 110, 170),
    warn: rgb(230, 180, 60),
    error: rgb(232, 96, 86),
    success: rgb(90, 190, 90),
    missing_prefab: rgb(232, 96, 86),
    graph_canvas: rgb(34, 35, 38),
    graph_node: rgb(52, 53, 57),
    graph_header: rgb(66, 66, 70),
    graph_select: rgb(88, 156, 236),
    code_bg: rgb(30, 30, 32),
    code_text: gray(212),
    code_line_no: rgb(110, 118, 129),
    code_current_line: rgb(40, 40, 44),
    code_selection: rgb(38, 79, 120),
};

pub const LIGHT: Palette = Palette {
    tab_bar_bg: rgb(206, 209, 214),
    panel_bg: rgb(234, 235, 238),
    tab_active: rgb(234, 235, 238),
    tab_inactive: rgb(218, 220, 225),
    toolbar_bg: rgb(224, 226, 230),
    sidebar_bg: rgb(228, 230, 234),
    popup_bg: rgb(246, 247, 249),
    card_bg: rgb(242, 243, 246),
    card_header: rgb(226, 228, 233),
    field_bg: rgb(255, 255, 255),
    input_bg: rgb(250, 250, 252),
    input_hover: rgb(242, 244, 248),
    button_bg: rgb(214, 217, 223),
    hover_bg: rgb(198, 203, 212),
    border: rgb(196, 199, 205),
    divider: rgb(212, 214, 219),
    text: rgb(30, 32, 36),
    text_dim: rgb(98, 103, 112),
    text_strong: rgb(8, 9, 11),
    selection_text: rgb(10, 22, 40),
    accent: rgb(28, 98, 200),
    selection: rgb(172, 204, 242),
    pressed: rgb(150, 184, 228),
    prefab: rgb(26, 94, 196),
    prefab_dim: rgb(96, 128, 180),
    overridden: rgb(24, 116, 230),
    play_tint: rgb(120, 160, 222),
    warn: rgb(168, 112, 0),
    error: rgb(196, 44, 36),
    success: rgb(30, 128, 52),
    missing_prefab: rgb(196, 52, 44),
    graph_canvas: rgb(222, 224, 229),
    graph_node: rgb(246, 247, 249),
    graph_header: rgb(226, 229, 235),
    graph_select: rgb(30, 110, 220),
    code_bg: rgb(252, 252, 253),
    code_text: rgb(30, 30, 32),
    code_line_no: rgb(140, 146, 154),
    code_current_line: rgb(239, 242, 247),
    code_selection: rgb(176, 212, 250),
};

pub const HIGH_CONTRAST: Palette = Palette {
    tab_bar_bg: gray(0),
    panel_bg: gray(10),
    tab_active: gray(30),
    tab_inactive: gray(0),
    toolbar_bg: gray(16),
    sidebar_bg: gray(6),
    popup_bg: gray(24),
    card_bg: gray(18),
    card_header: gray(34),
    field_bg: gray(0),
    input_bg: gray(0),
    input_hover: gray(28),
    button_bg: gray(50),
    hover_bg: gray(80),
    border: gray(150),
    divider: gray(110),
    text: gray(255),
    text_dim: gray(200),
    text_strong: gray(255),
    selection_text: gray(0),
    accent: rgb(255, 214, 0),
    selection: rgb(255, 214, 0),
    pressed: rgb(0, 150, 255),
    prefab: rgb(110, 200, 255),
    prefab_dim: rgb(90, 160, 210),
    overridden: rgb(0, 180, 255),
    play_tint: rgb(0, 120, 220),
    warn: rgb(255, 220, 0),
    error: rgb(255, 90, 80),
    success: rgb(80, 240, 110),
    missing_prefab: rgb(255, 100, 90),
    graph_canvas: gray(0),
    graph_node: gray(24),
    graph_header: gray(48),
    graph_select: rgb(255, 214, 0),
    code_bg: gray(0),
    code_text: gray(255),
    code_line_no: gray(190),
    code_current_line: gray(26),
    code_selection: rgb(0, 90, 190),
};

pub const WARM_DARK: Palette = Palette {
    tab_bar_bg: rgb(36, 32, 30),
    panel_bg: rgb(50, 45, 42),
    tab_active: rgb(56, 50, 46),
    tab_inactive: rgb(41, 37, 34),
    toolbar_bg: rgb(44, 40, 37),
    sidebar_bg: rgb(46, 42, 39),
    popup_bg: rgb(60, 54, 50),
    card_bg: rgb(58, 52, 48),
    card_header: rgb(68, 61, 56),
    field_bg: rgb(38, 34, 32),
    input_bg: rgb(43, 39, 36),
    input_hover: rgb(52, 47, 43),
    button_bg: rgb(76, 68, 62),
    hover_bg: rgb(92, 82, 74),
    border: rgb(40, 36, 33),
    divider: rgb(44, 40, 37),
    text: rgb(228, 218, 204),
    text_dim: rgb(150, 138, 124),
    text_strong: rgb(246, 238, 226),
    selection_text: rgb(255, 250, 240),
    accent: rgb(230, 146, 72),
    selection: rgb(128, 82, 48),
    pressed: rgb(122, 92, 66),
    prefab: rgb(130, 180, 240),
    prefab_dim: rgb(92, 116, 150),
    overridden: rgb(236, 152, 72),
    play_tint: rgb(150, 100, 60),
    warn: rgb(236, 190, 70),
    error: rgb(236, 100, 84),
    success: rgb(130, 196, 100),
    missing_prefab: rgb(236, 100, 84),
    graph_canvas: rgb(32, 29, 27),
    graph_node: rgb(52, 47, 43),
    graph_header: rgb(68, 61, 56),
    graph_select: rgb(236, 152, 72),
    code_bg: rgb(30, 27, 25),
    code_text: rgb(226, 216, 202),
    code_line_no: rgb(130, 118, 104),
    code_current_line: rgb(42, 38, 35),
    code_selection: rgb(96, 70, 46),
};

pub const MIDNIGHT: Palette = Palette {
    tab_bar_bg: rgb(20, 24, 33),
    panel_bg: rgb(30, 35, 47),
    tab_active: rgb(36, 42, 56),
    tab_inactive: rgb(24, 28, 38),
    toolbar_bg: rgb(26, 31, 42),
    sidebar_bg: rgb(28, 33, 45),
    popup_bg: rgb(40, 46, 62),
    card_bg: rgb(37, 43, 58),
    card_header: rgb(45, 52, 70),
    field_bg: rgb(22, 26, 36),
    input_bg: rgb(26, 30, 41),
    input_hover: rgb(34, 40, 54),
    button_bg: rgb(52, 60, 80),
    hover_bg: rgb(66, 76, 100),
    border: rgb(22, 26, 35),
    divider: rgb(26, 30, 40),
    text: rgb(208, 216, 232),
    text_dim: rgb(124, 136, 160),
    text_strong: rgb(236, 242, 252),
    selection_text: Color32::WHITE,
    accent: rgb(98, 156, 255),
    selection: rgb(46, 86, 150),
    pressed: rgb(62, 92, 140),
    prefab: rgb(130, 184, 255),
    prefab_dim: rgb(84, 112, 160),
    overridden: rgb(90, 160, 255),
    play_tint: rgb(60, 100, 170),
    warn: rgb(236, 190, 80),
    error: rgb(240, 104, 96),
    success: rgb(100, 200, 130),
    missing_prefab: rgb(240, 104, 96),
    graph_canvas: rgb(18, 22, 30),
    graph_node: rgb(34, 40, 54),
    graph_header: rgb(46, 54, 72),
    graph_select: rgb(98, 156, 255),
    code_bg: rgb(17, 20, 28),
    code_text: rgb(208, 216, 232),
    code_line_no: rgb(100, 112, 136),
    code_current_line: rgb(26, 31, 42),
    code_selection: rgb(40, 74, 126),
};

/// A built-in theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Dark,
    Light,
    HighContrast,
    WarmDark,
    Midnight,
}

impl Preset {
    pub const ALL: [Preset; 5] = [
        Preset::Dark,
        Preset::Light,
        Preset::HighContrast,
        Preset::WarmDark,
        Preset::Midnight,
    ];

    /// The id saved in preferences and `.mistytheme` files ("dark", "light" …).
    pub fn id(self) -> &'static str {
        match self {
            Preset::Dark => "dark",
            Preset::Light => "light",
            Preset::HighContrast => "high_contrast",
            Preset::WarmDark => "warm_dark",
            Preset::Midnight => "midnight",
        }
    }

    pub fn from_id(id: &str) -> Option<Preset> {
        let id = id.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        Preset::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The translated name shown in Preferences.
    pub fn label(self) -> String {
        use crate::i18n::tr;
        match self {
            Preset::Dark => tr!("Dark"),
            Preset::Light => tr!("Light#theme"),
            Preset::HighContrast => tr!("High Contrast"),
            Preset::WarmDark => tr!("Warm Dark"),
            Preset::Midnight => tr!("Midnight"),
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Preset::Dark => DARK,
            Preset::Light => LIGHT,
            Preset::HighContrast => HIGH_CONTRAST,
            Preset::WarmDark => WARM_DARK,
            Preset::Midnight => MIDNIGHT,
        }
    }
}
