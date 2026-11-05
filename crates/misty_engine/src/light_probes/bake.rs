//! Light probe baking: render a small cube at every probe with the reflection probe capture
//! renderer, seeing only the static scene (static renderers, terrain, sky; lights applied), and
//! project it onto order-2 SH. With two bounces the probes are captured again with the static
//! renderers themselves lit by the first result, so light bounces twice.
//!
//! [`Baker`] does the work in steps (one probe per call) so the editor can spread it over frames
//! with a progress bar; [`bake_scene`] runs it to the end.

use std::sync::Arc;

use glam::Vec3;

use super::{FieldHandle, LightProbeData, LightProbeField};
use crate::render::Renderer;
use crate::render::probes::capture::{CaptureInputs, CaptureSettings, ProbeCapture};
use crate::render::probes::sh;
use crate::render_scene::MaterialSource;
use crate::scene::reflection_probe::ProbeClearFlags;
use crate::scene::render_settings::SunDisk;
use crate::scene::{GameObject, MeshRenderer, Scene};

/// Bake options (Lighting window ▸ Light Probes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BakeSettings {
    /// 1 = direct light seen by the probes; 2 = the statics lit by the first bake as well.
    pub bounces: u32,
    /// Cube face size of each capture.
    pub resolution: u32,
}

impl Default for BakeSettings {
    fn default() -> Self {
        Self {
            bounces: 2,
            resolution: 32,
        }
    }
}

/// What the probes see: the scene with every non-static renderer switched off.
pub fn static_inputs(scene: &Scene, materials: &dyn MaterialSource) -> CaptureInputs {
    let mut copy = scene.clone();
    let movers: Vec<hecs::Entity> = copy
        .world
        .query::<(hecs::Entity, &GameObject, &MeshRenderer)>()
        .iter()
        .filter(|(_, go, _)| !go.is_static)
        .map(|(e, _, _)| e)
        .collect();
    for e in movers {
        if let Ok(mut r) = copy.world.get::<&mut MeshRenderer>(e) {
            r.enabled = false;
        }
    }
    // Probes keep indirect light only: lights stay realtime, so the sun disk (direct light)
    // is left out of the sky they see, and so are earlier bakes.
    copy.render_settings.light_probes = None;
    copy.render_settings.skybox.sun_disk = SunDisk::None;
    let mut inputs = CaptureInputs::from_scene(&copy, materials);
    for m in &mut inputs.meshes {
        m.material.light_probes = false;
    }
    inputs
}

/// A bake in progress.
pub struct Baker {
    positions: Vec<Vec3>,
    settings: BakeSettings,
    inputs: CaptureInputs,
    capture: Option<ProbeCapture>,
    pass: u32,
    index: usize,
    current: Vec<[Vec3; 9]>,
    previous: Vec<[Vec3; 9]>,
}

impl Baker {
    /// A bake of the probes at `positions` (world) in `scene`; `None` without probes.
    pub fn new(
        scene: &Scene,
        materials: &dyn MaterialSource,
        positions: Vec<Vec3>,
        settings: BakeSettings,
    ) -> Option<Self> {
        if positions.is_empty() {
            return None;
        }
        Some(Self {
            inputs: static_inputs(scene, materials),
            positions,
            settings: BakeSettings {
                bounces: settings.bounces.clamp(1, 2),
                resolution: settings.resolution.clamp(8, 128),
            },
            capture: None,
            pass: 0,
            index: 0,
            current: Vec::new(),
            previous: Vec::new(),
        })
    }

    pub fn probe_count(&self) -> usize {
        self.positions.len()
    }

    /// Fraction done and a label.
    pub fn progress(&self) -> (f32, String) {
        let n = self.positions.len();
        let total = (n * self.settings.bounces as usize).max(1);
        let done = self.pass as usize * n + self.index;
        let bounce = if self.settings.bounces > 1 {
            format!(" (bounce {})", self.pass + 1)
        } else {
            String::new()
        };
        (
            done as f32 / total as f32,
            format!(
                "Baking light probe {} of {n}{bounce}",
                (self.index + 1).min(n)
            ),
        )
    }

    pub fn is_done(&self) -> bool {
        self.pass >= self.settings.bounces
    }

    /// Capture the next probe. True when the bake is finished.
    pub fn step(
        &mut self,
        renderer: &mut Renderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> bool {
        if self.is_done() {
            return true;
        }
        let size = self.settings.resolution;
        let capture = self
            .capture
            .get_or_insert_with(|| ProbeCapture::new(renderer, device, size));
        let s = CaptureSettings {
            position: self.positions[self.index],
            size,
            near: 0.05,
            far: 2000.0,
            clear: ProbeClearFlags::Skybox,
            background: [0.0; 3],
        };
        let cube = capture.cube(renderer, device, queue, &self.inputs, &s);
        self.current.push(sh::project(&cube));
        self.index += 1;
        if self.index == self.positions.len() {
            self.index = 0;
            self.pass += 1;
            self.previous = std::mem::take(&mut self.current);
            if !self.is_done() {
                // Next bounce: statics lit by these probes.
                let field = LightProbeField::new(self.positions.clone(), &self.previous);
                self.inputs.environment.probes.light_probes = Some(FieldHandle(Arc::new(field)));
                for m in &mut self.inputs.meshes {
                    m.material.light_probes = true;
                }
            }
        }
        self.is_done()
    }

    /// The baked probes (after the last step).
    pub fn finish(self) -> LightProbeData {
        LightProbeData::new(self.positions, &self.previous)
    }
}

/// Bake the probes at `positions` in one go (tests, sample generators).
pub fn bake_scene(
    renderer: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    materials: &dyn MaterialSource,
    positions: Vec<Vec3>,
    settings: BakeSettings,
) -> Option<LightProbeData> {
    let mut b = Baker::new(scene, materials, positions, settings)?;
    while !b.step(renderer, device, queue) {}
    Some(b.finish())
}
