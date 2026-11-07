//! Netcode messages between server and clients, and their encoding.

use glam::{Quat, Vec3};

use super::value::{NetValue, Reader, Writer};

/// Bumped when messages change; mismatched clients are refused.
pub const PROTOCOL_VERSION: u32 = 1;

/// Where a spawned object comes from on clients.
#[derive(Clone, Debug, PartialEq)]
pub enum SpawnSource {
    /// Instantiate this prefab.
    Prefab(String),
    /// The object saved in the scene with this Network Object id key.
    Scene(String),
}

/// One Network Variable value: behaviour (script index within the object), field, value.
#[derive(Clone, Debug, PartialEq)]
pub struct VarChange {
    pub behaviour: u16,
    pub field: String,
    pub value: NetValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpawnMsg {
    pub id: u64,
    pub owner: u64,
    pub is_player: bool,
    pub source: SpawnSource,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub vars: Vec<VarChange>,
}

/// A transform state; components left out did not change.
#[derive(Clone, Debug, PartialEq)]
pub struct TransformMsg {
    pub id: u64,
    /// Sender's network clock, seconds.
    pub time: f64,
    pub position: Option<Vec3>,
    pub rotation: Option<Quat>,
    pub scale: Option<Vec3>,
    /// Server authority: the owner's last input frame this state includes (prediction).
    pub ack: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// Client → server, first message: protocol version and connection data (approval).
    Hello {
        version: u32,
        payload: String,
    },
    /// Server → client: approved, with its client id and the server's scene.
    Welcome {
        client_id: u64,
        scene: Option<String>,
        tick_rate: u32,
    },
    Denied {
        reason: String,
    },
    /// Client → server: the server's scene is loaded; send the objects.
    SceneReady,
    /// Server → client: load this scene (the server changed scenes).
    LoadScene {
        path: String,
    },
    Spawn(SpawnMsg),
    /// Server → client: every object that existed at connect time was sent.
    SyncDone,
    Despawn {
        id: u64,
        destroy: bool,
    },
    Ownership {
        id: u64,
        owner: u64,
    },
    Vars {
        id: u64,
        changes: Vec<VarChange>,
    },
    Rpc {
        id: u64,
        behaviour: u16,
        method: String,
        args: Vec<NetValue>,
        /// Caller's local frame (the server acknowledges owner input with it).
        frame: u32,
    },
    Transform(TransformMsg),
}

const POS: u8 = 1;
const ROT: u8 = 2;
const SCALE: u8 = 4;

fn write_vars(w: &mut Writer, vars: &[VarChange]) {
    w.varint(vars.len() as u64);
    for v in vars {
        w.u16(v.behaviour);
        w.str(&v.field);
        v.value.write(w);
    }
}

fn read_vars(r: &mut Reader) -> Option<Vec<VarChange>> {
    let n = r.varint()? as usize;
    if n > r.remaining() {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(VarChange {
            behaviour: r.u16()?,
            field: r.str()?,
            value: NetValue::read(r)?,
        });
    }
    Some(out)
}

impl Msg {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            Msg::Hello { version, payload } => {
                w.u8(1);
                w.u32(*version);
                w.str(payload);
            }
            Msg::Welcome {
                client_id,
                scene,
                tick_rate,
            } => {
                w.u8(2);
                w.varint(*client_id);
                w.str(scene.as_deref().unwrap_or(""));
                w.varint(u64::from(*tick_rate));
            }
            Msg::Denied { reason } => {
                w.u8(3);
                w.str(reason);
            }
            Msg::SceneReady => w.u8(4),
            Msg::LoadScene { path } => {
                w.u8(5);
                w.str(path);
            }
            Msg::Spawn(s) => {
                w.u8(6);
                w.varint(s.id);
                w.varint(s.owner);
                w.u8(u8::from(s.is_player));
                match &s.source {
                    SpawnSource::Prefab(p) => {
                        w.u8(0);
                        w.str(p);
                    }
                    SpawnSource::Scene(k) => {
                        w.u8(1);
                        w.str(k);
                    }
                }
                w.vec3(s.position);
                w.quat(s.rotation);
                w.vec3(s.scale);
                write_vars(&mut w, &s.vars);
            }
            Msg::SyncDone => w.u8(7),
            Msg::Despawn { id, destroy } => {
                w.u8(8);
                w.varint(*id);
                w.u8(u8::from(*destroy));
            }
            Msg::Ownership { id, owner } => {
                w.u8(9);
                w.varint(*id);
                w.varint(*owner);
            }
            Msg::Vars { id, changes } => {
                w.u8(10);
                w.varint(*id);
                write_vars(&mut w, changes);
            }
            Msg::Rpc {
                id,
                behaviour,
                method,
                args,
                frame,
            } => {
                w.u8(11);
                w.varint(*id);
                w.u16(*behaviour);
                w.str(method);
                w.u32(*frame);
                w.varint(args.len() as u64);
                for a in args {
                    a.write(&mut w);
                }
            }
            Msg::Transform(t) => {
                w.u8(12);
                w.varint(t.id);
                w.f64(t.time);
                let flags = if t.position.is_some() { POS } else { 0 }
                    | if t.rotation.is_some() { ROT } else { 0 }
                    | if t.scale.is_some() { SCALE } else { 0 };
                w.u8(flags);
                if let Some(p) = t.position {
                    w.vec3(p);
                }
                if let Some(q) = t.rotation {
                    w.quat(q);
                }
                if let Some(s) = t.scale {
                    w.vec3(s);
                }
                w.u32(t.ack);
            }
        }
        w.buf
    }

    /// `None` for garbled or unknown messages.
    pub fn decode(data: &[u8]) -> Option<Msg> {
        let mut r = Reader::new(data);
        let msg = match r.u8()? {
            1 => Msg::Hello {
                version: r.u32()?,
                payload: r.str()?,
            },
            2 => Msg::Welcome {
                client_id: r.varint()?,
                scene: Some(r.str()?).filter(|s| !s.is_empty()),
                tick_rate: u32::try_from(r.varint()?).ok()?,
            },
            3 => Msg::Denied { reason: r.str()? },
            4 => Msg::SceneReady,
            5 => Msg::LoadScene { path: r.str()? },
            6 => {
                let id = r.varint()?;
                let owner = r.varint()?;
                let is_player = r.u8()? != 0;
                let source = match r.u8()? {
                    0 => SpawnSource::Prefab(r.str()?),
                    1 => SpawnSource::Scene(r.str()?),
                    _ => return None,
                };
                Msg::Spawn(SpawnMsg {
                    id,
                    owner,
                    is_player,
                    source,
                    position: r.vec3()?,
                    rotation: r.quat()?,
                    scale: r.vec3()?,
                    vars: read_vars(&mut r)?,
                })
            }
            7 => Msg::SyncDone,
            8 => Msg::Despawn {
                id: r.varint()?,
                destroy: r.u8()? != 0,
            },
            9 => Msg::Ownership {
                id: r.varint()?,
                owner: r.varint()?,
            },
            10 => Msg::Vars {
                id: r.varint()?,
                changes: read_vars(&mut r)?,
            },
            11 => {
                let id = r.varint()?;
                let behaviour = r.u16()?;
                let method = r.str()?;
                let frame = r.u32()?;
                let n = r.varint()? as usize;
                if n > r.remaining() {
                    return None;
                }
                let mut args = Vec::with_capacity(n);
                for _ in 0..n {
                    args.push(NetValue::read(&mut r)?);
                }
                Msg::Rpc {
                    id,
                    behaviour,
                    method,
                    args,
                    frame,
                }
            }
            12 => {
                let id = r.varint()?;
                let time = r.f64()?;
                let flags = r.u8()?;
                let position = if flags & POS != 0 {
                    Some(r.vec3()?)
                } else {
                    None
                };
                let rotation = if flags & ROT != 0 {
                    Some(r.quat()?)
                } else {
                    None
                };
                let scale = if flags & SCALE != 0 {
                    Some(r.vec3()?)
                } else {
                    None
                };
                let finite = position.is_none_or(|p| p.is_finite())
                    && scale.is_none_or(|s| s.is_finite())
                    && time.is_finite();
                if !finite {
                    return None;
                }
                Msg::Transform(TransformMsg {
                    id,
                    time,
                    position,
                    rotation,
                    scale,
                    ack: r.u32()?,
                })
            }
            _ => return None,
        };
        r.is_empty().then_some(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_round_trips() {
        let msgs = [
            Msg::Hello {
                version: PROTOCOL_VERSION,
                payload: "name=Bo".into(),
            },
            Msg::Welcome {
                client_id: 3,
                scene: Some("Assets/Scenes/A.scene".into()),
                tick_rate: 30,
            },
            Msg::Welcome {
                client_id: 1,
                scene: None,
                tick_rate: 60,
            },
            Msg::Denied {
                reason: "full".into(),
            },
            Msg::SceneReady,
            Msg::LoadScene {
                path: "Assets/Scenes/B.scene".into(),
            },
            Msg::Spawn(SpawnMsg {
                id: 9,
                owner: 2,
                is_player: true,
                source: SpawnSource::Prefab("Assets/Prefabs/P.prefab".into()),
                position: Vec3::new(1.0, 2.0, 3.0),
                rotation: Quat::from_rotation_y(0.5),
                scale: Vec3::ONE,
                vars: vec![VarChange {
                    behaviour: 0,
                    field: "score".into(),
                    value: NetValue::Int(4),
                }],
            }),
            Msg::Spawn(SpawnMsg {
                id: 10,
                owner: 0,
                is_player: false,
                source: SpawnSource::Scene("abc".into()),
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::splat(2.0),
                vars: vec![],
            }),
            Msg::SyncDone,
            Msg::Despawn {
                id: 9,
                destroy: true,
            },
            Msg::Ownership { id: 9, owner: 1 },
            Msg::Vars {
                id: 9,
                changes: vec![VarChange {
                    behaviour: 1,
                    field: "name".into(),
                    value: NetValue::Text("Bo".into()),
                }],
            },
            Msg::Rpc {
                id: 9,
                behaviour: 0,
                method: "MoveServerRpc".into(),
                args: vec![NetValue::Vector3(Vec3::X), NetValue::Bool(true)],
                frame: 77,
            },
            Msg::Transform(TransformMsg {
                id: 9,
                time: 12.5,
                position: Some(Vec3::new(1.0, 0.0, -1.0)),
                rotation: None,
                scale: Some(Vec3::ONE),
                ack: 40,
            }),
        ];
        for m in msgs {
            let bytes = m.encode();
            assert_eq!(Msg::decode(&bytes).as_ref(), Some(&m));
            for cut in 0..bytes.len() {
                assert!(Msg::decode(&bytes[..cut]).is_none(), "{m:?} cut at {cut}");
            }
        }
        assert!(Msg::decode(&[200]).is_none());
    }
}
