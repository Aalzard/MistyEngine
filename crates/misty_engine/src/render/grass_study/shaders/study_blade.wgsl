// Copy of render/shaders/grass_blade.wgsl (main a91ac1e) for the grass study. Changes:
// the grass group is group 1 (+ the blades as a storage buffer for compute passes), the card
// can be evaluated at any (t, side) (`grass_blade_ts`, for the visibility-buffer resolve),
// and two study knobs: CLUMP (variant D: fewer, wider multi-leaf cards past ~2 LOD
// distances) and the split widening of variant H (`study_split_widen`).

struct GrassParams {
    origin: vec4<f32>,
    healthy: vec4<f32>,
    dry: vec4<f32>,
    wind: vec4<f32>,
    seed: vec4<f32>,
    look: vec4<f32>,
    draw: vec4<u32>,
    lush: vec4<f32>,
    amounts: vec4<f32>,
    camera: vec4<f32>,
    shadow: vec4<f32>,
    screen: vec4<f32>,
    canopy: vec4<f32>,
};

@group(1) @binding(0) var<uniform> grass: GrassParams;
@group(1) @binding(1) var grass_atlas: texture_2d<f32>;
@group(1) @binding(2) var grass_sampler: sampler;
// The same blades as the vertex buffer, 18 floats each (terrain/details.rs `Blade`).
@group(1) @binding(3) var<storage, read> blade_data: array<f32>;

const CELL_ASPECT = array<f32, 8>(0.28, 0.28, 0.28, 0.28, 0.069, 0.057, 0.146, 0.151);
const GRASS_SEGMENTS: f32 = 4.0;
const MIN_KEEP: f32 = 0.012;

// Variant D: past 1.5–3 LOD distances keep CLUMP times fewer cards, each CLUMP times wider
// with up to 4 × CLUMP leaves side by side (terrain-study twin: `field.rs` `clump_at`).
override CLUMP: f32 = 1.0;

struct BladeIn {
    @location(0) position: vec4<f32>,
    @location(1) shape: vec4<f32>,
    @location(2) extra: vec4<f32>,
    @location(3) field: vec4<f32>,
    @location(4) slope: vec2<f32>,
};

fn load_blade(i: u32) -> BladeIn {
    let o = i * 18u;
    var b: BladeIn;
    b.position = vec4<f32>(blade_data[o], blade_data[o + 1u], blade_data[o + 2u], blade_data[o + 3u]);
    b.shape = vec4<f32>(blade_data[o + 4u], blade_data[o + 5u], blade_data[o + 6u], blade_data[o + 7u]);
    b.extra = vec4<f32>(blade_data[o + 8u], blade_data[o + 9u], blade_data[o + 10u], blade_data[o + 11u]);
    b.field = vec4<f32>(blade_data[o + 12u], blade_data[o + 13u], blade_data[o + 14u], blade_data[o + 15u]);
    b.slope = vec2<f32>(blade_data[o + 16u], blade_data[o + 17u]);
    return b;
}

struct BladeVertex {
    p: vec3<f32>,
    across: vec3<f32>,
    tangent: vec3<f32>,
    root: vec3<f32>,
    t: f32,
    side: f32,
    above: f32,
    gust: f32,
    dist: f32,
    fade: f32,
    cell: u32,
    seed_head: bool,
    thin: f32,
    repeat: f32,
    // Drawn width (m) before the one-pixel minimum, and the drawn height (m).
    width: f32,
    height: f32,
};

fn grass_card_u(cell: u32, side: f32, repeat: f32) -> f32 {
    return f32(cell) * 8.0 + (side + 0.5) * repeat;
}

fn grass_card_v(t: f32, phase: f32) -> f32 {
    return 1.0 - t + 2.0 * floor(phase * 16.0);
}

fn grass_atlas_uv(card: vec2<f32>) -> vec3<f32> {
    let cell = floor(card.x / 8.0);
    let local = card.x - cell * 8.0;
    let leaf = floor(local);
    let blade = floor(card.y * 0.5);
    let v = card.y - blade * 2.0;
    let h = grass_hash(vec2<f32>(leaf * 7.1 + cell * 1.3, blade * 3.3 + 1.7));
    let reach = select(1.0 - 0.35 * h, 1.0, leaf < 0.5);
    let along = 1.0 - (1.0 - v) / reach;
    return vec3<f32>((cell + fract(local)) / 8.0, max(along, 0.0), step(0.0, along));
}

fn grass_atlas_dx(card: vec2<f32>) -> vec2<f32> {
    let d = dpdx(card);
    return vec2<f32>(d.x / 8.0, d.y);
}

fn grass_atlas_dy(card: vec2<f32>) -> vec2<f32> {
    let d = dpdy(card);
    return vec2<f32>(d.x / 8.0, d.y);
}

// Variant D's extra thinning at `dist` (1 = none).
fn clump_at(dist: f32) -> f32 {
    return mix(1.0, CLUMP, smoothstep(grass.look.x * 1.5, grass.look.x * 3.0, dist));
}

fn grass_keep(dist: f32) -> f32 {
    let r = grass.look.x / max(dist, 1e-3);
    return clamp(r * r, MIN_KEEP, 1.0) * grass.canopy.w / clump_at(dist);
}

fn grass_drawn_height(b: BladeIn) -> f32 {
    let seed_head = b.extra.x >= 2.0;
    let key = b.extra.x - select(0.0, 2.0, seed_head);
    let root = grass.origin.xyz + b.position.xyz;
    let dist = distance(root, grass.camera.xyz);
    let fade = 1.0 - smoothstep(grass.dry.w, grass.healthy.w, dist);
    let shadow = grass.shadow.z > 0.5;
    let keep = grass_keep(dist) * select(1.0, grass.shadow.y, shadow);
    var height = b.position.w * fade * (1.0 - smoothstep(keep * 0.7, keep, key));
    if (shadow) {
        height *= 1.0 - smoothstep(grass.shadow.x * 0.6, grass.shadow.x, dist);
        if (grass.screen.z > 0.0) {
            height *= smoothstep(grass.screen.z * 0.7, grass.screen.z, dist);
        }
    }
    return height;
}

fn grass_flat(b: BladeIn) -> bool {
    return grass_drawn_height(b) <= 0.0;
}

fn grass_outside(b: BladeIn, m: mat4x4<f32>, near: bool) -> bool {
    let root = vec4<f32>(grass.origin.xyz + b.position.xyz, 1.0);
    let dist = distance(root.xyz, grass.camera.xyz);
    let reach = b.position.w * 1.6 + 0.25 + dist * grass.screen.x * 2.0;
    let r0 = vec4<f32>(m[0].x, m[1].x, m[2].x, m[3].x);
    let r1 = vec4<f32>(m[0].y, m[1].y, m[2].y, m[3].y);
    let r2 = vec4<f32>(m[0].z, m[1].z, m[2].z, m[3].z);
    let r3 = vec4<f32>(m[0].w, m[1].w, m[2].w, m[3].w);
    let planes = array<vec4<f32>, 5>(r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2);
    let count = select(4u, 5u, near);
    for (var i = 0u; i < count; i++) {
        let pl = planes[i];
        if (dot(pl, root) < -reach * length(pl.xyz)) {
            return true;
        }
    }
    return false;
}

fn grass_blade(vi: u32, b: BladeIn) -> BladeVertex {
    return grass_blade_at(vi, b, grass.origin.w);
}

fn grass_blade_at(vi: u32, b: BladeIn, time: f32) -> BladeVertex {
    let coarse = vi >= 100u;
    let k = select(vi, vi - 100u, coarse);
    let t = f32(k / 2u) / select(GRASS_SEGMENTS, 3.0, coarse);
    let side = select(-0.5, 0.5, (k & 1u) == 1u);
    return grass_blade_ts(t, side, b, time);
}

// The card at (t 0 root .. 1 tip, side -0.5 .. 0.5), as `grass_blade_at` has its vertices.
fn grass_blade_ts(t: f32, side: f32, b: BladeIn, time: f32) -> BladeVertex {
    let seed_head = b.extra.x >= 2.0;
    let phase = b.shape.w;
    var cell = u32(phase * 4.0) & 3u;
    if (seed_head) {
        cell = 6u + (u32(fract(phase * 7.31) * 2.0) & 1u);
    }

    let root = grass.origin.xyz + b.position.xyz;
    let dist = distance(root, grass.camera.xyz);
    let fade = 1.0 - smoothstep(grass.dry.w, grass.healthy.w, dist);
    let shadow = grass.shadow.z > 0.5;
    let keep = grass_keep(dist) * select(1.0, grass.shadow.y, shadow);
    let height = grass_drawn_height(b);
    let widen = min(1.0 / keep, grass.look.z * clump_at(dist));
    var width = b.shape.y;
    if (seed_head) {
        width = b.position.w * CELL_ASPECT[cell] * (0.8 + 0.4 * fract(phase * 3.7));
    }
    width *= widen;
    if (!shadow && !seed_head) {
        let down = (grass.camera.y - root.y) / max(dist, 1e-3);
        width *= 1.0 + 0.3 * smoothstep(0.2, 0.9, down);
    }
    if (!shadow) {
        width *= study_split_widen(b, dist);
    }
    let drawn_width = width;
    var thin = 1.0;
    let least = dist * grass.screen.x * 0.8;
    if (!shadow && width < least) {
        thin = width / least;
        width = least;
    }
    var repeat = 1.0;
    if (!seed_head) {
        let natural = max(b.position.w * CELL_ASPECT[cell], 1e-4);
        var fit = width / natural;
        if (!shadow && grass.screen.x > 0.0) {
            fit = min(fit, width / (least * 2.5));
        }
        repeat = clamp(round(fit), 1.0, 4.0 * clump_at(dist));
    }

    let dir = grass.wind.zw;
    let gust = grass_gust(root.xz, dir, time, grass.wind.y, grass.camera.w);
    let flutter = sin(time * (4.5 + 3.0 * phase) + phase * 40.0 - dot(root.xz, dir) * 1.3);
    let give = clamp(b.position.w / 0.45, 0.35, 1.4) * select(1.0, 0.7, seed_head);
    let push = grass.wind.x * give
        * (0.2 + 1.3 * grass.amounts.z * gust + 0.15 * grass.amounts.w * flutter);
    let lean = vec2<f32>(cos(b.extra.z), sin(b.extra.z)) * b.extra.y;
    let sway = lean + dir * push;
    let amount = length(sway);
    let theta = min(amount * 2.0, 1.5);
    var bend = vec2<f32>(1.0, 0.0);
    if (amount > 1e-5) {
        bend = sway / amount;
    }
    let a = theta * t;
    var along = 0.5 * theta * t * t;
    var up = t;
    if (theta > 1e-3) {
        along = (1.0 - cos(a)) / theta;
        up = sin(a) / theta;
    }
    let bend3 = vec3<f32>(bend.x, 0.0, bend.y);
    let centre = root + (bend3 * along + vec3<f32>(0.0, up, 0.0)) * height;
    let tangent = normalize(bend3 * sin(a) + vec3<f32>(0.0, cos(a), 0.0));
    var facing_angle = b.extra.z + 1.5708 + (b.shape.x - 3.14159) * 0.25;
    if (seed_head) {
        facing_angle = b.shape.x;
    }
    var across = vec3<f32>(cos(facing_angle), 0.0, sin(facing_angle));
    across = normalize(across - tangent * dot(across, tangent) + vec3<f32>(1e-4, 0.0, 0.0));
    if (!seed_head) {
        let twist = (fract(phase * 5.3) - 0.5) * 0.9 * t;
        across = across * cos(twist) + cross(tangent, across) * sin(twist);
    }

    var v: BladeVertex;
    v.p = centre + across * side * width;
    v.across = across;
    v.tangent = tangent;
    v.root = root;
    v.t = t;
    v.side = side;
    v.above = up * height;
    v.gust = gust;
    v.dist = dist;
    v.fade = fade;
    v.cell = cell;
    v.seed_head = seed_head;
    v.thin = thin;
    v.repeat = repeat;
    v.width = drawn_width;
    v.height = height;
    return v;
}
