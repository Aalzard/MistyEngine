// Grass shadow casters: appended to grass_field.wgsl and grass_blade.wgsl (group 0 = the
// shadow map's view). Coarse blades, a share of the drawn ones made wider to cast the same
// amount of shade, alpha tested on a small mip of the atlas.

struct GrassShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_grass_shadow(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> GrassShadowOut {
    let b = grass_fetch(ii);
    var out: GrassShadowOut;
    // Blades outside this map skip the rest (its near plane is left alone: casters behind it
    // still reach the map), and so do the flat fans of the blades the LOD shrank away
    // (`grass_flat`): lying on the ground, they could only shade the ground under them.
    if (grass_flat(b) || grass_outside(b, view.view_proj, false)) {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let v = grass_blade(vi, b);
    out.clip = view.view_proj * vec4<f32>(v.p, 1.0);
    out.uv = v.card;
    return out;
}

@fragment
fn fs_grass_shadow(in: GrassShadowOut) {
    let leaf = grass_atlas_uv(in.uv);
    if (textureSampleLevel(grass_atlas, grass_sampler, leaf.xy, i32(leaf.w), 1.0).a * leaf.z < 0.3) {
        discard;
    }
}
