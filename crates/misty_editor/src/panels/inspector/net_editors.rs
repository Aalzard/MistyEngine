//! Inspectors of the Netcode components: Network Manager (transport, tick rate, player prefab,
//! approval, scene sync, network simulator; Start Host / Server / Client while playing),
//! Network Object (global id, owner and spawn state while playing) and Network Transform.

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::network::{
    AutoStart, NetAuthority, NetworkManager, NetworkObject, NetworkTransform,
};

use super::card::{grid, grid_sub_header};
use crate::panels::AssetPayload;
use crate::state::EditorState;

pub const NAMES: [&str; 3] = ["Network Manager", "Network Object", "Network Transform"];

pub fn icon(name: &str) -> Option<&'static str> {
    match name {
        "Network Manager" => Some("🖧"),
        "Network Object" => Some("🌐"),
        "Network Transform" => Some("⇄"),
        _ => None,
    }
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Network Manager" => manager(ui, state, e),
        "Network Object" => object(ui, state, e),
        _ => transform(ui, state, e),
    }
}

fn row(ui: &mut Ui, label: &str, tip: &str) {
    ui.label(label).on_hover_text(tip);
}

fn prefab_field(ui: &mut Ui, prefab: &mut Option<String>) -> bool {
    let mut ch = false;
    let current = prefab
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "None (Prefab)".into());
    let slot = egui::ComboBox::from_id_salt("net_player_prefab")
        .selected_text(format!("⬢ {current}"))
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(prefab, None, "None").changed();
            ui.weak("Drop a prefab with a Network Object from Asset Files here.");
        })
        .response
        .on_hover_text(
            prefab
                .as_deref()
                .unwrap_or("Spawned for every client that joins"),
        );
    if let Some(p) = slot.dnd_release_payload::<AssetPayload>()
        && p.0.ends_with(".prefab")
    {
        *prefab = Some(p.0.clone());
        ch = true;
    }
    ch
}

fn manager(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut m) = state
        .scene
        .world
        .get::<&NetworkManager>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let before = m.clone();
    grid(ui, "net_manager", |ui| {
        row(ui, "Address", "Where clients connect (the host's address).");
        ui.text_edit_singleline(&mut m.address);
        ui.end_row();
        row(ui, "Port", "UDP port of the server.");
        ui.add(egui::DragValue::new(&mut m.port).range(0..=65535));
        ui.end_row();
        row(
            ui,
            "Listen Address",
            "Network interface the server listens on (0.0.0.0 = all).",
        );
        ui.text_edit_singleline(&mut m.listen_address);
        ui.end_row();
        row(ui, "Tick Rate", "Network updates sent per second.");
        ui.add(egui::Slider::new(&mut m.tick_rate, 1..=120).suffix(" Hz"));
        ui.end_row();
        row(
            ui,
            "Player Prefab",
            "Spawned for every client that joins, owned by it.",
        );
        prefab_field(ui, &mut m.player_prefab);
        ui.end_row();
        row(
            ui,
            "Connection Approval",
            "Network.ConnectionApproval(clientId, payload) decides who may join.",
        );
        ui.checkbox(&mut m.connection_approval, "");
        ui.end_row();
        row(ui, "Max Connections", "Clients the server accepts at once.");
        ui.add(egui::DragValue::new(&mut m.max_connections).range(1..=1024));
        ui.end_row();
        row(
            ui,
            "Timeout",
            "Seconds of silence before a connection counts as lost.",
        );
        ui.add(
            egui::DragValue::new(&mut m.timeout)
                .range(1.0..=120.0)
                .speed(0.1)
                .suffix(" s"),
        );
        ui.end_row();
        row(
            ui,
            "Scene Management",
            "Clients load the server's scene when they join and when it changes.",
        );
        ui.checkbox(&mut m.scene_management, "");
        ui.end_row();
        row(
            ui,
            "LAN Discovery",
            "A listening server answers Network.FindLanGames() on the local network.",
        );
        ui.checkbox(&mut m.lan_discovery, "");
        ui.end_row();
        grid_sub_header(ui, "Network Simulator");
        row(ui, "Latency", "Extra one-way delay for testing.");
        ui.add(
            egui::DragValue::new(&mut m.simulated_latency_ms)
                .range(0.0..=2000.0)
                .suffix(" ms"),
        );
        ui.end_row();
        row(ui, "Jitter", "Random extra delay (reorders packets).");
        ui.add(
            egui::DragValue::new(&mut m.simulated_jitter_ms)
                .range(0.0..=1000.0)
                .suffix(" ms"),
        );
        ui.end_row();
        row(ui, "Packet Loss", "Share of packets dropped.");
        ui.add(egui::Slider::new(&mut m.simulated_packet_loss, 0.0..=50.0).suffix(" %"));
        ui.end_row();
    });
    let changed = m != before;
    if changed {
        // While playing, the simulator follows the Inspector at once.
        if let Some(rt) = state.runtime.as_ref() {
            let sim = m.clone();
            rt.with_network(|n| {
                n.config.manager.simulated_latency_ms = sim.simulated_latency_ms;
                n.config.manager.simulated_jitter_ms = sim.simulated_jitter_ms;
                n.config.manager.simulated_packet_loss = sim.simulated_packet_loss;
                let s = n.config.simulator();
                n.set_simulator(s);
            });
        }
        let _ = state.scene.world.insert_one(e, m);
    }
    play_controls(ui, state);
    changed
}

/// Start Host / Server / Client and Shutdown while playing, with the session's status.
fn play_controls(ui: &mut Ui, state: &mut EditorState) {
    let Some(rt) = state.runtime.as_mut() else {
        ui.weak("Start it from a script (Network.StartHost()), or tick players in Window ▸ Multiplayer ▸ Multiplayer Play Mode.");
        return;
    };
    ui.add_space(4.0);
    let status = rt.with_network(|n| {
        n.is_running().then(|| {
            let port = n
                .local_port()
                .map_or(String::new(), |p| format!(" · port {p}"));
            match n.mode {
                misty_engine::network::NetMode::Client => {
                    format!("Client {}{port}", n.local_client_id)
                }
                m => format!(
                    "{}{port} · {} client(s)",
                    m.label(),
                    n.connected_client_ids().iter().filter(|c| **c != 0).count()
                ),
            }
        })
    });
    let mut start = None;
    let mut stop = false;
    ui.horizontal(|ui| match &status {
        Some(text) => {
            ui.label(RichText::new(text).strong());
            if ui.button("Shutdown").clicked() {
                stop = true;
            }
        }
        None => {
            if ui.button("Start Host").clicked() {
                start = Some(AutoStart::Host);
            }
            if ui.button("Start Server").clicked() {
                start = Some(AutoStart::Server);
            }
            if ui.button("Start Client").clicked() {
                start = Some(AutoStart::Client(String::new()));
            }
        }
    });
    if stop {
        rt.network_shutdown(&mut state.scene);
    }
    if let Some(how) = start
        && let Err(err) = rt.network_start(&mut state.scene, &how)
    {
        state.console.warn(format!("Network: {err}"));
    }
}

fn object(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut n) = state
        .scene
        .world
        .get::<&NetworkObject>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let mut ch = false;
    grid(ui, "net_object", |ui| {
        row(
            ui,
            "Global ID",
            "Matches this scene object on the server and the clients. Copies get a new one.",
        );
        ui.horizontal(|ui| {
            let short: String = n.global_id.chars().take(8).collect();
            ui.monospace(RichText::new(format!("{short}…")).weak())
                .on_hover_text(&n.global_id);
            if ui.small_button("Regenerate").clicked() {
                n.global_id = misty_engine::save::new_id();
                ch = true;
            }
        });
        ui.end_row();
        row(
            ui,
            "Destroy With Owner",
            "Despawn it when its owner leaves (else the server takes it over).",
        );
        ch |= ui.checkbox(&mut n.destroy_with_owner, "").changed();
        ui.end_row();
    });
    if let Some(rt) = state.runtime.as_ref() {
        let text = rt.with_network(|s| {
            if !s.is_running() {
                return "Not networked (offline)".to_owned();
            }
            match s.by_entity.get(&e).and_then(|id| s.objects.get(id)) {
                Some(o) => format!(
                    "Network Object Id {} · owner client {}{}",
                    o.id,
                    o.owner,
                    if o.is_player { " · player" } else { "" }
                ),
                None => "Not spawned".to_owned(),
            }
        });
        ui.weak(text);
    }
    if ch {
        let _ = state.scene.world.insert_one(e, n);
    }
    ch
}

fn transform(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut t) = state
        .scene
        .world
        .get::<&NetworkTransform>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let before = t.clone();
    grid(ui, "net_transform", |ui| {
        row(
            ui,
            "Authority",
            "Server: the server moves it. Owner: the owning client moves it.",
        );
        ui.horizontal(|ui| {
            for a in NetAuthority::ALL {
                ui.selectable_value(&mut t.authority, a, a.label());
            }
        });
        ui.end_row();
        row(ui, "Sync", "Which parts of the transform are replicated.");
        ui.horizontal(|ui| {
            ui.checkbox(&mut t.sync_position, "Position");
            ui.checkbox(&mut t.sync_rotation, "Rotation");
            ui.checkbox(&mut t.sync_scale, "Scale");
        });
        ui.end_row();
        row(
            ui,
            "Interpolate",
            "Remote copies glide between received states, a little in the past.",
        );
        ui.horizontal(|ui| {
            ui.checkbox(&mut t.interpolate, "");
            ui.add_enabled(
                t.interpolate,
                egui::DragValue::new(&mut t.interpolation_delay)
                    .range(0.0..=1.0)
                    .speed(0.005)
                    .suffix(" s"),
            )
            .on_hover_text("How far behind the newest state remote copies play");
        });
        ui.end_row();
        row(ui, "Thresholds", "Smallest change that is sent.");
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut t.position_threshold)
                    .range(0.0..=1.0)
                    .speed(0.0005)
                    .suffix(" m"),
            );
            ui.add(
                egui::DragValue::new(&mut t.rotation_threshold)
                    .range(0.0..=45.0)
                    .speed(0.01)
                    .suffix("°"),
            );
        });
        ui.end_row();
        row(
            ui,
            "Snap Distance",
            "Longer jumps teleport instead of sliding.",
        );
        ui.add(
            egui::DragValue::new(&mut t.snap_distance)
                .range(0.0..=1000.0)
                .speed(0.05)
                .suffix(" m"),
        );
        ui.end_row();
        row(
            ui,
            "Predict",
            "Server authority: the owner moves its copy at once and is corrected by the \
             server's state (move it locally and send the input with a ServerRpc).",
        );
        ui.add_enabled(
            t.authority == NetAuthority::Server,
            egui::Checkbox::new(&mut t.predict, ""),
        );
        ui.end_row();
    });
    let changed = t != before;
    if changed {
        let _ = state.scene.world.insert_one(e, t);
    }
    changed
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::project::ProjectBrowser;

    fn state(name: &str) -> (EditorState, Entity) {
        let root =
            std::env::temp_dir().join(format!("misty_net_insp_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root));
        let e = s.scene.spawn("Net", None);
        s.scene.add(e, NetworkManager::default());
        s.scene.add(e, NetworkObject::default());
        s.scene.add(e, NetworkTransform::default());
        (s, e)
    }

    #[test]
    fn network_inspectors_show_and_edit_their_settings() {
        let (s, e) = state("edit");
        let mut h = Harness::builder().with_size([420.0, 900.0]).build_ui_state(
            move |ui, s: &mut EditorState| {
                for name in NAMES {
                    ui.label(RichText::new(name).strong());
                    edit(ui, s, e, name);
                }
            },
            s,
        );
        h.run();
        for label in [
            "Port",
            "Tick Rate",
            "Player Prefab",
            "Connection Approval",
            "Network Simulator",
            "Global ID",
            "Destroy With Owner",
            "Authority",
            "Interpolate",
            "Snap Distance",
            "Predict",
            "Start it from a script (Network.StartHost()), or tick players in Window ▸ Multiplayer ▸ Multiplayer Play Mode.",
        ] {
            h.get_by_label(label);
        }
        // Authority → Owner, and the scale checkbox.
        h.get_by_label("Owner").click();
        h.run();
        h.get_by_label("Scale").click();
        h.run();
        h.get_by_label("Regenerate").click();
        h.run();
        let s = h.state();
        let t = s.scene.world.get::<&NetworkTransform>(e).unwrap();
        assert_eq!(t.authority, NetAuthority::Owner);
        assert!(t.sync_scale);
        assert_eq!(
            s.scene
                .world
                .get::<&NetworkObject>(e)
                .unwrap()
                .global_id
                .len(),
            32
        );
    }

    #[test]
    fn the_manager_inspector_starts_and_stops_a_host_while_playing() {
        let (mut s, e) = state("play");
        s.scene.world.get::<&mut NetworkManager>(e).unwrap().port = 0;
        s.scene
            .world
            .get::<&mut NetworkManager>(e)
            .unwrap()
            .lan_discovery = false;
        s.scene
            .world
            .get::<&mut NetworkManager>(e)
            .unwrap()
            .listen_address = "127.0.0.1".into();
        crate::play::start(&mut s);
        let mut h = Harness::builder().with_size([420.0, 700.0]).build_ui_state(
            move |ui, s: &mut EditorState| {
                manager(ui, s, e);
            },
            s,
        );
        h.run();
        h.get_by_label("Start Host").click();
        h.run();
        h.run();
        assert_eq!(
            h.state()
                .runtime
                .as_ref()
                .and_then(|r| r.network_stats())
                .map(|st| st.mode),
            Some(misty_engine::network::NetMode::Host)
        );
        h.get_by_label_contains("Host · port").hover();
        h.get_by_label("Shutdown").click();
        h.run();
        h.run();
        assert!(
            h.state()
                .runtime
                .as_ref()
                .unwrap()
                .network_stats()
                .is_none()
        );
        crate::play::stop(h.state_mut());
    }
}
