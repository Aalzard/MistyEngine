// Grass blade geometry shared by the lit pass (grass.wgsl) and the shadow casters
// (grass_shadow.wgsl). One instance per blade or seed-head stem; ten vertices make a curved
// card of four segments (eight from vertex 100 on make a coarse one of three). The card
// bends along a circular arc, so it keeps its length however far the wind pushes it.
// Uses grass_field.wgsl (patches, gusts).

struct GrassParams {
    // xyz = terrain corner in the world, w = time in seconds.
    origin: vec4<f32>,
    // rgb = linear healthy colour, w = draw distance.
    healthy: vec4<f32>,
    // rgb = linear dry colour, w = distance where the fade starts.
    dry: vec4<f32>,
    // x = strength, y = gust travel speed (m/s), zw = direction (xz, unit).
    wind: vec4<f32>,
    // rgb = linear seed-head colour, w = translucency.
    seed: vec4<f32>,
    // x = full-density distance, y = tip dryness, z = most widening, w = colour variation.
    look: vec4<f32>,
    // x = the terrain's draw slot (reflection probes).
    draw: vec4<u32>,
    // rgb = linear lush colour, w = patch size (m).
    lush: vec4<f32>,
    // x = lush amount, y = dry amount, z = gust strength, w = turbulence.
    amounts: vec4<f32>,
    // xyz = camera position (LOD distances in every pass), w = gust wave length (m).
    camera: vec4<f32>,
    // x = caster range (m), y = share of the blades that cast, z = 1 in shadow passes,
    // w = ground tint.
    shadow: vec4<f32>,
    // x = angle of one pixel (radians, 0 = orthographic), y = 1 with TAA, z = radius of the
    // near-camera grass shadow map (cascade casters leave its inside to it; 0 = none),
    // w = seconds since last frame (the wind's motion vectors).
    screen: vec4<f32>,
    // x = shortest and y = tallest blade of the turf (m), z = height variation, w = the
    // quality level's share of the blades (the rest grow wider).
    canopy: vec4<f32>,
};

@group(2) @binding(0) var<uniform> grass: GrassParams;
@group(2) @binding(1) var grass_atlas: texture_2d<f32>;
@group(2) @binding(2) var grass_sampler: sampler;

// Width / height of the specimen in each atlas cell, and its average linear colour.
// The leaf cells hold clumps of thin blades: their width is the clump's.
const CELL_ASPECT = array<f32, 8>(0.28, 0.28, 0.28, 0.28, 0.069, 0.057, 0.146, 0.151);
const GRASS_SEGMENTS: f32 = 4.0;
const MIN_KEEP: f32 = 0.02;

struct BladeIn {
    // xyz = root (terrain-local), w = height.
    @location(0) position: vec4<f32>,
    // x = facing angle, y = width, z = dry mix, w = random phase.
    @location(1) shape: vec4<f32>,
    // x = drop-out key (+ 2 for a seed head), y = bend, z = lean angle, w = tuft tint.
    @location(2) extra: vec4<f32>,
};

struct BladeVertex {
    // World position of the vertex.
    p: vec3<f32>,
    across: vec3<f32>,
    // Along the blade towards the tip.
    tangent: vec3<f32>,
    root: vec3<f32>,
    // 0 at the root .. 1 at the tip; -0.5 / 0.5 across.
    t: f32,
    side: f32,
    // Height of this vertex above the root (m).
    above: f32,
    // 0 calm .. 1 in a gust.
    gust: f32,
    // Distance to the camera, and the distance fade (1 = full size).
    dist: f32,
    fade: f32,
    cell: u32,
    seed_head: bool,
    // Real width / drawn width: blades thinner than a pixel are drawn a pixel wide and this
    // much transparent (no stippled coverage on thin stems).
    thin: f32,
    // Leaves side by side on the card (see `grass_card_u`).
    repeat: f32,
};

// Card coordinate across a blade: 8 × atlas cell + 0..`repeat` from edge to edge. Cards much
// wider than their specimen (undergrowth, blades widened to cover the ground) show `repeat`
// narrow leaves side by side instead of one stretched strap.
fn grass_card_u(cell: u32, side: f32, repeat: f32) -> f32 {
    return f32(cell) * 8.0 + (side + 0.5) * repeat;
}

// Card coordinate along a blade: atlas v (0 at the tip .. 1 at the root) + 2 × a random
// blade number, so the leaves of a card can differ from blade to blade.
fn grass_card_v(t: f32, phase: f32) -> f32 {
    return 1.0 - t + 2.0 * floor(phase * 16.0);
}

// Atlas uv of an interpolated card coordinate (`grass_card_u`, `grass_card_v`) and z = 1
// where a leaf is (0 above the tip of a shortened one): the first leaf of a card has the
// full length, the others 65–100 % of it, so a card reads as a ragged tuft, not a comb.
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

// Screen gradients of the atlas uv for `grass_atlas_uv` (taken on the card coordinate, so the
// leaf seams do not pick the smallest mip).
fn grass_atlas_dx(card: vec2<f32>) -> vec2<f32> {
    let d = dpdx(card);
    return vec2<f32>(d.x / 8.0, d.y);
}

fn grass_atlas_dy(card: vec2<f32>) -> vec2<f32> {
    let d = dpdy(card);
    return vec2<f32>(d.x / 8.0, d.y);
}

// Share of blades drawn at `dist` (terrain_grass.rs `keep_at` × the quality's density).
fn grass_keep(dist: f32) -> f32 {
    let r = grass.look.x / max(dist, 1e-3);
    return clamp(r * r, MIN_KEEP, 1.0) * grass.canopy.w;
}

fn grass_blade(vi: u32, b: BladeIn) -> BladeVertex {
    return grass_blade_at(vi, b, grass.origin.w);
}

// The blade as it stood at `time` (last frame's position for the motion vectors).
fn grass_blade_at(vi: u32, b: BladeIn, time: f32) -> BladeVertex {
    let coarse = vi >= 100u;
    let k = select(vi, vi - 100u, coarse);
    let t = f32(k / 2u) / select(GRASS_SEGMENTS, 3.0, coarse);
    let side = select(-0.5, 0.5, (k & 1u) == 1u);
    let seed_head = b.extra.x >= 2.0;
    let key = b.extra.x - select(0.0, 2.0, seed_head);
    let phase = b.shape.w;
    var cell = u32(phase * 4.0) & 3u;
    if (seed_head) {
        // The open panicles only: thin pale stalks over short turf.
        cell = 6u + (u32(fract(phase * 7.31) * 2.0) & 1u);
    }

    let root = grass.origin.xyz + b.position.xyz;
    let dist = distance(root, grass.camera.xyz);
    let fade = 1.0 - smoothstep(grass.dry.w, grass.healthy.w, dist);
    // Fewer blades far away: each blade shrinks away as the share kept at its own distance
    // drops past its key (terrain_grass.rs draws the prefix of each key-sorted chunk its
    // nearest point needs), so thinning is smooth instead of popping. Shadow casters are a
    // fixed share of the drawn blades and fade out towards their range.
    let shadow = grass.shadow.z > 0.5;
    let keep = grass_keep(dist) * select(1.0, grass.shadow.y, shadow);
    var height = b.position.w * fade * (1.0 - smoothstep(keep * 0.7, keep, key));
    if (shadow) {
        height *= 1.0 - smoothstep(grass.shadow.x * 0.6, grass.shadow.x, dist);
        if (grass.screen.z > 0.0) {
            height *= smoothstep(grass.screen.z * 0.7, grass.screen.z, dist);
        }
    }
    // Fewer blades cover the same ground by growing wider: as much wider as there are fewer
    // of them, so the blade area per square metre (and the ground they hide) stays the same.
    let widen = min(1.0 / keep, grass.look.z);
    var width = b.shape.y;
    if (seed_head) {
        width = b.position.w * CELL_ASPECT[cell] * (0.8 + 0.4 * fract(phase * 3.7));
    }
    width *= widen;
    // Seen from above, cards show their thin side and the ground between them: blades the
    // camera looks down on grow wider (the main pass only; shadows keep their shape).
    if (!shadow && !seed_head) {
        let down = (grass.camera.y - root.y) / max(dist, 1e-3);
        width *= 1.0 + 0.3 * smoothstep(0.2, 0.9, down);
    }
    var thin = 1.0;
    let least = dist * grass.screen.x * 0.8;
    if (!shadow && width < least) {
        thin = width / least;
        width = least;
    }
    // As many leaves across as the card is wider than its specimen, each at least two
    // pixels wide (the shadow pass, without a pixel size, follows the specimen alone).
    var repeat = 1.0;
    if (!seed_head) {
        let natural = max(b.position.w * CELL_ASPECT[cell], 1e-4);
        var fit = width / natural;
        if (!shadow && grass.screen.x > 0.0) {
            fit = min(fit, width / (least * 2.5));
        }
        repeat = clamp(round(fit), 1.0, 4.0);
    }

    // Wind: gust waves rolling across the field, and a fast flutter whose phase runs along
    // the wind; tall blades give more than short stiff ones, seed stems least.
    let dir = grass.wind.zw;
    let gust = grass_gust(root.xz, dir, time, grass.wind.y, grass.camera.w);
    let flutter = sin(time * (4.5 + 3.0 * phase) + phase * 40.0 - dot(root.xz, dir) * 1.3);
    let give = clamp(b.position.w / 0.45, 0.35, 1.4) * select(1.0, 0.7, seed_head);
    let push = grass.wind.x * give
        * (0.2 + 1.3 * grass.amounts.z * gust + 0.15 * grass.amounts.w * flutter);
    // Lean out of the tuft (arched by `bend`) plus the wind; the card bends along an arc
    // whose total angle grows with both.
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
    // Blades turn their face into the lean; seed stems face anywhere.
    var facing_angle = b.extra.z + 1.5708 + (b.shape.x - 3.14159) * 0.25;
    if (seed_head) {
        facing_angle = b.shape.x;
    }
    var across = vec3<f32>(cos(facing_angle), 0.0, sin(facing_angle));
    across = normalize(across - tangent * dot(across, tangent) + vec3<f32>(1e-4, 0.0, 0.0));
    // A slight twist along the blade (each its own way), so no card is a flat strap.
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
    return v;
}
