//! Scene object search: name words plus `t:Component`, `tag:` and `layer:` filters.
//! Used by the Hierarchy search box (substring) and the palette (fuzzy).

use misty_engine::hecs::Entity;
use misty_engine::scene::{Scene, registry};
use misty_engine::tags_layers::TagsAndLayers;

use super::fuzzy::{self, Match, Mode};
use super::query::{Query, type_key};

/// Does `e` pass the query's type / tag / layer filters?
pub fn passes_filters(scene: &Scene, tl: &TagsAndLayers, e: Entity, q: &Query) -> bool {
    if !q.has_filters() {
        return true;
    }
    let go = scene.game_object(e);
    let tag_ok = q.tags.is_empty() || q.tags.iter().any(|t| go.tag.eq_ignore_ascii_case(t));
    let layer_ok = q.layers.is_empty() || {
        let name = tl.layer_name(go.layer as i32).to_lowercase();
        let number = go.layer.to_string();
        q.layers.iter().any(|l| *l == name || *l == number)
    };
    drop(go);
    tag_ok && layer_ok && (q.types.is_empty() || q.types.iter().any(|t| has_type(scene, e, t)))
}

/// `t:` value `key` (normalised) names a component `e` has; `t:GameObject` matches all.
fn has_type(scene: &Scene, e: Entity, key: &str) -> bool {
    key == "gameobject"
        || registry::components()
            .iter()
            .any(|c| type_key(c.name) == key && (c.has)(&scene.world, e))
}

/// Matching objects in Hierarchy order with their name matches.
/// Fuzzy results are sorted best first; `cap` = 0 means no limit.
pub fn search(
    scene: &Scene,
    tl: &TagsAndLayers,
    q: &Query,
    mode: Mode,
    cap: usize,
) -> Vec<(Entity, Match)> {
    let mut out: Vec<(Entity, Match)> = scene
        .walk()
        .into_iter()
        .filter_map(|(e, _)| {
            let m = fuzzy::match_words(&q.words, &scene.name(e), mode)?;
            passes_filters(scene, tl, e, q).then_some((e, m))
        })
        .collect();
    if mode == Mode::Fuzzy {
        // Stable: equal scores keep Hierarchy order.
        out.sort_by_key(|(_, m)| std::cmp::Reverse(m.score));
    }
    if cap > 0 {
        out.truncate(cap);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use misty_engine::scene::Light;

    fn scene() -> (Scene, TagsAndLayers) {
        (Scene::default_3d("Test"), TagsAndLayers::default())
    }

    fn names(scene: &Scene, hits: &[(Entity, Match)]) -> Vec<String> {
        hits.iter().map(|(e, _)| scene.name(*e)).collect()
    }

    #[test]
    fn type_filters() {
        let (mut s, tl) = scene();
        let lamp = s.spawn("Lamp", None);
        s.world.insert_one(lamp, Light::default()).unwrap();
        let hits = search(&s, &tl, &Query::parse("t:Light"), Mode::Contains, 0);
        assert_eq!(names(&s, &hits), ["Directional Light", "Lamp"]);
        let hits = search(&s, &tl, &Query::parse("t:Camera"), Mode::Contains, 0);
        assert_eq!(names(&s, &hits), ["Main Camera"]);
        let hits = search(&s, &tl, &Query::parse("lamp t:light"), Mode::Fuzzy, 0);
        assert_eq!(names(&s, &hits), ["Lamp"]);
        let hits = search(&s, &tl, &Query::parse("t:AudioListener"), Mode::Fuzzy, 0);
        assert_eq!(
            names(&s, &hits),
            ["Main Camera"],
            "multi-word component name"
        );
        assert!(search(&s, &tl, &Query::parse("t:Nope"), Mode::Fuzzy, 0).is_empty());
    }

    #[test]
    fn tag_and_layer_filters() {
        let (mut s, tl) = scene();
        let hero = s.spawn("Hero", None);
        {
            let mut go = s.game_object_mut(hero);
            go.tag = "Player".into();
            go.layer = 5;
        }
        let hits = search(&s, &tl, &Query::parse("tag:player"), Mode::Contains, 0);
        assert_eq!(names(&s, &hits), ["Hero"]);
        let ui_layer = tl.layer_name(5).to_owned();
        let hits = search(
            &s,
            &tl,
            &Query::parse(&format!("layer:{ui_layer}")),
            Mode::Contains,
            0,
        );
        assert_eq!(names(&s, &hits), ["Hero"]);
        let hits = search(&s, &tl, &Query::parse("layer:5"), Mode::Contains, 0);
        assert_eq!(names(&s, &hits), ["Hero"]);
        assert!(
            search(
                &s,
                &tl,
                &Query::parse("tag:Player layer:0"),
                Mode::Contains,
                0
            )
            .is_empty()
        );
    }

    #[test]
    fn fuzzy_sorts_and_caps() {
        let (mut s, tl) = scene();
        s.spawn("Camera Rig", None);
        let hits = search(&s, &tl, &Query::parse("cam"), Mode::Fuzzy, 0);
        assert_eq!(names(&s, &hits)[0], "Camera Rig", "prefix first");
        assert_eq!(search(&s, &tl, &Query::parse(""), Mode::Fuzzy, 2).len(), 2);
    }
}
