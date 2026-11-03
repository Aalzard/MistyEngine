//! Layer mask dropdown (Camera ▸ Culling Mask): Nothing, Everything, then one toggle per named
//! layer. The layer names are published by the Inspector each frame, so component editors
//! that only see their component can still list them.

use egui::{Id, Ui};
use misty_engine::tags_layers::TagsAndLayers;

fn names_id() -> Id {
    Id::new("misty_named_layers")
}

/// Make the project's layer names available to [`mask_field`] this frame.
pub fn publish_names(ctx: &egui::Context, t: &TagsAndLayers) {
    let names: Vec<(u8, String)> = t
        .named_layers()
        .into_iter()
        .map(|(i, n)| (i, n.to_owned()))
        .collect();
    ctx.data_mut(|d| d.insert_temp(names_id(), names));
}

fn names(ctx: &egui::Context) -> Vec<(u8, String)> {
    ctx.data(|d| d.get_temp(names_id())).unwrap_or_else(|| {
        TagsAndLayers::default()
            .named_layers()
            .into_iter()
            .map(|(i, n)| (i, n.to_owned()))
            .collect()
    })
}

/// "Nothing", "Everything", a layer name, or "Mixed…".
pub fn summary(mask: u32, names: &[(u8, String)]) -> String {
    if mask == u32::MAX {
        return "Everything".into();
    }
    if mask == 0 {
        return "Nothing".into();
    }
    let on: Vec<&str> = names
        .iter()
        .filter(|(i, _)| mask & (1 << i) != 0)
        .map(|(_, n)| n.as_str())
        .collect();
    match on.as_slice() {
        [one] if mask.count_ones() == 1 => (*one).to_owned(),
        _ => "Mixed…".into(),
    }
}

/// The dropdown; true when the mask changed.
pub fn mask_field(ui: &mut Ui, id: &str, mask: &mut u32) -> bool {
    let names = names(ui.ctx());
    let before = *mask;
    egui::ComboBox::from_id_salt(id)
        .selected_text(summary(*mask, &names))
        .width(150.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(*mask == 0, "Nothing").clicked() {
                *mask = 0;
            }
            if ui
                .selectable_label(*mask == u32::MAX, "Everything")
                .clicked()
            {
                *mask = u32::MAX;
            }
            for (i, name) in &names {
                let bit = 1u32 << i;
                let mut on = *mask & bit != 0;
                if ui.checkbox(&mut on, name.as_str()).changed() {
                    *mask = if on { *mask | bit } else { *mask & !bit };
                }
            }
        });
    *mask != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        let names = vec![(0, "Default".to_owned()), (8, "Hidden".to_owned())];
        assert_eq!(summary(u32::MAX, &names), "Everything");
        assert_eq!(summary(0, &names), "Nothing");
        assert_eq!(summary(1 << 8, &names), "Hidden");
        assert_eq!(summary(!(1 << 8), &names), "Mixed…");
    }
}
