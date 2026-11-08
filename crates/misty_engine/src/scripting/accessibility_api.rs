//! Lua `Accessibility`: the UI text scale and the colour-blind filter, for a game's options
//! menu. The game starts with Project Settings ▸ Player ▸ Accessibility; changes last until the
//! game quits (save them with `PlayerPrefs` to keep them).
//!
//! ```lua
//! Accessibility.textScale = 1.25
//! Accessibility.colorBlindMode = "Deuteranopia"   -- "Off", "Protanopia", "Tritanopia"
//! Accessibility.filterStrength = 0.8
//! ```

use mlua::{Lua, Result, UserData, UserDataFields};

use crate::accessibility::{self, ColorBlindMode, MAX_TEXT_SCALE, MIN_TEXT_SCALE};

struct LAccessibility;

impl UserData for LAccessibility {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        // Multiplies every UI Text's font size (0.5 to 3; 1 = as designed).
        f.add_field_method_get("textScale", |_, _| Ok(accessibility::text_scale()));
        f.add_field_method_set("textScale", |_, _, v: f32| {
            accessibility::update(|s| s.text_scale = v);
            Ok(())
        });
        // The colour-blind filter: "Off", "Protanopia", "Deuteranopia" or "Tritanopia".
        f.add_field_method_get("colorBlindMode", |_, _| {
            Ok(accessibility::current().color_filter.label())
        });
        f.add_field_method_set("colorBlindMode", |_, _, name: String| {
            let mode = ColorBlindMode::from_name(&name).ok_or_else(|| {
                mlua::Error::runtime(format!(
                    "Accessibility.colorBlindMode: unknown mode {name:?} (Off, Protanopia, \
                     Deuteranopia or Tritanopia)"
                ))
            })?;
            accessibility::update(|s| s.color_filter = mode);
            Ok(())
        });
        // How strongly the filter applies (0 to 1).
        f.add_field_method_get("filterStrength", |_, _| {
            Ok(accessibility::current().filter_strength)
        });
        f.add_field_method_set("filterStrength", |_, _, v: f32| {
            accessibility::update(|s| s.filter_strength = v);
            Ok(())
        });
        // Show the image as a colour-blind player sees it instead of correcting it (for testing).
        f.add_field_method_get("simulate", |_, _| Ok(accessibility::current().simulate));
        f.add_field_method_set("simulate", |_, _, on: bool| {
            accessibility::update(|s| s.simulate = on);
            Ok(())
        });
        // The filter mode names, for an options menu.
        f.add_field_method_get("colorBlindModes", |_, _| {
            Ok(ColorBlindMode::ALL.map(ColorBlindMode::label).to_vec())
        });
        // Smallest and largest text scale.
        f.add_field_method_get("minTextScale", |_, _| Ok(MIN_TEXT_SCALE));
        f.add_field_method_get("maxTextScale", |_, _| Ok(MAX_TEXT_SCALE));
    }
}

pub fn register(lua: &Lua) -> Result<()> {
    lua.globals().set("Accessibility", LAccessibility)
}

#[cfg(test)]
mod tests {
    use crate::accessibility::{self, ColorBlindMode};

    #[test]
    fn scripts_read_and_change_the_settings() {
        let lua = mlua::Lua::new();
        super::register(&lua).unwrap();
        accessibility::install(&Default::default());
        lua.load(
            r#"
            assert(Accessibility.textScale == 1)
            assert(Accessibility.colorBlindMode == "Off")
            Accessibility.textScale = 1.5
            Accessibility.colorBlindMode = "deuteranopia"
            Accessibility.filterStrength = 2
            Accessibility.simulate = true
            assert(#Accessibility.colorBlindModes == 4)
            assert(Accessibility.maxTextScale == 3)
            "#,
        )
        .exec()
        .unwrap();
        let s = accessibility::current();
        assert_eq!(s.text_scale, 1.5);
        assert_eq!(s.color_filter, ColorBlindMode::Deuteranopia);
        assert_eq!(s.filter_strength, 1.0, "clamped");
        assert!(s.simulate);
        let err = lua
            .load(r#"Accessibility.colorBlindMode = "Purple""#)
            .exec()
            .unwrap_err();
        assert!(err.to_string().contains("unknown mode"), "{err}");
        accessibility::install(&Default::default());
    }
}
