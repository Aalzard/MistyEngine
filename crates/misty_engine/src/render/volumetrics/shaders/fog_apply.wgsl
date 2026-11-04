// Volumetric / height fog applied by the forward shaders (included after common.wgsl).
// Volumetric: the integrated froxel volume up to the volumetric distance, then analytic height
// fog (unshadowed) up to the max fog distance. Analytic: height fog only. Everything farther
// than the max fog distance, the sky included, is fogged as if it were at that distance.

struct VolumetricFog {
    // x = mode (0 off, 1 analytic, 2 volumetric), y = debug (0 off, 1 density, 2 lighting),
    // z = volumetric distance (view depth), w = max fog distance.
    mode: vec4<f32>,
    // x = near slice depth, y = log(far / near), z = slice count.
    slices: vec4<f32>,
    // xyz = camera forward.
    forward: vec4<f32>,
    // x = extinction at the base height, y = base height, z = 1 / falloff height, w = anisotropy.
    height_fog: vec4<f32>,
    // xyz = direction towards the sun, w = 1 when there is a sun.
    sun_dir: vec4<f32>,
    // rgb = linear sun colour × intensity × volumetric multiplier.
    sun_color: vec4<f32>,
    // rgb = ambient light × dimmer.
    ambient: vec4<f32>,
    // rgb = global fog albedo (linear).
    albedo: vec4<f32>,
};

@group(0) @binding(1) var vfog_volume: texture_3d<f32>;
@group(0) @binding(2) var vfog_sampler: sampler;
@group(0) @binding(3) var<uniform> vfog: VolumetricFog;

fn vfog_phase(g: f32, c: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * 3.14159265 * pow(max(1.0 + g2 - 2.0 * g * c, 1e-4), 1.5));
}

// Average of e^(-k h) for heights h in [h0, h1] (relative to the base height).
fn vfog_exp_average(h0: f32, h1: f32, k: f32) -> f32 {
    let span = (h1 - h0) * k;
    if (abs(span) < 1e-3) {
        return exp(-k * (h0 + h1) * 0.5);
    }
    return (exp(-k * h0) - exp(-k * h1)) / span;
}

// Optical depth of the height fog along the segment a → b (uniform below the base height).
fn vfog_optical_depth(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let len = distance(a, b);
    let base = vfog.height_fog.y;
    let k = vfog.height_fog.z;
    let lo = min(a.y, b.y) - base;
    let hi = max(a.y, b.y) - base;
    var mean = 1.0;
    if (lo >= 0.0) {
        mean = vfog_exp_average(lo, hi, k);
    } else if (hi > 0.0) {
        let below = -lo / (hi - lo);
        mean = below + (1.0 - below) * vfog_exp_average(0.0, hi, k);
    }
    return vfog.height_fog.x * len * mean;
}

// Unshadowed light the height fog scatters towards the eye along view direction `v`.
fn vfog_analytic_light(v: vec3<f32>) -> vec3<f32> {
    var light = vfog.ambient.rgb;
    if (vfog.sun_dir.w > 0.5) {
        light += vfog.sun_color.rgb * vfog_phase(vfog.height_fog.w, dot(v, vfog.sun_dir.xyz)) * 3.14159265;
    }
    return light * vfog.albedo.rgb;
}

// rgb = light scattered towards the eye, a = transmittance, between the eye and `world`.
fn vfog_eval(world: vec3<f32>) -> vec4<f32> {
    let mode = vfog.mode.x;
    if (mode < 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let eye = view.camera_pos.xyz;
    let to_point = world - eye;
    let dist = max(length(to_point), 1e-5);
    let v = to_point / dist;
    let reach = min(dist, vfog.mode.w);
    let light = vfog_analytic_light(v);
    if (mode < 1.5) {
        let t = exp(-vfog_optical_depth(eye, eye + v * reach));
        return vec4<f32>(light * (1.0 - t), t);
    }
    // Froxels up to the volumetric distance (a view depth).
    let cos_v = max(dot(v, vfog.forward.xyz), 1e-4);
    let inside = min(reach, vfog.mode.z / cos_v);
    let p = eye + v * inside;
    let depth = inside * cos_v;
    let clip = view.view_proj * vec4<f32>(p, 1.0);
    let ndc = clip.xy / max(clip.w, 1e-5);
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    // Texel k holds the sum up to the far side of slice k.
    let n = vfog.slices.z;
    let w = log(max(depth, vfog.slices.x) / vfog.slices.x) / vfog.slices.y;
    var s = textureSampleLevel(vfog_volume, vfog_sampler, vec3<f32>(uv, w - 0.5 / n), 0.0);
    let first = vfog.slices.x * exp(vfog.slices.y / n);
    if (depth < first) {
        let f = depth / first;
        s = vec4<f32>(s.rgb * f, mix(1.0, s.a, f));
    }
    if (reach > inside) {
        let t = exp(-vfog_optical_depth(p, eye + v * reach));
        s = vec4<f32>(s.rgb + s.a * light * (1.0 - t), s.a * t);
    }
    return s;
}

// Fogged colour from `f = vfog_eval(..)`. `coverage` scales the in-scattered light for
// premultiplied surfaces (1 otherwise). The debug views replace the colour with the fog.
fn vfog_blend(color: vec3<f32>, f: vec4<f32>, coverage: f32) -> vec3<f32> {
    let debug = vfog.mode.y;
    if (debug > 1.5) {
        return f.rgb * coverage;
    }
    if (debug > 0.5) {
        return vec3<f32>(1.0 - f.a) * coverage;
    }
    return color * f.a + f.rgb * coverage;
}

// What is left of emitted light (bloom, additive particles); nothing in the debug views.
fn vfog_glow(f: vec4<f32>) -> f32 {
    if (vfog.mode.y > 0.5) {
        return 0.0;
    }
    return f.a;
}

// Fog towards the sky / background in direction `dir` (fogged at the max fog distance).
fn vfog_sky(dir: vec3<f32>) -> vec4<f32> {
    return vfog_eval(view.camera_pos.xyz + dir * vfog.mode.w * 2.0);
}
