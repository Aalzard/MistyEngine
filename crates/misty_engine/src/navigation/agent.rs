//! Navigation while playing: the scene's nav meshes (one per baked agent type and surface),
//! Nav Mesh Agents, off-mesh links and carving obstacles.
//!
//! Every frame: finished tile rebuilds are swapped in (agents on changed tiles are placed again
//! and re-plan), NavMesh Links are snapped onto the meshes, carving obstacles that moved or
//! settled mark their tiles for a rebuild on the background pool, then each agent steers to its
//! next corner (accelerating, braking before the end), avoids the others with ORCA, crosses
//! off-mesh links (on its own or until `CompleteOffMeshLink`), moves along the mesh surface and
//! writes its Transform (base offset above the surface, turning towards where it moves).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::heightfield::CarveShape;
use super::navmesh::{NearestHit, OffMeshLink, PolyRef, Tile, tile_of};
use super::orca;
use super::path::LinkStep;
use super::query::{EdgeHit, QueryFilter, RaycastHit};
use super::tiles::TileBuilder;
use super::{
    AREA_COUNT, AREA_JUMP, NavMesh, NavMeshAgent, NavMeshLink, NavMeshObstacle, NavMeshSurface,
    NavPath, NavigationSettings, ObstacleShape, PathStatus, geometry,
};
use crate::scene::Scene;

/// Avoidance quality: how many neighbours an agent considers and how far ahead it looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ObstacleAvoidance {
    None,
    LowQuality,
    MediumQuality,
    GoodQuality,
    #[default]
    HighQuality,
}

impl ObstacleAvoidance {
    pub const ALL: [ObstacleAvoidance; 5] = [
        Self::None,
        Self::LowQuality,
        Self::MediumQuality,
        Self::GoodQuality,
        Self::HighQuality,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::LowQuality => "Low Quality",
            Self::MediumQuality => "Medium Quality",
            Self::GoodQuality => "Good Quality",
            Self::HighQuality => "High Quality",
        }
    }

    /// (neighbours, seconds ahead), None = no avoidance.
    pub fn params(self) -> Option<(usize, f32)> {
        match self {
            Self::None => None,
            Self::LowQuality => Some((4, 1.0)),
            Self::MediumQuality => Some((6, 2.0)),
            Self::GoodQuality => Some((8, 3.0)),
            Self::HighQuality => Some((10, 4.0)),
        }
    }
}

/// Corners closer than this are treated as reached.
const CORNER_REACHED: f32 = 0.1;

/// Something that happened to an agent this frame (scripts get callbacks).
#[derive(Clone, Debug, PartialEq)]
pub struct NavEvent {
    pub entity: Entity,
    pub kind: NavEventKind,
    pub start: Vec3,
    pub end: Vec3,
    pub area: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavEventKind {
    /// Reached an off-mesh link and starts crossing (`OnOffMeshLinkStart`).
    LinkStart,
    /// Arrived at the link's other end (`OnOffMeshLinkEnd`).
    LinkEnd,
}

/// An agent crossing an off-mesh link.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkTraversal {
    pub start: Vec3,
    pub end: Vec3,
    pub area: u8,
    pub end_poly: PolyRef,
    /// 0..1 along the link.
    pub t: f32,
    /// Moving on its own (else waiting for `CompleteOffMeshLink`).
    pub auto: bool,
}

/// Live state of one agent.
#[derive(Clone, Debug, Default)]
pub struct AgentState {
    /// Position on the nav mesh surface (without the base offset).
    pub position: Vec3,
    poly: PolyRef,
    /// Index of the runtime mesh it walks on.
    mesh: usize,
    pub on_mesh: bool,
    /// Requested destination, snapped onto the mesh.
    pub destination: Vec3,
    /// Destination as asked (for re-planning).
    target: Option<Vec3>,
    /// Remaining corners (the first is the next one to walk to).
    pub corners: Vec<Vec3>,
    /// Off-mesh link starting at each corner.
    pub corner_links: Vec<Option<LinkStep>>,
    path_polys: Vec<PolyRef>,
    pub status: PathStatus,
    pub velocity: Vec3,
    /// Velocity the agent wants before avoidance.
    pub desired_velocity: Vec3,
    pub is_stopped: bool,
    /// A path is waiting for a nav mesh rebuild.
    pub path_pending: bool,
    pub link: Option<LinkTraversal>,
    /// Per-agent area costs (`SetAreaCost`).
    pub area_costs: Option<[f32; AREA_COUNT]>,
    /// Transform position written last frame (a script moving it means a warp).
    written: Vec3,
}

impl AgentState {
    pub fn has_path(&self) -> bool {
        !self.corners.is_empty()
    }

    /// Distance left along the path (0 without a path).
    pub fn remaining_distance(&self) -> f32 {
        let Some(first) = self.corners.first() else {
            return 0.0;
        };
        self.position.distance(*first)
            + self
                .corners
                .windows(2)
                .map(|w| w[0].distance(w[1]))
                .sum::<f32>()
    }

    pub fn is_on_off_mesh_link(&self) -> bool {
        self.link.is_some()
    }
}

/// Where a runtime mesh came from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    /// The Navigation window's bake.
    Scene,
    /// A NavMesh Surface.
    Surface(Entity),
    /// Given directly (tests); rebuilt like the Navigation Static bake.
    Given,
}

struct Job {
    result: Arc<Mutex<Option<Vec<Tile>>>>,
}

/// One nav mesh while playing (tiles can be rebuilt).
struct RuntimeMesh {
    mesh: NavMesh,
    agent_type: u32,
    source: Source,
    builder: Option<Arc<TileBuilder>>,
    job: Option<Job>,
    dirty: HashSet<(i32, i32)>,
}

/// A carving obstacle's state.
#[derive(Clone, Debug, Default)]
struct ObstacleState {
    last: Vec3,
    still: f32,
    carved: Option<CarveShape>,
}

/// Counters for the editor and tests.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeStats {
    pub tiles_rebuilt: usize,
    pub rebuilds: usize,
}

/// All agents of a running game plus the scene's nav meshes.
pub struct NavRuntime {
    meshes: Vec<RuntimeMesh>,
    agents: HashMap<Entity, AgentState>,
    logs: Vec<String>,
    events: Vec<NavEvent>,
    settings: NavigationSettings,
    costs: [f32; AREA_COUNT],
    obstacles: HashMap<Entity, ObstacleState>,
    links: Vec<OffMeshLink>,
    /// `stats.rebuilds` when the links were last snapped.
    link_epoch: usize,
    /// Build changed tiles on the calling thread (tests).
    pub sync_rebuilds: bool,
    pub stats: RuntimeStats,
}

impl Default for NavRuntime {
    fn default() -> Self {
        Self::with_settings(NavigationSettings::default())
    }
}

fn agent_of(scene: &Scene, e: Entity) -> Option<NavMeshAgent> {
    if !scene.contains(e) || !scene.active_in_hierarchy(e) {
        return None;
    }
    let a = scene.world.get::<&NavMeshAgent>(e).ok()?;
    a.enabled.then(|| (*a).clone())
}

/// Move `v` towards `target` by at most `step`.
fn move_towards(v: Vec3, target: Vec3, step: f32) -> Vec3 {
    let d = target - v;
    let len = d.length();
    if len <= step || len < 1e-6 {
        target
    } else {
        v + d / len * step
    }
}

/// Yaw (degrees) whose forward is `dir` (x/z).
fn yaw_of(dir: Vec2) -> f32 {
    dir.x.atan2(dir.y).to_degrees()
}

impl NavRuntime {
    fn with_settings(settings: NavigationSettings) -> Self {
        Self {
            meshes: Vec::new(),
            agents: HashMap::new(),
            logs: Vec::new(),
            events: Vec::new(),
            costs: settings.area_costs(),
            settings,
            obstacles: HashMap::new(),
            links: Vec::new(),
            link_epoch: 0,
            sync_rebuilds: false,
            stats: RuntimeStats::default(),
        }
    }

    /// A runtime over one mesh (agent type 0).
    pub fn new(mesh: Option<Arc<NavMesh>>) -> Self {
        let mut rt = Self::with_settings(NavigationSettings::default());
        if let Some(m) = mesh {
            rt.push_mesh((*m).clone(), Source::Given);
        }
        rt
    }

    fn push_mesh(&mut self, mesh: NavMesh, source: Source) {
        let agent_type = mesh.settings.agent_type;
        self.meshes.push(RuntimeMesh {
            mesh,
            agent_type,
            source,
            builder: None,
            job: None,
            dirty: HashSet::new(),
        });
    }

    /// The scene's baked nav meshes: the Navigation window's (`scene.navigation.nav_mesh`) and
    /// every enabled NavMesh Surface's (paths relative to `root`).
    pub fn load(scene: &Scene, root: &Path) -> Self {
        let mut rt = Self::with_settings(scene.navigation.clone());
        if let Some(rel) = scene.navigation.nav_mesh.as_ref() {
            match super::store::load(&root.join(rel)) {
                Ok(m) => rt.push_mesh((*m).clone(), Source::Scene),
                Err(e) => rt.logs.push(format!("Navigation: {e}")),
            }
        }
        for (e, _) in scene.walk() {
            let Ok(s) = scene.world.get::<&NavMeshSurface>(e) else {
                continue;
            };
            if !s.enabled || !scene.active_in_hierarchy(e) {
                continue;
            }
            let Some(rel) = s.nav_mesh_data.as_ref() else {
                continue;
            };
            match super::store::load(&root.join(rel)) {
                Ok(m) => {
                    let mut m = (*m).clone();
                    m.settings.agent_type = s.agent_type;
                    rt.push_mesh(m, Source::Surface(e));
                }
                Err(err) => rt.logs.push(format!("Navigation: {err}")),
            }
        }
        // Links are there from the start (queries in Start see them).
        let links: Vec<(Entity, NavMeshLink)> = scene
            .walk()
            .into_iter()
            .filter(|(e, _)| scene.active_in_hierarchy(*e))
            .filter_map(|(e, _)| {
                let l = scene.world.get::<&NavMeshLink>(e).ok()?;
                l.enabled.then(|| (e, (*l).clone()))
            })
            .collect();
        rt.update_links(scene, &links);
        rt
    }

    /// The first nav mesh (Humanoid), if any.
    pub fn mesh(&self) -> Option<&NavMesh> {
        self.meshes
            .iter()
            .find(|m| m.agent_type == 0)
            .or(self.meshes.first())
            .map(|m| &m.mesh)
    }

    /// Every live nav mesh (with carved tiles) and its agent type.
    pub fn meshes(&self) -> impl Iterator<Item = (&NavMesh, u32)> + '_ {
        self.meshes.iter().map(|m| (&m.mesh, m.agent_type))
    }

    /// Whether a tile rebuild is running or waiting.
    pub fn rebuilding(&self) -> bool {
        self.meshes
            .iter()
            .any(|m| m.job.is_some() || !m.dirty.is_empty())
    }

    /// Console messages since the last call.
    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }

    /// Agent events since the last call (for script callbacks).
    pub fn take_events(&mut self) -> Vec<NavEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn settings(&self) -> &NavigationSettings {
        &self.settings
    }

    // ---- areas and queries ----------------------------------------------------------------

    pub fn area_cost(&self, area: usize) -> f32 {
        self.costs.get(area).copied().unwrap_or(1.0)
    }

    /// `NavMesh.SetAreaCost` for the rest of the game.
    pub fn set_area_cost(&mut self, area: usize, cost: f32) {
        if let Some(c) = self.costs.get_mut(area) {
            *c = cost.max(1.0);
        }
    }

    pub fn filter(&self, area_mask: u32) -> QueryFilter {
        QueryFilter::new(area_mask, self.costs)
    }

    /// Nearest point on the meshes of `agent_type` (index of the mesh, hit).
    fn nearest_on(
        &self,
        agent_type: u32,
        p: Vec3,
        max: f32,
        filter: &QueryFilter,
    ) -> Option<(usize, NearestHit)> {
        self.meshes
            .iter()
            .enumerate()
            .filter(|(_, m)| m.agent_type == agent_type)
            .filter_map(|(i, m)| m.mesh.nearest_filtered(p, max, filter).map(|h| (i, h)))
            .min_by(|a, b| a.1.distance.total_cmp(&b.1.distance))
    }

    /// `NavMesh.SamplePosition`.
    pub fn sample_position(
        &self,
        p: Vec3,
        max: f32,
        mask: u32,
        agent_type: u32,
    ) -> Option<NearestHit> {
        self.nearest_on(agent_type, p, max, &self.filter(mask))
            .map(|(_, h)| h)
    }

    /// A path between two points (`NavMesh.CalculatePath`).
    pub fn calculate_path(&self, a: Vec3, b: Vec3) -> NavPath {
        self.calculate_path_filtered(a, b, super::ALL_AREAS, 0)
    }

    pub fn calculate_path_filtered(&self, a: Vec3, b: Vec3, mask: u32, agent_type: u32) -> NavPath {
        let f = self.filter(mask);
        let reach = self.reach(agent_type);
        match self.nearest_on(agent_type, a, reach, &f) {
            Some((i, _)) => self.meshes[i].mesh.find_path_filtered(a, b, &f),
            None => NavPath::invalid(),
        }
    }

    /// `NavMesh.Raycast`.
    pub fn raycast(&self, a: Vec3, b: Vec3, mask: u32, agent_type: u32) -> Option<RaycastHit> {
        let f = self.filter(mask);
        let (i, _) = self.nearest_on(agent_type, a, self.reach(agent_type), &f)?;
        self.meshes[i].mesh.raycast(a, b, &f)
    }

    /// `NavMesh.FindClosestEdge`.
    pub fn closest_edge(&self, p: Vec3, mask: u32, agent_type: u32) -> Option<EdgeHit> {
        let f = self.filter(mask);
        let (i, _) = self.nearest_on(agent_type, p, self.reach(agent_type), &f)?;
        self.meshes[i].mesh.closest_edge(p, &f)
    }

    fn reach(&self, agent_type: u32) -> f32 {
        self.meshes
            .iter()
            .find(|m| m.agent_type == agent_type)
            .map_or(2.0, |m| m.mesh.agent.height.max(m.mesh.cell_size * 6.0))
    }

    // ---- agents ---------------------------------------------------------------------------

    fn agent_filter(&self, comp: &NavMeshAgent, st: &AgentState) -> QueryFilter {
        QueryFilter::new(comp.area_mask, st.area_costs.unwrap_or(self.costs))
    }

    /// State of `e`'s agent, placing it on the mesh the first time.
    pub fn agent(&mut self, scene: &Scene, e: Entity) -> Option<&mut AgentState> {
        let comp = agent_of(scene, e)?;
        if !self.agents.contains_key(&e) {
            let mut st = AgentState::default();
            let at = scene.world_position(e) - Vec3::Y * comp.base_offset;
            if !self.place(&mut st, &comp, at) {
                self.logs.push(format!(
                    "'{}': failed to create agent because it is not close enough to the NavMesh",
                    scene.name(e)
                ));
            }
            st.written = scene.world_position(e);
            self.agents.insert(e, st);
        }
        self.agents.get_mut(&e)
    }

    /// Read-only state (None before the agent's first frame).
    pub fn state(&self, e: Entity) -> Option<&AgentState> {
        self.agents.get(&e)
    }

    /// Snap `st` onto the nearest mesh of the agent's type near `at`.
    fn place(&self, st: &mut AgentState, comp: &NavMeshAgent, at: Vec3) -> bool {
        let reach = self.reach(comp.agent_type);
        let f = self.agent_filter(comp, st);
        let hit = self.nearest_on(comp.agent_type, at, reach, &f);
        st.on_mesh = hit.is_some();
        if let Some((m, h)) = hit {
            st.position = h.position;
            st.poly = h.poly;
            st.mesh = m;
        }
        st.corners.clear();
        st.corner_links.clear();
        st.path_polys.clear();
        st.link = None;
        st.velocity = Vec3::ZERO;
        st.on_mesh
    }

    /// Plan `st`'s path to its target now.
    fn plan(&self, st: &mut AgentState, comp: &NavMeshAgent) -> bool {
        let Some(target) = st.target else {
            return false;
        };
        let Some(m) = self.meshes.get(st.mesh) else {
            return false;
        };
        st.path_pending = false;
        let f = self.agent_filter(comp, st);
        let path = m.mesh.find_path_filtered(st.position, target, &f);
        st.status = path.status;
        if path.status == PathStatus::Invalid {
            st.corners.clear();
            st.corner_links.clear();
            st.path_polys.clear();
            return false;
        }
        st.destination = path.corners.last().copied().unwrap_or(st.position);
        // The start corner is where the agent stands, unless a link starts right there.
        let skip = usize::from(path.links.first().is_none_or(|l| l.is_none()));
        st.corners = path.corners.into_iter().skip(skip).collect();
        st.corner_links = path.links.into_iter().skip(skip).collect();
        st.path_polys = path.polys;
        true
    }

    /// `SetDestination`: plan a path (waits while its tiles rebuild); false when the agent
    /// isn't on the mesh.
    pub fn set_destination(&mut self, scene: &Scene, e: Entity, target: Vec3) -> bool {
        if self.meshes.is_empty() {
            self.logs.push(format!(
                "'{}': SetDestination needs a baked NavMesh (Window ▸ AI ▸ Navigation)",
                scene.name(e)
            ));
            return false;
        }
        let Some(comp) = agent_of(scene, e) else {
            return false;
        };
        let Some(mut st) = self.agent(scene, e).cloned() else {
            return false;
        };
        if !st.on_mesh {
            return false;
        }
        st.target = Some(target);
        let ok = if st.link.is_some() || self.meshes[st.mesh].job.is_some() {
            // Planned after the link or once the rebuilt tiles are in.
            st.path_pending = true;
            true
        } else {
            self.plan(&mut st, &comp)
        };
        self.agents.insert(e, st);
        ok
    }

    pub fn reset_path(&mut self, scene: &Scene, e: Entity) {
        if let Some(st) = self.agent(scene, e) {
            st.corners.clear();
            st.corner_links.clear();
            st.path_polys.clear();
            st.target = None;
            st.path_pending = false;
        }
    }

    /// `Warp`: jump to the mesh point nearest `pos` (clears the path).
    pub fn warp(&mut self, scene: &mut Scene, e: Entity, pos: Vec3) -> bool {
        let Some(comp) = agent_of(scene, e) else {
            return false;
        };
        let mut st = self.agent(scene, e).cloned().unwrap_or_default();
        st.target = None;
        st.path_pending = false;
        let ok = self.place(&mut st, &comp, pos);
        if ok {
            let world = st.position + Vec3::Y * comp.base_offset;
            scene.set_world_position(e, world);
            st.written = world;
        }
        self.agents.insert(e, st);
        ok
    }

    /// `CompleteOffMeshLink`: finish the link the agent is on (it lands at the far end).
    pub fn complete_off_mesh_link(&mut self, scene: &mut Scene, e: Entity) -> bool {
        let Some(comp) = agent_of(scene, e) else {
            return false;
        };
        let Some(mut st) = self.agents.get(&e).cloned() else {
            return false;
        };
        if st.link.is_none() {
            return false;
        }
        self.finish_link(e, &mut st);
        let world = st.position + Vec3::Y * comp.base_offset;
        scene.set_world_position(e, world);
        st.written = world;
        self.agents.insert(e, st);
        true
    }

    /// Corners still to walk, starting at the agent (for the editor's path gizmo).
    pub fn path_of(&self, e: Entity) -> Option<Vec<Vec3>> {
        let st = self.agents.get(&e)?;
        st.has_path().then(|| {
            std::iter::once(st.position)
                .chain(st.corners.iter().copied())
                .collect()
        })
    }

    /// Every agent's position, radius-free (editor gizmos).
    pub fn agent_entities(&self) -> Vec<Entity> {
        self.agents.keys().copied().collect()
    }

    // ---- runtime rebuilds -----------------------------------------------------------------

    /// Bake input for a runtime mesh from the scene as it is now.
    fn builder_for(&self, scene: &Scene, i: usize) -> Option<Arc<TileBuilder>> {
        let m = &self.meshes[i];
        let input = match m.source {
            // Given meshes are assumed to come from the Navigation Static bake.
            Source::Scene | Source::Given => geometry::collect_static(scene),
            Source::Surface(e) => {
                let s = scene.world.get::<&NavMeshSurface>(e).ok()?.clone();
                geometry::collect_surface(scene, e, &s)
            }
        };
        Some(Arc::new(TileBuilder::new(input, m.mesh.settings)))
    }

    /// `NavMeshSurface.BuildNavMesh` (`full`) / `UpdateNavMesh` while playing: rebuild the
    /// surface's tiles in the background (only those whose input changed unless `full`). A
    /// surface without baked data gets a mesh.
    pub fn rebuild_surface(&mut self, scene: &Scene, e: Entity, full: bool) -> bool {
        let s = match scene.world.get::<&NavMeshSurface>(e) {
            Ok(s) => (*s).clone(),
            Err(_) => return false,
        };
        let i = match self
            .meshes
            .iter()
            .position(|m| m.source == Source::Surface(e))
        {
            Some(i) => i,
            None => {
                let input = geometry::collect_surface(scene, e, &s);
                let settings = super::bake::surface_settings(&input, &self.settings, &s);
                self.push_mesh(
                    NavMesh::from_tiles(settings, Vec::new()),
                    Source::Surface(e),
                );
                self.meshes.len() - 1
            }
        };
        let Some(b) = self.builder_for(scene, i) else {
            return false;
        };
        let carves = self.carve_shapes();
        let m = &mut self.meshes[i];
        let mut dirty: HashSet<(i32, i32)> = HashSet::new();
        for (x, z) in b.tiles() {
            let same = m
                .mesh
                .tile_index(x, z)
                .is_some_and(|t| m.mesh.tiles()[t as usize].hash == b.hash(x, z, &carves));
            if full || !same {
                dirty.insert((x, z));
            }
        }
        // Tiles with nothing left become empty.
        let have: HashSet<_> = b.tiles().into_iter().collect();
        for t in m.mesh.tiles() {
            if !have.contains(&(t.x, t.z)) && !t.polys.is_empty() {
                dirty.insert((t.x, t.z));
            }
        }
        m.builder = Some(b);
        m.dirty.extend(dirty);
        self.start_jobs();
        true
    }

    fn carve_shapes(&self) -> Vec<CarveShape> {
        let mut v: Vec<(u64, CarveShape)> = self
            .obstacles
            .iter()
            .filter_map(|(e, o)| o.carved.map(|c| (e.to_bits().get(), c)))
            .collect();
        v.sort_by_key(|(k, _)| *k);
        v.into_iter().map(|(_, c)| c).collect()
    }

    /// Start a rebuild of each mesh's dirty tiles (one job per mesh at a time).
    fn start_jobs(&mut self) {
        let carves = self.carve_shapes();
        for m in &mut self.meshes {
            if m.job.is_some() || m.dirty.is_empty() {
                continue;
            }
            let Some(b) = m.builder.clone() else {
                m.dirty.clear();
                continue;
            };
            let mut tiles: Vec<(i32, i32)> = m.dirty.drain().collect();
            tiles.sort_unstable();
            let result = Arc::new(Mutex::new(None));
            let out = result.clone();
            let carves = carves.clone();
            let work = move || {
                use rayon::prelude::*;
                let built: Vec<Tile> = tiles
                    .par_iter()
                    .map(|&(x, z)| b.build(x, z, &carves))
                    .collect();
                *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(built);
            };
            if self.sync_rebuilds {
                crate::background::pool().install(work);
            } else {
                crate::background::pool().spawn(work);
            }
            m.job = Some(Job { result });
        }
    }

    /// Swap in finished rebuilds; agents on changed tiles are placed again and re-plan.
    fn poll_jobs(&mut self, scene: &Scene) {
        for i in 0..self.meshes.len() {
            let done = self.meshes[i]
                .job
                .as_ref()
                .and_then(|j| j.result.lock().unwrap_or_else(|e| e.into_inner()).take());
            let Some(tiles) = done else {
                continue;
            };
            self.meshes[i].job = None;
            let coords: Vec<(i32, i32)> = tiles.iter().map(|t| (t.x, t.z)).collect();
            self.stats.tiles_rebuilt += tiles.len();
            self.stats.rebuilds += 1;
            let m = &mut self.meshes[i];
            m.mesh.replace_tiles(tiles);
            let changed: HashSet<u32> = coords
                .iter()
                .filter_map(|&(x, z)| m.mesh.tile_index(x, z))
                .collect();
            self.refresh_agents(scene, i, &changed);
        }
        self.start_jobs();
    }

    /// Agents on mesh `mi` touching `changed` tiles: placed again, paths planned again.
    fn refresh_agents(&mut self, scene: &Scene, mi: usize, changed: &HashSet<u32>) {
        let ids: Vec<Entity> = self.agents.keys().copied().collect();
        for e in ids {
            let Some(comp) = agent_of(scene, e) else {
                continue;
            };
            let mut st = self.agents[&e].clone();
            if st.mesh != mi || !st.on_mesh {
                continue;
            }
            let on_changed = changed.contains(&tile_of(st.poly));
            let crosses = st.path_polys.iter().any(|p| changed.contains(&tile_of(*p)));
            if on_changed && st.link.is_none() {
                let m = &self.meshes[mi].mesh;
                let f = self.agent_filter(&comp, &st);
                match m.nearest_filtered(st.position, self.reach(comp.agent_type), &f) {
                    Some(h) => {
                        st.poly = h.poly;
                        st.position = h.position;
                    }
                    None => st.on_mesh = false,
                }
            }
            if (on_changed || crosses || st.path_pending)
                && st.target.is_some()
                && st.link.is_none()
            {
                if comp.auto_repath || st.path_pending {
                    self.plan(&mut st, &comp);
                } else {
                    st.path_polys.clear();
                }
            }
            self.agents.insert(e, st);
        }
    }

    // ---- frame ----------------------------------------------------------------------------

    /// Move every agent by one frame.
    pub fn update(&mut self, scene: &mut Scene, dt: f32) {
        let mut agents: Vec<(Entity, NavMeshAgent)> = Vec::new();
        let mut obstacle_list: Vec<(Entity, NavMeshObstacle)> = Vec::new();
        let mut link_list: Vec<(Entity, NavMeshLink)> = Vec::new();
        for (e, _) in scene.walk() {
            if !scene.active_in_hierarchy(e) {
                continue;
            }
            if let Some(a) = agent_of(scene, e) {
                agents.push((e, a));
            }
            if let Ok(o) = scene.world.get::<&NavMeshObstacle>(e)
                && o.enabled
            {
                obstacle_list.push((e, (*o).clone()));
            }
            if let Ok(l) = scene.world.get::<&NavMeshLink>(e)
                && l.enabled
            {
                link_list.push((e, (*l).clone()));
            }
        }
        if self.meshes.is_empty() {
            return;
        }
        self.agents
            .retain(|e, _| agents.iter().any(|(a, _)| a == e));
        for (e, _) in &agents {
            self.agent(scene, *e);
        }
        self.poll_jobs(scene);
        self.update_links(scene, &link_list);
        self.update_carving(scene, &obstacle_list, dt);
        if dt <= 0.0 {
            return;
        }
        // Avoidance obstacles: everything not carved right now.
        let footprints: Vec<Obstacle> = obstacle_list
            .iter()
            .filter(|(e, _)| self.obstacles.get(e).is_none_or(|s| s.carved.is_none()))
            .map(|(e, o)| obstacle_footprint(scene, *e, o))
            .collect();
        // 1. Desired velocities (and links).
        let mut states: Vec<(Entity, AgentState)> = Vec::with_capacity(agents.len());
        for (e, comp) in &agents {
            let Some(mut st) = self.agents.get(e).cloned() else {
                continue;
            };
            // A script moved the object: start from there (not while it handles a link).
            let now = scene.world_position(*e);
            let manual_link = st.link.as_ref().is_some_and(|l| !l.auto);
            if now.distance(st.written) > 1e-3 && !manual_link {
                let target = st.target;
                self.place(&mut st, comp, now - Vec3::Y * comp.base_offset);
                st.target = target;
                if target.is_some() && st.on_mesh {
                    self.plan(&mut st, comp);
                }
            }
            if st.path_pending && st.link.is_none() && self.meshes[st.mesh].job.is_none() {
                self.plan(&mut st, comp);
            }
            if st.on_mesh && st.link.is_none() {
                self.steer(*e, &mut st, comp);
            }
            states.push((*e, st));
        }
        // 2. Avoidance.
        let new_vel = self.avoid(&states, &agents, &footprints, dt);
        // 3. Move.
        let comp_of: HashMap<Entity, &NavMeshAgent> = agents.iter().map(|(e, a)| (*e, a)).collect();
        for ((e, mut st), v) in states.into_iter().zip(new_vel) {
            let comp = comp_of[&e];
            if st.link.is_some() {
                self.traverse(e, &mut st, comp, dt);
                if st.link.as_ref().is_none_or(|l| l.auto) {
                    write_transform(scene, e, &mut st, comp, dt);
                } else {
                    st.written = scene.world_position(e);
                }
            } else if st.on_mesh {
                st.velocity = v;
                self.advance(&mut st, comp, dt);
                write_transform(scene, e, &mut st, comp, dt);
            }
            self.agents.insert(e, st);
        }
        self.resolve_overlaps(scene, &agents, &footprints);
    }

    /// Desired velocity towards the next corner; starts crossing a link when at its start.
    fn steer(&mut self, e: Entity, st: &mut AgentState, comp: &NavMeshAgent) {
        while st.corners.len() > 1
            && st.corner_links.first().is_some_and(|l| l.is_none())
            && st.position.xz().distance(st.corners[0].xz()) < CORNER_REACHED
        {
            st.corners.remove(0);
            st.corner_links.remove(0);
        }
        // At a link start: cross it.
        if let (Some(&c), Some(Some(step))) = (st.corners.first(), st.corner_links.first())
            && st.position.xz().distance(c.xz()) < CORNER_REACHED.max(comp.radius * 0.5)
            && !st.is_stopped
        {
            let step = *step;
            self.begin_link(e, st, comp, step);
            st.desired_velocity = Vec3::ZERO;
            return;
        }
        let mut desired = Vec3::ZERO;
        let remaining = st.remaining_distance();
        if let Some(&target) = st.corners.first()
            && !st.is_stopped
        {
            let last = st.corners.len() == 1;
            if last && remaining <= comp.stopping_distance.max(0.0) + 1e-3 {
                // Arrived: an exact stop drops the path.
                if comp.stopping_distance <= 1e-3 {
                    st.position = Vec3::new(target.x, st.position.y, target.z);
                    st.corners.clear();
                    st.corner_links.clear();
                    st.path_polys.clear();
                }
                st.velocity = Vec3::ZERO;
            } else {
                let dir = (target - st.position).xz().normalize_or_zero();
                let mut speed = comp.speed.max(0.0);
                if comp.auto_braking && st.corner_links.iter().all(|l| l.is_none()) {
                    let left = (remaining - comp.stopping_distance).max(0.0);
                    speed = speed.min((2.0 * comp.acceleration.max(0.01) * left).sqrt());
                }
                desired = Vec3::new(dir.x, 0.0, dir.y) * speed;
            }
        }
        st.desired_velocity = desired;
    }

    fn begin_link(&mut self, e: Entity, st: &mut AgentState, comp: &NavMeshAgent, step: LinkStep) {
        let Some(m) = self.meshes.get(st.mesh) else {
            return;
        };
        let Some(link) = m.mesh.off_links.get(step.link as usize) else {
            st.corner_links[0] = None;
            return;
        };
        let (from, to, _, to_poly) = link.ends(step.forward);
        st.link = Some(LinkTraversal {
            start: from,
            end: to,
            area: link.area,
            end_poly: to_poly,
            t: 0.0,
            auto: comp.auto_traverse_off_mesh_link,
        });
        st.position = from;
        st.velocity = Vec3::ZERO;
        self.events.push(NavEvent {
            entity: e,
            kind: NavEventKind::LinkStart,
            start: from,
            end: to,
            area: link.area,
        });
    }

    /// Cross the link (auto) or wait for `CompleteOffMeshLink`.
    fn traverse(&mut self, e: Entity, st: &mut AgentState, comp: &NavMeshAgent, dt: f32) {
        let Some(l) = st.link.clone() else {
            return;
        };
        if !l.auto {
            st.velocity = Vec3::ZERO;
            return;
        }
        let len = l.start.distance(l.end).max(0.01);
        let t = (l.t + dt * comp.speed.max(0.1) / len).min(1.0);
        let mut p = l.start.lerp(l.end, t);
        if l.area == AREA_JUMP {
            let h = (len * 0.25).max(0.5);
            p.y += 4.0 * h * t * (1.0 - t);
        }
        st.velocity = (l.end - l.start).with_y(0.0) / len * comp.speed;
        st.position = p;
        if let Some(lk) = st.link.as_mut() {
            lk.t = t;
        }
        if t >= 1.0 {
            self.finish_link(e, st);
        }
    }

    fn finish_link(&mut self, e: Entity, st: &mut AgentState) {
        let Some(l) = st.link.take() else {
            return;
        };
        st.position = l.end;
        st.poly = l.end_poly;
        // Drop the link's start corner, and its end unless it is the destination.
        if !st.corners.is_empty() {
            st.corners.remove(0);
            st.corner_links.remove(0);
        }
        if st
            .corners
            .first()
            .is_some_and(|c| c.xz().distance(l.end.xz()) < 0.05)
            && st.corners.len() > 1
        {
            st.corners.remove(0);
            st.corner_links.remove(0);
        }
        self.events.push(NavEvent {
            entity: e,
            kind: NavEventKind::LinkEnd,
            start: l.start,
            end: l.end,
            area: l.area,
        });
    }

    /// ORCA velocities for every agent (in `states` order).
    fn avoid(
        &self,
        states: &[(Entity, AgentState)],
        agents: &[(Entity, NavMeshAgent)],
        obstacles: &[Obstacle],
        dt: f32,
    ) -> Vec<Vec3> {
        let comps: HashMap<Entity, &NavMeshAgent> = agents.iter().map(|(e, a)| (*e, a)).collect();
        let bodies: Vec<(Entity, orca::Body, f32, i32, f32, bool)> = states
            .iter()
            .map(|(e, st)| {
                let c = comps[e];
                (
                    *e,
                    orca::Body {
                        position: st.position.xz(),
                        velocity: st.velocity.xz(),
                        radius: c.radius,
                    },
                    st.position.y,
                    c.priority,
                    c.height,
                    st.on_mesh && st.link.is_none(),
                )
            })
            .collect();
        // Bodies bucketed in 4 m cells for the neighbour search.
        let cell_of = |p: Vec2| ((p.x / 4.0).floor() as i32, (p.y / 4.0).floor() as i32);
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (j, b) in bodies.iter().enumerate() {
            grid.entry(cell_of(b.1.position)).or_default().push(j);
        }
        states
            .iter()
            .enumerate()
            .map(|(i, (e, st))| {
                let c = comps[e];
                // Preferred velocity: the current one turned towards the desired one.
                let pref = move_towards(
                    st.velocity,
                    st.desired_velocity,
                    c.acceleration.max(0.0) * dt,
                );
                let Some((max_n, horizon)) = c.obstacle_avoidance.params() else {
                    return pref;
                };
                if !bodies[i].5 {
                    return pref;
                }
                let me = bodies[i].1;
                // Neighbours within the horizon, at most twelve radii away.
                let range = (c.speed * horizon).min(c.radius * 12.0) + c.radius * 2.0 + 1.0;
                let (x0, z0) = cell_of(me.position - Vec2::splat(range));
                let (x1, z1) = cell_of(me.position + Vec2::splat(range));
                let mut near: Vec<(f32, orca::Body, f32)> = (z0..=z1)
                    .flat_map(|z| (x0..=x1).map(move |x| (x, z)))
                    .filter_map(|k| grid.get(&k))
                    .flatten()
                    .map(|&j| (j, &bodies[j]))
                    .filter(|(j, b)| {
                        *j != i && b.5 && (b.2 - st.position.y).abs() < c.height.max(b.4)
                    })
                    .filter_map(|(_, b)| {
                        let d = b.1.position.distance(me.position);
                        if d > range {
                            return None;
                        }
                        let share = match c.priority.cmp(&b.3) {
                            std::cmp::Ordering::Less => 0.0,
                            std::cmp::Ordering::Greater => 1.0,
                            std::cmp::Ordering::Equal => 0.5,
                        };
                        Some((d, b.1, share))
                    })
                    .collect();
                near.sort_by(|a, b| a.0.total_cmp(&b.0));
                near.truncate(max_n);
                let neighbours: Vec<(orca::Body, f32)> =
                    near.into_iter().map(|(_, b, s)| (b, s)).collect();
                let obstacle_range = c.radius * 2.0 + c.speed + 0.5;
                let points: Vec<Vec2> = obstacles
                    .iter()
                    .filter_map(|o| {
                        let (p, d) = o.closest(st.position);
                        (d < obstacle_range).then_some(p.xz())
                    })
                    .collect();
                let params = orca::Params {
                    max_speed: c.speed.max(0.0),
                    horizon,
                    obstacle_horizon: 1.0,
                    dt,
                };
                let pref2 = if pref.length_squared() > 1e-6 {
                    pref.xz() + orca::nudge(e.to_bits().get())
                } else {
                    pref.xz()
                };
                let v = orca::velocity(&me, pref2, &neighbours, &points, &params);
                Vec3::new(v.x, 0.0, v.y)
            })
            .collect()
    }

    /// Move one agent by its velocity along the mesh (not past its destination or a link).
    fn advance(&self, st: &mut AgentState, comp: &NavMeshAgent, dt: f32) {
        let Some(m) = self.meshes.get(st.mesh) else {
            return;
        };
        let mut to = st.position + st.velocity * dt;
        let stop_at = st.corners.first().copied().filter(|_| {
            st.corners.len() == 1 || st.corner_links.first().is_some_and(|l| l.is_some())
        });
        if let Some(end) = stop_at
            && st.desired_velocity.length_squared() > 0.0
        {
            let is_link = st.corner_links.first().is_some_and(|l| l.is_some());
            let stop = if is_link { 0.0 } else { comp.stopping_distance };
            let left = (end - st.position).xz().length();
            if st.velocity.length() * dt >= left - stop && left > 0.0 {
                to = if stop > 0.0 {
                    st.position + (to - st.position).clamp_length_max((left - stop).max(0.0))
                } else {
                    end
                };
            }
        }
        let f = self.agent_filter(comp, st);
        let (poly, p) = m.mesh.move_along(st.poly, st.position, to, &f);
        st.poly = poly;
        st.position = p;
    }

    /// Push overlapping agents apart (and out of obstacles), staying on the mesh.
    fn resolve_overlaps(
        &mut self,
        scene: &mut Scene,
        agents: &[(Entity, NavMeshAgent)],
        obstacles: &[Obstacle],
    ) {
        let avoiding: Vec<&(Entity, NavMeshAgent)> = agents
            .iter()
            .filter(|(e, a)| {
                a.obstacle_avoidance != ObstacleAvoidance::None
                    && self
                        .agents
                        .get(e)
                        .is_some_and(|s| s.on_mesh && s.link.is_none())
            })
            .collect();
        let mut offsets: HashMap<Entity, Vec3> = HashMap::new();
        for (i, (ea, a)) in avoiding.iter().enumerate() {
            let pa = self.agents[ea].position;
            for (eb, b) in avoiding.iter().skip(i + 1) {
                let pb = self.agents[eb].position;
                let d = (pa - pb).xz();
                let overlap = a.radius + b.radius - d.length();
                if overlap <= 0.0 || (pa.y - pb.y).abs() > a.height.max(b.height) {
                    continue;
                }
                let dir = d.try_normalize().unwrap_or(Vec2::X);
                let (wa, wb) = priority_weights(a.priority, b.priority);
                let push = Vec3::new(dir.x, 0.0, dir.y) * overlap;
                *offsets.entry(*ea).or_default() += push * wa;
                *offsets.entry(*eb).or_default() -= push * wb;
            }
            for o in obstacles {
                if let Some(out) = o.push_out(pa, a.radius) {
                    *offsets.entry(*ea).or_default() += out;
                }
            }
        }
        let mut ids: Vec<Entity> = offsets.keys().copied().collect();
        ids.sort_by_key(|e| e.to_bits());
        for e in ids {
            let off = offsets[&e];
            let Some((_, comp)) = agents.iter().find(|(a, _)| *a == e) else {
                continue;
            };
            let Some(mut st) = self.agents.get(&e).cloned() else {
                continue;
            };
            let Some(m) = self.meshes.get(st.mesh) else {
                continue;
            };
            let f = self.agent_filter(comp, &st);
            let (poly, p) = m
                .mesh
                .move_along(st.poly, st.position, st.position + off, &f);
            st.poly = poly;
            st.position = p;
            let world = p + Vec3::Y * comp.base_offset;
            scene.set_world_position(e, world);
            st.written = world;
            self.agents.insert(e, st);
        }
    }

    /// Snap the scene's NavMesh Links onto the meshes of their agent types (when they change).
    fn update_links(&mut self, scene: &Scene, list: &[(Entity, NavMeshLink)]) {
        let links: Vec<(u32, OffMeshLink)> = list
            .iter()
            .map(|(e, l)| {
                let m = scene.world_matrix(*e);
                (
                    l.agent_type,
                    OffMeshLink {
                        start: m.transform_point3(l.start),
                        end: m.transform_point3(l.end),
                        width: l.width,
                        bidirectional: l.bidirectional,
                        cost_modifier: l.cost_modifier,
                        area: l.area,
                        owner: e.to_bits().get(),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let plain: Vec<OffMeshLink> = links.iter().map(|(_, l)| l.clone()).collect();
        let tiles_changed = self.stats.rebuilds;
        if plain == self.links && self.link_epoch == tiles_changed {
            return;
        }
        self.links = plain;
        self.link_epoch = tiles_changed;
        for m in &mut self.meshes {
            let mine: Vec<OffMeshLink> = links
                .iter()
                .filter(|(t, _)| *t == m.agent_type)
                .map(|(_, l)| l.clone())
                .collect();
            m.mesh.set_off_mesh_links(mine);
        }
    }

    /// Track carving obstacles: carve when stationary (or always), mark tiles they touched.
    fn update_carving(&mut self, scene: &Scene, list: &[(Entity, NavMeshObstacle)], dt: f32) {
        let mut dirty: Vec<(Vec2, Vec2)> = Vec::new();
        let alive: HashSet<Entity> = list
            .iter()
            .filter(|(_, o)| o.carve)
            .map(|(e, _)| *e)
            .collect();
        // Gone or no longer carving: uncarve.
        let gone: Vec<Entity> = self
            .obstacles
            .keys()
            .filter(|e| !alive.contains(e))
            .copied()
            .collect();
        for e in gone {
            if let Some(c) = self.obstacles.remove(&e).and_then(|s| s.carved) {
                dirty.push(c.bounds(0.0));
            }
        }
        for (e, o) in list {
            if !o.carve {
                continue;
            }
            let shape = carve_shape(scene, *e, o);
            let pos = Vec3::new(shape.center.x, shape.y_min, shape.center.y);
            let st = self.obstacles.entry(*e).or_insert_with(|| ObstacleState {
                last: pos,
                ..Default::default()
            });
            if pos.distance(st.last) > 1e-4 {
                st.still = 0.0;
            } else {
                st.still += dt.max(0.0);
            }
            st.last = pos;
            let carve_now = !o.carve_only_stationary || st.still >= o.time_to_stationary;
            match (carve_now, st.carved) {
                (true, None) => {
                    dirty.push(shape.bounds(0.0));
                    st.carved = Some(shape);
                }
                (true, Some(old)) => {
                    let moved = old.center.distance(shape.center) > o.move_threshold.max(0.0)
                        || (old.y_min - shape.y_min).abs() > o.move_threshold.max(0.0)
                        || old.axis.dot(shape.axis) < 0.999
                        || old.half != shape.half
                        || old.radius != shape.radius
                        || old.capsule != shape.capsule;
                    if moved {
                        dirty.push(old.bounds(0.0));
                        dirty.push(shape.bounds(0.0));
                        st.carved = Some(shape);
                    }
                }
                (false, Some(old)) => {
                    dirty.push(old.bounds(0.0));
                    st.carved = None;
                }
                (false, None) => {}
            }
        }
        if dirty.is_empty() {
            return;
        }
        for i in 0..self.meshes.len() {
            if self.meshes[i].builder.is_none() {
                self.meshes[i].builder = self.builder_for(scene, i);
            }
            let Some(b) = self.meshes[i].builder.clone() else {
                continue;
            };
            for (lo, hi) in &dirty {
                self.meshes[i].dirty.extend(b.tiles_touching(*lo, *hi));
            }
        }
        self.start_jobs();
    }

    /// Wait until every running rebuild is in (tests, editor stop).
    pub fn finish_rebuilds(&mut self, scene: &Scene) {
        for _ in 0..10_000 {
            if !self.rebuilding() {
                return;
            }
            self.poll_jobs(scene);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

/// Share of an overlap each agent moves: the less important one (higher number) moves more.
fn priority_weights(a: i32, b: i32) -> (f32, f32) {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => (0.2, 0.8),
        std::cmp::Ordering::Greater => (0.8, 0.2),
        std::cmp::Ordering::Equal => (0.5, 0.5),
    }
}

/// Place the object at the agent position (+ base offset) and turn it towards the velocity.
fn write_transform(
    scene: &mut Scene,
    e: Entity,
    st: &mut AgentState,
    comp: &NavMeshAgent,
    dt: f32,
) {
    let world = st.position + Vec3::Y * comp.base_offset;
    scene.set_world_position(e, world);
    st.written = world;
    let v = st.velocity.xz();
    if v.length() > 0.05 {
        let current = scene.world_rotation(e) * Vec3::Z;
        let from = yaw_of(current.xz().try_normalize().unwrap_or(Vec2::Y));
        let to = yaw_of(v);
        let diff = (to - from + 540.0).rem_euclid(360.0) - 180.0;
        // A negative or NaN Angular Speed (scripts, scene files) made `clamp` panic.
        let max = (comp.angular_speed * dt).abs();
        let turn = if max.is_nan() {
            0.0
        } else {
            diff.clamp(-max, max)
        };
        let yaw = (from + turn).to_radians();
        scene.set_world_rotation(e, Quat::from_rotation_y(yaw));
    }
}

/// A Nav Mesh Obstacle's footprint on x/z.
pub(super) struct Obstacle {
    center: Vec2,
    /// Box axes (unit) and half sizes; a capsule is a circle (`radius`).
    axes: [Vec2; 2],
    half: Vec2,
    radius: f32,
    capsule: bool,
}

impl Obstacle {
    /// Closest point of the footprint to `p` (with p's height) and the distance to it.
    fn closest(&self, p: Vec3) -> (Vec3, f32) {
        let q = p.xz() - self.center;
        let c = if self.capsule {
            q.clamp_length_max(self.radius)
        } else {
            let (u, v) = (q.dot(self.axes[0]), q.dot(self.axes[1]));
            self.axes[0] * u.clamp(-self.half.x, self.half.x)
                + self.axes[1] * v.clamp(-self.half.y, self.half.y)
        };
        let world = Vec3::new(c.x + self.center.x, p.y, c.y + self.center.y);
        (world, (q - c).length())
    }

    /// Offset moving a circle of radius `r` at `p` out of the footprint (None = clear).
    fn push_out(&self, p: Vec3, r: f32) -> Option<Vec3> {
        let (c, dist) = self.closest(p);
        if dist >= r {
            return None;
        }
        let d = (p - c).xz();
        let dir = if dist > 1e-5 {
            d / dist
        } else {
            // Inside: leave through the nearest side.
            let q = p.xz() - self.center;
            q.try_normalize().unwrap_or(Vec2::X)
        };
        let depth = if dist > 1e-5 {
            r - dist
        } else {
            r + self.half.max_element().max(self.radius)
        };
        Some(Vec3::new(dir.x, 0.0, dir.y) * depth)
    }
}

fn obstacle_footprint(scene: &Scene, e: Entity, o: &NavMeshObstacle) -> Obstacle {
    let m = scene.world_matrix(e);
    let (scale, rot, _) = m.to_scale_rotation_translation();
    let center = m.transform_point3(o.center).xz();
    let x = (rot * Vec3::X).xz().try_normalize().unwrap_or(Vec2::X);
    Obstacle {
        center,
        axes: [x, Vec2::new(-x.y, x.x)],
        half: (o.size.xz() * scale.xz().abs() * 0.5).max(Vec2::ZERO),
        radius: o.radius * scale.x.abs().max(scale.z.abs()),
        capsule: o.shape == ObstacleShape::Capsule,
    }
}

/// A carving obstacle's footprint and height range (world).
pub(super) fn carve_shape(scene: &Scene, e: Entity, o: &NavMeshObstacle) -> CarveShape {
    let m = scene.world_matrix(e);
    let (scale, rot, _) = m.to_scale_rotation_translation();
    let c = m.transform_point3(o.center);
    let x = (rot * Vec3::X).xz().try_normalize().unwrap_or(Vec2::X);
    let (half_y, radius, half) = match o.shape {
        ObstacleShape::Box => (
            o.size.y * scale.y.abs() * 0.5,
            0.0,
            (o.size.xz() * scale.xz().abs() * 0.5).max(Vec2::ZERO),
        ),
        ObstacleShape::Capsule => (
            o.height * scale.y.abs() * 0.5,
            o.radius * scale.x.abs().max(scale.z.abs()),
            Vec2::ZERO,
        ),
    };
    CarveShape {
        center: c.xz(),
        axis: x,
        half,
        radius,
        capsule: o.shape == ObstacleShape::Capsule,
        y_min: c.y - half_y,
        y_max: c.y + half_y,
    }
}
