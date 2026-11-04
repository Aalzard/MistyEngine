//! Realtime probes: re-captured while the editor or game runs, every `refresh_interval` frames
//! (0 = once, or when [`RealtimeProbes::request`] asks). With time slicing one face is rendered
//! per frame; a finished cube is prefiltered on a worker thread and then replaces the probe's data
//! in `store`.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread::JoinHandle;

use glam::Vec3;

use super::capture::{CaptureInputs, CaptureSettings, ProbeCapture, REALTIME_SAMPLES};
use super::cube::Cube;
use super::{ProbeData, store};
use crate::render::Renderer;
use crate::render_scene::MaterialSource;
use crate::scene::Scene;
use crate::scene::reflection_probe::ProbeMode;

#[derive(Default)]
struct Job {
    faces: Vec<Vec<Vec3>>,
    /// Frame the last cube was finished (None = never captured).
    finished: Option<u64>,
    requested: bool,
}

#[derive(Default)]
pub struct RealtimeProbes {
    frame: u64,
    capture: Option<ProbeCapture>,
    jobs: HashMap<u64, Job>,
    filtering: Vec<(u64, JoinHandle<ProbeData>)>,
    /// Round-robin start, so several probes share the per-frame budget.
    next: usize,
}

impl RealtimeProbes {
    /// Re-capture probe `key` (an entity's bits) as soon as possible.
    pub fn request(&mut self, key: u64) {
        self.jobs.entry(key).or_default().requested = true;
    }

    /// One frame of work: at most one probe renders this frame. Returns the faces rendered.
    pub fn update(
        &mut self,
        renderer: &mut Renderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        materials: &dyn MaterialSource,
    ) -> usize {
        self.frame += 1;
        self.collect();
        let probes: Vec<_> = super::scene_probe_list(scene)
            .into_iter()
            .filter(|(_, p)| p.mode == ProbeMode::Realtime)
            .collect();
        if probes.is_empty() {
            return 0;
        }
        let frame = self.frame;
        let due = |job: Option<&Job>, interval: u32| match job {
            None => true,
            Some(j) if !j.faces.is_empty() || j.requested => true,
            Some(j) => match j.finished {
                None => true,
                Some(_) if interval == 0 => false,
                Some(f) => frame - f >= interval as u64,
            },
        };
        let busy: Vec<u64> = self.filtering.iter().map(|(k, _)| *k).collect();
        let n = probes.len();
        let pick = (0..n).map(|i| (self.next + i) % n).find(|&i| {
            let (e, p) = &probes[i];
            let key = e.to_bits().get();
            !busy.contains(&key) && due(self.jobs.get(&key), p.refresh_interval)
        });
        let Some(i) = pick else {
            return 0;
        };
        let (e, p) = &probes[i];
        let key = e.to_bits().get();
        let settings = CaptureSettings::of(p, scene.world_position(*e));
        if self.capture.as_ref().map(|c| c.size()) != Some(settings.size) {
            self.capture = Some(ProbeCapture::new(renderer, device, settings.size));
        }
        let capture = self.capture.as_ref().expect("made above");
        let inputs = CaptureInputs::from_scene(scene, materials);
        let job = self.jobs.entry(key).or_default();
        job.requested = false;
        let count = if p.time_slicing {
            1
        } else {
            6 - job.faces.len()
        };
        for _ in 0..count {
            let face = job.faces.len();
            let pixels = capture.face(renderer, device, queue, &inputs, &settings, face);
            job.faces.push(pixels);
        }
        if job.faces.len() == 6 {
            let mut cube = Cube::new(settings.size);
            for (dst, src) in cube.faces.iter_mut().zip(job.faces.drain(..)) {
                *dst = src;
            }
            job.finished = Some(frame);
            let handle =
                std::thread::spawn(move || ProbeData::from_capture(&cube, REALTIME_SAMPLES));
            self.filtering.push((key, handle));
            self.next = (i + 1) % n;
        }
        count
    }

    /// Store the cubes whose prefiltering finished.
    fn collect(&mut self) {
        let (done, running): (Vec<_>, Vec<_>) = std::mem::take(&mut self.filtering)
            .into_iter()
            .partition(|(_, h)| h.is_finished());
        self.filtering = running;
        for (key, handle) in done {
            if let Ok(data) = handle.join() {
                store::set_realtime(key, Arc::new(data));
            }
        }
    }

    /// Wait for all prefiltering (tests).
    pub fn finish(&mut self) {
        for (key, handle) in std::mem::take(&mut self.filtering) {
            if let Ok(data) = handle.join() {
                store::set_realtime(key, Arc::new(data));
            }
        }
    }
}
