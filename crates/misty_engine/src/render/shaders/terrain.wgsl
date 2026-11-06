// Terrain: appended to mesh.wgsl, terrain_vertex.wgsl and terrain_layers.wgsl (same draws and
// lights; lit by `shade`). Chunks are drawn from index patterns over the heightmap texture.

@vertex
fn vs_terrain(@builtin(vertex_index) vi: u32, @builtin(instance_index) instance: u32) -> MeshOut {
    let c = terrain_sample(vi);
    let draw_index = slots[instance];
    let d = draws[draw_index];
    let world = d.model * vec4<f32>(terrain_local(c), 1.0);
    let n = normalize(textureLoad(terrain_normals, c, 0).xyz);
    var out: MeshOut;
    out.clip = view.view_proj * world;
    out.world = world.xyz;
    out.normal = n;
    out.uv = vec2<f32>(f32(c.x), f32(c.y)) / (terrain.grid.x - 1.0);
    out.draw = draw_index;
    // Tangent along +x, lying in the surface (layer normal maps on the top projection).
    out.tangent = vec4<f32>(normalize(vec3<f32>(1.0, 0.0, 0.0) - n * n.x), 1.0);
    // Terrains don't move: camera motion only.
    out.clip_prev = prev_clip(world.xyz);
    return out;
}

@fragment
fn fs_terrain(in: MeshOut) -> MeshTargets {
    var out = shade(in.world, terrain_surface(in));
    out.velocity = mesh_velocity(in);
    return out;
}
