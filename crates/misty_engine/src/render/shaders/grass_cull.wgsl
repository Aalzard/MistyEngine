// Grass culling on the GPU (grass_cull.rs): before the frame's passes, every blade the
// passes would draw is tested here once instead of in all ten vertices of its card in every
// pass, and the blades that can show are written, in order, into a compact list per (chunk,
// pass) with its count in the draw's indirect arguments. A blade is left out of a pass when
// its vertex stage would put the whole card outside the view (`grass_outside`) or, for the
// passes that skip them, collapse it to a flat fan (`grass_flat`): both mirrored from
// grass_blade.wgsl with a small safety margin, so a blade is only left out when the vertex
// stage would draw nothing of it.
//
// A job is one chunk with the passes ("lanes") that draw it: its blades' records are read
// once for all of them. Three dispatches keep the order: cs_count marks each workgroup's
// visible blades per lane (a 256-bit mask) and counts them, cs_scan turns the counts into
// offsets and writes every lane's draw arguments, cs_write writes the lists from the masks.

const MIN_KEEP: f32 = 0.012;
const GROUP: u32 = 256u;
// Most lanes a job can have (grass_cull.rs `KINDS`).
const LANES: u32 = 9u;
// Relative margin on every left-out test (the vertex stage's float math may differ by ulps).
const MARGIN: f32 = 1e-3;

// One pass's view of the grass (the fields of its GrassParams the tests read).
struct Kind {
    view_proj: mat4x4<f32>,
    // xyz = terrain corner in the world, w = 1 when the near plane counts (camera passes).
    origin: vec4<f32>,
    // xyz = camera (LOD distances), w = full-density distance.
    camera: vec4<f32>,
    // x = draw distance, y = 1 = skip flat blades too, z = the quality's share of the
    // blades, w = angle of one pixel.
    lod: vec4<f32>,
    // x = caster range, y = caster share, z = 1 in shadow passes, w = the near grass shadow
    // map's radius (cascade casters leave its inside to it).
    shadow: vec4<f32>,
};

// Blades `base .. base + count` of the arena from workgroup `first_wg` on, drawn by lanes
// `first_lane .. first_lane + lanes`.
struct Job {
    base: u32,
    count: u32,
    first_wg: u32,
    first_lane: u32,
    lanes: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
};

// A pass drawing a job's chunk: its kind, the blades of the chunk's prefix it draws, its
// draw arguments and x = vertices, y = first vertex of the card.
struct Lane {
    kind: u32,
    count: u32,
    args: u32,
    pad: u32,
    card: vec2<u32>,
    pad1: vec2<u32>,
};

struct Frame {
    // x = jobs, y = workgroups, z = lanes per workgroup in `offsets` (the most of any job).
    counts: vec4<u32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<uniform> kinds: array<Kind, LANES>;
@group(0) @binding(2) var<storage, read> jobs: array<Job>;
// Per blade: xyz = root (terrain-local, f32 bits), w = height in mm (rounded up) << 16 |
// drop-out key × 65535 (rounded down).
@group(0) @binding(3) var<storage, read> records: array<vec4<u32>>;
// Lane-major, `counts.z` rows of one entry per workgroup: the visible blades of that lane in
// that workgroup, then (cs_scan) where they start; one more entry at the end.
@group(0) @binding(4) var<storage, read_write> offsets: array<u32>;
// Per drawn blade: its index in the chunk.
@group(0) @binding(5) var<storage, read_write> indices: array<u32>;
// DrawIndirectArgs per lane (vertex count, instance count, first vertex, first instance).
@group(0) @binding(6) var<storage, read_write> args: array<u32>;
@group(0) @binding(7) var<storage, read> lanes: array<Lane>;
// Job of each workgroup.
@group(0) @binding(8) var<storage, read> wg_job: array<u32>;
// Per workgroup and lane: which of its 256 blades show (8 words).
@group(0) @binding(9) var<storage, read_write> masks: array<u32>;

var<workgroup> lane_count: array<atomic<u32>, LANES>;
var<workgroup> lane_mask: array<atomic<u32>, 72>;
var<workgroup> scan: array<u32, GROUP>;

// The vertex stage of a pass of kind `k` draws something of the blade at `root` (world).
fn visible(k: Kind, root: vec3<f32>, height: f32, key: f32) -> bool {
    let dist = distance(root, k.camera.xyz);
    // grass_outside: past a side plane (or in front of the near plane) by more than the card
    // can reach.
    let reach = (height * 1.6 + 0.25 + dist * k.lod.w * 2.0) * (1.0 + MARGIN) + 1e-3;
    let m = k.view_proj;
    let p = vec4<f32>(root, 1.0);
    let r0 = vec4<f32>(m[0].x, m[1].x, m[2].x, m[3].x);
    let r1 = vec4<f32>(m[0].y, m[1].y, m[2].y, m[3].y);
    let r2 = vec4<f32>(m[0].z, m[1].z, m[2].z, m[3].z);
    let r3 = vec4<f32>(m[0].w, m[1].w, m[2].w, m[3].w);
    let planes = array<vec4<f32>, 5>(r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2);
    let count = select(4u, 5u, k.origin.w > 0.5);
    for (var n = 0u; n < count; n++) {
        let pl = planes[n];
        if (dot(pl, p) < -reach * length(pl.xyz)) {
            return false;
        }
    }
    if (k.lod.y < 0.5) {
        return true;
    }
    // grass_flat (`grass_drawn_height` 0): beyond the draw distance, past its key's share,
    // outside a caster range or inside the near map's square.
    if (height <= 0.0 || dist >= k.lod.x * (1.0 + MARGIN)) {
        return false;
    }
    let ratio = k.camera.w / max(dist, 1e-3);
    let shadow = k.shadow.z > 0.5;
    let keep = clamp(ratio * ratio, MIN_KEEP, 1.0) * k.lod.z * select(1.0, k.shadow.y, shadow);
    if (key >= keep * (1.0 + MARGIN)) {
        return false;
    }
    if (shadow) {
        if (dist >= k.shadow.x * (1.0 + MARGIN)) {
            return false;
        }
        if (k.shadow.w > 0.0 && dist <= k.shadow.w * 0.7 * (1.0 - MARGIN)) {
            return false;
        }
    }
    return true;
}

fn workgroup_index(wg: vec3<u32>) -> u32 {
    return wg.x + wg.y * 65535u;
}

@compute @workgroup_size(256)
fn cs_count(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let w = workgroup_index(wg);
    let total = frame.counts.y;
    if (w >= total) {
        return;
    }
    if (lid < LANES) {
        atomicStore(&lane_count[lid], 0u);
    }
    if (lid < LANES * 8u) {
        atomicStore(&lane_mask[lid], 0u);
    }
    workgroupBarrier();
    let j = jobs[wg_job[w]];
    let i = (w - j.first_wg) * GROUP + lid;
    if (i < j.count) {
        let r = records[j.base + i];
        let local = bitcast<vec3<f32>>(r.xyz);
        let height = f32(r.w >> 16u) * 0.001;
        let key = f32(r.w & 0xffffu) / 65535.0;
        for (var l = 0u; l < j.lanes; l++) {
            let lane = lanes[j.first_lane + l];
            let k = kinds[lane.kind];
            if (i < lane.count && visible(k, k.origin.xyz + local, height, key)) {
                atomicAdd(&lane_count[l], 1u);
                atomicOr(&lane_mask[l * 8u + lid / 32u], 1u << (lid % 32u));
            }
        }
    }
    workgroupBarrier();
    let rows = frame.counts.z;
    if (lid < rows) {
        offsets[lid * total + w] = atomicLoad(&lane_count[lid]);
    }
    if (lid < rows * 8u) {
        masks[w * rows * 8u + lid] = atomicLoad(&lane_mask[lid]);
    }
}

// One workgroup: exclusive prefix sum of all the counts (each thread a run of them), then
// every lane's draw arguments.
@compute @workgroup_size(256)
fn cs_scan(@builtin(local_invocation_index) lid: u32) {
    let total = frame.counts.y * frame.counts.z;
    let per = (total + GROUP - 1u) / GROUP;
    let start = min(lid * per, total);
    let end = min(start + per, total);
    var sum = 0u;
    for (var i = start; i < end; i++) {
        sum += offsets[i];
    }
    scan[lid] = sum;
    workgroupBarrier();
    // Inclusive scan of the run sums (Hillis-Steele).
    for (var s = 1u; s < GROUP; s = s * 2u) {
        var add = 0u;
        if (lid >= s) {
            add = scan[lid - s];
        }
        workgroupBarrier();
        scan[lid] += add;
        workgroupBarrier();
    }
    var at = scan[lid] - sum;
    for (var i = start; i < end; i++) {
        let n = offsets[i];
        offsets[i] = at;
        at += n;
    }
    if (lid == GROUP - 1u) {
        offsets[total] = scan[GROUP - 1u];
    }
    storageBarrier();
    workgroupBarrier();
    let wgs_total = frame.counts.y;
    for (var j = lid; j < frame.counts.x; j += GROUP) {
        let job = jobs[j];
        let wgs = (job.count + GROUP - 1u) / GROUP;
        for (var l = 0u; l < job.lanes; l++) {
            let lane = lanes[job.first_lane + l];
            let row = l * wgs_total;
            let first = offsets[row + job.first_wg];
            args[lane.args * 4u] = lane.card.x;
            args[lane.args * 4u + 1u] = offsets[row + job.first_wg + wgs] - first;
            args[lane.args * 4u + 2u] = lane.card.y;
            args[lane.args * 4u + 3u] = first;
        }
    }
}

@compute @workgroup_size(256)
fn cs_write(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let w = workgroup_index(wg);
    let total = frame.counts.y;
    if (w >= total) {
        return;
    }
    let j = jobs[wg_job[w]];
    let i = (w - j.first_wg) * GROUP + lid;
    let rows = frame.counts.z;
    let word = lid / 32u;
    let below = (1u << (lid % 32u)) - 1u;
    for (var l = 0u; l < j.lanes; l++) {
        let at = w * rows * 8u + l * 8u;
        let bits = masks[at + word];
        if (((bits >> (lid % 32u)) & 1u) == 0u) {
            continue;
        }
        var n = countOneBits(bits & below);
        for (var k = 0u; k < word; k++) {
            n += countOneBits(masks[at + k]);
        }
        indices[offsets[l * total + w] + n] = i;
    }
}
