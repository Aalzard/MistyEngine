//! Frame Debugger capture: an optional recorder the renderer reports every pass and draw call
//! to, and that can stop the frame after a chosen event (replay up to event N).
//!
//! With no capture installed the renderer pays one `Option::is_none` check per hook. While a
//! capture is installed ([`super::Renderer::begin_capture`]) every hook builds a
//! [`FrameEvent`]; events after [`CaptureRequest::stop_after`] are recorded but not drawn, and
//! the selected event's render target is shown in the output image (see `blit.rs`).
//! GPU culling and GPU timing are off while capturing so every draw is a CPU-side event.

mod blit;
pub(super) mod events;
#[cfg(test)]
mod tests;

pub(crate) use blit::DebugBlit;

use std::sync::Mutex;

/// Names the editor knows for the frame's mesh draws (by index into `Frame::meshes`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawLabel {
    /// GameObject name.
    pub object: String,
    /// Material asset path (`None` = default material or a script instance).
    pub material: Option<String>,
    /// Materials element of the object this draw is.
    pub submesh: u32,
}

/// What a capture should do.
#[derive(Clone, Debug, Default)]
pub struct CaptureRequest {
    /// Draw events `0..=n` only and show event `n`'s render target (`None` = whole frame).
    pub stop_after: Option<usize>,
    /// Object / material names per mesh draw (may be shorter than the draw list).
    pub labels: Vec<DrawLabel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// A pass begins and clears its attachments.
    Clear,
    /// Indexed / instanced geometry.
    Draw,
    /// A fullscreen triangle (sky, grid, post-processing).
    Fullscreen,
    /// Compute dispatches (AO, froxels, clouds).
    Compute,
    /// A texture copy (opaque scene copy).
    Copy,
    /// Canvas UI drawn over the image.
    Ui,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Draw => "Draw",
            Self::Fullscreen => "Fullscreen",
            Self::Compute => "Compute",
            Self::Copy => "Copy",
            Self::Ui => "UI",
        }
    }
}

/// Which image an event writes (what replay shows in the Game view).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventTarget {
    /// No colour image to show (compute passes, prepasses).
    #[default]
    None,
    /// The scene's HDR colour (resolved from MSAA at the end of the main pass).
    Hdr,
    /// A shadow map layer: sun cascade or local light layer.
    Shadow { local: bool, layer: u32 },
    /// A bloom mip.
    Bloom(u32),
    /// The composite before FXAA / sharpening.
    PreFinish,
    /// The final image.
    Output,
}

/// Size and format of an event's render target.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TargetInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

/// One recorded event of the frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameEvent {
    /// Pass path, outermost first ("Shadows", "Cascade 0").
    pub pass: Vec<String>,
    /// Row label ("Draw Mesh Cube").
    pub name: String,
    pub kind: Option<EventKind>,
    pub object: Option<String>,
    pub mesh: Option<String>,
    pub material: Option<String>,
    /// Materials element (submesh) of the object.
    pub submesh: Option<u32>,
    pub vertices: u32,
    pub indices: u32,
    pub instances: u32,
    /// Why the draw is (or isn't) batched.
    pub batch: Option<String>,
    /// Shader / pipeline description.
    pub shader: String,
    pub target: TargetInfo,
    pub display: EventTarget,
    pub blend: String,
    pub depth: String,
    pub cull: String,
    /// (slot, project-relative texture path).
    pub textures: Vec<(String, String)>,
    /// Material values (name, value).
    pub properties: Vec<(String, String)>,
}

impl FrameEvent {
    pub fn new(kind: EventKind, name: impl Into<String>) -> Self {
        Self {
            kind: Some(kind),
            name: name.into(),
            instances: 1,
            ..Default::default()
        }
    }

    pub fn kind(&self) -> EventKind {
        self.kind.unwrap_or(EventKind::Draw)
    }

    /// The outermost pass name ("Shadows", "Opaque", "Post").
    pub fn root(&self) -> &str {
        self.pass.first().map_or("", String::as_str)
    }

    /// Draws geometry (counts toward the draw call total).
    pub fn is_draw(&self) -> bool {
        matches!(self.kind(), EventKind::Draw | EventKind::Fullscreen)
    }
}

/// The events of one captured frame.
#[derive(Clone, Debug, Default)]
pub struct FrameCapture {
    pub events: Vec<FrameEvent>,
    /// Size of the view the frame was rendered for.
    pub size: (u32, u32),
    /// The event the frame stopped after (when replaying).
    pub stopped_at: Option<usize>,
}

impl FrameCapture {
    /// Events whose outermost pass is `root`.
    pub fn count_in(&self, root: &str) -> usize {
        self.events.iter().filter(|e| e.root() == root).count()
    }

    /// Draw-kind events (geometry and fullscreen draws).
    pub fn draw_calls(&self) -> usize {
        self.events.iter().filter(|e| e.is_draw()).count()
    }

    /// Outermost pass names in frame order.
    pub fn roots(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in &self.events {
            if out.last().map(String::as_str) != Some(e.root()) {
                out.push(e.root().to_owned());
            }
        }
        out
    }
}

/// Recording state while a capture is installed.
pub(crate) struct Recorder {
    request: CaptureRequest,
    pass: Vec<String>,
    target: TargetInfo,
    display: EventTarget,
    events: Vec<FrameEvent>,
    size: (u32, u32),
}

impl Recorder {
    pub fn new(request: CaptureRequest) -> Self {
        Self {
            request,
            pass: Vec::new(),
            target: TargetInfo::default(),
            display: EventTarget::None,
            events: Vec::new(),
            size: (0, 0),
        }
    }

    pub fn label(&self, draw: usize) -> Option<&DrawLabel> {
        self.request.labels.get(draw)
    }

    /// Add an event in the current pass; true = draw it (at or before the stop event).
    fn push(&mut self, mut ev: FrameEvent) -> bool {
        let index = self.events.len();
        if ev.pass.is_empty() {
            ev.pass = self.pass.clone();
        }
        if ev.target.name.is_empty() {
            ev.target = self.target.clone();
        }
        ev.display = self.display;
        self.events.push(ev);
        self.request.stop_after.is_none_or(|s| index <= s)
    }

    /// The stop event's image, when the frame stops before its last event.
    pub fn stop_display(&self) -> Option<EventTarget> {
        let stop = self.request.stop_after?;
        let ev = self.events.get(stop)?;
        (stop + 1 < self.events.len() || ev.display != EventTarget::Output).then_some(ev.display)
    }

    pub fn finish(self) -> FrameCapture {
        let stopped_at = self.request.stop_after.filter(|s| *s < self.events.len());
        FrameCapture {
            events: self.events,
            size: self.size,
            stopped_at,
        }
    }
}

/// The renderer's capture slot (`None` = not capturing: every hook is a no-op).
pub(crate) type Capture = Option<Mutex<Recorder>>;

fn lock(m: &Mutex<Recorder>) -> std::sync::MutexGuard<'_, Recorder> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Start a pass: later events belong to `path`, write `target` and show `display`.
pub(crate) fn pass(
    cap: &Capture,
    path: &[&str],
    target: impl FnOnce() -> TargetInfo,
    display: EventTarget,
) {
    if let Some(m) = cap {
        let mut r = lock(m);
        r.pass = path.iter().map(|s| (*s).to_owned()).collect();
        r.target = target();
        r.display = display;
    }
}

/// Record one event (built only while capturing); true = issue it.
pub(crate) fn record(cap: &Capture, make: impl FnOnce(&Recorder) -> FrameEvent) -> bool {
    match cap {
        None => true,
        Some(m) => {
            let mut r = lock(m);
            let ev = make(&r);
            r.push(ev)
        }
    }
}

/// A whole pass that is a single event: starts the pass and records it.
pub(crate) fn whole(
    cap: &Capture,
    path: &[&str],
    target: impl FnOnce() -> TargetInfo,
    display: EventTarget,
    make: impl FnOnce() -> FrameEvent,
) -> bool {
    if cap.is_none() {
        return true;
    }
    pass(cap, path, target, display);
    record(cap, |_| make())
}

pub(crate) fn set_size(cap: &Capture, size: (u32, u32)) {
    if let Some(m) = cap {
        lock(m).size = size;
    }
}

pub(crate) fn stop_display(cap: &Capture) -> Option<EventTarget> {
    cap.as_ref().and_then(|m| lock(m).stop_display())
}

/// Target description helper.
pub(crate) fn target(name: &str, (width, height): (u32, u32), format: &str) -> TargetInfo {
    TargetInfo {
        name: name.to_owned(),
        width,
        height,
        format: format.to_owned(),
    }
}
