// Shared per-view uniform and helpers. Prepended to every shader.

struct View {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz = camera position, w = 1 when orthographic.
    camera_pos: vec4<f32>,
    // x = plane normal axis (0 = X, 1 = Y, 2 = Z), y = fade distance, z = enabled, w = LOD reference size.
    grid: vec4<f32>,
    // rgb = background colour (linear), w = 1 to draw the skybox.
    background: vec4<f32>,
    // ---- Environment (render/environment.rs) ----
    // xyz = unit direction towards the sun, w = 1 when there is a sun.
    sun_dir: vec4<f32>,
    // rgb = linear sun colour × intensity, w = disc (0 none, 1 simple, 2 high quality).
    sun_color: vec4<f32>,
    // rgb = sky tint factor (1 = none), w = atmosphere thickness.
    sky_tint: vec4<f32>,
    // rgb = linear ground colour, w = exposure.
    sky_ground: vec4<f32>,
    // rgb = linear Solid Color skybox, w = 1 when the skybox is a solid colour.
    sky_solid: vec4<f32>,
    // x = sun disc angular radius (radians), y = distance used to fog the horizon.
    sky_misc: vec4<f32>,
    // rgb = linear fog colour, w = mode (0 off, 1 linear, 2 exponential, 3 exponential squared).
    fog_color: vec4<f32>,
    // x = density, y = linear start, z = linear end.
    fog: vec4<f32>,
    // ---- Post-processing (render/post.rs) ----
    // x = tonemapper (0 none, 1 neutral, 2 ACES, 3 AgX, 4 Khronos PBR Neutral), y = exposure multiplier, z = contrast, w = saturation.
    grade: vec4<f32>,
    // rgb = linear colour filter × white balance, w = AgX look (0 base, 1 punchy).
    color_filter: vec4<f32>,
    // x = intensity, y = threshold, z = weight of smaller levels when upsampling.
    bloom: vec4<f32>,
    // rgb = linear bloom tint.
    bloom_tint: vec4<f32>,
    // x = intensity (× 3), y = smoothness exponent.
    vignette: vec4<f32>,
    // rgb = linear vignette colour.
    vignette_color: vec4<f32>,
    // ---- Motion (render/motion.rs) ----
    // Last frame's view-projection without jitter: world -> last frame's clip space.
    prev_view_proj: mat4x4<f32>,
    // xy = this frame's TAA jitter in NDC (view_proj = translation(xy) × the unjittered
    // matrix), zw = target size in pixels.
    motion: vec4<f32>,
    // Camera rotation (quaternion xyzw) and x, y = tangents of half the field of view
    // (0 when orthographic): exact view rays (`view_ray`).
    view_rotation: vec4<f32>,
    view_tan: vec4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;

// ---- Motion vectors (render/motion.rs) ----
// Every main-pass and prepass shader writes where its surface was last frame, so the
// temporal passes (TAA, AO, shadow mask, SSGI / SSR) follow moving objects and wind instead
// of assuming a static world. Vertex stages pass `prev_clip(..)` through, fragment stages
// turn it into a uv offset with `velocity_of`.

// xyw of last frame's clip position of a world point (its previous position for moving
// objects); z is dropped (a vec3 varying).
fn prev_clip(world_prev: vec3<f32>) -> vec3<f32> {
    let c = view.prev_view_proj * vec4<f32>(world_prev, 1.0);
    return vec3<f32>(c.x, c.y, c.w);
}

// Motion vector of a fragment: last frame's uv minus this frame's uv (0..1, y down), from
// its raster position `pos` (@builtin(position)) and the vertex stage's `prev_clip` value.
fn velocity_of(pos: vec4<f32>, prev: vec3<f32>) -> vec2<f32> {
    if (prev.z <= 1e-6) {
        return vec2<f32>(0.0);
    }
    // The raster position is this frame's jittered NDC; take the jitter back out.
    let now = vec2<f32>(pos.x / view.motion.z * 2.0 - 1.0, 1.0 - pos.y / view.motion.w * 2.0) - view.motion.xy;
    let before = prev.xy / prev.z;
    return (before - now) * vec2<f32>(0.5, -0.5);
}

// Motion vector of the sky / background seen in `dir` (camera rotation only; `ndc` = the
// fragment's raster NDC).
fn sky_velocity(dir: vec3<f32>, ndc: vec2<f32>) -> vec2<f32> {
    let c = view.prev_view_proj * vec4<f32>(dir, 0.0);
    if (c.w <= 1e-6) {
        return vec2<f32>(0.0);
    }
    let before = c.xy / c.w;
    let now = ndc - view.motion.xy;
    return (before - now) * vec2<f32>(0.5, -0.5);
}

// ---- HDRI sky (render/hdri_sky) ----
struct SkyEnv {
    // x = 1 when the sky is an HDRI, y / z = cos / sin of its rotation, w = highest specular level.
    params: vec4<f32>,
    // rgb = linear tint × 2^exposure.
    color: vec4<f32>,
    // Diffuse SH (irradiance / π, world space, intensity applied) for surfaces no probe covers.
    sh: array<vec4<f32>, 9>,
    // ---- Moon and night sky (render/moon.rs) ----
    // xyz = towards the moon, w = angular radius (0 = no moon).
    moon_dir: vec4<f32>,
    // xyz = east on the disc, w = texels across the moon's diameter.
    moon_right: vec4<f32>,
    // xyz = north on the disc, w = flags (1 = height in alpha, 2 = square near-side image).
    moon_up: vec4<f32>,
    // xyz = towards the sun that lights it, w = earthshine.
    moon_light: vec4<f32>,
    // rgb = linear radiance of the lit disc.
    moon_color: vec4<f32>,
    // rgb = night zenith colour, w = night share (0 = day).
    night_zenith: vec4<f32>,
    // rgb = night horizon colour, w = star brightness.
    night_horizon: vec4<f32>,
    // rgb = night ground colour.
    night_ground: vec4<f32>,
};

@group(0) @binding(4) var<uniform> sky_env: SkyEnv;
// Background cube (box-filtered mips) and the GGX roughness chain (one level per mip).
@group(0) @binding(5) var sky_cube: texture_cube<f32>;
@group(0) @binding(6) var sky_specular: texture_cube<f32>;
@group(0) @binding(7) var sky_sampler: sampler;

fn sky_is_hdri() -> bool {
    return sky_env.params.x > 0.5;
}

// World direction → the HDRI's own frame (undo the Rotation about +Y).
fn sky_local(d: vec3<f32>) -> vec3<f32> {
    let c = sky_env.params.y;
    let s = sky_env.params.z;
    return vec3<f32>(c * d.x - s * d.z, d.y, s * d.x + c * d.z);
}

// HDRI radiance in `dir`, blurred by `lod` background mips.
fn sky_hdri(dir: vec3<f32>, lod: f32) -> vec3<f32> {
    return textureSampleLevel(sky_cube, sky_sampler, sky_local(dir), lod).rgb * sky_env.color.rgb;
}

// Prefiltered HDRI reflection for perceptual roughness `perceptual`.
fn sky_hdri_specular(r: vec3<f32>, perceptual: f32) -> vec3<f32> {
    let lod = perceptual * sky_env.params.w;
    return textureSampleLevel(sky_specular, sky_sampler, sky_local(r), lod).rgb * sky_env.color.rgb;
}

// Diffuse sky light at normal `n` (SH9, basis as probes/sh.rs).
fn sky_hdri_diffuse(n: vec3<f32>) -> vec3<f32> {
    let c = sky_env.sh;
    var r = c[0].rgb * 0.282095;
    r += c[1].rgb * (0.488603 * n.y) + c[2].rgb * (0.488603 * n.z) + c[3].rgb * (0.488603 * n.x);
    r += c[4].rgb * (1.092548 * n.x * n.y) + c[5].rgb * (1.092548 * n.y * n.z);
    r += c[6].rgb * (0.315392 * (3.0 * n.z * n.z - 1.0)) + c[7].rgb * (1.092548 * n.x * n.z);
    r += c[8].rgb * (0.546274 * (n.x * n.x - n.y * n.y));
    return max(r, vec3<f32>(0.0));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

// 0 = sun high, 1 = sun at or below the horizon.
fn sky_sunset() -> f32 {
    if (view.sun_dir.w < 0.5) {
        return 0.0;
    }
    return 1.0 - smoothstep(0.0, 0.42, view.sun_dir.y);
}

// Night darkening (1 = day).
fn sky_daylight() -> f32 {
    if (view.sun_dir.w < 0.5) {
        return 1.0;
    }
    return 0.04 + 0.96 * smoothstep(-0.3, 0.05, view.sun_dir.y);
}

// The procedural sky colour `day` blended towards the night sky as the sun goes down (dark
// blue, lit by the moon; render/moon.rs `night_mix` on the CPU). Unchanged by day.
fn sky_night(dir: vec3<f32>, day: vec3<f32>) -> vec3<f32> {
    let share = sky_env.night_zenith.w;
    if (share <= 0.0) {
        return day;
    }
    var night: vec3<f32>;
    if (dir.y >= 0.0) {
        night = mix(sky_env.night_horizon.rgb, sky_env.night_zenith.rgb, sqrt(dir.y));
    } else {
        night = mix(sky_env.night_horizon.rgb, sky_env.night_ground.rgb, sqrt(min(-dir.y * 6.0, 1.0)));
    }
    return mix(day, night, share);
}

// Linear sky colour for a direction, without the sun disc: zenith, horizon (warm at sunset),
// ground. Used by the skybox, reflections and (on the CPU, environment.rs) ambient light.
fn sky_color(dir: vec3<f32>) -> vec3<f32> {
    if (view.sky_solid.w > 0.5) {
        return view.sky_solid.rgb;
    }
    if (sky_is_hdri()) {
        return sky_hdri(dir, 0.0);
    }
    let zenith0 = srgb_to_linear(vec3<f32>(0.40, 0.55, 0.76));
    let horizon0 = srgb_to_linear(vec3<f32>(0.76, 0.79, 0.83));
    let t = view.sky_tint.w;
    let tint = view.sky_tint.rgb;
    let thin = clamp(t, 0.0, 1.0);
    let haze = clamp((t - 1.0) * 0.25, 0.0, 1.0);
    var zenith = mix(zenith0, horizon0, haze * 0.8) * tint * (thin * thin);
    var horizon = horizon0 * mix(vec3<f32>(1.0), tint, 0.5) * (0.3 + 0.7 * thin);
    let sunset = sky_sunset();
    if (sunset > 0.0) {
        let toward = dot(dir, view.sun_dir.xyz) * 0.5 + 0.5;
        let glow = srgb_to_linear(vec3<f32>(1.0, 0.55, 0.25)) * (0.3 + 1.1 * toward * toward);
        horizon = mix(horizon, glow, sunset * (0.5 + 0.5 * toward));
        zenith *= 1.0 - 0.65 * sunset;
    }
    let ground = view.sky_ground.rgb * (1.0 - 0.4 * sunset);
    var c: vec3<f32>;
    if (dir.y >= 0.0) {
        c = mix(horizon, zenith, pow(dir.y, 0.6 - 0.2 * sunset));
    } else {
        c = mix(horizon, ground, pow(min(-dir.y * 6.0, 1.0), 0.5));
    }
    return sky_night(dir, c * sky_daylight() * view.sky_ground.w);
}

// Fraction of a surface still visible through fog at distance `d` (1 = no fog).
fn fog_visibility(d: f32) -> f32 {
    let mode = view.fog_color.w;
    if (mode < 0.5) {
        return 1.0;
    }
    var f: f32;
    if (mode < 1.5) {
        f = (view.fog.z - d) / max(view.fog.z - view.fog.y, 1e-4);
    } else if (mode < 2.5) {
        f = exp(-view.fog.x * d);
    } else {
        let k = view.fog.x * d;
        f = exp(-k * k);
    }
    return clamp(f, 0.0, 1.0);
}

// Fog distance of a world point: from the eye (perspective) or the view plane (orthographic).
fn fog_distance(world: vec3<f32>) -> f32 {
    let to_point = world - view.camera_pos.xyz;
    if (view.camera_pos.w > 0.5) {
        let forward = normalize((view.inv_view_proj * vec4<f32>(0.0, 0.0, 1.0, 0.0)).xyz);
        return max(dot(to_point, forward), 0.0);
    }
    return length(to_point);
}

// World position on the near (z = 0) or far (z = 1) plane under an NDC point.
// Unit direction the pixel at (jittered) `ndc` looks along, from the camera's rotation and
// field of view. Unlike unprojecting the far plane through `inv_view_proj` it stays exact
// with a large near / far ratio, so the sky, the sun disc and the clouds hold still while
// the view turns.
fn view_ray(ndc: vec2<f32>) -> vec3<f32> {
    let q = view.view_rotation;
    let v = vec3<f32>((ndc - view.motion.xy) * view.view_tan.xy, 1.0);
    let t = 2.0 * cross(q.xyz, v);
    return normalize(v + q.w * t + cross(q.xyz, t));
}

fn unproject(ndc: vec2<f32>, z: f32) -> vec3<f32> {
    let p = view.inv_view_proj * vec4<f32>(ndc, z, 1.0);
    return p.xyz / p.w;
}

struct FullscreenOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

// One triangle covering the screen.
@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> FullscreenOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    let ndc = uv * 2.0 - 1.0;
    var out: FullscreenOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.ndc = ndc;
    return out;
}
