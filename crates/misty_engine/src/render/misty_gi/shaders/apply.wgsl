// Per-pixel gather (appended to gi_common.wgsl): at every gather texel (the top-left full
// pixel of its block, as the compose's bilateral upsample expects), the irradiance of the probe
// clipmap at the prepass surface. rgb = mean incoming radiance, a = 1 (0 = sky).

fn world_at(pc: vec2<f32>, d: f32) -> vec3<f32> {
    let ndc = vec2<f32>(pc.x / p.size.x * 2.0 - 1.0, 1.0 - pc.y / p.size.y * 2.0);
    let h = p.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    return h.xyz / h.w;
}

@compute @workgroup_size(8, 8)
fn cs_apply(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = vec2<i32>(id.xy);
    if (any(vec2<f32>(t) >= p.size.zw)) {
        return;
    }
    let scale = p.info3.z;
    let full = vec2<i32>(p.size.xy) - 1;
    let fp = min(vec2<i32>(vec2<f32>(t) * scale), full);
    let d = textureLoad(depth_tex, fp, 0);
    let nt = textureLoad(scene_normal_tex, fp, 0);
    if (d >= 1.0 || abs(nt.w) < 0.5) {
        textureStore(gather_out, t, vec4<f32>(0.0));
        return;
    }
    let n = normalize(nt.xyz);
    let pos = world_at(vec2<f32>(fp) + 0.5, d);
    let e = gi_irradiance(pos, n, i32(p.info3.y));
    textureStore(gather_out, t, vec4<f32>(e, 1.0));
}
