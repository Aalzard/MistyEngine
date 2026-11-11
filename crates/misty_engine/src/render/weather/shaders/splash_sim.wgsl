// Splash spawning (render/weather): each thread picks a random pixel of this frame's scene
// depth, rebuilds the surface point and its normal, and starts a crown splash (sometimes a
// mist puff) there when the point faces up, is open to the sky (the occlusion map: splashes
// land on roofs, not under them) and it is raining there. Splashes go into a ring buffer the
// composite draws; nothing is read back. weather_common.wgsl (group 0) comes first.

struct SplashSim {
    // This frame's view-projection inverse (device depth → world).
    inv_view_proj: mat4x4<f32>,
    // xyz = camera, w = weather clock.
    camera: vec4<f32>,
    // xy = scene depth size, z = ring slots, w = spawn threads this frame.
    sizes: vec4<f32>,
    // x = most rain / m³, y = farthest splash (m), z = frame seed, w = mist share 0..1.
    params: vec4<f32>,
    // x = first ring slot written this frame, y = splash size (m).
    ring: vec4<f32>,
};

struct Splash {
    pos: vec4<f32>,
    info: vec4<f32>,
};

@group(1) @binding(0) var<uniform> sim: SplashSim;
@group(1) @binding(1) var depth_copy: texture_2d<f32>;
@group(1) @binding(2) var<storage, read_write> splash_out: array<Splash>;

fn s_pcg(n: u32) -> u32 {
    var h = n * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}

fn s_rand(n: u32) -> f32 {
    return f32(s_pcg(n) >> 8u) / 16777216.0;
}

fn world_at(px: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(sim.sizes.xy);
    let p = clamp(px, vec2<i32>(0), size - 1);
    let d = textureLoad(depth_copy, p, 0).r;
    let uv = (vec2<f32>(p) + 0.5) / sim.sizes.xy;
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    let w = sim.inv_view_proj * ndc;
    return vec4<f32>(w.xyz / w.w, d);
}

@compute @workgroup_size(64)
fn cs_spawn(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (f32(i) >= sim.sizes.w) {
        return;
    }
    let seed = s_pcg(i * 1664525u + u32(sim.params.z));
    let px = vec2<i32>(vec2<f32>(s_rand(seed), s_rand(seed ^ 0x68E31DA4u)) * sim.sizes.xy);
    let here = world_at(px);
    if (here.w >= 1.0) {
        return;
    }
    let p = here.xyz;
    let dist = distance(p, sim.camera.xyz);
    if (dist > sim.params.y) {
        return;
    }
    // Pixels far away cover more ground: keep more of them so splashes spread evenly.
    let keep = clamp(dist * dist / 144.0, 0.03, 1.0);
    if (s_rand(seed ^ 0xB5297A4Du) > keep) {
        return;
    }
    // Surface normal from the neighbouring depths.
    let dx = world_at(px + vec2<i32>(1, 0)).xyz - p;
    let dy = world_at(px + vec2<i32>(0, 1)).xyz - p;
    var n = normalize(cross(dy, dx));
    if (dot(n, sim.camera.xyz - p) < 0.0) {
        n = -n;
    }
    if (n.y < 0.45 || any(n != n)) {
        return;
    }
    // Raining here, and open to the sky.
    let rain = weather_precip_at(p);
    if (s_rand(seed ^ 0x1B56C4E9u) * sim.params.x >= rain.x * min(rain.z, 1.0)) {
        return;
    }
    if (s_rand(seed ^ 0x2C1B3C6Du) > weather_exposure(p + n * 0.05)) {
        return;
    }
    let slot = (u32(sim.ring.x) + i) % u32(sim.sizes.z);
    let mist = s_rand(seed ^ 0x297A2D39u) < sim.params.w;
    let size = sim.ring.y * (0.7 + s_rand(seed ^ 0x3C6EF372u) * 0.6);
    var s: Splash;
    s.pos = vec4<f32>(p + n * 0.01, sim.camera.w);
    s.info = vec4<f32>(n, select(size, -(0.5 + size * 8.0), mist));
    splash_out[slot] = s;
}
