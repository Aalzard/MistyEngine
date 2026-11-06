// Tree shadow casters: appended to draw.wgsl, grass_field.wgsl and tree_vertex.wgsl (group 0 =
// the shadow map's view). Bark and leaf clusters (alpha tested on a small mip) swaying with the
// wind like the drawn trees; far trees cast with their impostor turned towards the light.

struct TreeShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_tree_shadow(v: TreeVertexIn, inst: TreeInstance) -> TreeShadowOut {
    let p = tree_point(v, inst);
    var out: TreeShadowOut;
    out.clip = view.view_proj * vec4<f32>(p.world, 1.0);
    out.uv = v.uv;
    return out;
}

@fragment
fn fs_tree_shadow_leaves(in: TreeShadowOut) {
    if (textureSampleLevel(tree_albedo, tree_sampler, in.uv, 1.5).a < 0.4) {
        discard;
    }
}

@vertex
fn vs_tree_shadow_impostor(@builtin(vertex_index) vi: u32, inst: TreeInstance) -> TreeShadowOut {
    let q = impostor_point(vi, inst, tree.light.xz);
    var out: TreeShadowOut;
    out.clip = view.view_proj * vec4<f32>(q.world, 1.0);
    out.uv = q.uv0;
    return out;
}

@fragment
fn fs_tree_shadow_impostor(in: TreeShadowOut) {
    if (textureSampleLevel(tree_albedo, tree_sampler, in.uv, 1.0).a < 0.4) {
        discard;
    }
}
