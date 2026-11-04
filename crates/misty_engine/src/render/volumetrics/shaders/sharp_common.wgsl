// Uniform of the sharp sun shafts (render/volumetrics/sharp_shafts.rs), shared by the march /
// accumulate compute passes and the full-resolution composite.

struct SharpShafts {
    // xy = march texture size, zw = full-resolution size.
    size: vec4<f32>,
    // x = steps, y = frame index, z = history weight (0 = none), w = pixels per march texel.
    params: vec4<f32>,
    // x = froxel reach (view depth), y = camera near, z = camera far, w = 1 when the jitter
    // changes every frame.
    range: vec4<f32>,
};

// View depth of a [0, 1] depth-buffer value (perspective, depth 0 at the near plane).
fn sharp_view_depth(z: f32, s: SharpShafts) -> f32 {
    let n = s.range.y;
    let f = s.range.z;
    if (z >= 1.0) {
        return f;
    }
    return n * f / max(f - z * (f - n), 1e-6);
}

// Full-resolution pixel a march texel stands for (the centre-most pixel of its block).
fn sharp_pixel(texel: vec2<u32>, s: SharpShafts) -> vec2<u32> {
    let scale = u32(s.params.w);
    return min(texel * scale + vec2<u32>(scale / 2u), vec2<u32>(s.size.zw) - 1u);
}
