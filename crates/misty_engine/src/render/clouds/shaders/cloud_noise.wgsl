// One-time generation of the tiling cloud noise textures (render/clouds/noise.rs).
// Base shape (128³): r = Perlin-Worley, gba = Worley fBm at 4, 8 and 16 cells.
// Detail (32³): rgb = Worley fBm at 2, 4 and 8 cells.
// Weather (512²): r = coverage, g = cloud type (0 flat .. 1 towering).

@group(0) @binding(0) var volume_out: texture_storage_3d<rgba8unorm, write>;
@group(0) @binding(1) var weather_out: texture_storage_2d<rgba8unorm, write>;

fn hash_u(p: vec3<u32>) -> u32 {
    var n = p.x * 1664525u + p.y * 1013904223u + p.z * 2654435761u;
    n = (n ^ (n >> 16u)) * 2246822519u;
    n = (n ^ (n >> 13u)) * 3266489917u;
    return n ^ (n >> 16u);
}

fn hash3(p: vec3<u32>) -> vec3<f32> {
    let a = hash_u(p);
    let b = hash_u(p + vec3<u32>(101u, 37u, 59u));
    let c = hash_u(p + vec3<u32>(13u, 211u, 7u));
    return vec3<f32>(vec3<u32>(a, b, c) & vec3<u32>(0xffffffu)) / 16777215.0;
}

// Lattice point `i` wrapped to `period` cells (so the noise tiles).
fn wrap(i: vec3<i32>, period: i32) -> vec3<u32> {
    return vec3<u32>(((i % period) + period) % period);
}

fn gradient(i: vec3<i32>, period: i32, f: vec3<f32>) -> f32 {
    let g = normalize(hash3(wrap(i, period)) * 2.0 - 1.0 + vec3<f32>(1e-4));
    return dot(g, f);
}

// Tiling gradient noise in about -1..1; `p` is in cells.
fn perlin(p: vec3<f32>, period: i32) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    var x: array<f32, 8>;
    for (var k = 0; k < 8; k++) {
        let o = vec3<i32>(k & 1, (k >> 1) & 1, (k >> 2) & 1);
        x[k] = gradient(i + o, period, f - vec3<f32>(o));
    }
    let y0 = mix(mix(x[0], x[1], u.x), mix(x[2], x[3], u.x), u.y);
    let y1 = mix(mix(x[4], x[5], u.x), mix(x[6], x[7], u.x), u.y);
    return mix(y0, y1, u.z);
}

// Inverted tiling cellular noise (1 at feature points, 0 far away); `p` is in cells.
fn worley(p: vec3<f32>, cells: i32) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    var d = 1.0;
    for (var k = 0; k < 27; k++) {
        let o = vec3<i32>(k % 3 - 1, (k / 3) % 3 - 1, k / 9 - 1);
        let feature = vec3<f32>(o) + hash3(wrap(i + o, cells));
        d = min(d, length(feature - f));
    }
    return 1.0 - d;
}

// Three Worley octaves starting at `cells`.
fn worley_fbm(uvw: vec3<f32>, cells: i32) -> f32 {
    let c = f32(cells);
    return worley(uvw * c, cells) * 0.625
        + worley(uvw * c * 2.0, cells * 2) * 0.25
        + worley(uvw * c * 4.0, cells * 4) * 0.125;
}

// Perlin fBm mapped to 0..1.
fn perlin_fbm(uvw: vec3<f32>, period: i32, octaves: i32) -> f32 {
    var sum = 0.0;
    var amp = 1.0;
    var total = 0.0;
    var per = period;
    for (var o = 0; o < octaves; o++) {
        sum += perlin(uvw * f32(per), per) * amp;
        total += amp;
        amp *= 0.5;
        per *= 2;
    }
    return clamp(sum / total * 0.9 + 0.5, 0.0, 1.0);
}

fn remap(x: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    return c + (x - a) / (b - a) * (d - c);
}

@compute @workgroup_size(4, 4, 4)
fn gen_base(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(volume_out);
    if (any(id >= size)) {
        return;
    }
    let uvw = (vec3<f32>(id) + 0.5) / vec3<f32>(size);
    let w4 = worley_fbm(uvw, 4);
    let pw = clamp(remap(perlin_fbm(uvw, 4, 4), 0.0, 1.0, w4, 1.0), 0.0, 1.0);
    textureStore(volume_out, id, vec4<f32>(pw, w4, worley_fbm(uvw, 8), worley_fbm(uvw, 16)));
}

@compute @workgroup_size(4, 4, 4)
fn gen_detail(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(volume_out);
    if (any(id >= size)) {
        return;
    }
    let uvw = (vec3<f32>(id) + 0.5) / vec3<f32>(size);
    textureStore(volume_out, id, vec4<f32>(worley_fbm(uvw, 2), worley_fbm(uvw, 4), worley_fbm(uvw, 8), 1.0));
}

@compute @workgroup_size(8, 8, 1)
fn gen_weather(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(weather_out);
    if (any(id.xy >= size)) {
        return;
    }
    let uv = vec3<f32>((vec2<f32>(id.xy) + 0.5) / vec2<f32>(size), 0.31);
    // Large weather systems broken into clumps of cells.
    let systems = perlin_fbm(uv, 3, 5);
    let clumps = worley(uv * 7.0, 7) * 0.6 + worley(uv * 14.0, 14) * 0.4;
    let coverage = clamp((systems * 0.7 + clumps * 0.45 - 0.35) * 1.7, 0.0, 1.0);
    let kind = perlin_fbm(uv + vec3<f32>(0.37, 0.71, 0.0), 2, 3);
    textureStore(weather_out, id.xy, vec4<f32>(coverage, kind, 0.0, 1.0));
}
