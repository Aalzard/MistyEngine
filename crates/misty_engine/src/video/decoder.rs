//! Background decoding: a worker thread decodes frames in order into a small queue, so the game
//! thread only picks finished images. Seeking restarts the worker at a new frame; frames from
//! before the seek carry an old generation number and are dropped.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError, channel, sync_channel};

use image::RgbaImage;

use super::source::VideoSource;

/// Frames decoded ahead of playback.
pub const QUEUE: usize = 4;

/// A decoded frame. `seq` counts frames across loops (`loop * frame_count + index`).
pub struct Decoded {
    pub generation: u64,
    pub seq: u64,
    pub index: u64,
    pub image: Result<RgbaImage, String>,
}

enum Cmd {
    /// Start decoding at `frame` (sequence restarts at `frame`).
    Seek {
        frame: u64,
        generation: u64,
    },
    Looping(bool),
}

/// The game thread's end of a decoding worker. Dropping it ends the thread.
pub struct Decoder {
    commands: Sender<Cmd>,
    frames: Receiver<Decoded>,
    generation: u64,
}

impl Decoder {
    /// Start a worker decoding `source` from frame 0.
    pub fn start(source: Arc<VideoSource>, looping: bool) -> Self {
        let (commands, cmd_rx) = channel();
        let (tx, frames) = sync_channel(QUEUE);
        std::thread::Builder::new()
            .name("misty video decoder".into())
            .spawn(move || worker(source, cmd_rx, tx, looping))
            .expect("spawn the video decoder thread");
        Self {
            commands,
            frames,
            generation: 0,
        }
    }

    /// Restart at `frame`; frames already queued are thrown away.
    pub fn seek(&mut self, frame: u64) {
        self.generation += 1;
        let _ = self.commands.send(Cmd::Seek {
            frame,
            generation: self.generation,
        });
        while self.frames.try_recv().is_ok() {}
    }

    pub fn set_looping(&self, looping: bool) {
        let _ = self.commands.send(Cmd::Looping(looping));
    }

    /// The next decoded frame of the current generation, if one is ready.
    pub fn try_next(&self) -> Option<Decoded> {
        loop {
            match self.frames.try_recv() {
                Ok(d) if d.generation == self.generation => return Some(d),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    }

    /// Wait up to `timeout` for the next frame of the current generation.
    pub fn next_timeout(&self, timeout: std::time::Duration) -> Option<Decoded> {
        let end = std::time::Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(std::time::Instant::now());
            match self.frames.recv_timeout(left) {
                Ok(d) if d.generation == self.generation => return Some(d),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    }
}

fn worker(
    source: Arc<VideoSource>,
    commands: Receiver<Cmd>,
    out: SyncSender<Decoded>,
    looping: bool,
) {
    let count = source.info().frame_count.max(1);
    let (mut index, mut seq, mut generation, mut looping) = (0u64, 0u64, 0u64, looping);
    let apply = |c: Cmd, index: &mut u64, seq: &mut u64, generation: &mut u64, l: &mut bool| match c
    {
        Cmd::Seek {
            frame,
            generation: g,
        } => {
            *index = frame.min(count - 1);
            *seq = *index;
            *generation = g;
        }
        Cmd::Looping(on) => *l = on,
    };
    loop {
        // Take every pending command first.
        loop {
            match commands.try_recv() {
                Ok(c) => apply(c, &mut index, &mut seq, &mut generation, &mut looping),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        if index >= count {
            if looping {
                index = 0;
            } else {
                // Past the end: wait for a seek (or the player going away).
                match commands.recv() {
                    Ok(c) => apply(c, &mut index, &mut seq, &mut generation, &mut looping),
                    Err(_) => return,
                }
                continue;
            }
        }
        let image = source.frame(index);
        let d = Decoded {
            generation,
            seq,
            index,
            image,
        };
        if out.send(d).is_err() {
            return;
        }
        index += 1;
        seq += 1;
    }
}
