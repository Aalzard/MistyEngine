// Grass blades: appended to mesh.wgsl (lit by `shade`, receives shadows). One instance per
// blade; seven vertices make a tapered strip (three segments and a tip) that sways in the wind
// and shrinks to nothing towards the draw distance.

struct GrassParams {
    // xyz = terrain corner in the world, w = time in seconds.
    origin: vec4<f32>,
    // rgb = linear healthy colour, w = draw distance.
    healthy: vec4<f32>,
    // rgb = linear dry colour, w = distance where the fade starts.
    dry: vec4<f32>,
    // x = strength, y = speed, zw = direction (xz).
    wind: vec4<f32>,
    // x = the terrain's draw slot (reflection probes).
    draw: vec4<u32>,
};

@group(2) @binding(0) var<uniform> grass: GrassParams;

struct BladeIn {
    // xyz = root (terrain-local), w = height.
    @location(0) position: vec4<f32>,
    // x = facing angle, y = width, z = dry mix, w = random phase.
    @location(1) shape: vec4<f32>,
};

struct GrassOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    // 0 at the root, 1 at the tip.
    @location(3) t: f32,
};

@vertex
fn vs_grass(@builtin(vertex_index) vi: u32, b: BladeIn) -> GrassOut {
    let t = min(f32(vi / 2u) / 3.0, 1.0);
    let side = select(-0.5, 0.5, (vi & 1u) == 1u);
    let root = grass.origin.xyz + b.position.xyz;
    let dist = distance(root, view.camera_pos.xyz);
    let fade = 1.0 - smoothstep(grass.dry.w, grass.healthy.w, dist);
    let height = b.position.w * fade;
    let across = vec3<f32>(cos(b.shape.x), 0.0, sin(b.shape.x));
    let facing = vec3<f32>(-across.z, 0.0, across.x);
    // Natural lean plus a gust wave travelling along the wind direction.
    let dir = vec3<f32>(grass.wind.z, 0.0, grass.wind.w);
    let phase = grass.origin.w * grass.wind.y + dot(root.xz, grass.wind.zw) * 0.18 + b.shape.w * 6.28;
    let gust = (sin(phase) * 0.7 + sin(phase * 2.3 + 1.7) * 0.3) * grass.wind.x;
    let bend = t * t;
    let width = b.shape.y * (1.0 - t * 0.85) * select(1.0, 0.0, vi >= 6u);
    var p = root + across * side * width + vec3<f32>(0.0, height * t * (1.0 - 0.15 * bend), 0.0);
    p += (facing * (0.15 + 0.1 * b.shape.w) + dir * gust) * height * bend;
    let to_camera = view.camera_pos.xyz - root;
    let n = facing * select(-1.0, 1.0, dot(facing, to_camera) > 0.0);
    var out: GrassOut;
    out.clip = view.view_proj * vec4<f32>(p, 1.0);
    out.world = p;
    // Mostly up-facing so blades light like the ground they grow on.
    out.normal = normalize(n * 0.5 + vec3<f32>(0.0, 1.0, 0.0));
    out.color = mix(grass.healthy.rgb, grass.dry.rgb, b.shape.z);
    out.t = t;
    return out;
}

@fragment
fn fs_grass(in: GrassOut) -> MeshTargets {
    var s: Surface;
    s.albedo = in.color * (0.55 + 0.45 * in.t);
    s.alpha = 1.0;
    s.metallic = 0.0;
    s.smoothness = 0.12;
    s.normal = normalize(in.normal);
    s.emission = vec3<f32>(0.0);
    s.receive_shadows = true;
    // Darker near the root (blades shade each other).
    s.occlusion = mix(0.45, 1.0, in.t);
    s.premultiply = false;
    s.draw = grass.draw.x;
    return shade(in.world, s);
}
