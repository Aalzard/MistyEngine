// Cloud layer shared by the trace and shadow passes (render/clouds): the uniform, the noise
// textures and the density at a world point. The layer is a shell of a planet whose surface is
// the world's y = 0 plane under the camera.

struct Clouds {
    inv_view_proj: mat4x4<f32>,
    prev_view_proj: mat4x4<f32>,
    // xyz = eye, w = frame index.
    eye: vec4<f32>,
    // x = planet radius, y = base altitude, z = top altitude, w = max trace distance (m).
    layer: vec4<f32>,
    // x = coverage, y = extinction at full density (1/m), z = shape (0 fluffy, 1 sharp), w = time (s).
    shape: vec4<f32>,
    // xy = wind offset of the base shape (m, xz), zw = wind offset of the detail noise.
    wind: vec4<f32>,
    // xy = wind direction (xz, unit), z = lean of the tops downwind (m).
    wind_dir: vec4<f32>,
    // xyz = direction towards the sun, w = silver lining (0..1).
    sun_dir: vec4<f32>,
    // rgb = linear sunlight reaching the clouds, w = 1 when there is a sun.
    sun_color: vec4<f32>,
    // rgb = sky light from above.
    ambient_top: vec4<f32>,
    // rgb = light from the horizon and ground below.
    ambient_bottom: vec4<f32>,
    // rgb = horizon haze colour, w = distance where half of a cloud fades into it (m).
    haze: vec4<f32>,
    // x, y = trace size (texels), z = view steps, w = light steps.
    trace: vec4<f32>,
    // x = history weight (0 = no history), y = shadow map size, z = shadow map world size (m),
    // w = shadow steps.
    misc: vec4<f32>,
    // xy = shadow map origin (world xz of its corner).
    shadow: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cl: Clouds;
@group(0) @binding(1) var base_noise: texture_3d<f32>;
@group(0) @binding(2) var detail_noise: texture_3d<f32>;
@group(0) @binding(3) var weather_map: texture_2d<f32>;
@group(0) @binding(4) var blue_noise: texture_2d<f32>;
@group(0) @binding(5) var repeat_linear: sampler;

const PI_C: f32 = 3.14159265;
// World size of one tile of each noise (m).
const BASE_TILE: f32 = 5200.0;
const DETAIL_TILE: f32 = 640.0;
const WEATHER_TILE: f32 = 36000.0;

fn remap(x: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    return c + (x - a) / (b - a) * (d - c);
}

// `p` relative to the ground point under the eye (the planet's top). Every sphere test is
// written in these small numbers: the planet radius squared would lose all precision in f32.
fn local(p: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(p.x - cl.eye.x, p.y, p.z - cl.eye.z);
}

// Altitude of a world point above the curved ground.
fn altitude(p: vec3<f32>) -> f32 {
    let v = local(p);
    let r = cl.layer.x;
    return (dot(v, v) + 2.0 * r * v.y) / (2.0 * r + v.y);
}

// 0 at the cloud base, 1 at the top.
fn height_fraction(p: vec3<f32>) -> f32 {
    return clamp((altitude(p) - cl.layer.y) / max(cl.layer.z - cl.layer.y, 1.0), 0.0, 1.0);
}

// Coverage of the weather map at `p` with the Coverage control applied (0..1).
fn local_coverage(p: vec3<f32>) -> vec2<f32> {
    let w = textureSampleLevel(weather_map, repeat_linear, (p.xz + cl.wind.xy) / WEATHER_TILE, 0.0);
    let c = cl.shape.x;
    let cov = clamp((w.r - 0.5) * 0.9 + c * 1.15 - 0.05, 0.0, 1.0);
    return vec2<f32>(cov * smoothstep(0.0, 0.25, c), w.g);
}

// Vertical profile: rounded base, tops between half and full height depending on the type.
fn height_profile(h: f32, kind: f32) -> f32 {
    let top = mix(0.55, 1.0, kind);
    return smoothstep(0.0, 0.1, h) * (1.0 - smoothstep(top * 0.55, top, h));
}

// Base-shape density (no detail erosion) at `p`, 0..1. `cov` from `local_coverage`.
fn base_density(p: vec3<f32>, h: f32, cov: vec2<f32>) -> f32 {
    if (cov.x <= 0.0) {
        return 0.0;
    }
    let lean = cl.wind_dir.xy * cl.wind_dir.z * h;
    let q = vec3<f32>(p.x + cl.wind.x + lean.x, p.y, p.z + cl.wind.y + lean.y) / BASE_TILE;
    let n = textureSampleLevel(base_noise, repeat_linear, q, 0.0);
    let low = n.g * 0.625 + n.b * 0.25 + n.a * 0.125;
    var d = clamp(remap(n.r, low - 1.0, 1.0, 0.0, 1.0), 0.0, 1.0);
    d *= height_profile(h, cov.y);
    // Coverage carves the shapes: low coverage keeps only the densest cores.
    d = clamp(remap(d, 1.0 - cov.x, 1.0, 0.0, 1.0), 0.0, 1.0) * cov.x;
    return d;
}

// Full density (0..1): base shape eroded by the detail noise (wispy at the base, billowy above).
fn full_density(p: vec3<f32>, h: f32, base: f32) -> f32 {
    let q = vec3<f32>(p.x + cl.wind.z, p.y, p.z + cl.wind.w) / DETAIL_TILE;
    let n = textureSampleLevel(detail_noise, repeat_linear, q, 0.0);
    let hf = n.r * 0.625 + n.g * 0.25 + n.b * 0.125;
    let billow = mix(1.0 - hf, hf, clamp(h * 8.0, 0.0, 1.0));
    let sharp = cl.shape.z;
    let erosion = mix(0.55, 0.22, sharp);
    var d = clamp(remap(base, billow * erosion, 1.0, 0.0, 1.0), 0.0, 1.0);
    // Sharp clouds reach full density right inside their edge.
    d = clamp(d * mix(1.0, 3.5, sharp), 0.0, 1.0);
    return d;
}

// Distances (near, far) along a ray to the sphere at altitude `h`, or (-1, -1). Stable form of
// the quadratic (no cancellation between the two roots).
fn shell_hits(o: vec3<f32>, d: vec3<f32>, h: f32) -> vec2<f32> {
    let v = local(o);
    let r = cl.layer.x;
    let b = dot(v, d) + r * d.y;
    let c = dot(v, v) - h * h + 2.0 * r * (v.y - h);
    let disc = b * b - c;
    if (disc < 0.0) {
        return vec2<f32>(-1.0);
    }
    let q = -(b + select(-1.0, 1.0, b >= 0.0) * sqrt(disc));
    let t0 = q;
    let t1 = c / select(q, -1e-6, abs(q) < 1e-6);
    return vec2<f32>(min(t0, t1), max(t0, t1));
}

// The part of a ray inside the cloud layer (x = enter, y = leave; y <= x when it misses).
fn layer_span(o: vec3<f32>, d: vec3<f32>) -> vec2<f32> {
    let inner = shell_hits(o, d, cl.layer.y);
    let outer = shell_hits(o, d, cl.layer.z);
    let ground = shell_hits(o, d, 0.0);
    let a = altitude(o);
    var span = vec2<f32>(0.0, -1.0);
    if (a < cl.layer.y) {
        // Below the layer: from the base up to the top (the ground blocks rays heading down).
        if (ground.x > 0.0) {
            return span;
        }
        span = vec2<f32>(inner.y, outer.y);
    } else if (a < cl.layer.z) {
        // Inside: to the top, or down to the base.
        var leave = outer.y;
        if (inner.x > 0.0) {
            leave = inner.x;
        }
        span = vec2<f32>(0.0, leave);
    } else {
        // Above: from the top down to the base (or out through the top again).
        if (outer.x < 0.0) {
            return span;
        }
        var leave = outer.y;
        if (inner.x > 0.0) {
            leave = inner.x;
        }
        span = vec2<f32>(outer.x, leave);
    }
    span.y = min(span.y, span.x + cl.layer.w * 0.5);
    if (span.x > cl.layer.w) {
        span.y = -1.0;
    }
    return span;
}
