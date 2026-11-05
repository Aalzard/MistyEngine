// Grass: appended to mesh.wgsl (lit by `shade`, receives shadows). One instance per blade or
// seed-head stem; fourteen vertices make a curved card (six segments) that leans out of its
// tuft, sways in travelling gusts and is textured from the built-in grass atlas: eight
// photographed specimens (four leaf blades, two timothy heads, two open panicles) with
// alpha-to-coverage edges. Sunlight shines through backlit blades (Surface.transmission).
// Far blades are fewer (terrain_grass.rs draws a prefix of each chunk) and wider, and shrink
// away towards the draw distance.

struct GrassParams {
    // xyz = terrain corner in the world, w = time in seconds.
    origin: vec4<f32>,
    // rgb = linear healthy colour, w = draw distance.
    healthy: vec4<f32>,
    // rgb = linear dry colour, w = distance where the fade starts.
    dry: vec4<f32>,
    // x = strength, y = speed, zw = direction (xz).
    wind: vec4<f32>,
    // rgb = linear seed-head colour, w = translucency.
    seed: vec4<f32>,
    // x = full-density distance, y = tip dryness.
    look: vec4<f32>,
    // x = the terrain's draw slot (reflection probes).
    draw: vec4<u32>,
};

@group(2) @binding(0) var<uniform> grass: GrassParams;
@group(2) @binding(1) var grass_atlas: texture_2d<f32>;
@group(2) @binding(2) var grass_sampler: sampler;

// Width / height of the specimen in each atlas cell, and its average linear colour.
const CELL_ASPECT = array<f32, 8>(0.058, 0.059, 0.083, 0.067, 0.069, 0.057, 0.146, 0.151);
const CELL_AVG = array<vec3<f32>, 8>(
    vec3<f32>(0.168, 0.224, 0.021),
    vec3<f32>(0.245, 0.326, 0.017),
    vec3<f32>(0.288, 0.299, 0.019),
    vec3<f32>(0.151, 0.208, 0.026),
    vec3<f32>(0.372, 0.279, 0.076),
    vec3<f32>(0.369, 0.301, 0.090),
    vec3<f32>(0.232, 0.197, 0.061),
    vec3<f32>(0.326, 0.257, 0.085),
);
const GRASS_SEGMENTS: f32 = 6.0;
const MIN_KEEP: f32 = 0.02;

struct BladeIn {
    // xyz = root (terrain-local), w = height.
    @location(0) position: vec4<f32>,
    // x = facing angle, y = width, z = dry mix, w = random phase.
    @location(1) shape: vec4<f32>,
    // x = kind (0 blade, 1 seed head), y = bend, z = lean angle, w = tuft tint.
    @location(2) extra: vec4<f32>,
};

struct GrassOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    // x = atlas u, y = atlas v, z = 0 at the root .. 1 at the tip, w = distance fade 0..1.
    @location(3) uv: vec4<f32>,
    // Atlas cell.
    @location(4) @interpolate(flat) cell: u32,
    // 0 close up .. 1 where blades get smaller than a few pixels (softer shading).
    @location(5) far: f32,
    // Height above the ground in metres (canopy shade).
    @location(6) above: f32,
};

// Smooth gusts rolling across the field along the wind (0..1).
fn grass_gust(p: vec2<f32>, dir: vec2<f32>, time: f32, speed: f32) -> f32 {
    let along = dot(p, dir);
    let across = dot(p, vec2<f32>(-dir.y, dir.x));
    let w = sin(along * 0.09 - time * speed * 1.7 + sin(across * 0.05) * 2.0) * 0.5
        + sin(along * 0.23 - time * speed * 2.9 + across * 0.11) * 0.3
        + sin(along * 0.031 - time * speed * 0.8) * 0.2;
    return clamp(w * 0.5 + 0.5, 0.0, 1.0);
}

@vertex
fn vs_grass(@builtin(vertex_index) vi: u32, b: BladeIn) -> GrassOut {
    // Vertices from 100 on draw the coarse blade of far chunks (three segments).
    let coarse = vi >= 100u;
    let k = select(vi, vi - 100u, coarse);
    let t = f32(k / 2u) / select(GRASS_SEGMENTS, 3.0, coarse);
    let side = select(-0.5, 0.5, (k & 1u) == 1u);
    let seed_head = b.extra.x > 0.5;
    let phase = b.shape.w;
    var cell = u32(phase * 4.0) & 3u;
    if (seed_head) {
        cell = 4u + (u32(fract(phase * 7.31) * 4.0) & 3u);
    }

    let root = grass.origin.xyz + b.position.xyz;
    let dist = distance(root, view.camera_pos.xyz);
    let fade = 1.0 - smoothstep(grass.dry.w, grass.healthy.w, dist);
    let height = b.position.w * fade;
    // Fewer blades far away (a prefix of each chunk is drawn): widen them to cover the ground.
    let r = grass.look.x / max(dist, 1e-3);
    let keep = clamp(r * r, MIN_KEEP, 1.0);
    let widen = min(inverseSqrt(keep), 3.0);
    var width = b.shape.y;
    if (seed_head) {
        width = b.position.w * CELL_ASPECT[cell] * (0.8 + 0.4 * fract(phase * 3.7));
    }
    width *= widen;

    // Lean: out of the tuft, arched by `bend`; wind pushes the tip along the gusts.
    let lean = vec3<f32>(cos(b.extra.z), 0.0, sin(b.extra.z));
    let dir = vec2<f32>(grass.wind.z, grass.wind.w);
    let gust = grass_gust(root.xz, dir, grass.origin.w, grass.wind.y);
    let flutter = sin(grass.origin.w * (2.5 + 2.0 * phase) + phase * 40.0) * 0.12;
    let push = grass.wind.x * (0.25 + 0.9 * gust + flutter) * select(1.0, 0.7, seed_head);
    let wind = vec3<f32>(dir.x, 0.0, dir.y) * push;
    let sway = lean * b.extra.y + wind;
    let droop = min(dot(sway, sway), 1.5);
    // Centre line and its tangent (the card keeps roughly its length as it bends).
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let centre = root + (up * (t - 0.3 * droop * t * t) + sway * t * t) * height;
    let tangent = normalize(up * (1.0 - 0.6 * droop * t) + sway * 2.0 * t);
    // Blades turn their face into the bend; seed stems face anywhere.
    var facing_angle = b.extra.z + 1.5708 + (b.shape.x - 3.14159) * 0.25;
    if (seed_head) {
        facing_angle = b.shape.x;
    }
    let across = vec3<f32>(cos(facing_angle), 0.0, sin(facing_angle));
    let p = centre + across * side * width;
    let face = normalize(cross(across, tangent));

    var out: GrassOut;
    out.clip = view.view_proj * vec4<f32>(p, 1.0);
    out.world = p;
    // Rounded across the blade, so a card is not one flat shade.
    out.normal = normalize(face + across * side * 0.8);
    // Far blades are a few pixels wide: their differences turn into flicker, so they move
    // towards the local average (patches stay, single blades blend in).
    let far = smoothstep(4.0, 35.0, dist);
    let dry_mix = mix(b.shape.z, 0.3 + 0.4 * b.shape.z, far * 0.6);
    let tint = b.extra.w * (1.0 - 0.6 * far) * 0.5;
    // Healthy / dry mix, tufts a little lighter or darker.
    let base = mix(grass.healthy.rgb, grass.dry.rgb, dry_mix) * (1.0 + 0.16 * tint);
    out.color = select(base, grass.seed.rgb * (1.0 + 0.1 * tint), seed_head);
    out.far = far;
    out.above = t * height;
    out.uv = vec4<f32>((f32(cell) + side + 0.5) / 8.0, 1.0 - t, t, fade);
    out.cell = cell;
    return out;
}

@fragment
fn fs_grass(in: GrassOut, @builtin(front_facing) front: bool) -> MeshTargets {
    let tex = textureSample(grass_atlas, grass_sampler, in.uv.xy);
    // Sharpened alpha keeps thin shapes solid under minification (alpha to coverage).
    let sharp = clamp((tex.a - 0.5) / max(fwidth(tex.a), 1e-4) + 0.5, 0.0, 1.0);
    // Far away the filtered alpha goes straight to coverage (soft, no crawling edges).
    let coverage = mix(sharp, clamp(tex.a * 1.6, 0.0, 1.0), mix(0.35, 1.0, in.far));
    if (coverage < 0.01) {
        discard;
    }
    let t = in.uv.z;
    let cell = in.cell;
    let seed_head = cell >= 4u;
    let avg = CELL_AVG[cell];
    // The photo's detail (veins, spikelets, browned tips) on top of the chosen colour.
    let detail = clamp(tex.rgb / max(avg, vec3<f32>(0.01)), vec3<f32>(0.25), vec3<f32>(2.5));
    let lum = dot(tex.rgb, vec3<f32>(0.2126, 0.7152, 0.0722))
        / max(dot(avg, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.01);
    var color = in.color;
    if (!seed_head) {
        // Some tips dry out.
        let dry_tip = smoothstep(0.55, 1.0, t) * grass.look.y * smoothstep(0.3, 0.9, fract(f32(cell) * 0.37 + in.color.g * 9.0));
        color = mix(color, grass.dry.rgb, dry_tip);
    } else {
        // Stems below the head stay green-ish.
        color = mix(mix(grass.healthy.rgb, grass.dry.rgb, 0.5), color, smoothstep(0.35, 0.6, t));
    }
    // Photo detail close up, smooth colour far away.
    let textured = mix(vec3<f32>(lum), detail, 0.4);
    let albedo = clamp(color * mix(textured, vec3<f32>(1.0), in.far * 0.85), vec3<f32>(0.0), vec3<f32>(1.0));

    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    // Lean the shading normal up: thin blades light more like the canopy than like cards.
    // (more so far away, where the canopy reads as one surface).
    n = normalize(mix(n, vec3<f32>(0.0, 1.0, 0.0), mix(0.5, 0.8, in.far)));

    var s: Surface;
    s.albedo = albedo;
    s.alpha = coverage;
    s.metallic = 0.0;
    // Waxy blades: a soft sheen close up, matte far away (no sparkle).
    s.smoothness = mix(select(0.25, 0.15, seed_head), 0.05, in.far) * in.uv.w;
    s.normal = n;
    s.emission = vec3<f32>(0.0);
    s.receive_shadows = true;
    s.fast_shadows = true;
    // Blades shade each other near the ground.
    // Canopy shade: light reaches the ground only through the blades above, so everything
    // low in the grass (short undergrowth, the feet of tall blades) is darker.
    let canopy = smoothstep(0.0, 0.45, in.above);
    s.occlusion = mix(0.22, 1.0, canopy);
    // Sunlight is blocked down there too (not just sky light).
    s.albedo = albedo * mix(0.45, 1.0, canopy);
    s.premultiply = false;
    s.draw = grass.draw.x;
    s.transmission = albedo * grass.seed.w * select(1.0, 0.6, seed_head) * mix(0.15, 1.0, canopy);
    return shade(in.world, s);
}
