// Rectangle area lights with linearly transformed cosines (render/area). Appended after
// mesh.wgsl and shadows.wgsl (uses `LightItem`, `shadow_factor`, `contact_shadow`).
// A cosine integrated over a polygon has a closed form: transform the corners, clip them to the
// upper hemisphere and sum each edge's angle × the z of its plane normal. Lambert diffuse uses
// the plain cosine; GGX uses the table's inverse matrix, which turns the lobe into a cosine.
// The table (fitted by render/area/ltc_fit.rs) also holds the lobe's Fresnel scale and bias.

// LTC_SIZE² inverse matrices [a, b, d, e], then LTC_SIZE² magnitudes [scale, bias, -, -];
// row = √(1 - n·v), column = perceptual roughness.
@group(1) @binding(7) var<storage, read> ltc_table: array<vec4<f32>>;

const LTC_SIZE: u32 = 64u;

// Bilinear lookup; `base` = 0 (matrices) or LTC_SIZE² (magnitudes).
fn ltc_fetch(base: u32, uv: vec2<f32>) -> vec4<f32> {
    let last = f32(LTC_SIZE - 1u);
    let p = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)) * last;
    let p0 = min(floor(p), vec2<f32>(last - 1.0));
    let f = p - p0;
    let i = base + u32(p0.x) + u32(p0.y) * LTC_SIZE;
    let top = mix(ltc_table[i], ltc_table[i + 1u], f.x);
    let bottom = mix(ltc_table[i + LTC_SIZE], ltc_table[i + LTC_SIZE + 1u], f.x);
    return mix(top, bottom, f.y);
}

// θ / sin θ × (a × b).z for unit a, b.
fn ltc_edge(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let x = clamp(dot(a, b), -1.0, 1.0);
    var k = 1.0 + (1.0 - x) / 3.0;
    if (x <= 0.9999) {
        k = acos(x) / sqrt(max(1.0 - x * x, 1e-12));
    }
    return cross(a, b).z * k;
}

// Running sum of the clipped polygon's edges, and where its outline leaves and re-enters the
// upper hemisphere (a convex polygon crosses the horizon at most twice).
struct LtcClip {
    sum: f32,
    exit: vec3<f32>,
    entry: vec3<f32>,
    clipped: bool,
};

// Add the part of edge a → b above the horizon. Plain values, no indexed arrays: local arrays
// indexed in loops spill to slow memory on many GPUs.
fn ltc_side(c: LtcClip, a: vec3<f32>, b: vec3<f32>) -> LtcClip {
    var o = c;
    let up_a = a.z >= 0.0;
    let up_b = b.z >= 0.0;
    if (up_a && up_b) {
        o.sum += ltc_edge(normalize(a), normalize(b));
    } else if (up_a != up_b) {
        let p = normalize(mix(a, b, a.z / (a.z - b.z)));
        o.clipped = true;
        if (up_a) {
            o.sum += ltc_edge(normalize(a), p);
            o.exit = p;
        } else {
            o.sum += ltc_edge(p, normalize(b));
            o.entry = p;
        }
    }
    return o;
}

// Signed cosine integral of the quad q0..q3 (shading frame) after `minv`: the clipped
// polygon's edges plus, when clipped, the closing edge along the horizon.
fn ltc_integrate(minv: mat3x3<f32>, q0: vec3<f32>, q1: vec3<f32>, q2: vec3<f32>, q3: vec3<f32>) -> f32 {
    let a = minv * q0;
    let b = minv * q1;
    let c = minv * q2;
    let d = minv * q3;
    if (max(max(a.z, b.z), max(c.z, d.z)) <= 0.0) {
        return 0.0;
    }
    var s = LtcClip(0.0, vec3<f32>(0.0), vec3<f32>(0.0), false);
    s = ltc_side(s, a, b);
    s = ltc_side(s, b, c);
    s = ltc_side(s, c, d);
    s = ltc_side(s, d, a);
    if (s.clipped) {
        s.sum += ltc_edge(s.exit, s.entry);
    }
    return s.sum / 6.2831853;
}

// Range window at the rectangle's closest point (no inverse-square: the solid angle falls off).
fn rect_window(li: LightItem, world: vec3<f32>) -> f32 {
    let d = world - li.pos_range.xyz;
    let ax = li.area_x.xyz;
    let ay = li.area_y.xyz;
    let sx = clamp(dot(d, ax) / max(dot(ax, ax), 1e-8), -1.0, 1.0);
    let sy = clamp(dot(d, ay) / max(dot(ay, ay), 1e-8), -1.0, 1.0);
    let closest = li.pos_range.xyz + ax * sx + ay * sy;
    let x = distance(world, closest) / max(li.pos_range.w, 1e-4);
    let w = clamp(1.0 - x * x * x * x, 0.0, 1.0);
    return w * w;
}

struct RectTerms {
    // Lambert cosine integral (× albedo × radiance = diffuse light).
    diffuse: f32,
    // GGX × Fresnel integral (× radiance = specular light).
    specular: vec3<f32>,
};

// The two integrals of rectangle light `li` at a point with normal `n` and view `v`.
fn rect_light(li: LightItem, world: vec3<f32>, n: vec3<f32>, v: vec3<f32>, perceptual: f32, f0: vec3<f32>) -> RectTerms {
    // Shading frame with the view in its XZ plane.
    var t1 = v - n * dot(n, v);
    if (dot(t1, t1) < 1e-8) {
        t1 = cross(n, select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(n.x) > 0.9));
    }
    t1 = normalize(t1);
    let to_local = transpose(mat3x3<f32>(t1, cross(n, t1), n));
    let c = li.pos_range.xyz - world;
    let ax = li.area_x.xyz;
    let ay = li.area_y.xyz;
    var out: RectTerms;
    let two_sided = li.area_x.w > 0.5;
    // Behind a one-sided light: nothing to integrate.
    if (!two_sided && dot(c, cross(ax, ay)) >= 0.0) {
        return out;
    }
    // Counter-clockwise seen from the lit side: positive in front of the light.
    let q0 = to_local * (c - ax + ay);
    let q1 = to_local * (c + ax + ay);
    let q2 = to_local * (c + ax - ay);
    let q3 = to_local * (c - ax - ay);
    let identity = mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    var diffuse = ltc_integrate(identity, q0, q1, q2, q3);
    let uv = vec2<f32>(perceptual, sqrt(1.0 - clamp(dot(n, v), 0.0, 1.0)));
    let m = ltc_fetch(0u, uv);
    let minv = mat3x3<f32>(vec3<f32>(m.x, 0.0, m.z), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(m.y, 0.0, m.w));
    var spec = ltc_integrate(minv, q0, q1, q2, q3);
    if (two_sided) {
        diffuse = abs(diffuse);
        spec = abs(spec);
    } else {
        diffuse = max(diffuse, 0.0);
        spec = max(spec, 0.0);
    }
    let s = ltc_fetch(LTC_SIZE * LTC_SIZE, uv);
    out.diffuse = diffuse;
    out.specular = spec * (f0 * s.x + vec3<f32>(s.y));
    return out;
}

// Light from rectangle light `i` reflected towards the eye: diffuse + GGX, windowed by range,
// shadowed by the map rendered from its centre (like a spot light) and contact shadows.
fn rect_shade(i: u32, li: LightItem, world: vec3<f32>, receive_shadows: bool, n: vec3<f32>, v: vec3<f32>, perceptual: f32, f0: vec3<f32>, diffuse_color: vec3<f32>, ssao: f32) -> vec3<f32> {
    var atten = rect_window(li, world);
    if (atten <= 0.0) {
        return vec3<f32>(0.0);
    }
    let t = rect_light(li, world, n, v, perceptual, f0);
    if (t.diffuse <= 0.0 && all(t.specular <= vec3<f32>(0.0))) {
        return vec3<f32>(0.0);
    }
    if (receive_shadows) {
        let l = normalize(li.pos_range.xyz - world);
        atten *= shadow_factor(i, li, world, n, l);
        atten *= contact_shadow(li, world, n, l);
    }
    atten *= direct_occlusion(ssao);
    return (diffuse_color * t.diffuse + t.specular) * li.color.rgb * atten;
}
