//! Per-frame counters of the Physics, Audio and UI modules: set (or added to) from any thread
//! while recording, snapshotted and reset by `end_frame`.

use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

/// Which Profiler module a counter belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CounterGroup {
    Physics,
    Audio,
    Ui,
}

/// A value reported once per frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Counter {
    /// Fixed physics steps run this frame.
    PhysicsSteps,
    PhysicsBodies,
    /// Dynamic bodies awake.
    PhysicsActiveBodies,
    PhysicsSleepingBodies,
    PhysicsKinematicBodies,
    PhysicsStaticBodies,
    PhysicsColliders,
    PhysicsTriggers,
    /// Collider pairs touching (with at least one active contact).
    PhysicsContacts,
    PhysicsJoints,
    PhysicsCharacterControllers,
    /// Audio Sources playing (real voices, heard).
    AudioPlayingSources,
    /// Sources beyond the voice limit, silenced.
    AudioVirtualVoices,
    /// Voices in the mixer graph (including one-shots and fading tails).
    AudioGraphVoices,
    AudioMixerGroups,
    AudioEffects,
    /// Voices streamed from disk by a worker.
    AudioStreams,
    /// Audio-thread time spent mixing during this frame.
    AudioMixMs,
    /// Mixing time over the length of the audio it produced, in percent of one core.
    AudioDspLoad,
    /// Samples a streamed voice waited for its worker.
    AudioUnderruns,
    UiCanvases,
    /// Laid-out UI elements (every node of every canvas).
    UiElements,
    UiTexts,
    /// Draw batches of the canvases.
    UiBatches,
    UiVertices,
    /// Elements with a pointer handler (buttons, toggles, sliders…).
    UiInteractive,
}

impl Counter {
    pub const COUNT: usize = 26;
    pub const ALL: [Self; Self::COUNT] = [
        Self::PhysicsSteps,
        Self::PhysicsBodies,
        Self::PhysicsActiveBodies,
        Self::PhysicsSleepingBodies,
        Self::PhysicsKinematicBodies,
        Self::PhysicsStaticBodies,
        Self::PhysicsColliders,
        Self::PhysicsTriggers,
        Self::PhysicsContacts,
        Self::PhysicsJoints,
        Self::PhysicsCharacterControllers,
        Self::AudioPlayingSources,
        Self::AudioVirtualVoices,
        Self::AudioGraphVoices,
        Self::AudioMixerGroups,
        Self::AudioEffects,
        Self::AudioStreams,
        Self::AudioMixMs,
        Self::AudioDspLoad,
        Self::AudioUnderruns,
        Self::UiCanvases,
        Self::UiElements,
        Self::UiTexts,
        Self::UiBatches,
        Self::UiVertices,
        Self::UiInteractive,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn group(self) -> CounterGroup {
        match self.index() {
            i if i <= Self::PhysicsCharacterControllers.index() => CounterGroup::Physics,
            i if i <= Self::AudioUnderruns.index() => CounterGroup::Audio,
            _ => CounterGroup::Ui,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::PhysicsSteps => "Fixed steps",
            Self::PhysicsBodies => "Rigidbodies",
            Self::PhysicsActiveBodies => "Active dynamic bodies",
            Self::PhysicsSleepingBodies => "Sleeping bodies",
            Self::PhysicsKinematicBodies => "Kinematic bodies",
            Self::PhysicsStaticBodies => "Static bodies",
            Self::PhysicsColliders => "Colliders",
            Self::PhysicsTriggers => "Trigger colliders",
            Self::PhysicsContacts => "Contact pairs",
            Self::PhysicsJoints => "Joints",
            Self::PhysicsCharacterControllers => "Character Controllers",
            Self::AudioPlayingSources => "Playing Audio Sources",
            Self::AudioVirtualVoices => "Virtual voices (over the limit)",
            Self::AudioGraphVoices => "Mixer graph voices",
            Self::AudioMixerGroups => "Mixer groups",
            Self::AudioEffects => "Effects",
            Self::AudioStreams => "Streamed voices",
            Self::AudioMixMs => "Audio thread mixing (ms)",
            Self::AudioDspLoad => "DSP load (% of one core)",
            Self::AudioUnderruns => "Stream underruns",
            Self::UiCanvases => "Canvases",
            Self::UiElements => "Elements",
            Self::UiTexts => "Texts",
            Self::UiBatches => "Batches",
            Self::UiVertices => "Vertices",
            Self::UiInteractive => "Interactive elements",
        }
    }

    /// Counters of one module, in display order.
    pub fn of(group: CounterGroup) -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(move |c| c.group() == group)
    }
}

static VALUES: [AtomicU32; Counter::COUNT] = [const { AtomicU32::new(0) }; Counter::COUNT];

/// Report `value` for this frame (ignored while not recording).
#[inline]
pub fn set_counter(c: Counter, value: f32) {
    if super::enabled() {
        VALUES[c.index()].store(value.to_bits(), Ordering::Relaxed);
    }
}

/// Add `value` to this frame's counter (any thread; ignored while not recording).
pub fn add_counter(c: Counter, value: f32) {
    if super::enabled() {
        let _ = VALUES[c.index()].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            Some((f32::from_bits(v) + value).to_bits())
        });
    }
}

/// This frame's values (indexed by [`Counter::index`]), reset to zero.
pub(super) fn take() -> Vec<f32> {
    VALUES
        .iter()
        .map(|v| f32::from_bits(v.swap(0, Ordering::Relaxed)))
        .collect()
}
