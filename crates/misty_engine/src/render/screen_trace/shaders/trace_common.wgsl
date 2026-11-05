// Shared part of the screen-space trace passes (render/screen_trace): parameters, bindings and
// helpers. Prepended to ssgi.wgsl, ssr.wgsl and compose.wgsl; each entry point uses a subset
// of the bindings. "Full" pixels are the prepass resolution; GI and SSR images may be half of it.

struct TraceParams {
    inv_view_proj: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    prev_view_proj: mat4x4<f32>,
    // xyz = camera position, w = 1 when orthographic.
    camera: vec4<f32>,
    // xyz = camera forward, w = frame index (noise rotation).
    forward: vec4<f32>,
    // xyz = last frame's camera position, w = 1 when the GI / SSR history can be reprojected.
    prev_camera: vec4<f32>,
    // xyz = last frame's camera forward, w = 1 when the lit colour history is valid.
    prev_forward: vec4<f32>,
    // xy = full resolution, zw = GI resolution.
    size: vec4<f32>,
    // xy = SSR resolution, z = colour history mip count, w = Hi-Z mip count.
    size2: vec4<f32>,
    // x = near, y = far, z = full pixels per GI texel, w = full pixels per SSR texel.
    clip: vec4<f32>,
    // x = ray length (m), y = rays per pixel, z = steps per ray, w = thickness (m).
    gi: vec4<f32>,
    // x = max history frames, y = 1 when GI is on, z = 1 when SSR is on.
    gi2: vec4<f32>,
    // x = max perceptual roughness, y = thickness (m), z = Hi-Z iterations, w = edge fade.
    ssr: vec4<f32>,
    // x = history weight (0 = none), y = max ray length (m).
    ssr2: vec4<f32>,
};

// What the lit shaders read (screen_trace.wgsl): header + one packed texel per full pixel.
struct ScreenTrace {
    flags: vec4<f32>,
    misc: vec4<f32>,
    // x = GI rg, y = GI b + visibility, z = SSR rg, w = SSR b + confidence (f16 pairs).
    texels: array<vec4<u32>>,
};

@group(0) @binding(0) var<uniform> p: TraceParams;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
// World-space normals of the prepass; w = 1 + smoothness (0 = no surface).
@group(0) @binding(2) var normal_tex: texture_2d<f32>;
// Last frame's lit colour (opaque scene) with a box-filtered mip chain.
@group(0) @binding(3) var color_tex: texture_2d<f32>;
@group(0) @binding(4) var linear_sampler: sampler;
// Closest-depth pyramid of the prepass depth.
@group(0) @binding(5) var hiz_tex: texture_2d<f32>;
@group(0) @binding(6) var in_a: texture_2d<f32>;
@group(0) @binding(7) var in_b: texture_2d<f32>;
@group(0) @binding(8) var in_c: texture_2d<f32>;
@group(0) @binding(9) var out_a: texture_storage_2d<rgba16float, write>;
@group(0) @binding(10) var out_b: texture_storage_2d<rgba16float, write>;
@group(0) @binding(11) var<storage, read_write> trace_out: ScreenTrace;

const PI: f32 = 3.14159265;

fn full_size() -> vec2<i32> {
    return vec2<i32>(p.size.xy);
}

fn load_depth(px: vec2<i32>) -> f32 {
    return textureLoad(depth_tex, clamp(px, vec2<i32>(0), full_size() - 1), 0);
}

// World position of full pixel centre `pc` with device depth `d`.
fn world_at(pc: vec2<f32>, d: f32) -> vec3<f32> {
    let ndc = vec2<f32>(pc.x / p.size.x * 2.0 - 1.0, 1.0 - pc.y / p.size.y * 2.0);
    let h = p.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    return h.xyz / h.w;
}

// Distance along the camera forward.
fn lin_depth(world: vec3<f32>) -> f32 {
    return dot(world - p.camera.xyz, p.forward.xyz);
}

// Device depth → distance along the forward (DirectX-style 0..1 depth).
fn linearize(d: f32) -> f32 {
    let n = p.clip.x;
    let f = p.clip.y;
    if (p.camera.w > 0.5) {
        return n + d * (f - n);
    }
    return n * f / max(f - d * (f - n), 1e-6);
}

// xy = uv (0..1, y down), z = device depth, w = 1 when in front of the camera and on screen.
fn project(world: vec3<f32>, m: mat4x4<f32>) -> vec4<f32> {
    let c = m * vec4<f32>(world, 1.0);
    if (c.w <= 1e-6) {
        return vec4<f32>(0.0);
    }
    let ndc = c.xyz / c.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let inside = all(uv >= vec2<f32>(0.0)) && all(uv < vec2<f32>(1.0));
    return vec4<f32>(uv, ndc.z, select(0.0, 1.0, inside));
}

// xyz = normal, w = smoothness (-1 = no surface).
fn surface_at(px: vec2<i32>) -> vec4<f32> {
    let t = textureLoad(normal_tex, clamp(px, vec2<i32>(0), full_size() - 1), 0);
    if (t.w < 0.5) {
        return vec4<f32>(0.0, 1.0, 0.0, -1.0);
    }
    return vec4<f32>(normalize(t.xyz), clamp(t.w - 1.0, 0.0, 1.0));
}

// Direction from a surface towards the eye.
fn to_eye(world: vec3<f32>) -> vec3<f32> {
    if (p.camera.w > 0.5) {
        return -p.forward.xyz;
    }
    return normalize(p.camera.xyz - world);
}

// Per-pixel noise in 0..1 (interleaved gradient noise).
fn ign(px: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
}

// Full pixel represented by texel `t` of an image `scale` times smaller.
fn full_px(t: vec2<i32>, scale: f32) -> vec2<i32> {
    return min(vec2<i32>(vec2<f32>(t) * scale), full_size() - 1);
}

// Similarity of two surface samples for edge-aware filters: plane distance (relative to the
// depth) and normal agreement.
fn edge_weight(pos: vec3<f32>, n: vec3<f32>, z: f32, q: vec3<f32>, nq: vec3<f32>, spread: f32) -> f32 {
    let plane = abs(dot(n, q - pos)) / (0.01 * z * spread + 0.01);
    let wn = pow(max(dot(n, nq), 0.0), 16.0);
    return exp(-plane) * wn;
}

// Bilateral upsample of a smaller image `img` (`scale` full pixels per texel, `size` texels)
// at full pixel `fp`: the four nearest texels, weighted by how well their surface matches.
fn upsample(img: texture_2d<f32>, size: vec2<f32>, scale: f32, fp: vec2<i32>) -> vec4<f32> {
    if (scale < 1.5) {
        return textureLoad(img, fp, 0);
    }
    let d = load_depth(fp);
    let pc = vec2<f32>(fp) + 0.5;
    let pos = world_at(pc, d);
    let n = surface_at(fp).xyz;
    let z = lin_depth(pos);
    let g = pc / scale - 0.5;
    let base = vec2<i32>(floor(g));
    let f = g - floor(g);
    let last = vec2<i32>(size) - 1;
    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var k = 0; k < 4; k++) {
        let o = vec2<i32>(k & 1, k >> 1u);
        let t = clamp(base + o, vec2<i32>(0), last);
        let q = full_px(t, scale);
        let dq = load_depth(q);
        if (dq >= 1.0) {
            continue;
        }
        let bw = select(1.0 - f.x, f.x, o.x == 1) * select(1.0 - f.y, f.y, o.y == 1);
        let w = (bw + 1e-3) * edge_weight(pos, n, z, world_at(vec2<f32>(q) + 0.5, dq), surface_at(q).xyz, 1.0);
        sum += textureLoad(img, t, 0) * w;
        wsum += w;
    }
    if (wsum < 1e-4) {
        // No matching neighbour (thin detail): the texel under the pixel.
        return textureLoad(img, clamp(vec2<i32>(pc / scale), vec2<i32>(0), last), 0);
    }
    return sum / wsum;
}
