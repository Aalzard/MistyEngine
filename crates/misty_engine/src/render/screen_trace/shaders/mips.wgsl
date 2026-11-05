// Mip chains of the screen-space trace (render/screen_trace):
//   cs_hiz_copy  - prepass depth → level 0 of the closest-depth pyramid;
//   cs_hiz_down  - level k → k + 1, the closest (smallest) depth of the texels it covers
//                  (three per axis where level k has an odd size, so nothing is skipped);
//   cs_color_down - lit colour history level k → k + 1, 2×2 box filter.
// Each entry point uses its own bindings (separate pipeline layouts).

@group(0) @binding(0) var depth_src: texture_depth_2d;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var hiz_dst: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var color_dst: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn cs_hiz_copy(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(hiz_dst);
    if (any(id.xy >= size)) {
        return;
    }
    let d = textureLoad(depth_src, vec2<i32>(id.xy), 0);
    textureStore(hiz_dst, vec2<i32>(id.xy), vec4<f32>(d, 0.0, 0.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn cs_hiz_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(hiz_dst);
    if (any(id.xy >= size)) {
        return;
    }
    let src_size = vec2<i32>(textureDimensions(src));
    let base = vec2<i32>(id.xy) * 2;
    // The last texel of an odd-sized level also covers the leftover row / column.
    let extra = vec2<i32>(
        select(1, 2, id.x == size.x - 1u && (src_size.x & 1) == 1),
        select(1, 2, id.y == size.y - 1u && (src_size.y & 1) == 1),
    );
    var d = 1.0;
    for (var y = 0; y <= extra.y; y++) {
        for (var x = 0; x <= extra.x; x++) {
            let q = min(base + vec2<i32>(x, y), src_size - 1);
            d = min(d, textureLoad(src, q, 0).r);
        }
    }
    textureStore(hiz_dst, vec2<i32>(id.xy), vec4<f32>(d, 0.0, 0.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn cs_color_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(color_dst);
    if (any(id.xy >= size)) {
        return;
    }
    let last = vec2<i32>(textureDimensions(src)) - 1;
    let base = vec2<i32>(id.xy) * 2;
    var c = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        c += textureLoad(src, min(base + vec2<i32>(k & 1, k >> 1u), last), 0);
    }
    textureStore(color_dst, vec2<i32>(id.xy), c * 0.25);
}
