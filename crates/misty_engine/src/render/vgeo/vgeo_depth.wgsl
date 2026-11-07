// Misty Virtual Geometry shadow casters (after DEPTH_SRC and vgeo_fetch.wgsl).

@vertex
fn vs_cluster_depth(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> @builtin(position) vec4<f32> {
    let v = vg_fetch(vertex, instance);
    return view.view_proj * draws[v.draw].model * vec4<f32>(v.position, 1.0);
}

@vertex
fn vs_cluster_depth_indexed(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> @builtin(position) vec4<f32> {
    let v = vg_fetch_indexed(vertex, instance);
    return view.view_proj * draws[v.draw].model * vec4<f32>(v.position, 1.0);
}
