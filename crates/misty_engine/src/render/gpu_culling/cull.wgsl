// GPU culling of the opaque draws (see gpu_culling/mod.rs).
// cs_cull tests each candidate (frustum, then Hi-Z occlusion in the second phase) and sets its
// flags; cs_compact writes each batch's visible draws, in order, into the slots buffer and
// their count into the batch's indirect draw arguments.

struct Cull {
    view_proj: mat4x4<f32>,
    // Inside where dot(plane.xyz, p) + plane.w >= 0 (same planes as culling.rs).
    planes: array<vec4<f32>, 6>,
    // x = candidates, y = phase (0 = last frame's visible set, 1 = the rest against Hi-Z),
    // z = Hi-Z mip count (0 = no occlusion test), w = batches.
    info: vec4<u32>,
    // xy = target size in pixels.
    size: vec4<f32>,
};

struct Bounds {
    lo: vec4<f32>,
    hi: vec4<f32>,
};

// Per batch: its candidates and where each phase writes its visible draws.
struct BatchInfo {
    first: u32,
    count: u32,
    out_a: u32,
    out_b: u32,
};

@group(0) @binding(0) var<uniform> cull: Cull;
@group(0) @binding(1) var<storage, read> bounds: array<Bounds>;
// Candidate → draw index.
@group(0) @binding(2) var<storage, read> candidates: array<u32>;
// Per draw: visible last frame (1) or not (0).
@group(0) @binding(3) var<storage, read_write> visible_last: array<u32>;
// Per candidate: bit 0 = drawn in the first phase, bit 1 = drawn in the second.
@group(0) @binding(4) var<storage, read_write> flags: array<u32>;
@group(0) @binding(5) var hiz: texture_2d<f32>;
@group(0) @binding(6) var<storage, read> batches: array<BatchInfo>;
@group(0) @binding(7) var<storage, read_write> slots_out: array<u32>;
// DrawIndexedIndirectArgs (5 words) per batch: the first phase's, then the second's.
@group(0) @binding(8) var<storage, read_write> args: array<u32>;

fn in_frustum(b: Bounds) -> bool {
    let center = (b.lo.xyz + b.hi.xyz) * 0.5;
    let half = (b.hi.xyz - b.lo.xyz) * 0.5;
    for (var i = 0u; i < 6u; i++) {
        let p = cull.planes[i];
        let reach = dot(half, abs(p.xyz));
        if (dot(p.xyz, center) + p.w + reach < -1e-4 * (1.0 + reach)) {
            return false;
        }
    }
    return true;
}

// Hidden behind the depth already drawn this frame? Conservative: the box's nearest depth
// against the farthest depth of the pyramid texels covering its screen rectangle.
fn occluded(b: Bounds) -> bool {
    var lo = vec2<f32>(1e9);
    var hi = vec2<f32>(-1e9);
    var z = 1e9;
    for (var i = 0u; i < 8u; i++) {
        let corner = vec3<f32>(
            select(b.lo.x, b.hi.x, (i & 1u) != 0u),
            select(b.lo.y, b.hi.y, (i & 2u) != 0u),
            select(b.lo.z, b.hi.z, (i & 4u) != 0u),
        );
        let c = cull.view_proj * vec4<f32>(corner, 1.0);
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
    // NDC → pixels (y down), clamped to the screen.
    let full = cull.size.xy;
    let p0 = clamp(vec2<f32>(lo.x * 0.5 + 0.5, 0.5 - hi.y * 0.5) * full, vec2<f32>(0.0), full - 1.0);
    let p1 = clamp(vec2<f32>(hi.x * 0.5 + 0.5, 0.5 - lo.y * 0.5) * full, vec2<f32>(0.0), full - 1.0);
    // Level-0 texels cover 2×2 pixels; pick the level where the rectangle spans ≤ 2 texels.
    var t0 = vec2<u32>(p0) / 2u;
    var t1 = vec2<u32>(p1) / 2u;
    var level = 0u;
    let mips = cull.info.z;
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
fn cs_cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let j = id.x;
    if (j >= cull.info.x) {
        return;
    }
    let d = candidates[j];
    let b = bounds[d];
    let inside = in_frustum(b);
    if (cull.info.y == 0u) {
        flags[j] = select(0u, 1u, inside && visible_last[d] != 0u);
        return;
    }
    var visible = inside;
    if (visible && cull.info.z > 0u) {
        visible = !occluded(b);
    }
    let first = flags[j] & 1u;
    flags[j] = first | select(0u, 2u, visible && first == 0u);
    visible_last[d] = select(0u, 1u, visible);
}

@compute @workgroup_size(64)
fn cs_compact(@builtin(global_invocation_id) id: vec3<u32>) {
    let b = id.x;
    let count = cull.info.w;
    if (b >= count) {
        return;
    }
    let info = batches[b];
    let phase = cull.info.y;
    let bit = select(1u, 2u, phase == 1u);
    let out = select(info.out_a, info.out_b, phase == 1u);
    var n = 0u;
    for (var k = 0u; k < info.count; k++) {
        let j = info.first + k;
        if ((flags[j] & bit) != 0u) {
            slots_out[out + n] = candidates[j];
            n++;
        }
    }
    args[(phase * count + b) * 5u + 1u] = n;
}
