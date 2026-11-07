// Screen-space traced lighting of the lit shaders (appended after screen_space.wgsl): Screen
// Space Global Illumination and Screen Space Reflections from the trace passes
// (render/screen_trace). With both off the header is zero and `shade` is unchanged.

struct ScreenTrace {
    // x = 1 when SSGI is on, y = SSGI intensity, z = 1 when SSR is on, w = SSR intensity.
    flags: vec4<f32>,
    // x = 1 when lightmapped surfaces take SSGI, y = 1 when Misty GI is on (the GI replaces the
    // ambient and takes the screen AO).
    misc: vec4<f32>,
    // x = GI rg, y = GI b + visibility, z = SSR rg, w = SSR b + confidence (non-negative f16 pairs, low half first).
    texels: array<vec4<u32>>,
};

@group(1) @binding(17) var<storage, read> screen_trace: ScreenTrace;

struct TraceSample {
    // rgb = mean radiance of the GI rays that hit a surface, a = share that saw none.
    gi: vec4<f32>,
    // rgb = reflected radiance, a = confidence.
    ssr: vec4<f32>,
    // Screen-space AO at the point (1 = none or AO off).
    ao: f32,
    valid: bool,
};

// One half float (low 16 bits of `h`) without the f16 shader capability.
fn screen_trace_half(h: u32) -> f32 {
    let e = (h >> 10u) & 0x1fu;
    let m = f32(h & 0x3ffu);
    if (e == 0u) {
        return m * exp2(-24.0);
    }
    return (1.0 + m / 1024.0) * exp2(f32(e) - 15.0);
}

fn screen_trace_unpack(v: u32) -> vec2<f32> {
    return vec2<f32>(screen_trace_half(v & 0xffffu), screen_trace_half(v >> 16u));
}

// The traced terms at `world`; invalid when both effects are off or the prepass saw another
// surface there (transparent meshes, water).
fn screen_trace_sample(world: vec3<f32>) -> TraceSample {
    var s: TraceSample;
    s.gi = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    s.ssr = vec4<f32>(0.0);
    s.ao = 1.0;
    s.valid = false;
    if (screen_trace.flags.x < 0.5 && screen_trace.flags.z < 0.5) {
        return s;
    }
    let px = screen_pixel(world);
    if (px.z < 0.5) {
        return s;
    }
    let z = screen_depth(world);
    if (abs(screen_texel(px.xy).y - z) > 0.02 * z + 0.05) {
        return s;
    }
    let p = vec2<u32>(px.xy);
    let e = screen_trace.texels[p.y * u32(screen.misc.z) + p.x];
    s.gi = vec4<f32>(screen_trace_unpack(e.x), screen_trace_unpack(e.y));
    s.ssr = vec4<f32>(screen_trace_unpack(e.z), screen_trace_unpack(e.w));
    s.ao = screen_ao(world);
    s.valid = true;
    return s;
}

// Ambient diffuse with one screen-space bounce: rays that hit a surface bring its light (×
// albedo) instead of the ambient, so the ambient keeps only the share of rays that saw no
// surface. Without that the probes' diffuse (which already holds the room's bounce) and the
// traced bounce would both light the same directions: in a closed room every surface came
// out too bright and flat. When SSAO is on it has already darkened the ambient by `t.ao`;
// the darker of the two visibilities wins instead of their product (no double darkening).
fn screen_gi_apply(t: TraceSample, ambient_diffuse: vec3<f32>, diffuse_color: vec3<f32>, occlusion: f32, lightmapped: bool) -> vec3<f32> {
    if (!t.valid || screen_trace.flags.x < 0.5 || (lightmapped && screen_trace.misc.x < 0.5)) {
        return ambient_diffuse;
    }
    let intensity = screen_trace.flags.y;
    let traced = mix(1.0, t.gi.a, min(intensity, 1.0));
    let visibility = select(traced, min(traced / max(t.ao, 0.05), 1.0), screen.ao.x > 0.5);
    // Misty GI's probes can't see crevices smaller than their grid: the screen AO adds them.
    let gi_ao = select(1.0, t.ao, screen_trace.misc.y > 0.5);
    return ambient_diffuse * visibility + t.gi.rgb * diffuse_color * occlusion * intensity * gi_ao;
}

// Reflection of probes / sky, replaced by the traced reflection where it found something;
// `weight` = the same Fresnel and roughness factors the sky reflection uses.
fn screen_reflection_apply(t: TraceSample, reflection: vec3<f32>, weight: vec3<f32>) -> vec3<f32> {
    if (!t.valid || screen_trace.flags.z < 0.5) {
        return reflection;
    }
    let a = saturate(t.ssr.a * screen_trace.flags.w);
    return mix(reflection, t.ssr.rgb * weight, a);
}

// ---- Misty GI world lookup ------------------------------------------------------------------
// For surfaces without a GI texel of their own (grass blades, tree leaves, a visibility-buffer
// resolve): Misty GI copies its probe clipmap after the texels each frame. Layout from vec4
// `misc.z` of `texels` (as f32): [0] = (levels, probes per side), [1 + k] = level k's first
// probe cell (xyz) and spacing (w), then 4 vec4s per probe: SH L1 of r, g, b and (cell, state).
// `misc.w` = Misty GI intensity (0 = no copy this frame).

fn misty_world(i: u32) -> vec4<f32> {
    return bitcast<vec4<f32>>(screen_trace.texels[u32(screen_trace.misc.z) + i]);
}

fn misty_world_mod(a: i32, n: i32) -> i32 {
    return ((a % n) + n) % n;
}

// How far `world` is inside level k's probe grid, in cells (< 0 = outside it).
fn misty_world_inside(k: u32, world: vec3<f32>) -> f32 {
    let lv = misty_world(1u + k);
    let q = world / lv.w - 0.5 - lv.xyz;
    let m = min(q, vec3<f32>(misty_world(0u).y - 1.0) - q);
    return min(m.x, min(m.y, m.z));
}

// Level k's irradiance at `world` for normal `n`: trilinear probes with a soft back-face weight
// (no visibility test: foliage tolerates it and it keeps the lookup cheap). w = summed weight.
fn misty_world_level(k: u32, world: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    let lv = misty_world(1u + k);
    let side = i32(misty_world(0u).y);
    let spacing = lv.w;
    let p = world + n * (0.25 * spacing);
    let q = p / spacing - 0.5;
    let base = vec3<i32>(floor(q));
    let f = q - floor(q);
    var sum = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var c = 0; c < 8; c++) {
        let o = vec3<i32>(c & 1, (c >> 1u) & 1, (c >> 2u) & 1);
        let g = base + o;
        let idx = ((i32(k) * side + misty_world_mod(g.z, side)) * side + misty_world_mod(g.y, side)) * side + misty_world_mod(g.x, side);
        let at = 8u + u32(idx) * 4u;
        let st = misty_world(at + 3u);
        if (st.w <= 0.0 || any(vec3<i32>(st.xyz) != g)) {
            continue;
        }
        let fo = vec3<f32>(o);
        let tri = (fo.x * f.x + (1.0 - fo.x) * (1.0 - f.x)) * (fo.y * f.y + (1.0 - fo.y) * (1.0 - f.y)) * (fo.z * f.z + (1.0 - fo.z) * (1.0 - f.z));
        let rel = (vec3<f32>(g) + 0.5) * spacing - world;
        let back = (dot(rel / max(length(rel), 1e-5), n) + 1.0) * 0.5;
        let w = (tri + 1e-3) * (back * back + 0.05);
        let r = misty_world(at);
        let gg = misty_world(at + 1u);
        let b = misty_world(at + 2u);
        let k1 = 0.488603 * (2.0 / 3.0) * 0.9;
        let e = vec3<f32>(r.x * 0.282095 + k1 * dot(r.yzw, n), gg.x * 0.282095 + k1 * dot(gg.yzw, n), b.x * 0.282095 + k1 * dot(b.yzw, n));
        sum += max(e, vec3<f32>(0.0)) * w;
        wsum += w;
    }
    if (wsum <= 1e-4) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(sum / wsum, wsum);
}

// Misty GI irradiance (mean incoming radiance, × its intensity) at `world` for normal `n`, from
// the finest probe level holding it, blended into the next near its border. w = 0 when Misty GI
// is off or `world` is outside every level.
fn misty_world_irradiance(world: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    if (screen_trace.misc.w <= 0.0) {
        return vec4<f32>(0.0);
    }
    let levels = u32(misty_world(0u).x);
    for (var k = 0u; k < levels; k++) {
        let inside = misty_world_inside(k, world);
        if (inside < 0.0) {
            continue;
        }
        var e = misty_world_level(k, world, n);
        if (inside < 2.0 && k + 1u < levels) {
            let outer = misty_world_level(k + 1u, world, n);
            if (outer.w > 0.0) {
                e = vec4<f32>(mix(outer.rgb, e.rgb, clamp(inside * 0.5, 0.0, 1.0)), max(e.w, outer.w));
            }
        }
        if (e.w <= 0.0) {
            return vec4<f32>(0.0);
        }
        return vec4<f32>(e.rgb * screen_trace.misc.w, 1.0);
    }
    return vec4<f32>(0.0);
}

// The ambient of a shader with its own lighting, for dense surfaces (grass): Misty GI's
// per-pixel irradiance where the prepass saw this surface (one texel read), else the shader's
// own `hemisphere` ambient (already × the environment intensity). The world lookup below costs
// 32–64 storage reads per call: far too many for every grass fragment.
fn misty_ambient(hemisphere: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    if (screen_trace.misc.w <= 0.0) {
        return hemisphere;
    }
    let t = screen_trace_sample(world);
    return select(hemisphere, t.gi.rgb, t.valid);
}

// `misty_ambient` for sparser surfaces (tree crowns, impostors): where there is no texel, the
// world lookup of the probe clipmap.
fn misty_ambient_world(hemisphere: vec3<f32>, world: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    if (screen_trace.misc.w <= 0.0) {
        return hemisphere;
    }
    let t = screen_trace_sample(world);
    if (t.valid) {
        return t.gi.rgb;
    }
    let e = misty_world_irradiance(world, n);
    return select(hemisphere, e.rgb, e.w > 0.0);
}
