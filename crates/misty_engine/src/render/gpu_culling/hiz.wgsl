// Hi-Z depth pyramid: level 0 holds the farthest depth of each 2×2 pixel block (all MSAA
// samples), each further level the farthest of 2×2 texels below it. Sizes round up and reads
// clamp at the edge, so every texel covers the whole area under it.

@group(0) @binding(0) var depth_ms: texture_depth_multisampled_2d;
@group(0) @binding(1) var src_level: texture_2d<f32>;
@group(0) @binding(2) var dst: texture_storage_2d<r32float, write>;

@compute @workgroup_size(8, 8)
fn cs_level0(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let src = textureDimensions(depth_ms) - vec2<u32>(1u);
    let samples = textureNumSamples(depth_ms);
    var far = 0.0;
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let p = min(id.xy * 2u + vec2<u32>(x, y), src);
            for (var s = 0u; s < samples; s++) {
                far = max(far, textureLoad(depth_ms, p, i32(s)));
            }
        }
    }
    textureStore(dst, id.xy, vec4<f32>(far, 0.0, 0.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn cs_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let src = textureDimensions(src_level) - vec2<u32>(1u);
    var far = 0.0;
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let p = min(id.xy * 2u + vec2<u32>(x, y), src);
            far = max(far, textureLoad(src_level, p, 0).r);
        }
    }
    textureStore(dst, id.xy, vec4<f32>(far, 0.0, 0.0, 0.0));
}
