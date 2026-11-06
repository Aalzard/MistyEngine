// Screen-space ambient occlusion, horizon based (GTAO-style), as three compute passes:
//   cs_gtao    - for each AO texel, find the highest horizon on both sides of a few screen
//                directions (slices) and integrate the visible cosine-weighted arc;
//   cs_denoise - depth/normal-aware (bilateral) blur at AO resolution;
//   cs_resolve - bilateral upsample to full resolution, temporal accumulation with
//                reprojection, and the final (AO, linear depth) texels the lit shaders read.
// View space here is left-handed: x right, y up, z = distance along the camera forward.

const PI: f32 = 3.14159265;
const HALF_PI: f32 = 1.57079633;
// Texel depth marking the sky (no surface).
const SKY: f32 = 1e30;

struct AoParams {
    inv_view_proj: mat4x4<f32>,
    prev_view_proj: mat4x4<f32>,
    // xyz = camera position, w = 1 when orthographic.
    camera: vec4<f32>,
    // Camera basis in world space.
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    // x = radius (world units), y = intensity, z = steps per side, w = slices.
    params: vec4<f32>,
    // xy = AO resolution, zw = full resolution.
    size: vec4<f32>,
    // x = history weight (0 = camera cut or temporal off), y = frame index,
    // z = full-resolution pixels per AO texel (1 or 2), w = denoise radius (AO texels).
    temporal: vec4<f32>,
    // x, y = half view size at depth 1 (perspective: tan of the half angles) or the half view
    // size (orthographic), z = 1 when AO is on (else the resolve only writes depth).
    proj: vec4<f32>,
};

// Shared with the lit shaders (screen_space.wgsl): header + one texel per pixel.
struct ScreenSpace {
    ao: vec4<f32>,
    contact: vec4<f32>,
    misc: vec4<f32>,
    forward: vec4<f32>,
    // x = ambient occlusion (1 = none), y = linear depth.
    texels: array<vec2<f32>>,
};

@group(0) @binding(0) var<uniform> p: AoParams;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
// World-space normals of the prepass.
@group(0) @binding(2) var normal_tex: texture_2d<f32>;
// xy = (AO, linear depth) at AO resolution.
@group(0) @binding(3) var ao_in: texture_2d<f32>;
@group(0) @binding(4) var ao_out: texture_storage_2d<rg32float, write>;
@group(0) @binding(5) var<storage, read> history: ScreenSpace;
@group(0) @binding(6) var<storage, read_write> current: ScreenSpace;
// Motion vectors of the prepass (last frame's uv minus this frame's).
@group(0) @binding(7) var velocity_tex: texture_2d<f32>;

fn load_depth(px: vec2<f32>) -> f32 {
    return textureLoad(depth_tex, vec2<i32>(px), 0);
}

fn world_pos(px: vec2<f32>, d: f32) -> vec3<f32> {
    let ndc = vec2<f32>(px.x / p.size.z * 2.0 - 1.0, 1.0 - px.y / p.size.w * 2.0);
    let h = p.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    return h.xyz / h.w;
}

fn to_view(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(v, p.right.xyz), dot(v, p.up.xyz), dot(v, p.forward.xyz));
}

// View-space position of the surface at full-resolution pixel `px` with depth `d`.
fn view_pos(px: vec2<f32>, d: f32) -> vec3<f32> {
    return to_view(world_pos(px, d) - p.camera.xyz);
}

fn view_normal(px: vec2<f32>) -> vec3<f32> {
    return to_view(textureLoad(normal_tex, vec2<i32>(px), 0).xyz);
}

// Per-pixel noise in 0..1 (interleaved gradient noise).
fn ign(px: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
}

// Horizon cosine of one sample, faded towards the tangent-plane horizon `low` as the
// occluder gets farther than the radius.
fn horizon(px: vec2<f32>, pos: vec3<f32>, v: vec3<f32>, low: f32, falloff: vec2<f32>) -> f32 {
    // The texel centre, so the position matches the depth that was read.
    let q = floor(clamp(px, vec2<f32>(0.5), p.size.zw - 0.5)) + 0.5;
    let delta = view_pos(q, load_depth(q)) - pos;
    let dist = length(delta);
    // The centre texel itself (offsets under a pixel) is no occluder.
    if (dist < 1e-4) {
        return low;
    }
    let w = saturate(dist * falloff.x + falloff.y);
    return mix(low, dot(delta / max(dist, 1e-5), v), w);
}

@compute @workgroup_size(8, 8)
fn cs_gtao(@builtin(global_invocation_id) id: vec3<u32>) {
    if (f32(id.x) >= p.size.x || f32(id.y) >= p.size.y) {
        return;
    }
    // Centre of the full-resolution pixel this AO texel stands for.
    let full = vec2<f32>(id.xy) * p.temporal.z + 0.5;
    let d = load_depth(full);
    if (d >= 1.0) {
        textureStore(ao_out, id.xy, vec4<f32>(1.0, SKY, 0.0, 0.0));
        return;
    }
    let pos = view_pos(full, d);
    let n = view_normal(full);
    let ortho = p.camera.w > 0.5;
    let v = select(normalize(-pos), vec3<f32>(0.0, 0.0, -1.0), ortho);

    // The world radius in full-resolution pixels at this depth.
    let radius = p.params.x;
    var radius_px = radius * p.size.z / (2.0 * p.proj.x * select(pos.z, 1.0, ortho));
    // Under about a pixel there is nothing to find: no AO (fades in smoothly).
    let fade = saturate(radius_px - 1.0);
    if (fade <= 0.0) {
        textureStore(ao_out, id.xy, vec4<f32>(1.0, pos.z, 0.0, 0.0));
        return;
    }
    radius_px = min(radius_px, p.size.w * 0.3);
    // Occluders fade out over the outer 60% of the radius.
    let range = 0.6 * radius;
    let falloff = vec2<f32>(-1.0 / range, (radius - range) / range + 1.0);

    let seed = vec2<f32>(id.xy) + p.temporal.y * 5.588238;
    let noise_slice = ign(seed);
    let noise_step = ign(seed.yx + vec2<f32>(17.0, 31.0));
    let slices = u32(p.params.w);
    let steps = u32(p.params.z);
    var visibility = 0.0;
    for (var s = 0u; s < slices; s++) {
        let phi = (f32(s) + noise_slice) / f32(slices) * PI;
        let dir = vec2<f32>(cos(phi), sin(phi));
        // Screen y points down, view y up.
        let dir_px = vec2<f32>(dir.x, -dir.y);
        let dir3 = vec3<f32>(dir, 0.0);
        let ortho_dir = dir3 - dot(dir3, v) * v;
        let axis = normalize(cross(ortho_dir, v));
        // The normal projected into the slice plane, and its angle from the view vector.
        let proj_n = n - axis * dot(n, axis);
        let len_n = length(proj_n);
        let cos_n = saturate(dot(proj_n, v) / max(len_n, 1e-4));
        let angle_n = sign(dot(ortho_dir, proj_n)) * acos(cos_n);
        let low0 = cos(angle_n + HALF_PI);
        let low1 = cos(angle_n - HALF_PI);
        var h0c = low0;
        var h1c = low1;
        for (var k = 0u; k < steps; k++) {
            // Denser near the centre, where contact occlusion is.
            var t = (f32(k) + noise_step) / f32(steps);
            t = t * t;
            let off = dir_px * (1.0 + t * (radius_px - 1.0));
            h0c = max(h0c, horizon(full + off, pos, v, low0, falloff));
            h1c = max(h1c, horizon(full - off, pos, v, low1, falloff));
        }
        // Horizon angles, clamped to the hemisphere around the normal.
        var h0 = -acos(clamp(h1c, -1.0, 1.0));
        var h1 = acos(clamp(h0c, -1.0, 1.0));
        h0 = angle_n + clamp(h0 - angle_n, -HALF_PI, HALF_PI);
        h1 = angle_n + clamp(h1 - angle_n, -HALF_PI, HALF_PI);
        // Cosine-weighted visible arc between the two horizons.
        let sin_n = sin(angle_n);
        let arc0 = (cos_n + 2.0 * h0 * sin_n - cos(2.0 * h0 - angle_n)) * 0.25;
        let arc1 = (cos_n + 2.0 * h1 * sin_n - cos(2.0 * h1 - angle_n)) * 0.25;
        visibility += mix(len_n, 1.0, 0.05) * (arc0 + arc1);
    }
    visibility = saturate(visibility / f32(slices));
    let ao = mix(1.0, pow(visibility, p.params.y), fade);
    textureStore(ao_out, id.xy, vec4<f32>(ao, pos.z, 0.0, 0.0));
}

// Weight of a neighbour by how close its depth is (relative), so edges stay sharp.
fn depth_weight(z: f32, center: f32) -> f32 {
    return exp(-abs(z - center) / (0.02 * center + 0.01));
}

@compute @workgroup_size(8, 8)
fn cs_denoise(@builtin(global_invocation_id) id: vec3<u32>) {
    if (f32(id.x) >= p.size.x || f32(id.y) >= p.size.y) {
        return;
    }
    let c = textureLoad(ao_in, vec2<i32>(id.xy), 0).xy;
    if (c.y >= SKY) {
        textureStore(ao_out, id.xy, vec4<f32>(c, 0.0, 0.0));
        return;
    }
    let scale = p.temporal.z;
    let n = view_normal(vec2<f32>(id.xy) * scale + 0.5);
    let r = i32(p.temporal.w);
    let sigma = f32(r) * 0.6 + 0.5;
    let last = vec2<i32>(p.size.xy) - 1;
    var sum = 0.0;
    var weights = 0.0;
    for (var y = -r; y <= r; y++) {
        for (var x = -r; x <= r; x++) {
            let q = clamp(vec2<i32>(id.xy) + vec2<i32>(x, y), vec2<i32>(0), last);
            let s = textureLoad(ao_in, q, 0).xy;
            let nq = view_normal(vec2<f32>(q) * scale + 0.5);
            let spatial = exp(-f32(x * x + y * y) / (2.0 * sigma * sigma));
            let w = spatial * depth_weight(s.y, c.y) * pow(saturate(dot(n, nq)), 8.0);
            sum += s.x * w;
            weights += w;
        }
    }
    textureStore(ao_out, id.xy, vec4<f32>(sum / max(weights, 1e-5), c.y, 0.0, 0.0));
}

// AO at full-resolution pixel `px` (depth `z`) from the denoised AO image: bilinear taps
// weighted by depth similarity when the AO is half resolution.
fn upsample(px: vec2<f32>, z: f32) -> f32 {
    if (p.temporal.z < 1.5) {
        return textureLoad(ao_in, vec2<i32>(px), 0).x;
    }
    // AO texel k sits at full-resolution coordinate 2k + 0.5.
    let h = (px - 0.5) * 0.5;
    let base = floor(h);
    let f = h - base;
    let last = vec2<i32>(p.size.xy) - 1;
    var sum = 0.0;
    var weights = 0.0;
    for (var i = 0u; i < 4u; i++) {
        let o = vec2<f32>(f32(i & 1u), f32(i >> 1u));
        let q = clamp(vec2<i32>(base + o), vec2<i32>(0), last);
        let s = textureLoad(ao_in, q, 0).xy;
        let b = mix(1.0 - f, f, o);
        let bilinear = b.x * b.y;
        let w = bilinear * depth_weight(s.y, z);
        sum += s.x * w;
        weights += w;
    }
    // No half-resolution texel saw this surface (thin edges): leave it unoccluded rather
    // than borrow the AO of what is behind it.
    if (weights < 0.02) {
        return 1.0;
    }
    return sum / weights;
}

@compute @workgroup_size(8, 8)
fn cs_resolve(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = u32(p.size.z);
    if (id.x >= width || f32(id.y) >= p.size.w) {
        return;
    }
    let index = id.y * width + id.x;
    let px = vec2<f32>(id.xy) + 0.5;
    let d = load_depth(px);
    if (d >= 1.0) {
        current.texels[index] = vec2<f32>(1.0, SKY);
        return;
    }
    let world = world_pos(px, d);
    let z = dot(world - p.camera.xyz, p.forward.xyz);
    var ao = 1.0;
    if (p.proj.z > 0.5) {
        ao = upsample(px, z);
        // Reproject into last frame (through the motion vector, so moving objects keep
        // their own AO) and blend with its result when it saw the same surface.
        if (p.temporal.x > 0.0) {
            let velocity = textureLoad(velocity_tex, vec2<i32>(id.xy), 0).xy;
            let prev = px + velocity * p.size.zw;
            let c = p.prev_view_proj * vec4<f32>(world, 1.0);
            if (c.w > 0.0 && all(prev >= vec2<f32>(0.0)) && all(prev < p.size.zw)) {
                let hp = vec2<u32>(prev);
                let h = history.texels[hp.y * width + hp.x];
                // Perspective: w is last frame's view depth. Orthographic: depth barely moves.
                let expected = select(c.w, z, p.camera.w > 0.5);
                if (abs(h.y - expected) < 0.03 * expected + 0.02) {
                    ao = mix(ao, h.x, p.temporal.x);
                }
            }
        }
    }
    current.texels[index] = vec2<f32>(ao, z);
}
