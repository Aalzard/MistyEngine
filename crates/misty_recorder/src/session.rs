//! One running recording: its schedule (which frames of capture time to keep), its output and
//! the worker thread encoding it. The editor steps it once per frame of capture time and hands
//! it the frames that come back from the GPU, and the taps on the game's mix for its sound.

use misty_engine::audio::MixTap;

use crate::audio::{AudioFeed, samples_in};
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
    /// The sound (outputs that record it).
    feed: Option<AudioFeed>,
}

impl Recording {
    /// Open the output and start the encoder.
    pub fn start(settings: RecorderSettings, output: Output) -> Result<Self, String> {
        let sink = open_sink(&output)?;
        let feed = output.audio.then(AudioFeed::new);
        Ok(Self {
            feed,
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

    /// This recording keeps the game's sound.
    pub fn wants_audio(&self) -> bool {
        self.feed.is_some()
    }

    /// A tap on the game's mix (taken in capture time).
    pub fn add_tap(&mut self, tap: MixTap) {
        if let Some(f) = self.feed.as_mut() {
            f.add_tap(tap);
        }
    }

    /// Its sound lost the tap that fed it while the game goes on (the mix closed it): the
    /// editor gives it a new one.
    pub fn needs_tap(&self) -> bool {
        self.feed.as_ref().is_some_and(AudioFeed::needs_tap)
    }

    /// A frame was captured at capture-time second `at`: the first one starts the sound.
    pub fn mark_frame(&mut self, at: f64) {
        if let Some(f) = self.feed.as_mut() {
            f.begin(at);
        }
    }

    /// Capture time reached `now`: hand the sound that is ready to the encoder.
    pub fn pump_audio(&mut self, now: f64) {
        if self.error.is_some() {
            return;
        }
        let Some(f) = self.feed.as_mut() else { return };
        let samples = f.pull(now);
        if let Some(w) = self.writer.as_mut()
            && let Err(e) = w.send_audio(samples)
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

    /// Wait for the encoder and finish the files. The sound is cut or padded to exactly the
    /// length of the frames.
    pub fn finish(mut self) -> Result<Summary, String> {
        let mut writer = self.writer.take().ok_or("the recording is finished")?;
        if let Some(feed) = self.feed.take() {
            let fps = self.settings.frame_rate() as f64;
            let total = samples_in(writer.sent() as f64 / fps);
            let rest = feed.finish(total, std::time::Duration::from_millis(300));
            if let Err(e) = writer.send_audio(rest) {
                self.error.get_or_insert(e);
            }
        }
        let summary = writer.finish();
        match self.error.take() {
            Some(e) => Err(e),
            None => summary,
        }
    }
}
