//! The mixer graph that renders sound, on the audio thread (or offline in tests).
//!
//! - **Voices** read a clip (decoded in memory, or streamed by a worker thread), resample it to
//!   the output rate with 4-point Hermite interpolation at their pitch, pan it (or render it
//!   binaurally) and add it into their track.
//! - **Tracks** are mixer groups and per-source filter chains. Each block a track runs its input
//!   (voices plus child tracks) through its effect chain, applies its fader and adds the result
//!   to its parent. Sends copy the signal at their place in the chain into a Receive or Duck
//!   Volume unit on another track; tracks run in dependency order (children and senders before
//!   parents and receivers), so a send arrives in the same block. A loop (A sends to B, B to A)
//!   is cut where it closes and delivers one block late.
//! - **Live edits** never restart anything: inserted effects fade in, removed ones fade out
//!   (tails ring on), bypass fades, a reordered chain crossfades from the old chain (with its
//!   state) to the new one, and a rerouted track or voice feeds both destinations for
//!   [`super::dsp::FADE_SECONDS`] while it moves.
//!
//! Commands arrive through a lock-free queue; levels, positions and finished voices go back
//! through [`Status`], updated with `try_lock` so the audio thread never waits.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::Vec3;

use super::backend::{TrackId, VoiceId};
use super::dsp::fx::Processor;
use super::dsp::hrtf::Binaural;
use super::dsp::{BLOCK, Frame, Ramp, fade_samples};
use super::effects::{EffectKind, EffectSpec, db_to_amplitude};
use super::spatial::VoiceParams;
use super::stream::{Pull, StreamReader};

/// A clip's audio.
pub enum Source {
    /// Decoded in memory (Decompress On Load).
    Static { frames: Arc<[Frame]>, rate: u32 },
    /// Decoded by a worker thread as it plays (Streaming, Compressed In Memory).
    Stream(StreamReader),
}

/// An effect in a chain, ready to run (built on the main thread: no allocation while mixing).
pub struct Slot {
    pub spec: EffectSpec,
    proc: Processor,
    /// 0..1: how much of the effect is in the signal (fades on insert, remove and bypass).
    mix: Ramp,
    wet: Ramp,
    /// Send level (linear).
    level: Ramp,
    /// What Sends delivered this block (Receive, Duck Volume).
    received: Vec<Frame>,
    removing: bool,
}

impl Slot {
    /// A slot for `spec`; `fade_in` starts it silent and fades it in (inserted while playing).
    pub fn new(spec: EffectSpec, sample_rate: f32, fade_in: bool) -> Self {
        let on = if spec.bypass { 0.0 } else { 1.0 };
        let received = if spec.kind.receives_sends() {
            vec![Frame::ZERO; BLOCK]
        } else {
            Vec::new()
        };
        Self {
            proc: Processor::new(spec.kind, sample_rate),
            mix: Ramp::new(if fade_in { 0.0 } else { on }),
            wet: Ramp::new(spec.wet),
            level: Ramp::new(send_level(&spec)),
            received,
            removing: false,
            spec,
        }
    }

    fn update(&mut self, spec: &EffectSpec, fade: f32) {
        self.spec.values.clone_from(&spec.values);
        self.spec.bypass = spec.bypass;
        self.spec.target = spec.target;
        self.spec.wet = spec.wet;
        self.retarget(fade);
    }

    fn retarget(&mut self, fade: f32) {
        let on = if self.spec.bypass || self.removing {
            0.0
        } else {
            1.0
        };
        self.mix.set(on, fade);
        self.wet.set(self.spec.wet.clamp(0.0, 1.0), fade);
        self.level.set(send_level(&self.spec), fade);
    }
}

fn send_level(spec: &EffectSpec) -> f32 {
    if spec.kind == EffectKind::Send {
        db_to_amplitude(super::dsp::fx::value(spec.kind, &spec.values, 0))
    } else {
        0.0
    }
}

/// Main thread → audio thread.
pub enum Command {
    Play {
        id: VoiceId,
        source: Source,
        params: VoiceParams,
        track: Option<TrackId>,
    },
    SetVoice {
        id: VoiceId,
        params: VoiceParams,
    },
    Pause(VoiceId),
    Resume(VoiceId),
    Stop(VoiceId),
    Seek {
        id: VoiceId,
        seconds: f64,
    },
    MoveVoice {
        id: VoiceId,
        track: Option<TrackId>,
    },
    AddTrack {
        id: TrackId,
        parent: Option<TrackId>,
        chain: Vec<Slot>,
    },
    /// New fader and effect values (same chain).
    SetTrack {
        id: TrackId,
        volume: f32,
        effects: Vec<EffectSpec>,
    },
    /// A different chain: effects added, removed or moved.
    SetChain {
        id: TrackId,
        chain: Vec<Slot>,
    },
    SetParent {
        id: TrackId,
        parent: Option<TrackId>,
    },
    RemoveTrack(TrackId),
}

/// Audio thread → main thread.
#[derive(Default)]
pub struct Status {
    /// Output rate (0 until the first block).
    pub sample_rate: f32,
    /// Playback position (seconds) of each voice.
    pub positions: HashMap<VoiceId, f64>,
    /// Voices that finished on their own.
    pub ended: Vec<VoiceId>,
    /// Loudest sample per track since the main thread last took it.
    pub peaks: HashMap<TrackId, f32>,
    /// Strongest gain reduction (dB, ≤ 0) of a Compressor / Duck Volume per track since last taken.
    pub reductions: HashMap<TrackId, f32>,
    /// Samples a streamed voice had to wait for its worker.
    pub underruns: u64,
    /// Frames rendered so far.
    pub frames: u64,
}

struct Track {
    parent: Option<TrackId>,
    /// While rerouting: the previous parent and its share (1 → 0).
    reroute: Option<(Option<TrackId>, Ramp)>,
    volume: Ramp,
    chain: Vec<Slot>,
    /// After a reorder: the previous chain fading out (1 → 0), still ringing.
    old_chain: Option<(Vec<Slot>, Ramp)>,
    input: Vec<Frame>,
    peak: f32,
    reduction: f32,
    removing: bool,
}

/// Where a voice reads its clip.
struct Reader {
    source: Source,
    /// Stream: the last four frames, for indices `next - 4 .. next`.
    hist: [Frame; 4],
    next: i64,
    ended_at: Option<i64>,
    /// A decoded clip has wrapped round its loop at least once (before that, there is nothing
    /// before its first frame, as with a stream).
    looped: bool,
}

impl Reader {
    fn rate(&self) -> f32 {
        match &self.source {
            Source::Static { rate, .. } => *rate as f32,
            Source::Stream(s) => s.rate as f32,
        }
    }

    /// Frames in the clip (0 = unknown).
    fn len(&self) -> i64 {
        match &self.source {
            Source::Static { frames, .. } => frames.len() as i64,
            Source::Stream(s) => s.frames as i64,
        }
    }

    /// Frames `i - 1 ..= i + 2`; `None` when a stream hasn't caught up.
    fn window(
        &mut self,
        i: i64,
        looping: bool,
        wait: bool,
        underruns: &mut u64,
    ) -> Option<[Frame; 4]> {
        match &mut self.source {
            Source::Static { frames, .. } => {
                let len = frames.len() as i64;
                let looped = self.looped;
                let get = |k: i64| {
                    if k < 0 && !looped {
                        Frame::ZERO
                    } else if looping && len > 0 {
                        frames[k.rem_euclid(len) as usize]
                    } else if (0..len).contains(&k) {
                        frames[k as usize]
                    } else {
                        Frame::ZERO
                    }
                };
                Some([get(i - 1), get(i), get(i + 1), get(i + 2)])
            }
            Source::Stream(s) => {
                while self.next <= i + 2 {
                    let f = loop {
                        match s.pop() {
                            Pull::Frame(f) => break f,
                            Pull::End => {
                                self.ended_at.get_or_insert(self.next);
                                break Frame::ZERO;
                            }
                            Pull::Underrun if wait => std::thread::yield_now(),
                            Pull::Underrun => {
                                *underruns += 1;
                                return None;
                            }
                        }
                    };
                    self.hist.rotate_left(1);
                    self.hist[3] = f;
                    self.next += 1;
                }
                let base = self.next - 4;
                let at = |k: i64| self.hist[(k - base).clamp(0, 3) as usize];
                Some([at(i - 1), at(i), at(i + 1), at(i + 2)])
            }
        }
    }

    fn seek(&mut self, frame: i64) {
        self.looped = false;
        if let Source::Stream(s) = &mut self.source {
            // From the frame before, so interpolation right after the seek has its neighbour.
            let from = (frame - 1).max(0);
            s.seek(from as u64);
            self.hist = [Frame::ZERO; 4];
            self.next = from;
            self.ended_at = None;
        }
    }
}

struct Voice {
    id: VoiceId,
    reader: Reader,
    /// Read position in clip frames (streams: counts on through loops).
    pos: f64,
    params: VoiceParams,
    volume: Ramp,
    pan: Ramp,
    /// Share of the binaural rendering.
    spatial: Ramp,
    /// 1 = sounding; fades to 0 to pause or stop without a click.
    gate: Ramp,
    paused: bool,
    stopping: bool,
    finished: bool,
    track: Option<TrackId>,
    reroute: Option<(Option<TrackId>, Ramp)>,
    binaural: Option<Box<Binaural>>,
}

impl Voice {
    fn seconds(&self) -> f64 {
        let len = self.reader.len();
        let p = if len > 0 && self.params.looping {
            self.pos.rem_euclid(len as f64)
        } else {
            self.pos
        };
        p / self.reader.rate() as f64
    }
}

/// The graph.
pub struct Engine {
    pub sample_rate: f32,
    tracks: HashMap<TrackId, Track>,
    order: Vec<TrackId>,
    order_dirty: bool,
    voices: Vec<Voice>,
    rx: Option<rtrb::Consumer<Command>>,
    status: Arc<Mutex<Status>>,
    work: Vec<Frame>,
    scratch: Vec<Frame>,
    old: Vec<Frame>,
    voice_buf: Vec<Frame>,
    voice_raw: Vec<Frame>,
    master: Vec<Frame>,
    ended: Vec<VoiceId>,
    underruns: u64,
    frames: u64,
    /// Offline: wait for stream workers instead of playing silence (deterministic renders).
    pub wait_for_streams: bool,
}

impl Engine {
    pub fn new(
        sample_rate: f32,
        rx: Option<rtrb::Consumer<Command>>,
        status: Arc<Mutex<Status>>,
    ) -> Self {
        Self {
            sample_rate,
            tracks: HashMap::new(),
            order: Vec::new(),
            order_dirty: false,
            voices: Vec::new(),
            rx,
            status,
            work: vec![Frame::ZERO; BLOCK],
            scratch: vec![Frame::ZERO; BLOCK],
            old: vec![Frame::ZERO; BLOCK],
            voice_buf: vec![Frame::ZERO; BLOCK],
            voice_raw: vec![Frame::ZERO; BLOCK],
            master: vec![Frame::ZERO; BLOCK],
            ended: Vec::new(),
            underruns: 0,
            frames: 0,
            wait_for_streams: false,
        }
    }

    fn fade(&self) -> f32 {
        fade_samples(self.sample_rate)
    }

    // ---- commands -------------------------------------------------------------------------------

    fn poll(&mut self) {
        while let Some(cmd) = self.rx.as_mut().and_then(|rx| rx.pop().ok()) {
            self.apply(cmd);
        }
    }

    pub fn apply(&mut self, cmd: Command) {
        let fade = self.fade();
        match cmd {
            Command::Play {
                id,
                source,
                params,
                track,
            } => {
                self.voices.retain(|v| v.id != id);
                self.voices.push(Voice {
                    id,
                    reader: Reader {
                        source,
                        hist: [Frame::ZERO; 4],
                        next: 0,
                        ended_at: None,
                        looped: false,
                    },
                    pos: 0.0,
                    params,
                    volume: Ramp::new(params.volume),
                    pan: Ramp::new(params.pan),
                    spatial: Ramp::new(params.binaural.map_or(0.0, |b| b.mix)),
                    gate: Ramp::new(1.0),
                    paused: false,
                    stopping: false,
                    finished: false,
                    track,
                    reroute: None,
                    binaural: params
                        .binaural
                        .map(|_| Box::new(Binaural::new(self.sample_rate))),
                });
            }
            Command::SetVoice { id, params } => {
                let sr = self.sample_rate;
                if let Some(v) = self.voice(id) {
                    let smooth = fade * 0.5;
                    v.volume.set(params.volume, smooth);
                    v.pan.set(params.pan, smooth);
                    v.spatial
                        .set(params.binaural.map_or(0.0, |b| b.mix), smooth);
                    if let Source::Stream(s) = &v.reader.source
                        && params.looping != v.params.looping
                    {
                        s.set_looping(params.looping);
                    }
                    if params.binaural.is_some() && v.binaural.is_none() {
                        v.binaural = Some(Box::new(Binaural::new(sr)));
                    }
                    v.params = params;
                }
            }
            Command::Pause(id) => {
                if let Some(v) = self.voice(id) {
                    v.paused = true;
                    v.gate.set(0.0, fade * 0.25);
                }
            }
            Command::Resume(id) => {
                if let Some(v) = self.voice(id) {
                    v.paused = false;
                    v.gate.set(1.0, fade * 0.25);
                }
            }
            Command::Stop(id) => {
                if let Some(v) = self.voice(id) {
                    v.stopping = true;
                    v.gate.set(0.0, fade * 0.25);
                }
            }
            Command::Seek { id, seconds } => {
                if let Some(v) = self.voice(id) {
                    let frame = seconds.max(0.0) * v.reader.rate() as f64;
                    v.pos = frame;
                    v.reader.seek(frame.floor() as i64);
                }
            }
            Command::MoveVoice { id, track } => {
                if let Some(v) = self.voice(id)
                    && v.track != track
                {
                    let mut r = Ramp::new(1.0);
                    r.set(0.0, fade);
                    v.reroute = Some((v.track, r));
                    v.track = track;
                }
            }
            Command::AddTrack { id, parent, chain } => {
                self.tracks.insert(
                    id,
                    Track {
                        parent,
                        reroute: None,
                        volume: Ramp::new(1.0),
                        chain,
                        old_chain: None,
                        input: vec![Frame::ZERO; BLOCK],
                        peak: 0.0,
                        reduction: 0.0,
                        removing: false,
                    },
                );
                self.order_dirty = true;
            }
            Command::SetTrack {
                id,
                volume,
                effects,
            } => {
                if let Some(t) = self.tracks.get_mut(&id) {
                    t.volume.set(volume, fade * 0.5);
                    let live: Vec<usize> = (0..t.chain.len())
                        .filter(|i| !t.chain[*i].removing)
                        .collect();
                    for (n, spec) in effects.iter().enumerate() {
                        let slot = if spec.id != 0 {
                            t.chain
                                .iter_mut()
                                .find(|s| s.spec.id == spec.id && !s.removing)
                        } else {
                            live.get(n).and_then(|i| t.chain.get_mut(*i))
                        };
                        if let Some(s) = slot.filter(|s| s.spec.kind == spec.kind) {
                            s.update(spec, fade);
                        }
                    }
                    self.order_dirty = true;
                }
            }
            Command::SetChain { id, chain } => {
                if let Some(t) = self.tracks.get_mut(&id) {
                    merge_chain(t, chain, fade);
                    self.order_dirty = true;
                }
            }
            Command::SetParent { id, parent } => {
                if let Some(t) = self.tracks.get_mut(&id)
                    && t.parent != parent
                {
                    let mut r = Ramp::new(1.0);
                    r.set(0.0, fade);
                    t.reroute = Some((t.parent, r));
                    t.parent = parent;
                    self.order_dirty = true;
                }
            }
            Command::RemoveTrack(id) => {
                if let Some(t) = self.tracks.get_mut(&id) {
                    t.removing = true;
                    t.volume.set(0.0, fade);
                }
                // Its sounds stop with it.
                for v in self.voices.iter_mut().filter(|v| v.track == Some(id)) {
                    if !v.stopping {
                        v.stopping = true;
                        v.gate.set(0.0, fade * 0.25);
                        self.ended.push(v.id);
                    }
                }
            }
        }
    }

    fn voice(&mut self, id: VoiceId) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|v| v.id == id)
    }

    // ---- rendering ------------------------------------------------------------------------------

    /// Fill `out` with the next frames of the mix.
    pub fn render(&mut self, out: &mut [Frame]) {
        self.poll();
        for chunk in out.chunks_mut(BLOCK) {
            self.block(chunk);
        }
        self.publish();
    }

    fn block(&mut self, out: &mut [Frame]) {
        let n = out.len();
        for t in self.tracks.values_mut() {
            t.input[..n].fill(Frame::ZERO);
        }
        self.master[..n].fill(Frame::ZERO);
        if self.order_dirty {
            self.order = dependency_order(&self.tracks);
            self.order_dirty = false;
        }
        self.render_voices(n);
        let order = std::mem::take(&mut self.order);
        for id in &order {
            self.process_track(*id, n);
        }
        self.order = order;
        out.copy_from_slice(&self.master[..n]);
        // Tracks that finished fading out.
        let before = self.tracks.len();
        self.tracks
            .retain(|_, t| !(t.removing && t.volume.is_at(0.0)));
        if self.tracks.len() != before {
            self.order_dirty = true;
        }
        self.frames += n as u64;
    }

    fn render_voices(&mut self, n: usize) {
        let sr = self.sample_rate;
        let wait = self.wait_for_streams;
        let mut voices = std::mem::take(&mut self.voices);
        let mut buf = std::mem::take(&mut self.voice_buf);
        let mut raw = std::mem::take(&mut self.voice_raw);
        for v in &mut voices {
            if v.paused && v.gate.is_at(0.0) {
                continue;
            }
            let step = v.params.pitch.abs().max(0.01) as f64 * v.reader.rate() as f64 / sr as f64;
            let len = v.reader.len();
            let is_static = matches!(v.reader.source, Source::Static { .. });
            for f in raw[..n].iter_mut() {
                let i = v.pos.floor() as i64;
                let t = (v.pos - i as f64) as f32;
                match v
                    .reader
                    .window(i, v.params.looping, wait, &mut self.underruns)
                {
                    Some(w) => {
                        *f = hermite(w, t);
                        v.pos += step;
                    }
                    None => *f = Frame::ZERO,
                }
                if is_static {
                    if v.params.looping && len > 0 {
                        if v.pos >= len as f64 {
                            v.reader.looped = true;
                        }
                        v.pos = v.pos.rem_euclid(len as f64);
                    } else if v.pos >= len as f64 {
                        v.finished = true;
                    }
                } else if let Some(end) = v.reader.ended_at
                    && v.pos >= end as f64
                {
                    v.finished = true;
                }
            }
            // Gain, gate and pan (the reference's equal-power pan law).
            let binaural = v.params.binaural.map(|b| b.dir);
            for (o, r) in buf[..n].iter_mut().zip(raw[..n].iter_mut()) {
                let g = v.volume.next() * v.gate.next();
                *r = *r * g;
                let pan = v.pan.next().clamp(-1.0, 1.0);
                let mix = (pan + 1.0) * 0.5;
                *o = Frame::new(r.left * (1.0 - mix).sqrt(), r.right * mix.sqrt())
                    * std::f32::consts::SQRT_2;
            }
            if let (Some(dir), Some(b)) = (binaural, v.binaural.as_mut()) {
                b.process(Vec3::from(dir), &mut raw[..n]);
                for (o, r) in buf[..n].iter_mut().zip(raw[..n].iter()) {
                    let s = v.spatial.next();
                    *o = *o * (1.0 - s) + *r * s;
                }
            }
            if (v.stopping && v.gate.is_at(0.0)) || v.finished {
                v.finished = true;
            }
            route(
                &mut self.tracks,
                &mut self.master,
                v.track,
                &mut v.reroute,
                &buf[..n],
            );
        }
        voices.retain(|v| {
            if v.finished {
                if !v.stopping {
                    self.ended.push(v.id);
                }
                false
            } else {
                true
            }
        });
        self.voices = voices;
        self.voice_buf = buf;
        self.voice_raw = raw;
    }

    fn process_track(&mut self, id: TrackId, n: usize) {
        let sr = self.sample_rate;
        let Some(t) = self.tracks.get_mut(&id) else {
            return;
        };
        let mut work = std::mem::take(&mut self.work);
        let mut scratch = std::mem::take(&mut self.scratch);
        let mut old = std::mem::take(&mut self.old);
        work[..n].copy_from_slice(&t.input[..n]);
        let mut chain = std::mem::take(&mut t.chain);
        let mut old_chain = t.old_chain.take();
        if old_chain.is_some() {
            old[..n].copy_from_slice(&t.input[..n]);
        }
        let mut reduction = 0.0f32;
        run_chain(
            &mut chain,
            &mut self.tracks,
            &mut work[..n],
            &mut scratch,
            sr,
            &mut reduction,
        );
        chain.retain(|s| !(s.removing && s.mix.is_at(0.0)));
        if let Some((oc, ramp)) = old_chain.as_mut() {
            run_chain(
                oc,
                &mut self.tracks,
                &mut old[..n],
                &mut scratch,
                sr,
                &mut reduction,
            );
            for (w, o) in work[..n].iter_mut().zip(old[..n].iter()) {
                let r = ramp.next();
                *w = *w * (1.0 - r) + *o * r;
            }
        }
        let t = self.tracks.get_mut(&id).expect("still there");
        t.chain = chain;
        t.old_chain = old_chain.filter(|(_, r)| !r.is_at(0.0));
        let mut peak = t.peak;
        for w in work[..n].iter_mut() {
            *w = *w * t.volume.next();
            peak = peak.max(w.left.abs()).max(w.right.abs());
        }
        t.peak = peak;
        t.reduction = t.reduction.min(reduction);
        let parent = t.parent;
        let mut reroute = t.reroute.take();
        route(
            &mut self.tracks,
            &mut self.master,
            parent,
            &mut reroute,
            &work[..n],
        );
        if let Some(t) = self.tracks.get_mut(&id) {
            t.reroute = reroute;
        }
        self.work = work;
        self.scratch = scratch;
        self.old = old;
    }

    fn publish(&mut self) {
        let Ok(mut s) = self.status.try_lock() else {
            return;
        };
        s.sample_rate = self.sample_rate;
        for v in &self.voices {
            s.positions.insert(v.id, v.seconds());
        }
        s.ended.append(&mut self.ended);
        for (id, t) in &mut self.tracks {
            let p = s.peaks.entry(*id).or_insert(0.0);
            *p = p.max(t.peak);
            t.peak = 0.0;
            let r = s.reductions.entry(*id).or_insert(0.0);
            *r = r.min(t.reduction);
            t.reduction = 0.0;
        }
        s.underruns += std::mem::take(&mut self.underruns);
        s.frames = self.frames;
    }

    /// Voices currently in the graph.
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }
}

/// Add `buf` to `target`'s input (or the output), splitting it with the previous destination
/// while a reroute fades.
fn route(
    tracks: &mut HashMap<TrackId, Track>,
    master: &mut [Frame],
    target: Option<TrackId>,
    reroute: &mut Option<(Option<TrackId>, Ramp)>,
    buf: &[Frame],
) {
    match reroute {
        None => add_into(tracks, master, target, buf, |_| 1.0),
        Some((old, ramp)) => {
            let start = *ramp;
            let mut r = start;
            add_into(tracks, master, *old, buf, |_| r.next());
            let mut r = start;
            add_into(tracks, master, target, buf, |_| 1.0 - r.next());
            for _ in 0..buf.len() {
                ramp.next();
            }
            if ramp.is_at(0.0) {
                *reroute = None;
            }
        }
    }
}

fn add_into(
    tracks: &mut HashMap<TrackId, Track>,
    master: &mut [Frame],
    target: Option<TrackId>,
    buf: &[Frame],
    mut gain: impl FnMut(usize) -> f32,
) {
    let dst = match target.and_then(|t| tracks.get_mut(&t)) {
        Some(t) => &mut t.input[..buf.len()],
        None => &mut master[..buf.len()],
    };
    for (i, (d, s)) in dst.iter_mut().zip(buf).enumerate() {
        *d += *s * gain(i);
    }
}

/// Run a chain over `buf`. Sends write into their target's `received` buffer in `tracks`.
fn run_chain(
    chain: &mut [Slot],
    tracks: &mut HashMap<TrackId, Track>,
    buf: &mut [Frame],
    scratch: &mut [Frame],
    sr: f32,
    reduction: &mut f32,
) {
    let n = buf.len();
    for slot in chain.iter_mut() {
        let kind = slot.spec.kind;
        match kind {
            EffectKind::Send => {
                let Some((tid, eid)) = slot.spec.target else {
                    continue;
                };
                let Some(dst) = tracks.get_mut(&tid).and_then(|t| {
                    t.chain
                        .iter_mut()
                        .find(|s| s.spec.id == eid && s.spec.kind.receives_sends())
                }) else {
                    // Keep the ramps moving even with nowhere to send.
                    for _ in 0..n {
                        slot.mix.next();
                        slot.level.next();
                    }
                    continue;
                };
                for (r, s) in dst.received[..n].iter_mut().zip(buf.iter()) {
                    *r += *s * (slot.level.next() * slot.mix.next());
                }
            }
            EffectKind::Receive => {
                for (b, r) in buf.iter_mut().zip(slot.received[..n].iter_mut()) {
                    *b += *r * slot.mix.next();
                    *r = Frame::ZERO;
                }
            }
            _ => {
                if slot.mix.is_at(0.0) {
                    slot.received[..].fill(Frame::ZERO);
                    continue;
                }
                scratch[..n].copy_from_slice(buf);
                let side = (kind == EffectKind::DuckVolume).then(|| &slot.received[..n]);
                slot.proc
                    .process(kind, &slot.spec.values, &mut scratch[..n], sr, side);
                *reduction = reduction.min(slot.proc.reduction_db());
                let routing = kind.is_routing();
                for (b, s) in buf.iter_mut().zip(scratch[..n].iter()) {
                    let m = slot.mix.next();
                    let a = if routing { m } else { m * slot.wet.next() };
                    *b = *b * (1.0 - a) + *s * a;
                }
                if !slot.received.is_empty() {
                    slot.received[..n].fill(Frame::ZERO);
                }
            }
        }
    }
}

/// Fit a new chain onto a track while it plays.
///
/// When the effects that stay keep their order, they keep their state; new ones fade in and
/// dropped ones fade out in place. A reorder crossfades from the whole old chain (still
/// ringing) to the new one.
fn merge_chain(t: &mut Track, new: Vec<Slot>, fade: f32) {
    let old_ids: Vec<u32> = t
        .chain
        .iter()
        .filter(|s| !s.removing)
        .map(|s| s.spec.id)
        .collect();
    let new_ids: Vec<u32> = new.iter().map(|s| s.spec.id).collect();
    let kept_old: Vec<u32> = old_ids
        .iter()
        .copied()
        .filter(|id| new_ids.contains(id))
        .collect();
    let kept_new: Vec<u32> = new_ids
        .iter()
        .copied()
        .filter(|id| old_ids.contains(id))
        .collect();
    if kept_old != kept_new {
        // Reordered.
        let mut ramp = Ramp::new(1.0);
        ramp.set(0.0, fade);
        let old = std::mem::take(&mut t.chain);
        t.old_chain = Some((old, ramp));
        t.chain = new;
        return;
    }
    let mut incoming: std::collections::VecDeque<Slot> = new.into();
    let mut result = Vec::with_capacity(t.chain.len() + incoming.len());
    for mut slot in std::mem::take(&mut t.chain) {
        if !slot.removing && new_ids.contains(&slot.spec.id) {
            while let Some(front) = incoming.pop_front() {
                if front.spec.id == slot.spec.id {
                    slot.update(&front.spec, fade);
                    break;
                }
                result.push(fade_in(front, fade));
            }
            result.push(slot);
        } else {
            slot.removing = true;
            slot.retarget(fade);
            result.push(slot);
        }
    }
    result.extend(incoming.into_iter().map(|s| fade_in(s, fade)));
    t.chain = result;
}

fn fade_in(mut slot: Slot, fade: f32) -> Slot {
    slot.mix = Ramp::new(0.0);
    slot.retarget(fade);
    slot
}

/// Tracks in processing order: a track after every track that feeds it (children, senders).
fn dependency_order(tracks: &HashMap<TrackId, Track>) -> Vec<TrackId> {
    let mut ids: Vec<TrackId> = tracks.keys().copied().collect();
    ids.sort_unstable();
    // Edges feeder → fed.
    let mut fed_by: HashMap<TrackId, Vec<TrackId>> = HashMap::new();
    for id in &ids {
        let t = &tracks[id];
        let mut outs: Vec<TrackId> = t.parent.into_iter().collect();
        outs.extend(t.reroute.as_ref().and_then(|(p, _)| *p));
        let chains = t
            .chain
            .iter()
            .chain(t.old_chain.iter().flat_map(|(c, _)| c));
        outs.extend(chains.filter_map(|s| s.spec.target.map(|(tid, _)| tid)));
        for o in outs {
            if o != *id && tracks.contains_key(&o) {
                fed_by.entry(o).or_default().push(*id);
            }
        }
    }
    let mut pending: HashMap<TrackId, usize> = ids
        .iter()
        .map(|id| (*id, fed_by.get(id).map_or(0, |v| v.len())))
        .collect();
    let mut feeds: HashMap<TrackId, Vec<TrackId>> = HashMap::new();
    for (fed, feeders) in &fed_by {
        for f in feeders {
            feeds.entry(*f).or_default().push(*fed);
        }
    }
    let mut ready: Vec<TrackId> = ids.iter().copied().filter(|id| pending[id] == 0).collect();
    let mut out = Vec::with_capacity(ids.len());
    while let Some(id) = ready.pop() {
        out.push(id);
        for f in feeds.get(&id).into_iter().flatten() {
            let p = pending.get_mut(f).expect("known");
            *p -= 1;
            if *p == 0 {
                ready.push(*f);
            }
        }
    }
    // A loop: the rest in id order (the edge closing it delivers a block late).
    for id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// 4-point Hermite interpolation between `w[1]` and `w[2]` at `t` (0..1).
#[inline]
fn hermite(w: [Frame; 4], t: f32) -> Frame {
    if t == 0.0 {
        return w[1];
    }
    let [x0, x1, x2, x3] = w;
    let c1 = (x2 - x0) * 0.5;
    let c2 = x0 - x1 * 2.5 + x2 * 2.0 - x3 * 0.5;
    let c3 = (x3 - x0) * 0.5 + (x1 - x2) * 1.5;
    ((c3 * t + c2) * t + c1) * t + x1
}

// ---- the device ------------------------------------------------------------------------------

/// The graph as one endless kira sound on the main output.
pub struct EngineSound(pub Engine);

impl kira::sound::Sound for EngineSound {
    fn process(&mut self, out: &mut [Frame], dt: f64, _info: &kira::info::Info) {
        if dt > 0.0 {
            self.0.sample_rate = (1.0 / dt) as f32;
        }
        self.0.render(out);
    }

    fn finished(&self) -> bool {
        false
    }
}

impl kira::sound::SoundData for EngineSound {
    type Error = ();
    type Handle = ();

    fn into_sound(self) -> Result<(Box<dyn kira::sound::Sound>, ()), ()> {
        Ok((Box::new(self), ()))
    }
}
