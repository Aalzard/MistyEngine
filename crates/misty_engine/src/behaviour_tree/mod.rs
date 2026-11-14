//! Behaviour trees: AI for enemies and NPCs built from nodes (the optional Behaviour Trees
//! package of the editor; the runtime is always part of the engine so built games run them).
//!
//! - [`asset`]: `.btree` files: nodes (composites, decorators, actions, conditions, scripting
//!   leaves), blackboard keys, comments and groups.
//! - [`blackboard`]: typed keys (bool, int, float, vector, object, string), per-agent values and
//!   shared blackboards.
//! - [`compile`]: an asset turned into a flat, depth-first node list with resolved keys.
//! - [`exec`]: the tick, event-driven aborts, and the [`exec::Host`] trait that reaches the world.
//! - [`component`]: the Behaviour Tree Agent component.
//! - [`runtime`]: the agents of a running game, shared blackboards and the debugger's view.
//!
//! The script host ticks the agents after Script Graphs' Update and before navigation, so a
//! Move To started this frame moves the agent this frame (scripting/behaviour_tree_api.rs, which
//! also holds the `BehaviourTree` Lua table).

pub mod asset;
pub mod blackboard;
pub mod compile;
pub mod component;
pub mod exec;
pub mod runtime;

#[cfg(test)]
mod tests;

pub use asset::{BehaviourTree, BtNode, EXTENSION, NodeKind, is_tree_path};
pub use blackboard::{BbValue, Blackboard, KeyDef, KeyType};
pub use component::BehaviourTreeAgent;
pub use exec::Status;
pub use runtime::BtRuntime;
