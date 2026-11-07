//! Lua networking: the `Network` global (start a host, server or client, spawn, events),
//! `RpcTarget`, the `NetworkObject` / `NetworkTransform` components, network properties on
//! scripts of network objects (`self.IsOwner`, `self.IsServer`, …), Network Variables
//! (`NetworkVariable()` fields) and RPCs (`…ServerRpc` / `…ClientRpc` functions); and the
//! script host's per-frame network work (applying what arrived, sending what changed).
//!
//! ```lua
//! local Player = { score = 0, speed = 5 }
//! Player._attributes = { score = NetworkVariable({ onChanged = "OnScoreChanged" }) }
//!
//! function Player:Update()
//!     if not self.IsOwner then return end
//!     local move = Vector3.new(Input.GetAxis("Horizontal"), 0, Input.GetAxis("Vertical"))
//!     self:MoveServerRpc(move * (self.speed * Time.deltaTime))
//! end
//! function Player:MoveServerRpc(delta) self.transform.position = self.transform.position + delta end
//! function Player:OnScoreChanged(old, new) Debug.Log("score", new) end
//! return Player
//! ```

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use glam::{Quat, Vec3};
use hecs::Entity;
use mlua::{
    Function, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
    Variadic,
};

use super::api::{LGameObject, LVec3, with_world};
use super::{LogKind, Request, ScriptHost, World};
use crate::network::interpolation::Snapshot;
use crate::network::protocol::{Msg, SpawnMsg, SpawnSource, TransformMsg, VarChange};
use crate::network::session::{
    Behaviour, ClientPhase, Inbound, NetEvent, NetObj, NetVar, TransformSync,
};
use crate::network::transport::Channel;
use crate::network::value::MAX_DEPTH;
use crate::network::{
    AutoStart, NetAuthority, NetMode, NetSession, NetValue, NetVarSpec, NetworkManager,
    NetworkObject, NetworkTransform, ReadPermission, RpcKind, RpcSpec, SERVER_CLIENT_ID,
    WritePermission,
};
use crate::scene::{Scene, Scripts};

const LISTENERS: &str = "misty.net.listeners";
const APPROVAL: &str = "misty.net.approval";

// ---- script classes ---------------------------------------------------------------------------

/// What the network needs of a script class: its Network Variables and RPC functions.
#[derive(Clone, Debug, Default)]
pub struct NetClassInfo {
    /// Replicated fields, sorted by name (the same order on every peer).
    pub vars: Vec<(String, NetVarSpec)>,
    pub rpcs: HashMap<String, RpcSpec>,
}

impl NetClassInfo {
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty() && self.rpcs.is_empty()
    }
}

/// Read a class's Network Variables (`_attributes`) and RPCs (attributes or name suffix).
pub fn class_info(class: &Table) -> NetClassInfo {
    let attrs = super::attributes::parse(class);
    let mut vars: Vec<(String, NetVarSpec)> = attrs
        .iter()
        .filter_map(|(k, a)| a.network.clone().map(|s| (k.clone(), s)))
        .collect();
    vars.sort_by(|a, b| a.0.cmp(&b.0));
    let mut rpcs = HashMap::new();
    for pair in class.pairs::<Value, Value>() {
        let Ok((Value::String(k), Value::Function(_))) = pair else {
            continue;
        };
        let name = k.to_string_lossy();
        let spec = attrs
            .get(&name)
            .and_then(|a| a.rpc)
            .or_else(|| RpcSpec::from_name(&name));
        if let Some(s) = spec {
            rpcs.insert(name, s);
        }
    }
    NetClassInfo { vars, rpcs }
}

/// The host's networking memory besides the session.
#[derive(Default)]
pub(super) struct NetHostState {
    classes: HashMap<String, NetClassInfo>,
    settling: bool,
    /// Warnings already shown (one per object and field).
    warned: HashSet<String>,
}

/// Give a new script instance its RPC wrappers and network properties. `networked`: the
/// object is (inside) a Network Object.
pub(super) fn prepare_instance(
    lua: &Lua,
    t: &Table,
    class: &Table,
    script: &str,
    info: &NetClassInfo,
    networked: bool,
) -> Result<()> {
    for (name, spec) in &info.rpcs {
        let Ok(Value::Function(original)) = class.raw_get::<Value>(name.as_str()) else {
            continue;
        };
        let (method, script, spec) = (name.clone(), script.to_owned(), *spec);
        let wrapper = lua.create_function(move |lua, args: Variadic<Value>| {
            rpc_call(lua, &script, &method, spec, &original, args)
        })?;
        t.raw_set(name.as_str(), wrapper)?;
    }
    if networked || !info.is_empty() {
        let class = class.clone();
        let index = lua.create_function(move |lua, (me, key): (Table, Value)| {
            let v: Value = class.get(key.clone())?;
            if !v.is_nil() {
                return Ok(v);
            }
            match key {
                Value::String(s) => net_property(lua, &me, &s.to_string_lossy()),
                _ => Ok(Value::Nil),
            }
        })?;
        if let Some(meta) = t.metatable() {
            meta.set("__index", index)?;
        }
    }
    Ok(())
}

fn entity_of_instance(t: &Table) -> Result<Entity> {
    match t.raw_get::<Value>("gameObject")? {
        Value::UserData(ud) => Ok(ud.borrow::<LGameObject>()?.0),
        _ => Err(mlua::Error::runtime("not a script instance")),
    }
}

/// `self.IsOwner`, `self.IsServer`, … on scripts of network objects.
fn net_property(lua: &Lua, me: &Table, key: &str) -> Result<Value> {
    const KEYS: [&str; 11] = [
        "IsServer",
        "IsClient",
        "IsHost",
        "IsOwner",
        "IsOwnedByServer",
        "OwnerClientId",
        "NetworkObjectId",
        "IsSpawned",
        "IsLocalPlayer",
        "NetworkObject",
        "LocalClientId",
    ];
    if !KEYS.contains(&key) {
        return Ok(Value::Nil);
    }
    let e = entity_of_instance(me)?;
    enum Out {
        Bool(bool),
        Id(u64),
        Object(Option<Entity>),
    }
    let out = with_world(lua, |w| {
        let s = &w.network;
        let obj = s.object_of(&w.scene, e).and_then(|id| s.objects.get(&id));
        match key {
            "IsServer" => Out::Bool(s.is_server()),
            "IsClient" => Out::Bool(s.is_client()),
            "IsHost" => Out::Bool(s.is_host()),
            "IsOwner" => Out::Bool(obj.is_some_and(|o| o.owner == s.local_client_id)),
            "IsOwnedByServer" => Out::Bool(obj.is_some_and(|o| o.owner == SERVER_CLIENT_ID)),
            "OwnerClientId" => Out::Id(obj.map_or(SERVER_CLIENT_ID, |o| o.owner)),
            "NetworkObjectId" => Out::Id(obj.map_or(0, |o| o.id)),
            "IsSpawned" => Out::Bool(obj.is_some()),
            "IsLocalPlayer" => Out::Bool(
                obj.is_some_and(|o| o.is_player && o.owner == s.local_client_id && s.is_client()),
            ),
            "LocalClientId" => Out::Id(s.local_client_id),
            _ => Out::Object(network_object_entity(&w.scene, e)),
        }
    })?;
    Ok(match out {
        Out::Bool(b) => Value::Boolean(b),
        Out::Id(i) => Value::Integer(i as i64),
        Out::Object(Some(e)) => Value::UserData(lua.create_userdata(LNetworkObject(e))?),
        Out::Object(None) => Value::Nil,
    })
}

/// The nearest object with a Network Object component: `e` or an ancestor.
fn network_object_entity(scene: &Scene, e: Entity) -> Option<Entity> {
    let mut cur = Some(e);
    while let Some(x) = cur {
        if !scene.contains(x) {
            return None;
        }
        if scene.world.get::<&NetworkObject>(x).is_ok() {
            return Some(x);
        }
        cur = scene.parent(x);
    }
    None
}

/// Whether `e` is (inside) a Network Object.
pub(super) fn is_networked(scene: &Scene, e: Entity) -> bool {
    network_object_entity(scene, e).is_some()
}

// ---- values -----------------------------------------------------------------------------------

fn key_rank(v: &NetValue) -> (u8, f64, &str) {
    match v {
        NetValue::Int(i) => (0, *i as f64, ""),
        NetValue::Number(n) => (0, *n, ""),
        NetValue::Text(s) => (1, 0.0, s.as_str()),
        NetValue::Bool(b) => (2, f64::from(u8::from(*b)), ""),
        _ => (3, 0.0, ""),
    }
}

fn key_order(a: &NetValue, b: &NetValue) -> Ordering {
    let (ra, na, sa) = key_rank(a);
    let (rb, nb, sb) = key_rank(b);
    ra.cmp(&rb).then(na.total_cmp(&nb)).then(sa.cmp(sb))
}

/// A Lua value as a network value (GameObjects must be spawned network objects).
pub fn to_net(w: &World, v: &Value, depth: usize) -> std::result::Result<NetValue, String> {
    Ok(match v {
        Value::Nil => NetValue::Nil,
        Value::Boolean(b) => NetValue::Bool(*b),
        Value::Integer(i) => NetValue::Int(*i),
        Value::Number(n) => NetValue::Number(*n),
        Value::String(s) => NetValue::Text(s.to_string_lossy()),
        Value::UserData(ud) if ud.is::<LVec3>() => {
            NetValue::Vector3(ud.borrow::<LVec3>().map_err(|e| e.to_string())?.0)
        }
        Value::UserData(ud) if ud.is::<LGameObject>() || ud.is::<LNetworkObject>() => {
            let e = if ud.is::<LGameObject>() {
                ud.borrow::<LGameObject>().map_err(|e| e.to_string())?.0
            } else {
                ud.borrow::<LNetworkObject>().map_err(|e| e.to_string())?.0
            };
            let id = w
                .network
                .by_entity
                .get(&e)
                .copied()
                .ok_or("a GameObject that isn't a spawned network object")?;
            NetValue::Object(id)
        }
        Value::Table(t) => {
            if depth >= MAX_DEPTH {
                return Err("tables nested too deeply".into());
            }
            let mut pairs = Vec::new();
            for p in t.pairs::<Value, Value>() {
                let (k, v) = p.map_err(|e| e.to_string())?;
                pairs.push((to_net(w, &k, depth + 1)?, to_net(w, &v, depth + 1)?));
            }
            pairs.sort_by(|a, b| key_order(&a.0, &b.0));
            NetValue::Table(pairs)
        }
        other => {
            return Err(format!(
                "{} values can't be sent over the network",
                other.type_name()
            ));
        }
    })
}

/// A network value as a Lua value (unknown objects become nil).
pub fn from_net(lua: &Lua, w: &World, v: &NetValue) -> Result<Value> {
    Ok(match v {
        NetValue::Nil => Value::Nil,
        NetValue::Bool(b) => Value::Boolean(*b),
        NetValue::Int(i) => Value::Integer(*i),
        NetValue::Number(n) => Value::Number(*n),
        NetValue::Text(s) => Value::String(lua.create_string(s)?),
        NetValue::Vector3(v) => Value::UserData(lua.create_userdata(LVec3(*v))?),
        NetValue::Object(id) => match w.network.objects.get(id) {
            Some(o) if w.scene.contains(o.entity) => {
                Value::UserData(lua.create_userdata(LGameObject(o.entity))?)
            }
            _ => Value::Nil,
        },
        NetValue::Table(pairs) => {
            let t = lua.create_table()?;
            for (k, v) in pairs {
                let k = from_net(lua, w, k)?;
                if !k.is_nil() {
                    t.raw_set(k, from_net(lua, w, v)?)?;
                }
            }
            Value::Table(t)
        }
    })
}

// ---- world-side helpers (no Lua calls) --------------------------------------------------------

/// The scene's Network Manager settings (first one in Hierarchy order), else defaults.
pub fn manager_of(scene: &Scene) -> NetworkManager {
    scene
        .walk()
        .into_iter()
        .find_map(|(e, _)| {
            scene
                .world
                .get::<&NetworkManager>(e)
                .ok()
                .map(|m| (*m).clone())
        })
        .unwrap_or_default()
}

/// Network Objects saved in the scene with their matching keys (global id; repeated ids get
/// "#n" so copies with the same id still pair up by Hierarchy order). Network Objects inside
/// another one belong to it and are left out.
pub fn scene_keys(scene: &Scene) -> Vec<(Entity, String)> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for (e, _) in scene.walk() {
        let Ok(n) = scene.world.get::<&NetworkObject>(e) else {
            continue;
        };
        let nested = scene
            .parent(e)
            .is_some_and(|p| network_object_entity(scene, p).is_some());
        if nested {
            continue;
        }
        let count = seen.entry(n.global_id.clone()).or_insert(0);
        let key = if *count == 0 {
            n.global_id.clone()
        } else {
            format!("{}#{count}", n.global_id)
        };
        *count += 1;
        out.push((e, key));
    }
    out
}

fn log(w: &mut World, kind: LogKind, text: impl Into<String>) {
    w.logs.push((kind, text.into()));
}

/// Track `e` as network object `id` (assigned by the server when `None`).
fn track_object(
    w: &mut World,
    e: Entity,
    id: Option<u64>,
    owner: u64,
    is_player: bool,
    source: SpawnSource,
) -> u64 {
    let id = id.unwrap_or_else(|| w.network.new_object_id());
    let destroy_with_owner = w
        .scene
        .world
        .get::<&NetworkObject>(e)
        .map_or(true, |n| n.destroy_with_owner);
    let transform = w
        .scene
        .world
        .get::<&NetworkTransform>(e)
        .ok()
        .map(|t| TransformSync {
            settings: (*t).clone(),
            ..Default::default()
        });
    w.network.objects.insert(
        id,
        NetObj {
            id,
            entity: e,
            owner,
            is_player,
            source,
            destroy_with_owner,
            behaviours: Vec::new(),
            behaviours_ready: false,
            announced: false,
            spawn_called: false,
            transform,
            last_input_frame: 0,
            initial_vars: Vec::new(),
        },
    );
    w.network.by_entity.insert(e, id);
    if is_player && owner == w.network.local_client_id && w.network.is_client() {
        w.network.local_player = Some(id);
    }
    id
}

fn register_scene_objects(w: &mut World) {
    for (e, key) in scene_keys(&w.scene) {
        if !w.network.by_entity.contains_key(&e) {
            track_object(w, e, None, SERVER_CLIENT_ID, false, SpawnSource::Scene(key));
        }
    }
}

/// Spawn the Network Manager's player prefab for `client`.
fn spawn_player(w: &mut World, client: u64) -> Option<u64> {
    let path = w.network.config.manager.player_prefab.clone()?;
    if path.is_empty() {
        return None;
    }
    match super::api::instantiate_prefab_path(w, &path, None) {
        Ok(e) => {
            if w.scene.world.get::<&NetworkObject>(e).is_err() {
                log(
                    w,
                    LogKind::Error,
                    format!("Network: the player prefab {path} has no Network Object"),
                );
                w.destroy.push(e);
                return None;
            }
            Some(track_object(
                w,
                e,
                None,
                client,
                true,
                SpawnSource::Prefab(path),
            ))
        }
        Err(err) => {
            log(w, LogKind::Error, format!("Network: player prefab: {err}"));
            None
        }
    }
}

/// Start networking in the world (scripts' `Network.Start…`, or the editor / player).
pub(super) fn start_in_world(w: &mut World, start: &AutoStart) -> std::result::Result<(), String> {
    if w.network.is_running() {
        return Err(format!("already running as {}", w.network.mode.label()));
    }
    w.network.config.manager = manager_of(&w.scene);
    match start {
        AutoStart::Host => w.network.start_server(true)?,
        AutoStart::Server => w.network.start_server(false)?,
        AutoStart::Client(addr) => {
            let addr = if addr.trim().is_empty() {
                w.network.config.manager.address.clone()
            } else {
                addr.clone()
            };
            w.network.start_client(&addr)?;
        }
    }
    if w.network.is_server() {
        w.network.scene_path = w.scene.active_path();
        register_scene_objects(w);
        w.network.events.push(NetEvent::ServerStarted);
        if w.network.is_host() {
            if let Some(c) = w.network.clients.get_mut(&SERVER_CLIENT_ID) {
                c.joined = true;
            }
            spawn_player(w, SERVER_CLIENT_ID);
            w.network.events.push(NetEvent::ClientStarted);
            w.network
                .events
                .push(NetEvent::ClientConnected(SERVER_CLIENT_ID));
        }
    } else {
        w.network.events.push(NetEvent::ClientStarted);
    }
    Ok(())
}

/// Spawn `e` (an instantiated prefab or a scene object) on every client (server only).
fn spawn_existing(
    w: &mut World,
    e: Entity,
    owner: u64,
    is_player: bool,
) -> std::result::Result<u64, String> {
    if !w.network.is_server() {
        return Err("only the server can spawn network objects".into());
    }
    if !w.scene.contains(e) {
        return Err("the object was destroyed".into());
    }
    if w.scene.world.get::<&NetworkObject>(e).is_err() {
        return Err(format!("'{}' has no Network Object", w.scene.name(e)));
    }
    if let Some(id) = w.network.by_entity.get(&e) {
        return Ok(*id);
    }
    // Clients build it from its prefab: spawned with Instantiate(path), or a prefab instance.
    let path = w
        .save
        .spawned
        .iter()
        .find(|(x, _)| *x == e)
        .map(|(_, p)| p.clone())
        .or_else(|| w.scene.prefab_link(e));
    let source = match path {
        Some(p) => SpawnSource::Prefab(p),
        None => match scene_keys(&w.scene).into_iter().find(|(x, _)| *x == e) {
            Some((_, key)) => SpawnSource::Scene(key),
            None => {
                return Err(format!(
                    "'{}' can't be spawned: clients can only build prefabs (Instantiate a \
                     prefab path) or objects saved in the scene",
                    w.scene.name(e)
                ));
            }
        },
    };
    Ok(track_object(w, e, None, owner, is_player, source))
}

/// The client ids an RPC target means.
fn targets(t: &RpcTarget, s: &NetSession, owner: u64) -> Vec<u64> {
    let all = s.connected_client_ids();
    match t {
        RpcTarget::Everyone => all,
        RpcTarget::Owner => vec![owner],
        RpcTarget::NotOwner => all.into_iter().filter(|c| *c != owner).collect(),
        RpcTarget::NotServer => all.into_iter().filter(|c| *c != SERVER_CLIENT_ID).collect(),
        RpcTarget::Clients(ids) => ids.clone(),
    }
}

fn channel(reliable: bool) -> Channel {
    if reliable {
        Channel::ReliableOrdered
    } else {
        Channel::Unreliable
    }
}

/// Values of variables `client` may read.
fn readable(obj: &NetObj, client: u64) -> Vec<VarChange> {
    let mut out = Vec::new();
    for (bi, b) in obj.behaviours.iter().enumerate() {
        for v in &b.vars {
            if v.spec.read == ReadPermission::Owner && client != obj.owner {
                continue;
            }
            out.push(VarChange {
                behaviour: bi as u16,
                field: v.field.clone(),
                value: v.value.clone(),
            });
        }
    }
    out
}

fn spawn_msg(w: &World, obj: &NetObj, client: u64) -> SpawnMsg {
    let e = obj.entity;
    SpawnMsg {
        id: obj.id,
        owner: obj.owner,
        is_player: obj.is_player,
        source: obj.source.clone(),
        position: w.scene.world_position(e),
        rotation: w.scene.world_rotation(e),
        scale: w.scene.transform(e).scale,
        vars: readable(obj, client),
    }
}

fn authority(s: &NetSession, obj: &NetObj, ts: &TransformSync) -> bool {
    match ts.settings.authority {
        NetAuthority::Server => s.is_server(),
        NetAuthority::Owner => s.is_running() && obj.owner == s.local_client_id,
    }
}

fn predicting(s: &NetSession, obj: &NetObj, ts: &TransformSync) -> bool {
    ts.settings.authority == NetAuthority::Server
        && ts.settings.predict
        && s.mode == NetMode::Client
        && obj.owner == s.local_client_id
}

/// Put `e` at a received pose (with its physics body).
fn place(w: &mut World, e: Entity, snap: &Snapshot, settings: &NetworkTransform) {
    if !w.scene.contains(e) {
        return;
    }
    if settings.sync_position {
        w.scene.set_world_position(e, snap.position);
    }
    if settings.sync_rotation {
        w.scene.set_world_rotation(e, snap.rotation);
    }
    if settings.sync_scale {
        let mut t = w.scene.transform(e);
        t.scale = snap.scale;
        w.scene.set_transform(e, t);
    }
    if w.physics.has_body(e) {
        let (_, rot, pos) = w.scene.world_matrix(e).to_scale_rotation_translation();
        w.physics.teleport(e, pos, rot);
    }
}

fn pose(w: &World, e: Entity) -> (Vec3, Quat, Vec3) {
    (
        w.scene.world_position(e),
        w.scene.world_rotation(e),
        w.scene.transform(e).scale,
    )
}

// ---- RPC calls --------------------------------------------------------------------------------

/// What an RPC wrapper does after checking.
enum RpcPlan {
    /// Run the function here, with this sender id.
    Local(u64),
    Sent,
}

fn rpc_call(
    lua: &Lua,
    script: &str,
    method: &str,
    spec: RpcSpec,
    original: &Function,
    args: Variadic<Value>,
) -> Result<()> {
    let mut args: Vec<Value> = args.into_iter().collect();
    let me = match args.first() {
        Some(Value::Table(t)) => t.clone(),
        _ => {
            return Err(mlua::Error::runtime(format!(
                "call RPCs with a colon: self:{method}(...)"
            )));
        }
    };
    let e = entity_of_instance(&me)?;
    let target = match args.last() {
        Some(Value::UserData(ud)) if args.len() > 1 && ud.is::<LRpcTarget>() => {
            let t = ud.borrow::<LRpcTarget>()?.0.clone();
            args.pop();
            Some(t)
        }
        _ => None,
    };
    let plan = with_world(lua, |w| -> std::result::Result<RpcPlan, String> {
        if !w.network.is_running() {
            return Err("networking is not running".into());
        }
        let name = w.scene.name(e);
        let id = w
            .network
            .object_of(&w.scene, e)
            .ok_or_else(|| format!("'{name}' is not a spawned network object"))?;
        let obj = &w.network.objects[&id];
        let behaviour = obj
            .behaviours
            .iter()
            .position(|b| b.entity == e && b.script == script)
            .ok_or_else(|| format!("'{name}' is still spawning"))? as u16;
        let owner = obj.owner;
        let mut net_args = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate().skip(1) {
            net_args.push(to_net(w, a, 0).map_err(|err| format!("argument {i}: {err}"))?);
        }
        let s = &mut w.network;
        let local = s.local_client_id;
        let msg = Msg::Rpc {
            id,
            behaviour,
            method: method.to_owned(),
            args: net_args,
            frame: s.frame,
        };
        match spec.kind {
            RpcKind::Server => {
                if spec.require_ownership && owner != local {
                    return Err(format!(
                        "only the owner (client {owner}) may call it (set requireOwnership = \
                         false to allow everyone)"
                    ));
                }
                if s.is_server() {
                    Ok(RpcPlan::Local(local))
                } else {
                    s.send_to_server(channel(spec.reliable), &msg);
                    Ok(RpcPlan::Sent)
                }
            }
            RpcKind::Client => {
                if !s.is_server() {
                    return Err("only the server can call client RPCs".into());
                }
                let to = targets(target.as_ref().unwrap_or(&RpcTarget::Everyone), s, owner);
                let ready: HashSet<u64> = s.ready_remote_clients().into_iter().collect();
                for c in &to {
                    if ready.contains(c) {
                        s.send(*c, channel(spec.reliable), &msg);
                    }
                }
                if s.is_host() && to.contains(&SERVER_CLIENT_ID) {
                    Ok(RpcPlan::Local(SERVER_CLIENT_ID))
                } else {
                    Ok(RpcPlan::Sent)
                }
            }
        }
    })?;
    match plan {
        Ok(RpcPlan::Sent) => Ok(()),
        Ok(RpcPlan::Local(sender)) => {
            let old = with_world(lua, |w| {
                std::mem::replace(&mut w.network.rpc_sender, sender)
            })?;
            let result = original.call::<()>(Variadic::from_iter(args));
            with_world(lua, |w| w.network.rpc_sender = old)?;
            result
        }
        Err(msg) => {
            with_world(lua, |w| {
                log(w, LogKind::Error, format!("{script}.{method}: {msg}"));
            })?;
            Ok(())
        }
    }
}

// ---- Lua types --------------------------------------------------------------------------------

/// Who a client RPC goes to.
#[derive(Clone, Debug, PartialEq)]
pub enum RpcTarget {
    Everyone,
    Owner,
    NotOwner,
    NotServer,
    Clients(Vec<u64>),
}

#[derive(Clone)]
pub struct LRpcTarget(pub RpcTarget);
impl UserData for LRpcTarget {}

/// One of `Network`'s events (`OnClientConnected`, …).
#[derive(Clone, Copy)]
struct LNetEvent(&'static str);

impl UserData for LNetEvent {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("AddListener", |lua, ev, f: Function| {
            let list = listener_list(lua, ev.0)?;
            list.push(f)
        });
        m.add_method("RemoveListener", |lua, ev, f: Function| {
            let list = listener_list(lua, ev.0)?;
            let kept = lua.create_table()?;
            for g in list.sequence_values::<Function>().flatten() {
                if g != f {
                    kept.push(g)?;
                }
            }
            listeners_table(lua)?.set(ev.0, kept)
        });
    }
}

fn listeners_table(lua: &Lua) -> Result<Table> {
    match lua.named_registry_value::<Option<Table>>(LISTENERS)? {
        Some(t) => Ok(t),
        None => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(LISTENERS, t.clone())?;
            Ok(t)
        }
    }
}

fn listener_list(lua: &Lua, event: &str) -> Result<Table> {
    let all = listeners_table(lua)?;
    match all.get::<Option<Table>>(event)? {
        Some(t) => Ok(t),
        None => {
            let t = lua.create_table()?;
            all.set(event, t.clone())?;
            Ok(t)
        }
    }
}

fn listeners(lua: &Lua, event: &str) -> Vec<Function> {
    listener_list(lua, event)
        .map(|t| t.sequence_values::<Function>().flatten().collect())
        .unwrap_or_default()
}

const EVENTS: [&str; 6] = [
    "OnServerStarted",
    "OnClientStarted",
    "OnServerStopped",
    "OnClientStopped",
    "OnClientConnected",
    "OnClientDisconnected",
];

/// The `Network` global.
struct LNetwork;

fn start_from_lua(lua: &Lua, start: AutoStart) -> Result<bool> {
    with_world(lua, |w| match start_in_world(w, &start) {
        Ok(()) => true,
        Err(e) => {
            log(w, LogKind::Error, format!("Network: {e}"));
            false
        }
    })
}

fn entity_arg(v: &Value) -> Result<Entity> {
    match v {
        Value::UserData(ud) if ud.is::<LGameObject>() => Ok(ud.borrow::<LGameObject>()?.0),
        Value::UserData(ud) if ud.is::<LNetworkObject>() => Ok(ud.borrow::<LNetworkObject>()?.0),
        Value::UserData(ud) if ud.is::<super::api::LTransform>() => {
            Ok(ud.borrow::<super::api::LTransform>()?.0)
        }
        _ => Err(mlua::Error::runtime("expected a GameObject")),
    }
}

impl UserData for LNetwork {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("IsServer", |lua, _| {
            with_world(lua, |w| w.network.is_server())
        });
        f.add_field_method_get("IsClient", |lua, _| {
            with_world(lua, |w| w.network.is_client())
        });
        f.add_field_method_get("IsHost", |lua, _| with_world(lua, |w| w.network.is_host()));
        f.add_field_method_get("IsListening", |lua, _| {
            with_world(lua, |w| w.network.is_running())
        });
        f.add_field_method_get("IsConnectedClient", |lua, _| {
            with_world(lua, |w| w.network.is_connected_client())
        });
        f.add_field_method_get("LocalClientId", |lua, _| {
            with_world(lua, |w| w.network.local_client_id)
        });
        f.add_field_method_get("ServerClientId", |_, _| Ok(SERVER_CLIENT_ID));
        f.add_field_method_get("SenderClientId", |lua, _| {
            with_world(lua, |w| w.network.rpc_sender)
        });
        f.add_field_method_get("Mode", |lua, _| {
            with_world(lua, |w| w.network.mode.label().to_owned())
        });
        f.add_field_method_get("ConnectedClientsIds", |lua, _| {
            let ids = with_world(lua, |w| {
                if w.network.is_server() {
                    w.network.connected_client_ids()
                } else if w.network.is_connected_client() {
                    vec![w.network.local_client_id]
                } else {
                    Vec::new()
                }
            })?;
            lua.create_sequence_from(ids)
        });
        f.add_field_method_get("LocalPlayer", |lua, _| {
            let e = with_world(lua, |w| {
                let s = &w.network;
                s.local_player
                    .and_then(|id| s.objects.get(&id))
                    .map(|o| o.entity)
                    .filter(|e| w.scene.contains(*e))
            })?;
            e.map(|e| lua.create_userdata(LGameObject(e))).transpose()
        });
        f.add_field_method_get("LanGames", |lua, _| {
            let games = with_world(lua, |w| w.network.lan_games())?;
            let list = lua.create_table()?;
            for g in games {
                let t = lua.create_table()?;
                t.set("address", g.address.ip().to_string())?;
                t.set("port", g.address.port())?;
                t.set("name", g.name)?;
                t.set("players", g.players)?;
                t.set("maxPlayers", g.max_players)?;
                list.push(t)?;
            }
            Ok(list)
        });
        f.add_field_method_get("connectionData", |lua, _| {
            with_world(lua, |w| w.network.config.connection_data.clone())
        });
        f.add_field_method_set("connectionData", |lua, _, data: String| {
            with_world(lua, |w| w.network.config.connection_data = data)
        });
        f.add_field_method_get("ConnectionApproval", |lua, _| {
            lua.named_registry_value::<Value>(APPROVAL)
        });
        f.add_field_method_set("ConnectionApproval", |lua, _, f: Option<Function>| {
            lua.set_named_registry_value(APPROVAL, f)
        });
        for name in EVENTS {
            f.add_field_method_get(name, move |_, _| Ok(LNetEvent(name)));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_function("StartHost", |lua, ()| start_from_lua(lua, AutoStart::Host));
        m.add_function("StartServer", |lua, ()| {
            start_from_lua(lua, AutoStart::Server)
        });
        m.add_function(
            "StartClient",
            |lua, (addr, port): (Option<String>, Option<u16>)| {
                let mut addr = addr.unwrap_or_default();
                if let Some(p) = port {
                    if addr.is_empty() {
                        addr = with_world(lua, |w| manager_of(&w.scene).address)?;
                    }
                    addr = format!("{addr}:{p}");
                }
                start_from_lua(lua, AutoStart::Client(addr))
            },
        );
        m.add_function("Shutdown", |lua, ()| {
            with_world(lua, |w| {
                if w.network.is_running() {
                    w.network.shutdown_requested = true;
                }
            })
        });
        m.add_function("FindLanGames", |lua, ()| {
            with_world(lua, |w| {
                if let Err(e) = w.network.find_lan_games() {
                    log(w, LogKind::Warning, e);
                }
            })
        });
        m.add_function(
            "Spawn",
            |lua, (what, position, owner): (Value, Option<LVec3>, Option<u64>)| {
                let e = with_world(lua, |w| -> std::result::Result<Entity, mlua::Error> {
                    if !w.network.is_server() {
                        return Err(mlua::Error::runtime(
                            "Network.Spawn: only the server can spawn",
                        ));
                    }
                    let e = match &what {
                        Value::String(s) => super::api::instantiate_prefab_path(
                            w,
                            &s.to_string_lossy(),
                            position.map(|p| p.0),
                        )?,
                        other => {
                            let e = entity_arg(other)?;
                            if let Some(p) = position {
                                w.scene.set_world_position(e, p.0);
                            }
                            e
                        }
                    };
                    spawn_existing(w, e, owner.unwrap_or(SERVER_CLIENT_ID), false)
                        .map_err(|m| mlua::Error::runtime(format!("Network.Spawn: {m}")))?;
                    Ok(e)
                })??;
                Ok(LGameObject(e))
            },
        );
        m.add_function("GetPlayerObject", |lua, client: u64| {
            let e = with_world(lua, |w| {
                w.network
                    .objects
                    .values()
                    .find(|o| o.is_player && o.owner == client)
                    .map(|o| o.entity)
                    .filter(|e| w.scene.contains(*e))
            })?;
            e.map(|e| lua.create_userdata(LGameObject(e))).transpose()
        });
        m.add_function("GetNetworkObject", |lua, id: u64| {
            let e = with_world(lua, |w| {
                w.network
                    .objects
                    .get(&id)
                    .map(|o| o.entity)
                    .filter(|e| w.scene.contains(*e))
            })?;
            e.map(|e| lua.create_userdata(LGameObject(e))).transpose()
        });
        m.add_function("DisconnectClient", |lua, client: u64| {
            with_world(lua, |w| {
                if !w.network.is_server() || client == SERVER_CLIENT_ID {
                    return false;
                }
                let ok = w.network.disconnect_client(client);
                if ok {
                    w.network.events.push(NetEvent::ClientDisconnected(client));
                    despawn_owned_by(w, client);
                }
                ok
            })
        });
    }
}

/// Server: what a leaving client owned is despawned (or taken over by the server).
fn despawn_owned_by(w: &mut World, client: u64) {
    let owned: Vec<(u64, bool)> = w
        .network
        .objects
        .values()
        .filter(|o| o.owner == client)
        .map(|o| (o.id, o.destroy_with_owner || o.is_player))
        .collect();
    for (id, destroy) in owned {
        if destroy {
            if let Some(o) = w.network.objects.get(&id) {
                w.destroy.push(o.entity);
            }
        } else {
            change_owner(w, id, SERVER_CLIENT_ID);
        }
    }
}

/// Server: give object `id` to `owner` and tell everyone (callbacks run after the frame's
/// current call, see [`ScriptHost::network_settle`]).
fn change_owner(w: &mut World, id: u64, owner: u64) -> bool {
    let Some(obj) = w.network.objects.get_mut(&id) else {
        return false;
    };
    if obj.owner == owner {
        return true;
    }
    let old = obj.owner;
    obj.owner = owner;
    if let Some(ts) = obj.transform.as_mut() {
        ts.buffer.clear();
        ts.last_sent = None;
    }
    if obj.is_player && owner == w.network.local_client_id && w.network.is_client() {
        w.network.local_player = Some(id);
    }
    w.network.broadcast(
        Channel::ReliableOrdered,
        &Msg::Ownership { id, owner },
        |_| true,
    );
    w.network.owner_changes.push((id, old, owner));
    true
}

/// `GetComponent("NetworkObject")`.
#[derive(Clone, Copy)]
pub struct LNetworkObject(pub Entity);

fn with_obj<R>(lua: &Lua, e: Entity, f: impl FnOnce(&World, Option<&NetObj>) -> R) -> Result<R> {
    with_world(lua, |w| {
        let obj = w
            .network
            .by_entity
            .get(&e)
            .and_then(|id| w.network.objects.get(id));
        f(w, obj)
    })
}

fn spawn_method(lua: &Lua, e: Entity, owner: u64, player: bool) -> Result<()> {
    with_world(lua, |w| {
        spawn_existing(w, e, owner, player)
            .map(|_| ())
            .map_err(|m| mlua::Error::runtime(format!("Spawn: {m}")))
    })?
}

impl UserData for LNetworkObject {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("NetworkObjectId", |lua, n| {
            with_obj(lua, n.0, |_, o| o.map_or(0, |o| o.id))
        });
        f.add_field_method_get("OwnerClientId", |lua, n| {
            with_obj(lua, n.0, |_, o| o.map_or(SERVER_CLIENT_ID, |o| o.owner))
        });
        f.add_field_method_get("IsOwner", |lua, n| {
            with_obj(lua, n.0, |w, o| {
                o.is_some_and(|o| o.owner == w.network.local_client_id)
            })
        });
        f.add_field_method_get("IsOwnedByServer", |lua, n| {
            with_obj(lua, n.0, |_, o| {
                o.is_some_and(|o| o.owner == SERVER_CLIENT_ID)
            })
        });
        f.add_field_method_get("IsSpawned", |lua, n| with_obj(lua, n.0, |_, o| o.is_some()));
        f.add_field_method_get("IsPlayerObject", |lua, n| {
            with_obj(lua, n.0, |_, o| o.is_some_and(|o| o.is_player))
        });
        f.add_field_method_get("IsLocalPlayer", |lua, n| {
            with_obj(lua, n.0, |w, o| {
                o.is_some_and(|o| {
                    o.is_player && o.owner == w.network.local_client_id && w.network.is_client()
                })
            })
        });
        f.add_field_method_get("gameObject", |_, n| Ok(LGameObject(n.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Spawn", |lua, n, owner: Option<u64>| {
            spawn_method(lua, n.0, owner.unwrap_or(SERVER_CLIENT_ID), false)
        });
        m.add_method("SpawnWithOwnership", |lua, n, owner: u64| {
            spawn_method(lua, n.0, owner, false)
        });
        m.add_method("SpawnAsPlayerObject", |lua, n, owner: u64| {
            spawn_method(lua, n.0, owner, true)
        });
        m.add_method("Despawn", |lua, n, destroy: Option<bool>| {
            with_world(lua, |w| {
                if !w.network.is_server() {
                    return Err(mlua::Error::runtime("Despawn: only the server can despawn"));
                }
                let Some(id) = w.network.by_entity.get(&n.0).copied() else {
                    return Ok(());
                };
                if destroy.unwrap_or(true) {
                    w.destroy.push(n.0);
                } else {
                    w.network.despawns.push(id);
                }
                Ok(())
            })?
        });
        m.add_method("ChangeOwnership", |lua, n, owner: u64| {
            with_world(lua, |w| {
                if !w.network.is_server() {
                    return Err(mlua::Error::runtime(
                        "ChangeOwnership: only the server can change owners",
                    ));
                }
                let id = w.network.by_entity.get(&n.0).copied();
                match id {
                    Some(id) => {
                        change_owner(w, id, owner);
                        Ok(())
                    }
                    None => Err(mlua::Error::runtime("ChangeOwnership: not spawned")),
                }
            })?
        });
        m.add_method("RemoveOwnership", |lua, n, ()| {
            with_world(lua, |w| {
                if let Some(id) = w.network.by_entity.get(&n.0).copied()
                    && w.network.is_server()
                {
                    change_owner(w, id, SERVER_CLIENT_ID);
                }
            })
        });
        m.add_meta_method(MetaMethod::Eq, |_, a, b: mlua::AnyUserData| {
            Ok(b.borrow::<LNetworkObject>().is_ok_and(|b| b.0 == a.0))
        });
    }
}

/// `GetComponent("NetworkTransform")`.
#[derive(Clone, Copy)]
pub struct LNetworkTransform(pub Entity);

impl UserData for LNetworkTransform {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("authority", |lua, n| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&NetworkTransform>(n.0)
                    .map_or("Server", |t| t.authority.label())
                    .to_owned()
            })
        });
        // Whether this peer moves the object (the server or the owner, by authority).
        f.add_field_method_get("CanCommitToTransform", |lua, n| {
            with_obj(lua, n.0, |w, o| {
                o.and_then(|o| o.transform.as_ref().map(|t| authority(&w.network, o, t)))
                    .unwrap_or(false)
            })
        });
        f.add_field_method_get("interpolate", |lua, n| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&NetworkTransform>(n.0)
                    .is_ok_and(|t| t.interpolate)
            })
        });
        f.add_field_method_set("interpolate", |lua, n, on: bool| {
            with_world(lua, |w| {
                if let Ok(mut t) = w.scene.world.get::<&mut NetworkTransform>(n.0) {
                    t.interpolate = on;
                }
                if let Some(id) = w.network.by_entity.get(&n.0).copied()
                    && let Some(ts) = w
                        .network
                        .objects
                        .get_mut(&id)
                        .and_then(|o| o.transform.as_mut())
                {
                    ts.settings.interpolate = on;
                }
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // Move at once: remote copies jump instead of sliding there.
        m.add_method(
            "Teleport",
            |lua, n, (pos, euler): (LVec3, Option<LVec3>)| {
                with_world(lua, |w| {
                    w.scene.set_world_position(n.0, pos.0);
                    if let Some(r) = euler {
                        w.scene
                            .set_world_rotation(n.0, crate::camera::euler_to_quat(r.0));
                    }
                    if w.physics.has_body(n.0) {
                        let (_, rot, p) = w.scene.world_matrix(n.0).to_scale_rotation_translation();
                        w.physics.teleport(n.0, p, rot);
                    }
                    if let Some(id) = w.network.by_entity.get(&n.0).copied()
                        && let Some(ts) = w
                            .network
                            .objects
                            .get_mut(&id)
                            .and_then(|o| o.transform.as_mut())
                    {
                        ts.last_sent = None;
                    }
                })
            },
        );
    }
}

/// `GetComponent("NetworkObject" / "NetworkTransform")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let has = with_world(lua, |w| match name {
        "NetworkObject" => w.scene.world.get::<&NetworkObject>(e).is_ok(),
        "NetworkTransform" => w.scene.world.get::<&NetworkTransform>(e).is_ok(),
        _ => false,
    })?;
    if !has {
        return Ok(None);
    }
    Ok(Some(Value::UserData(match name {
        "NetworkObject" => lua.create_userdata(LNetworkObject(e))?,
        _ => lua.create_userdata(LNetworkTransform(e))?,
    })))
}

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    g.set("Network", LNetwork)?;
    let t = lua.create_table()?;
    t.set("Everyone", LRpcTarget(RpcTarget::Everyone))?;
    t.set("Owner", LRpcTarget(RpcTarget::Owner))?;
    t.set("NotOwner", LRpcTarget(RpcTarget::NotOwner))?;
    t.set("NotServer", LRpcTarget(RpcTarget::NotServer))?;
    t.set(
        "Client",
        lua.create_function(|_, id: u64| Ok(LRpcTarget(RpcTarget::Clients(vec![id]))))?,
    )?;
    t.set(
        "Clients",
        lua.create_function(|_, ids: Vec<u64>| Ok(LRpcTarget(RpcTarget::Clients(ids))))?,
    )?;
    g.set("RpcTarget", t)?;
    Ok(())
}

// ---- the script host's network frame ----------------------------------------------------------

impl ScriptHost {
    /// Networking info of the script class at `path` (loaded once).
    pub(super) fn net_class_info(&mut self, path: &str) -> NetClassInfo {
        if let Some(i) = self.net.classes.get(path) {
            return i.clone();
        }
        let info = self.class(path).map(|c| class_info(&c)).unwrap_or_default();
        self.net.classes.insert(path.to_owned(), info.clone());
        info
    }

    fn warn_once(&mut self, key: String, text: String) {
        if self.net.warned.insert(key) {
            self.log(LogKind::Warning, text);
        }
    }

    /// Start networking between frames (the editor's Multiplayer Play Mode, the player's
    /// `--net-client`, Inspector buttons).
    pub fn network_start(
        &mut self,
        scene: &mut Scene,
        start: &AutoStart,
    ) -> std::result::Result<(), String> {
        let scene_in = std::mem::take(scene);
        let result = self.with_world(|w| {
            w.scene = scene_in;
            start_in_world(w, start)
        });
        self.flush_spawned();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
        result
    }

    /// Stop networking between frames.
    pub fn network_shutdown(&mut self, scene: &mut Scene) {
        let scene_in = std::mem::take(scene);
        self.with_world(|w| w.scene = scene_in);
        self.stop_network();
        self.network_settle();
        self.process_destroy();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
    }

    /// The session, between frames.
    pub fn with_network<R>(&self, f: impl FnOnce(&mut NetSession) -> R) -> R {
        self.with_world(|w| f(&mut w.network))
    }

    /// Hand the session to the next scene's host (scene loads keep the connection).
    pub fn take_network(&mut self) -> NetSession {
        self.with_world(|w| std::mem::take(&mut w.network))
    }

    /// Continue `session` in this (freshly loaded) scene: a server spawns the new scene's
    /// objects and tells clients to load it; a client that was loading reports in.
    pub fn adopt_network(&mut self, scene: &mut Scene, mut session: NetSession) {
        session.objects.clear();
        session.by_entity.clear();
        session.local_player = None;
        let scene_in = std::mem::take(scene);
        self.with_world(|w| {
            w.scene = scene_in;
            w.network = session;
            if !w.network.is_running() {
                return;
            }
            if w.network.is_server() {
                let path = w.scene.active_path();
                w.network.scene_path = path.clone();
                register_scene_objects(w);
                let remote: Vec<u64> = w
                    .network
                    .clients
                    .iter()
                    .filter(|(_, c)| c.conn.is_some())
                    .map(|(id, _)| *id)
                    .collect();
                for c in remote {
                    if let Some(info) = w.network.clients.get_mut(&c) {
                        info.ready = false;
                    }
                    if let Some(p) = &path
                        && w.network.config.manager.scene_management
                    {
                        let msg = Msg::LoadScene { path: p.clone() };
                        w.network.send(c, Channel::ReliableOrdered, &msg);
                    }
                }
                if w.network.is_host() {
                    spawn_player(w, SERVER_CLIENT_ID);
                }
            } else if w.network.phase == ClientPhase::LoadingScene {
                w.network.phase = ClientPhase::Syncing;
                w.network
                    .send_to_server(Channel::ReliableOrdered, &Msg::SceneReady);
            }
        });
        self.flush_spawned();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
    }

    /// Stop: `OnNetworkDespawn` for every spawned object, prefab copies destroyed (scene
    /// objects stay), connections closed, stop events raised.
    fn stop_network(&mut self) {
        let (ids, was_server, was_client) = self.with_world(|w| {
            w.network.shutdown_requested = false;
            (
                w.network.objects.keys().copied().collect::<Vec<_>>(),
                w.network.is_server(),
                w.network.is_client(),
            )
        });
        if !was_server && !was_client {
            return;
        }
        for id in ids {
            self.despawn_local(id, None);
        }
        self.with_world(|w| {
            w.network.shutdown();
            if was_server {
                w.network.events.push(NetEvent::ServerStopped);
            }
            if was_client {
                w.network.events.push(NetEvent::ClientStopped);
            }
        });
    }

    /// Forget object `id` here: `OnNetworkDespawn`, then destroy it when `destroy` says so
    /// (`None`: destroy prefab copies, keep scene objects).
    fn despawn_local(&mut self, id: u64, destroy: Option<bool>) {
        let behaviours = self.with_world(|w| {
            w.network
                .objects
                .get(&id)
                .filter(|o| o.spawn_called)
                .map(|o| {
                    o.behaviours
                        .iter()
                        .map(|b| (b.entity, b.script.clone()))
                        .collect()
                })
                .unwrap_or_else(Vec::new)
        });
        self.call_behaviours(&behaviours, "OnNetworkDespawn", Vec::new);
        self.with_world(|w| {
            let Some(obj) = w.network.objects.remove(&id) else {
                return;
            };
            w.network.by_entity.remove(&obj.entity);
            if w.network.local_player == Some(id) {
                w.network.local_player = None;
            }
            let prefab = matches!(obj.source, SpawnSource::Prefab(_));
            if destroy.unwrap_or(prefab) && w.scene.contains(obj.entity) {
                w.destroy.push(obj.entity);
            }
        });
    }

    /// Call `method` on the given script instances that define it.
    fn call_behaviours(
        &mut self,
        list: &[(Entity, String)],
        method: &str,
        args: impl Fn() -> Vec<Value>,
    ) {
        for (e, script) in list {
            let Some(t) = super::script_instance(&self.lua, *e, script) else {
                continue;
            };
            let Ok(Some(f)) = t.get::<Option<Function>>(method) else {
                continue;
            };
            let mut all = vec![Value::Table(t)];
            all.extend(args());
            if let Err(err) = f.call::<()>(Variadic::from_iter(all)) {
                self.log(LogKind::Error, format!("{script} ({method}): {err}"));
            }
            self.flush_spawned();
        }
    }

    /// The script instances of object `id` as (entity, script name).
    fn behaviour_list(&self, id: u64) -> Vec<(Entity, String)> {
        self.with_world(|w| {
            w.network
                .objects
                .get(&id)
                .map(|o| {
                    o.behaviours
                        .iter()
                        .map(|b| (b.entity, b.script.clone()))
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// Build the script list of object `id` (after its scripts' `Awake`).
    fn build_behaviours(&mut self, id: u64) {
        let scripted: Vec<(Entity, crate::scene::ScriptRef)> = self.with_world(|w| {
            let Some(root) = w.network.objects.get(&id).map(|o| o.entity) else {
                return Vec::new();
            };
            let scene = &w.scene;
            scene
                .walk()
                .into_iter()
                .map(|(e, _)| e)
                .filter(|e| scene.is_ancestor(root, *e))
                // Scripts under a nested Network Object belong to that one.
                .filter(|e| network_object_entity(scene, *e) == Some(root))
                .flat_map(|e| {
                    scene
                        .world
                        .get::<&Scripts>(e)
                        .map(|s| s.list.iter().map(|r| (e, r.clone())).collect::<Vec<_>>())
                        .unwrap_or_default()
                })
                .collect()
        });
        let mut behaviours = Vec::new();
        for (e, r) in scripted {
            let info = self.net_class_info(&r.path);
            let script = r.name();
            let table = super::script_instance(&self.lua, e, &script);
            let vars = info
                .vars
                .iter()
                .map(|(field, spec)| {
                    let value = table
                        .as_ref()
                        .and_then(|t| t.get::<Value>(field.as_str()).ok())
                        .map(|v| self.with_world(|w| to_net(w, &v, 0).unwrap_or(NetValue::Nil)))
                        .unwrap_or(NetValue::Nil);
                    NetVar {
                        field: field.clone(),
                        spec: spec.clone(),
                        value,
                    }
                })
                .collect();
            behaviours.push(Behaviour {
                entity: e,
                script,
                path: r.path.clone(),
                vars,
                rpcs: info.rpcs.clone(),
            });
        }
        self.with_world(|w| {
            if let Some(o) = w.network.objects.get_mut(&id) {
                o.behaviours = behaviours;
                o.behaviours_ready = true;
            }
        });
    }

    /// Read every variable of object `id` from its scripts into the synced values (no
    /// callbacks: the baseline after spawning).
    fn refresh_var_baseline(&mut self, id: u64) {
        let fields: Vec<(usize, usize, Entity, String, String)> = self.with_world(|w| {
            let Some(o) = w.network.objects.get(&id) else {
                return Vec::new();
            };
            o.behaviours
                .iter()
                .enumerate()
                .flat_map(|(bi, b)| {
                    b.vars
                        .iter()
                        .enumerate()
                        .map(move |(vi, v)| (bi, vi, b.entity, b.script.clone(), v.field.clone()))
                })
                .collect()
        });
        for (bi, vi, e, script, field) in fields {
            let Some(t) = super::script_instance(&self.lua, e, &script) else {
                continue;
            };
            let Ok(v) = t.get::<Value>(field.as_str()) else {
                continue;
            };
            self.with_world(|w| {
                let value = to_net(w, &v, 0).unwrap_or(NetValue::Nil);
                if let Some(var) = w
                    .network
                    .objects
                    .get_mut(&id)
                    .and_then(|o| o.behaviours.get_mut(bi))
                    .and_then(|b| b.vars.get_mut(vi))
                {
                    var.value = value;
                }
            });
        }
    }

    /// Set received variables on object `id`'s scripts (and run their change callbacks).
    /// `from`: a client writing owner variables (server side), checked and relayed.
    fn apply_vars(&mut self, id: u64, changes: Vec<VarChange>, from: Option<u64>) {
        let mut accepted = Vec::new();
        for c in changes {
            let found = self.with_world(|w| {
                let o = w.network.objects.get(&id)?;
                let b = o.behaviours.get(c.behaviour as usize)?;
                let v = b.vars.iter().find(|v| v.field == c.field)?;
                Some((
                    o.owner,
                    w.scene.name(o.entity),
                    b.entity,
                    b.script.clone(),
                    v.spec.clone(),
                    v.value.clone(),
                ))
            });
            let Some((owner, name, e, script, spec, old)) = found else {
                continue;
            };
            if let Some(client) = from
                && (spec.write != WritePermission::Owner || owner != client)
            {
                self.warn_once(
                    format!("var-write {id} {}", c.field),
                    format!(
                        "Network: client {client} tried to write {script}.{} on '{name}' \
                         without permission",
                        c.field
                    ),
                );
                continue;
            }
            self.set_var(
                id,
                c.behaviour as usize,
                &c.field,
                e,
                &script,
                &spec,
                old,
                c.value.clone(),
            );
            accepted.push(c);
        }
        // Server: pass owner writes on to the other clients that may read them.
        if let Some(client) = from
            && !accepted.is_empty()
        {
            self.with_world(|w| {
                let Some(o) = w.network.objects.get(&id) else {
                    return;
                };
                let owner = o.owner;
                let everyone: Vec<VarChange> = accepted
                    .iter()
                    .filter(|c| {
                        o.behaviours
                            .get(c.behaviour as usize)
                            .and_then(|b| b.vars.iter().find(|v| v.field == c.field))
                            .is_some_and(|v| v.spec.read == ReadPermission::Everyone)
                    })
                    .cloned()
                    .collect();
                if !everyone.is_empty() {
                    let msg = Msg::Vars {
                        id,
                        changes: everyone,
                    };
                    w.network.broadcast(Channel::ReliableOrdered, &msg, |c| {
                        c != client && c != owner
                    });
                }
            });
        }
    }

    /// Write one variable into its script and the synced value; callback when it changed.
    #[allow(clippy::too_many_arguments)]
    fn set_var(
        &mut self,
        id: u64,
        behaviour: usize,
        field: &str,
        e: Entity,
        script: &str,
        spec: &NetVarSpec,
        old: NetValue,
        new: NetValue,
    ) {
        let Some(t) = super::script_instance(&self.lua, e, script) else {
            return;
        };
        let lua_new = self.with_world(|w| from_net(&self.lua, w, &new).unwrap_or(Value::Nil));
        let lua_old = self.with_world(|w| from_net(&self.lua, w, &old).unwrap_or(Value::Nil));
        if let Err(err) = t.set(field, lua_new.clone()) {
            self.log(LogKind::Error, format!("{script}.{field}: {err}"));
        }
        self.with_world(|w| {
            if let Some(v) = w
                .network
                .objects
                .get_mut(&id)
                .and_then(|o| o.behaviours.get_mut(behaviour))
                .and_then(|b| b.vars.iter_mut().find(|v| v.field == field))
            {
                v.value = new.clone();
            }
        });
        if old != new
            && let Some(method) = &spec.on_changed
        {
            self.call_behaviours(&[(e, script.to_owned())], method, || {
                vec![lua_old.clone(), lua_new.clone()]
            });
        }
    }

    /// Spawn bookkeeping that needs the scripts: script lists, received variables,
    /// `OnNetworkSpawn`, the server's spawn messages, ownership callbacks and connection
    /// events. Runs after objects were created (and their `Awake` ran).
    pub(super) fn network_settle(&mut self) {
        if self.net.settling {
            return;
        }
        let busy = self.lua.app_data_ref::<World>().is_some_and(|w| {
            let s = &w.network;
            !s.objects.is_empty()
                || !s.events.is_empty()
                || s.shutdown_requested
                || !s.owner_changes.is_empty()
                || !s.despawns.is_empty()
        });
        if !busy {
            return;
        }
        self.net.settling = true;
        for _ in 0..32 {
            let mut progressed = false;
            if self.with_world(|w| w.network.shutdown_requested) {
                self.stop_network();
                progressed = true;
            }
            // Despawn without destroying (`NetworkObject:Despawn(false)`).
            let despawns = self.with_world(|w| std::mem::take(&mut w.network.despawns));
            for id in despawns {
                self.with_world(|w| {
                    let msg = Msg::Despawn { id, destroy: false };
                    w.network
                        .broadcast(Channel::ReliableOrdered, &msg, |_| true);
                });
                self.despawn_local(id, Some(false));
                progressed = true;
            }
            let (to_build, to_spawn) = self.with_world(|w| {
                let pending: HashSet<Entity> = w.spawned.iter().copied().collect();
                let mut build = Vec::new();
                let mut spawn = Vec::new();
                for o in w.network.objects.values() {
                    if !w.scene.contains(o.entity) || pending.contains(&o.entity) {
                        continue;
                    }
                    if !o.behaviours_ready {
                        build.push(o.id);
                    } else if !o.spawn_called {
                        spawn.push(o.id);
                    }
                }
                (build, spawn)
            });
            for id in &to_build {
                self.build_behaviours(*id);
                progressed = true;
            }
            for id in to_build.into_iter().chain(to_spawn) {
                self.spawn_scripts(id);
                progressed = true;
            }
            // Server: tell the ready clients about new objects.
            let fresh: Vec<u64> = self.with_world(|w| {
                if !w.network.is_server() {
                    return Vec::new();
                }
                w.network
                    .objects
                    .values()
                    .filter(|o| o.spawn_called && !o.announced)
                    .map(|o| o.id)
                    .collect()
            });
            for id in fresh {
                self.with_world(|w| {
                    let clients = w.network.ready_remote_clients();
                    let Some(obj) = w.network.objects.get(&id) else {
                        return;
                    };
                    let msgs: Vec<(u64, Msg)> = clients
                        .into_iter()
                        .map(|c| (c, Msg::Spawn(spawn_msg(w, obj, c))))
                        .collect();
                    for (c, m) in msgs {
                        w.network.send(c, Channel::ReliableOrdered, &m);
                    }
                    if let Some(o) = w.network.objects.get_mut(&id) {
                        o.announced = true;
                    }
                });
                progressed = true;
            }
            let changes = self.with_world(|w| std::mem::take(&mut w.network.owner_changes));
            for (id, old, new) in changes {
                self.ownership_callbacks(id, old, new);
                progressed = true;
            }
            let events = self.with_world(|w| std::mem::take(&mut w.network.events));
            for ev in events {
                let (name, arg) = match ev {
                    NetEvent::ServerStarted => ("OnServerStarted", None),
                    NetEvent::ClientStarted => ("OnClientStarted", None),
                    NetEvent::ServerStopped => ("OnServerStopped", None),
                    NetEvent::ClientStopped => ("OnClientStopped", None),
                    NetEvent::ClientConnected(c) => ("OnClientConnected", Some(c)),
                    NetEvent::ClientDisconnected(c) => ("OnClientDisconnected", Some(c)),
                };
                for f in listeners(&self.lua, name) {
                    let result = match arg {
                        Some(c) => f.call::<()>(c),
                        None => f.call::<()>(()),
                    };
                    if let Err(err) = result {
                        self.log(LogKind::Error, format!("Network.{name} listener: {err}"));
                    }
                    self.flush_spawned();
                }
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
        self.net.settling = false;
    }

    /// Received variables, then `OnNetworkSpawn` (and the server's baseline after it).
    fn spawn_scripts(&mut self, id: u64) {
        let initial = self.with_world(|w| {
            w.network
                .objects
                .get_mut(&id)
                .map(|o| {
                    o.spawn_called = true;
                    std::mem::take(&mut o.initial_vars)
                })
                .unwrap_or_default()
        });
        if !initial.is_empty() {
            self.apply_vars(id, initial, None);
        }
        let list = self.behaviour_list(id);
        self.call_behaviours(&list, "OnNetworkSpawn", Vec::new);
        if self.with_world(|w| w.network.is_server()) {
            self.refresh_var_baseline(id);
        }
    }

    fn ownership_callbacks(&mut self, id: u64, old: u64, new: u64) {
        let local = self.with_world(|w| w.network.local_client_id);
        let list = self.behaviour_list(id);
        if old == local && new != local {
            self.call_behaviours(&list, "OnLostOwnership", Vec::new);
        }
        if new == local && old != local {
            self.call_behaviours(&list, "OnGainedOwnership", Vec::new);
        }
        self.call_behaviours(&list, "OnOwnershipChanged", || {
            vec![Value::Integer(old as i64), Value::Integer(new as i64)]
        });
    }

    /// Objects about to be destroyed (`Destroy`, scene unloads): spawned ones are despawned
    /// (the server tells the clients).
    pub(super) fn network_destroying(&mut self, doomed: &[Entity]) {
        let ids: Vec<(u64, bool)> = self.with_world(|w| {
            doomed
                .iter()
                .filter_map(|e| w.network.by_entity.get(e).copied())
                .map(|id| (id, w.network.is_server()))
                .collect()
        });
        for (id, server) in ids {
            if server {
                self.with_world(|w| {
                    let msg = Msg::Despawn { id, destroy: true };
                    w.network
                        .broadcast(Channel::ReliableOrdered, &msg, |_| true);
                });
            } else {
                let name = self.with_world(|w| {
                    w.network
                        .objects
                        .get(&id)
                        .map(|o| w.scene.name(o.entity))
                        .unwrap_or_default()
                });
                self.warn_once(
                    format!("client-destroy {id}"),
                    format!(
                        "Network: '{name}' was destroyed on a client; only the server can \
                         despawn network objects (it stays on the others)"
                    ),
                );
            }
            // Already being destroyed: don't queue it again.
            self.despawn_local(id, Some(false));
        }
    }

    /// Start of a frame: receive and apply what arrived, then move remote copies.
    pub(super) fn network_begin(&mut self, dt: f64) {
        let inbound = {
            let Some(mut w) = self.lua.app_data_mut::<World>() else {
                return;
            };
            if !w.network.is_running() && w.network.events.is_empty() {
                w.network.receive(dt);
                return;
            }
            w.network.receive(dt)
        };
        for item in inbound {
            self.on_inbound(item);
        }
        self.flush_spawned();
        self.network_settle();
        self.interpolate();
    }

    fn on_inbound(&mut self, item: Inbound) {
        match item {
            Inbound::Approve { client, payload } => {
                let decision = self
                    .lua
                    .named_registry_value::<Option<Function>>(APPROVAL)
                    .ok()
                    .flatten()
                    .map(|f| f.call::<(Option<bool>, Option<String>)>((client, payload)));
                let (ok, reason) = match decision {
                    None => (true, String::new()),
                    Some(Ok((ok, reason))) => (ok.unwrap_or(false), reason.unwrap_or_default()),
                    Some(Err(err)) => {
                        self.log(LogKind::Error, format!("Network.ConnectionApproval: {err}"));
                        (false, "approval failed".into())
                    }
                };
                self.with_world(|w| w.network.approve(client, ok, &reason));
            }
            Inbound::SceneReady { client } => self.on_scene_ready(client),
            Inbound::ClientLeft { client, reason } => {
                self.log(
                    LogKind::Info,
                    format!("Network: client {client} disconnected ({reason})"),
                );
                self.with_world(|w| {
                    w.network.events.push(NetEvent::ClientDisconnected(client));
                    despawn_owned_by(w, client);
                });
            }
            Inbound::Welcome { client_id, scene } => {
                self.with_world(|w| {
                    let here = w.scene.active_path();
                    let load = scene.filter(|p| Some(p) != here.as_ref());
                    match load {
                        Some(path) => {
                            w.network.phase = ClientPhase::LoadingScene;
                            w.requests.push(Request::NetworkLoadScene(path));
                        }
                        None => {
                            w.network.phase = ClientPhase::Syncing;
                            w.network
                                .send_to_server(Channel::ReliableOrdered, &Msg::SceneReady);
                        }
                    }
                    log(
                        w,
                        LogKind::Info,
                        format!("Network: connected as client {client_id}"),
                    );
                });
            }
            Inbound::Denied(reason) => {
                self.log(
                    LogKind::Warning,
                    format!("Network: connection refused: {reason}"),
                );
            }
            Inbound::LoadScene(path) => {
                let ids =
                    self.with_world(|w| w.network.objects.keys().copied().collect::<Vec<_>>());
                for id in ids {
                    self.despawn_local(id, Some(false));
                }
                self.with_world(|w| {
                    w.network.phase = ClientPhase::LoadingScene;
                    w.requests.push(Request::NetworkLoadScene(path));
                });
            }
            Inbound::Spawn(msg) => self.on_spawn(msg),
            Inbound::SyncDone => {
                self.with_world(|w| {
                    // Scene objects the server no longer has go away here too.
                    let bound: HashSet<Entity> = w.network.by_entity.keys().copied().collect();
                    for (e, _) in scene_keys(&w.scene) {
                        if !bound.contains(&e) {
                            w.destroy.push(e);
                        }
                    }
                    w.network.phase = ClientPhase::Synced;
                    let me = w.network.local_client_id;
                    w.network.events.push(NetEvent::ClientConnected(me));
                });
            }
            Inbound::Despawn { id, destroy } => self.despawn_local(id, Some(destroy)),
            Inbound::Ownership { id, owner } => {
                self.with_world(|w| {
                    let local = w.network.local_client_id;
                    if let Some(o) = w.network.objects.get_mut(&id) {
                        let old = o.owner;
                        o.owner = owner;
                        if let Some(ts) = o.transform.as_mut() {
                            ts.buffer.clear();
                            ts.last_sent = None;
                        }
                        if o.is_player && owner == local {
                            w.network.local_player = Some(id);
                        }
                        w.network.owner_changes.push((id, old, owner));
                    }
                });
            }
            Inbound::Disconnected(reason) => {
                self.log(LogKind::Warning, format!("Network: {reason}"));
                let me = self.with_world(|w| w.network.local_client_id);
                self.stop_network();
                self.with_world(|w| w.network.events.push(NetEvent::ClientDisconnected(me)));
            }
            Inbound::Vars { from, id, changes } => {
                let server = self.with_world(|w| w.network.is_server());
                self.apply_vars(id, changes, server.then_some(from));
            }
            Inbound::Rpc {
                from,
                id,
                behaviour,
                method,
                args,
                frame,
            } => self.on_rpc(from, id, behaviour, &method, args, frame),
            Inbound::Transform { from, msg } => self.on_transform(from, msg),
        }
    }

    /// Server: client `client` has the scene: send it every object, spawn its player.
    fn on_scene_ready(&mut self, client: u64) {
        let first = self.with_world(|w| {
            let msgs: Vec<Msg> = w
                .network
                .objects
                .values()
                .filter(|o| o.announced && w.scene.contains(o.entity))
                .map(|o| Msg::Spawn(spawn_msg(w, o, client)))
                .collect();
            for m in msgs {
                w.network.send(client, Channel::ReliableOrdered, &m);
            }
            let has_player = w
                .network
                .objects
                .values()
                .any(|o| o.is_player && o.owner == client);
            if !has_player {
                spawn_player(w, client);
            }
            w.network
                .send(client, Channel::ReliableOrdered, &Msg::SyncDone);
            let info = w.network.clients.get_mut(&client);
            let first = info.as_ref().is_some_and(|c| !c.joined);
            if let Some(c) = info {
                c.joined = true;
            }
            if first {
                w.network.events.push(NetEvent::ClientConnected(client));
            }
            first
        });
        if first {
            self.log(LogKind::Info, format!("Network: client {client} joined"));
        }
        self.flush_spawned();
    }

    /// Client: build a spawned object (from its prefab, or bind the scene's copy).
    fn on_spawn(&mut self, msg: SpawnMsg) {
        self.with_world(|w| {
            if w.network.objects.contains_key(&msg.id) {
                return;
            }
            let e = match &msg.source {
                SpawnSource::Prefab(path) => {
                    match super::api::instantiate_prefab_path(w, path, Some(msg.position)) {
                        Ok(e) => e,
                        Err(err) => {
                            log(w, LogKind::Error, format!("Network: spawn {path}: {err}"));
                            return;
                        }
                    }
                }
                SpawnSource::Scene(key) => {
                    match scene_keys(&w.scene).into_iter().find(|(_, k)| k == key) {
                        Some((e, _)) => e,
                        None => {
                            log(
                                w,
                                LogKind::Warning,
                                "Network: the server has a scene object this scene lacks \
                                 (are both on the same scene version?)",
                            );
                            return;
                        }
                    }
                }
            };
            w.scene.set_world_position(e, msg.position);
            w.scene.set_world_rotation(e, msg.rotation);
            let mut t = w.scene.transform(e);
            t.scale = msg.scale;
            w.scene.set_transform(e, t);
            if w.physics.has_body(e) {
                w.physics.teleport(e, msg.position, msg.rotation);
            }
            let id = track_object(
                w,
                e,
                Some(msg.id),
                msg.owner,
                msg.is_player,
                msg.source.clone(),
            );
            if let Some(o) = w.network.objects.get_mut(&id) {
                o.initial_vars = msg.vars.clone();
            }
        });
    }

    fn on_rpc(
        &mut self,
        from: u64,
        id: u64,
        behaviour: u16,
        method: &str,
        args: Vec<NetValue>,
        frame: u32,
    ) {
        let found = self.with_world(|w| {
            let server = w.network.is_server();
            let o = w.network.objects.get_mut(&id)?;
            let b = o.behaviours.get(behaviour as usize)?;
            let spec = *b.rpcs.get(method)?;
            let (e, script, path) = (b.entity, b.script.clone(), b.path.clone());
            let owner = o.owner;
            if server && from == owner {
                o.last_input_frame = frame;
            }
            Some((server, spec, e, script, path, owner, w.scene.name(o.entity)))
        });
        let Some((server, spec, e, script, path, owner, name)) = found else {
            return;
        };
        let allowed = match spec.kind {
            RpcKind::Server => server && (!spec.require_ownership || from == owner),
            RpcKind::Client => !server,
        };
        if !allowed {
            self.warn_once(
                format!("rpc {id} {method} {from}"),
                format!("Network: refused {script}.{method} on '{name}' from client {from}"),
            );
            return;
        }
        let Some(t) = super::script_instance(&self.lua, e, &script) else {
            return;
        };
        let Some(class) = self.class(&path) else {
            return;
        };
        let Ok(Some(f)) = class.raw_get::<Option<Function>>(method) else {
            return;
        };
        let mut all = vec![Value::Table(t)];
        let converted = self.with_world(|w| {
            args.iter()
                .map(|a| from_net(&self.lua, w, a))
                .collect::<Result<Vec<Value>>>()
        });
        match converted {
            Ok(v) => all.extend(v),
            Err(err) => {
                self.log(LogKind::Error, format!("{script}.{method}: {err}"));
                return;
            }
        }
        let old = self.with_world(|w| std::mem::replace(&mut w.network.rpc_sender, from));
        if let Err(err) = f.call::<()>(Variadic::from_iter(all)) {
            self.log(LogKind::Error, format!("{script} ({method}): {err}"));
        }
        self.with_world(|w| w.network.rpc_sender = old);
        self.flush_spawned();
    }

    fn on_transform(&mut self, from: u64, msg: TransformMsg) {
        self.with_world(|w| {
            let server = w.network.is_server();
            let Some(obj) = w.network.objects.get(&msg.id) else {
                return;
            };
            let Some(ts) = obj.transform.as_ref() else {
                return;
            };
            if server && !(ts.settings.authority == NetAuthority::Owner && obj.owner == from) {
                return; // only owners move owner-authority objects
            }
            if authority(&w.network, obj, ts) {
                return;
            }
            let e = obj.entity;
            let is_predicting = predicting(&w.network, obj, ts);
            let base = ts
                .buffer
                .latest()
                .map(|s| (s.position, s.rotation, s.scale))
                .unwrap_or_else(|| pose(w, e));
            let snap = Snapshot {
                time: msg.time,
                position: msg.position.unwrap_or(base.0),
                rotation: msg.rotation.unwrap_or(base.1),
                scale: msg.scale.unwrap_or(base.2),
            };
            let conn = w.network.sample_clock(from, msg.time);
            let current = pose(w, e);
            let Some(ts) = w
                .network
                .objects
                .get_mut(&msg.id)
                .and_then(|o| o.transform.as_mut())
            else {
                return;
            };
            ts.clock_conn = conn;
            if is_predicting {
                let settings = ts.settings.clone();
                let fixed = match ts.history.reconcile(msg.ack, snap.position, snap.rotation) {
                    Some((dp, dr)) => {
                        let off = dp.length() > settings.position_threshold.max(1e-4);
                        off.then(|| Snapshot {
                            position: current.0 + dp,
                            rotation: (dr * current.1).normalize(),
                            ..snap
                        })
                    }
                    // No input acknowledged yet: only a teleport moves us.
                    None if msg.ack == 0 => {
                        (current.0.distance(snap.position) > settings.snap_distance).then_some(snap)
                    }
                    None => Some(snap),
                };
                if let Some(s) = fixed {
                    let mut settings = settings;
                    settings.sync_scale = false;
                    place(w, e, &s, &settings);
                }
                return;
            }
            ts.buffer.push(snap, ts.settings.snap_distance);
            // Server: pass the owner's motion on to everyone else.
            if server {
                let relay = Msg::Transform(TransformMsg {
                    time: w.network.now,
                    ack: 0,
                    ..msg
                });
                w.network
                    .broadcast(Channel::Unreliable, &relay, |c| c != from);
            }
        });
    }

    /// Move remote copies along their received states.
    fn interpolate(&mut self) {
        self.with_world(|w| {
            let ids: Vec<u64> = w.network.objects.keys().copied().collect();
            for id in ids {
                let Some(obj) = w.network.objects.get(&id) else {
                    continue;
                };
                let Some(ts) = obj.transform.as_ref() else {
                    continue;
                };
                let e = obj.entity;
                if !w.scene.contains(e) {
                    continue;
                }
                let local_authority =
                    authority(&w.network, obj, ts) || predicting(&w.network, obj, ts);
                let clock = w.network.clock_now(ts.clock_conn);
                let has_body = w.physics.has_body(e);
                let Some(ts) = w
                    .network
                    .objects
                    .get_mut(&id)
                    .and_then(|o| o.transform.as_mut())
                else {
                    continue;
                };
                // Bodies moved by another peer don't simulate here.
                if has_body && local_authority == ts.made_kinematic {
                    let kinematic = if local_authority {
                        w.scene
                            .world
                            .get::<&crate::scene::Rigidbody>(e)
                            .is_ok_and(|r| r.is_kinematic)
                    } else {
                        true
                    };
                    w.physics.set_kinematic(e, kinematic);
                    ts.made_kinematic = !local_authority;
                }
                if local_authority || ts.buffer.is_empty() {
                    continue;
                }
                let settings = ts.settings.clone();
                let target = if settings.interpolate {
                    let time = clock.unwrap_or(f64::MAX) - f64::from(settings.interpolation_delay);
                    ts.buffer.sample(time)
                } else {
                    ts.buffer.sample(f64::MAX)
                };
                if let Some(s) = target {
                    place(w, e, &s, &settings);
                }
            }
        });
    }

    /// End of a frame: settle new objects, send changed variables and transforms, flush.
    pub(super) fn network_end(&mut self) {
        let running = self.with_world(|w| w.network.is_running());
        if !running {
            self.network_settle();
            return;
        }
        self.network_settle();
        let tick = self.with_world(|w| w.network.tick_due);
        if tick {
            self.sync_vars();
        }
        self.send_transforms(tick);
        self.with_world(|w| w.network.flush());
    }

    /// Compare Network Variables with their synced values: writers send changes (and run
    /// callbacks); other peers get their value back (with an error).
    fn sync_vars(&mut self) {
        let list: Vec<(
            u64,
            usize,
            usize,
            Entity,
            String,
            String,
            NetVarSpec,
            NetValue,
            bool,
        )> = self.with_world(|w| {
            let s = &w.network;
            let mut out = Vec::new();
            for o in s.objects.values() {
                if !o.spawn_called {
                    continue;
                }
                for (bi, b) in o.behaviours.iter().enumerate() {
                    for (vi, v) in b.vars.iter().enumerate() {
                        let writer = match v.spec.write {
                            WritePermission::Server => s.is_server(),
                            WritePermission::Owner => o.owner == s.local_client_id,
                        };
                        out.push((
                            o.id,
                            bi,
                            vi,
                            b.entity,
                            b.script.clone(),
                            v.field.clone(),
                            v.spec.clone(),
                            v.value.clone(),
                            writer,
                        ));
                    }
                }
            }
            out
        });
        let mut outgoing: HashMap<u64, Vec<(VarChange, ReadPermission)>> = HashMap::new();
        for (id, bi, _vi, e, script, field, spec, old, writer) in list {
            let Some(t) = super::script_instance(&self.lua, e, &script) else {
                continue;
            };
            let Ok(v) = t.get::<Value>(field.as_str()) else {
                continue;
            };
            let now = match self.with_world(|w| to_net(w, &v, 0)) {
                Ok(n) => n,
                Err(err) => {
                    self.warn_once(
                        format!("var-type {id} {field}"),
                        format!("Network: {script}.{field} can't be sent: {err}"),
                    );
                    continue;
                }
            };
            if now == old {
                continue;
            }
            if writer {
                self.set_var(id, bi, &field, e, &script, &spec, old, now.clone());
                outgoing.entry(id).or_default().push((
                    VarChange {
                        behaviour: bi as u16,
                        field,
                        value: now,
                    },
                    spec.read,
                ));
            } else {
                let who = match spec.write {
                    WritePermission::Server => "the server",
                    WritePermission::Owner => "the owner",
                };
                self.warn_once(
                    format!("var-perm {id} {field}"),
                    format!(
                        "Network: only {who} may write {script}.{field}; the change was undone"
                    ),
                );
                let back = self.with_world(|w| from_net(&self.lua, w, &old).unwrap_or(Value::Nil));
                let _ = t.set(field.as_str(), back);
            }
        }
        if outgoing.is_empty() {
            return;
        }
        self.with_world(|w| {
            let server = w.network.is_server();
            for (id, changes) in outgoing {
                let owner = w
                    .network
                    .objects
                    .get(&id)
                    .map_or(SERVER_CLIENT_ID, |o| o.owner);
                if server {
                    let all: Vec<VarChange> = changes
                        .iter()
                        .filter(|(_, r)| *r == ReadPermission::Everyone)
                        .map(|(c, _)| c.clone())
                        .collect();
                    let owner_only: Vec<VarChange> =
                        changes.iter().map(|(c, _)| c.clone()).collect();
                    for c in w.network.ready_remote_clients() {
                        let set = if c == owner { &owner_only } else { &all };
                        if !set.is_empty() {
                            let msg = Msg::Vars {
                                id,
                                changes: set.clone(),
                            };
                            w.network.send(c, Channel::ReliableOrdered, &msg);
                        }
                    }
                } else {
                    let msg = Msg::Vars {
                        id,
                        changes: changes.into_iter().map(|(c, _)| c).collect(),
                    };
                    w.network.send_to_server(Channel::ReliableOrdered, &msg);
                }
            }
        });
    }

    /// Authority sides send changed transforms each tick; predicting owners record poses.
    fn send_transforms(&mut self, tick: bool) {
        self.with_world(|w| {
            let now = w.network.now;
            let frame = w.network.frame;
            let server = w.network.is_server();
            let mut out: Vec<(Msg, Option<u64>)> = Vec::new();
            let ids: Vec<u64> = w.network.objects.keys().copied().collect();
            for id in ids {
                let Some(obj) = w.network.objects.get(&id) else {
                    continue;
                };
                let Some(ts) = obj.transform.as_ref() else {
                    continue;
                };
                let e = obj.entity;
                if !w.scene.contains(e) || !obj.spawn_called {
                    continue;
                }
                let (pos, rot, scale) = pose(w, e);
                let is_predicting = predicting(&w.network, obj, ts);
                let has_authority = authority(&w.network, obj, ts);
                let (owner, ack) = (obj.owner, obj.last_input_frame);
                if let Some(ts) = w
                    .network
                    .objects
                    .get_mut(&id)
                    .and_then(|o| o.transform.as_mut())
                {
                    if is_predicting {
                        ts.history.record(frame, pos, rot);
                        continue;
                    }
                    if !has_authority || !tick {
                        continue;
                    }
                    let st = &ts.settings;
                    let full = ts.last_sent.is_none() || now - ts.last_send_time >= 1.0;
                    let (lp, lr, ls) = ts.last_sent.unwrap_or((pos, rot, scale));
                    let moved = st.sync_position && lp.distance(pos) > st.position_threshold;
                    let turned = st.sync_rotation
                        && lr.angle_between(rot).to_degrees() > st.rotation_threshold;
                    let scaled = st.sync_scale && ls.distance(scale) > st.scale_threshold;
                    let acked = st.predict && ack != ts.last_sent_ack;
                    if !(full || moved || turned || scaled || acked) {
                        continue;
                    }
                    let msg = TransformMsg {
                        id,
                        time: now,
                        position: (st.sync_position && (full || moved || acked)).then_some(pos),
                        rotation: (st.sync_rotation && (full || turned || acked)).then_some(rot),
                        scale: (st.sync_scale && (full || scaled)).then_some(scale),
                        ack,
                    };
                    let mut sent = ts.last_sent.unwrap_or((pos, rot, scale));
                    if msg.position.is_some() {
                        sent.0 = pos;
                    }
                    if msg.rotation.is_some() {
                        sent.1 = rot;
                    }
                    if msg.scale.is_some() {
                        sent.2 = scale;
                    }
                    ts.last_sent = Some(sent);
                    ts.last_sent_ack = ack;
                    if full {
                        ts.last_send_time = now;
                    }
                    // Owners don't need their own motion back (owner authority).
                    let skip = (ts.settings.authority == NetAuthority::Owner).then_some(owner);
                    out.push((Msg::Transform(msg), skip));
                }
            }
            for (msg, skip) in out {
                if server {
                    w.network
                        .broadcast(Channel::Unreliable, &msg, |c| Some(c) != skip);
                } else {
                    w.network.send_to_server(Channel::Unreliable, &msg);
                }
            }
        });
    }
}
