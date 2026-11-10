// Terrain tree culling on the GPU (render/trees_cull.rs). Every placed tree of a terrain lives
// in a persistent buffer (uploaded when the terrain's trees change); one thread per tree tests
// it against every view of the frame (the camera, then the sun's cascades): the camera picks the
// level of detail by distance (full mesh, lighter mesh, impostor), each cascade uses its own
// level (by its texel size). A visible tree is appended to the list of its (view, prototype,
// level), whose count lives in the list's indirect draw arguments; the passes then draw every
// list with one indirect draw (bark and leaves share a mesh list). The CPU no longer touches
// single trees per frame, whatever their number.

struct Params {
    // xyz = camera position, w = number of trees.
    camera: vec4<f32>,
    // x = end of the full mesh, y = start of the impostors, z = draw distance (metres, LOD
    // bias applied), w = reach of the shadow cascades (metres).
    lod: vec4<f32>,
    // x = views (1 + cascades), y = prototypes, z = lists.
    counts: vec4<u32>,
};

struct Tree {
    // xyz = foot (world), w = turn (radians).
    foot: vec4<f32>,
    // x = height scale, y = width scale, z = colour variation, w = prototype.
    scale: vec4<f32>,
};

struct Proto {
    // x = height of the bounding sphere's centre (unscaled), y = its radius (unscaled),
    // z = 1 when the prototype draws, w = its distance factor (small plants < 1).
    bounds: vec4<f32>,
};

struct View {
    planes: array<vec4<f32>, 6>,
    // x = level of detail of a cascade (0..2; the camera: -1 = by distance), y = a cascade's
    // texel size (metres).
    misc: vec4<f32>,
};

struct List {
    // Where the list's instances start, how many fit, and its first u32 in `args`
    // (two indexed draws of five u32: bark and leaves; an impostor list uses the first
    // record as a plain draw).
    base: u32,
    capacity: u32,
    args: u32,
    mesh: u32,
};

struct Instance {
    foot: vec4<f32>,
    scale: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> trees: array<Tree>;
@group(0) @binding(2) var<storage, read> protos: array<Proto>;
@group(0) @binding(3) var<storage, read> views: array<View>;
@group(0) @binding(4) var<storage, read> lists: array<List>;
@group(0) @binding(5) var<storage, read_write> instances: array<Instance>;
@group(0) @binding(6) var<storage, read_write> args: array<atomic<u32>>;

// The box `center` ± `half` touches the frustum (render/culling.rs `Frustum::intersects`).
fn intersects(v: u32, center: vec3<f32>, half: f32) -> bool {
    for (var i = 0u; i < 6u; i++) {
        let p = views[v].planes[i];
        let n = p.xyz;
        let reach = half * (abs(n.x) + abs(n.y) + abs(n.z));
        if (dot(n, center) + p.w + reach < -1e-4 * (1.0 + reach)) {
            return false;
        }
    }
    return true;
}

fn append(list: u32, t: Tree) {
    let l = lists[list];
    let slot = atomicAdd(&args[l.args + 1u], 1u);
    if (l.mesh == 1u) {
        atomicAdd(&args[l.args + 6u], 1u);
    }
    if (slot < l.capacity) {
        instances[l.base + slot] = Instance(t.foot, vec4<f32>(t.scale.x, t.scale.y, t.scale.z, 0.0));
    }
}

// Share of a level's distance past which the next level takes over (trees.rs `FADE`): the
// CPU path switches at `edge × (1 + FADE)`.
const FADE: f32 = 0.0001;
// trees.rs `SMALL_PLANT`, `SMALL_TEXELS`.
const SMALL_PLANT: f32 = 2.0;
const SMALL_TEXELS: f32 = 16.0;

@compute @workgroup_size(256)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * 65535u * 256u;
    if (i >= u32(params.camera.w)) {
        return;
    }
    let t = trees[i];
    let proto = u32(t.scale.w);
    if (proto >= params.counts.y) {
        return;
    }
    let b = protos[proto].bounds;
    if (b.z < 0.5) {
        return;
    }
    let scale = max(t.scale.x, t.scale.y);
    let center = t.foot.xyz + vec3<f32>(0.0, b.x * t.scale.x, 0.0);
    let radius = b.y * scale;
    let dist = distance(center, params.camera.xyz);
    // Small plants: nearer levels and draw distance (trees.rs `size_factor`).
    let k = b.w;
    let draw = params.lod.z * k;
    if (dist > draw + radius) {
        return;
    }
    let protos_n = params.counts.y;
    // The camera: level by distance.
    if (intersects(0u, center, radius)) {
        var lod = 3u;
        if (dist < params.lod.x * k * (1.0 + FADE)) {
            lod = 0u;
        } else if (dist < params.lod.y * k * (1.0 + FADE)) {
            lod = 1u;
        } else if (dist < draw * (1.0 - FADE) * (1.0 + FADE)) {
            lod = 2u;
        }
        if (lod < 3u) {
            append(proto * 3u + lod, t);
        }
    }
    // The cascades: their own level, within their reach.
    if (dist < params.lod.w + radius) {
        for (var v = 1u; v < params.counts.x; v++) {
            // A small plant casts only where the cascade's texels resolve it (trees.rs
            // `casts_into`).
            let small = b.y < SMALL_PLANT && views[v].misc.y * SMALL_TEXELS > radius;
            if (!small && intersects(v, center, radius)) {
                let lod = u32(views[v].misc.x);
                append((v * protos_n + proto) * 3u + lod, t);
            }
        }
    }
}

// Counts past a list's capacity are clamped (the draws must not read past the list).
@compute @workgroup_size(64)
fn clamp_counts(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.counts.z) {
        return;
    }
    let l = lists[id.x];
    let n = min(atomicLoad(&args[l.args + 1u]), l.capacity);
    atomicStore(&args[l.args + 1u], n);
    if (l.mesh == 1u) {
        atomicStore(&args[l.args + 6u], n);
    }
}
