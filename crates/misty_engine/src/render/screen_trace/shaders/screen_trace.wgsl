// Screen-space traced lighting of the lit shaders (appended after screen_space.wgsl): Screen
// Space Global Illumination and Screen Space Reflections from the trace passes
// (render/screen_trace). With both off the header is zero and `shade` is unchanged.

struct ScreenTrace {
    // x = 1 when SSGI is on, y = SSGI intensity, z = 1 when SSR is on, w = SSR intensity.
    flags: vec4<f32>,
    // x = 1 when lightmapped surfaces take SSGI.
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
    return ambient_diffuse * visibility + t.gi.rgb * diffuse_color * occlusion * intensity;
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
