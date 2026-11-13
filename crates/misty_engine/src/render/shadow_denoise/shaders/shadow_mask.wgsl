// Temporal shadow mask (render/shadow_denoise), two compute passes before the lit pass:
//   cs_trace  - for each pixel of the depth + normal prepass, the soft shadow of up to four
//               lights through shadows.wgsl (appended after this file) with a sample pattern
//               that moves every frame; blended with last frame's reprojected result, clamped
//               to this frame's neighbourhood so moving shadows don't smear;
//   cs_filter - a depth / normal-aware blur in screen space, as wide as half of each light's
//               penumbra at that pixel, so sharp contact shadows stay sharp.
// 16-32 taps a frame over ~16 frames average hundreds of shadow-map samples.

const MAX_LOCAL_SHADOWS: u32 = 16u;
// Marks the sky (no surface) in a texel's depth.
const SKY: f32 = 65504.0;

// The parts of mesh.wgsl that shadows.wgsl reads (same layout, group 1).
struct LightItem {
    pos_range: vec4<f32>,
    dir_type: vec4<f32>,
    color: vec4<f32>,
    spot: vec4<f32>,
    shadow: vec4<f32>,
    area_x: vec4<f32>,
    area_y: vec4<f32>,
};

struct Lights {
    info: vec4<f32>,
    shadow_view_proj: mat4x4<f32>,
    sky: vec4<f32>,
    equator: vec4<f32>,
    ground: vec4<f32>,
    items: array<LightItem, 8>,
    local_view_proj: array<mat4x4<f32>, MAX_LOCAL_SHADOWS>,
};

@group(1) @binding(0) var<uniform> lights: Lights;
@group(1) @binding(2) var shadow_map: texture_depth_2d_array;
@group(1) @binding(3) var shadow_sampler: sampler_comparison;
@group(1) @binding(4) var local_shadow_map: texture_depth_2d_array;
@group(1) @binding(10) var vsm_table: texture_2d_array<u32>;
@group(1) @binding(11) var vsm_pool: texture_depth_2d;
@group(1) @binding(12) var vsm_near: texture_depth_2d;

// The camera as shadows.wgsl expects it in `view`.
struct MaskView {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
};
var<private> view: MaskView;

// No mask while making it.
fn shadow_mask_lookup(i: u32, world: vec3<f32>) -> f32 {
    return -1.0;
}

struct MaskParams {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    prev_view_proj: mat4x4<f32>,
    // xyz = camera position, w = 1 when orthographic.
    camera: vec4<f32>,
    // xyz = camera forward.
    forward: vec4<f32>,
    // Last frame's camera position and forward.
    prev_camera: vec4<f32>,
    prev_forward: vec4<f32>,
    // xy = size (pixels), z = frame number, w = 1 when last frame's result can be reused.
    size: vec4<f32>,
    // x = most history frames, y = blur strength (0 = no blur), z = world size of a pixel per
    // metre of depth (perspective), w = world size of a pixel (orthographic).
    settings: vec4<f32>,
    // Light slot of each channel (255 = none).
    slots: vec4<u32>,
};

// Texels: four `u32`s per pixel (see `pack_texel`), or two when the mask covers at most two
// lights (`narrow`): half the memory traffic of every pass that reads or writes it.
struct ShadowMask {
    info: vec4<u32>,
    channels: array<vec4<u32>, 2>,
    pad: vec4<u32>,
    texels: array<vec2<u32>>,
};

@group(0) @binding(0) var<uniform> mp: MaskParams;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
// World-space normals of the prepass.
@group(0) @binding(2) var normal_tex: texture_2d<f32>;
@group(0) @binding(3) var<storage, read> src: ShadowMask;
@group(0) @binding(4) var<storage, read_write> dst: ShadowMask;
// Motion vectors of the prepass (last frame's uv minus this frame's).
@group(0) @binding(5) var velocity_tex: texture_2d<f32>;

fn world_at(px: vec2<f32>, d: f32) -> vec3<f32> {
    let ndc = vec2<f32>(px.x / mp.size.x * 2.0 - 1.0, 1.0 - px.y / mp.size.y * 2.0);
    let h = mp.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    return h.xyz / h.w;
}

fn texel_index(p: vec2<u32>) -> u32 {
    return p.y * u32(mp.size.x) + p.x;
}

fn pack_texel(v: vec4<f32>, depth: f32, frames: u32, radii: vec4<u32>) -> vec4<u32> {
    let c = clamp(v, vec4<f32>(0.0), vec4<f32>(1.0));
    let r = min(radii, vec4<u32>(63u));
    return vec4<u32>(
        pack2x16unorm(c.xy),
        pack2x16unorm(c.zw),
        bitcast<u32>(min(depth, SKY)),
        min(frames, 255u) | (r.x << 8u) | (r.y << 14u) | (r.z << 20u) | (r.w << 26u),
    );
}

fn texel_values(t: vec4<u32>) -> vec4<f32> {
    return vec4<f32>(unpack2x16unorm(t.x), unpack2x16unorm(t.y));
}

fn texel_depth(t: vec4<u32>) -> f32 {
    return bitcast<f32>(t.z);
}

fn texel_frames(t: vec4<u32>) -> u32 {
    return t.w & 0xffu;
}

fn texel_radii(t: vec4<u32>) -> vec4<u32> {
    return (vec4<u32>(t.w) >> vec4<u32>(8u, 14u, 20u, 26u)) & vec4<u32>(63u);
}

// At most two lights (channels 2 and 3 unused): two `u32`s per texel, [visibility 0 | 1
// (unorm16 × 2), depth (f16 bits without the sign, 15) | frames (5) | radii 0, 1 (6 + 6)].
fn narrow() -> bool {
    return mp.slots.z >= 8u && mp.slots.w >= 8u;
}

fn narrow_texel(t: vec4<u32>) -> vec2<u32> {
    let d = pack2x16float(vec2<f32>(bitcast<f32>(t.z), 0.0)) & 0x7fffu;
    let f = min(t.w & 0xffu, 31u);
    let r = (vec2<u32>(t.w) >> vec2<u32>(8u, 14u)) & vec2<u32>(63u);
    return vec2<u32>(t.x, d | (f << 15u) | (r.x << 20u) | (r.y << 26u));
}

fn wide_texel(n: vec2<u32>) -> vec4<u32> {
    let d = unpack2x16float(n.y & 0x7fffu).x;
    let f = (n.y >> 15u) & 31u;
    let r = (vec2<u32>(n.y) >> vec2<u32>(20u, 26u)) & vec2<u32>(63u);
    return vec4<u32>(n.x, 0xffffffffu, bitcast<u32>(d), f | (r.x << 8u) | (r.y << 14u));
}

fn src_texel(i: u32) -> vec4<u32> {
    if (narrow()) {
        return wide_texel(src.texels[i]);
    }
    return vec4<u32>(src.texels[2u * i], src.texels[2u * i + 1u]);
}

fn put_texel(i: u32, t: vec4<u32>) {
    if (narrow()) {
        dst.texels[i] = narrow_texel(t);
    } else {
        dst.texels[2u * i] = t.xy;
        dst.texels[2u * i + 1u] = t.zw;
    }
}

var<workgroup> tile: array<vec4<f32>, 64>;

@compute @workgroup_size(8, 8)
fn cs_trace(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    let inside = f32(id.x) < mp.size.x && f32(id.y) < mp.size.y;
    view.view_proj = mp.view_proj;
    view.camera_pos = mp.camera;
    shadow_frame = u32(mp.size.z) + 1u;
    let px = vec2<f32>(id.xy) + 0.5;
    var raw = vec4<f32>(1.0);
    var radii = vec4<u32>(0u);
    var z = SKY;
    var world = vec3<f32>(0.0);
    var surface = false;
    if (inside) {
        let d = textureLoad(depth_tex, vec2<i32>(id.xy), 0);
        if (d < 1.0) {
            surface = true;
            world = world_at(px, d);
            z = dot(world - mp.camera.xyz, mp.forward.xyz);
            let n = normalize(textureLoad(normal_tex, vec2<i32>(id.xy), 0).xyz);
            let pixel_world = mp.settings.z * z + mp.settings.w;
            for (var c = 0u; c < 4u; c++) {
                let slot = mp.slots[c];
                if (slot >= 8u) {
                    continue;
                }
                let li = lights.items[slot];
                var l = -li.dir_type.xyz;
                if (u32(li.dir_type.w) != 0u) {
                    l = normalize(li.pos_range.xyz - world);
                }
                shadow_radius_out = 0.0;
                raw[c] = shadow_direct(slot, li, world, n, l);
                // Blur up to half the penumbra (in pixels): sharp edges stay sharp.
                let r = 0.5 * shadow_radius_out / max(pixel_world, 1e-6);
                radii[c] = u32(clamp(r, 0.0, 63.0));
            }
        }
    }
    let here = lid.y * 8u + lid.x;
    tile[here] = raw;
    workgroupBarrier();
    if (!inside) {
        return;
    }
    let index = texel_index(id.xy);
    if (!surface) {
        put_texel(index, pack_texel(vec4<f32>(1.0), SKY, 0u, vec4<u32>(0u)));
        return;
    }
    // This frame's 3×3 neighbourhood (within the tile): last frame's value is kept inside it.
    var lo = raw;
    var hi = raw;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let q = vec2<i32>(lid.xy) + vec2<i32>(dx, dy);
            if (any(q < vec2<i32>(0)) || any(q > vec2<i32>(7))) {
                continue;
            }
            let v = tile[u32(q.y) * 8u + u32(q.x)];
            lo = min(lo, v);
            hi = max(hi, v);
        }
    }
    var value = raw;
    var frames = 1u;
    if (mp.size.w > 0.5) {
        let prev = history_at(world, px, textureLoad(velocity_tex, vec2<i32>(id.xy), 0).xy);
        if (prev.frames > 0u) {
            let margin = vec4<f32>(0.02);
            let kept = clamp(prev.value, lo - margin, hi + margin);
            var count = min(prev.frames, u32(mp.settings.x));
            // A big correction: the shadow moved; trust this frame more.
            let moved = max(max(abs(kept.x - prev.value.x), abs(kept.y - prev.value.y)), max(abs(kept.z - prev.value.z), abs(kept.w - prev.value.w)));
            if (moved > 0.15) {
                count = min(count, 3u);
            }
            frames = count + 1u;
            value = mix(kept, raw, 1.0 / f32(frames));
        }
    }
    put_texel(index, pack_texel(value, z, frames, radii));
}

struct History {
    value: vec4<f32>,
    frames: u32,
};

// Pixel of `world` (at pixel `px` now) in last frame's image (xy), through the motion vector
// `velocity`, and its expected depth there (z); w = 1 when on screen.
fn reproject(world: vec3<f32>, px: vec2<f32>, velocity: vec2<f32>) -> vec4<f32> {
    let p = px + velocity * mp.size.xy;
    let expected = dot(world - mp.prev_camera.xyz, mp.prev_forward.xyz);
    let on = all(p >= vec2<f32>(0.0)) && all(p < mp.size.xy);
    return vec4<f32>(p, expected, select(0.0, 1.0, on));
}

// Last frame's accumulated value at `world`: bilinear over the four nearest pixels that saw
// the same surface (depth within 3%). frames = 0 when none did (disocclusion).
fn history_at(world: vec3<f32>, px: vec2<f32>, velocity: vec2<f32>) -> History {
    var out: History;
    out.value = vec4<f32>(1.0);
    out.frames = 0u;
    let r = reproject(world, px, velocity);
    if (r.w < 0.5) {
        return out;
    }
    let base = r.xy - 0.5;
    let f = fract(base);
    let p0 = vec2<i32>(floor(base));
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    var frames = 0.0;
    for (var k = 0; k < 4; k++) {
        let o = vec2<i32>(k & 1, k >> 1);
        let q = p0 + o;
        if (any(q < vec2<i32>(0)) || f32(q.x) >= mp.size.x || f32(q.y) >= mp.size.y) {
            continue;
        }
        let t = src_texel(texel_index(vec2<u32>(q)));
        let frames_t = texel_frames(t);
        if (frames_t == 0u || abs(texel_depth(t) - r.z) > 0.03 * r.z + 0.02) {
            continue;
        }
        let wx = select(1.0 - f.x, f.x, o.x == 1);
        let wy = select(1.0 - f.y, f.y, o.y == 1);
        let w = wx * wy + 1e-4;
        sum += texel_values(t) * w;
        frames += f32(frames_t) * w;
        weight += w;
    }
    if (weight > 0.0) {
        out.value = sum / weight;
        out.frames = u32(round(frames / weight));
    }
    return out;
}

@compute @workgroup_size(8, 8)
fn cs_filter(@builtin(global_invocation_id) id: vec3<u32>) {
    if (f32(id.x) >= mp.size.x || f32(id.y) >= mp.size.y) {
        return;
    }
    let index = texel_index(id.xy);
    let t = src_texel(index);
    let z = texel_depth(t);
    let radii = texel_radii(t);
    if (z >= SKY || mp.settings.y <= 0.0 || all(radii == vec4<u32>(0u))) {
        put_texel(index, t);
        return;
    }
    let n = normalize(textureLoad(normal_tex, vec2<i32>(id.xy), 0).xyz);
    let px = vec2<f32>(id.xy) + 0.5;
    let world = world_at(px, textureLoad(depth_tex, vec2<i32>(id.xy), 0));
    let center = texel_values(t);
    // Fewer accumulated frames: noisier, blur a little wider.
    let young = 1.0 + 2.0 / f32(max(texel_frames(t), 1u));
    let phi = interleaved_gradient(px) * 6.2831853;
    let plane_tolerance = 0.01 * z + 0.01;
    var out = center;
    for (var c = 0u; c < 4u; c++) {
        let r = min(f32(radii[c]) * mp.settings.y * young, 12.0);
        if (mp.slots[c] >= 8u || r < 0.75) {
            continue;
        }
        var sum = center[c];
        var weight = 1.0;
        let taps = 12u;
        for (var i = 0u; i < taps; i++) {
            let s = sqrt((f32(i) + 0.5) / f32(taps));
            let a = f32(i) * GOLDEN_ANGLE + phi;
            let o = vec2<f32>(cos(a), sin(a)) * s * r;
            let q = vec2<i32>(px + o);
            if (any(q < vec2<i32>(0)) || f32(q.x) >= mp.size.x || f32(q.y) >= mp.size.y) {
                continue;
            }
            let d = textureLoad(depth_tex, q, 0);
            if (d >= 1.0) {
                continue;
            }
            let wq = world_at(vec2<f32>(q) + 0.5, d);
            let plane = abs(dot(wq - world, n));
            let nq = textureLoad(normal_tex, q, 0).xyz;
            let wn = pow(max(dot(n, nq), 0.0), 8.0);
            let w = exp(-s * s * 2.0) * wn * exp(-plane / plane_tolerance);
            sum += texel_values(src_texel(texel_index(vec2<u32>(q))))[c] * w;
            weight += w;
        }
        out[c] = sum / weight;
    }
    put_texel(index, pack_texel(out, z, texel_frames(t), radii));
}
