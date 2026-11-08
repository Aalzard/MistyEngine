# Navigation: NavMesh parity (round 19B, branch r19-nav)

Goal: the familiar engine's AI Navigation behaviour — tiled voxel nav meshes rebuilt at runtime,
NavMesh Surface / Modifier / Modifier Volume / Link components, areas with costs and masks,
carving obstacles, several agent types, reciprocal avoidance and the scripting API. The behaviour
follows the familiar package; the code and the file format are our own.

## What works

| Feature | How it behaves | Where |
|---|---|---|
| Voxel bake | Triangles of the collected objects are rasterised into a heightfield per tile (plus a border of `ceil(radius / voxel) + 3` voxels so erosion and corner heights see past the tile edge). Spans an agent can stand on (slope, clearance, step) become cells linked to their neighbours; kerbs up to the step height become walkable; cells closer than the agent radius to an edge are eroded (chamfer distance); islands smaller than Min Region Area are dropped (unless they continue in the next tile). Closed primitives are solids. A face that only touches a voxel column's side doesn't spill into it. | `navigation/heightfield.rs` |
| Regions | Monotone partition: each row is cut into runs of connected cells of one area; a run joins the region of the row before when that region continues into it alone. No holes, no mixed areas; border cells get no region, so outlines follow the tile edge. | `navigation/regions.rs` |
| Contours | Each region's boundary is walked along cell edges; corner heights are the mean of the cells around the corner (exact on planes, never changed by carving). Simplified: points where the neighbouring region changes stay; wall edges and edges to another area are refined until no raw point is further than 1.3 voxels; wall edges longer than 12 m are split; the four tile sides are separate "regions" so tile corners stay. Shared edges are walked in a fixed order so both sides simplify alike. | `navigation/contours.rs` |
| Polygons | Ear clipping on exact integer corner coordinates (shortest diagonal first), then triangles of a region merged into convex polygons of up to 6 corners (longest shared edge first). | `navigation/polymesh.rs` |
| Detail heights | Every polygon has detail triangles: polygon edges are split where the voxel surface bends away from them (sampled from the lower endpoint and averaged across the edge, so neighbours — also in other tiles — agree), then interior samples further than the detail error (half a voxel, at least 5 cm) are inserted, up to 32. `height()` reads them (terrain, ramps meeting platforms). | `polymesh.rs` (`add_edge_detail`, `add_detail`), `NavPoly::detail_at` |
| Tiles | 64-voxel tiles by default (Navigation ▸ Bake ▸ Tile Size, Surface ▸ Override Tile Size) on a grid anchored at the world origin, so adding geometry at the edge doesn't move the other tiles. Polygons are addressed as tile << 16 \| polygon. Portals inside a tile come from shared edges, between tiles from overlapping edges on the shared tile side (heights within the step height). Bakes run tile by tile in parallel on the background pool; a tile whose input hash (triangles, areas, volumes, carving shapes, settings) is unchanged since the previous bake is reused. | `navigation/tiles.rs`, `bake.rs`, `navmesh.rs` (`link_internal`, `link_external`, `replace_tiles`) |
| NavMesh Surface | Agent Type, Default Area, Collect Objects (All Game Objects / Volume with Center and Size, clipped to it / Current Object Hierarchy), Include Layers, Override Voxel Size, Override Tile Size, Minimum Region Area, Bake and Clear with a progress bar. Objects with a Nav Mesh Agent or Obstacle are never collected; no Static flag is needed. Data goes to `<Scene>/NavMesh-<object>.navmesh`. | `components.rs`, `geometry::collect_surface`, `inspector/nav_editors.rs` |
| NavMesh Modifier | Ignore From Build, Override Area, Apply To Children, Affected Agents. Area merging where surfaces meet within a step: Not Walkable wins, then the higher area. | `geometry::modifier_of`, `heightfield::merge_area` |
| NavMesh Modifier Volume | Size, Center, Area Type, Affected Agents: cells inside get the area after erosion (Not Walkable cuts the box out). | `geometry::AreaVolume`, `heightfield::mark_volumes` |
| NavMesh Link | Agent Type, Start / End Point, Width, Cost Modifier (or the area's cost), Bidirectional, Area Type. Ends snap to the nearest polygon of the agent type's meshes when play starts and whenever a link moves or tiles change. A* crosses links; paths mark the corner where a link starts. | `navmesh::OffMeshLink`, `agent::update_links`, `path::search` |
| NavMesh Obstacle | Box or Capsule; Carve with Move Threshold, Time To Stationary, Carve Only Stationary. A carving obstacle that settles (or moves more than the threshold while carving) marks the tiles its footprint + agent radius touches; those tiles are rebuilt on the background pool with every carved footprint cut out and swapped in when done; agents on them are placed again and re-plan (`pathPending` while their mesh rebuilds). Non-carving (or moving) obstacles are avoided by ORCA as hard constraints. | `agent::update_carving`, `heightfield::carve`, `agent::start_jobs` / `poll_jobs` |
| Areas and costs | 32 areas: Walkable (0), Not Walkable (1), Jump (2) built in, 29 user areas with names and costs ≥ 1 (Navigation ▸ Areas). Paths are A* over polygons, cost = metres × area cost, entering each polygon at the portal point a taut line would cross; per-agent area masks and per-agent cost overrides (`SetAreaCost`). | `mod.rs` (`NavArea`), `query::QueryFilter`, `path.rs` |
| String pulling | The polygon corridor is pulled tight per stretch between links (funnel), then corners the surface can see past are skipped (raycasts, never through areas dearer than the path's cheapest): A* over large polygons may pick a staircase of them where a straight line fits. | `funnel.rs`, `path::shortcut` |
| Agent types | Navigation ▸ Agents lists Humanoid (id 0) plus added types (radius, height, step, slope). Each Surface bakes for one type; agents walk on the meshes of their `agentTypeID`. | `NavigationSettings::agent_types`, `agent::nearest_on` |
| Agents | Speed, Angular Speed, Acceleration, Stopping Distance, Auto Braking (also not braking into links), Radius, Height, Base Offset, Quality, Priority, Auto Traverse Off Mesh Link, Auto Repath, Area Mask. | `components::NavMeshAgent`, `agent.rs` |
| ORCA | Each agent picks the velocity nearest its preferred one (the current velocity turned towards the steering target by the acceleration) inside every neighbour's half-plane over a time horizon; quality sets neighbours / horizon (Low 4 / 1 s, Medium 6 / 2 s, Good 8 / 3 s, High 10 / 4 s). Priority: equal shares the avoiding, the more important agent (lower number) leaves it all to the other. A 2D linear program (and its fallback when the lines can't all hold) solves it; a tiny fixed nudge per agent breaks perfectly symmetric meetings. Overlaps left (an agent pushed by the mesh edge) are resolved afterwards. | `navigation/orca.rs`, `agent::avoid` |
| Off-mesh links | At a link's start the agent crosses it at its speed (jumps arc), `isOnOffMeshLink` is true, `OnOffMeshLinkStart(link)` / `OnOffMeshLinkEnd(link)` run on its scripts. With Auto Traverse off it waits at the start (scripts move it as they like) until `CompleteOffMeshLink()` puts it on the far end. | `agent::begin_link`, `traverse`, `finish_link`, `navigation_api::dispatch_navigation_events` |
| Runtime rebuild | `GetComponent("NavMeshSurface"):BuildNavMesh()` rebuilds every tile of the surface from the scene as it is (a surface without baked data gets a mesh); `UpdateNavMesh()` only the tiles whose input changed. In the background, swapped in when done. | `NavRuntime::rebuild_surface` |

### Queries

`NavMesh.SamplePosition`, `CalculatePath`, `Raycast` (walks polygons along the segment; stops at a
wall or an excluded area with the wall's normal), `FindClosestEdge` (nearest uncovered edge part,
tile seams aren't walls) — all with an area mask and optional agent type.

### Lua

Agent: `SetDestination`, `destination`, `remainingDistance`, `hasPath`, `pathPending`, `pathStatus`,
`isOnNavMesh`, `isOnOffMeshLink`, `currentOffMeshLinkData` (`valid`, `activated`, `startPos`,
`endPos`, `area`), `CompleteOffMeshLink()`, `autoTraverseOffMeshLink`, `autoRepath`, `isStopped`,
`velocity`, `desiredVelocity`, `nextPosition`, `steeringTarget`, `path`, `Warp`, `Move`,
`ResetPath`, `areaMask`, `agentTypeID`, `obstacleAvoidanceType` (0–4), `SetAreaCost` /
`GetAreaCost`, `CalculatePath(target)`, `Raycast(target)` (→ blocked, hit), `FindClosestEdge()`,
plus the settings (`speed`, `acceleration`, …). Surface: `BuildNavMesh()`, `UpdateNavMesh()`,
`agentTypeID`. Obstacle: `carving`, `carveOnlyStationary`, `enabled`. `NavMesh`:
`SamplePosition(p, max [, mask [, agentType]])` (→ `position`, `distance`, `hit`, `mask`),
`CalculatePath(a, b [, mask [, agentType]])` (corners with `status`), `Raycast(a, b [, mask [,
agentType]])` (→ blocked, `{ hit, position, normal, distance, mask }`), `FindClosestEdge(p [, mask
[, agentType]])`, `GetAreaFromName`, `GetAreaCost`, `SetAreaCost`, `AllAreas` (-1). Callbacks
`OnOffMeshLinkStart(link)`, `OnOffMeshLinkEnd(link)`.

### Editor

- **Navigation window**: Agents (type list with + / −, name, radius / height / step / slope with the
  side-view diagram), Areas (32 rows: colour, name, cost), Bake (the scene's Navigation Static bake
  for the Humanoid with voxel, region and tile settings, Bake All Surfaces, a progress bar per bake,
  reused-tile counts in the status line), Object. Show NavMesh / Show Tiles / Show All Agent Paths.
- **Scene view**: nav meshes coloured by area (detail triangles, polygon edges), tile borders, link
  arcs; shown while the window is open or a Surface, Volume or Link is selected; in Play mode the
  live meshes with carved holes; agent paths (selected, or all). Gizmos: Modifier Volume boxes in
  their area colour, Surface volume, Link arcs with width bars, carving obstacles in red.
- **Inspector**: editors for all six components (Surface Bake / Clear and progress, agent type and
  area pickers with colours, area mask and affected-agents dropdowns).

## File format

- `.navmesh` format 2: a header line (`version`, `agent`, `cell_size`, `build`: agent, agent type,
  voxel, tile size, origin, min region area), then one tile per line: `{ "x", "z", "hash",
  "polygons": [{ "vertices", "area" (left out when Walkable), "detail_verts", "detail_tris" }] }`.
  Links are made when loading. Format 1 files (one tile, stored links) still load.
- Scene `"navigation"` block: new `agent_types`, `areas`, `tile_size` are written only when set, so
  existing scenes are unchanged. Components: Agent `agent_type`, `area_mask`,
  `auto_traverse_off_mesh_link`, `auto_repath` and Obstacle `carve` and its settings are written only
  when they differ from the defaults; NavMesh Surface / Modifier / Modifier Volume / Link are new.

## Sample

`Assets/Scenes/NavigationDemo2.scene` (generator and checks: `sample_navigation2`): a 28 × 20 m yard
split by a wall with a 1.6 m gap (Humanoids only) and a 4 m gap. Two NavMesh Surfaces (Humanoid,
Ogre: radius 0.9). Twelve Humanoids cross both ways (`Crowd.lua`) and walk round a Mud pool (NavMesh
Modifier, area "Mud" cost 6); two Ogres patrol through the wide gap; a jumper reaches a 1.2 m ledge
over a NavMesh Link; a carving Rolling Block slides back and forth (`Mover.lua`), its hole following
it tile by tile. NavigationDemo's nav mesh was rebaked in format 2.

## Tests

- `navigation::tests_build`: voxelising a box (one cell per column, erosion by the radius), contours
  of an L (six corners, exact area, convex polygons), tiles linking across borders (a straight path
  over 16+ tiles), partial rebake (only tiles near a moved box), detail heights on bumpy ground,
  modifiers and volumes (areas, ignore, cut out), agent types baking their own meshes.
- `navigation::tests_query`: funnel bends, path optimality on a pillar grid against an exact
  visibility-graph shortest path (within 6 %), area costs and masks, off-mesh links, raycasts,
  SamplePosition and FindClosestEdge.
- `navigation::tests_runtime`: carving rebuilds only the touched tiles and agents plan round the hole,
  moving carvers, link traversal (auto with events, manual with `CompleteOffMeshLink`), crowds
  crossing without overlapping, agents on their type's mesh, runtime `UpdateNavMesh`.
- `navigation::orca`: two crossing streams never overlap; obstacles are hard constraints.
- `navigation::tests` (updated): the earlier bake, path and agent behaviour.
- `scripting::navigation_api::tests`: links, areas, queries and callbacks from Lua (plus the earlier
  test).
- kittest (`panels/navigation_tests.rs`): menu, Bake / reuse / Clear, Agents tab add / remove / undo,
  Areas tab naming, Surface inspector bake with a Modifier Volume.
- `sample_navigation2`: matches its generator and plays (no overlaps, nobody in the mud, Ogres only
  through the wide gap, the jumper on the ledge, carved tiles rebuilt).

## Screenshots

`docs/screenshots/r19/`: `nav_window_agents.png` (Agents tab, both surfaces' meshes coloured by
area, the Mud pool in magenta), `nav_areas_tiles.png` (Areas tab, tile borders),
`nav_surface_inspector.png` (NavMesh Surface inspector), `nav_link_inspector.png` (NavMesh Link
inspector and its arc onto the ledge), `nav_play_carving.png` (Play mode: live meshes with the
carved holes, every agent's path, tiles). Captured with `MISTY_SCENE`, `MISTY_TAB`,
`MISTY_SELECT`, `MISTY_NAV_PAGE`, `MISTY_NAV_TILES`, `MISTY_NAV_PATHS`, `MISTY_NAV_VIEW`
(x,y,z,pitch,yaw of the Scene view camera) and `MISTY_PLAY`.

## Performance

`cargo test --release -p misty_engine -- --ignored nav_perf_report --nocapture` (this machine):

| What | Time |
|---|---|
| Bake a 160 m bumpy terrain with 80 boxes (206k triangles, 0.167 m voxels): 256 tiles, 1117 polygons | 0.14 s |
| Rebake with nothing changed (every tile reused) | 0.02 s |
| A carving obstacle settling: 1 tile rebuilt (background pool) | ≈ 5 ms |
| `SetDestination` | 0.006 ms per agent |
| Agent update, 100 agents crossing (High Quality avoidance) | 0.52 ms per frame |
| Agent update, 400 agents | 4.0 ms per frame |

## Not done yet

- Regions are monotone only (no watershed): long thin polygons in cluttered levels; the raycast
  shortcut hides most of the path-quality cost.
- No automatic drop / jump link generation; links connect polygons of one mesh (not across
  surfaces); link Width only widens the end snapping.
- Carving rebuilds whole tiles from the collected geometry (no compressed tile cache); the first
  carve of a game collects the scene's geometry on the main thread.
- No NavMesh Agent `Move` with collisions against the mesh edge (it warps), no `SamplePathPosition`,
  no `NavMeshData` assets shared between scenes, no Physics Colliders as bake geometry.
- Avoidance ignores nav mesh walls (the mesh constrains the move afterwards, as in the familiar
  engine).
