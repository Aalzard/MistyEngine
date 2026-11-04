// Water surfaces (oceans, lakes, rivers): Gerstner + FFT displacement on a camera-centred
// clipmap or a bounded grid, Fresnel sky / screen-space reflections, refraction with depth
// absorption, subsurface scattering on crests, sun glitter, crest and shore foam, caustics on
// what is underwater, and the view from below. Appended to mesh.wgsl (lights, shadows, fog).
// The underwater fullscreen pass is at the bottom.

struct WaterUniform {
    model: mat4x4<f32>,
    // x = kind (0 ocean, 1 lake, 2 river), y = time, z = FFT tile (m), w = FFT height scale.
    p0: vec4<f32>,
    // x = ripples, y = smoothness, z = reflections, w = screen-space reflections (0/1).
    p1: vec4<f32>,
    // x = refraction, y = clarity (m), z = scattering, w = camera under this surface (0/1).
    p2: vec4<f32>,
    // x = crest foam, y = shore foam, z = shore foam width (m), w = caustics.
    p3: vec4<f32>,
    // x = caustics scale (m), y = caustics depth (m), z = underwater visibility (m), w = distortion.
    p4: vec4<f32>,
    // xy = flow (m/s), z = world cell size of the bounded grid, w = displacement fade distance.
    p5: vec4<f32>,
    // x = 1 / typical FFT amplitude (crest factor scale).
    p6: vec4<f32>,
    shallow: vec4<f32>,
    deep: vec4<f32>,
    scatter: vec4<f32>,
    foam_color: vec4<f32>,
    under_color: vec4<f32>,
    // Gerstner: (dir.x, dir.z, k, amplitude), (omega, phase, steepness, 0).
    wave_a: array<vec4<f32>, 8>,
    wave_b: array<vec4<f32>, 8>,
};

@group(2) @binding(10) var<uniform> water: WaterUniform;
@group(2) @binding(11) var ocean_displacement: texture_2d<f32>;
@group(2) @binding(12) var ocean_slope: texture_2d<f32>;
@group(2) @binding(13) var water_sampler: sampler;
@group(2) @binding(14) var water_scene_color: texture_2d<f32>;
@group(2) @binding(15) var water_scene_depth: texture_2d<f32>;
@group(2) @binding(16) var water_clamp: sampler;

const FFT_TEXELS: f32 = 256.0;

struct WaterIn {
    // xy = local position, z = cell size, w = ring half extent (0 = no morph).
    @location(0) grid: vec4<f32>,
};

struct WaterOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    // Rest position (world XZ) for the detail textures.
    @location(1) rest: vec2<f32>,
    // Gerstner slope (d height / dx, dz) at the vertex.
    @location(2) slope: vec2<f32>,
    // x = crest factor (-1 trough .. 1 crest), y = detail fade (1 near, 0 far), z = bounded edge.
    @location(3) info: vec3<f32>,
};

struct Displaced {
    offset: vec3<f32>,
    slope: vec2<f32>,
    crest: f32,
};

// Gerstner sum; waves shorter than ~4 cells fade out (they can't be represented).
fn gerstner(p: vec2<f32>, t: f32, cell: f32) -> Displaced {
    var d: Displaced;
    d.offset = vec3<f32>(0.0);
    d.slope = vec2<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < 8; i++) {
        let a = water.wave_a[i];
        let b = water.wave_b[i];
        let k = a.z;
        let amp = a.w * clamp(3.14159 / max(k * cell, 1e-4) - 1.0, 0.0, 1.0);
        if (amp <= 0.0) {
            continue;
        }
        let dir = a.xy;
        let theta = k * dot(dir, p) - b.x * t + b.y;
        let s = sin(theta);
        let c = cos(theta);
        let side = b.z * amp * c;
        d.offset += vec3<f32>(side * dir.x, amp * s, side * dir.y);
        d.slope += dir * (k * amp * c);
        total += amp;
    }
    d.crest = d.offset.y / max(total, 0.05);
    return d;
}

fn fft_uv(rest: vec2<f32>) -> vec2<f32> {
    return (rest - water.p5.xy * water.p0.y) / water.p0.z;
}

@vertex
fn vs_water(in: WaterIn) -> WaterOut {
    var local = in.grid.xy;
    var cell = in.grid.z;
    let ring = in.grid.w;
    let kind = water.p0.x;
    if (ring > 0.0) {
        // Near a ring's edge, slide onto the next ring's coarser grid (no cracks).
        let edge = max(abs(local.x), abs(local.y)) / ring;
        let morph = smoothstep(0.7, 0.95, edge);
        let coarse = floor(local / (2.0 * cell) + vec2<f32>(0.5)) * (2.0 * cell);
        local = mix(local, coarse, morph);
        cell = mix(cell, cell * 2.0, morph);
    }
    if (kind > 0.5) {
        cell = water.p5.z;
    }
    let rest_world = (water.model * vec4<f32>(local.x, 0.0, local.y, 1.0)).xyz;
    let p = rest_world.xz;
    let t = water.p0.y;
    // Fade waves into a flat far sea, and lakes / rivers towards their banks.
    let dist = distance(rest_world, view.camera_pos.xyz);
    let far = 1.0 - smoothstep(water.p5.w * 0.4, water.p5.w, dist);
    var edge = 1.0;
    if (kind > 0.5) {
        edge = 1.0 - smoothstep(0.42, 0.5, max(abs(local.x), abs(local.y)));
    }
    var g = gerstner(p, t, cell);
    let lod = max(log2(cell * FFT_TEXELS / water.p0.z), 0.0);
    let fft = textureSampleLevel(ocean_displacement, water_sampler, fft_uv(p), lod).xyz;
    let offset = (g.offset + fft * water.p0.w) * far * edge;
    var out: WaterOut;
    out.world = rest_world + offset;
    out.clip = view.view_proj * vec4<f32>(out.world, 1.0);
    out.rest = p;
    out.slope = g.slope * far * edge;
    let fft_crest = fft.y * water.p0.w * water.p6.x;
    out.info = vec3<f32>(clamp(g.crest * 0.6 + fft_crest * 0.6, -1.0, 1.0) * far * edge, far, edge);
    return out;
}

// ---- Procedural textures ----

fn water_hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = water_hash2(i).x;
    let b = water_hash2(i + vec2<f32>(1.0, 0.0)).x;
    let c = water_hash2(i + vec2<f32>(0.0, 1.0)).x;
    let d = water_hash2(i + vec2<f32>(1.0, 1.0)).x;
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Bubbly foam texture (0..1).
fn foam_pattern(p: vec2<f32>, t: f32) -> f32 {
    var n = value_noise(p * 1.7 + vec2<f32>(t * 0.15, 0.0)) * 0.5;
    n += value_noise(p * 4.3 - vec2<f32>(0.0, t * 0.25)) * 0.3;
    n += value_noise(p * 11.0 + vec2<f32>(t * 0.4, t * 0.2)) * 0.2;
    return n;
}

// One layer of moving cells: bright where two cell borders meet (F2 - F1 small).
fn caustic_layer(p: vec2<f32>, t: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    var f1 = 8.0;
    var f2 = 8.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let h = water_hash2(i + o);
            let pt = o + 0.5 + 0.4 * sin(t * (0.6 + h) + 6.2831 * h);
            let d = distance(f, pt);
            if (d < f1) {
                f2 = f1;
                f1 = d;
            } else if (d < f2) {
                f2 = d;
            }
        }
    }
    return pow(clamp(1.0 - (f2 - f1) * 2.2, 0.0, 1.0), 6.0);
}

// Caustic brightness at a world point (two drifting layers, slightly split per colour).
fn caustics(p: vec2<f32>, t: f32) -> vec3<f32> {
    let q = p / max(water.p4.x, 0.1);
    let a = caustic_layer(q, t);
    let b = caustic_layer(q * 0.77 + vec2<f32>(3.1, 1.7) + vec2<f32>(t * 0.05), t * 1.3);
    let c = caustic_layer(q * 1.01 + vec2<f32>(0.02, 0.0), t);
    return vec3<f32>(a + b * 0.6, (a + b * 0.6) * 0.95 + c * 0.05, c + b * 0.6) * 0.8;
}

// ---- Lighting helpers ----

fn water_ambient(n: vec3<f32>) -> vec3<f32> {
    // An HDRI sky lights the water through its SH (intensity already applied).
    if (sky_is_hdri()) {
        return sky_hdri_diffuse(n);
    }
    var a: vec3<f32>;
    if (n.y >= 0.0) {
        a = mix(lights.equator.rgb, lights.sky.rgb, n.y);
    } else {
        a = mix(lights.equator.rgb, lights.ground.rgb, -n.y);
    }
    return a * lights.info.z;
}

// Shadow of the first directional light on a point (1 = lit).
fn water_sun_shadow(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let count = u32(lights.info.x);
    for (var i = 0u; i < count; i++) {
        let li = lights.items[i];
        if (u32(li.dir_type.w) == 0u) {
            return shadow_factor(i, li, world, n, -normalize(li.dir_type.xyz));
        }
    }
    return 1.0;
}

fn sun_light() -> vec3<f32> {
    return view.sun_color.rgb * view.sun_dir.w;
}

fn scene_raw_depth(uv: vec2<f32>) -> f32 {
    let size = textureDimensions(water_scene_depth);
    let p = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), vec2<i32>(size) - 1);
    return textureLoad(water_scene_depth, p, 0).r;
}

fn uv_to_ndc(uv: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
}

// March the reflected ray through the depth copy; rgb = hit colour, a = confidence.
fn screen_reflection(world: vec3<f32>, r: vec3<f32>) -> vec4<f32> {
    var step = 0.4;
    var p = world + r * 0.2;
    let eye = view.camera_pos.xyz;
    for (var i = 0; i < 24; i++) {
        p += r * step;
        step *= 1.3;
        let c = view.view_proj * vec4<f32>(p, 1.0);
        if (c.w <= 0.0) {
            break;
        }
        let ndc = c.xyz / c.w;
        if (any(abs(ndc.xy) > vec2<f32>(1.0))) {
            break;
        }
        let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        let d = scene_raw_depth(uv);
        if (d >= 0.99999 || d >= ndc.z) {
            continue;
        }
        let behind = distance(eye, p) - distance(eye, unproject(ndc.xy, d));
        if (behind > 0.0 && behind < step * 2.0 + 0.5) {
            let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
            let fade = clamp(edge * 8.0, 0.0, 1.0) * (1.0 - f32(i) / 24.0);
            return vec4<f32>(textureSampleLevel(water_scene_color, water_clamp, uv, 0.0).rgb, fade);
        }
    }
    return vec4<f32>(0.0);
}

// Light passing through `thickness` metres of water (red goes first).
fn transmittance(thickness: f32) -> vec3<f32> {
    return exp(-thickness * vec3<f32>(2.2, 0.9, 0.7) / max(water.p2.y, 0.05));
}

// Distance fog for the view from below.
fn underwater_fog(color: vec3<f32>, dist: f32) -> vec3<f32> {
    let light = water_ambient(vec3<f32>(0.0, 1.0, 0.0)) + sun_light() * max(view.sun_dir.y, 0.0) * 0.4;
    let fog = water.under_color.rgb * light;
    let k = exp(-dist / max(water.p4.z, 0.1));
    return mix(fog, color * transmittance(dist * 0.3), k);
}

@fragment
fn fs_water(in: WaterOut, @builtin(front_facing) front: bool) -> MeshTargets {
    let t = water.p0.y;
    let eye = view.camera_pos.xyz;
    let to_eye = eye - in.world;
    let water_dist = length(to_eye);
    let v = to_eye / max(water_dist, 1e-4);

    // Normal: Gerstner slope + the FFT slope + finer ripples (same texture, other scales).
    let uv = fft_uv(in.rest);
    let s0 = textureSample(ocean_slope, water_sampler, uv);
    let s1 = textureSample(ocean_slope, water_sampler, uv * 3.7 + vec2<f32>(0.31, 0.77) + water.p5.xy * t * 0.02);
    let s2 = textureSample(ocean_slope, water_sampler, uv * 0.27 + vec2<f32>(0.53, 0.11));
    let ripples = water.p1.x;
    let far = in.info.y;
    var slope = in.slope + s0.xy * water.p0.w;
    slope += (s1.xy * 0.45 * ripples + s2.xy * 0.6) * water.p0.w * max(far, 0.25);
    var n = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
    // Flatten far normals (mip-filtered detail would sparkle).
    n = normalize(mix(vec3<f32>(0.0, 1.0, 0.0), n, 0.55 + 0.45 * far));

    let screen = vec2<f32>(textureDimensions(water_scene_color));
    let screen_uv = in.clip.xy / screen;
    let l = normalize(view.sun_dir.xyz);
    let sun = sun_light();

    if (!front) {
        // From below: Snell's window shows the world above; outside it, total reflection.
        let nu = -n;
        let cos_v = max(dot(nu, v), 0.0);
        let window = smoothstep(0.62, 0.72, cos_v);
        let offset = n.xz * water.p2.x * 0.06;
        let above = textureSampleLevel(water_scene_color, water_clamp, screen_uv + offset, 0.0).rgb;
        var c = mix(water.under_color.rgb * water_ambient(vec3<f32>(0.0, 1.0, 0.0)) * 0.6, above, window);
        c = underwater_fog(c, water_dist);
        var out: MeshTargets;
        out.color = vec4<f32>(c, 1.0);
        out.glow = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return out;
    }

    // ---- Refraction: what is under the surface, absorbed by its depth ----
    let undistorted = scene_raw_depth(screen_uv);
    var ruv = screen_uv + n.xz * water.p2.x * 0.04 * clamp(water_dist * 0.1, 0.2, 1.0);
    var raw = scene_raw_depth(ruv);
    var scene_pos = unproject(uv_to_ndc(ruv), raw);
    // Don't pull in things in front of the water.
    if (distance(eye, scene_pos) < water_dist) {
        ruv = screen_uv;
        raw = undistorted;
        scene_pos = unproject(uv_to_ndc(ruv), raw);
    }
    let sky_behind = raw >= 0.99999;
    var thickness = 1000.0;
    var depth = 1000.0;
    if (!sky_behind) {
        thickness = max(distance(eye, scene_pos) - water_dist, 0.0);
        depth = max(in.world.y - scene_pos.y, 0.0);
    }
    var behind = textureSampleLevel(water_scene_color, water_clamp, ruv, 0.0).rgb;
    let shadow = water_sun_shadow(in.world, n);
    if (!sky_behind && water.p3.w > 0.0) {
        // Caustics on the underwater surface, projected along the sun.
        let along = scene_pos.xz + l.xz / max(l.y, 0.2) * depth;
        let fade = exp(-depth / max(water.p4.y, 0.1)) * clamp(depth * 3.0, 0.0, 1.0);
        behind += behind * caustics(along, t) * water.p3.w * fade * max(l.y, 0.0) * 2.0;
    }
    let depth_tint = 1.0 - exp(-depth / max(water.p2.y * 0.6, 0.05));
    let body_light = water_ambient(vec3<f32>(0.0, 1.0, 0.0)) + sun * max(l.y, 0.0) * 0.35 * shadow;
    let body = mix(water.shallow.rgb, water.deep.rgb, depth_tint) * body_light;
    let trans = transmittance(thickness);
    var refracted = behind * trans + body * (1.0 - trans);

    // ---- Reflection: sky (+ the scene when screen-space reflections are on) ----
    var r = reflect(-v, n);
    r.y = abs(r.y);
    var reflected = sky_color(r);
    if (sky_is_hdri()) {
        // The HDRI's prefiltered cube, blurred by the surface's roughness.
        reflected = sky_hdri_specular(r, 1.0 - clamp(water.p1.y, 0.0, 1.0));
    }
    if (water.p1.w > 0.5) {
        let hit = screen_reflection(in.world, r);
        reflected = mix(reflected, hit.rgb, hit.a);
    }
    let n_dot_v = max(dot(n, v), 0.0);
    let fresnel = (0.02 + 0.98 * pow(1.0 - n_dot_v, 5.0)) * water.p1.z;

    // ---- Sun glitter (GGX, roughened with distance against sparkle aliasing) ----
    let smooth_ = clamp(water.p1.y - (1.0 - far) * 0.12, 0.0, 1.0);
    let rough = max((1.0 - smooth_) * (1.0 - smooth_), 0.0015);
    let h = normalize(l + v);
    let n_dot_l = max(dot(n, l), 0.0);
    let spec_f = 0.02 + 0.98 * pow(1.0 - max(dot(l, h), 0.0), 5.0);
    let spec = sun * d_ggx(max(dot(n, h), 0.0), rough) * spec_f * n_dot_l * 0.25 * shadow * water.p1.z;

    // ---- Subsurface scattering: sunlight through thin crests ----
    let crest = clamp(in.info.x * 0.5 + 0.5, 0.0, 1.0);
    let through = pow(clamp(dot(v, -normalize(l + n * 0.5)), 0.0, 1.0), 4.0);
    let sss = water.scatter.rgb * water.p2.z * (sun * shadow * through * crest * 1.5 + body_light * crest * 0.15);

    // ---- Foam: breaking crests and shorelines ----
    let pattern = foam_pattern(in.rest * 0.9, t);
    let crest_foam = water.p3.x * clamp(s0.z * 1.6 + max(in.info.x - 0.55, 0.0) * 1.5, 0.0, 1.0);
    var shore = 0.0;
    if (!sky_behind) {
        let band = 1.0 - clamp(depth / max(water.p3.z, 0.01), 0.0, 1.0);
        let waves = 0.5 + 0.5 * sin(depth * 6.0 / max(water.p3.z, 0.05) - t * 2.0 + pattern * 4.0);
        shore = water.p3.y * band * mix(waves, 1.0, band * band);
    }
    let foam = clamp(smoothstep(0.35, 0.75, pattern + max(crest_foam, shore) * 0.9) * max(crest_foam, shore) * 1.4, 0.0, 1.0);
    let foam_lit = water.foam_color.rgb * (water_ambient(n) + sun * (n_dot_l * 0.8 + 0.2) * shadow);

    var color = mix(refracted, reflected, fresnel) + sss;
    color = mix(color, foam_lit, foam) + spec * (1.0 - foam);
    let fog = fog_visibility(fog_distance(in.world));
    color = mix(view.fog_color.rgb, color, fog);
    // Shoreline edges of lakes blend into the bank.
    var out: MeshTargets;
    out.color = vec4<f32>(color, 1.0);
    out.glow = vec4<f32>(max(spec - vec3<f32>(1.0), vec3<f32>(0.0)) * 0.3 * fog, 1.0);
    return out;
}

// ---- Camera under the water: fog, tint, wobble and caustics over the whole view ----
@fragment
fn fs_underwater(in: FullscreenOut) -> MeshTargets {
    let t = water.p0.y;
    let uv0 = vec2<f32>(in.ndc.x * 0.5 + 0.5, 0.5 - in.ndc.y * 0.5);
    let wobble = vec2<f32>(sin(uv0.y * 25.0 + t * 1.7), cos(uv0.x * 21.0 + t * 1.3)) * 0.004 * water.p4.w;
    let uv = uv0 + wobble;
    var c = textureSampleLevel(water_scene_color, water_clamp, uv, 0.0).rgb;
    let raw = scene_raw_depth(uv);
    let level = water.model[3].y;
    var dist = 1000.0;
    if (raw < 0.99999) {
        let p = unproject(uv_to_ndc(uv), raw);
        dist = distance(view.camera_pos.xyz, p);
        let depth = level - p.y;
        if (depth > 0.0 && water.p3.w > 0.0) {
            let l = normalize(view.sun_dir.xyz);
            let fade = exp(-depth / max(water.p4.y, 0.1));
            c += c * caustics(p.xz + l.xz / max(l.y, 0.2) * depth, t) * water.p3.w * fade * max(l.y, 0.0) * 2.0;
        }
    }
    // Light fades as the camera goes deeper.
    let below = max(level - view.camera_pos.y, 0.0);
    c = underwater_fog(c, dist) * exp(-below * 0.04);
    var out: MeshTargets;
    out.color = vec4<f32>(c, 1.0);
    out.glow = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return out;
}
