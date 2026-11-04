// HDRI sky cubemaps (render/hdri_sky): equirect → cube faces, box-filtered mips, and the GGX
// roughness chain for reflections. Face layout as probes/cube.rs (+X −X +Y −Y +Z −Z, v grows
// downwards); the equirect mapping as hdri/equirect.rs. Pipelines use automatic layouts, so
// each entry point binds only what it reads.

const PI: f32 = 3.14159265;
const TAU: f32 = 6.2831853;

struct Params {
    // x = output face size, y = perceptual roughness, z = GGX samples, w = source cube size.
    a: vec4<f32>,
    // x = source mip matching the output size (roughness 0), y = source mip count.
    b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var equirect: texture_2d<f32>;
@group(0) @binding(2) var src_cube: texture_cube<f32>;
@group(0) @binding(3) var src_level: texture_2d_array<f32>;
@group(0) @binding(4) var bilinear: sampler;
@group(0) @binding(5) var dst: texture_storage_2d_array<rgba16float, write>;

// Unit direction through face coordinates u, v in -1..1.
fn face_dir(face: u32, u: f32, v: f32) -> vec3<f32> {
    var d: vec3<f32>;
    switch face {
        case 0u: { d = vec3<f32>(1.0, -v, -u); }
        case 1u: { d = vec3<f32>(-1.0, -v, u); }
        case 2u: { d = vec3<f32>(u, 1.0, v); }
        case 3u: { d = vec3<f32>(u, -1.0, -v); }
        case 4u: { d = vec3<f32>(u, -v, 1.0); }
        default: { d = vec3<f32>(-u, -v, -1.0); }
    }
    return normalize(d);
}

fn face_uv(id: vec2<u32>, size: f32, offset: vec2<f32>) -> vec2<f32> {
    return (vec2<f32>(id) + offset) / size * 2.0 - 1.0;
}

fn equirect_uv(d: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(0.5 + atan2(d.x, d.z) / TAU, acos(clamp(d.y, -1.0, 1.0)) / PI);
}

// Mip 0 of the background cube: 2 × 2 equirect taps per texel.
@compute @workgroup_size(8, 8, 1)
fn to_cube(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = params.a.x;
    if (f32(id.x) >= size || f32(id.y) >= size) {
        return;
    }
    var c = vec3<f32>(0.0);
    for (var j = 0u; j < 2u; j++) {
        for (var i = 0u; i < 2u; i++) {
            let uv = face_uv(id.xy, size, vec2<f32>(0.25 + 0.5 * f32(i), 0.25 + 0.5 * f32(j)));
            let d = face_dir(id.z, uv.x, uv.y);
            c += textureSampleLevel(equirect, bilinear, equirect_uv(d), 0.0).rgb;
        }
    }
    textureStore(dst, vec2<i32>(id.xy), i32(id.z), vec4<f32>(c * 0.25, 1.0));
}

// Next mip of the background cube: 2 × 2 box filter of the level above.
@compute @workgroup_size(8, 8, 1)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = params.a.x;
    if (f32(id.x) >= size || f32(id.y) >= size) {
        return;
    }
    let p = vec2<i32>(id.xy) * 2;
    let layer = i32(id.z);
    var c = textureLoad(src_level, p, layer, 0).rgb;
    c += textureLoad(src_level, p + vec2<i32>(1, 0), layer, 0).rgb;
    c += textureLoad(src_level, p + vec2<i32>(0, 1), layer, 0).rgb;
    c += textureLoad(src_level, p + vec2<i32>(1, 1), layer, 0).rgb;
    textureStore(dst, vec2<i32>(id.xy), layer, vec4<f32>(c * 0.25, 1.0));
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(n), f32(reverseBits(i)) * 2.3283064e-10);
}

// GGX half vector around n for alpha a.
fn importance_ggx(xi: vec2<f32>, n: vec3<f32>, a: f32) -> vec3<f32> {
    let phi = TAU * xi.x;
    let cos_t = sqrt(max((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y), 0.0));
    let sin_t = sqrt(max(1.0 - cos_t * cos_t, 0.0));
    let h = vec3<f32>(sin_t * cos(phi), sin_t * sin(phi), cos_t);
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(n.z) < 0.999);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return normalize(t * h.x + b * h.y + n * h.z);
}

fn d_ggx(n_dot_h: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// One roughness level of the specular cube (split sum, N = V = R). Filtered importance
// sampling: each sample reads the background mip matching its solid angle (no fireflies).
@compute @workgroup_size(8, 8, 1)
fn prefilter(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = params.a.x;
    if (f32(id.x) >= size || f32(id.y) >= size) {
        return;
    }
    let uv = face_uv(id.xy, size, vec2<f32>(0.5));
    let n = face_dir(id.z, uv.x, uv.y);
    let rough = params.a.y;
    var c: vec3<f32>;
    if (rough <= 0.0) {
        c = textureSampleLevel(src_cube, bilinear, n, params.b.x).rgb;
    } else {
        let a = max(rough * rough, 1e-3);
        let samples = u32(params.a.z);
        let base = params.a.w;
        let texel_angle = 4.0 * PI / (6.0 * base * base);
        let max_lod = params.b.y - 1.0;
        var sum = vec3<f32>(0.0);
        var weight = 0.0;
        for (var i = 0u; i < samples; i++) {
            let h = importance_ggx(hammersley(i, samples), n, a);
            let n_dot_h = dot(n, h);
            let l = h * (2.0 * n_dot_h) - n;
            let n_dot_l = dot(n, l);
            if (n_dot_l <= 0.0) {
                continue;
            }
            let pdf = d_ggx(n_dot_h, a) * 0.25;
            let sample_angle = 1.0 / (f32(samples) * pdf + 1e-6);
            let lod = clamp(0.5 * log2(sample_angle / texel_angle) + 1.0, 0.0, max_lod);
            sum += textureSampleLevel(src_cube, bilinear, l, lod).rgb * n_dot_l;
            weight += n_dot_l;
        }
        c = sum / max(weight, 1e-6);
    }
    textureStore(dst, vec2<i32>(id.xy), i32(id.z), vec4<f32>(c, 1.0));
}
