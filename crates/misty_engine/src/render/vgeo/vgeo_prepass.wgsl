// Misty Virtual Geometry in the depth + normal prepass of the screen-space effects (after
// PREPASS_SRC and vgeo_fetch.wgsl).

@vertex
fn vs_cluster_prepass(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> PrepassOut {
    return vg_prepass_out(vg_fetch(vertex, instance));
}

@vertex
fn vs_cluster_prepass_indexed(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> PrepassOut {
    return vg_prepass_out(vg_fetch_indexed(vertex, instance));
}

fn vg_prepass_out(v: VgVertex) -> PrepassOut {
    let d = draws[v.draw];
    var out: PrepassOut;
    out.clip = view.view_proj * d.model * vec4<f32>(v.position, 1.0);
    out.normal = (d.normal * vec4<f32>(v.normal, 0.0)).xyz;
    out.uv = draw_uv(d, v.uv);
    out.alpha = vec4<f32>(d.color.a, d.surface.y, d.surface.w, d.params.y);
    out.no_gi = d.lightmap.y;
    out.clip_prev = prev_clip((d.prev_model * vec4<f32>(v.position, 1.0)).xyz);
    return out;
}
