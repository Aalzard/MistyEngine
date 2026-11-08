//! Audio Mixer window: sidebar lists, adding groups and effects, M/S/B buttons, the fader, the
//! Inspector for a selected group, snapshots and Project ▸ Create ▸ Audio Mixer.

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::audio::EffectKind;
use misty_engine::audio::mixer::{MASTER, volume_key};

use super::strip::{db_to_pos, fader_height, pos_to_db};
use super::*;
use crate::commands;
use crate::project::ProjectBrowser;

const PATH: &str = "Assets/Audio/Main.mixer";

fn project(tag: &str) -> (EditorState, std::path::PathBuf, u32) {
    let root = std::env::temp_dir().join(format!("misty_mixer_win_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Audio")).unwrap();
    let mut m = AudioMixer::default();
    let music = m.add_group(MASTER, "Music");
    m.add_group(MASTER, "SFX");
    m.add_snapshot(0, "Muffled");
    m.save(&root.join(PATH)).unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    s.audio_mixer.open(Some(PATH.into()));
    (s, root, music)
}

/// The Mixer window on the left, the Inspector on the right.
fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1200.0, 600.0])
        .build_ui_state(
            |ui, s: &mut EditorState| {
                let (left, right) = ui.max_rect().split_left_right_at_x(860.0);
                ui.scope_builder(UiBuilder::new().max_rect(left), |ui| show(ui, s));
                ui.scope_builder(UiBuilder::new().max_rect(right), |ui| {
                    crate::panels::inspector::show(ui, s)
                });
            },
            s,
        );
    h.run();
    h
}

fn saved(root: &std::path::Path) -> AudioMixer {
    AudioMixer::load(&root.join(PATH)).unwrap()
}

fn drag(h: &mut Harness<'_, EditorState>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=6 {
        h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.run();
}

#[test]
fn fader_scale_round_trips() {
    for db in [-80.0, -40.0, -25.0, -3.0, 0.0, 6.0, 20.0] {
        assert!((pos_to_db(db_to_pos(db)) - db).abs() < 1e-3, "{db}");
    }
    assert_eq!(db_to_pos(0.0), 0.78, "0 dB sits high on the fader");
    assert!(db_to_pos(-10.0) < db_to_pos(-9.0));
}

#[test]
fn sidebar_lists_and_group_inspector() {
    let (s, root, music) = project("sidebar");
    let mut h = window(s);
    for label in [
        "Mixers",
        "Snapshots",
        "Groups",
        "Exposed Parameters (0)",
        "★ Snapshot",
        "   Muffled",
    ] {
        h.get_by_label(label);
    }
    // Strips: a header per group, plus the sidebar's tree entry.
    assert_eq!(h.get_all_by_label("Music").count(), 2);

    // Selecting Music shows it in the Inspector; Add... ▸ Lowpass adds an effect slot.
    h.get_all_by_label("Music").next().unwrap().click();
    h.run();
    assert_eq!(
        h.state().audio_mixer.selected,
        Some(MixerItem::Group(music, None))
    );
    h.get_by_label("Music (Audio Mixer Group)");
    h.get_all_by_label("Add...").last().unwrap().click();
    h.run();
    h.get_by_label("Lowpass").click();
    h.run();
    let m = saved(&root);
    assert_eq!(m.group(music).unwrap().effects[0].kind, EffectKind::Lowpass);
    h.get_by_label("Cutoff freq");

    // M on the Music strip mutes it (strips go Master, Music, SFX).
    h.get_all_by_label("M").nth(1).unwrap().click();
    h.run();
    assert!(saved(&root).group(music).unwrap().mute);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn fader_drag_sets_the_snapshot_volume_and_groups_can_be_added() {
    let (s, root, music) = project("fader");
    let mut h = window(s);
    // Drag Music's fader from its 0 dB knob to the top: +20 dB in the edited snapshot.
    let head = h.get_all_by_label("Music").last().unwrap().rect();
    let strip_h = 600.0 - head.top() - 6.0;
    let fader = fader_height(strip_h, 0);
    let knob = egui::pos2(
        head.left() + 21.0,
        head.bottom() + 28.0 + fader * (1.0 - 0.78),
    );
    drag(&mut h, knob, knob - egui::vec2(0.0, 300.0));
    let m = saved(&root);
    assert_eq!(m.value(0, &volume_key(music)), 20.0);
    assert_eq!(
        m.value(1, &volume_key(music)),
        0.0,
        "only the edited snapshot"
    );
    h.get_all_by_label("Music").next().unwrap().click();
    h.run();

    // Groups ▸ + adds a child of the selected group and starts renaming it.
    h.get_all_by_label("+").nth(2).unwrap().click();
    h.run();
    let m = saved(&root);
    let added = m.group_by_name("New Group").expect("added");
    assert_eq!(added.parent, Some(music), "under the selected group");
    assert!(h.state().audio_mixer.renaming.is_some());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn sends_effect_order_and_wet_levels_in_the_window() {
    let (mut s, root, music) = project("sends");
    let mut ids = (0, 0, 0, 0, 0);
    crate::audio_mixers::edit(&mut s, PATH, "Setup", false, |m| {
        let sfx = m.group_by_name("SFX").unwrap().id;
        let chorus = m.add_effect(sfx, EffectKind::Chorus).unwrap();
        let lowpass = m.add_effect(sfx, EffectKind::Lowpass).unwrap();
        let send = m.add_effect(sfx, EffectKind::Send).unwrap();
        let duck = m.add_effect(music, EffectKind::DuckVolume).unwrap();
        ids = (sfx, chorus, lowpass, send, duck);
    });
    let (sfx, chorus, lowpass, send, duck) = ids;
    let mut h = window(s);
    for label in ["Chorus", "Lowpass", "Send (none)", "Duck Volume"] {
        h.get_by_label(label);
    }
    // Selecting the Send shows its target picker; pick the Duck Volume on Music.
    h.get_by_label("Send (none)").click();
    h.run();
    assert_eq!(
        h.state().audio_mixer.selected,
        Some(MixerItem::Group(sfx, Some(send)))
    );
    h.get_by_label("SFX (Audio Mixer Group)");
    h.get_by_label("Send level");
    h.get_by_value("None").click();
    h.run();
    h.get_by_label("Duck Volume on Music").click();
    h.run();
    assert_eq!(saved(&root).effect(sfx, send).unwrap().target, Some(duck));
    h.get_by_label("Send → Music");
    h.get_by_label("Duck Volume ← 1");
    // Every signal effect has a wet mix level; the Inspector shows it.
    h.get_all_by_label("Wet Mix Level")
        .next()
        .expect("wet level row");
    // Drag Lowpass onto Chorus: the chain becomes Lowpass, Chorus, Send.
    let from = h
        .get_all_by_label("Lowpass")
        .next()
        .unwrap()
        .rect()
        .center();
    let to = h.get_all_by_label("Chorus").next().unwrap().rect().center();
    drag(&mut h, from, to);
    let order: Vec<u32> = saved(&root)
        .group(sfx)
        .unwrap()
        .effects
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(order, [lowpass, chorus, send]);
    // Selecting the Duck Volume lists who sends to it.
    h.get_by_label("Duck Volume ← 1").click();
    h.run();
    h.get_by_label("Sends from");
    h.get_all_by_label("SFX").next().unwrap();
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn create_menu_makes_a_mixer_asset() {
    let (mut s, root, _) = project("create");
    s.project.current = root.join("Assets/Audio");
    commands::apply(
        &mut s,
        crate::actions::Action::CreateAsset(AssetKind::AudioMixer),
    );
    let rel = "Assets/Audio/New Audio Mixer.mixer";
    assert!(root.join(rel).is_file());
    assert!(s.assets.mixers.contains(rel));
    let m = AudioMixer::load(&root.join(rel)).unwrap();
    assert_eq!(m.groups[0].name, "Master");
    std::fs::remove_dir_all(root).ok();
}
