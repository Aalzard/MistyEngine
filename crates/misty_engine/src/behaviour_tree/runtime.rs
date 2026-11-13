//! The behaviour trees of a running game: one [`BtAgent`] per Behaviour Tree Agent component,
//! the compiled trees (each asset compiled once), the shared blackboards, and what the editor's
//! debugger reads.
//!
//! The script host owns the frame loop (scripting/behaviour_tree_api.rs): it calls
//! [`BtRuntime::sync`] to pick up new and removed agents, then ticks each one through a host that
//! reaches the world, navigation, animation, physics and Lua.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use hecs::Entity;

use super::asset::BehaviourTree;
use super::blackboard::{BbValue, Blackboard, KeyType, ObjRef, SharedBlackboards};
use super::compile::{Compiled, compile};
use super::component::{BehaviourTreeAgent, name_seed};
use super::exec::{ExecState, Status};
use crate::scene::Scene;

/// Whether an agent's tree runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunState {
    /// Ticking.
    #[default]
    Running,
    /// Stopped by a script, or never started (Start On Play off).
    Stopped,
    /// Finished with this result and doesn't restart by itself.
    Done(Status),
}

/// A Start / Stop / Restart asked for by a script, applied before the agent's next tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Start,
    Stop,
    Restart,
}

/// One agent.
pub struct BtAgent {
    pub entity: Entity,
    /// The tree's asset path.
    pub path: Rc<str>,
    pub tree: Rc<Compiled>,
    /// Tree memory (taken out while the agent ticks).
    pub exec: ExecState,
    pub bb: Blackboard,
    pub run: RunState,
    /// Seconds gathered towards the next tick.
    pub accum: f32,
    /// Results `BehaviourTree.Finish` reported for script nodes: (node id, success).
    pub finishes: Vec<(u32, bool)>,
    pub command: Option<Command>,
    alive: u64,
}

/// What the editor shows for one agent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebugView {
    pub entity: Option<Entity>,
    pub path: String,
    /// (node id, current status, last result, ticks since that result).
    pub nodes: Vec<(u32, Status, Status, u64)>,
    /// (key, type, value, shared).
    pub keys: Vec<(String, KeyType, String, bool)>,
    pub run: RunState,
    pub ticks: u64,
}

/// Every behaviour tree of a running game.
#[derive(Default)]
pub struct BtRuntime {
    pub agents: Vec<BtAgent>,
    index: HashMap<Entity, usize>,
    pub shared: SharedBlackboards,
    trees: HashMap<String, Rc<Compiled>>,
    root: PathBuf,
    frame: u64,
    pub logs: Vec<String>,
}

impl BtRuntime {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            ..Default::default()
        }
    }

    /// The compiled tree at `path` (compiled once; problems logged once).
    pub fn tree(&mut self, path: &str) -> Rc<Compiled> {
        if let Some(t) = self.trees.get(path) {
            return t.clone();
        }
        let compiled = match crate::vfs::project_file(&self.root, path)
            .and_then(|f| BehaviourTree::load(&f))
        {
            Ok(t) => {
                let c = compile(&t);
                for e in &c.errors {
                    let at = e.node.map(|n| format!(" (node {n})")).unwrap_or_default();
                    self.logs
                        .push(format!("Behaviour Tree {path}{at}: {}", e.message));
                }
                c
            }
            Err(e) => {
                self.logs.push(format!("Behaviour Tree: {e}"));
                Compiled::default()
            }
        };
        let rc = Rc::new(compiled);
        self.trees.insert(path.to_owned(), rc.clone());
        rc
    }

    /// Use an already compiled tree for `path` (tests).
    pub fn insert_tree(&mut self, path: &str, tree: &BehaviourTree) {
        self.trees.insert(path.to_owned(), Rc::new(compile(tree)));
    }

    pub fn agent_index(&self, e: Entity) -> Option<usize> {
        self.index.get(&e).copied()
    }

    pub fn agent(&self, e: Entity) -> Option<&BtAgent> {
        self.agents.get(self.agent_index(e)?)
    }

    pub fn agent_mut(&mut self, e: Entity) -> Option<&mut BtAgent> {
        let i = self.agent_index(e)?;
        self.agents.get_mut(i)
    }

    /// Pick up agents added to (or removed from) the scene. Returns the new agents with the
    /// Script Graphs their trees send events to.
    pub fn sync(&mut self, scene: &Scene) -> Vec<(Entity, Vec<Rc<str>>)> {
        self.frame += 1;
        let frame = self.frame;
        let mut fresh: Vec<Entity> = Vec::new();
        for (e, a) in scene.world.query::<&BehaviourTreeAgent>().iter() {
            match self.index.get(&e) {
                Some(&i) => self.agents[i].alive = frame,
                None if a.tree.as_deref().is_some_and(|t| !t.is_empty()) => fresh.push(e),
                None => {}
            }
        }
        if self.agents.iter().any(|a| a.alive != frame) {
            self.agents.retain(|a| a.alive == frame);
            self.index = self
                .agents
                .iter()
                .enumerate()
                .map(|(i, a)| (a.entity, i))
                .collect();
        }
        // New agents in a stable order (by object id), so runs repeat exactly.
        fresh.sort_by_key(|e| e.to_bits());
        let mut out = Vec::new();
        for e in fresh {
            let Ok(c) = scene.world.get::<&BehaviourTreeAgent>(e).map(|c| (*c).clone()) else {
                continue;
            };
            let path = c.tree.clone().unwrap_or_default();
            let tree = self.tree(&path);
            let mut bb = Blackboard::new(&tree.keys, c.group(), &mut self.shared);
            for o in &c.overrides {
                let v = match bb.index(&o.key).map(|i| bb.types[i]) {
                    Some(KeyType::Object) => BbValue::Object(ObjRef(o.object)),
                    Some(_) => o.value.clone(),
                    None => {
                        self.logs.push(format!(
                            "'{}': the Behaviour Tree has no key '{}' to override",
                            scene.name(e),
                            o.key
                        ));
                        continue;
                    }
                };
                if let Err(err) = bb.set_named(&mut self.shared, &o.key, &v) {
                    self.logs.push(format!("'{}': {err}", scene.name(e)));
                }
            }
            let seed = if c.seed != 0 {
                c.seed
            } else {
                name_seed(&scene.name(e))
            };
            out.push((e, tree.graphs.clone()));
            self.index.insert(e, self.agents.len());
            self.agents.push(BtAgent {
                entity: e,
                path: Rc::from(path.as_str()),
                exec: ExecState::new(&tree, seed),
                tree,
                bb,
                run: if c.start_on_play {
                    RunState::Running
                } else {
                    RunState::Stopped
                },
                accum: 0.0,
                finishes: Vec::new(),
                command: None,
                alive: frame,
            });
        }
        out
    }

    /// Objects destroyed: blackboards stop pointing at them.
    pub fn forget(&mut self, gone: &[Entity]) {
        for e in gone {
            for a in &mut self.agents {
                a.bb.forget_object(*e);
            }
        }
    }

    /// The debugger's view of the agent on `watch` (else the first agent running `path`).
    pub fn debug_view(&self, path: &str, watch: Option<Entity>) -> Option<DebugView> {
        let a = watch
            .and_then(|e| self.agent(e))
            .filter(|a| &*a.path == path)
            .or_else(|| self.agents.iter().find(|a| &*a.path == path))?;
        let ticks = a.exec.ticks;
        let nodes = a
            .tree
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let st = a.exec.nodes.get(i).copied().unwrap_or_default();
                (n.id, st.status, st.last, ticks.saturating_sub(st.last_tick))
            })
            .collect();
        Some(DebugView {
            entity: Some(a.entity),
            path: path.to_owned(),
            nodes,
            keys: a.bb.describe(&self.shared),
            run: a.run,
            ticks,
        })
    }

    /// Objects running behaviour trees, with their tree path (the editor's agent picker).
    pub fn agents_of(&self, path: &str) -> Vec<Entity> {
        self.agents
            .iter()
            .filter(|a| &*a.path == path)
            .map(|a| a.entity)
            .collect()
    }

    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }
}
