// Light halos and lens flares (included after common.wgsl + fog_apply.wgsl). Drawn additively
// onto the HDR and glow images after the main pass, so bloom spreads them. The vertex shader
// reads the scene depth at TAPS points around the light: halos and flares fade smoothly as
// objects cover the light instead of popping.

struct GlowIn {
    // xyz = light position (world), w = halo radius in metres (0 for flares).
    @location(0) center: vec4<f32>,
    // rgb = linear HDR colour, w = occlusion bias in metres.
    @location(1) color: vec4<f32>,
    // Halo: x = softness, y = share of the hot core taken out (Soft Core), z = share kept
    // out of bloom (Glow Spread). Flare: x = kind (1 glow, 2 ring, 3 streak, 4 ghost), y = axis
    // position (1 = at the light), z = size (fraction of the screen height), w = x stretch.
    @location(2) shape: vec4<f32>,
};

@group(1) @binding(0) var scene_depth: texture_depth_multisampled_2d;

struct GlowOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) color: vec3<f32>,
    // xyz = light position, w = halo radius.
    @location(2) @interpolate(flat) light: vec4<f32>,
    @location(3) @interpolate(flat) shape: vec4<f32>,
};

struct GlowTargets {
    @location(0) color: vec4<f32>,
    @location(1) glow: vec4<f32>,
};

const TAPS: i32 = 13;

fn quad_corner(i: u32) -> vec2<f32> {
    var c = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    return c[i];
}

fn depth_size() -> vec2<f32> {
    return vec2<f32>(textureDimensions(scene_depth));
}

// Distance from the eye to the scene under NDC point `ndc` (huge for the sky).
fn scene_distance(ndc: vec2<f32>) -> f32 {
    let size = depth_size();
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let px = vec2<i32>(clamp(uv * size, vec2<f32>(0.0), size - 1.0));
    let z = textureLoad(scene_depth, px, 0);
    if (z >= 1.0) {
        return 1e30;
    }
    return distance(unproject(ndc, z), view.camera_pos.xyz);
}

// Share of the taps in a disc of radius `r` (NDC height units) around `ndc` where nothing is
// closer than the light (`dist` away) by more than `bias`. Taps off screen count as visible.
fn visibility(ndc: vec2<f32>, r: f32, dist: f32, bias: f32) -> f32 {
    let size = depth_size();
    let aspect = size.x / size.y;
    var seen = 0.0;
    for (var i = 0; i < TAPS; i++) {
        // Vogel disc: evenly spread, no clumps.
        let a = f32(i) * 2.39996;
        let k = r * sqrt((f32(i) + 0.5) / f32(TAPS));
        let p = ndc + vec2<f32>(cos(a) / aspect, sin(a)) * k;
        if (abs(p.x) > 1.0 || abs(p.y) > 1.0 || scene_distance(p) > dist - bias) {
            seen += 1.0;
        }
    }
    return seen / f32(TAPS);
}

// Everything at one point: a zero-area triangle the rasterizer skips.
fn collapsed(out_in: GlowOut) -> GlowOut {
    var out = out_in;
    out.pos = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    return out;
}

// Halo: a sphere-facing quad around the light with the radius in metres.
@vertex
fn vs_halo(@builtin(vertex_index) vi: u32, inst: GlowIn) -> GlowOut {
    var out: GlowOut;
    let c = inst.center.xyz;
    let r = inst.center.w;
    let corner = quad_corner(vi);
    out.uv = corner;
    out.light = inst.center;
    out.shape = inst.shape;
    out.color = vec3<f32>(0.0);
    let eye = view.camera_pos.xyz;
    let d = distance(eye, c);
    let n = (eye - c) / max(d, 1e-4);
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.99) {
        up = vec3<f32>(0.0, 0.0, 1.0);
    }
    let right = normalize(cross(up, n));
    let up2 = cross(n, right);
    let clip = view.view_proj * vec4<f32>(c, 1.0);
    if (clip.w <= 1e-4) {
        return collapsed(out);
    }
    let ndc = clip.xy / clip.w;
    // Occlusion taps cover the inner fifth of the halo.
    let edge = view.view_proj * vec4<f32>(c + up2 * r * 0.2, 1.0);
    let reach = length(edge.xy / max(edge.w, 1e-4) - ndc);
    let vis = visibility(ndc, clamp(reach, 0.003, 0.08), d, inst.color.w);
    // Fade out as the camera walks into the halo.
    let near = smoothstep(0.3, 1.2, d / max(r, 1e-3));
    let k = vis * near;
    if (k <= 0.0) {
        return collapsed(out);
    }
    out.color = inst.color.rgb * k;
    out.pos = view.view_proj * vec4<f32>(c + (right * corner.x + up2 * corner.y) * r, 1.0);
    return out;
}

// What is left of the light's glow through the fog between it and the eye.
fn glow_through_fog(p: vec3<f32>) -> f32 {
    return vfog_glow(vfog_eval(p));
}

@fragment
fn fs_halo(in: GlowOut) -> GlowTargets {
    let r = length(in.uv);
    if (r >= 1.0) {
        discard;
    }
    let soft = in.shape.x;
    // A wide falloff plus a hot core that softness spreads out.
    let wide = pow(1.0 - r, mix(3.2, 1.5, soft));
    let core = exp(-r * r * mix(90.0, 25.0, soft)) * mix(1.0, 0.35, soft) * (1.0 - in.shape.y);
    var i = wide * 0.55 + core;
    // Soft depth fade: geometry just in front of the halo's plane eats into it smoothly.
    let size = depth_size();
    let px = vec2<i32>(clamp(in.pos.xy, vec2<f32>(0.0), size - 1.0));
    let z = textureLoad(scene_depth, px, 0);
    if (z < 1.0) {
        let ndc = vec2<f32>(in.pos.x / size.x * 2.0 - 1.0, 1.0 - in.pos.y / size.y * 2.0);
        let eye = view.camera_pos.xyz;
        let scene = distance(unproject(ndc, z), eye);
        let plane = distance(in.light.xyz, eye) - in.light.w * 0.5;
        i *= clamp((scene - plane) / max(in.light.w * 0.5, 1e-3), 0.0, 1.0);
    }
    let c = in.color * i * glow_through_fog(in.light.xyz);
    // Glow Spread: that share of the halo's bloom is drawn by fs_spread instead.
    return GlowTargets(vec4<f32>(c, 1.0), vec4<f32>(c * (1.0 - in.shape.z), 1.0));
}

// Lens flare element: a screen-space quad on the line from the light through the centre.
@vertex
fn vs_flare(@builtin(vertex_index) vi: u32, inst: GlowIn) -> GlowOut {
    var out: GlowOut;
    let corner = quad_corner(vi);
    out.uv = corner;
    out.light = inst.center;
    out.shape = inst.shape;
    out.color = vec3<f32>(0.0);
    let c = inst.center.xyz;
    let clip = view.view_proj * vec4<f32>(c, 1.0);
    if (clip.w <= 1e-4) {
        return collapsed(out);
    }
    let ndc = clip.xy / clip.w;
    let vis = visibility(ndc, 0.015, distance(c, view.camera_pos.xyz), inst.color.w);
    if (vis <= 0.0) {
        return collapsed(out);
    }
    out.color = inst.color.rgb * vis;
    let size = depth_size();
    let aspect = size.x / size.y;
    let s = inst.shape.z;
    let at = ndc * inst.shape.y;
    out.pos = vec4<f32>(at + corner * vec2<f32>(s * inst.shape.w / aspect, s), 0.0, 1.0);
    return out;
}

@fragment
fn fs_flare(in: GlowOut) -> GlowTargets {
    let r = length(in.uv);
    let kind = u32(in.shape.x + 0.5);
    var i = 0.0;
    var tint = vec3<f32>(1.0);
    if (kind == 3u) {
        // Streak: thin and long, brightest through the light.
        let x = 1.0 - abs(in.uv.x);
        i = x * x * x * exp(-in.uv.y * in.uv.y * 12.0);
    } else if (r < 1.0) {
        if (kind == 1u) {
            i = pow(1.0 - r, 2.5) + exp(-r * r * 60.0);
        } else if (kind == 2u) {
            // Ring: a thin band, warmer outside, cooler inside.
            let band = (r - 0.82) / 0.07;
            i = exp(-band * band);
            tint = vec3<f32>(1.0 + 0.25 * band, 1.0, 1.0 - 0.25 * band);
        } else {
            // Ghost: a soft disc with a brighter rim.
            i = smoothstep(1.0, 0.8, r) * (0.45 + 0.55 * smoothstep(0.35, 0.9, r));
        }
    }
    let c = in.color * tint * max(i, 0.0) * glow_through_fog(in.light.xyz);
    return GlowTargets(vec4<f32>(c, 1.0), vec4<f32>(c, 1.0));
}

// ---- Glow Spread (render/glow/spread.rs) ------------------------------------------------------
// A Rectangle light's (or a halo's) share of glow taken out of bloom, drawn as a mix of three Gaussians of
// the screen distance to its projected quad. Positions are in "q space": NDC with x × aspect.
// Only the HDR image: this glow is bloom already.

struct SpreadIn {
    @location(0) c01: vec4<f32>,
    @location(1) c23: vec4<f32>,
    // rgb = linear radiance, w = occlusion bias in metres.
    @location(2) color: vec4<f32>,
    // xyz = sigmas, w = reach past the outline.
    @location(3) sigma: vec4<f32>,
    // xyz = peak weights, w = occlusion tap radius.
    @location(4) weight: vec4<f32>,
    // xyz = light centre, w = 1 when it is in front of the camera.
    @location(5) center: vec4<f32>,
};

struct SpreadOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) q: vec2<f32>,
    @location(1) @interpolate(flat) c01: vec4<f32>,
    @location(2) @interpolate(flat) c23: vec4<f32>,
    @location(3) @interpolate(flat) color: vec3<f32>,
    @location(4) @interpolate(flat) sigma: vec4<f32>,
    @location(5) @interpolate(flat) weight: vec3<f32>,
    @location(6) @interpolate(flat) center: vec3<f32>,
};

@vertex
fn vs_spread(@builtin(vertex_index) vi: u32, inst: SpreadIn) -> SpreadOut {
    var out: SpreadOut;
    out.c01 = inst.c01;
    out.c23 = inst.c23;
    out.sigma = inst.sigma;
    out.weight = inst.weight.xyz;
    let size = depth_size();
    let aspect = size.x / size.y;
    var vis = 1.0;
    if (inst.center.w > 0.5) {
        let c = inst.center.xyz;
        let clip = view.view_proj * vec4<f32>(c, 1.0);
        vis = visibility(clip.xy / clip.w, inst.weight.w, distance(c, view.camera_pos.xyz), inst.color.w);
    }
    out.color = inst.color.rgb * vis;
    out.center = inst.center.xyz;
    if (vis <= 0.0) {
        out.pos = vec4<f32>(0.0, 0.0, 2.0, 1.0);
        return out;
    }
    // The quad's bounds plus the reach, clamped to the screen.
    let lo = min(min(inst.c01.xy, inst.c01.zw), min(inst.c23.xy, inst.c23.zw)) - inst.sigma.w;
    let hi = max(max(inst.c01.xy, inst.c01.zw), max(inst.c23.xy, inst.c23.zw)) + inst.sigma.w;
    let screen = vec2<f32>(aspect, 1.0);
    let a = clamp(lo, -screen, screen);
    let b = clamp(hi, -screen, screen);
    let t = quad_corner(vi) * 0.5 + 0.5;
    let q = mix(a, b, t);
    out.q = q;
    out.pos = vec4<f32>(q.x / aspect, q.y, 0.0, 1.0);
    return out;
}

// Signed distance from `p` to the polygon `v` (negative inside).
fn quad_distance(p: vec2<f32>, v: array<vec2<f32>, 4>) -> f32 {
    var d = dot(p - v[0], p - v[0]);
    var s = 1.0;
    var j = 3u;
    for (var i = 0u; i < 4u; i++) {
        let e = v[j] - v[i];
        let w = p - v[i];
        let b = w - e * clamp(dot(w, e) / max(dot(e, e), 1e-12), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c = vec3<bool>((p.y >= v[i].y), (v[j].y > p.y), (e.x * w.y > e.y * w.x));
        if (all(c) || !any(c)) {
            s = -s;
        }
        j = i;
    }
    return s * sqrt(d);
}

@fragment
fn fs_spread(in: SpreadOut) -> GlowTargets {
    let v = array<vec2<f32>, 4>(in.c01.xy, in.c01.zw, in.c23.xy, in.c23.zw);
    let d = max(quad_distance(in.q, v), 0.0);
    // Each Gaussian minus its value at the reach: exactly 0 at the halo's edge.
    let s2 = 2.0 * in.sigma.xyz * in.sigma.xyz;
    let edge = exp(-vec3<f32>(in.sigma.w * in.sigma.w) / s2);
    let g = max(exp(-vec3<f32>(d * d) / s2) - edge, vec3<f32>(0.0));
    let k = dot(g, in.weight);
    if (k <= 0.0) {
        discard;
    }
    let c = in.color * k * glow_through_fog(in.center);
    return GlowTargets(vec4<f32>(c, 1.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
}
