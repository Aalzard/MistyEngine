// Visual Effect shared WGSL: the particle layout, random numbers, noise and ramp sampling.
// Prepended to every generated simulation and output shader (after their bindings).

struct VfxParticle {
    position: vec3<f32>,
    age: f32,
    velocity: vec3<f32>,
    lifetime: f32,
    // Linear colour (HDR) + alpha.
    color: vec4<f32>,
    size: f32,
    // Degrees, around the facing axis.
    angle: f32,
    seed: u32,
    alive: u32,
};

struct VfxProps {
    v: array<vec4<f32>, 64>,
};

const VFX_RAMP_SAMPLES: u32 = 64u;

var<private> vfx_rng: u32;

// PCG hash.
fn vfx_hash(x: u32) -> u32 {
    let s = x * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn vfx_unit(h: u32) -> f32 {
    return f32(h >> 8u) / 16777216.0;
}

fn vfx_rand() -> f32 {
    vfx_rng = vfx_hash(vfx_rng);
    return vfx_unit(vfx_rng);
}

fn vfx_rand3() -> vec3<f32> {
    let x = vfx_rand();
    let y = vfx_rand();
    let z = vfx_rand();
    return vec3<f32>(x, y, z);
}

fn vfx_rand4() -> vec4<f32> {
    let xyz = vfx_rand3();
    return vec4<f32>(xyz, vfx_rand());
}

// A fixed random number per particle and salt.
fn vfx_prand(seed: u32, salt: u32) -> f32 {
    return vfx_unit(vfx_hash(seed ^ vfx_hash(salt)));
}

fn vfx_prand3(seed: u32, salt: u32) -> vec3<f32> {
    return vec3<f32>(vfx_prand(seed, salt), vfx_prand(seed, salt + 1u), vfx_prand(seed, salt + 2u));
}

fn vfx_prand4(seed: u32, salt: u32) -> vec4<f32> {
    return vec4<f32>(vfx_prand3(seed, salt), vfx_prand(seed, salt + 3u));
}

// Uniform direction on the unit sphere.
fn vfx_rand_dir() -> vec3<f32> {
    let z = vfx_rand() * 2.0 - 1.0;
    let a = vfx_rand() * 6.2831853;
    let r = sqrt(max(1.0 - z * z, 0.0));
    return vec3<f32>(r * cos(a), z, r * sin(a));
}

fn vfx_safe_normalize(v: vec3<f32>) -> vec3<f32> {
    let l = length(v);
    if (l < 1e-6) {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    return v / l;
}

// Gradient noise (0..1) with integer hashing, so it is the same everywhere.
fn vfx_grad(i: vec3<i32>) -> vec3<f32> {
    let hz = vfx_hash(u32(i.z) * 83492791u);
    let hy = vfx_hash((u32(i.y) * 19349663u) ^ hz);
    let h = vfx_hash((u32(i.x) * 73856093u) ^ hy);
    let a = vfx_unit(h) * 6.2831853;
    let z = vfx_unit(vfx_hash(h)) * 2.0 - 1.0;
    let r = sqrt(max(1.0 - z * z, 0.0));
    return vec3<f32>(r * cos(a), z, r * sin(a));
}

fn vfx_noise_signed(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    var corners: array<f32, 8>;
    for (var k = 0u; k < 8u; k = k + 1u) {
        let o = vec3<i32>(i32(k & 1u), i32((k >> 1u) & 1u), i32((k >> 2u) & 1u));
        corners[k] = dot(vfx_grad(i + o), f - vec3<f32>(o));
    }
    let x00 = mix(corners[0], corners[1], u.x);
    let x10 = mix(corners[2], corners[3], u.x);
    let x01 = mix(corners[4], corners[5], u.x);
    let x11 = mix(corners[6], corners[7], u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

fn vfx_noise(p: vec3<f32>) -> f32 {
    return clamp(vfx_noise_signed(p) * 0.9 + 0.5, 0.0, 1.0);
}

// Curl of a noise vector potential: a divergence-free swirling field.
fn vfx_curl(p: vec3<f32>) -> vec3<f32> {
    let e = 0.1;
    let o1 = vec3<f32>(31.4, 7.7, 19.3);
    let o2 = vec3<f32>(-12.1, 45.2, 3.9);
    let dx = vec3<f32>(e, 0.0, 0.0);
    let dy = vec3<f32>(0.0, e, 0.0);
    let dz = vec3<f32>(0.0, 0.0, e);
    // Potential components a (offset 0), b (o1), c (o2).
    let a_y = vfx_noise_signed(p + dy) - vfx_noise_signed(p - dy);
    let a_z = vfx_noise_signed(p + dz) - vfx_noise_signed(p - dz);
    let b_x = vfx_noise_signed(p + o1 + dx) - vfx_noise_signed(p + o1 - dx);
    let b_z = vfx_noise_signed(p + o1 + dz) - vfx_noise_signed(p + o1 - dz);
    let c_x = vfx_noise_signed(p + o2 + dx) - vfx_noise_signed(p + o2 - dx);
    let c_y = vfx_noise_signed(p + o2 + dy) - vfx_noise_signed(p + o2 - dy);
    return vec3<f32>(c_y - b_z, a_z - c_x, b_x - a_y) / (2.0 * e);
}

// A baked gradient / curve row (linear colours; curves in x).
fn vfx_ramp(row: u32, t: f32) -> vec4<f32> {
    let x = clamp(t, 0.0, 1.0) * f32(VFX_RAMP_SAMPLES - 1u);
    let i = min(u32(floor(x)), VFX_RAMP_SAMPLES - 1u);
    let j = min(i + 1u, VFX_RAMP_SAMPLES - 1u);
    let base = row * VFX_RAMP_SAMPLES;
    return mix(vfx_ramps[base + i], vfx_ramps[base + j], fract(x));
}

fn vfx_life(p: VfxParticle) -> f32 {
    return clamp(p.age / max(p.lifetime, 0.0001), 0.0, 1.0);
}
