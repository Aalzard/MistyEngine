// Front-to-back integration of the lit froxels along each screen column: every texel ends up
// holding rgb = light scattered towards the eye up to the far side of its slice, a =
// transmittance from the eye. Energy-conserving per slice: S * (1 - e^(-σt Δ)) / σt.

@group(0) @binding(1) var lit: texture_3d<f32>;
@group(0) @binding(2) var integrated: texture_storage_3d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec3<u32>(fx.grid.xyz);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    // Slices are in view depth; the ray through this column is longer by 1 / cos.
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size.xy);
    var stretch = 1.0;
    if (fx.eye.w < 0.5) {
        stretch = 1.0 / max(dot(froxel_ray(uv), fx.forward.xyz), 1e-3);
    }
    var scatter = vec3<f32>(0.0);
    var transmittance = 1.0;
    var start = 0.0;
    let n = f32(size.z);
    for (var k = 0u; k < size.z; k++) {
        let end = slice_depth(f32(k + 1u) / n);
        let thickness = (end - start) * stretch;
        start = end;
        let s = textureLoad(lit, vec3<u32>(id.xy, k), 0);
        let t = exp(-s.a * thickness);
        var slab = s.rgb * thickness;
        if (s.a > 1e-7) {
            slab = s.rgb * (1.0 - t) / s.a;
        }
        scatter += transmittance * slab;
        transmittance *= t;
        textureStore(integrated, vec3<u32>(id.xy, k), vec4<f32>(scatter, transmittance));
    }
}
