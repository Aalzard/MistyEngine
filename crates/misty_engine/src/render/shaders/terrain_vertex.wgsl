// Terrain vertex pulling, shared by the lit pass (after mesh.wgsl) and the shadow pass (after
// depth.wgsl). There is no vertex buffer: chunk index patterns address heightmap samples
// (index = iz * resolution + ix, offset by the chunk's base vertex) and heights and normals
// come from textures, so a brush stroke only re-uploads the texels it changed.

struct TerrainParams {
    // xyz = size (width, height, length), w = layer count.
    size: vec4<f32>,
    // x = heightmap samples per side, y = sample spacing along x, z = along z.
    grid: vec4<f32>,
    // Per layer: xy = tile size (metres), zw = tile offset (metres).
    tiles: array<vec4<f32>, 4>,
    // Per layer: x = metallic, y = smoothness, z = normal scale.
    surface: array<vec4<f32>, 4>,
    // Per layer Color Harmony: x = hue shift (turns), y = saturation scale,
    // z = brightness scale, w = 1 when active.
    harmony: array<vec4<f32>, 4>,
    // x = triplanar on, y = |normal.xz| where triplanar starts, z = projection sharpness,
    // w = height-blend window (1 = plain weights).
    cliff: vec4<f32>,
    // Ground under grass: rgb = linear healthy grass colour, w = ground tint (0 = none).
    grass_healthy: vec4<f32>,
    // rgb = linear dry colour, w = patch size (m).
    grass_dry: vec4<f32>,
    // rgb = linear lush colour, w = grass draw distance.
    grass_lush: vec4<f32>,
    // x = lush amount, y = dry amount, z = the quality's share of the blades with the ground
    // cover (Quality ▸ Grass Ground Cover; 0 = off), w = the grass's full-density distance.
    grass_amounts: vec4<f32>,
    // Quality ▸ Grass Ground Cover (terrain.rs `GROUND_COVER`): x = how much of the ground the
    // fans cover, y / z = where that fades in, w = where it has faded out (× the grass's
    // full-density distance).
    ground_cover: vec4<f32>,
    // rgb = the fans' colour against the ground's looking into the sun / with it behind.
    ground_tint: vec4<f32>,
    ground_front: vec4<f32>,
};

@group(3) @binding(0) var<uniform> terrain: TerrainParams;
// Normalised heights (R32Float, read with textureLoad).
@group(3) @binding(12) var terrain_heights: texture_2d<f32>;
// Smooth surface normals (RGBA8 snorm).
@group(3) @binding(13) var terrain_normals: texture_2d<f32>;

// Heightmap sample addressed by a vertex index.
fn terrain_sample(vi: u32) -> vec2<i32> {
    let r = u32(terrain.grid.x);
    return vec2<i32>(i32(vi % r), i32(vi / r));
}

// Terrain-local position of a heightmap sample.
fn terrain_local(c: vec2<i32>) -> vec3<f32> {
    let h = textureLoad(terrain_heights, c, 0).r;
    return vec3<f32>(f32(c.x) * terrain.grid.y, h * terrain.size.y, f32(c.y) * terrain.grid.z);
}

// Shadow casters: position only.
@vertex
fn vs_terrain_depth(
    @builtin(vertex_index) vi: u32,
    @builtin(instance_index) instance: u32,
) -> @builtin(position) vec4<f32> {
    let p = terrain_local(terrain_sample(vi));
    return view.view_proj * draws[slots[instance]].model * vec4<f32>(p, 1.0);
}
