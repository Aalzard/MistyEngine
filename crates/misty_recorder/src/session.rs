//! One running recording: its schedule (which frames of capture time to keep), its output and
//! the worker thread encoding it. The editor steps it once per frame of capture time and hands
//! it the frames that come back from the GPU.

use crate::schedule::{Schedule, Step};
use crate::writer::{Summary, Writer};
use crate::{Frame, Output, RecorderSettings, open_sink};

pub struct Recording {
    pub settings: RecorderSettings,
    pub output: Output,
    schedule: Schedule,
    writer: Option<Writer>,
    /// Frames asked for (captured on the GPU) so far.
    requested: u64,
    error: Option<String>,
}

impl Recording {
    /// Open the output and start the encoder.
    pub fn start(settings: RecorderSettings, output: Output) -> Result<Self, String> {
        let sink = open_sink(&output)?;
        Ok(Self {
            schedule: Schedule::for_settings(&settings),
            settings,
            output,
            writer: Some(Writer::spawn(sink)),
            requested: 0,
            error: None,
        })
    }

    /// The game advanced one frame of capture time: keep it, skip it, or stop.
    pub fn step(&mut self) -> Step {
        let s = self.schedule.step();
        if matches!(s, Step::Capture(_)) {
            self.requested += 1;
        }
        s
    }

    /// Every frame of the range was asked for.
    pub fn complete(&self) -> bool {
        self.schedule.complete()
    }

    /// A captured frame back from the GPU (in order).
    pub fn push(&mut self, frame: Frame) {
        if self.error.is_some() {
            return;
        }
        if let Some(w) = self.writer.as_mut()
            && let Err(e) = w.send(frame)
        {
            self.error = Some(e);
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Frames asked for so far.
    pub fn requested(&self) -> u64 {
        self.requested
    }

    /// Frames handed to the encoder so far.
    pub fn written(&self) -> u64 {
        self.writer.as_ref().map_or(0, Writer::sent)
    }

    /// Frames the range holds (`None` = open-ended).
    pub fn total(&self) -> Option<u64> {
        self.schedule.total()
    }

    /// Frames of capture time stepped through (skipped ones included).
    pub fn elapsed(&self) -> u64 {
        self.schedule.elapsed()
    }

    /// Wait for the encoder and finish the files.
    pub fn finish(mut self) -> Result<Summary, String> {
        let writer = self.writer.take().ok_or("the recording is finished")?;
        let summary = writer.finish();
        match self.error.take() {
            Some(e) => Err(e),
            None => summary,
        }
    }
}
