// Froxel grid shared by the volumetric fog compute passes (render/volumetrics/froxels.rs).
// The grid is camera aligned: x/y follow the screen, z is exponential in view depth, so slice
// coordinate w in (0, 1] sits at depth near * (far / near)^w and slice 0 starts at the eye.

struct FroxelLight {
    // xyz = position, w = range.
    pos_range: vec4<f32>,
    // xyz = direction the light travels, w = type (0 directional, 1 point, 2 spot).
    dir_type: vec4<f32>,
    // rgb = linear colour × intensity × volumetric multiplier.
    color: vec4<f32>,
    // x = cos(outer half angle), y = cos(inner half angle).
    spot: vec4<f32>,
    // x = 1 when shadowed, y = shadow layer (-1 = directional map), z = shadow dimmer.
    shadow: vec4<f32>,
};

struct FogBox {
    // World → box space where the box is [-1, 1]^3.
    world_to_box: mat4x4<f32>,
    // xyz = half extents (world units), w = extinction (1/m).
    half_ext: vec4<f32>,
    // rgb = linear albedo, w = soft edge width (m).
    albedo: vec4<f32>,
};

struct Froxels {
    inv_view_proj: mat4x4<f32>,
    prev_view_proj: mat4x4<f32>,
    sun_shadow: mat4x4<f32>,
    // xyz = eye, w = 1 when orthographic.
    eye: vec4<f32>,
    forward: vec4<f32>,
    prev_eye: vec4<f32>,
    // xyz = previous forward, w = history weight (0 = no history).
    prev_forward: vec4<f32>,
    // xyz = grid size, w = frame index.
    grid: vec4<f32>,
    // x = near slice depth, y = log(far / near), z = far.
    slices: vec4<f32>,
    // xyz = sample position inside each froxel this frame (0..1).
    jitter: vec4<f32>,
    // x = extinction at the base height, y = base height, z = 1 / falloff height, w = anisotropy.
    height_fog: vec4<f32>,
    // rgb = global fog albedo, w = 1 when the global fog is on.
    albedo: vec4<f32>,
    // rgb = ambient light × dimmer.
    ambient: vec4<f32>,
    // x = noise intensity, y = 1 / noise scale, z = time (s).
    noise: vec4<f32>,
    // xyz = noise scroll (m/s).
    scroll: vec4<f32>,
    // x = light count, y = fog box count.
    counts: vec4<f32>,
    lights: array<FroxelLight, 8>,
    // Spot: one layer, point: six (+X, -X, +Y, -Y, +Z, -Z).
    local_shadow: array<mat4x4<f32>, 16>,
    boxes: array<FogBox, 16>,
};

@group(0) @binding(0) var<uniform> fx: Froxels;

const PI: f32 = 3.14159265;

// View depth of slice coordinate `w` (0 = the eye).
fn slice_depth(w: f32) -> f32 {
    if (w <= 0.0) {
        return 0.0;
    }
    return fx.slices.x * exp(fx.slices.y * w);
}

// Slice coordinate of view depth `d` (inverse of `slice_depth` past the near slice).
fn depth_slice(d: f32) -> f32 {
    return log(max(d, fx.slices.x) / fx.slices.x) / fx.slices.y;
}

fn unproject_froxel(ndc: vec2<f32>, z: f32) -> vec3<f32> {
    let p = fx.inv_view_proj * vec4<f32>(ndc, z, 1.0);
    return p.xyz / p.w;
}

// Unit ray through screen uv (y down).
fn froxel_ray(uv: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    return normalize(unproject_froxel(ndc, 1.0) - unproject_froxel(ndc, 0.0));
}

// World point at screen uv and view depth `d`.
fn froxel_world(uv: vec2<f32>, d: f32) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let near = unproject_froxel(ndc, 0.0);
    let ray = normalize(unproject_froxel(ndc, 1.0) - near);
    let near_depth = dot(near - fx.eye.xyz, fx.forward.xyz);
    return near + ray * ((d - near_depth) / max(dot(ray, fx.forward.xyz), 1e-4));
}
