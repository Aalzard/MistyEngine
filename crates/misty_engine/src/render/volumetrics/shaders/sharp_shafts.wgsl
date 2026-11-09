// Sharp sun shafts (included after froxel_common.wgsl, froxel_medium.wgsl and
// sharp_common.wgsl). `march`: one ray per texel at reduced resolution from the eye to the
// depth buffer (capped by the froxel reach), blue-noise start offset, sampling the same density
// and phase as the froxels (with fog noise, the extinction is read from the lit froxels) and
// the sun's cascades with a hard 2×2 PCF lookup (the tightest one whose texels the march
// can resolve).
// `accumulate`: reprojects last frame's result and blends it in, clamped to this frame's
// neighbourhood while the view moves (so thin shafts don't smear). rgb = sun light scattered towards the
// eye, a = view depth of the texel's pixel.

@group(0) @binding(9) var<uniform> sharp: SharpShafts;
@group(0) @binding(10) var scene_depth: texture_depth_multisampled_2d;
@group(0) @binding(11) var blue_noise: texture_2d<f32>;
@group(0) @binding(12) var march_out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(13) var raw: texture_2d<f32>;
@group(0) @binding(14) var history: texture_2d<f32>;
@group(0) @binding(15) var accum_out: texture_storage_2d<rgba16float, write>;
// This frame's lit froxels (a = extinction, noise included).
@group(0) @binding(16) var froxel_lit: texture_3d<f32>;

// Blue noise for this texel, golden-ratio animated when temporal accumulation is on.
fn shaft_jitter(texel: vec2<u32>) -> f32 {
    let size = textureDimensions(blue_noise);
    let n = textureLoad(blue_noise, texel % size, 0).r;
    return fract(n + sharp.range.w * sharp.params.y * 0.61803399);
}

fn texel_uv(texel: vec2<u32>) -> vec2<f32> {
    return (vec2<f32>(sharp_pixel(texel, sharp)) + 0.5) / sharp.size.zw;
}

@compute @workgroup_size(8, 8, 1)
fn march(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= vec2<u32>(sharp.size.xy))) {
        return;
    }
    let z = textureLoad(scene_depth, sharp_pixel(id.xy, sharp), 0);
    let depth = sharp_view_depth(z, sharp);
    let uv = texel_uv(id.xy);
    let ray = froxel_ray(uv);
    let cos_v = max(dot(ray, fx.forward.xyz), 1e-4);
    // Same reach as the froxels; the analytic tail of fog_apply continues past it. The march
    // starts at the near plane: the cascades don't cover the few centimetres before it.
    let start = sharp.range.y / cos_v;
    let end = max(min(depth, sharp.range.x) / cos_v, start);
    let li = fx.lights[u32(max(fx.sun.x, 0.0))];
    let l = -normalize(li.dir_type.xyz);
    let steps = u32(sharp.params.x);
    let dt = (end - start) / f32(steps);
    let j = shaft_jitter(id.xy);
    var transmittance = 1.0;
    var scatter = vec3<f32>(0.0);
    var cascade = 0;
    // The sun cascades' texels (metres) and the march texel's footprint per metre along the
    // ray: a sample reads the first cascade whose texel is at least `sharp.cascades.x` of
    // its footprint (finer texels only alias at the march's spacing).
    let footprint = sharp.cascades.x
        * distance(ray, froxel_ray(uv + vec2<f32>(sharp.params.w / sharp.size.z, 0.0)));
    let map_size = f32(textureDimensions(sun_shadow_map).x);
    let last_cascade = i32(fx.counts.z) - 1;
    var cascade_texel = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    for (var c = 0; c < 4; c++) {
        let m = fx.sun_cascades[c];
        cascade_texel[c] = 2.0 / max(length(vec3<f32>(m[0].x, m[1].x, m[2].x)) * map_size, 1e-6);
    }
    for (var i = 0u; i < steps; i++) {
        let s = start + (f32(i) + j) * dt;
        let p = fx.eye.xyz + ray * s;
        var m = density_base(p);
        if (m.a <= 1e-7) {
            continue;
        }
        if (fx.noise.x > 0.0) {
            // The noise is costly per step: take the froxels' extinction (they evaluated it at
            // their own resolution) with the medium's albedo.
            let w = depth_slice(s * cos_v);
            let e = textureSampleLevel(froxel_lit, linear_clamp, vec3<f32>(uv, w), 0.0).a;
            if (e <= 1e-7) {
                continue;
            }
            m *= e / m.a;
        }
        var lit = 1.0;
        if (li.shadow.x > 0.5) {
            while (cascade < last_cascade && cascade_texel[cascade] < s * footprint) {
                cascade++;
            }
            let sc = sun_cascade_shadow_from(p, cascade);
            cascade = i32(sc.y);
            lit = mix(1.0, sc.x, li.shadow.z);
        }
        lit *= cloud_shadow_lookup(cloud_shadow_map, linear_clamp, cloud_apply, p);
        // Energy-conserving step, as in froxel_integrate.wgsl.
        let t = exp(-m.a * dt);
        scatter += transmittance * m.rgb * lit * (1.0 - t) / m.a;
        transmittance *= t;
        if (transmittance < 1e-3) {
            break;
        }
    }
    // Lights are in surface units, hence the PI (as in froxel_light.wgsl).
    let light = li.color.rgb * phase(fx.height_fog.w, dot(ray, l)) * PI;
    textureStore(march_out, id.xy, vec4<f32>(scatter * light, depth));
}

@compute @workgroup_size(8, 8, 1)
fn accumulate(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(sharp.size.xy);
    let c = vec2<i32>(id.xy);
    if (any(c >= size)) {
        return;
    }
    let cur = textureLoad(raw, c, 0);
    let weight = sharp.params.z;
    if (weight <= 0.0) {
        textureStore(accum_out, c, cur);
        return;
    }
    // 3×3 neighbourhood: min/max box tightened to mean ± 1 sigma (variance clipping).
    var lo = cur.rgb;
    var hi = cur.rgb;
    var m1 = vec3<f32>(0.0);
    var m2 = vec3<f32>(0.0);
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let q = clamp(c + vec2<i32>(dx, dy), vec2<i32>(0), size - 1);
            let s = textureLoad(raw, q, 0).rgb;
            lo = min(lo, s);
            hi = max(hi, s);
            m1 += s;
            m2 += s * s;
        }
    }
    let mean = m1 / 9.0;
    let sigma = sqrt(max(m2 / 9.0 - mean * mean, vec3<f32>(0.0)));
    let box_lo = min(max(lo, mean - sigma), cur.rgb);
    let box_hi = max(min(hi, mean + sigma), cur.rgb);

    // Where this texel's surface point was last frame.
    let p = froxel_world(texel_uv(id.xy), cur.a);
    let clip = fx.prev_view_proj * vec4<f32>(p, 1.0);
    var result = cur;
    if (clip.w > 0.0) {
        let ndc = clip.xy / clip.w;
        let prev_uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        // Full-resolution pixel → march texel coordinates (inverse of `sharp_pixel`).
        let scale = sharp.params.w;
        let k = (prev_uv * sharp.size.zw - 0.5 - floor(scale * 0.5)) / scale;
        let huv = (k + 0.5) / sharp.size.xy;
        if (all(huv >= vec2<f32>(0.0)) && all(huv <= vec2<f32>(1.0))) {
            let h = textureSampleLevel(history, linear_clamp, huv, 0.0);
            let expected = dot(p - fx.prev_eye.xyz, fx.prev_forward.xyz);
            // Disocclusion: the history saw a different surface.
            if (abs(h.a - expected) < 0.1 * max(expected, 0.1)) {
                // A still view keeps its history unclamped (a thin shaft is hit by only a few
                // rays per frame, so this frame's neighbourhood can be all dark); once the view
                // moves by half a pixel the clamp holds the lines in place.
                let motion = length((prev_uv - texel_uv(id.xy)) * sharp.size.zw);
                let tight = smoothstep(0.1, 0.5, motion);
                let kept = mix(h.rgb, clamp(h.rgb, box_lo, box_hi), tight);
                result = vec4<f32>(mix(cur.rgb, kept, weight), cur.a);
            }
        }
    }
    textureStore(accum_out, c, result);
}
