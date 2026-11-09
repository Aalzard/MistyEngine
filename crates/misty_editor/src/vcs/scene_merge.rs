//! Merge helper for a conflicted scene: the base, ours and theirs versions compared object by
//! object; each object takes ours or theirs (objects only one side changed start on that side,
//! conflicts on ours) and the merged scene is written as one file.

use super::scene_diff::{FieldChange, object_change};
use super::scene_doc::{Obj, SceneDoc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Ours,
    Theirs,
}

/// How an object differs between the two sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Both sides agree (nothing to pick).
    Same,
    OursChanged,
    TheirsChanged,
    /// Both sides changed it differently.
    Conflict,
}

/// One object (or the scene settings) to pick a side for.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Hierarchy path, or [`SETTINGS`].
    pub key: String,
    pub state: State,
    pub pick: Side,
    /// One-line summaries of each side's change against the base.
    pub ours: String,
    pub theirs: String,
}

/// The item key standing for the lighting / navigation blocks.
pub const SETTINGS: &str = "(Scene settings)";

#[derive(Clone, Debug, PartialEq)]
pub struct SceneMerge {
    pub base: SceneDoc,
    pub ours: SceneDoc,
    pub theirs: SceneDoc,
    /// Every object either side has (Hierarchy order of ours, then theirs' additions), then the
    /// settings.
    pub items: Vec<Item>,
}

impl SceneMerge {
    /// Compare the three versions' texts (`base` may be empty when both sides added the file).
    pub fn new(base: &str, ours: &str, theirs: &str) -> Result<SceneMerge, String> {
        let base = if base.trim().is_empty() {
            SceneDoc::default()
        } else {
            SceneDoc::parse(base)?
        };
        let (ours, theirs) = (SceneDoc::parse(ours)?, SceneDoc::parse(theirs)?);
        let mut keys: Vec<String> = ours.objects.iter().map(|o| o.key.clone()).collect();
        for o in &theirs.objects {
            if !keys.contains(&o.key) {
                keys.push(o.key.clone());
            }
        }
        // Deleted on both sides: nothing to show.
        let mut items: Vec<Item> = keys
            .into_iter()
            .map(|k| {
                let (b, o, t) = (base.get(&k), ours.get(&k), theirs.get(&k));
                item(
                    &k,
                    b.map(|x| &x.components),
                    o.map(|x| &x.components),
                    t.map(|x| &x.components),
                    |x, y| summary(&k, x, y),
                )
            })
            .collect();
        let (bs, os, ts) = (&base.settings, &ours.settings, &theirs.settings);
        items.push(item(SETTINGS, Some(bs), Some(os), Some(ts), |x, y| {
            let mut changes = Vec::new();
            let (x, y) = (
                x.cloned().unwrap_or_default(),
                y.cloned().unwrap_or_default(),
            );
            let wrap = |m: std::collections::BTreeMap<String, serde_json::Value>| Obj {
                key: SETTINGS.into(),
                parent: None,
                name: SETTINGS.into(),
                components: m,
            };
            if let Some(c) = object_change(&wrap(x), &wrap(y)) {
                changes = c.fields;
            }
            fields_summary(&changes)
        }));
        Ok(SceneMerge {
            base,
            ours,
            theirs,
            items,
        })
    }

    pub fn conflicts(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.state == State::Conflict)
            .count()
    }

    /// Items worth showing (both sides agreeing needs no choice).
    pub fn choices(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| i.state != State::Same)
    }

    #[cfg(test)]
    pub fn set(&mut self, key: &str, side: Side) {
        if let Some(i) = self.items.iter_mut().find(|i| i.key == key) {
            i.pick = side;
        }
    }

    /// Pick one side for every object.
    pub fn set_all(&mut self, side: Side) {
        for i in &mut self.items {
            i.pick = side;
        }
    }

    /// The merged scene: each object from its picked side (left out when that side deleted it).
    pub fn merged(&self) -> SceneDoc {
        let mut objects: Vec<Obj> = Vec::new();
        for it in self.items.iter().filter(|i| i.key != SETTINGS) {
            let from = match it.pick {
                Side::Ours => &self.ours,
                Side::Theirs => &self.theirs,
            };
            if let Some(o) = from.get(&it.key) {
                objects.push(o.clone());
            }
        }
        let settings = match self
            .items
            .iter()
            .find(|i| i.key == SETTINGS)
            .map(|i| i.pick)
        {
            Some(Side::Theirs) => self.theirs.settings.clone(),
            _ => self.ours.settings.clone(),
        };
        SceneDoc { settings, objects }
    }

    pub fn merged_text(&self) -> String {
        self.merged().write()
    }
}

fn item<T: PartialEq>(
    key: &str,
    b: Option<&T>,
    o: Option<&T>,
    t: Option<&T>,
    summary: impl Fn(Option<&T>, Option<&T>) -> String,
) -> Item {
    let state = if o == t {
        State::Same
    } else if o == b {
        State::TheirsChanged
    } else if t == b {
        State::OursChanged
    } else {
        State::Conflict
    };
    let pick = match state {
        State::TheirsChanged => Side::Theirs,
        _ => Side::Ours,
    };
    Item {
        key: key.to_owned(),
        state,
        pick,
        ours: if o == b {
            "unchanged".into()
        } else {
            summary(b, o)
        },
        theirs: if t == b {
            "unchanged".into()
        } else {
            summary(b, t)
        },
    }
}

type Components = std::collections::BTreeMap<String, serde_json::Value>;

/// What one side did to an object compared with the base.
fn summary(key: &str, base: Option<&Components>, side: Option<&Components>) -> String {
    match (base, side) {
        (None, Some(_)) => "added".into(),
        (Some(_), None) => "deleted".into(),
        (None, None) => "absent".into(),
        (Some(b), Some(s)) => {
            let wrap = |m: &Components| Obj {
                key: key.to_owned(),
                parent: None,
                name: String::new(),
                components: m.clone(),
            };
            let Some(c) = object_change(&wrap(b), &wrap(s)) else {
                return "unchanged".into();
            };
            let mut parts: Vec<String> = Vec::new();
            parts.extend(c.components_added.iter().map(|n| format!("+{n}")));
            parts.extend(c.components_removed.iter().map(|n| format!("-{n}")));
            let fields = fields_summary(&c.fields);
            if !fields.is_empty() {
                parts.push(fields);
            }
            parts.join(", ")
        }
    }
}

fn fields_summary(fields: &[FieldChange]) -> String {
    match fields {
        [] => String::new(),
        [one] => one.text(),
        [first, rest @ ..] => format!("{} (+{} more)", first.text(), rest.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::scene_doc::tests::sample;
    use super::*;

    fn versions() -> (String, String, String) {
        let base = sample();
        // Ours: camera fov 70, Player moved, a new Light.
        let ours = base
            .replace(r#""fov": 60.0"#, r#""fov": 70.0"#)
            .replace("[0.0, 1.0, 0.0]", "[5.0, 1.0, 0.0]")
            .replace(
                r#""parent": null }
  ],"#,
                r#""parent": null },
    { "components": { "GameObject": { "name": "Light" } }, "parent": null }
  ],"#,
            );
        // Theirs: camera fov 90 (conflict), the second Cube deleted, fog on.
        let theirs = base
            .replace(r#""fov": 60.0"#, r#""fov": 90.0"#)
            .replace(
                r#",
    { "components": { "GameObject": { "name": "Cube" } }, "parent": null }
  ],"#,
                "\n  ],",
            )
            .replace(r#""enabled": false"#, r#""enabled": true"#);
        (base, ours, theirs)
    }

    #[test]
    fn states_and_default_picks() {
        let (b, o, t) = versions();
        let m = SceneMerge::new(&b, &o, &t).unwrap();
        let get = |k: &str| m.items.iter().find(|i| i.key == k).unwrap().clone();
        assert_eq!(get("Main Camera").state, State::Conflict);
        assert_eq!(get("Main Camera").pick, Side::Ours);
        assert_eq!(get("Main Camera").ours, "Camera.fov: 60 → 70");
        assert_eq!(get("Main Camera").theirs, "Camera.fov: 60 → 90");
        assert_eq!(get("Player").state, State::OursChanged);
        assert_eq!(get("Light").state, State::OursChanged);
        assert_eq!(get("Light").ours, "added");
        assert_eq!(get("Cube [2]").state, State::TheirsChanged);
        assert_eq!(get("Cube [2]").pick, Side::Theirs);
        assert_eq!(get("Cube [2]").theirs, "deleted");
        assert_eq!(get(SETTINGS).state, State::TheirsChanged);
        assert_eq!(get("Button").state, State::Same);
        assert_eq!(m.conflicts(), 1);
        assert_eq!(m.choices().count(), 5);
    }

    #[test]
    fn merged_file_takes_each_pick() {
        let (b, o, t) = versions();
        let mut m = SceneMerge::new(&b, &o, &t).unwrap();
        m.set("Main Camera", Side::Theirs);
        let merged = SceneDoc::parse(&m.merged_text()).unwrap();
        let keys: Vec<&str> = merged.objects.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "Main Camera",
                "Player",
                "Player/Child",
                "Button",
                "Cube",
                "Light"
            ]
        );
        assert_eq!(
            merged.get("Main Camera").unwrap().components["Camera"]["fov"],
            90.0
        );
        assert_eq!(
            merged.get("Player").unwrap().components["Transform"]["position"][0],
            5.0
        );
        assert_eq!(merged.settings["lighting"]["fog"]["enabled"], true);
        // The Button's reference still finds Player.
        let text = m.merged_text();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            v["objects"][3]["components"]["Button"]["on_click"][0]["target"],
            1
        );

        // All ours: the base's second Cube stays, theirs' fog doesn't.
        m.set_all(Side::Ours);
        let ours = SceneDoc::parse(&m.merged_text()).unwrap();
        assert!(ours.get("Cube [2]").is_some());
        assert_eq!(ours.settings["lighting"]["fog"]["enabled"], false);
        assert_eq!(ours, SceneDoc::parse(&o).unwrap());
    }
}
