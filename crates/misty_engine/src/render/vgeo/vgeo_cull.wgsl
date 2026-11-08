// Misty Virtual Geometry: instance and cluster culling with per-cluster level of detail
// (render/vgeo/mod.rs). Prepended with draw.wgsl (`struct Draw`).
//
// Views are culled together: one dispatch of each pass covers a run of views (the main camera's
// phases, then every shadow map at once), `range.x` the first and the workgroup id's y (direct
// dispatches) or z (indirect ones) the view within the run. Per view:
//   cs_instances  one thread per instance: frustum (and Hi-Z in the second phase) test, then
//                 queue the instance's groups to decide (chunks of 64) and its candidate
//                 clusters (chunks of 64). Groups and clusters that are surely fine enough
//                 at this distance are skipped with one binary search each (each mesh's groups
//                 are sorted by error, its clusters by parent error).
//   cs_prepare    one thread: write each batch's draw arguments (its region of the visible
//                 list is a fixed share of the view's) and the two indirect dispatches.
//   cs_groups     one workgroup per group chunk: the level-of-detail decision of each group,
//                 made once ("fine enough": its error projects to at most the threshold), as
//                 64 bits written over the chunk's own record.
//   cs_clusters   one workgroup per cluster chunk: drawn when its own group is fine enough and
//                 its parent group is not (both read from cs_groups' bits: the two sides of
//                 every group border can't disagree, so no cracks whatever the rounding), then
//                 back-face, frustum and Hi-Z tests, append to its material slot's batch.
//   cs_finish     clamp each batch's cluster count to its region.

const ROOT_ERROR: f32 = 3.0e38;
const NO_GROUP: u32 = 0xffffffffu;
const NO_DRAW: u32 = 0xffffffffu;
const CHUNK: u32 = 64u;
const VERTICES_PER_CLUSTER: u32 = 384u;
const MAX_GROUPS_X: u32 = 65535u;
// Start of the dispatch arguments in `control` (after 8 counters per view, MAX_VIEWS = 34).
const DISPATCH_BASE: u32 = 272u;

struct View {
    view_proj: mat4x4<f32>,
    // Inside where dot(plane.xyz, p) + plane.w >= 0 (unit normals, culling.rs).
    planes: array<vec4<f32>, 6>,
    // xyz = the camera the level of detail is chosen for (the main camera, also for shadow
    // maps), w = pixels per unit at distance 1 (per unit when orthographic).
    camera: vec4<f32>,
    // x = largest error on screen (pixels), y = 1 when orthographic, zw = target size (pixels).
    params: vec4<f32>,
    // x = instances, y = mode (0 first phase, 1 second phase, 2 shadow map, 3 the only phase),
    // z = Hi-Z mip count (0 = no occlusion test), w = batches.
    info: vec4<u32>,
    // x = view index, y = this view's first visible-list entry, z = entries per batch,
    // w = its first cluster chunk entry in `lists`.
    region: vec4<u32>,
    // x = cluster chunk capacity, y = instances `instance_state` is laid out for; where
    // `control` holds z = the limits and w = the draw arguments.
    caps: vec4<u32>,
    // This view's eye (w = 1) or, orthographic, its forward direction (w = 0): back faces.
    eye: vec4<f32>,
    // x = its first group chunk entry in `lists`, y = group chunk capacity.
    groups: vec4<u32>,
};

// `instances` holds the instances, then their material slot records:
//   instance  x = the draw whose model matrix places it, y = its mesh record, z = its first
//             slot record (`instances[z + material]`), w = bit 0: some slot casts shadows
//   slot      x = its draw (material, per-object values; NO_DRAW = the object doesn't draw
//             that part), y = batch, z = bit 0: casts shadows

// Mesh records, then group records (the pool's record section, render/vgeo/mod.rs).
//   mesh:  a = object-space sphere around every cluster, b = (first cluster, clusters,
//          first group record, groups)
//   group: a = sphere its error is measured from, b.x = error (f32 bits)
struct Record {
    a: vec4<f32>,
    b: vec4<u32>,
};

struct Cluster {
    bounds: vec4<f32>,
    // Mesh-local groups: the one that made it and the one it was simplified into.
    group: u32,
    parent_group: u32,
    // The parent group's error (sorted on: the binary search).
    parent_error: f32,
    // Triangles (8 bits) | level << 8 (6 bits) | material slot << 14.
    packed: u32,
    // Its own vertex list (source vertex ids) and its local indices (indexed path).
    vertex_first: u32,
    local_first: u32,
    // Normal cone: octahedral axis (2 × snorm16) and cutoff (> 1 = never back-facing).
    cone_axis: u32,
    cone_cutoff: f32,
};

// x = the first view of this dispatch.
@group(0) @binding(0) var<uniform> range: vec4<u32>;
// Every view (MAX_VIEWS).
@group(0) @binding(10) var<uniform> views: array<View, 34>;
// The view this workgroup culls for (set first thing by every entry point).
var<private> vu: View;
@group(0) @binding(1) var<storage, read> draws: array<Draw>;
@group(0) @binding(2) var<storage, read> instances: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read> records: array<Record>;
@group(0) @binding(4) var<storage, read> clusters: array<Cluster>;
// Per instance: [4i] = visible at the end of last frame (main camera), [4i + 1] = drawn by
// the first phase this frame; then per view and instance (`view_state`, views run at the same
// time): its first group chunk in the view and its first decided group (the ones below are
// fine enough).
@group(0) @binding(5) var<storage, read_write> instance_state: array<u32>;
// The visible lists, (draw, cluster) per drawn cluster, every view's; then the cluster chunks,
// (instance, first candidate cluster) each; then the group chunks, (instance, first group)
// each until cs_groups replaces it by its 64 decisions (one buffer: 8 storage buffers per
// stage).
@group(0) @binding(6) var<storage, read_write> lists: array<vec2<u32>>;
// Counters and arguments, one buffer (the device allows 8 storage buffers per stage):
//   [8 v ..]             per view: cluster chunks queued, triangles drawn, clusters drawn,
//                        clusters dropped (list full), group chunks queued, instances dropped
//                        (group chunks full), instances queued, unused
//   [DISPATCH_BASE + 8 v ..]  per view: the cluster pass's workgroups (x, y, z, unused), the
//                        group pass's (x, y, z, unused)
//   [caps.z + v B + b]   per view and batch: entries of its region
//   [caps.w + 4 (v B + b) ..]  per view and batch: DrawIndirectArgs (vertex count, instance
//                        count = clusters, first vertex, first instance = first entry)
@group(0) @binding(7) var<storage, read_write> control: array<atomic<u32>>;
// Per visible-list entry: DrawIndexedIndirectArgs of that cluster (index count, 1, first
// local index, base vertex = entry × 384, first instance = entry), drawn by
// multi_draw_indexed_indirect_count with the batch's cluster count.
@group(0) @binding(8) var<storage, read_write> cluster_args: array<u32>;
@group(0) @binding(9) var hiz: texture_2d<f32>;

// Where instance `i`'s two words for this view are in `instance_state` (caps.y = the
// instances the buffer is laid out for).
fn view_state(i: u32) -> u32 {
    return 4u * vu.caps.y + 2u * (vu.region.x * vu.caps.y + i);
}

fn world_scale(m: mat4x4<f32>) -> f32 {
    return max(length(m[0].xyz), max(length(m[1].xyz), length(m[2].xyz)));
}

// As vg_octahedral (vgeo_fetch.wgsl).
fn octahedral(w: u32) -> vec3<f32> {
    let e = unpack2x16snorm(w);
    var n = vec3<f32>(e.x, e.y, 1.0 - abs(e.x) - abs(e.y));
    let t = max(-n.z, 0.0);
    n.x += select(t, -t, n.x >= 0.0);
    n.y += select(t, -t, n.y >= 0.0);
    return normalize(n);
}

// Does every triangle of the cluster face away from this view (vgeo::Cluster::backfacing)?
// Object to world must keep angles (uniform scale) and winding (no mirror); otherwise never.
fn backfacing(c: Cluster, model: mat4x4<f32>, center: vec3<f32>, radius: f32) -> bool {
    if (c.cone_cutoff > 1.0) {
        return false;
    }
    let lx = length(model[0].xyz);
    let ly = length(model[1].xyz);
    let lz = length(model[2].xyz);
    let mirrored = dot(cross(model[0].xyz, model[1].xyz), model[2].xyz) < 0.0;
    if (mirrored || max(lx, max(ly, lz)) > 1.001 * min(lx, min(ly, lz))) {
        return false;
    }
    let axis = normalize((model * vec4<f32>(octahedral(c.cone_axis), 0.0)).xyz);
    if (vu.eye.w < 0.5) {
        return dot(vu.eye.xyz, axis) >= c.cone_cutoff;
    }
    let to = center - vu.eye.xyz;
    return dot(to, axis) >= c.cone_cutoff * length(to) + radius;
}

fn sphere_inside(c: vec3<f32>, r: f32) -> bool {
    for (var i = 0u; i < 6u; i++) {
        let p = vu.planes[i];
        if (dot(p.xyz, c) + p.w < -r - 1e-4 * (1.0 + r)) {
            return false;
        }
    }
    return true;
}

// An error of `error` world units measured from the sphere, in pixels (`LodView::projected`,
// vgeo/mod.rs). Only cs_groups calls it: each group's decision is made once per view.
//
// Shadow maps of the sun (mode 2, params.z = shadow-map pixels per world unit) take the
// coarser of the camera's level of detail and one texel of their own: the smaller of two
// projections that both grow towards the roots still grows towards the roots, so the cut
// stays coherent.
fn projected_error(center: vec3<f32>, radius: f32, error: f32) -> f32 {
    var p = 0.0;
    if (vu.params.y > 0.5) {
        p = error * vu.camera.w;
    } else {
        let d = length(center - vu.camera.xyz) - radius;
        p = error * vu.camera.w / max(d, 1e-4);
    }
    if (vu.info.y == 2u && vu.params.z > 0.0) {
        p = min(p, error * vu.params.z);
    }
    return p;
}

// Hidden behind the depth drawn by the first phase? Conservative: the box's nearest depth
// against the farthest depth of the pyramid texels covering its screen rectangle (the same
// test as gpu_culling/cull.wgsl).
fn occluded(lo_box: vec3<f32>, hi_box: vec3<f32>) -> bool {
    var lo = vec2<f32>(1e9);
    var hi = vec2<f32>(-1e9);
    var z = 1e9;
    for (var i = 0u; i < 8u; i++) {
        let corner = vec3<f32>(
            select(lo_box.x, hi_box.x, (i & 1u) != 0u),
            select(lo_box.y, hi_box.y, (i & 2u) != 0u),
            select(lo_box.z, hi_box.z, (i & 4u) != 0u),
        );
        let c = vu.view_proj * vec4<f32>(corner, 1.0);
        if (c.w <= 1e-5) {
            return false;
        }
        let ndc = c.xyz / c.w;
        lo = min(lo, ndc.xy);
        hi = max(hi, ndc.xy);
        z = min(z, ndc.z);
    }
    if (z <= 0.0) {
        return false;
    }
    let full = vu.params.zw;
    let p0 = clamp(vec2<f32>(lo.x * 0.5 + 0.5, 0.5 - hi.y * 0.5) * full, vec2<f32>(0.0), full - 1.0);
    let p1 = clamp(vec2<f32>(hi.x * 0.5 + 0.5, 0.5 - lo.y * 0.5) * full, vec2<f32>(0.0), full - 1.0);
    let t0 = vec2<u32>(p0) / 2u;
    let t1 = vec2<u32>(p1) / 2u;
    var level = 0u;
    let mips = vu.info.z;
    loop {
        let a = t0 >> vec2<u32>(level);
        let c = t1 >> vec2<u32>(level);
        if ((c.x - a.x <= 1u && c.y - a.y <= 1u) || level + 1u >= mips) {
            break;
        }
        level++;
    }
    let dims = textureDimensions(hiz, level) - vec2<u32>(1u);
    let a = min(t0 >> vec2<u32>(level), dims);
    let c = min(t1 >> vec2<u32>(level), dims);
    if (c.x - a.x > 1u || c.y - a.y > 1u) {
        return false;
    }
    let far = max(
        max(textureLoad(hiz, a, i32(level)).r, textureLoad(hiz, vec2<u32>(c.x, a.y), i32(level)).r),
        max(textureLoad(hiz, vec2<u32>(a.x, c.y), i32(level)).r, textureLoad(hiz, c, i32(level)).r),
    );
    return z > far;
}

@compute @workgroup_size(64)
fn cs_instances(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(workgroup_id) wg: vec3<u32>,
) {
    vu = views[range.x + wg.y];
    let i = id.x;
    if (i >= vu.info.x) {
        return;
    }
    let vi = vu.region.x;
    let inst = instances[i];
    let mesh = records[inst.y];
    let model = draws[inst.x].model;
    let s = world_scale(model);
    let center = (model * vec4<f32>(mesh.a.xyz, 1.0)).xyz;
    let radius = mesh.a.w * s;
    let inside = sphere_inside(center, radius);
    let mode = vu.info.y;
    if (mode == 0u) {
        let go = inside && instance_state[4u * i] != 0u;
        instance_state[4u * i + 1u] = select(0u, 1u, go);
        if (!go) {
            return;
        }
    } else if (mode == 1u) {
        var seen = inside;
        if (seen && vu.info.z > 0u) {
            seen = !occluded(center - vec3<f32>(radius), center + vec3<f32>(radius));
        }
        instance_state[4u * i] = select(0u, 1u, seen);
        if (!seen || instance_state[4u * i + 1u] != 0u) {
            return;
        }
    } else if (mode == 3u) {
        // The only main phase (few instances: no Hi-Z): every instance in the frustum.
        instance_state[4u * i] = select(0u, 1u, inside);
        instance_state[4u * i + 1u] = select(0u, 1u, inside);
        if (!inside) {
            return;
        }
    } else if (!inside || (inst.w & 1u) == 0u) {
        return;
    }
    // A cluster can only be drawn if its parent group is not fine enough; every group sphere
    // is inside the mesh's, so it is at least `near` away, and groups (and clusters whose
    // parent group) with an error of at most `skip_below` are surely fine enough.
    let px = vu.camera.w * s;
    var skip_below = vu.params.x / px;
    if (vu.params.y < 0.5) {
        let near = max(length(center - vu.camera.xyz) - radius, 1e-4);
        skip_below = vu.params.x * near / px;
    }
    if (mode == 2u && vu.params.z > 0.0) {
        // The texel projection only ever lowers the error: skip at least as far.
        skip_below = max(skip_below, vu.params.x / (vu.params.z * s));
    }
    skip_below *= 0.999;
    // Candidate clusters: parent error above `skip_below`.
    var lo = 0u;
    var hi = mesh.b.y;
    loop {
        if (lo >= hi) {
            break;
        }
        let mid = (lo + hi) / 2u;
        if (clusters[mesh.b.x + mid].parent_error > skip_below) {
            hi = mid;
        } else {
            lo = mid + 1u;
        }
    }
    let n = mesh.b.y - lo;
    if (n == 0u) {
        return;
    }
    // Groups to decide: error above `skip_below` (the same comparison on the same values as
    // the clusters' parent errors, so a skipped cluster's parent group is an undecided one).
    var glo = 0u;
    var ghi = mesh.b.w;
    loop {
        if (glo >= ghi) {
            break;
        }
        let mid = (glo + ghi) / 2u;
        if (bitcast<f32>(records[mesh.b.z + mid].b.x) > skip_below) {
            ghi = mid;
        } else {
            glo = mid + 1u;
        }
    }
    let kg = (mesh.b.w - glo + CHUNK - 1u) / CHUNK;
    var gbase = 0u;
    if (kg > 0u) {
        gbase = atomicAdd(&control[vi * 8u + 4u], kg);
        if (gbase + kg > vu.groups.y) {
            // No room to decide its groups: not drawn in this view (counted).
            atomicAdd(&control[vi * 8u + 5u], 1u);
            return;
        }
        for (var j = 0u; j < kg; j++) {
            lists[vu.groups.x + gbase + j] = vec2<u32>(i, glo + j * CHUNK);
        }
    }
    instance_state[view_state(i)] = gbase;
    instance_state[view_state(i) + 1u] = glo;
    atomicAdd(&control[vi * 8u + 6u], 1u);
    let k = (n + CHUNK - 1u) / CHUNK;
    let base = atomicAdd(&control[vi * 8u], k);
    for (var j = 0u; j < k; j++) {
        if (base + j < vu.caps.x) {
            lists[vu.region.w + base + j] = vec2<u32>(i, lo + j * CHUNK);
        }
    }
}

@compute @workgroup_size(1)
fn cs_prepare(
    @builtin(workgroup_id) wg: vec3<u32>,
    @builtin(num_workgroups) runs: vec3<u32>,
) {
    vu = views[range.x + wg.x];
    let vi = vu.region.x;
    let batches = vu.info.w;
    let per_batch = vu.region.z;
    for (var b = 0u; b < batches; b++) {
        let at = vi * batches + b;
        atomicStore(&control[vu.caps.z + at], per_batch);
        let a = vu.caps.w + at * 4u;
        atomicStore(&control[a], VERTICES_PER_CLUSTER);
        atomicStore(&control[a + 1u], 0u);
        atomicStore(&control[a + 2u], 0u);
        atomicStore(&control[a + 3u], vu.region.y + b * per_batch);
    }
    // One dispatch of the cluster and group passes for the whole run (in the first view's
    // words, zeroed every frame): as many workgroups as the busiest view needs, z = the views;
    // the others' extra workgroups return at once.
    let d = DISPATCH_BASE + range.x * 8u;
    let queued = min(atomicLoad(&control[vi * 8u]), vu.caps.x);
    atomicMax(&control[d], min(queued, MAX_GROUPS_X));
    atomicMax(&control[d + 1u], (queued + MAX_GROUPS_X - 1u) / MAX_GROUPS_X);
    atomicStore(&control[d + 2u], runs.x);
    let groups = min(atomicLoad(&control[vi * 8u + 4u]), vu.groups.y);
    atomicMax(&control[d + 4u], min(groups, MAX_GROUPS_X));
    atomicMax(&control[d + 5u], (groups + MAX_GROUPS_X - 1u) / MAX_GROUPS_X);
    atomicStore(&control[d + 6u], runs.x);
}

var<workgroup> wg_bits: array<atomic<u32>, 2>;
var<workgroup> wg_queued: u32;

@compute @workgroup_size(64)
fn cs_groups(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    vu = views[range.x + group.z];
    let vi = vu.region.x;
    if (lane == 0u) {
        atomicStore(&wg_bits[0], 0u);
        atomicStore(&wg_bits[1], 0u);
        wg_queued = min(atomicLoad(&control[vi * 8u + 4u]), vu.groups.y);
    }
    let queued = workgroupUniformLoad(&wg_queued);
    let chunk = group.y * MAX_GROUPS_X + group.x;
    if (chunk >= queued) {
        return;
    }
    let entry = vu.groups.x + chunk;
    let item = lists[entry];
    var fine = false;
    if (item.x < vu.info.x) {
        let inst = instances[item.x];
        let mesh = records[inst.y];
        let g = item.y + lane;
        if (g < mesh.b.w) {
            let model = draws[inst.x].model;
            let s = world_scale(model);
            let r = records[mesh.b.z + g];
            let center = (model * vec4<f32>(r.a.xyz, 1.0)).xyz;
            fine = projected_error(center, r.a.w * s, bitcast<f32>(r.b.x) * s) <= vu.params.x;
        }
    }
    if (fine) {
        atomicOr(&wg_bits[lane >> 5u], 1u << (lane & 31u));
    }
    workgroupBarrier();
    if (lane == 0u) {
        lists[entry] = vec2<u32>(atomicLoad(&wg_bits[0]), atomicLoad(&wg_bits[1]));
    }
}

// The decision cs_groups made for mesh-local group `g` of an instance (its first chunk and
// first decided group); `none` for no group (true for the source clusters' own group, false
// for the roots' parent).
fn group_fine(g: u32, first_chunk: u32, first_group: u32, none: bool) -> bool {
    if (g == NO_GROUP) {
        return none;
    }
    if (g < first_group) {
        return true;
    }
    let k = g - first_group;
    let bits = lists[vu.groups.x + first_chunk + (k >> 6u)];
    let word = select(bits.x, bits.y, (k & 63u) >= 32u);
    return ((word >> (k & 31u)) & 1u) != 0u;
}

@compute @workgroup_size(64)
fn cs_clusters(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    vu = views[range.x + group.z];
    let vi = vu.region.x;
    let chunk = group.y * MAX_GROUPS_X + group.x;
    let queued = min(atomicLoad(&control[vi * 8u]), vu.caps.x);
    if (chunk >= queued) {
        return;
    }
    let item = lists[vu.region.w + chunk];
    let inst = instances[item.x];
    let mesh = records[inst.y];
    let local = item.y + lane;
    if (local >= mesh.b.y) {
        return;
    }
    let ci = mesh.b.x + local;
    let c = clusters[ci];
    let first_chunk = instance_state[view_state(item.x)];
    let first_group = instance_state[view_state(item.x) + 1u];
    if (!group_fine(c.group, first_chunk, first_group, true)
        || group_fine(c.parent_group, first_chunk, first_group, false)) {
        return;
    }
    // Its material slot: which draw (material, per-object values) and batch take it.
    let slot = instances[inst.z + (c.packed >> 14u)];
    if (slot.x == NO_DRAW || (vu.info.y == 2u && (slot.z & 1u) == 0u)) {
        return;
    }
    let model = draws[inst.x].model;
    let s = world_scale(model);
    let center = (model * vec4<f32>(c.bounds.xyz, 1.0)).xyz;
    let r = c.bounds.w * s;
    if (!sphere_inside(center, r) || backfacing(c, model, center, r)) {
        return;
    }
    if (vu.info.y == 1u && vu.info.z > 0u && occluded(center - vec3<f32>(r), center + vec3<f32>(r))) {
        return;
    }
    let at = vi * vu.info.w + slot.y;
    let a = vu.caps.w + at * 4u;
    let at_slot = atomicAdd(&control[a + 1u], 1u);
    if (at_slot >= atomicLoad(&control[vu.caps.z + at])) {
        atomicAdd(&control[vi * 8u + 3u], 1u);
        return;
    }
    let triangles = c.packed & 255u;
    let entry = atomicLoad(&control[a + 3u]) + at_slot;
    lists[entry] = vec2<u32>(slot.x, ci);
    let k = entry * 5u;
    cluster_args[k] = triangles * 3u;
    cluster_args[k + 1u] = 1u;
    cluster_args[k + 2u] = c.local_first;
    cluster_args[k + 3u] = entry * VERTICES_PER_CLUSTER;
    cluster_args[k + 4u] = entry;
    atomicAdd(&control[vi * 8u + 1u], triangles);
    atomicAdd(&control[vi * 8u + 2u], 1u);
}

@compute @workgroup_size(64)
fn cs_finish(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(workgroup_id) wg: vec3<u32>,
) {
    vu = views[range.x + wg.y];
    let b = id.x;
    if (b >= vu.info.w) {
        return;
    }
    let at = vu.region.x * vu.info.w + b;
    let a = vu.caps.w + at * 4u;
    atomicStore(&control[a + 1u], min(atomicLoad(&control[a + 1u]), atomicLoad(&control[vu.caps.z + at])));
}
