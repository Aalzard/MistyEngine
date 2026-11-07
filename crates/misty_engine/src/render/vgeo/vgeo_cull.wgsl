// Misty Virtual Geometry: instance and cluster culling with per-cluster level of detail
// (render/vgeo/mod.rs). Prepended with draw.wgsl (`struct Draw`).
//
// Per view (the main camera's two phases, then each shadow map):
//   cs_instances  one thread per instance: frustum (and Hi-Z in the second phase) test, then
//                 queue the instance's candidate clusters in chunks of 64. Clusters whose
//                 parent can never be coarse enough at this distance are skipped with one
//                 binary search (each mesh's clusters are sorted by parent error).
//   cs_prepare    one thread: write each batch's draw arguments (its region of the visible
//                 list is a fixed share of the view's) and the cluster dispatch.
//   cs_clusters   one workgroup per chunk: LOD test (own error small enough on screen, the
//                 parent's not), frustum and Hi-Z test, append to the batch's visible list
//                 (and, for the indexed draw path, the cluster's own indexed draw arguments).
//   cs_finish     clamp each batch's cluster count to its region.

const ROOT_ERROR: f32 = 3.0e38;
const CHUNK: u32 = 64u;
const VERTICES_PER_CLUSTER: u32 = 384u;
const MAX_GROUPS_X: u32 = 65535u;
// Start of the dispatch arguments in `control` (after 4 counters per view, MAX_VIEWS = 8).
const DISPATCH_BASE: u32 = 32u;

struct View {
    view_proj: mat4x4<f32>,
    // Inside where dot(plane.xyz, p) + plane.w >= 0 (unit normals, culling.rs).
    planes: array<vec4<f32>, 6>,
    // xyz = the camera the level of detail is chosen for (the main camera, also for shadow
    // maps), w = pixels per unit at distance 1 (per unit when orthographic).
    camera: vec4<f32>,
    // x = largest error on screen (pixels), y = 1 when orthographic, zw = target size (pixels).
    params: vec4<f32>,
    // x = instances, y = mode (0 first phase, 1 second phase, 2 shadow map),
    // z = Hi-Z mip count (0 = no occlusion test), w = batches.
    info: vec4<u32>,
    // x = view index, y = this view's first visible-list entry, z = entries per batch,
    // w = its first chunk entry (chunks follow the visible lists in `lists`).
    region: vec4<u32>,
    // x = chunk capacity; where `control` holds z = the limits and w = the draw arguments
    // (y unused).
    caps: vec4<u32>,
};

struct Instance {
    draw: u32,
    mesh: u32,
    batch: u32,
    // Bit 0 = casts shadows.
    flags: u32,
};

struct MeshInfo {
    // Object-space sphere around every cluster's bounds and LOD spheres.
    bounds: vec4<f32>,
    first: u32,
    count: u32,
    pad0: u32,
    pad1: u32,
};

struct Cluster {
    bounds: vec4<f32>,
    lod: vec4<f32>,
    parent: vec4<f32>,
    error: f32,
    parent_error: f32,
    first: u32,
    triangles: u32,
    // The cluster's own vertex list (source vertex ids) and its local indices (indexed path).
    vertex_first: u32,
    local_first: u32,
    pad0: u32,
    pad1: u32,
};

@group(0) @binding(0) var<uniform> vu: View;
@group(0) @binding(1) var<storage, read> draws: array<Draw>;
@group(0) @binding(2) var<storage, read> instances: array<Instance>;
@group(0) @binding(3) var<storage, read> meshes: array<MeshInfo>;
@group(0) @binding(4) var<storage, read> clusters: array<Cluster>;
// Per instance: [2i] = visible at the end of last frame (main camera), [2i + 1] = drawn by
// the first phase this frame.
@group(0) @binding(5) var<storage, read_write> instance_state: array<u32>;
// The visible lists, (draw, cluster) per drawn cluster, every view's, then the chunks,
// (instance, first candidate cluster) each (one buffer: 8 storage buffers per stage).
@group(0) @binding(6) var<storage, read_write> lists: array<vec2<u32>>;
// Counters and arguments, one buffer (the device allows 8 storage buffers per stage):
//   [4 v ..]             per view: chunks queued, triangles drawn, clusters drawn, clusters
//                        dropped (list full)
//   [DISPATCH_BASE + 4 v ..]  per view: the cluster pass's workgroups (x, y, z, unused)
//   [caps.z + v B + b]   per view and batch: entries of its region
//   [caps.w + 4 (v B + b) ..]  per view and batch: DrawIndirectArgs (vertex count, instance
//                        count = clusters, first vertex, first instance = first entry)
@group(0) @binding(7) var<storage, read_write> control: array<atomic<u32>>;
// Per visible-list entry: DrawIndexedIndirectArgs of that cluster (index count, 1, first
// local index, base vertex = entry × 384, first instance = entry), drawn by
// multi_draw_indexed_indirect_count with the batch's cluster count.
@group(0) @binding(8) var<storage, read_write> cluster_args: array<u32>;
@group(0) @binding(9) var hiz: texture_2d<f32>;

fn world_scale(m: mat4x4<f32>) -> f32 {
    return max(length(m[0].xyz), max(length(m[1].xyz), length(m[2].xyz)));
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

// An error of `error` world units measured from the sphere, in pixels. Must match
// `LodView::projected` (vgeo/mod.rs): the same inputs give the same value for a cluster's own
// error and for its children's parent error, so exactly one level covers each spot.
//
// Shadow maps of the sun (mode 2, params.z = shadow-map pixels per world unit) take the
// coarser of the camera's level of detail and one texel of their own: the smaller of two
// projections that both grow towards the roots still grows towards the roots, so the cut
// stays unique and crack-free.
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
fn cs_instances(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= vu.info.x) {
        return;
    }
    let inst = instances[i];
    let mesh = meshes[inst.mesh];
    let model = draws[inst.draw].model;
    let s = world_scale(model);
    let center = (model * vec4<f32>(mesh.bounds.xyz, 1.0)).xyz;
    let radius = mesh.bounds.w * s;
    let inside = sphere_inside(center, radius);
    let mode = vu.info.y;
    if (mode == 0u) {
        let go = inside && instance_state[2u * i] != 0u;
        instance_state[2u * i + 1u] = select(0u, 1u, go);
        if (!go) {
            return;
        }
    } else if (mode == 1u) {
        var seen = inside;
        if (seen && vu.info.z > 0u) {
            seen = !occluded(center - vec3<f32>(radius), center + vec3<f32>(radius));
        }
        instance_state[2u * i] = select(0u, 1u, seen);
        if (!seen || instance_state[2u * i + 1u] != 0u) {
            return;
        }
    } else if (!inside || (inst.flags & 1u) == 0u) {
        return;
    }
    // A cluster can only be drawn if its parent projects to more than the threshold; every
    // parent sphere is inside the mesh's, so it is at least `near` away and clusters whose
    // parent error is at most `skip_below` can be skipped.
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
    var lo = 0u;
    var hi = mesh.count;
    loop {
        if (lo >= hi) {
            break;
        }
        let mid = (lo + hi) / 2u;
        if (clusters[mesh.first + mid].parent_error > skip_below) {
            hi = mid;
        } else {
            lo = mid + 1u;
        }
    }
    let n = mesh.count - lo;
    if (n == 0u) {
        return;
    }
    let k = (n + CHUNK - 1u) / CHUNK;
    let base = atomicAdd(&control[vu.region.x * 4u], k);
    for (var j = 0u; j < k; j++) {
        if (base + j < vu.caps.x) {
            lists[vu.region.w + base + j] = vec2<u32>(i, lo + j * CHUNK);
        }
    }
}

@compute @workgroup_size(1)
fn cs_prepare() {
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
    let queued = min(atomicLoad(&control[vi * 4u]), vu.caps.x);
    let d = DISPATCH_BASE + vi * 4u;
    atomicStore(&control[d], min(queued, MAX_GROUPS_X));
    atomicStore(&control[d + 1u], (queued + MAX_GROUPS_X - 1u) / MAX_GROUPS_X);
    atomicStore(&control[d + 2u], 1u);
}

@compute @workgroup_size(64)
fn cs_clusters(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    let vi = vu.region.x;
    let chunk = group.y * MAX_GROUPS_X + group.x;
    let queued = min(atomicLoad(&control[vi * 4u]), vu.caps.x);
    if (chunk >= queued) {
        return;
    }
    let item = lists[vu.region.w + chunk];
    let inst = instances[item.x];
    let mesh = meshes[inst.mesh];
    let local = item.y + lane;
    if (local >= mesh.count) {
        return;
    }
    let ci = mesh.first + local;
    let c = clusters[ci];
    let model = draws[inst.draw].model;
    let s = world_scale(model);
    let threshold = vu.params.x;
    let own = projected_error((model * vec4<f32>(c.lod.xyz, 1.0)).xyz, c.lod.w * s, c.error * s);
    if (own > threshold) {
        return;
    }
    if (c.parent_error < ROOT_ERROR) {
        let parent = projected_error(
            (model * vec4<f32>(c.parent.xyz, 1.0)).xyz,
            c.parent.w * s,
            c.parent_error * s,
        );
        if (parent <= threshold) {
            return;
        }
    }
    let center = (model * vec4<f32>(c.bounds.xyz, 1.0)).xyz;
    let r = c.bounds.w * s;
    if (!sphere_inside(center, r)) {
        return;
    }
    if (vu.info.y == 1u && vu.info.z > 0u && occluded(center - vec3<f32>(r), center + vec3<f32>(r))) {
        return;
    }
    let at = vi * vu.info.w + inst.batch;
    let a = vu.caps.w + at * 4u;
    let slot = atomicAdd(&control[a + 1u], 1u);
    if (slot >= atomicLoad(&control[vu.caps.z + at])) {
        atomicAdd(&control[vi * 4u + 3u], 1u);
        return;
    }
    let entry = atomicLoad(&control[a + 3u]) + slot;
    lists[entry] = vec2<u32>(inst.draw, ci);
    let k = entry * 5u;
    cluster_args[k] = c.triangles * 3u;
    cluster_args[k + 1u] = 1u;
    cluster_args[k + 2u] = c.local_first;
    cluster_args[k + 3u] = entry * VERTICES_PER_CLUSTER;
    cluster_args[k + 4u] = entry;
    atomicAdd(&control[vi * 4u + 1u], c.triangles);
    atomicAdd(&control[vi * 4u + 2u], 1u);
}

@compute @workgroup_size(64)
fn cs_finish(@builtin(global_invocation_id) id: vec3<u32>) {
    let b = id.x;
    if (b >= vu.info.w) {
        return;
    }
    let at = vu.region.x * vu.info.w + b;
    let a = vu.caps.w + at * 4u + 1u;
    atomicStore(&control[a], min(atomicLoad(&control[a]), atomicLoad(&control[vu.caps.z + at])));
}
