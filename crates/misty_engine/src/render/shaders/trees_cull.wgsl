// Terrain tree culling on the GPU (render/trees_cull.rs). Every placed tree of a terrain lives
// in a persistent buffer (uploaded when the terrain's trees change); one thread per tree tests
// it against every view of the frame (the camera, then the sun's cascades): the camera picks the
// level of detail by distance (full mesh, lighter mesh, impostor; inside a cross-fade band a
// tree goes into both levels' lists with complementary dither values), each cascade uses its own
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
    // x = Hi-Z mip levels (0 = no occlusion test), yz = the target's size (pixels), w = the LOD
    // cross-fade band (a share of each switch distance; 0 = instant).
    occlusion: vec4<f32>,
    // The view-projection the Hi-Z pyramid's depth was drawn with (last frame's).
    hiz_view_proj: mat4x4<f32>,
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
// Last frame's depth, farthest per texel (gpu_culling/hiz.wgsl; level 0 = 2 × 2 pixels).
@group(1) @binding(0) var hiz: texture_2d<f32>;

// Hidden behind last frame's depth? Conservative (gpu_culling/cull.wgsl `occluded`): the
// sphere's box's nearest depth against the farthest depth of the pyramid texels covering its
// screen rectangle.
fn occluded(center: vec3<f32>, radius: f32) -> bool {
    let mips = u32(params.occlusion.x);
    if (mips == 0u) {
        return false;
    }
    var lo = vec2<f32>(1e9);
    var hi = vec2<f32>(-1e9);
    var z = 1e9;
    for (var i = 0u; i < 8u; i++) {
        let corner = center + radius * vec3<f32>(
            select(-1.0, 1.0, (i & 1u) != 0u),
            select(-1.0, 1.0, (i & 2u) != 0u),
            select(-1.0, 1.0, (i & 4u) != 0u),
        );
        let c = params.hiz_view_proj * vec4<f32>(corner, 1.0);
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
    let full = params.occlusion.yz;
    let p0 = clamp(vec2<f32>(lo.x * 0.5 + 0.5, 0.5 - hi.y * 0.5) * full, vec2<f32>(0.0), full - 1.0);
    let p1 = clamp(vec2<f32>(hi.x * 0.5 + 0.5, 0.5 - lo.y * 0.5) * full, vec2<f32>(0.0), full - 1.0);
    let t0 = vec2<u32>(p0) / 2u;
    let t1 = vec2<u32>(p1) / 2u;
    var level = 0u;
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

fn append(list: u32, t: Tree, fade: f32) {
    let l = lists[list];
    let slot = atomicAdd(&args[l.args + 1u], 1u);
    if (l.mesh == 1u) {
        atomicAdd(&args[l.args + 6u], 1u);
    }
    if (slot < l.capacity) {
        instances[l.base + slot] = Instance(t.foot, vec4<f32>(t.scale.x, t.scale.y, t.scale.z, fade));
    }
}

// Share (0..1) of the way to the next level at distance `d` for a switch at `edge`, with a
// cross-fade band of `width` × `edge` just beyond it (render/lod.rs `distance_fade`).
fn fade_at(d: f32, edge: f32, width: f32) -> f32 {
    let band = max(width, 0.0) * edge;
    if (band <= 0.0) {
        return select(0.0, 1.0, d >= edge);
    }
    return clamp((d - edge) / band, 0.0, 1.0);
}

// A tree at level `level` (fade 0), or inside a band the outgoing level with its share and
// the incoming one with the complementary dither (trees.rs `levels_at`; `next` 3 = none).
fn append_pair(proto: u32, level: u32, next: u32, x: f32, t: Tree) {
    if (x <= 0.0) {
        append(proto * 3u + level, t, 0.0);
        return;
    }
    append(proto * 3u + level, t, clamp(1.0 - x, 0.001, 0.999));
    if (next < 3u) {
        append(proto * 3u + next, t, clamp(-x, -0.999, -0.001));
    }
}

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
    // The camera: level by distance (unless hidden behind last frame's depth; a little
    // margin for the camera's jitter and the leaves' sway).
    if (intersects(0u, center, radius) && !occluded(center, radius * 1.1 + 0.3)) {
        let w = params.occlusion.w;
        let a = fade_at(dist, params.lod.x * k, w);
        let bb = fade_at(dist, params.lod.y * k, w);
        let far = fade_at(dist, draw / (1.0 + w), w);
        if (a < 1.0) {
            append_pair(proto, 0u, 1u, a, t);
        } else if (bb < 1.0) {
            append_pair(proto, 1u, 2u, bb, t);
        } else if (far < 1.0) {
            append_pair(proto, 2u, 3u, far, t);
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
                append((v * protos_n + proto) * 3u + lod, t, 0.0);
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
