//! The User Guide's files, built into the editor so it works in every install: the chapters of
//! `docs/guide/<lang>/` in English and Arabic, and the pictures of `docs/guide/images/`.
//! Turkish (and any other language) reads the English chapters.

use crate::i18n::Language;

/// One chapter: its file name (without `.md`, the same in every language) and its text.
pub struct Chapter {
    pub id: &'static str,
    pub en: &'static str,
    pub ar: &'static str,
}

impl Chapter {
    /// The text in `lang` (English when there is no translation).
    pub fn text(&self, lang: Language) -> &'static str {
        match lang {
            Language::Arabic => self.ar,
            _ => self.en,
        }
    }
}

macro_rules! chapter {
    ($id:literal) => {
        Chapter {
            id: $id,
            en: include_str!(concat!("../../../../../docs/guide/en/", $id, ".md")),
            ar: include_str!(concat!("../../../../../docs/guide/ar/", $id, ".md")),
        }
    };
}

/// The chapters in reading order (the index first, the glossary last).
pub static CHAPTERS: &[Chapter] = &[
    chapter!("index"),
    chapter!("01-layout"),
    chapter!("02-performance"),
    chapter!("03-quality-and-rendering"),
    chapter!("04-script-graphs-basics"),
    chapter!("05-guide-first-person"),
    chapter!("06-guide-door"),
    chapter!("07-guide-saving"),
    chapter!("08-guide-animation"),
    chapter!("glossary"),
];

#[cfg(test)]
pub fn chapter(id: &str) -> Option<&'static Chapter> {
    CHAPTERS.iter().find(|c| c.id == id)
}

macro_rules! image {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!("../../../../../docs/guide/images/", $name)) as &[u8],
        )
    };
}

/// The pictures, by file name (chapters write `../images/<name>`).
pub static IMAGES: &[(&str, &[u8])] = &[
    image!("layout-overview.jpg"),
    image!("layout-overview-ar.jpg"),
    image!("stats.jpg"),
    image!("profiler.jpg"),
    image!("frame-debugger.jpg"),
    image!("quality-menu.jpg"),
    image!("project-settings-quality.jpg"),
    image!("script-graph-window.jpg"),
    image!("guide-first-person-graph.jpg"),
    image!("guide-door-graph.jpg"),
    image!("guide-saving-graph.jpg"),
    image!("guide-animation-graph.jpg"),
    image!("guide-animation-controller.jpg"),
];

/// The bytes of the picture a chapter names (`../images/x.jpg` or `x.jpg`).
#[cfg(test)]
pub fn image(path: &str) -> Option<&'static [u8]> {
    let name = path.rsplit('/').next().unwrap_or(path);
    IMAGES.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

/// The example scene and Script Graph of a guide chapter (its "Open example" button). The files
/// are in the sample project, and built in too so any project can get a copy.
pub struct Example {
    pub scene: &'static str,
    pub graph: &'static str,
    /// Every file the example needs: (project path, contents).
    pub files: &'static [(&'static str, &'static [u8])],
}

macro_rules! sample {
    ($path:literal) => {
        (
            $path,
            include_bytes!(concat!("../../../../../SampleProject/", $path)) as &[u8],
        )
    };
}

static FIRST_PERSON_FILES: &[(&str, &[u8])] = &[
    sample!("Assets/Guides/FirstPerson/FirstPerson.scene"),
    sample!("Assets/Guides/FirstPerson/FirstPersonPlayer.vgraph"),
];

static DOOR_FILES: &[(&str, &[u8])] = &[
    sample!("Assets/Guides/Door/Door.scene"),
    sample!("Assets/Guides/Door/Door.vgraph"),
    sample!("Assets/Guides/FirstPerson/FirstPersonPlayer.vgraph"),
];

static SAVING_FILES: &[(&str, &[u8])] = &[
    sample!("Assets/Guides/Saving/Saving.scene"),
    sample!("Assets/Guides/Saving/SaveLoad.vgraph"),
    sample!("Assets/Guides/FirstPerson/FirstPersonPlayer.vgraph"),
];

static ANIMATION_FILES: &[(&str, &[u8])] = &[
    sample!("Assets/Guides/Animation/Animation.scene"),
    sample!("Assets/Guides/Animation/CharacterAnimation.vgraph"),
    sample!("Assets/Guides/Animation/Character.controller"),
    sample!("Assets/Guides/Animation/Idle.anim"),
    sample!("Assets/Guides/Animation/Walk.anim"),
    sample!("Assets/Guides/Animation/Run.anim"),
    sample!("Assets/Guides/Animation/Jump.anim"),
];

pub fn example(chapter: &str) -> Option<Example> {
    Some(match chapter {
        "05-guide-first-person" => Example {
            scene: "Assets/Guides/FirstPerson/FirstPerson.scene",
            graph: "Assets/Guides/FirstPerson/FirstPersonPlayer.vgraph",
            files: FIRST_PERSON_FILES,
        },
        "06-guide-door" => Example {
            scene: "Assets/Guides/Door/Door.scene",
            graph: "Assets/Guides/Door/Door.vgraph",
            files: DOOR_FILES,
        },
        "07-guide-saving" => Example {
            scene: "Assets/Guides/Saving/Saving.scene",
            graph: "Assets/Guides/Saving/SaveLoad.vgraph",
            files: SAVING_FILES,
        },
        "08-guide-animation" => Example {
            scene: "Assets/Guides/Animation/Animation.scene",
            graph: "Assets/Guides/Animation/CharacterAnimation.vgraph",
            files: ANIMATION_FILES,
        },
        _ => return None,
    })
}
