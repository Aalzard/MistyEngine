//! Generic editors for a preset's stored values (serde JSON): checkboxes, numbers, text, colours,
//! small vectors, enum dropdowns and foldouts for nested structs and lists. Enum variants are
//! read from the deserializer's "expected one of" message, so every component type works
//! without its own preset editor.

use egui::{Id, Ui};
use misty_engine::preset::Preset;
use serde_json::{Map, Number, Value};

/// One step from the values root to a field.
#[derive(Clone, Debug, Hash)]
pub enum Seg {
    Key(String),
    Index(usize),
}

/// Lookups shared by every field of one preset.
pub struct Fields<'a> {
    /// The preset as it was at the start of the frame (for enum discovery).
    pub preset: &'a Preset,
}

/// "spot_angle" → "Spot Angle".
pub fn nice(key: &str) -> String {
    key.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Fields<'_> {
    /// Edit `v` (the field at `path`); true when it changed.
    pub fn value(&self, ui: &mut Ui, path: &mut Vec<Seg>, key: &str, v: &mut Value) -> bool {
        match v {
            Value::Bool(b) => ui.checkbox(b, "").changed(),
            Value::Number(n) => number(ui, n),
            Value::String(text) => self.string(ui, path, text),
            Value::Null => {
                ui.weak("None");
                false
            }
            Value::Array(a) if is_color(key, a) => color(ui, a),
            Value::Array(a) if a.len() <= 4 && a.iter().all(is_scalar) => {
                let mut ch = false;
                ui.horizontal(|ui| {
                    for (i, x) in a.iter_mut().enumerate() {
                        path.push(Seg::Index(i));
                        ch |= self.value(ui, path, key, x);
                        path.pop();
                    }
                });
                ch
            }
            Value::Array(a) => {
                let id = Id::new(("preset_list", self.preset.target.as_str(), &*path));
                let mut ch = false;
                egui::CollapsingHeader::new(format!("{} items", a.len()))
                    .id_salt(id)
                    .show(ui, |ui| {
                        for (i, x) in a.iter_mut().enumerate() {
                            path.push(Seg::Index(i));
                            ch |= self.row(ui, path, &format!("Element {i}"), key, x);
                            path.pop();
                        }
                    });
                ch
            }
            Value::Object(m) => self.object(ui, path, m),
        }
    }

    /// A nested struct as a foldout of rows.
    fn object(&self, ui: &mut Ui, path: &mut Vec<Seg>, m: &mut Map<String, Value>) -> bool {
        let id = Id::new(("preset_obj", self.preset.target.as_str(), &*path));
        let mut ch = false;
        egui::CollapsingHeader::new(format!("{} fields", m.len()))
            .id_salt(id)
            .show(ui, |ui| {
                for (k, x) in m.iter_mut() {
                    path.push(Seg::Key(k.clone()));
                    ch |= self.row(ui, path, &nice(k), k, x);
                    path.pop();
                }
            });
        ch
    }

    /// Label + editor.
    pub fn row(
        &self,
        ui: &mut Ui,
        path: &mut Vec<Seg>,
        label: &str,
        key: &str,
        v: &mut Value,
    ) -> bool {
        ui.horizontal_top(|ui| {
            name_label(ui, label);
            self.value(ui, path, key, v)
        })
        .inner
    }

    /// Enum dropdown when the field only takes fixed names, else a text field.
    fn string(&self, ui: &mut Ui, path: &[Seg], text: &mut String) -> bool {
        let id = Id::new(("preset_enum", self.preset.target.as_str(), path));
        let variants = ui.ctx().data_mut(|d| {
            d.get_temp_mut_or_insert_with(id, || variants(self.preset, path))
                .clone()
        });
        let Some(variants) = variants else {
            return ui
                .add(egui::TextEdit::singleline(text).desired_width(160.0))
                .changed();
        };
        let mut ch = false;
        egui::ComboBox::from_id_salt(id)
            .selected_text(text.as_str())
            .show_ui(ui, |ui| {
                for name in &variants {
                    if ui.selectable_label(text == name, name).clicked() && text != name {
                        *text = name.clone();
                        ch = true;
                    }
                }
            });
        ch
    }
}

/// Left-aligned field name in a fixed-width column.
pub fn name_label(ui: &mut Ui, text: &str) {
    let size = egui::vec2(130.0, 18.0);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(size.x);
            ui.add(egui::Label::new(text).truncate());
        },
    );
}

fn is_scalar(v: &Value) -> bool {
    matches!(v, Value::Number(_) | Value::Bool(_))
}

fn is_color(key: &str, a: &[Value]) -> bool {
    let key = key.to_ascii_lowercase();
    (key.contains("color") || key.contains("tint"))
        && matches!(a.len(), 3 | 4)
        && a.iter().all(Value::is_number)
}

/// f32 values written as their shortest decimal (0.7, not 0.699999988).
fn num(x: f32) -> Value {
    format!("{x}")
        .parse::<f64>()
        .ok()
        .and_then(Number::from_f64)
        .map_or(Value::Null, Value::Number)
}

fn number(ui: &mut Ui, n: &mut Number) -> bool {
    if let Some(mut i) = n.as_i64().filter(|_| !n.is_f64()) {
        let ch = ui.add(egui::DragValue::new(&mut i)).changed();
        if ch {
            *n = if n.is_u64() {
                Number::from(i.max(0) as u64)
            } else {
                Number::from(i)
            };
        }
        return ch;
    }
    let mut x = n.as_f64().unwrap_or(0.0) as f32;
    let speed = (x.abs() * 0.01).max(0.01);
    let ch = ui.add(egui::DragValue::new(&mut x).speed(speed)).changed();
    if ch && let Value::Number(v) = num(x) {
        *n = v;
    }
    ch
}

fn color(ui: &mut Ui, a: &mut Vec<Value>) -> bool {
    let f = |i: usize| a[i].as_f64().unwrap_or(1.0) as f32;
    let mut c = [f(0), f(1), f(2), if a.len() == 4 { f(3) } else { 1.0 }];
    let ch = if a.len() == 4 {
        crate::panels::inspector::card::wide(ui, |ui| {
            ui.color_edit_button_rgba_unmultiplied(&mut c)
        })
        .changed()
    } else {
        let mut rgb = [c[0], c[1], c[2]];
        let ch = crate::panels::inspector::card::wide(ui, |ui| ui.color_edit_button_rgb(&mut rgb))
            .changed();
        c[..3].copy_from_slice(&rgb);
        ch
    };
    if ch {
        *a = c[..a.len()].iter().map(|x| num(*x)).collect();
    }
    ch
}

/// The names a string field accepts, if it is an enum: write a value no enum has and read the
/// variants from the error ("unknown variant `X`, expected one of `A`, `B`").
fn variants(preset: &Preset, path: &[Seg]) -> Option<Vec<String>> {
    let mut probe = preset.clone();
    let (Seg::Key(first), rest) = path.split_first()? else {
        return None;
    };
    let mut slot = probe.values.get_mut(first)?;
    for seg in rest {
        slot = match seg {
            Seg::Key(k) => slot.get_mut(k)?,
            Seg::Index(i) => slot.get_mut(*i)?,
        };
    }
    *slot = Value::String("\u{1}".into());
    let err = probe.check().err()?;
    let list = err
        .split("expected one of ")
        .nth(1)
        .or_else(|| err.split("expected ").nth(1))?;
    let names: Vec<String> = list
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    (!names.is_empty() && err.contains("unknown variant")).then_some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_fields_list_their_variants() {
        let mut w = misty_engine::hecs::World::new();
        let e = w.spawn((misty_engine::scene::Light::default(),));
        let p = Preset::capture(&w, e, "Light").unwrap();
        let kinds = variants(&p, &[Seg::Key("kind".into())]).unwrap();
        assert!(kinds.contains(&"Directional".to_owned()) && kinds.contains(&"Spot".to_owned()));
        assert_eq!(nice("spot_angle"), "Spot Angle");
        assert_eq!(num(0.7), serde_json::json!(0.7));
    }
}
