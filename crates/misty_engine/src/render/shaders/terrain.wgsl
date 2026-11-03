// Terrain: appended to mesh.wgsl (same vertex stage, draws and lights; lit by `shade`).
// The splat map's RGBA channels weight up to four tiled layers (albedo + normal map each).

struct TerrainParams {
    // xyz = size (width, height, length), w = layer count.
    size: vec4<f32>,
    // Per layer: xy = tile size (metres), zw = tile offset (metres).
    tiles: array<vec4<f32>, 4>,
    // Per layer: x = metallic, y = smoothness, z = normal scale (0 = no normal map).
    surface: array<vec4<f32>, 4>,
};

@group(3) @binding(0) var<uniform> terrain: TerrainParams;
@group(3) @binding(1) var splat_map: texture_2d<f32>;
@group(3) @binding(2) var splat_sampler: sampler;
@group(3) @binding(3) var layer_albedo0: texture_2d<f32>;
@group(3) @binding(4) var layer_albedo1: texture_2d<f32>;
@group(3) @binding(5) var layer_albedo2: texture_2d<f32>;
@group(3) @binding(6) var layer_albedo3: texture_2d<f32>;
@group(3) @binding(7) var layer_normal0: texture_2d<f32>;
@group(3) @binding(8) var layer_normal1: texture_2d<f32>;
@group(3) @binding(9) var layer_normal2: texture_2d<f32>;
@group(3) @binding(10) var layer_normal3: texture_2d<f32>;
@group(3) @binding(11) var layer_sampler: sampler;

// Texture coordinates of layer `i` from the terrain's 0..1 uv.
fn layer_uv(uv: vec2<f32>, i: u32) -> vec2<f32> {
    let t = terrain.tiles[i];
    return (uv * terrain.size.xz + t.zw) / t.xy;
}

// Tangent-space normal of a layer's normal map sample, scaled (flat when the scale is 0).
fn layer_normal(sampled: vec3<f32>, scale: f32) -> vec3<f32> {
    let m = sampled * 2.0 - 1.0;
    return vec3<f32>(m.xy * scale, max(m.z, 1e-3));
}

@fragment
fn fs_terrain(in: MeshOut) -> MeshTargets {
    // Sample everything first (implicit derivatives need uniform control flow).
    var w = textureSample(splat_map, splat_sampler, in.uv);
    let a0 = textureSample(layer_albedo0, layer_sampler, layer_uv(in.uv, 0u)).rgb;
    let a1 = textureSample(layer_albedo1, layer_sampler, layer_uv(in.uv, 1u)).rgb;
    let a2 = textureSample(layer_albedo2, layer_sampler, layer_uv(in.uv, 2u)).rgb;
    let a3 = textureSample(layer_albedo3, layer_sampler, layer_uv(in.uv, 3u)).rgb;
    let n0 = textureSample(layer_normal0, layer_sampler, layer_uv(in.uv, 0u)).xyz;
    let n1 = textureSample(layer_normal1, layer_sampler, layer_uv(in.uv, 1u)).xyz;
    let n2 = textureSample(layer_normal2, layer_sampler, layer_uv(in.uv, 2u)).xyz;
    let n3 = textureSample(layer_normal3, layer_sampler, layer_uv(in.uv, 3u)).xyz;

    // Channels past the layer count don't exist; normalise what's left.
    let count = u32(terrain.size.w);
    let mask = vec4<f32>(
        select(0.0, 1.0, count > 0u),
        select(0.0, 1.0, count > 1u),
        select(0.0, 1.0, count > 2u),
        select(0.0, 1.0, count > 3u),
    );
    w = w * mask;
    let sum = w.x + w.y + w.z + w.w;
    if (sum > 1e-4) {
        w = w / sum;
    } else {
        w = vec4<f32>(mask.x, 0.0, 0.0, 0.0);
    }

    var s: Surface;
    if (count == 0u) {
        // No layers yet: plain light grey.
        s.albedo = vec3<f32>(0.5);
    } else {
        s.albedo = a0 * w.x + a1 * w.y + a2 * w.z + a3 * w.w;
    }
    s.alpha = 1.0;
    let sf = terrain.surface;
    s.metallic = dot(w, vec4<f32>(sf[0].x, sf[1].x, sf[2].x, sf[3].x));
    s.smoothness = dot(w, vec4<f32>(sf[0].y, sf[1].y, sf[2].y, sf[3].y));
    let m = layer_normal(n0, sf[0].z) * w.x + layer_normal(n1, sf[1].z) * w.y
        + layer_normal(n2, sf[2].z) * w.z + layer_normal(n3, sf[3].z) * w.w;
    let n = normalize(in.normal);
    if (count == 0u) {
        s.normal = n;
    } else {
        let t = normalize(in.tangent.xyz - n * dot(n, in.tangent.xyz));
        let b = cross(n, t) * in.tangent.w;
        s.normal = normalize(t * m.x + b * m.y + n * max(m.z, 1e-3));
    }
    s.emission = vec3<f32>(0.0);
    s.receive_shadows = true;
    s.occlusion = 1.0;
    s.draw = in.draw;
    return shade(in.world, s);
}
