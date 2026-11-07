// Grass study: the ground under the grass (terrain stand-in: the grass colour under the turf,
// lit like it) and, for variant E, the far field: past `study.params.y` metres the blades
// are gone and the ground pass marches a thin layer of procedural grass instead (one
// parallax pass, shaded once per pixel).

override FAR_FIELD: bool = false;

struct GroundOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_ground(@location(0) p: vec3<f32>) -> GroundOut {
    var out: GroundOut;
    out.clip = study.view_proj * vec4<f32>(p, 1.0);
    out.world = p;
    return out;
}

fn ground_color(world: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    let local = world.xz - grass.origin.xz;
    let patches = grass_patches(local, grass.lush.w);
    let dist = distance(world, study.camera.xyz);
    var base = grass_base_color(patches, 0.3, grass.healthy.rgb, grass.dry.rgb, grass.lush.rgb, grass.amounts.xy);
    base = grass_swathe_tint(base, grass_swathe(local, grass.lush.w)) * grass_far_tint(dist);
    let soil = vec3<f32>(0.09, 0.07, 0.045);
    let albedo = mix(soil, base * 0.55, grass.shadow.w);
    var color = study_shade(world, pixel, albedo, vec3<f32>(0.0, 1.0, 0.0), 0.45, albedo * 0.2, 0.03, 0.0);
    if (FAR_FIELD) {
        let start = study.params.y;
        let w = smoothstep(start * 0.8, start, dist);
        if (w > 0.0) {
            color = mix(color, far_field(world, pixel, local, dist), w);
        }
    }
    return color;
}

// Variant E: march the view ray through a grass layer over the ground (blades on a 4 cm
// grid, each its own height and lean; tufts of 0.25 m set the hue) and shade the first hit.
fn far_field(world: vec3<f32>, pixel: vec2<f32>, local: vec2<f32>, dist: f32) -> vec3<f32> {
    let d = normalize(world - study.camera.xyz);
    let down = max(-d.y, 0.02);
    let top = grass.canopy.y * 0.85;
    // Ray from where it enters the layer down to the ground point.
    let span = top / down;
    let steps = 10u;
    var hit_t = -1.0;
    var cell_id = 0.0;
    var hit_h = 0.0;
    let cell = 0.04;
    for (var i = 0u; i < steps; i++) {
        let f = (f32(i) + 0.5) / f32(steps);
        let p = local - d.xz * span * (1.0 - f);
        let h = top * (1.0 - f);
        let g = floor(p / cell);
        let r = study_hash(g * 1.7 + 0.3);
        let bh = top * (0.45 + 0.55 * study_hash(g + 11.1));
        // Blades thin out towards their tips (fewer of them reach this height).
        if (h < bh && r < mix(0.95, 0.35, h / bh)) {
            hit_t = h / bh;
            hit_h = h;
            cell_id = study_hash(floor(p / 0.25) + 5.7);
            break;
        }
    }
    let patches = grass_patches(local, grass.lush.w);
    var base = grass_base_color(patches, 0.2 + 0.4 * cell_id, grass.healthy.rgb, grass.dry.rgb, grass.lush.rgb, grass.amounts.xy);
    base = grass_swathe_tint(base, grass_swathe(local, grass.lush.w)) * grass_far_tint(dist);
    let tint = (cell_id - 0.5) * 2.0 * 0.6;
    let cv = grass.look.w * 0.5;
    base *= vec3<f32>(1.0 + 0.22 * tint * cv, 1.0, 1.0 - 0.32 * tint * cv);
    if (hit_t < 0.0) {
        // Down to the ground between the blades: the turf's dark feet.
        hit_t = 0.0;
    }
    let t = hit_t;
    var albedo = mix(base, grass.dry.rgb, smoothstep(0.45, 1.0, t) * grass.look.y * 0.7);
    albedo *= mix(vec3<f32>(0.82, 0.95, 0.78), vec3<f32>(1.1, 1.08, 1.0), t);
    let canopy = smoothstep(0.0, 0.85, hit_h / max(top, 0.01));
    let relief = grass_canopy(local);
    let dip = mix(0.84, 1.06, relief.x);
    let occlusion = mix(0.4, 1.0, canopy) * dip;
    let n = normalize(vec3<f32>(-relief.y * 8.0 * (grass.canopy.y - grass.canopy.x), 1.0, -relief.z * 8.0 * (grass.canopy.y - grass.canopy.x)));
    let lit = albedo * mix(0.62, 1.0, canopy) * dip;
    let transmission = albedo * grass.seed.w * mix(0.15, 1.0, canopy);
    let world_hit = vec3<f32>(world.x, world.y + hit_h, world.z);
    return study_shade(world_hit, pixel, lit, n, occlusion, transmission, 0.04, canopy * 2.0);
}

@fragment
fn fs_ground_mrt(in: GroundOut) -> MeshTargets {
    var out: MeshTargets;
    out.color = vec4<f32>(ground_color(in.world, in.clip.xy), 1.0);
    out.glow = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    out.velocity = vec2<f32>(0.0);
    return out;
}

@fragment
fn fs_ground(in: GroundOut) -> @location(0) vec4<f32> {
    return vec4<f32>(ground_color(in.world, in.clip.xy), 1.0);
}
