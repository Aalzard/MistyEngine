// Terrain trees, shared by the lit, shadow and prepass shaders (render/trees.rs): the tree
// uniform and textures (group 2), the instance and vertex layout, wind and impostor quads.
// Appended to grass_field.wgsl (the gust waves the grass rolls with, so trees and grass move
// together).
//
// Wind, in world space after the instance's turn and scale:
// - the trunk bends from its foot along the wind (height share², stronger in a gust);
// - each limb sways with its own phase, its side branches and leaf clusters with it (one
//   phase per limb, so nothing tears apart), more towards the tips (`wind.x`);
// - leaf clusters flutter along their normal, faster and smaller.
// The vertex stages evaluate it twice while TAA wants motion vectors (now and a frame ago).

struct TreeParams {
    // xyz = terrain origin (gusts are terrain-local, like the grass's), w = wind clock (s).
    origin: vec4<f32>,
    // x = wind strength, y = gust travel speed (m/s), zw = direction (xz, unit).
    wind: vec4<f32>,
    // x = gust strength, y = flutter (turbulence), z = gust wave length (m), w = seconds since
    // last frame (wind motion vectors; 0 = skip).
    gust: vec4<f32>,
    // rgb = linear leaf tint, w = translucency.
    leaf: vec4<f32>,
    // rgb = linear bark tint, w = tree height (m, unscaled).
    bark: vec4<f32>,
    // x = trunk bend, y = branch sway, z = leaf flutter, w = leaf colour variation.
    motion: vec4<f32>,
    // x = impostor width, y = height, z = bottom (m, unscaled), w = views around.
    impostor: vec4<f32>,
    // xyz = unit direction towards the shadow-casting light (impostor shadows), w = 1 with
    // TAA (fades use a per-frame noise).
    light: vec4<f32>,
    // x = crown shading (0..1).
    shade: vec4<f32>,
};

@group(2) @binding(0) var<uniform> tree: TreeParams;
// The draw's two textures: bark (albedo, normal map), leaves (atlas with the cut-out in
// alpha, normal map) or the impostor (albedo + coverage, view-frame normal + occlusion). One
// pair per bind group keeps the lit pass under 16 textures per stage.
@group(2) @binding(1) var tree_albedo: texture_2d<f32>;
@group(2) @binding(2) var tree_normal_map: texture_2d<f32>;
@group(2) @binding(3) var tree_sampler: sampler;

// Per instance: xyz = foot (world), w = turn about Y (radians); x = height scale,
// y = width scale, z = colour variation 0..1, w = LOD fade (0 none; > 0 keep noise < w;
// < 0 keep noise >= 1 + w).
struct TreeInstance {
    @location(5) foot: vec4<f32>,
    @location(6) scale: vec4<f32>,
};

struct TreeVertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // x = limb sway weight, y = limb phase, z = leaf flutter weight, w = height share.
    @location(3) wind: vec4<f32>,
    // x = ambient occlusion, y = cluster colour variation, z = 1 on leaves.
    @location(4) extra: vec4<f32>,
};

fn tree_turn(v: vec3<f32>, yaw: f32) -> vec3<f32> {
    let c = cos(yaw);
    let s = sin(yaw);
    return vec3<f32>(c * v.x + s * v.z, v.y, -s * v.x + c * v.z);
}

// Wind offset (world) of a point `p` (turned and scaled, relative to the foot) of a tree at
// `foot`, at wind clock `time`.
fn tree_wind(p: vec3<f32>, n: vec3<f32>, wind: vec4<f32>, foot: vec3<f32>, scale: vec4<f32>, time: f32) -> vec3<f32> {
    let dir2 = tree.wind.zw;
    let dir = vec3<f32>(dir2.x, 0.0, dir2.y);
    let side = vec3<f32>(-dir2.y, 0.0, dir2.x);
    let gust = grass_gust(foot.xz - tree.origin.xz, dir2, time, tree.wind.y, tree.gust.z);
    let strength = tree.wind.x;
    let height = max(tree.bark.w * scale.x, 1.0);
    // Each tree its own rhythm.
    let own = scale.z * 6.2831853 + dot(foot.xz, vec2<f32>(0.37, 0.61));
    let push = strength * (0.45 + 1.6 * tree.gust.x * gust);
    let h = wind.w;
    // Trunk: leans with the push and rocks slowly around it.
    let rock = sin(time * 0.75 + own) * 0.55 + sin(time * 1.9 + own * 1.7) * 0.2;
    let bend = (push + strength * 0.25 * rock) * h * h * height * 0.045 * tree.motion.x;
    var offset = dir * bend;
    offset.y -= bend * bend / height * 0.5;
    // Limbs: their own phase, pushed downwind and bobbing.
    let ph = wind.y * 6.2831853 + own;
    let sway = sin(time * (1.4 + 0.5 * wind.y) + ph) * 0.65 + sin(time * 2.9 + ph * 2.3) * 0.25;
    let limb = wind.x * scale.x * tree.motion.y * (push * 0.9 + strength * 0.6 * sway);
    offset += (dir * 0.7 + side * sway * 0.45 + vec3<f32>(0.0, 0.35, 0.0) * sway) * limb;
    // Leaf clusters flutter along their normal.
    let flutter = wind.z * tree.motion.z * (strength * (0.25 + 0.6 * gust) + 0.02 * tree.gust.y)
        * (0.6 + 0.4 * tree.gust.y);
    let f = sin(time * (9.0 + 3.0 * wind.y) + ph * 5.0 + dot(p, vec3<f32>(2.7, 1.3, 3.1)));
    offset += n * f * flutter * 0.35;
    return offset;
}

struct TreePoint {
    world: vec3<f32>,
    normal: vec3<f32>,
    // Last frame's world position (the wind a frame ago; = world without TAA).
    prev: vec3<f32>,
};

// A mesh vertex placed in the world, with the wind now and a frame ago.
fn tree_point(v: TreeVertexIn, inst: TreeInstance) -> TreePoint {
    let foot = inst.foot.xyz;
    let yaw = inst.foot.w;
    let scaled = vec3<f32>(v.position.x * inst.scale.y, v.position.y * inst.scale.x, v.position.z * inst.scale.y);
    let p = tree_turn(scaled, yaw);
    let n = normalize(tree_turn(v.normal * vec3<f32>(inst.scale.x, inst.scale.y, inst.scale.x), yaw));
    let time = tree.origin.w;
    var out: TreePoint;
    out.world = foot + p + tree_wind(p, n, v.wind, foot, inst.scale, time);
    out.normal = n;
    out.prev = out.world;
    if (tree.gust.w > 0.0) {
        out.prev = foot + p + tree_wind(p, n, v.wind, foot, inst.scale, time - tree.gust.w);
    }
    return out;
}

// Noise 0..1 per pixel (and per frame with TAA) for dithered LOD fades.
fn tree_noise(pixel: vec2<f32>) -> f32 {
    var p = floor(pixel);
    if (tree.light.w > 0.5) {
        // TAA's jitter changes every frame (8 phases): the history averages the pattern out.
        p += fract(view.motion.xy * vec2<f32>(4129.3, 2971.7)) * vec2<f32>(53.7, 91.3);
    }
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// Is this pixel dropped by the instance's LOD fade?
fn tree_faded(fade: f32, pixel: vec2<f32>) -> bool {
    if (fade == 0.0) {
        return false;
    }
    let t = tree_noise(pixel);
    if (fade > 0.0) {
        return t >= fade;
    }
    return t < 1.0 + fade;
}

struct ImpostorPoint {
    world: vec3<f32>,
    // Atlas uv of the two nearest baked views, and the blend towards the second.
    uv0: vec2<f32>,
    uv1: vec2<f32>,
    blend: f32,
    // Billboard frame (world): right and towards the viewer.
    right: vec3<f32>,
    toward: vec3<f32>,
};

// Corner `vi` (0..5, two triangles) of an impostor quad turned towards `eye_dir` (xz, from the
// tree towards the viewer or the light).
fn impostor_point(vi: u32, inst: TreeInstance, eye_dir: vec2<f32>) -> ImpostorPoint {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, 0.0), vec2<f32>(-0.5, 1.0), vec2<f32>(0.5, 0.0),
        vec2<f32>(0.5, 0.0), vec2<f32>(-0.5, 1.0), vec2<f32>(0.5, 1.0),
    );
    let c = corners[vi % 6u];
    var toward = vec3<f32>(eye_dir.x, 0.0, eye_dir.y);
    if (dot(toward, toward) < 1e-6) {
        toward = vec3<f32>(0.0, 0.0, 1.0);
    }
    toward = normalize(toward);
    let right = vec3<f32>(toward.z, 0.0, -toward.x);
    let foot = inst.foot.xyz;
    let width = tree.impostor.x * inst.scale.y;
    var out: ImpostorPoint;
    out.world = foot + right * c.x * width
        + vec3<f32>(0.0, (tree.impostor.z + c.y * tree.impostor.y) * inst.scale.x, 0.0);
    // The viewing direction in the tree's own frame picks the baked views.
    let local = tree_turn(toward, -inst.foot.w);
    let views = tree.impostor.w;
    var a = atan2(local.z, local.x) / 6.2831853;
    a = fract(a + 1.0) * views;
    let v0 = floor(a);
    let v1 = v0 + 1.0;
    out.blend = a - v0;
    let u = c.x + 0.5;
    out.uv0 = vec2<f32>((v0 % views + u) / views, 1.0 - c.y);
    out.uv1 = vec2<f32>((v1 % views + u) / views, 1.0 - c.y);
    out.right = right;
    out.toward = toward;
    return out;
}
