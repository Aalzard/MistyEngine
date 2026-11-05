// Screen Space Global Illumination (appended to trace_common.wgsl), three passes at GI
// resolution:
//   cs_gi_trace    - a few cosine-weighted rays per pixel, marched in world space with
//                    quadratic steps through the prepass depth. A ray that ends behind a
//                    surface (within the thickness) takes that surface's light from last
//                    frame's lit colour, reprojected and read from a mip matched to the
//                    cone. A ray that leaves the screen, finds nothing, or hits a point last
//                    frame didn't see counts as "visible": the lit shader gives it the
//                    ambient / light probe / reflection probe diffuse it already computes, so
//                    the bounce never darkens to black.
//   cs_gi_temporal - reprojected exponential history (bilinear taps rejected by depth),
//                    at most `gi2.x` frames.
//   cs_gi_denoise  - one edge-aware à-trous iteration (5×5 B3 kernel, spacing STEP).
// Output texels: rgb = mean radiance of the rays that hit, a = share of rays that didn't.

// Spacing of the à-trous taps (1, 2, 4 for the three iterations).
override STEP: f32 = 1.0;

// Brightest radiance a ray may bring back (fireflies from tiny bright spots).
const MAX_RADIANCE: f32 = 32.0;

fn gi_scale() -> f32 {
    return p.clip.z;
}

fn gi_size() -> vec2<i32> {
    return vec2<i32>(p.size.zw);
}

// Orthonormal basis around `n` (Duff et al., "Building an Orthonormal Basis, Revisited").
fn basis(n: vec3<f32>) -> mat3x3<f32> {
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    let t = vec3<f32>(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
    let bt = vec3<f32>(b, s + n.y * n.y * a, -n.y);
    return mat3x3<f32>(t, bt, n);
}

fn cosine_dir(u: vec2<f32>, frame: mat3x3<f32>) -> vec3<f32> {
    let r = sqrt(u.x);
    let phi = 2.0 * PI * u.y;
    let local = vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(1.0 - u.x, 0.0)));
    return frame * local;
}

// One ray: rgb = radiance brought back, a = 1 when it saw no on-screen surface.
fn gi_march(pos: vec3<f32>, n: vec3<f32>, dir: vec3<f32>, z0: f32, jitter: f32, pc: vec2<f32>) -> vec4<f32> {
    let steps = u32(p.gi.z);
    let radius = p.gi.x;
    let start = pos + n * (0.01 + 0.003 * z0);
    for (var i = 0u; i < steps; i++) {
        let s = (f32(i) + jitter) / f32(steps);
        let t = radius * s * s + 0.01;
        let q = start + dir * t;
        let c = project(q, p.view_proj);
        if (c.w < 0.5) {
            break;
        }
        let px = vec2<i32>(c.xy * p.size.xy);
        let sd = load_depth(px);
        if (sd >= 1.0) {
            continue;
        }
        let surface = linearize(sd);
        let behind = linearize(c.z) - surface;
        if (behind > 0.002 * surface + 0.01 && behind < p.gi.w) {
            // A surface facing away from us can't send its lit side's light here.
            let hit_n = surface_at(px).xyz;
            let facing = smoothstep(-0.05, 0.25, -dot(hit_n, dir));
            if (p.prev_forward.w < 0.5) {
                return vec4<f32>(0.0, 0.0, 0.0, 1.0);
            }
            let hit = world_at(vec2<f32>(px) + 0.5, sd);
            let prev = project(hit, p.prev_view_proj);
            if (prev.w < 0.5) {
                return vec4<f32>(0.0, 0.0, 0.0, 1.0);
            }
            // Cone footprint: the march spacing in pixels at the hit.
            let dist_px = length(c.xy * p.size.xy - pc);
            let lod = clamp(log2(max(dist_px / f32(steps), 1.0)), 0.0, p.size2.z - 1.0);
            let l = textureSampleLevel(color_tex, linear_sampler, prev.xy, lod).rgb;
            return vec4<f32>(min(l, vec3<f32>(MAX_RADIANCE)) * facing, 0.0);
        }
    }
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

@compute @workgroup_size(8, 8)
fn cs_gi_trace(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = vec2<i32>(id.xy);
    if (any(t >= gi_size())) {
        return;
    }
    let fp = full_px(t, gi_scale());
    let d = load_depth(fp);
    if (d >= 1.0) {
        textureStore(out_a, t, vec4<f32>(0.0, 0.0, 0.0, 1.0));
        return;
    }
    let pc = vec2<f32>(fp) + 0.5;
    let pos = world_at(pc, d);
    let n = surface_at(fp).xyz;
    let z0 = lin_depth(pos);
    let frame = basis(n);
    let rays = u32(p.gi.y);
    let f = p.forward.w;
    // Per-pixel rotation of an R2 sequence that advances every frame (temporal accumulation
    // integrates the hemisphere over a few frames).
    let noise = vec2<f32>(ign(pc + f * 5.588238), ign(pc.yx + f * 7.11));
    var sum = vec4<f32>(0.0);
    for (var r = 0u; r < rays; r++) {
        let k = f32(r) + f * f32(rays);
        let u = fract(noise + k * vec2<f32>(0.7548776662, 0.5698402910));
        let dir = cosine_dir(vec2<f32>(min(u.x, 0.999), u.y), frame);
        let jitter = fract(noise.x * 0.61803 + noise.y + f32(r) * 0.38197);
        sum += gi_march(pos, n, dir, z0, jitter, pc);
    }
    textureStore(out_a, t, sum / f32(rays));
}

// in_a = this frame's trace, in_b / in_c = last frame's accumulation / (length, depth);
// out_a / out_b = this frame's.
@compute @workgroup_size(8, 8)
fn cs_gi_temporal(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = vec2<i32>(id.xy);
    let size = gi_size();
    if (any(t >= size)) {
        return;
    }
    let fp = full_px(t, gi_scale());
    let d = load_depth(fp);
    if (d >= 1.0) {
        textureStore(out_a, t, vec4<f32>(0.0, 0.0, 0.0, 1.0));
        textureStore(out_b, t, vec4<f32>(0.0));
        return;
    }
    let pos = world_at(vec2<f32>(fp) + 0.5, d);
    let current = textureLoad(in_a, t, 0);
    var history = vec4<f32>(0.0);
    var len = 0.0;
    let prev = project(pos, p.prev_view_proj);
    if (p.prev_camera.w > 0.5 && prev.w > 0.5) {
        // Where this surface was last frame, and how far it was from that camera.
        let expected = dot(pos - p.prev_camera.xyz, p.prev_forward.xyz);
        let g = prev.xy * vec2<f32>(size) - 0.5;
        let base = vec2<i32>(floor(g));
        let f = g - floor(g);
        var acc = vec4<f32>(0.0);
        var lsum = 0.0;
        var wsum = 0.0;
        for (var k = 0; k < 4; k++) {
            let o = vec2<i32>(k & 1, k >> 1u);
            let q = base + o;
            if (any(q < vec2<i32>(0)) || any(q >= size)) {
                continue;
            }
            let info = textureLoad(in_c, q, 0);
            // Disocclusion: last frame saw another surface there.
            if (info.x < 0.5 || abs(info.y - expected) > 0.03 * expected + 0.03) {
                continue;
            }
            let w = select(1.0 - f.x, f.x, o.x == 1) * select(1.0 - f.y, f.y, o.y == 1);
            acc += textureLoad(in_b, q, 0) * w;
            lsum += info.x * w;
            wsum += w;
        }
        if (wsum > 0.05) {
            history = acc / wsum;
            len = lsum / wsum;
        }
    }
    let n = min(len + 1.0, p.gi2.x);
    textureStore(out_a, t, mix(history, current, 1.0 / n));
    textureStore(out_b, t, vec4<f32>(n, lin_depth(pos), 0.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn cs_gi_denoise(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = vec2<i32>(id.xy);
    let size = gi_size();
    if (any(t >= size)) {
        return;
    }
    let center = textureLoad(in_a, t, 0);
    let fp = full_px(t, gi_scale());
    let d = load_depth(fp);
    if (d >= 1.0) {
        textureStore(out_a, t, center);
        return;
    }
    let pos = world_at(vec2<f32>(fp) + 0.5, d);
    let n = surface_at(fp).xyz;
    let z = lin_depth(pos);
    var kernel = array<f32, 3>(0.375, 0.25, 0.0625);
    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    let step = i32(STEP);
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let q = t + vec2<i32>(dx, dy) * step;
            if (any(q < vec2<i32>(0)) || any(q >= size)) {
                continue;
            }
            let fq = full_px(q, gi_scale());
            let dq = load_depth(fq);
            if (dq >= 1.0) {
                continue;
            }
            let h = kernel[abs(dx)] * kernel[abs(dy)];
            let pq = world_at(vec2<f32>(fq) + 0.5, dq);
            let w = h * edge_weight(pos, n, z, pq, surface_at(fq).xyz, STEP * gi_scale());
            sum += textureLoad(in_a, q, 0) * w;
            wsum += w;
        }
    }
    textureStore(out_a, t, select(center, sum / wsum, wsum > 1e-5));
}
