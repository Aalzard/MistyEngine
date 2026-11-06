// Grass shadow casters: appended to grass_field.wgsl and grass_blade.wgsl (group 0 = the
// shadow map's view). Coarse blades, a share of the drawn ones made wider to cast the same
// amount of shade, alpha tested on a small mip of the atlas.

struct GrassShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_grass_shadow(@builtin(vertex_index) vi: u32, b: BladeIn) -> GrassShadowOut {
    let v = grass_blade(vi, b);
    var out: GrassShadowOut;
    out.clip = view.view_proj * vec4<f32>(v.p, 1.0);
    out.uv = vec2<f32>(grass_card_u(v.cell, v.side, v.repeat), grass_card_v(v.t, b.shape.w));
    return out;
}

@fragment
fn fs_grass_shadow(in: GrassShadowOut) {
    let leaf = grass_atlas_uv(in.uv);
    if (textureSampleLevel(grass_atlas, grass_sampler, leaf.xy, 1.0).a * leaf.z < 0.3) {
        discard;
    }
}
