// Temporal reprojection of the cloud trace (after cloud_common.wgsl for the uniform). Each
// texel finds where its clouds were last frame (their distance → last frame's screen), clamps
// that history to the current 3×3 neighbourhood (no ghosting when clouds move or the camera
// cuts) and blends: the blue-noise start offsets average out over a few frames.

@group(0) @binding(6) var raw: texture_2d_array<f32>;
@group(0) @binding(7) var history: texture_2d_array<f32>;
@group(0) @binding(8) var clamp_linear: sampler;
@group(0) @binding(9) var resolved: texture_storage_2d_array<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(cl.trace.xy);
    if (any(id.xy >= size)) {
        return;
    }
    let texel = vec2<i32>(id.xy);
    let cur = textureLoad(raw, texel, 0, 0);
    let cur_d = textureLoad(raw, texel, 1, 0);
    var out = cur;
    var out_d = cur_d;
    let weight = cl.misc.x;
    if (weight > 0.0) {
        // Neighbourhood mean and spread (variance clipping).
        var m1 = vec4<f32>(0.0);
        var m2 = vec4<f32>(0.0);
        for (var k = 0; k < 9; k++) {
            let o = vec2<i32>(k % 3 - 1, k / 3 - 1);
            let q = clamp(texel + o, vec2<i32>(0), vec2<i32>(size) - 1);
            let s = textureLoad(raw, q, 0, 0);
            m1 += s;
            m2 += s * s;
        }
        m1 /= 9.0;
        let sigma = sqrt(max(m2 / 9.0 - m1 * m1, vec4<f32>(0.0)));
        let lo = m1 - sigma * 1.5;
        let hi = m1 + sigma * 1.5;

        let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size);
        let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let far = cl.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
        let near = cl.inv_view_proj * vec4<f32>(ndc, 0.0, 1.0);
        let dir = normalize(far.xyz / far.w - near.xyz / near.w);
        let world = cl.eye.xyz + dir * cur_d.r * 1000.0;
        let clip = cl.prev_view_proj * vec4<f32>(world, 1.0);
        if (clip.w > 0.0) {
            let pn = clip.xy / clip.w;
            let puv = vec2<f32>(pn.x * 0.5 + 0.5, 0.5 - pn.y * 0.5);
            if (all(puv >= vec2<f32>(0.0)) && all(puv <= vec2<f32>(1.0))) {
                let h = textureSampleLevel(history, clamp_linear, puv, 0, 0.0);
                let hd = textureSampleLevel(history, clamp_linear, puv, 1, 0.0);
                out = mix(cur, clamp(h, lo, hi), weight);
                out_d = mix(cur_d, hd, weight);
            }
        }
    }
    textureStore(resolved, id.xy, 0, out);
    textureStore(resolved, id.xy, 1, out_d);
}
