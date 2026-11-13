//! A recording's worker thread: frames go in through a bounded queue and the [`Sink`] encodes
//! them in order. When the queue is full, [`Writer::send`] waits: capture time is fixed, so a
//! slow encoder makes the recording take longer, never lose frames.

use std::path::PathBuf;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::JoinHandle;

use crate::{Frame, Sink};

/// Frames waiting for the encoder at most (each is width x height x 4 bytes).
pub const QUEUE: usize = 6;

/// What a finished recording wrote.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub frames: u64,
    pub files: Vec<PathBuf>,
}

type Done = Result<(u64, Vec<PathBuf>), String>;

pub struct Writer {
    tx: Option<SyncSender<Frame>>,
    handle: Option<JoinHandle<Done>>,
    sent: u64,
}

impl Writer {
    pub fn spawn(mut sink: Box<dyn Sink>) -> Self {
        let (tx, rx) = sync_channel::<Frame>(QUEUE);
        let handle = std::thread::Builder::new()
            .name("misty recorder encoder".into())
            .spawn(move || {
                let mut frames = 0u64;
                let mut failed = None;
                for frame in rx {
                    if failed.is_some() {
                        continue;
                    }
                    match sink.write(frame) {
                        Ok(()) => frames += 1,
                        Err(e) => failed = Some(e),
                    }
                }
                let files = sink.finish();
                match failed {
                    Some(e) => Err(e),
                    None => files.map(|f| (frames, f)),
                }
            })
            .expect("spawn recorder encoder");
        Self {
            tx: Some(tx),
            handle: Some(handle),
            sent: 0,
        }
    }

    /// Queue a frame (waits while the encoder is behind).
    pub fn send(&mut self, frame: Frame) -> Result<(), String> {
        let tx = self.tx.as_ref().ok_or("the recording is finished")?;
        tx.send(frame)
            .map_err(|_| "the encoder stopped".to_owned())?;
        self.sent += 1;
        Ok(())
    }

    /// Frames handed to the encoder.
    pub fn sent(&self) -> u64 {
        self.sent
    }

    /// Close the queue, wait for the encoder to write everything and finish its files.
    pub fn finish(mut self) -> Result<Summary, String> {
        self.tx = None;
        let handle = self.handle.take().ok_or("the recording is finished")?;
        let (frames, files) = handle
            .join()
            .map_err(|_| "the encoder crashed".to_owned())??;
        Ok(Summary { frames, files })
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
