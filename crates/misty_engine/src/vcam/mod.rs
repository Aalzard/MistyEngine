//! Virtual cameras: a Camera Brain on the real camera shows the live Virtual Camera (highest
//! priority, ties → most recently enabled) and blends between them. Each virtual camera has a
//! lens, a body (Transposer, Third Person, Orbital), an aim (Composer, Hard Look At) and
//! optional Collider and Noise extensions. Runs only while playing (see `CameraRig`).

pub mod components;
pub mod noise;
pub mod rig;
pub mod solve;

#[cfg(test)]
mod tests;

pub use components::*;
pub use rig::CameraRig;
pub use solve::{CamState, SolveContext, damp};
