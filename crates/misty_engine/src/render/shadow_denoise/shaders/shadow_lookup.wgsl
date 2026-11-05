// Temporal shadow mask lookup of the lit shaders (appended after shadows.wgsl): the smooth,
// accumulated visibility the render/shadow_denoise passes wrote for up to four soft-shadowed
// lights. -1 = not covered (mask off, another light, off screen, or the prepass saw another
// surface at this pixel: transparent meshes, MSAA edges), and the shadow maps are read directly.

struct ShadowMask {
    // x = 1 when on, y = width, z = height.
    info: vec4<u32>,
    // Channel (0..3) of each light slot, 255 = none.
    channels: array<vec4<u32>, 2>,
    pad: vec4<u32>,
    // Per pixel: [visibility 0 | 1 (unorm16 × 2), visibility 2 | 3, linear depth (f32 bits),
    // history frames (8 bits) | blur radii (4 × 6 bits)].
    texels: array<vec4<u32>>,
};

@group(1) @binding(18) var<storage, read> shadow_mask: ShadowMask;

fn shadow_mask_lookup(i: u32, world: vec3<f32>) -> f32 {
    if (shadow_mask.info.x == 0u || i >= 8u) {
        return -1.0;
    }
    let ch = shadow_mask.channels[i / 4u][i % 4u];
    if (ch > 3u) {
        return -1.0;
    }
    let c = view.view_proj * vec4<f32>(world, 1.0);
    if (c.w <= 0.0) {
        return -1.0;
    }
    let size = vec2<f32>(shadow_mask.info.yz);
    let ndc = c.xy / c.w;
    let px = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * size;
    if (any(px < vec2<f32>(0.0)) || any(px >= size)) {
        return -1.0;
    }
    let p = vec2<u32>(px);
    let t = shadow_mask.texels[p.y * shadow_mask.info.y + p.x];
    let depth = bitcast<f32>(t.z);
    let z = dot(world - view.camera_pos.xyz, shadow_params.view_axis.xyz);
    if (abs(depth - z) > 0.02 * z + 0.05) {
        return -1.0;
    }
    let v = vec4<f32>(unpack2x16unorm(t.x), unpack2x16unorm(t.y));
    return v[ch];
}
