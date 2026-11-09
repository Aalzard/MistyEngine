//! Cells streaming in and out: background reads, time-sliced integration on the main thread.
//!
//! Every frame [`Streamer::update`] works out which cells the sources want, starts a job on the
//! background pool for each cell that should load (read the file, parse it, read the terrain
//! data and models it names), and then spends at most the World Partition's budget on the main
//! thread: removing whole top-level objects of cells that unload, then creating whole top-level
//! objects of parsed cells, nearest cells first. A cell's proxy stays drawn until its last object
//! exists. The time each frame's main-thread work took is kept ([`StreamingStats`]), so a test
//! can prove no frame went over budget.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use glam::Vec3;
use hecs::Entity;

use super::{CellCoord, Source, WorldPartition};
use crate::scene::streaming::{ParsedScene, StreamedLoad};
use crate::scene::{Scene, SceneId};
use crate::terrain::TerrainData;

/// What the game does when streamed objects come and go (the Runtime starts their physics
/// bodies and scripts); the defaults just add and despawn.
pub trait StreamHooks {
    /// Complete top-level objects that were just added.
    fn added(&mut self, _scene: &mut Scene, _roots: &[Entity]) {}
    /// Remove these top-level objects (with their children).
    fn remove(&mut self, scene: &mut Scene, roots: &[Entity]) {
        for r in roots {
            scene.despawn(*r);
        }
    }
}

/// Hooks of a world without a running game (the editor's Edit mode, tests).
pub struct NoHooks;
impl StreamHooks for NoHooks {}

/// Where a cell is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellPhase {
    Unloaded,
    /// Being read and parsed on the background pool.
    Reading,
    /// Parsed, waiting for main-thread time.
    Waiting,
    /// Objects being added a slice at a time.
    Adding,
    Loaded,
    /// Objects being removed a slice at a time.
    Removing,
    Failed,
}

impl CellPhase {
    pub fn name(self) -> &'static str {
        match self {
            CellPhase::Unloaded => "Unloaded",
            CellPhase::Reading => "Reading",
            CellPhase::Waiting => "Waiting",
            CellPhase::Adding => "Adding",
            CellPhase::Loaded => "Loaded",
            CellPhase::Removing => "Removing",
            CellPhase::Failed => "Failed",
        }
    }

    /// On its way in (reading, waiting or being added).
    pub fn is_pending(self) -> bool {
        matches!(
            self,
            CellPhase::Reading | CellPhase::Waiting | CellPhase::Adding
        )
    }
}

/// A cell file read and parsed off the main thread, with the assets it names.
pub struct CellPayload {
    parsed: ParsedScene,
    /// Terrain data read ahead (project-relative path as the objects name it).
    terrains: Vec<(String, TerrainData)>,
    /// Models read ahead into the model cache (held so they stay cached).
    _models: Vec<Arc<crate::models::Model>>,
    /// Memory estimate of the loaded cell.
    bytes: u64,
}

enum Phase {
    Unloaded,
    Reading(u64),
    Waiting(Box<CellPayload>),
    Adding {
        load: StreamedLoad,
        terrains: Vec<String>,
        bytes: u64,
    },
    Loaded {
        id: SceneId,
        terrains: Vec<String>,
        bytes: u64,
    },
    Removing {
        id: SceneId,
        roots: VecDeque<Entity>,
        terrains: Vec<String>,
    },
    Failed(String),
}

impl Phase {
    fn kind(&self) -> CellPhase {
        match self {
            Phase::Unloaded => CellPhase::Unloaded,
            Phase::Reading(_) => CellPhase::Reading,
            Phase::Waiting(_) => CellPhase::Waiting,
            Phase::Adding { .. } => CellPhase::Adding,
            Phase::Loaded { .. } => CellPhase::Loaded,
            Phase::Removing { .. } => CellPhase::Removing,
            Phase::Failed(_) => CellPhase::Failed,
        }
    }
}

struct Cell {
    phase: Phase,
    /// Loaded by hand (World Partition window, `WorldPartition.load`): stays loaded.
    pinned: bool,
    /// Unload asked for by hand (done once, even if a source still wants the cell).
    unload: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            phase: Phase::Unloaded,
            pinned: false,
            unload: false,
        }
    }
}

struct JobDone {
    cell: CellCoord,
    ticket: u64,
    result: Result<CellPayload, String>,
}

/// Counters of the streamer (World Partition window, stats overlay, `WorldPartition` in Lua).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamingStats {
    /// Cells of the partition.
    pub cells: usize,
    pub loaded: usize,
    pub reading: usize,
    pub waiting: usize,
    pub adding: usize,
    pub removing: usize,
    pub failed: usize,
    /// Memory estimate of the loaded cells (objects and terrain data).
    pub memory_bytes: u64,
    /// Main-thread time of the last frame / the worst frame so far (ms).
    pub last_ms: f32,
    pub max_ms: f32,
    /// Frames whose main-thread streaming work went over the budget.
    pub over_budget: u64,
    pub frames: u64,
    pub budget_ms: f32,
    /// Cells loaded / unloaded since the streamer started.
    pub loads: u64,
    pub unloads: u64,
    /// The slowest single step so far (adding or removing one top-level object with its
    /// children, physics and scripts): (what, ms). A step is never split, so this is the floor
    /// of what one frame can take.
    pub slowest: Option<(String, f32)>,
}

impl StreamingStats {
    /// Cells on their way in.
    pub fn pending(&self) -> usize {
        self.reading + self.waiting + self.adding
    }
}

/// Frames of main-thread times kept for the stats graph.
const HISTORY: usize = 240;

pub struct Streamer {
    cells: BTreeMap<CellCoord, Cell>,
    tx: Sender<JobDone>,
    rx: Receiver<JobDone>,
    next_ticket: u64,
    /// Sources drive loading (Play mode, the player); off = only cells loaded by hand.
    pub auto: bool,
    /// Resolve cell files (and the asset paths inside them) against this folder instead of
    /// the project root (tests and benches that run beside tests moving the project root).
    root: Option<PathBuf>,
    stats: StreamingStats,
    /// Main-thread ms of the last frames.
    history: VecDeque<f32>,
    /// The sources of the last update (gizmos).
    sources: Vec<Source>,
    /// The partition object the state belongs to (a different one starts over).
    owner: Option<Entity>,
    /// Messages for the Console (failed cells).
    logs: Vec<String>,
    /// Time the hooks took per added object (ms, smoothed): kept free in the budget.
    hook_ms: f32,
    /// Time removing one top-level object took (ms, peak fading slowly).
    remove_ms: f32,
    /// Objects made but not started yet (physics bodies, scripts), oldest first.
    starting: VecDeque<(CellCoord, Entity)>,
}

impl Default for Streamer {
    fn default() -> Self {
        Self::new()
    }
}

impl Streamer {
    pub fn new() -> Self {
        // The background pool starts its threads now, at Play / load time, not inside the first
        // streaming frame (tens of milliseconds on a busy machine).
        let _ = crate::background::pool();
        let (tx, rx) = channel();
        Self {
            cells: BTreeMap::new(),
            tx,
            rx,
            next_ticket: 1,
            auto: true,
            root: None,
            stats: StreamingStats::default(),
            history: VecDeque::new(),
            sources: Vec::new(),
            owner: None,
            logs: Vec::new(),
            hook_ms: 0.2,
            remove_ms: 0.2,
            starting: VecDeque::new(),
        }
    }

    /// Cell files and the assets they name resolve against `root` (see [`Streamer::root`]).
    pub fn with_root(mut self, root: PathBuf) -> Self {
        self.root = Some(root);
        self
    }

    /// [`Streamer::with_root`] on an existing streamer (the Runtime's, in benches).
    pub fn set_root(&mut self, root: PathBuf) {
        self.root = Some(root);
    }

    pub fn stats(&self) -> &StreamingStats {
        &self.stats
    }

    /// Main-thread ms of the last frames (oldest first).
    pub fn history(&self) -> impl Iterator<Item = f32> + '_ {
        self.history.iter().copied()
    }

    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }

    pub fn phase(&self, c: CellCoord) -> CellPhase {
        self.cells
            .get(&c)
            .map_or(CellPhase::Unloaded, |cell| cell.phase.kind())
    }

    pub fn is_pinned(&self, c: CellCoord) -> bool {
        self.cells.get(&c).is_some_and(|cell| cell.pinned)
    }

    /// Why a cell failed to load.
    pub fn error(&self, c: CellCoord) -> Option<&str> {
        match &self.cells.get(&c)?.phase {
            Phase::Failed(e) => Some(e),
            _ => None,
        }
    }

    /// Objects made / in the file of a cell being added.
    pub fn progress(&self, c: CellCoord) -> Option<(usize, usize)> {
        match &self.cells.get(&c)?.phase {
            Phase::Adding { load, .. } => Some(load.progress()),
            _ => None,
        }
    }

    /// Load a cell and keep it loaded whatever the sources want (until [`Streamer::unload`]).
    pub fn load(&mut self, c: CellCoord) {
        let cell = self.cells.entry(c).or_default();
        cell.pinned = true;
        cell.unload = false;
        if matches!(cell.phase, Phase::Failed(_)) {
            cell.phase = Phase::Unloaded;
        }
    }

    /// Unload a cell now (sources may load it again later).
    pub fn unload(&mut self, c: CellCoord) {
        let cell = self.cells.entry(c).or_default();
        cell.pinned = false;
        cell.unload = true;
    }

    /// Load every cell of the partition by hand.
    pub fn load_all(&mut self, wp: &WorldPartition) {
        for c in &wp.cells {
            self.load(c.coord());
        }
    }

    /// Unload every loaded cell.
    pub fn unload_all(&mut self) {
        let all: Vec<CellCoord> = self.cells.keys().copied().collect();
        for c in all {
            self.unload(c);
        }
    }

    /// Every cell fully loaded or failed and nothing in flight.
    pub fn is_idle(&self) -> bool {
        self.cells.values().all(|c| {
            !matches!(
                c.phase,
                Phase::Reading(_)
                    | Phase::Waiting(_)
                    | Phase::Adding { .. }
                    | Phase::Removing { .. }
            )
        })
    }

    /// Forget every cell (a new scene was opened); objects already in the world stay.
    pub fn reset(&mut self) {
        self.cells.clear();
        self.starting.clear();
        self.owner = None;
        self.sources.clear();
    }

    /// One frame: pick cells for the sources (and `extra` ones, like the Scene view camera),
    /// start background loads, and add / remove objects within the budget.
    pub fn update(&mut self, scene: &mut Scene, extra: &[Source], hooks: &mut dyn StreamHooks) {
        crate::profile_scope!("WorldPartition.Update");
        let started = Instant::now();
        let Some((owner, wp)) = super::find(scene) else {
            if self.owner.is_some() {
                self.reset();
            }
            self.stats = StreamingStats::default();
            return;
        };
        if self.owner != Some(owner) {
            self.reset();
            self.owner = Some(owner);
        }
        let budget = Duration::from_secs_f32(wp.budget_ms.max(0.1) / 1000.0);
        let deadline = started + budget;

        let mut sources = if self.auto {
            super::sources(scene, &wp)
        } else {
            Vec::new()
        };
        sources.extend_from_slice(extra);
        self.reconcile(scene, &wp);
        self.collect_jobs();
        self.plan(scene, &wp, &sources);
        // Nearest cells first.
        let distance = |c: CellCoord| {
            sources
                .iter()
                .map(|s| wp.distance_to_cell(s.position, c))
                .fold(f32::MAX, f32::min)
        };
        self.dispatch(&wp, &distance);
        self.remove_some(scene, hooks, deadline);
        self.add_some(scene, &wp, hooks, deadline, &distance);

        let ms = started.elapsed().as_secs_f32() * 1000.0;
        self.sources = sources;
        self.count(ms, wp.budget_ms, wp.cells.len());
    }

    /// Follow scenes the user loaded, unloaded or removed by hand.
    fn reconcile(&mut self, scene: &Scene, wp: &WorldPartition) {
        for info in &wp.cells {
            let c = info.coord();
            let path = wp.cell_path(c);
            let open = scene
                .find_by_path(&path)
                .filter(|id| scene.entry(*id).is_some_and(|e| e.loaded));
            let cell = self.cells.entry(c).or_default();
            match (&cell.phase, open) {
                (Phase::Loaded { id, .. }, now) if now != Some(*id) => cell.phase = Phase::Unloaded,
                (Phase::Unloaded | Phase::Failed(_), Some(id))
                    if !scene.entry(id).is_some_and(|e| e.partial) =>
                {
                    cell.phase = Phase::Loaded {
                        id,
                        terrains: Vec::new(),
                        bytes: 0,
                    };
                }
                _ => {}
            }
        }
    }

    fn collect_jobs(&mut self) {
        while let Ok(done) = self.rx.try_recv() {
            let Some(cell) = self.cells.get_mut(&done.cell) else {
                continue;
            };
            // A cell that was cancelled while reading drops the result.
            if !matches!(cell.phase, Phase::Reading(t) if t == done.ticket) {
                continue;
            }
            cell.phase = match done.result {
                Ok(payload) => Phase::Waiting(Box::new(payload)),
                Err(e) => {
                    self.logs.push(format!(
                        "World Partition: {} failed to load: {e}",
                        super::cell_name(done.cell)
                    ));
                    Phase::Failed(e)
                }
            };
        }
    }

    /// Cells to load and to unload this frame.
    fn plan(&mut self, scene: &mut Scene, wp: &WorldPartition, sources: &[Source]) {
        for info in &wp.cells {
            let c = info.coord();
            let want = super::wanted(wp, sources, c);
            let cell = self.cells.entry(c).or_default();
            // Sources drive cells in Play mode, or when given (the Scene camera in Edit mode).
            let driven = !sources.is_empty();
            let unload = cell.unload || (!cell.pinned && driven && want.is_none());
            let load = !cell.unload && (cell.pinned || (driven && want == Some(true)));
            if cell.unload && matches!(cell.phase, Phase::Unloaded | Phase::Failed(_)) {
                cell.unload = false;
            }
            if load && matches!(cell.phase, Phase::Unloaded) {
                // Marked for dispatch (a ticket is handed out in `dispatch`).
                cell.phase = Phase::Reading(0);
            } else if unload {
                cell.unload = false;
                let phase = std::mem::replace(&mut cell.phase, Phase::Unloaded);
                cell.phase = match phase {
                    Phase::Loaded { id, terrains, .. } if scene.active_scene() != id => {
                        Phase::Removing {
                            id,
                            roots: scene.roots_in(id).into(),
                            terrains,
                        }
                    }
                    Phase::Adding { load, terrains, .. } => {
                        let id = load.id;
                        scene.abandon_streamed(load);
                        self.starting.retain(|(cc, _)| *cc != c);
                        Phase::Removing {
                            id,
                            roots: scene.roots_in(id).into(),
                            terrains,
                        }
                    }
                    // Being read or waiting: dropped (a late job result is ignored).
                    Phase::Reading(_) | Phase::Waiting(_) => Phase::Unloaded,
                    other => other,
                };
            }
        }
    }

    /// Start background loads for marked cells, nearest first, up to the partition's limit.
    fn dispatch(&mut self, wp: &WorldPartition, distance: &dyn Fn(CellCoord) -> f32) {
        let running = self
            .cells
            .values()
            .filter(|c| matches!(c.phase, Phase::Reading(t) if t != 0))
            .count();
        let mut marked: Vec<CellCoord> = self
            .cells
            .iter()
            .filter(|(_, c)| matches!(c.phase, Phase::Reading(0)))
            .map(|(k, _)| *k)
            .collect();
        marked.sort_by(|a, b| distance(*a).total_cmp(&distance(*b)));
        let free = (wp.max_loads.max(1) as usize).saturating_sub(running);
        for (i, c) in marked.into_iter().enumerate() {
            let cell = self.cells.get_mut(&c).expect("listed");
            if i >= free {
                // Waits for a free slot (marked again next frame).
                cell.phase = Phase::Unloaded;
                continue;
            }
            let ticket = self.next_ticket;
            self.next_ticket += 1;
            cell.phase = Phase::Reading(ticket);
            let rel = wp.cell_path(c);
            let file = match &self.root {
                Some(root) => root.join(&rel),
                None => crate::assets::absolute(&rel),
            };
            let root = self.root.clone();
            let tx = self.tx.clone();
            crate::background::pool().spawn(move || {
                let result = read_cell(&file, root.as_deref());
                let _ = tx.send(JobDone {
                    cell: c,
                    ticket,
                    result,
                });
            });
        }
    }

    /// Remove objects of unloading cells until the deadline.
    fn remove_some(&mut self, scene: &mut Scene, hooks: &mut dyn StreamHooks, deadline: Instant) {
        // Like starting: one object at a time against what one costs (peak, fading slowly);
        // at least one a frame while there is time, so unloading always progresses.
        let mut removed = 0;
        for (c, cell) in self.cells.iter_mut() {
            let Phase::Removing {
                id,
                roots,
                terrains,
            } = &mut cell.phase
            else {
                continue;
            };
            while let Some(r) = roots.front().copied() {
                let each = Duration::from_secs_f32(self.remove_ms.max(0.0) / 1000.0);
                let now = Instant::now();
                if now >= deadline || (removed > 0 && now + each >= deadline) {
                    return;
                }
                roots.pop_front();
                if scene.contains(r) {
                    let t = Instant::now();
                    let name = scene.name(r);
                    hooks.remove(scene, &[r]);
                    let ms = t.elapsed().as_secs_f32() * 1000.0;
                    self.remove_ms = if ms > self.remove_ms {
                        ms
                    } else {
                        self.remove_ms * 0.97 + ms * 0.03
                    };
                    removed += 1;
                    note_step(&mut self.stats, || format!("removing {name}"), t);
                }
            }
            scene.remove_streamed(*id);
            for t in terrains.drain(..) {
                crate::terrain::store::unload_if_saved(&t);
            }
            let _ = c;
            cell.phase = Phase::Unloaded;
            self.stats.unloads += 1;
        }
    }

    /// Add objects of parsed cells (nearest first) until the deadline.
    fn add_some(
        &mut self,
        scene: &mut Scene,
        wp: &WorldPartition,
        hooks: &mut dyn StreamHooks,
        deadline: Instant,
        distance: &dyn Fn(CellCoord) -> f32,
    ) {
        // Objects made in earlier frames start first (physics bodies, scripts), then new ones
        // are made with what is left, then as many of those start as fit.
        self.start_some(scene, hooks, deadline, true);
        let mut order: Vec<CellCoord> = self
            .cells
            .iter()
            .filter(|(_, c)| matches!(c.phase, Phase::Waiting(_) | Phase::Adding { .. }))
            .map(|(k, _)| *k)
            .collect();
        // Cells already being added finish first, then the nearest.
        order.sort_by(|a, b| {
            let adding = |c: &CellCoord| !matches!(self.cells[c].phase, Phase::Adding { .. });
            adding(a)
                .cmp(&adding(b))
                .then(distance(*a).total_cmp(&distance(*b)))
        });
        // Starting an object costs about this much: making one is only worth it with room left
        // to start it this frame too.
        let per_root = Duration::from_secs_f32(self.hook_ms.max(0.0) / 1000.0);
        let mut made_any = false;
        'cells: for c in order {
            if Instant::now() + per_root >= deadline && (made_any || Instant::now() >= deadline) {
                break;
            }
            let cell = self.cells.get_mut(&c).expect("listed");
            if let Phase::Waiting(_) = cell.phase {
                let Phase::Waiting(payload) = std::mem::replace(&mut cell.phase, Phase::Unloaded)
                else {
                    unreachable!()
                };
                let CellPayload {
                    parsed,
                    terrains,
                    bytes,
                    ..
                } = *payload;
                let mut names = Vec::new();
                for (path, data) in terrains {
                    crate::terrain::store::insert_loaded(&path, data);
                    names.push(path);
                }
                match scene.begin_streamed(&wp.cell_path(c), parsed) {
                    Ok(load) => {
                        cell.phase = Phase::Adding {
                            load,
                            terrains: names,
                            bytes,
                        };
                    }
                    Err(e) => {
                        self.logs
                            .push(format!("World Partition: {}: {e}", super::cell_name(c)));
                        cell.phase = Phase::Failed(e);
                        continue;
                    }
                }
            }
            let Phase::Adding { load, .. } = &mut cell.phase else {
                continue;
            };
            // One top-level subtree per step.
            while !load.is_done() {
                let t = Instant::now();
                let roots = scene.continue_streamed(load, Instant::now());
                if let Some(r) = roots.first() {
                    let ms = t.elapsed().as_secs_f32() * 1000.0;
                    if self.stats.slowest.as_ref().is_none_or(|(_, w)| ms > *w) {
                        self.stats.slowest = Some((format!("adding {}", scene.name(*r)), ms));
                    }
                }
                made_any = true;
                self.starting.extend(roots.into_iter().map(|r| (c, r)));
                if Instant::now() + per_root >= deadline {
                    if !load.is_done() {
                        break 'cells;
                    }
                    break;
                }
            }
        }
        self.start_some(scene, hooks, deadline, false);
        self.finish_cells(scene);
    }

    /// Start objects waiting for their physics and scripts, as many as fit before `deadline`
    /// (at least one when `first` and time is left, so starting always progresses).
    fn start_some(
        &mut self,
        scene: &mut Scene,
        hooks: &mut dyn StreamHooks,
        deadline: Instant,
        first: bool,
    ) {
        if self.starting.is_empty() {
            return;
        }
        // One object at a time, the clock checked against what one costs (its peak, fading
        // slowly), so a frame goes past the deadline by one misjudged object at most.
        let track = |old: f32, new: f32| {
            if new > old {
                new
            } else {
                old * 0.97 + new * 0.03
            }
        };
        let mut started = 0;
        while let Some(&(_, e)) = self.starting.front() {
            let each = Duration::from_secs_f32(self.hook_ms.max(0.0) / 1000.0);
            let now = Instant::now();
            let may = if started == 0 && first {
                now < deadline
            } else {
                now + each < deadline
            };
            if !may {
                break;
            }
            self.starting.pop_front();
            if !scene.contains(e) {
                continue;
            }
            let t = Instant::now();
            hooks.added(scene, &[e]);
            let ms = t.elapsed().as_secs_f32() * 1000.0;
            self.hook_ms = track(self.hook_ms, ms);
            started += 1;
            if self.stats.slowest.as_ref().is_none_or(|(_, w)| ms > *w) {
                self.stats.slowest = Some((format!("starting {}", scene.name(e)), ms));
            }
        }
    }

    /// Cells whose objects are all made and started become Loaded (their proxy hides).
    fn finish_cells(&mut self, scene: &mut Scene) {
        let waiting: BTreeSet<CellCoord> = self.starting.iter().map(|(c, _)| *c).collect();
        for (c, cell) in self.cells.iter_mut() {
            let done = matches!(&cell.phase, Phase::Adding { load, .. } if load.is_done());
            if !done || waiting.contains(c) {
                continue;
            }
            let Phase::Adding {
                load,
                terrains,
                bytes,
            } = std::mem::replace(&mut cell.phase, Phase::Unloaded)
            else {
                unreachable!()
            };
            scene.finish_streamed(load.id);
            for w in &load.report.warnings {
                self.logs.push(format!("{}: {w}", super::cell_name(*c)));
            }
            cell.phase = Phase::Loaded {
                id: load.id,
                terrains,
                bytes,
            };
            self.stats.loads += 1;
        }
    }

    fn count(&mut self, ms: f32, budget_ms: f32, cells: usize) {
        let s = &mut self.stats;
        s.cells = cells;
        s.budget_ms = budget_ms;
        (
            s.loaded, s.reading, s.waiting, s.adding, s.removing, s.failed,
        ) = (0, 0, 0, 0, 0, 0);
        s.memory_bytes = 0;
        for c in self.cells.values() {
            match &c.phase {
                Phase::Loaded { bytes, .. } => {
                    s.loaded += 1;
                    s.memory_bytes += bytes;
                }
                Phase::Reading(_) => s.reading += 1,
                Phase::Waiting(p) => {
                    s.waiting += 1;
                    s.memory_bytes += p.bytes;
                }
                Phase::Adding { bytes, .. } => {
                    s.adding += 1;
                    s.memory_bytes += bytes;
                }
                Phase::Removing { .. } => s.removing += 1,
                Phase::Failed(_) => s.failed += 1,
                Phase::Unloaded => {}
            }
        }
        s.last_ms = ms;
        s.max_ms = s.max_ms.max(ms);
        s.frames += 1;
        if ms > budget_ms {
            s.over_budget += 1;
        }
        self.history.push_back(ms);
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
    }

    /// Cells currently loaded (all objects in).
    pub fn loaded_cells(&self) -> Vec<CellCoord> {
        self.cells
            .iter()
            .filter(|(_, c)| matches!(c.phase, Phase::Loaded { .. }))
            .map(|(k, _)| *k)
            .collect()
    }

    /// Run updates (with `sources`) until nothing is in flight or `timeout` passes; returns
    /// whether it settled (tests, loading screens).
    pub fn settle(
        &mut self,
        scene: &mut Scene,
        extra: &[Source],
        hooks: &mut dyn StreamHooks,
        timeout: Duration,
    ) -> bool {
        let start = Instant::now();
        loop {
            self.update(scene, extra, hooks);
            if self.is_idle() && self.wanted_all_loaded(scene, extra) {
                return true;
            }
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Every cell the sources want loaded is loaded (or failed).
    fn wanted_all_loaded(&self, scene: &Scene, extra: &[Source]) -> bool {
        let Some((_, wp)) = super::find(scene) else {
            return true;
        };
        let mut sources = if self.auto {
            super::sources(scene, &wp)
        } else {
            Vec::new()
        };
        sources.extend_from_slice(extra);
        wp.cells.iter().all(|info| {
            let c = info.coord();
            let cell = self.cells.get(&c);
            let wanted = cell.is_some_and(|c| c.pinned)
                || (self.auto || !extra.is_empty())
                    && super::wanted(&wp, &sources, c) == Some(true);
            !wanted || matches!(self.phase(c), CellPhase::Loaded | CellPhase::Failed)
        })
    }

    /// The cells near `p` within `radius` (x/z) that are not loaded yet.
    pub fn missing_near(&self, wp: &WorldPartition, p: Vec3, radius: f32) -> Vec<CellCoord> {
        wp.cells
            .iter()
            .map(|c| c.coord())
            .filter(|c| wp.distance_to_cell(p, *c) <= radius)
            .filter(|c| self.phase(*c) != CellPhase::Loaded)
            .collect()
    }
}

/// Read, parse and read ahead the assets of one cell file (on the background pool).
fn read_cell(
    file: &std::path::Path,
    root: Option<&std::path::Path>,
) -> Result<CellPayload, String> {
    crate::profile_scope!("WorldPartition.ReadCell");
    let text = crate::vfs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut parsed = crate::scene::streaming::parse(&text)?;
    if let Some(root) = root {
        parsed.rebase(root);
    }
    let mut bytes = text.len() as u64 * 3;
    // Static Mesh Collider shapes (their BVH takes milliseconds) are built here too.
    let colliders = parsed.mesh_colliders();
    let mut terrains = Vec::new();
    let mut models = Vec::new();
    let mut seen = BTreeSet::new();
    for rel in parsed.asset_refs() {
        if !seen.insert(rel.clone()) {
            continue;
        }
        let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "terrain" => {
                if crate::terrain::store::is_loaded(&rel) {
                    continue;
                }
                let data = TerrainData::load(&crate::assets::absolute(&rel))?;
                bytes += terrain_bytes(&data);
                // The trees it shows are built here too, not on the render thread.
                for p in &data.trees.prototypes {
                    let _ = crate::trees::store::get(&p.tree);
                }
                terrains.push((rel, data));
            }
            "tree" => {
                let _ = crate::trees::store::get(&rel);
            }
            // Its textures are decoded here, not on the render thread at the first draw.
            "mat" => crate::render::texture::prefetch_material(&rel),
            e if crate::models::EXTENSIONS.contains(&e) => {
                if let Ok(m) = crate::models::model(&rel) {
                    models.push(m);
                }
            }
            _ => {}
        }
    }
    for (mesh, scale) in colliders {
        crate::physics::shape_cache::prebuild(&mesh, scale);
    }
    Ok(CellPayload {
        parsed,
        terrains,
        _models: models,
        bytes,
    })
}

/// Memory of a terrain's arrays.
pub fn terrain_bytes(d: &TerrainData) -> u64 {
    (d.heights.len() * 4
        + (d.alphamap.len() + d.alphamap_extra.len()) * 4
        + d.detail_map.len()
        + d.holes.len()
        + d.trees.instances.len() * std::mem::size_of::<crate::terrain::trees::TreeInstance>())
        as u64
}

/// Keep the slowest step (`what` only made when it is the slowest).
fn note_step(stats: &mut StreamingStats, what: impl FnOnce() -> String, started: Instant) {
    let ms = started.elapsed().as_secs_f32() * 1000.0;
    if stats.slowest.as_ref().is_none_or(|(_, worst)| ms > *worst) {
        stats.slowest = Some((what(), ms));
    }
}
