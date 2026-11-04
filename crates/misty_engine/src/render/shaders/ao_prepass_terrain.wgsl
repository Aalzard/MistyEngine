// Terrain in the depth + normal prepass: appended to ao_prepass.wgsl and terrain_vertex.wgsl,
// so terrains are drawn by vertex pulling from their heightmap textures like the lit pass.

@vertex
fn vs_terrain_prepass(
    @builtin(vertex_index) vi: u32,
    @builtin(instance_index) instance: u32,
) -> PrepassOut {
    let c = terrain_sample(vi);
    let d = draws[slots[instance]];
    var out: PrepassOut;
    out.clip = view.view_proj * d.model * vec4<f32>(terrain_local(c), 1.0);
    // Terrains are only translated, so the local normal is the world normal.
    out.normal = textureLoad(terrain_normals, c, 0).xyz;
    out.uv = vec2<f32>(0.0);
    out.alpha = vec3<f32>(1.0, 0.0, 0.0);
    return out;
}
