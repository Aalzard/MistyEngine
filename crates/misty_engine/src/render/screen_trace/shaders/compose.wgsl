// Final pass of the screen-space trace (appended to trace_common.wgsl): bilateral upsample of
// the GI (in_a) and SSR (in_b) images to full resolution, packed as f16 pairs into the
// texels the lit shaders read (screen_trace.wgsl).

// Half float bits of a non-negative value (no f16 shader capability needed; read back by
// `screen_trace_half`).
fn half_bits(v: f32) -> u32 {
    let x = clamp(v, 0.0, 65504.0);
    if (x < 6.103515625e-5) {
        return u32(round(x * 16777216.0));
    }
    var e = floor(log2(x));
    if (x < exp2(e)) {
        e -= 1.0;
    } else if (x >= exp2(e + 1.0)) {
        e += 1.0;
    }
    let m = x / exp2(e) - 1.0;
    // A mantissa that rounds up to 1024 carries into the exponent.
    return min((u32(e + 15.0) << 10u) + u32(round(m * 1024.0)), 0x7bffu);
}

fn pack_halves(v: vec2<f32>) -> u32 {
    return half_bits(v.x) | (half_bits(v.y) << 16u);
}

@compute @workgroup_size(8, 8)
fn cs_compose(@builtin(global_invocation_id) id: vec3<u32>) {
    let fp = vec2<i32>(id.xy);
    if (any(fp >= full_size())) {
        return;
    }
    var gi = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    var ssr = vec4<f32>(0.0);
    if (load_depth(fp) < 1.0) {
        if (p.gi2.y > 0.5) {
            gi = upsample(in_a, p.size.zw, p.clip.z, fp);
        }
        if (p.misty.y > 0.5) {
            // Misty GI fills the share of SSGI's rays that hit nothing on screen (all of it
            // with SSGI off); the ambient is then fully replaced (visibility 0).
            let m = upsample(in_c, ceil(p.size.xy / p.misty.x), p.misty.x, fp);
            let visible = mix(1.0, gi.a, min(p.misty.w, 1.0));
            gi = vec4<f32>(gi.rgb * p.misty.w + visible * m.rgb * p.misty.z, 0.0);
        }
        if (p.gi2.z > 0.5) {
            ssr = upsample(in_b, p.size2.xy, p.clip.w, fp);
        }
        if (p.misty2.x > 0.5) {
            // Misty GI's glossy reflection behind SSR's: SSR wins where it is confident.
            let m = upsample(in_d, ceil(p.size.xy / p.misty.x), p.misty.x, fp);
            let a = ssr.a + m.a * (1.0 - ssr.a);
            if (a > 1e-4) {
                ssr = vec4<f32>((ssr.rgb * ssr.a + m.rgb * m.a * (1.0 - ssr.a)) / a, a);
            }
        }
    }
    let i = u32(fp.y) * u32(p.size.x) + u32(fp.x);
    trace_out.texels[i] = vec4<u32>(
        pack_halves(gi.rg),
        pack_halves(vec2<f32>(gi.b, saturate(gi.a))),
        pack_halves(ssr.rg),
        pack_halves(vec2<f32>(ssr.b, saturate(ssr.a))),
    );
}
