//! Project Settings ▸ Player: the game's name, icon, splash screen and how its window opens.
//! The standalone player applies all of it at startup.

use serde::{Deserialize, Serialize};

/// How the game window starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenMode {
    /// Borderless window covering the whole monitor (fast Alt+Tab).
    #[default]
    FullscreenWindow,
    /// The monitor switches to the game's resolution.
    ExclusiveFullscreen,
    /// A normal window of the default size.
    Windowed,
    /// A normal window, maximised.
    Maximized,
}

impl ScreenMode {
    pub const ALL: [Self; 4] = [
        Self::FullscreenWindow,
        Self::ExclusiveFullscreen,
        Self::Windowed,
        Self::Maximized,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::FullscreenWindow => "Fullscreen Window",
            Self::ExclusiveFullscreen => "Exclusive Fullscreen",
            Self::Windowed => "Windowed",
            Self::Maximized => "Maximized Window",
        }
    }

    pub fn is_fullscreen(self) -> bool {
        matches!(self, Self::FullscreenWindow | Self::ExclusiveFullscreen)
    }
}

/// Splash screen shown before the first scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SplashScreen {
    pub enabled: bool,
    /// Project-relative PNG/JPG ("Assets/Splash.png").
    pub image: Option<String>,
    /// Seconds on screen.
    pub duration: f32,
    /// sRGB background around the image.
    pub background: [f32; 3],
}

impl Default for SplashScreen {
    fn default() -> Self {
        Self {
            enabled: false,
            image: None,
            duration: 2.0,
            background: [0.0, 0.0, 0.0],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerSettings {
    /// Window title, executable and data folder name; empty = the project folder's name.
    pub product_name: String,
    /// Used for the per-user save folder (`<Company>/<Product>`).
    pub company_name: String,
    pub version: String,
    /// Project-relative PNG used as the executable and window icon.
    pub icon: Option<String>,
    pub splash: SplashScreen,
    pub screen_mode: ScreenMode,
    /// Window size in Windowed mode (and the Exclusive Fullscreen resolution).
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    /// Keep playing when the window loses focus (else the game pauses).
    pub run_in_background: bool,
    /// Wait for the display's refresh (no tearing).
    pub vsync: bool,
    /// Frame cap when VSync is off; -1 = unlimited.
    pub target_frame_rate: i32,
    /// UI text scale and the colour-blind filter the game starts with (scripts change them at
    /// run time through `Accessibility`).
    pub accessibility: crate::accessibility::AccessibilitySettings,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            product_name: String::new(),
            company_name: "DefaultCompany".into(),
            version: "1.0".into(),
            icon: None,
            splash: SplashScreen::default(),
            screen_mode: ScreenMode::FullscreenWindow,
            width: 1280,
            height: 720,
            resizable: true,
            run_in_background: false,
            vsync: true,
            target_frame_rate: -1,
            accessibility: Default::default(),
        }
    }
}

impl PlayerSettings {
    pub fn sanitize(&mut self) {
        self.width = self.width.clamp(64, 16384);
        self.height = self.height.clamp(64, 16384);
        if self.target_frame_rate < -1 || self.target_frame_rate == 0 {
            self.target_frame_rate = -1;
        }
        self.target_frame_rate = self.target_frame_rate.min(1000);
        let d = self.splash.duration;
        self.splash.duration = if d.is_finite() {
            d.clamp(0.0, 30.0)
        } else {
            2.0
        };
        self.accessibility.sanitize();
        self.icon = self.icon.take().filter(|p| !p.trim().is_empty());
        self.splash.image = self.splash.image.take().filter(|p| !p.trim().is_empty());
    }

    /// Settings from a project made before the Player page: the old Build Settings window held
    /// the product name, size and a fullscreen box.
    pub fn from_legacy(build: &crate::build::BuildSettings) -> Self {
        Self {
            product_name: build.product_name.clone(),
            width: build.width,
            height: build.height,
            screen_mode: if build.fullscreen {
                ScreenMode::FullscreenWindow
            } else {
                ScreenMode::Windowed
            },
            ..Default::default()
        }
    }

    /// Product name, or the project folder's name when none is set.
    pub fn product_name_or(&self, root: &std::path::Path) -> String {
        let name = self.product_name.trim();
        if !name.is_empty() {
            return name.to_owned();
        }
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Misty Game".into())
    }

    /// Frame time to wait for when VSync is off and a target is set.
    pub fn frame_interval(&self) -> Option<std::time::Duration> {
        (!self.vsync && self.target_frame_rate > 0)
            .then(|| std::time::Duration::from_secs_f64(1.0 / self.target_frame_rate as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::BuildSettings;

    #[test]
    fn legacy_build_settings_become_player_settings() {
        let b = BuildSettings {
            product_name: "Rolling Ball".into(),
            width: 800,
            height: 600,
            fullscreen: false,
            ..Default::default()
        };
        let p = PlayerSettings::from_legacy(&b);
        assert_eq!(p.product_name, "Rolling Ball");
        assert_eq!((p.width, p.height), (800, 600));
        assert_eq!(p.screen_mode, ScreenMode::Windowed);
    }

    #[test]
    fn sanitize_and_frame_interval() {
        let mut p = PlayerSettings {
            target_frame_rate: 0,
            width: 1,
            icon: Some(" ".into()),
            ..Default::default()
        };
        p.sanitize();
        assert_eq!(p.target_frame_rate, -1);
        assert_eq!(p.width, 64);
        assert!(p.icon.is_none());
        assert!(p.frame_interval().is_none(), "vsync on");
        p.vsync = false;
        p.target_frame_rate = 50;
        assert_eq!(p.frame_interval().unwrap().as_millis(), 20);
    }
}
