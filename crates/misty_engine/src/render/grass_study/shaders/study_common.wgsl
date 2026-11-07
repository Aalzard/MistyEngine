// Grass study: the frame uniform, the shared textures and the study lighting. The lighting
// stands in for the engine's `shade_grass` (mesh.wgsl) at a similar cost per fragment:
// hemisphere ambient with screen AO, the sun with four hardware-compared cascade taps (one
// cascade pick), a near grass shadow tap, a cloud shadow, light through the blade, GGX sheen,
// distance fog and a froxel fog fetch. Every variant shades with it, so they compare fairly.

struct Study {
    // Jittered (TAA) view-projection and its inverse.
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // Last frame's and this frame's view-projection without jitter.
    prev_view_proj: mat4x4<f32>,
    view_proj_nj: mat4x4<f32>,
    // The sun's two cascades (0: around the camera with grass casters, 1: wide).
    sun_vp0: mat4x4<f32>,
    sun_vp1: mat4x4<f32>,
    // xyz = camera, w = cloud clock (s).
    camera: vec4<f32>,
    // xyz = direction the light travels, w = 1 with sun shadows.
    sun_dir: vec4<f32>,
    // rgb = linear colour × intensity, w = radius of cascade 0 (m).
    sun_color: vec4<f32>,
    // rgb = sky / equator / ground ambient; sky.w = ambient intensity.
    sky: vec4<f32>,
    equator: vec4<f32>,
    ground: vec4<f32>,
    // rgb = fog colour, w = density.
    fog: vec4<f32>,
    // x, y = size in pixels, zw = 1 / size.
    screen: vec4<f32>,
    // x = frame index, y = TAA on.
    frame: vec4<u32>,
    // x = software raster below this many pixels wide (C), y = far field from (m, E),
    // z = split from (m, H), w = split widening (H).
    params: vec4<f32>,
    // xy = lighting cache level 0 origin (world xz), z = its metres per texel, w = texels.
    cache0: vec4<f32>,
    // The same for level 1.
    cache1: vec4<f32>,
};

@group(0) @binding(0) var<uniform> study: Study;
@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_cmp: sampler_comparison;
@group(0) @binding(3) var cloud_tex: texture_2d<f32>;
@group(0) @binding(4) var lin_repeat: sampler;
@group(0) @binding(5) var ao_tex: texture_2d<f32>;
@group(0) @binding(6) var fog_vol: texture_3d<f32>;
// Variant G: sun visibility per height band (rgb = root, middle, tip) on two clip levels.
@group(0) @binding(7) var light_cache: texture_2d_array<f32>;
@group(0) @binding(8) var lin_clamp: sampler;

const PI: f32 = 3.14159265;

// Variant G: the sun's visibility (shadow, near grass shadow, cloud) comes from the
// world-space lighting cache instead of being worked out per fragment.
override CACHE: bool = false;
// Variant H: blades past `study.params.z` draw on alternate frames (half each frame).
override SPLIT: bool = false;

fn study_hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

// Per-pixel, per-frame noise 0..1 (stochastic coverage under TAA).
fn frame_noise(pixel: vec2<f32>) -> f32 {
    let f = f32(study.frame.x % 64u);
    return study_hash(pixel + vec2<f32>(f * 71.3, f * 37.9));
}

// Variant H: this blade sits out this frame.
fn study_split_skip(b: BladeIn, dist: f32) -> bool {
    if (!SPLIT || dist < study.params.z) {
        return false;
    }
    let id = u32(b.shape.w * 65536.0) ^ u32(b.position.x * 97.0);
    return ((id ^ study.frame.x) & 1u) == 1u;
}

fn study_split_widen(b: BladeIn, dist: f32) -> f32 {
    if (!SPLIT) {
        return 1.0;
    }
    return mix(1.0, study.params.w, smoothstep(study.params.z - 2.0, study.params.z, dist));
}

fn cloud_shadow(world: vec3<f32>) -> f32 {
    let uv = world.xz / 700.0 + vec2<f32>(0.0035, 0.0012) * study.camera.w;
    let c = textureSampleLevel(cloud_tex, lin_repeat, uv, 0.0).r;
    return mix(1.0, 0.35, smoothstep(0.5, 0.8, c));
}

fn cascade_tap(m: mat4x4<f32>, layer: i32, p: vec3<f32>, bias: f32) -> f32 {
    let c = m * vec4<f32>(p, 1.0);
    let uv = c.xy * vec2<f32>(0.5, -0.5) + 0.5;
    return textureSampleCompareLevel(shadow_map, shadow_cmp, uv, layer, c.z - bias);
}

// Four taps a few centimetres apart, one cascade pick (shadows.wgsl `sun_shadow_taps`).
fn sun_shadow(world: vec3<f32>, n: vec3<f32>, l: vec3<f32>) -> f32 {
    if (study.sun_dir.w < 0.5) {
        return 1.0;
    }
    let d = distance(world.xz, study.camera.xz);
    let near = d < study.sun_color.w * 0.9;
    let layer = select(1, 0, near);
    var m = study.sun_vp1;
    if (near) {
        m = study.sun_vp0;
    }
    let texel = select(0.12, 0.025, near);
    let t = normalize(cross(l, vec3<f32>(0.001, 1.0, 0.0)));
    let b = cross(l, t);
    let r = 0.025 * (1.0 + 0.08 * distance(world, study.camera.xyz));
    let n_dot_l = max(dot(n, l), 0.0);
    let p = world + n * texel * (1.0 + 2.0 * (1.0 - n_dot_l));
    let bias = select(0.0004, 0.0015, near);
    var s = cascade_tap(m, layer, p + (t + b * 0.5) * r, bias);
    s += cascade_tap(m, layer, p + (-t - b * 0.5) * r, bias);
    s += cascade_tap(m, layer, p + (b - t * 0.5) * r, bias);
    s += cascade_tap(m, layer, p + (-b + t * 0.5) * r, bias);
    s *= 0.25;
    let fade = clamp((140.0 - d) / 14.0, 0.0, 1.0);
    return mix(1.0, s, fade);
}

// Blades shading each other near the camera (stands in for the near grass shadow map).
fn near_shadow(world: vec3<f32>, l: vec3<f32>) -> f32 {
    if (study.sun_dir.w < 0.5) {
        return 1.0;
    }
    let d = distance(world.xz, study.camera.xz);
    if (d > study.sun_color.w * 0.8) {
        return 1.0;
    }
    let s = cascade_tap(study.sun_vp0, 0, world - l * 0.01, 0.0008);
    return mix(s, 1.0, smoothstep(0.6, 0.8, d / study.sun_color.w));
}

// Sun visibility at `world` (cloud × cascades × near grass shadow), the expensive part.
fn sun_visibility(world: vec3<f32>, n: vec3<f32>, l: vec3<f32>) -> f32 {
    return cloud_shadow(world) * sun_shadow(world, n, l) * mix(0.3, 1.0, near_shadow(world, l));
}

// Variant G: the cached visibility at `world`, `band` 0 (root) .. 2 (tip) of the canopy.
fn cached_visibility(world: vec3<f32>, band: f32) -> f32 {
    let d = abs(world.xz - study.camera.xz);
    var level = 1;
    var c = study.cache1;
    if (max(d.x, d.y) < study.cache0.z * study.cache0.w * 0.45) {
        level = 0;
        c = study.cache0;
    }
    let uv = (world.xz - c.xy) / (c.z * c.w);
    let v = textureSampleLevel(light_cache, lin_clamp, uv, level, 0.0).rgb;
    let b = clamp(band, 0.0, 2.0);
    return mix(mix(v.r, v.g, clamp(b, 0.0, 1.0)), v.b, clamp(b - 1.0, 0.0, 1.0));
}

fn d_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a2 = roughness * roughness;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn fog_apply(world: vec3<f32>, pixel: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
    let dist = distance(world, study.camera.xyz);
    let f = exp(-dist * study.fog.w);
    var c = mix(study.fog.rgb, color, f);
    // Froxel fog (constant volume here: the fetch is the cost).
    let uvw = vec3<f32>(pixel * study.screen.zw, clamp(dist / 300.0, 0.0, 1.0));
    let vf = textureSampleLevel(fog_vol, lin_clamp, uvw, 0.0);
    return c * vf.a + vf.rgb;
}

// The lit grass colour of one fragment (grass.wgsl `shade_grass` in study form). `band` =
// height within the canopy 0..2 for the cache.
fn study_shade(
    world: vec3<f32>,
    pixel: vec2<f32>,
    albedo: vec3<f32>,
    n: vec3<f32>,
    occlusion: f32,
    transmission: vec3<f32>,
    smoothness: f32,
    band: f32,
) -> vec3<f32> {
    let v = normalize(study.camera.xyz - world);
    var ambient: vec3<f32>;
    if (n.y >= 0.0) {
        ambient = mix(study.equator.rgb, study.sky.rgb, n.y);
    } else {
        ambient = mix(study.equator.rgb, study.ground.rgb, -n.y);
    }
    let ssao = mix(1.0, textureLoad(ao_tex, vec2<i32>(pixel), 0).r, 0.7);
    var color = ambient * study.sky.w * albedo * occlusion * ssao;
    let l = -study.sun_dir.xyz;
    var atten: f32;
    if (CACHE) {
        atten = cached_visibility(world, band);
    } else {
        atten = sun_visibility(world, n, l);
    }
    atten *= mix(1.0, ssao, 0.5);
    let back = pow(clamp(dot(v, -l), 0.0, 1.0), 5.0);
    let through = max(dot(-n, l), 0.0);
    color += transmission * study.sun_color.rgb * atten * (back * 2.5 + through * 0.8);
    let n_dot_l = dot(n, l);
    if (n_dot_l > 0.0) {
        let perceptual = 1.0 - smoothness;
        let roughness = max(perceptual * perceptual, 0.002);
        let h = normalize(l + v);
        let n_dot_h = max(dot(n, h), 0.0);
        let l_dot_h = max(dot(l, h), 0.0);
        let fresnel = 0.04 + 0.96 * pow(1.0 - l_dot_h, 5.0);
        let vis = 0.25 / max(l_dot_h * l_dot_h, 0.1);
        let spec = fresnel * d_ggx(n_dot_h, roughness) * vis * PI * smoothness;
        color += (albedo + vec3<f32>(spec)) * study.sun_color.rgb * n_dot_l * atten;
    }
    return fog_apply(world, pixel, color);
}

// Fragment counters (only in the counting build; see `count.wgsl`).
@group(0) @binding(9) var<storage, read_write> counters: array<atomic<u32>, 32>;
