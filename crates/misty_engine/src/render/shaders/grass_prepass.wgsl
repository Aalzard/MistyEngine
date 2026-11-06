// Grass in the depth + normal prepass of the screen-space effects: appended to
// grass_field.wgsl and grass_blade.wgsl. Near blades write the same depth and shading normal
// as the lit pass (alpha tested), so ambient occlusion, contact shadows, the shadow mask and
// SSGI / SSR see the blades instead of the ground behind them.

struct GrassPrepassOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    // xy = atlas uv, z = 0 close up .. 1 far.
    @location(1) uv: vec3<f32>,
};

@vertex
fn vs_grass_prepass(@builtin(vertex_index) vi: u32, b: BladeIn) -> GrassPrepassOut {
    let v = grass_blade(vi, b);
    let face = normalize(cross(v.across, v.tangent));
    var out: GrassPrepassOut;
    out.clip = view.view_proj * vec4<f32>(v.p, 1.0);
    out.normal = normalize(face + v.across * v.side * 0.8);
    out.uv = vec3<f32>(grass_card_u(v.cell, v.side, v.repeat), grass_card_v(v.t, b.shape.w), smoothstep(5.0, 55.0, v.dist));
    return out;
}

@fragment
fn fs_grass_prepass(in: GrassPrepassOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let leaf = grass_atlas_uv(in.uv.xy);
    let alpha = textureSampleGrad(grass_atlas, grass_sampler, leaf.xy, grass_atlas_dx(in.uv.xy), grass_atlas_dy(in.uv.xy)).a * leaf.z;
    if (alpha < 0.5) {
        discard;
    }
    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    // The lit pass leans blade normals up the same way (grass.wgsl).
    n = normalize(mix(n, vec3<f32>(0.0, 1.0, 0.0), mix(0.7, 0.8, in.uv.z)));
    // w = 1 + smoothness (a matte sheen); grass sends light through SSGI.
    return vec4<f32>(n, 1.1);
}
