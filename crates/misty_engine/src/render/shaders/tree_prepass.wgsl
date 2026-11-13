// Trees in the depth + normal prepass of the screen-space effects: appended to draw.wgsl,
// grass_field.wgsl and tree_vertex.wgsl. The same depth, cut-outs and LOD fades as the lit
// pass, so ambient occlusion, contact shadows and the temporal passes see the trees instead of
// what is behind them; motion vectors include the wind.

struct TreePrepassOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    // x = LOD fade, y = blend towards the second impostor view.
    @location(2) misc: vec2<f32>,
    @location(3) clip_prev: vec3<f32>,
    @location(4) uv1: vec2<f32>,
};

struct TreePrepassTargets {
    @location(0) normal: vec4<f32>,
    @location(1) velocity: vec2<f32>,
};

@vertex
fn vs_tree_prepass(v: TreeVertexIn, inst: TreeInstance) -> TreePrepassOut {
    let p = tree_point(v, inst);
    var out: TreePrepassOut;
    out.clip = view.view_proj * vec4<f32>(p.world, 1.0);
    out.clip_prev = prev_clip(p.prev);
    out.normal = p.normal;
    out.uv = v.uv;
    out.uv1 = v.uv;
    out.misc = vec2<f32>(inst.scale.w, 0.0);
    return out;
}

// w = 1 + smoothness (matte), positive: trees send light through SSGI.
fn tree_prepass_targets(in: TreePrepassOut, n: vec3<f32>) -> TreePrepassTargets {
    var out: TreePrepassTargets;
    out.normal = vec4<f32>(normalize(n), 1.05);
    out.velocity = velocity_of(in.clip, in.clip_prev);
    return out;
}

@fragment
fn fs_tree_prepass_bark(in: TreePrepassOut) -> TreePrepassTargets {
    if (tree_faded(in.misc.x, in.clip.xy)) {
        discard;
    }
    return tree_prepass_targets(in, in.normal);
}

@fragment
fn fs_tree_prepass_leaves(in: TreePrepassOut) -> TreePrepassTargets {
    let alpha = textureSampleBias(tree_albedo, tree_sampler, in.uv, view.view_tan.z).a;
    if (alpha < 0.5 || tree_faded(in.misc.x, in.clip.xy)) {
        discard;
    }
    return tree_prepass_targets(in, in.normal);
}

@vertex
fn vs_tree_prepass_impostor(@builtin(vertex_index) vi: u32, inst: TreeInstance) -> TreePrepassOut {
    let to_eye = view.camera_pos.xyz - inst.foot.xyz;
    let q = impostor_point(vi, inst, to_eye.xz);
    var out: TreePrepassOut;
    out.clip = view.view_proj * vec4<f32>(q.world, 1.0);
    out.clip_prev = prev_clip(q.world);
    out.normal = q.toward;
    out.uv = q.uv0;
    out.uv1 = q.uv1;
    out.misc = vec2<f32>(inst.scale.w, q.blend);
    return out;
}

@fragment
fn fs_tree_prepass_impostor(in: TreePrepassOut) -> TreePrepassTargets {
    let tex = mix(
        textureSampleBias(tree_albedo, tree_sampler, in.uv, view.view_tan.z),
        textureSampleBias(tree_albedo, tree_sampler, in.uv1, view.view_tan.z),
        in.misc.y,
    );
    if (tex.a < 0.5 || tree_faded(in.misc.x, in.clip.xy)) {
        discard;
    }
    // (After the test: a far hillside's impostors overlap, most fragments are cut away.)
    let nt = mix(
        textureSampleBias(tree_normal_map, tree_sampler, in.uv, view.view_tan.z),
        textureSampleBias(tree_normal_map, tree_sampler, in.uv1, view.view_tan.z),
        in.misc.y,
    );
    let m = nt.xyz * 2.0 - 1.0;
    let toward = in.normal;
    let right = vec3<f32>(toward.z, 0.0, -toward.x);
    return tree_prepass_targets(in, right * m.x + vec3<f32>(0.0, 1.0, 0.0) * m.y + toward * m.z);
}
