// Grass fields shared by the blades (grass.wgsl, grass_shadow.wgsl) and the ground under them
// (terrain_layers.wgsl): soft lush / dry colour patches in terrain-local metres (the same noise
// as `details::patches` on the CPU, which makes lush patches taller and fuller) and the wind.

fn grass_hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

// Value noise 0..1 on a 1 m lattice.
fn grass_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let a = grass_hash(i);
    let b = grass_hash(i + vec2<f32>(1.0, 0.0));
    let c = grass_hash(i + vec2<f32>(0.0, 1.0));
    let d = grass_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// x = lush weight, y = dry weight (0..1) at terrain-local `p` for patches of `scale` metres.
fn grass_patches(p: vec2<f32>, scale: f32) -> vec2<f32> {
    let q = p / max(scale, 0.5);
    let lush = 0.65 * grass_noise(q) + 0.35 * grass_noise(q * 2.3 + 17.1);
    let dry = 0.6 * grass_noise(q * 0.8 + 41.7) + 0.4 * grass_noise(q * 1.9 + 5.3);
    return vec2<f32>(smoothstep(0.42, 0.78, lush), smoothstep(0.45, 0.8, dry));
}

// Base colour of the grass at a point: healthy, pulled towards the lush and dry colours by the
// patches and by `dry_mix` (per blade).
fn grass_base_color(
    patches: vec2<f32>,
    dry_mix: f32,
    healthy: vec3<f32>,
    dry: vec3<f32>,
    lush: vec3<f32>,
    amounts: vec2<f32>,
) -> vec3<f32> {
    let d = clamp(dry_mix + patches.y * amounts.y * (1.0 - dry_mix), 0.0, 1.0);
    var c = mix(healthy, dry, d);
    return mix(c, lush, patches.x * amounts.x * (1.0 - 0.6 * d));
}

// Gusts 0..1 rolling across the field along `dir` (unit, xz) at `speed` m/s: broad waves of
// `scale` metres stretched across the wind, with smaller faster puffs inside them.
fn grass_gust(p: vec2<f32>, dir: vec2<f32>, time: f32, speed: f32, scale: f32) -> f32 {
    let along = dot(p, dir);
    let across = dot(p, vec2<f32>(-dir.y, dir.x));
    let s = max(scale, 1.0);
    let broad = grass_noise(vec2<f32>((along - time * speed) / s, across / (s * 2.2)));
    let puffs = grass_noise(vec2<f32>((along - time * speed * 1.35) / (s * 0.3), across / (s * 0.45)) + 31.7);
    let g = broad * 0.72 + puffs * 0.28;
    return smoothstep(0.25, 0.85, g);
}
