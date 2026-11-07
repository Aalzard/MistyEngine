// Surface (radiance) cache (appended to gi_common.wgsl): every voxel near a surface stores the
// diffuse radiance leaving it — direct light from every light with an SDF soft shadow, the
// probes' indirect light from the last frame (one more bounce each frame) and emission.
// Voxels away from surfaces store nothing (a = 0).

fn level_texel(k: i32, l: vec3<i32>) -> vec3<i32> {
    let last = i32(dim()) - 1;
    let c = clamp(l, vec3<i32>(0), vec3<i32>(last));
    return vec3<i32>(c.x, c.y, c.z + k * i32(dim()));
}

fn sdf_load(k: i32, l: vec3<i32>) -> f32 {
    return textureLoad(sdf_tex, level_texel(k, l), 0).x;
}

// Engine falloff of point and spot lights (mesh.wgsl).
fn light_falloff(li: GiLight, dist: f32) -> f32 {
    let x = dist / max(li.pos_range.w, 1e-4);
    let window = clamp(1.0 - x * x * x * x, 0.0, 1.0);
    return window * window / (1.0 + 25.0 * x * x);
}

// Direct diffuse light (without albedo) at surface point `ps` with normal `n`; `lift` = start
// offset of the shadow rays.
fn direct_light(ps: vec3<f32>, n: vec3<f32>, lift: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let count = u32(p.info3.x);
    let o = ps + n * lift;
    for (var i = 0u; i < count; i++) {
        let li = gi_lights[i];
        let kind = u32(li.dir_type.w);
        if (kind == 0u) {
            let l = -normalize(li.dir_type.xyz);
            let ndl = dot(n, l);
            if (ndl <= 0.0) {
                continue;
            }
            sum += li.color.rgb * ndl * soft_shadow(o, l, 1e4);
            continue;
        }
        let to_light = li.pos_range.xyz - ps;
        let dist = length(to_light);
        let l = to_light / max(dist, 1e-4);
        let ndl = dot(n, l);
        if (ndl <= 0.0 || dist > li.pos_range.w) {
            continue;
        }
        var amount = 0.0;
        if (kind == 3u) {
            // Rectangle: form factor of a small emitter (area × both cosines / (π d² + area)),
            // emitting along cross(x axis, y axis).
            let ax = li.area_x.xyz;
            let ay = li.area_y.xyz;
            let face = cross(ax, ay);
            let area = 4.0 * length(face);
            var cos_e = dot(normalize(face), -l);
            if (li.area_x.w > 0.5) {
                cos_e = abs(cos_e);
            }
            if (cos_e <= 0.0) {
                continue;
            }
            let x = dist / max(li.pos_range.w, 1e-4);
            let window = clamp(1.0 - x * x * x * x, 0.0, 1.0);
            amount = area * ndl * cos_e / (PI * dist * dist + area) * window * window;
        } else {
            amount = light_falloff(li, dist) * ndl;
            if (kind == 2u) {
                amount *= smoothstep(li.spot.x, li.spot.y, dot(-l, normalize(li.dir_type.xyz)));
            }
        }
        if (amount <= 0.0) {
            continue;
        }
        sum += li.color.rgb * amount * soft_shadow(o, l, max(dist - 2.0 * lift, 0.0));
    }
    return sum;
}

@compute @workgroup_size(4, 4, 4)
fn cs_surface(@builtin(global_invocation_id) id: vec3<u32>) {
    let d = u32(dim());
    let k = i32(id.z / d);
    if (id.x >= d || id.y >= d || k >= level_count()) {
        return;
    }
    let texel = vec3<i32>(id);
    let l = vec3<i32>(i32(id.x), i32(id.y), i32(id.z) - k * i32(d));
    let v = voxel(k);
    let here = textureLoad(sdf_tex, texel, 0);
    let s = here.x;
    if (s > 2.0 * v || s < -v) {
        textureStore(radiance_out, texel, vec4<f32>(0.0));
        textureStore(cache_normal_out, texel, vec4<f32>(0.0));
        return;
    }
    let grad = vec3<f32>(
        sdf_load(k, l + vec3<i32>(1, 0, 0)) - sdf_load(k, l - vec3<i32>(1, 0, 0)),
        sdf_load(k, l + vec3<i32>(0, 1, 0)) - sdf_load(k, l - vec3<i32>(0, 1, 0)),
        sdf_load(k, l + vec3<i32>(0, 0, 1)) - sdf_load(k, l - vec3<i32>(0, 0, 1)),
    );
    let n = select(vec3<f32>(0.0, 1.0, 0.0), normalize(grad), dot(grad, grad) > 1e-10);
    let pos = p.levels[k].sdf.xyz + (vec3<f32>(l) + 0.5) * v;
    // Closest surface point.
    let ps = pos - n * s;
    let albedo = here.yzw;
    let emission = textureLoad(emission_tex, texel, 0).rgb;
    let direct = direct_light(ps, n, 1.5 * v);
    let indirect = gi_irradiance(ps + n * (0.5 * v), n, 2);
    var radiance = albedo * (direct + indirect) + emission;
    if (any(radiance != radiance)) {
        radiance = vec3<f32>(1000.0, 0.0, 0.0);
    }
    textureStore(radiance_out, texel, vec4<f32>(min(radiance, vec3<f32>(64.0)), 1.0));
    textureStore(cache_normal_out, texel, vec4<f32>(n, 0.0));
}
