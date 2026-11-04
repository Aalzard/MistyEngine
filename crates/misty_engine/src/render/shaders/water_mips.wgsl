// FFT ocean mip chain (compute).
// Mip chain: average 2×2 texels of the level above.
@group(0) @binding(0) var src_displacement: texture_2d<f32>;
@group(0) @binding(1) var src_slope: texture_2d<f32>;
@group(0) @binding(2) var dst_displacement: texture_storage_2d<rgba16float, write>;
@group(0) @binding(3) var dst_slope: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst_displacement);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let s = vec2<i32>(id.xy) * 2;
    var d = vec4<f32>(0.0);
    var n = vec4<f32>(0.0);
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            d += textureLoad(src_displacement, s + vec2<i32>(x, y), 0);
            n += textureLoad(src_slope, s + vec2<i32>(x, y), 0);
        }
    }
    textureStore(dst_displacement, vec2<i32>(id.xy), d * 0.25);
    textureStore(dst_slope, vec2<i32>(id.xy), n * 0.25);
}
