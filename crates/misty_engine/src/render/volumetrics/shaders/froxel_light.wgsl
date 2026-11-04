// Froxel injection + lighting + temporal reprojection (one pass, one thread per froxel).
// Density: global height fog, local fog boxes and optional scrolling noise.
// Lighting: directional / spot / point lights with their shadow maps (Henyey-Greenstein
// phase) plus ambient sky light. The result is blended with last frame's reprojected value
// and written as rgb = in-scattering (1/m), a = extinction (1/m).

@group(0) @binding(1) var history: texture_3d<f32>;
@group(0) @binding(2) var linear_clamp: sampler;
@group(0) @binding(3) var lit_out: texture_storage_3d<rgba16float, write>;
@group(0) @binding(4) var sun_shadow_map: texture_depth_2d_array;
@group(0) @binding(5) var local_shadow_map: texture_depth_2d_array;
@group(0) @binding(6) var shadow_cmp: sampler_comparison;
@group(0) @binding(7) var cloud_shadow_map: texture_2d<f32>;
@group(0) @binding(8) var<uniform> cloud_apply: CloudApply;

fn hash(p: vec3<i32>) -> f32 {
    var n = u32(p.x) * 73856093u ^ u32(p.y) * 19349663u ^ u32(p.z) * 83492791u;
    n = (n ^ (n >> 13u)) * 1274126177u;
    n = n ^ (n >> 16u);
    return f32(n & 0xffffffu) / 16777215.0;
}

// Smooth 3D value noise in 0..1.
fn value_noise(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let x00 = mix(hash(i), hash(i + vec3<i32>(1, 0, 0)), u.x);
    let x10 = mix(hash(i + vec3<i32>(0, 1, 0)), hash(i + vec3<i32>(1, 1, 0)), u.x);
    let x01 = mix(hash(i + vec3<i32>(0, 0, 1)), hash(i + vec3<i32>(1, 0, 1)), u.x);
    let x11 = mix(hash(i + vec3<i32>(0, 1, 1)), hash(i + vec3<i32>(1, 1, 1)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

// Three octaves, 0..1.
fn fbm(p: vec3<f32>) -> f32 {
    return (value_noise(p) * 0.5 + value_noise(p * 2.03) * 0.25 + value_noise(p * 4.01) * 0.125) / 0.875;
}

// rgb = scattering, a = extinction at a world point.
fn density(p: vec3<f32>) -> vec4<f32> {
    var m = vec4<f32>(0.0);
    if (fx.albedo.w > 0.5) {
        let h = p.y - fx.height_fog.y;
        let e = fx.height_fog.x * select(exp(-h * fx.height_fog.z), 1.0, h <= 0.0);
        m += vec4<f32>(fx.albedo.rgb * e, e);
    }
    let boxes = u32(fx.counts.y);
    for (var i = 0u; i < boxes; i++) {
        let b = fx.boxes[i];
        let q = abs((b.world_to_box * vec4<f32>(p, 1.0)).xyz);
        if (any(q > vec3<f32>(1.0))) {
            continue;
        }
        let edge = clamp((1.0 - q) * b.half_ext.xyz / max(b.albedo.w, 1e-3), vec3<f32>(0.0), vec3<f32>(1.0));
        let e = b.half_ext.w * edge.x * edge.y * edge.z;
        m += vec4<f32>(b.albedo.rgb * e, e);
    }
    let amount = fx.noise.x;
    if (amount > 0.0 && m.a > 0.0) {
        let n = fbm((p - fx.scroll.xyz * fx.noise.z) * fx.noise.y);
        // Mean stays about 1, so the overall thickness doesn't change with the intensity.
        m *= mix(1.0, smoothstep(0.3, 0.7, n) * 2.0, amount);
    }
    return m;
}

// Henyey-Greenstein phase (normalised over the sphere); `c` = cosine of the scattering angle.
fn phase(g: f32, c: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(max(1.0 + g2 - 2.0 * g * c, 1e-4), 1.5));
}

// Shadow-map uv + depth of a world point, depth > 1 when outside the map.
fn shadow_coords(m: mat4x4<f32>, p: vec3<f32>) -> vec3<f32> {
    let c = m * vec4<f32>(p, 1.0);
    let ndc = c.xyz / c.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (c.w <= 0.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || ndc.z > 1.0) {
        return vec3<f32>(0.0, 0.0, 2.0);
    }
    return vec3<f32>(uv, ndc.z);
}

fn light_shadow(li: FroxelLight, p: vec3<f32>) -> f32 {
    if (li.shadow.x < 0.5) {
        return 1.0;
    }
    var layer = i32(li.shadow.y);
    var lit = 1.0;
    if (layer < 0) {
        // Sun: the first (tightest) cascade that contains the point. Cascades are nested
        // around the camera, so this matches picking by view depth without needing it here.
        for (var c = 0; c < i32(fx.counts.z); c++) {
            let sc = shadow_coords(fx.sun_cascades[c], p);
            if (sc.z <= 1.0) {
                lit = textureSampleCompareLevel(sun_shadow_map, shadow_cmp, sc.xy, c, sc.z - 0.0015);
                break;
            }
        }
    } else {
        if (u32(li.dir_type.w) == 1u) {
            let to_point = p - li.pos_range.xyz;
            let a = abs(to_point);
            if (a.x >= a.y && a.x >= a.z) {
                layer += select(1, 0, to_point.x > 0.0);
            } else if (a.y >= a.z) {
                layer += select(3, 2, to_point.y > 0.0);
            } else {
                layer += select(5, 4, to_point.z > 0.0);
            }
        }
        let sc = shadow_coords(fx.local_shadow[layer], p);
        if (sc.z <= 1.0) {
            lit = textureSampleCompareLevel(local_shadow_map, shadow_cmp, sc.xy, layer, sc.z - 0.0008);
        }
    }
    // Dimmer 0 = the light ignores its shadows in the fog.
    return mix(1.0, lit, li.shadow.z);
}

// Light arriving at `p` and scattered towards the eye along `-view_dir`.
fn in_scatter(p: vec3<f32>, view_dir: vec3<f32>) -> vec3<f32> {
    // Ambient sky light: the phase integrates to 1 over the sphere.
    var light = fx.ambient.rgb;
    let g = fx.height_fog.w;
    let count = u32(fx.counts.x);
    for (var i = 0u; i < count; i++) {
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
