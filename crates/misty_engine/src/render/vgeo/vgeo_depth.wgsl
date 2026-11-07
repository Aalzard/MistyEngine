// Misty Virtual Geometry shadow casters (after DEPTH_SRC and vgeo_fetch.wgsl).

@vertex
fn vs_cluster_depth(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> @builtin(position) vec4<f32> {
    let v = vg_fetch(vertex, instance);
    return view.view_proj * draws[v.draw].model * vec4<f32>(v.position, 1.0);
}

// Cutout casters: uv and alpha for depth.wgsl's fs_shadow_cutout.
fn vg_cutout_out(v: VgVertex) -> CutoutOut {
    let d = draws[v.draw];
    var out: CutoutOut;
    out.clip = view.view_proj * d.model * vec4<f32>(v.position, 1.0);
    out.uv = draw_uv(d, v.uv);
    out.alpha = vec4<f32>(d.color.a, d.surface.y, d.surface.w, 0.0);
    return out;
}

@vertex
fn vs_cluster_depth_uv(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> CutoutOut {
    return vg_cutout_out(vg_fetch(vertex, instance));
}

@vertex
fn vs_cluster_depth_uv_indexed(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> CutoutOut {
    return vg_cutout_out(vg_fetch_indexed(vertex, instance));
}

@vertex
fn vs_cluster_depth_indexed(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> @builtin(position) vec4<f32> {
    let v = vg_fetch_indexed(vertex, instance);
    return view.view_proj * draws[v.draw].model * vec4<f32>(v.position, 1.0);
}
