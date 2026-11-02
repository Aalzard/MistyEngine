// Selection outline: orange where the mask has an edge within 2 pixels.

@group(1) @binding(0) var mask: texture_2d<f32>;

fn covered(p: vec2<i32>) -> bool {
    let size = vec2<i32>(textureDimensions(mask));
    let q = clamp(p, vec2<i32>(0), size - 1);
    return textureLoad(mask, q, 0).r > 0.5;
}

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    if (covered(p)) {
        return vec4<f32>(0.0);
    }
    var hit = false;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            if (x * x + y * y <= 5 && covered(p + vec2<i32>(x, y))) {
                hit = true;
            }
        }
    }
    if (!hit) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(srgb_to_linear(vec3<f32>(1.0, 0.55, 0.0)), 1.0);
}
