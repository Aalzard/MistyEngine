// Per-pixel gather (appended to gi_common.wgsl): at every gather texel (the top-left full
// pixel of its block, as the compose's bilateral upsample expects), the irradiance of the probe
// clipmap at the prepass surface (rgb = mean incoming radiance, a = 1; 0 = sky) and, when
// reflections are on, a glossy reflection traced through the same distance field into the
// radiance cache (rgb, a = confidence: 0 where the ray found nothing or the surface is rough).

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
    textureStore(reflect_out, t, glossy_reflection(pos, n, clamp(abs(nt.w) - 1.0, 0.0, 1.0)));
}

// Perceptual roughness above which the probes' / sky's reflection is kept as it is.
const REFLECT_MAX_ROUGHNESS: f32 = 0.6;

// The surface the mirror direction meets (radiance cache), faded out with roughness and at the
// end of the ray's reach. Voxel-sharp, so it suits glossy rather than mirror surfaces; smooth
// surfaces keep SSR's sharper reflection where SSR finds one (the compose prefers it).
fn glossy_reflection(pos: vec3<f32>, n: vec3<f32>, smoothness: f32) -> vec4<f32> {
    let perceptual = 1.0 - smoothness;
    if (p.camera.w < 0.5 || perceptual >= REFLECT_MAX_ROUGHNESS) {
        return vec4<f32>(0.0);
    }
    let v = normalize(p.camera.xyz - pos);
    let r = reflect(-v, n);
    let lk = max(level_of(pos, 0), 0);
    let start = pos + n * (1.5 * voxel(lk));
    let h = trace(start, r, 80.0, 48, 0.5, 0.0);
    if (!h.hit) {
        return vec4<f32>(0.0);
    }
    let radiance = cache_radiance(h.level, h.pos, r);
    let rough_fade = 1.0 - smoothstep(0.35, REFLECT_MAX_ROUGHNESS, perceptual);
    return vec4<f32>(radiance, rough_fade);
}
