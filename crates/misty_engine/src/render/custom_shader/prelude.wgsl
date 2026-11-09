// Custom shader prelude: inputs, outputs and helpers shared by every `.shader` asset.
// Comes after common.wgsl and mesh.wgsl (view, lights, draws, shade()).

struct Globals {
    // x = seconds, y = sin(x), z = cos(x), w = frame delta.
    time: vec4<f32>,
    // x = width, y = height in pixels, zw = 1 / size.
    screen: vec4<f32>,
};

@group(3) @binding(0) var<uniform> globals: Globals;
// The opaque scene (linear HDR colour, device depth) for queue >= 3000; 1×1 placeholders otherwise.
@group(3) @binding(1) var scene_color: texture_2d<f32>;
@group(3) @binding(2) var scene_depth: texture_2d<f32>;
@group(3) @binding(3) var scene_sampler: sampler;

// What `fn vertex(v: VertexData) -> VertexData` may change (object space).
struct VertexData {
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    // xyz = tangent, w = bitangent sign.
    tangent: vec4<f32>,
    // Index into `draws` (read only): draws[v.draw].model is the object's matrix.
    draw: u32,
    // The mesh's vertex colour (linear RGBA; white when the mesh has none). Changes reach the
    // fragment stage as SurfaceInput.vertex_color.
    color: vec4<f32>,
};

struct SurfaceInput {
    world_pos: vec3<f32>,
    // Unit, flipped on back faces.
    world_normal: vec3<f32>,
    world_tangent: vec4<f32>,
    uv: vec2<f32>,
    // Unit vector from the surface towards the camera.
    view_dir: vec3<f32>,
    // 0..1 across the view, (0, 0) top left.
    screen_uv: vec2<f32>,
    // Pixel coordinates.
    screen_pos: vec2<f32>,
    // Device depth of this fragment (0 near .. 1 far).
    depth: f32,
    front_facing: bool,
    draw: u32,
    // Seconds (globals.time.x).
    time: f32,
    // Interpolated vertex colour (linear RGBA; white when the mesh has none).
    vertex_color: vec4<f32>,
};

struct SurfaceOutput {
    // Linear base colour.
    albedo: vec3<f32>,
    // World-space unit normal (tangent_to_world() turns a normal map sample into one).
    normal: vec3<f32>,
    metallic: f32,
    smoothness: f32,
    // Linear emitted light; values above 1 bloom.
    emission: vec3<f32>,
    alpha: f32,
    // Ambient occlusion (1 = none).
    occlusion: f32,
    // Light coming through thin surfaces from behind, tinted (0 = none).
    transmission: vec3<f32>,
    // Optional lighting features: writing one of these compiles its lighting in.
    // Clear coat strength (0 = none) and smoothness.
    coat: f32,
    coat_smoothness: f32,
    // Anisotropy (-1..1; negative stretches across) and the unit world direction highlights
    // stretch along (the tangent by default).
    anisotropy: f32,
    anisotropy_direction: vec3<f32>,
    // Tint of the wrap lighting that reaches round the terminator (0 = none; skin, leaves).
    subsurface: vec3<f32>,
};

fn default_output(input: SurfaceInput) -> SurfaceOutput {
    var o: SurfaceOutput;
    o.albedo = vec3<f32>(1.0);
    o.normal = input.world_normal;
    o.metallic = 0.0;
    o.smoothness = 0.5;
    o.emission = vec3<f32>(0.0);
    o.alpha = 1.0;
    o.occlusion = 1.0;
    o.transmission = vec3<f32>(0.0);
    o.coat = 0.0;
    o.coat_smoothness = 1.0;
    o.anisotropy = 0.0;
    o.anisotropy_direction = tangent_to_world(input, vec3<f32>(1.0, 0.0, 0.0));
    o.subsurface = vec3<f32>(0.0);
    return o;
}

// Tangent-space normal from a normal map sample (OpenGL convention, green = up).
fn unpack_normal(sample: vec4<f32>, scale: f32) -> vec3<f32> {
    // X and Y from the map, Z rebuilt: BC5-compressed normal maps store only X and Y.
    let xy = sample.xy * 2.0 - 1.0;
    let z = sqrt(saturate(1.0 - dot(xy, xy)));
    return normalize(vec3<f32>(xy * scale, max(z, 1e-3)));
}

// Tangent-space normal to world space.
fn tangent_to_world(input: SurfaceInput, n: vec3<f32>) -> vec3<f32> {
    let nn = input.world_normal;
    let t0 = input.world_tangent.xyz - nn * dot(nn, input.world_tangent.xyz);
    if (dot(t0, t0) < 1e-10) {
        return nn;
    }
    let t = normalize(t0);
    let b = cross(nn, t) * input.world_tangent.w;
    return normalize(t * n.x + b * n.y + nn * n.z);
}

fn object_to_world(input: SurfaceInput) -> mat4x4<f32> {
    return draws[input.draw].model;
}

// Fog applied to a colour at a world position: the Lighting window's, then the volumetric /
// height fog and the clouds' haze (fog_apply.wgsl), as the built-in shaders do.
fn apply_fog(color: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let c = mix(view.fog_color.rgb, color, fog_visibility(fog_distance(world)));
    return vfog_blend(c, vfog_eval(world), 1.0);
}

fn scene_pixel(uv: vec2<f32>, size: vec2<u32>) -> vec2<i32> {
    let p = vec2<i32>(uv * vec2<f32>(size));
    return clamp(p, vec2<i32>(0), vec2<i32>(size) - 1);
}

// Opaque scene colour behind a screen point (linear HDR).
fn scene_color_at(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(scene_color, scene_sampler, uv, 0.0).rgb;
}

// Device depth of the opaque scene at a screen point (1 = nothing / sky).
fn scene_depth_at(uv: vec2<f32>) -> f32 {
    return textureLoad(scene_depth, scene_pixel(uv, textureDimensions(scene_depth)), 0).r;
}

// World position of the opaque scene at a screen point.
fn scene_world_pos(uv: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    return unproject(ndc, scene_depth_at(uv));
}

// Distance from the camera to the opaque scene at a screen point.
fn scene_distance(uv: vec2<f32>) -> f32 {
    return distance(scene_world_pos(uv), view.camera_pos.xyz);
}

fn misty_surface_input(in: MeshOut, front: bool) -> SurfaceInput {
    var s: SurfaceInput;
    s.world_pos = in.world;
    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    s.world_normal = n;
    let t = in.tangent.xyz;
    s.world_tangent = vec4<f32>(t * inverseSqrt(max(dot(t, t), 1e-12)), in.tangent.w);
    s.uv = in.uv;
    if (view.camera_pos.w > 0.5) {
        s.view_dir = normalize(-(view.inv_view_proj * vec4<f32>(0.0, 0.0, 1.0, 0.0)).xyz);
    } else {
        s.view_dir = normalize(view.camera_pos.xyz - in.world);
    }
    s.screen_pos = in.clip.xy;
    s.screen_uv = in.clip.xy * globals.screen.zw;
    s.depth = in.clip.z;
    s.front_facing = front;
    s.draw = in.draw;
    s.time = globals.time.x;
    s.vertex_color = vec4<f32>(1.0);
    return s;
}

