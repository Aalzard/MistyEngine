// Sharp sun shafts → full resolution (included after common.wgsl and sharp_common.wgsl):
// depth-aware bilateral upsample of the accumulated march texture, added onto the HDR image
// (it already holds the surfaces fogged by the froxels without the sun).

@group(1) @binding(0) var shafts_tex: texture_2d<f32>;
@group(1) @binding(1) var scene_depth: texture_depth_multisampled_2d;
@group(1) @binding(2) var<uniform> sharp: SharpShafts;

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4<f32> {
    // The density debug view shows the fog's opacity only.
    if (vfog.mode.y > 0.5 && vfog.mode.y < 1.5) {
        return vec4<f32>(0.0);
    }
    let pixel = vec2<i32>(in.pos.xy);
    let depth = sharp_view_depth(textureLoad(scene_depth, pixel, 0), sharp);
    let size = vec2<i32>(sharp.size.xy);
    let scale = sharp.params.w;
    // Texel k stands for pixel k * scale + floor(scale / 2).
    let k = (vec2<f32>(pixel) - floor(scale * 0.5)) / scale;
    let base = vec2<i32>(floor(k));
    let f = k - floor(k);
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    var nearest = vec3<f32>(0.0);
    var nearest_err = 1e9;
    for (var i = 0; i < 4; i++) {
        let o = vec2<i32>(i & 1, i >> 1u);
        let q = clamp(base + o, vec2<i32>(0), size - 1);
        let s = textureLoad(shafts_tex, q, 0);
        let bw = select(1.0 - f.x, f.x, o.x == 1) * select(1.0 - f.y, f.y, o.y == 1);
        // Relative depth difference: taps on another surface barely count.
        let err = abs(s.a - depth) / max(depth, 1e-3);
        let w = (bw + 1e-3) * exp(-err * err * 400.0);
        sum += s.rgb * w;
        total += w;
        if (err < nearest_err) {
            nearest_err = err;
            nearest = s.rgb;
        }
    }
    var light = nearest;
    if (total > 1e-4) {
        light = sum / total;
    }
    return vec4<f32>(max(light, vec3<f32>(0.0)), 0.0);
}
