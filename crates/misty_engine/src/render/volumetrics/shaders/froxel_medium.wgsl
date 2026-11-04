// The participating medium shared by the froxel lighting pass and the sharp sun shafts
// (included after froxel_common.wgsl): density (height fog, Fog Volumes, noise), the phase
// function and the shadow-map lookups. Same bindings in both pipelines.

@group(0) @binding(2) var linear_clamp: sampler;
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
    var m = density_base(p);
    let amount = fx.noise.x;
    if (amount > 0.0 && m.a > 0.0) {
        let n = fbm((p - fx.scroll.xyz * fx.noise.z) * fx.noise.y);
        // Mean stays about 1, so the overall thickness doesn't change with the intensity.
        m *= mix(1.0, smoothstep(0.3, 0.7, n) * 2.0, amount);
    }
    return m;
}

// Height fog and Fog Volumes without the noise.
fn density_base(p: vec3<f32>) -> vec4<f32> {
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

// Sun: the first (tightest) cascade that contains the point. Cascades are nested around the
// camera, so this matches picking by view depth without needing it here. The comparison
// sampler's bilinear filter is a hard 2×2 PCF: shaft edges stay sharp (no PCSS in the fog).
fn sun_cascade_shadow(p: vec3<f32>) -> f32 {
    return sun_cascade_shadow_from(p, 0).x;
}

// x = lit, y = the cascade used (searching from `first`; the count when none contains `p`).
// A march away from the eye only moves outwards, so it passes the last cascade back in.
fn sun_cascade_shadow_from(p: vec3<f32>, first: i32) -> vec2<f32> {
    let count = i32(fx.counts.z);
    for (var c = first; c < count; c++) {
        let sc = shadow_coords(fx.sun_cascades[c], p);
        if (sc.z <= 1.0) {
            let lit = textureSampleCompareLevel(sun_shadow_map, shadow_cmp, sc.xy, c, sc.z - 0.0015);
            return vec2<f32>(lit, f32(c));
        }
    }
    return vec2<f32>(1.0, f32(count));
}

fn light_shadow(li: FroxelLight, p: vec3<f32>) -> f32 {
    if (li.shadow.x < 0.5) {
        return 1.0;
    }
    var layer = i32(li.shadow.y);
    var lit = 1.0;
    if (layer < 0) {
        lit = sun_cascade_shadow(p);
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
