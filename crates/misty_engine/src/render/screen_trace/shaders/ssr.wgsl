// Screen Space Reflections (appended to trace_common.wgsl), three passes at SSR resolution:
//   cs_ssr_trace    - the mirror ray of each smooth enough pixel, traced in screen space
//                     (pixels + device depth, which is affine along a projected line) through
//                     the closest-depth pyramid: it climbs a level after every empty cell and
//                     descends where the ray might pass behind the closest surface, testing
//                     the thickness at level 0. The hit's light comes from last frame's lit
//                     colour (reprojected) at a mip matched to the roughness cone.
//                     Alpha = confidence: faded by roughness, screen edges, ray length and
//                     back-facing hits (the lit shader keeps the probe / sky reflection for
//                     the rest).
//   cs_ssr_temporal - reprojected history clamped to the 3×3 neighbourhood of this frame.
//   cs_ssr_blur     - edge-aware disc blur sized by the pixel's roughness (premultiplied by
//                     confidence so misses don't darken hits).

fn ssr_scale() -> f32 {
    return p.clip.w;
}

fn ssr_size() -> vec2<i32> {
    return vec2<i32>(p.size2.xy);
}

fn hiz_load(cell: vec2<i32>, level: i32) -> f32 {
    let dims = vec2<i32>(textureDimensions(hiz_tex, level));
    return textureLoad(hiz_tex, clamp(cell, vec2<i32>(0), dims - 1), level).r;
}

// Screen position (full pixels) + device depth of a world point (no clipping).
fn to_screen(world: vec3<f32>) -> vec3<f32> {
    let c = p.view_proj * vec4<f32>(world, 1.0);
    let ndc = c.xyz / c.w;
    return vec3<f32>((ndc.x * 0.5 + 0.5) * p.size.x, (0.5 - ndc.y * 0.5) * p.size.y, ndc.z);
}

struct Hit {
    // Full pixel of the hit (valid when found).
    px: vec2<f32>,
    // 0..1 along the traced segment.
    t: f32,
    found: bool,
};

fn hiz_trace(o: vec3<f32>, dv: vec3<f32>) -> Hit {
    var out: Hit;
    out.found = false;
    let len_px = length(dv.xy);
    let max_level = i32(p.size2.w) - 1;
    let iterations = u32(p.ssr.z);
    let eps = 0.05 / len_px;
    var t = 1.0 / len_px;
    var level = 0;
    for (var i = 0u; i < iterations; i++) {
        if (t > 1.0) {
            break;
        }
        let pos = o + dv * t;
        if (any(pos.xy < vec2<f32>(0.0)) || any(pos.xy >= p.size.xy)) {
            break;
        }
        let cs = exp2(f32(level));
        let cell = floor(pos.xy / cs);
        let lo = cell * cs;
        let hi = lo + cs;
        var tx = 2.0;
        if (dv.x > 0.0) {
            tx = (hi.x - o.x) / dv.x;
        } else if (dv.x < 0.0) {
            tx = (lo.x - o.x) / dv.x;
        }
        var ty = 2.0;
        if (dv.y > 0.0) {
            ty = (hi.y - o.y) / dv.y;
        } else if (dv.y < 0.0) {
            ty = (lo.y - o.y) / dv.y;
        }
        let t_exit = min(min(tx, ty), 1.0);
        let closest = hiz_load(vec2<i32>(cell), level);
        let z_exit = o.z + dv.z * t_exit;
        let back = max(pos.z, z_exit);
        if (back < closest) {
            // The ray stays in front of everything in this cell.
            t = t_exit + eps;
            level = min(level + 1, max_level);
        } else if (level > 0) {
            // It may pass behind something here: skip to where it reaches that depth, refine.
            if (pos.z < closest && dv.z > 0.0) {
                t = clamp((closest - o.z) / dv.z, t, t_exit);
            }
            level -= 1;
        } else {
            let surface = linearize(closest);
            let ray = linearize(clamp(pos.z, 0.0, 1.0));
            if (ray - surface < p.ssr.y + 0.01 * surface) {
                out.px = pos.xy;
                out.t = t;
                out.found = true;
                return out;
            }
            // Behind a thin object: carry on past it.
            t = t_exit + eps;
            level = min(level + 1, max_level);
        }
    }
    return out;
}

@compute @workgroup_size(8, 8)
fn cs_ssr_trace(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = vec2<i32>(id.xy);
    if (any(t >= ssr_size())) {
        return;
    }
    let fp = full_px(t, ssr_scale());
    let d = load_depth(fp);
    let surf = surface_at(fp);
    let rough = 1.0 - surf.w;
    if (d >= 1.0 || surf.w < 0.0 || rough > p.ssr.x || p.prev_forward.w < 0.5) {
        textureStore(out_a, t, vec4<f32>(0.0));
        return;
    }
    let pc = vec2<f32>(fp) + 0.5;
    let pos = world_at(pc, d);
    let n = surf.xyz;
    let z0 = lin_depth(pos);
    let r = reflect(-to_eye(pos), n);
    // Keep the far end in front of the near plane.
    var len = p.ssr2.y;
    let rz = dot(r, p.forward.xyz);
    if (rz < 0.0) {
        len = min(len, (z0 - p.clip.x * 1.5) / -rz);
    }
    let start = pos + n * (0.002 * z0 + 0.005);
    let o = vec3<f32>(pc, to_screen(start).z);
    let e = to_screen(start + r * len);
    let dv = e - o;
    if (len <= 0.0 || length(dv.xy) < 1.0) {
        textureStore(out_a, t, vec4<f32>(0.0));
        return;
    }
    let hit = hiz_trace(o, dv);
    if (!hit.found) {
        textureStore(out_a, t, vec4<f32>(0.0));
        return;
    }
    let hp = vec2<i32>(hit.px);
    // Back faces of the hit surface are not on screen.
    let facing = smoothstep(0.0, 0.2, -dot(surface_at(hp).xyz, r));
    // Last frame's lit colour of the hit, through the hit pixel's motion vector.
    let prev = reproject_px(hp, vec2<f32>(hp) + 0.5);
    if (prev.w < 0.5 || facing <= 0.0) {
        textureStore(out_a, t, vec4<f32>(0.0));
        return;
    }
    let uv = vec2<f32>(hit.px) / p.size.xy;
    let edge = max(p.ssr.w, 1e-3);
    let border = min(min(uv, 1.0 - uv), min(prev.xy, 1.0 - prev.xy));
    let edge_fade = smoothstep(0.0, edge, border.x) * smoothstep(0.0, edge, border.y);
    let rough_fade = 1.0 - smoothstep(p.ssr.x * 0.7, p.ssr.x, rough);
    let far_fade = 1.0 - smoothstep(0.8, 1.0, hit.t);
    // The reflected cone widens with roughness and distance.
    let dist_px = length(hit.px - pc);
    let lod = clamp(log2(max(rough * dist_px * 0.5, 1.0)), 0.0, p.size2.z - 1.0);
    let color = min(textureSampleLevel(color_tex, linear_sampler, prev.xy, lod).rgb, vec3<f32>(64.0));
    textureStore(out_a, t, vec4<f32>(color, edge_fade * rough_fade * far_fade * facing));
}

// in_a = this frame's trace, in_b = last frame's result; out_a = this frame's.
@compute @workgroup_size(8, 8)
fn cs_ssr_temporal(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = vec2<i32>(id.xy);
    let size = ssr_size();
    if (any(t >= size)) {
        return;
    }
    let current = textureLoad(in_a, t, 0);
    let weight = p.ssr2.x;
    let fp = full_px(t, ssr_scale());
    let d = load_depth(fp);
    if (weight <= 0.0 || d >= 1.0) {
        textureStore(out_a, t, current);
        return;
    }
    let prev = reproject_px(fp, vec2<f32>(fp) + 0.5);
    if (prev.w < 0.5) {
        textureStore(out_a, t, current);
        return;
    }
    var lo = current;
    var hi = current;
    for (var k = 0; k < 9; k++) {
        let q = clamp(t + vec2<i32>(k % 3 - 1, k / 3 - 1), vec2<i32>(0), size - 1);
        let c = textureLoad(in_a, q, 0);
        lo = min(lo, c);
        hi = max(hi, c);
    }
    let q = clamp(vec2<i32>(prev.xy * vec2<f32>(size)), vec2<i32>(0), size - 1);
    let history = clamp(textureLoad(in_b, q, 0), lo, hi);
    textureStore(out_a, t, mix(current, history, weight));
}

@compute @workgroup_size(8, 8)
fn cs_ssr_blur(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = vec2<i32>(id.xy);
    let size = ssr_size();
    if (any(t >= size)) {
        return;
    }
    let center = textureLoad(in_a, t, 0);
    let fp = full_px(t, ssr_scale());
    let surf = surface_at(fp);
    let rough = 1.0 - max(surf.w, 0.0);
    // Disc radius in SSR texels: up to ~4 % of the image height at the roughness cut-off.
    let radius = rough * rough * 0.08 * f32(size.y);
    let d = load_depth(fp);
    if (radius < 0.75 || d >= 1.0) {
        textureStore(out_a, t, center);
        return;
    }
    let pos = world_at(vec2<f32>(fp) + 0.5, d);
    let z = lin_depth(pos);
    var sum = vec4<f32>(center.rgb * center.a, center.a);
    var wsum = 1.0;
    let spin = ign(vec2<f32>(t) + p.forward.w * 3.7) * 6.2831853;
    for (var i = 0; i < 12; i++) {
        // Vogel spiral.
        let k = f32(i) + 0.5;
        let a = k * 2.3999632 + spin;
        let o = vec2<f32>(cos(a), sin(a)) * radius * sqrt(k / 12.0);
        let q = clamp(t + vec2<i32>(round(o)), vec2<i32>(0), size - 1);
        let fq = full_px(q, ssr_scale());
        let dq = load_depth(fq);
        if (dq >= 1.0) {
            continue;
        }
        let w = edge_weight(pos, surf.xyz, z, world_at(vec2<f32>(fq) + 0.5, dq), surface_at(fq).xyz, 2.0)
            * exp(-2.0 * k / 12.0);
        let c = textureLoad(in_a, q, 0);
        sum += vec4<f32>(c.rgb * c.a, c.a) * w;
        wsum += w;
    }
    let a = sum.a / wsum;
    let rgb = select(vec3<f32>(0.0), sum.rgb / max(sum.a, 1e-5), sum.a > 1e-5);
    textureStore(out_a, t, vec4<f32>(rgb, a));
}
