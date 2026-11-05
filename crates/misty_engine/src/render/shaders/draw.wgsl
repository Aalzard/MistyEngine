// Per-draw values, `draws[slots[instance_index]]`: the one definition shared by mesh.wgsl,
// depth.wgsl and terrain.wgsl (prepended by pipelines.rs). Rust side: `DrawUniform` in
// render/mod.rs, 256 bytes. Every field is per object, so draws that differ only here
// (transform, colour, material values, per-object overrides) still share one instanced call.
//
//   offset  field     contents
//   0       model     object → world
//   64      normal    inverse transpose of model
//   128     color     sRGB base colour + alpha (× per-object colour override)
//   144     params    x = metallic, y = smoothness, z = receive shadows, w = normal map scale
//   160     emission  rgb = linear emission, w = -1 when the model matrix mirrors
//   176     uv_st     xy = tiling, zw = offset
//   192     surface   x = rendering mode, y = alpha cutoff, z = occlusion strength,
//                     w = 1 when smoothness comes from the albedo alpha
//   208     extra     x = cos, y = sin of the UV rotation (about the texture centre),
//                     z = LOD cross-fade (0 none; > 0 keep dither < z; < 0 keep dither >= 1 + z),
//                     w = 2D lighting: Sorting Layer bit + 1 (0 = not 2D lit)
//   224     lightmap_st  lightmap uv = uv2 * xy + zw
//   240     lightmap  x = lightmap layer + 1 (0 = not lightmapped), y = 1 when the surface
//                     sends no light through Screen Space GI (Contribute GI off)
struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
    emission: vec4<f32>,
    uv_st: vec4<f32>,
    surface: vec4<f32>,
    extra: vec4<f32>,
    lightmap_st: vec4<f32>,
    lightmap: vec4<f32>,
};

// Texture coordinates of a draw: rotate about (0.5, 0.5), then tile and offset.
// Written as uv + (R - I)·p so no rotation gives back `uv` exactly.
fn draw_uv(d: Draw, uv: vec2<f32>) -> vec2<f32> {
    let p = uv - vec2<f32>(0.5);
    let c = d.extra.x - 1.0;
    let s = d.extra.y;
    let r = uv + vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
    return r * d.uv_st.xy + d.uv_st.zw;
}

// LOD cross-fade: should this pixel be dropped? (4×4 ordered dither, complementary for the
// fading-in level so the two levels together cover every pixel once.)
fn lod_dithered_out(fade: f32, pixel: vec2<f32>) -> bool {
    if (fade == 0.0) {
        return false;
    }
    let p = vec2<u32>(pixel) % vec2<u32>(4u);
    var bayer = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let t = (bayer[p.y * 4u + p.x] + 0.5) / 16.0;
    if (fade > 0.0) {
        return t >= fade;
    }
    return t < 1.0 + fade;
}
