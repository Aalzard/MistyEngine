// Froxel injection + lighting + temporal reprojection (one pass, one thread per froxel).
// Density: global height fog, local fog boxes and optional scrolling noise.
// Lighting: directional / spot / point lights with their shadow maps (Henyey-Greenstein
// phase) plus ambient sky light. The result is blended with last frame's reprojected value
// and written as rgb = in-scattering (1/m), a = extinction (1/m).

@group(0) @binding(1) var history: texture_3d<f32>;
@group(0) @binding(3) var lit_out: texture_storage_3d<rgba16float, write>;

// Light arriving at `p` and scattered towards the eye along `-view_dir`.
fn in_scatter(p: vec3<f32>, view_dir: vec3<f32>) -> vec3<f32> {
    // Ambient sky light: the phase integrates to 1 over the sphere.
    var light = fx.ambient.rgb;
    let g = fx.height_fog.w;
    let count = u32(fx.counts.x);
    for (var i = 0u; i < count; i++) {
        // Sharp sun shafts on: the main sun's in-scatter is marched per pixel instead.
        if (fx.sun.y > 0.5 && i32(i) == i32(fx.sun.x)) {
            continue;
        }
        let li = fx.lights[i];
        let kind = u32(li.dir_type.w);
        var l: vec3<f32>;
        var atten = 1.0;
        if (kind == 0u) {
            l = -normalize(li.dir_type.xyz);
        } else {
            let to_light = li.pos_range.xyz - p;
            let dist = length(to_light);
            l = to_light / max(dist, 1e-4);
            // Same falloff as surfaces (mesh.wgsl).
            let x = dist / max(li.pos_range.w, 1e-4);
            let window = clamp(1.0 - x * x * x * x, 0.0, 1.0);
            atten = window * window / (1.0 + 25.0 * x * x);
            if (kind == 2u) {
                atten *= smoothstep(li.spot.x, li.spot.y, dot(-l, normalize(li.dir_type.xyz)));
            }
            if (kind == 3u) {
                // Rectangle: an emitter at its centre covering the rectangle's solid angle
                // (area × facing cosine / d², kept finite near it), ÷ PI for surface units.
                var facing = dot(-l, normalize(li.dir_type.xyz));
                if (li.spot.y > 0.5) {
                    facing = abs(facing);
                }
                let solid = li.spot.x * max(facing, 0.0) / (dist * dist + li.spot.x / PI);
                atten = window * window * solid / PI;
            }
        }
        if (atten <= 0.0) {
            continue;
        }
        atten *= light_shadow(li, p);
        if (kind == 0u) {
            // Volumetric clouds shade the sun (god rays through the gaps).
            atten *= cloud_shadow_lookup(cloud_shadow_map, linear_clamp, cloud_apply, p);
        }
        // Lights are in surface units (a white wall facing the light = colour); a medium that
        // scatters evenly returns a quarter of that, hence the PI.
        light += li.color.rgb * atten * phase(g, dot(view_dir, l)) * PI;
    }
    return light;
}

@compute @workgroup_size(4, 4, 4)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec3<u32>(fx.grid.xyz);
    if (any(id >= size)) {
        return;
    }
    let n = vec3<f32>(size);
    let cell = vec3<f32>(id);
    let uv = (cell.xy + fx.jitter.xy) / n.xy;
    let p = froxel_world(uv, slice_depth((cell.z + fx.jitter.z) / n.z));
    var view_dir = normalize(p - fx.eye.xyz);
    if (fx.eye.w > 0.5) {
        view_dir = fx.forward.xyz;
    }
    var result = vec4<f32>(0.0);
    let m = density(p);
    if (m.a > 1e-7) {
        result = vec4<f32>(m.rgb * in_scatter(p, view_dir), m.a);
    }

    // Temporal: blend with where this froxel's centre was last frame.
    let weight = fx.prev_forward.w;
    if (weight > 0.0) {
        let center = froxel_world((cell.xy + 0.5) / n.xy, slice_depth((cell.z + 0.5) / n.z));
        let clip = fx.prev_view_proj * vec4<f32>(center, 1.0);
        if (clip.w > 0.0) {
            let ndc = clip.xy / clip.w;
            let prev_uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
            let prev_w = depth_slice(dot(center - fx.prev_eye.xyz, fx.prev_forward.xyz));
            let uvw = vec3<f32>(prev_uv, prev_w);
            if (all(uvw >= vec3<f32>(0.0)) && all(uvw <= vec3<f32>(1.0))) {
                let h = textureSampleLevel(history, linear_clamp, uvw, 0.0);
                result = mix(result, h, weight);
            }
        }
    }
    textureStore(lit_out, id, result);
}
