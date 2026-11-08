//! Player accessibility: the UI text scale and the colour-blind filter (a full-screen colour
//! matrix over the final image, UI included). Defaults come from Project Settings ▸ Player ▸
//! Accessibility; scripts change them at run time through the Lua `Accessibility` table (a game's
//! options menu). Like [`crate::quality`] this is process-wide state the host installs, so it
//! works the same in the player and the editor's Play mode.

use serde::{Deserialize, Serialize};

/// Which colour vision deficiency the filter is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColorBlindMode {
    #[default]
    Off,
    /// Red-blind (no long-wavelength cones).
    Protanopia,
    /// Green-blind (no medium-wavelength cones), the most common.
    Deuteranopia,
    /// Blue-blind (no short-wavelength cones).
    Tritanopia,
}

impl ColorBlindMode {
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::Protanopia,
        Self::Deuteranopia,
        Self::Tritanopia,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Protanopia => "Protanopia",
            Self::Deuteranopia => "Deuteranopia",
            Self::Tritanopia => "Tritanopia",
        }
    }

    /// From a name, ignoring case ("deuteranopia", "Off").
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|m| m.label().eq_ignore_ascii_case(name.trim()))
    }
}

/// Project Settings ▸ Player ▸ Accessibility, and the values scripts change at run time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessibilitySettings {
    /// Multiplies every UI Text's font size (1 = as designed).
    pub text_scale: f32,
    pub color_filter: ColorBlindMode,
    /// 0 = no change, 1 = the full filter.
    pub filter_strength: f32,
    /// Show how the image looks with the deficiency instead of correcting for it (for checking
    /// a game's colours while making it).
    pub simulate: bool,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            text_scale: 1.0,
            color_filter: ColorBlindMode::Off,
            filter_strength: 1.0,
            simulate: false,
        }
    }
}

pub const MIN_TEXT_SCALE: f32 = 0.5;
pub const MAX_TEXT_SCALE: f32 = 3.0;

impl AccessibilitySettings {
    pub fn sanitize(&mut self) {
        self.text_scale = if self.text_scale.is_finite() {
            self.text_scale.clamp(MIN_TEXT_SCALE, MAX_TEXT_SCALE)
        } else {
            1.0
        };
        self.filter_strength = if self.filter_strength.is_finite() {
            self.filter_strength.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    /// The colour matrix for the filter, or `None` when it changes nothing.
    pub fn filter_matrix(&self) -> Option<Mat3> {
        filter_matrix(self.color_filter, self.filter_strength, self.simulate)
    }
}

/// Row-major 3×3 matrix on linear RGB.
pub type Mat3 = [[f32; 3]; 3];

const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Machado, Oliveira and Fernandes (2009) dichromacy simulation, severity 1, linear RGB.
fn simulation(mode: ColorBlindMode) -> Mat3 {
    match mode {
        ColorBlindMode::Off => IDENTITY,
        ColorBlindMode::Protanopia => [
            [0.152_286, 1.052_583, -0.204_868],
            [0.114_503, 0.786_281, 0.099_216],
            [-0.003_882, -0.048_116, 1.051_998],
        ],
        ColorBlindMode::Deuteranopia => [
            [0.367_322, 0.860_646, -0.227_968],
            [0.280_085, 0.672_501, 0.047_413],
            [-0.011_820, 0.042_940, 0.968_881],
        ],
        ColorBlindMode::Tritanopia => [
            [1.255_528, -0.076_749, -0.178_779],
            [-0.078_411, 0.930_809, 0.147_602],
            [0.004_733, 0.691_367, 0.303_900],
        ],
    }
}

/// Where the colour information a viewer can't see is moved to (daltonisation): red–green
/// losses into the green and blue channels, blue–yellow losses into red and green.
fn error_shift(mode: ColorBlindMode) -> Mat3 {
    match mode {
        ColorBlindMode::Tritanopia => [[1.0, 0.0, 0.7], [0.0, 1.0, 0.7], [0.0, 0.0, 0.0]],
        _ => [[0.0, 0.0, 0.0], [0.7, 1.0, 0.0], [0.7, 0.0, 1.0]],
    }
}

fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    m
}

fn lerp(a: &Mat3, b: &Mat3, t: f32) -> Mat3 {
    combine(a, b, |x, y| x + (y - x) * t)
}

/// Element-wise `f(a, b)`.
fn combine(a: &Mat3, b: &Mat3, f: impl Fn(f32, f32) -> f32) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for ((row, ra), rb) in m.iter_mut().zip(a).zip(b) {
        for ((v, x), y) in row.iter_mut().zip(ra).zip(rb) {
            *v = f(*x, *y);
        }
    }
    m
}

/// The filter as one matrix: correction `c + E·(c − S·c)` = `I + E·(I − S)`, or the simulation
/// `S`, blended with the identity by `strength`. `None` when it is the identity.
pub fn filter_matrix(mode: ColorBlindMode, strength: f32, simulate: bool) -> Option<Mat3> {
    let strength = if strength.is_finite() {
        strength.clamp(0.0, 1.0)
    } else {
        1.0
    };
    if mode == ColorBlindMode::Off || strength <= 0.0 {
        return None;
    }
    let s = simulation(mode);
    let full = if simulate {
        s
    } else {
        let shifted = mul(&error_shift(mode), &combine(&IDENTITY, &s, |i, x| i - x));
        combine(&IDENTITY, &shifted, |i, x| i + x)
    };
    Some(lerp(&IDENTITY, &full, strength))
}

/// `m · rgb`.
pub fn apply(m: &Mat3, rgb: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0; 3];
    for (r, v) in out.iter_mut().enumerate() {
        *v = (0..3).map(|k| m[r][k] * rgb[k]).sum();
    }
    out
}

// ---- Process-wide state ------------------------------------------------------------------------
//
// Unit tests of this crate keep the state per thread, so a test changing the text scale can't
// resize the text another test running alongside it is measuring.

#[cfg(not(test))]
static STATE: std::sync::RwLock<Option<AccessibilitySettings>> = std::sync::RwLock::new(None);

#[cfg(test)]
thread_local! {
    static STATE: std::cell::RefCell<Option<AccessibilitySettings>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(not(test))]
fn with<R>(f: impl FnOnce(&mut Option<AccessibilitySettings>) -> R) -> R {
    f(&mut STATE.write().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(test)]
fn with<R>(f: impl FnOnce(&mut Option<AccessibilitySettings>) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Use these settings (the project's at startup and when Play starts).
pub fn install(settings: &AccessibilitySettings) {
    let mut s = *settings;
    s.sanitize();
    with(|state| *state = Some(s));
}

/// The settings in effect.
pub fn current() -> AccessibilitySettings {
    with(|state| state.unwrap_or_default())
}

/// Change the settings in effect (Lua).
pub fn update(f: impl FnOnce(&mut AccessibilitySettings)) {
    with(|state| {
        let mut s = state.unwrap_or_default();
        f(&mut s);
        s.sanitize();
        *state = Some(s);
    });
}

/// The UI text scale in effect.
pub fn text_scale() -> f32 {
    current().text_scale
}

/// A UI Text's on-screen font size for its designed size.
pub fn scaled_font_size(size: f32) -> f32 {
    size * text_scale()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        a.iter()
            .zip(&b)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt()
    }

    #[test]
    fn off_or_zero_strength_is_no_filter() {
        assert_eq!(filter_matrix(ColorBlindMode::Off, 1.0, false), None);
        assert_eq!(filter_matrix(ColorBlindMode::Protanopia, 0.0, false), None);
        assert!(filter_matrix(ColorBlindMode::Tritanopia, 0.5, true).is_some());
    }

    #[test]
    fn simulation_rows_keep_grey_grey() {
        for mode in [
            ColorBlindMode::Protanopia,
            ColorBlindMode::Deuteranopia,
            ColorBlindMode::Tritanopia,
        ] {
            for simulate in [true, false] {
                let m = filter_matrix(mode, 1.0, simulate).unwrap();
                let grey = apply(&m, [0.5, 0.5, 0.5]);
                assert!(
                    dist(grey, [0.5, 0.5, 0.5]) < 0.01,
                    "{mode:?} {simulate}: {grey:?}"
                );
            }
        }
    }

    #[test]
    fn correction_separates_red_and_green_for_a_deuteranope() {
        let red = [0.8, 0.1, 0.1];
        let green = [0.1, 0.6, 0.1];
        let sim = filter_matrix(ColorBlindMode::Deuteranopia, 1.0, true).unwrap();
        let fix = filter_matrix(ColorBlindMode::Deuteranopia, 1.0, false).unwrap();
        // How far apart a deuteranope sees them, before and after the correction.
        let before = dist(apply(&sim, red), apply(&sim, green));
        let after = dist(
            apply(&sim, apply(&fix, red)),
            apply(&sim, apply(&fix, green)),
        );
        assert!(after > before * 1.2, "before {before}, after {after}");
        // Half strength sits between.
        let half = filter_matrix(ColorBlindMode::Deuteranopia, 0.5, false).unwrap();
        let mid = apply(&half, red);
        let full = apply(&fix, red);
        assert!(dist(mid, red) < dist(full, red) && dist(mid, red) > 0.0);
    }

    #[test]
    fn state_installs_updates_and_sanitizes() {
        assert_eq!(current(), AccessibilitySettings::default());
        install(&AccessibilitySettings {
            text_scale: 9.0,
            color_filter: ColorBlindMode::Protanopia,
            ..Default::default()
        });
        assert_eq!(text_scale(), MAX_TEXT_SCALE);
        assert_eq!(current().color_filter, ColorBlindMode::Protanopia);
        update(|s| s.text_scale = 1.5);
        assert_eq!(scaled_font_size(20.0), 30.0);
        install(&AccessibilitySettings::default());
        assert_eq!(text_scale(), 1.0);
        assert_eq!(
            ColorBlindMode::from_name(" deuteranopia"),
            Some(ColorBlindMode::Deuteranopia)
        );
        assert_eq!(ColorBlindMode::from_name("nope"), None);
    }

    #[test]
    fn text_scale_grows_ui_text() {
        let mut scene = crate::scene::Scene::default_3d("Text");
        let e = scene.spawn("Label", None);
        scene.add(
            e,
            crate::scene::UiText {
                text: "Hello there".into(),
                wrap: false,
                ..Default::default()
            },
        );
        let width = || crate::ui::auto_layout::sizes(&scene, e, 0, None).preferred;
        install(&AccessibilitySettings::default());
        let before = width();
        update(|s| s.text_scale = 2.0);
        let after = width();
        install(&AccessibilitySettings::default());
        assert!(before > 0.0);
        assert!((after / before - 2.0).abs() < 0.2, "{before} → {after}");
    }

    #[test]
    fn old_settings_files_get_defaults() {
        let s: AccessibilitySettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, AccessibilitySettings::default());
        let s: AccessibilitySettings =
            serde_json::from_str(r#"{"color_filter":"Tritanopia"}"#).unwrap();
        assert_eq!(s.color_filter, ColorBlindMode::Tritanopia);
    }
}
