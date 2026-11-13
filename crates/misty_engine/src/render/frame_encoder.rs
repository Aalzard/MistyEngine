//! A frame's commands recorded into a few command encoders in a row instead of one.
//!
//! wgpu 30 records commands as a list and does the real work (validation, resource tracking,
//! the backend's command encoding) when an encoder is finished: for a big scene that is most
//! of the render's CPU time (NoonValley in the editor: 1.7 of 3.6 ms). [`FrameEncoder::split`]
//! closes the part recorded so far and finishes it on a worker of rayon's global pool while the
//! render thread records the next part; [`FrameEncoder::finish`] finishes the last part and
//! returns every command buffer in recording order, submitted together (wgpu orders the
//! resource transitions between them at submit, as it does between passes of one encoder).
//!
//! It dereferences to the current [`wgpu::CommandEncoder`], so the passes take
//! `&mut encoder` as before.

use std::ops::{Deref, DerefMut};
use std::sync::mpsc;

pub(crate) struct FrameEncoder {
    current: wgpu::CommandEncoder,
    /// Earlier parts being finished on workers, in recording order.
    parts: Vec<mpsc::Receiver<wgpu::CommandBuffer>>,
}

impl FrameEncoder {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            current: encoder(device),
            parts: Vec::new(),
        }
    }

    /// Close the commands recorded so far (finished on a worker) and record on in a new
    /// encoder.
    pub fn split(&mut self, device: &wgpu::Device) {
        let done = std::mem::replace(&mut self.current, encoder(device));
        let (tx, rx) = mpsc::sync_channel(1);
        rayon::spawn(move || {
            crate::profile_scope!("Render.Submit.FinishPart");
            let _ = tx.send(done.finish());
        });
        self.parts.push(rx);
    }

    /// Every part's command buffer, in recording order (waits for the workers' parts).
    pub fn finish(self) -> Vec<wgpu::CommandBuffer> {
        let last = self.current.finish();
        let mut out: Vec<wgpu::CommandBuffer> = (self.parts.into_iter())
            .map(|rx| rx.recv().expect("a finishing worker panicked"))
            .collect();
        out.push(last);
        out
    }
}

fn encoder(device: &wgpu::Device) -> wgpu::CommandEncoder {
    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("misty frame"),
    })
}

impl Deref for FrameEncoder {
    type Target = wgpu::CommandEncoder;

    fn deref(&self) -> &wgpu::CommandEncoder {
        &self.current
    }
}

impl DerefMut for FrameEncoder {
    fn deref_mut(&mut self) -> &mut wgpu::CommandEncoder {
        &mut self.current
    }
}
