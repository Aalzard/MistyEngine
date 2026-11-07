// Misty GI shared part (render/misty_gi): parameters, bindings, clipmap addressing, distance
// field sampling, sphere tracing, the radiance cache lookup, the sky model and probe
// interpolation. Prepended to sdf.wgsl, surface.wgsl, probes.wgsl and apply.wgsl; each entry
// point's pipeline layout is derived from the bindings it uses.
//
// Units: radiance as the lit shaders use it (a light adds albedo × colour × N·L, the ambient is
// mean incoming radiance × albedo), so irradiance here is the cosine-weighted mean of incoming
// radiance (irradiance / π).

struct Level {
    // xyz = min corner of the SDF box (m), w = voxel size (m).
    sdf: vec4<f32>,
    // xyz = first probe cell (world cell coordinates), w = probe spacing (m).
    probe: vec4<f32>,
    // x = 1 when recomposed this frame, y = first index in `lists`, z = index count.
    flags: vec4<f32>,
};

struct GiParams {
    // x = levels, y = voxels per side, z = probes per side, w = frame.
    info: vec4<f32>,
    // x = rays per probe, y = update stride, z = phase (frame % stride), w = hysteresis.
    info2: vec4<f32>,
    // x = lights, y = visibility steps per pixel, z = full pixels per gather texel, w = 1 on the
    // first frame (probes take their first result whole).
    info3: vec4<f32>,
    // xyz = camera position, w = 1 when glossy reflections are traced.
    camera: vec4<f32>,
    // Sky radiance (linear, × the environment intensity) above, at and below the horizon.
    sky: vec4<f32>,
    equator: vec4<f32>,
    ground: vec4<f32>,
    // Rotation of this frame's probe rays (rows).
    rot0: vec4<f32>,
    rot1: vec4<f32>,
    rot2: vec4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xy = full resolution, zw = gather resolution.
    size: vec4<f32>,
    levels: array<Level, 6>,
};

struct Instance {
    inv: mat4x4<f32>,
    extent: vec4<f32>,
    shape: vec4<f32>,
    bounds_min: vec4<f32>,
    bounds_max: vec4<f32>,
    albedo: vec4<f32>,
    emission: vec4<f32>,
    aabb_min: vec4<f32>,
    aabb_max: vec4<f32>,
};

struct GiLight {
    pos_range: vec4<f32>,
    dir_type: vec4<f32>,
    color: vec4<f32>,
    spot: vec4<f32>,
    area_x: vec4<f32>,
    area_y: vec4<f32>,
};

@group(0) @binding(0) var<uniform> p: GiParams;
// x = distance (m), yzw = albedo of the closest shape. Levels stacked along z.
@group(0) @binding(1) var sdf_tex: texture_3d<f32>;
@group(0) @binding(2) var sdf_out: texture_storage_3d<rgba16float, write>;
@group(0) @binding(3) var emission_tex: texture_3d<f32>;
@group(0) @binding(4) var emission_out: texture_storage_3d<rgba16float, write>;
// rgb = outgoing diffuse radiance, a = 1 where a surface is near (the radiance band).
@group(0) @binding(5) var radiance_tex: texture_3d<f32>;
@group(0) @binding(6) var radiance_out: texture_storage_3d<rgba16float, write>;
@group(0) @binding(7) var cache_normal_tex: texture_3d<f32>;
@group(0) @binding(8) var cache_normal_out: texture_storage_3d<rgba8snorm, write>;
@group(0) @binding(9) var linear_sampler: sampler;
@group(0) @binding(10) var<storage, read> instances: array<Instance>;
@group(0) @binding(11) var<storage, read> lists: array<u32>;
@group(0) @binding(12) var<storage, read> gi_lights: array<GiLight>;
// Per probe: SH L1 of radiance for r, g, b (c0, c1 · xyz), then (cell xyz, state).
@group(0) @binding(13) var<storage, read_write> probes: array<vec4<f32>>;
@group(0) @binding(14) var mesh_atlas: texture_3d<f32>;
@group(0) @binding(15) var terrain_heights: texture_2d<f32>;
@group(0) @binding(16) var depth_tex: texture_depth_2d;
@group(0) @binding(17) var scene_normal_tex: texture_2d<f32>;
@group(0) @binding(18) var gather_out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(19) var reflect_out: texture_storage_2d<rgba16float, write>;

const PI: f32 = 3.14159265;
const SH_Y0: f32 = 0.282095;
const SH_Y1: f32 = 0.488603;
const MESH_BRICK: f32 = 32.0;

fn level_count() -> i32 {
    return i32(p.info.x);
}

fn dim() -> f32 {
    return p.info.y;
}

fn voxel(k: i32) -> f32 {
    return p.levels[k].sdf.w;
}

// `pos` lies inside level k's SDF box, `margin` voxels in from its faces.
fn in_level(k: i32, pos: vec3<f32>, margin: f32) -> bool {
    let l = (pos - p.levels[k].sdf.xyz) / voxel(k);
    return all(l >= vec3<f32>(margin)) && all(l <= vec3<f32>(dim() - margin));
}

// Finest level from `start` up whose box holds `pos` (-1 = outside every level).
fn level_of(pos: vec3<f32>, start: i32) -> i32 {
    for (var k = max(start, 0); k < level_count(); k++) {
        if (in_level(k, pos, 1.0)) {
            return k;
        }
    }
    return -1;
}

// Filtered sample of level k at `pos`: x = distance, yzw = albedo.
fn sdf_sample(k: i32, pos: vec3<f32>) -> vec4<f32> {
    let d = dim();
    let l = clamp((pos - p.levels[k].sdf.xyz) / voxel(k), vec3<f32>(0.5), vec3<f32>(d - 0.5));
    let uvw = vec3<f32>(l.x / d, l.y / d, (l.z + f32(k) * d) / (d * f32(level_count())));
    return textureSampleLevel(sdf_tex, linear_sampler, uvw, 0.0);
}

// Distance at `pos` from the finest level that holds it (a large value outside them all).
fn sdf_at(pos: vec3<f32>) -> f32 {
    let k = level_of(pos, 0);
    if (k < 0) {
        return 1e4;
    }
    return sdf_sample(k, pos).x;
}

struct Hit {
    hit: bool,
    t: f32,
    level: i32,
    pos: vec3<f32>,
};

// Sphere trace from `o` along `d` up to `max_t`. Surfaces are thickened by up to `eps` voxels
// of the level at each step so thin walls stay closed, but only while the ray closes in on
// them: a ray running alongside a wall (distance steady) doesn't hit it, or coarse levels would
// show a probe the wall in far more than half its directions. Hits closer than `t_min` count
// only deep inside a surface (a probe next to a wall doesn't see it everywhere).
fn trace(o: vec3<f32>, d: vec3<f32>, max_t: f32, steps: i32, eps: f32, t_min: f32) -> Hit {
    var h: Hit;
    h.hit = false;
    var t = 0.0;
    var last = 1e9;
    for (var i = 0; i < steps; i++) {
        let pos = o + d * t;
        let k = level_of(pos, 0);
        if (k < 0) {
            break;
        }
        let v = voxel(k);
        let dist = sdf_sample(k, pos).x;
        let closing = dist < last - 0.05 * v || dist < 0.2 * v;
        if (dist < v * eps && closing && (t >= t_min || dist < -0.5 * v)) {
            h.hit = true;
            h.t = t;
            h.level = k;
            h.pos = pos;
            return h;
        }
        last = dist;
        t += max(dist, v * 0.35);
        if (t > max_t) {
            break;
        }
    }
    h.t = t;
    return h;
}

// Soft shadow towards `l` from `o` (already off the surface): 1 = lit.
fn soft_shadow(o: vec3<f32>, l: vec3<f32>, max_t: f32) -> f32 {
    var lit = 1.0;
    var t = 0.0;
    for (var i = 0; i < 40; i++) {
        let pos = o + l * t;
        let k = level_of(pos, 0);
        if (k < 0) {
            break;
        }
        let v = voxel(k);
        let dist = sdf_sample(k, pos).x;
        if (dist < v * 0.3) {
            return 0.0;
        }
        lit = min(lit, 8.0 * dist / max(t, v));
        t += max(dist, v * 0.5);
        if (t > max_t) {
            break;
        }
    }
    return clamp(lit, 0.0, 1.0);
}

// Radiance leaving the surface at `pos` towards the ray that came along `dir`: the band
// voxels around it, weighted by how much their surface faces the ray (a ray hitting the inside
// of a thin wall doesn't average in its dark outside).
fn cache_radiance(k: i32, pos: vec3<f32>, dir: vec3<f32>) -> vec3<f32> {
    let d = dim();
    let l = (pos - p.levels[k].sdf.xyz) / voxel(k) - 0.5;
    let base = vec3<i32>(floor(l));
    let f = l - floor(l);
    var sum = vec3<f32>(0.0);
    var wsum = 0.0;
    let last = i32(d) - 1;
    for (var c = 0; c < 8; c++) {
        let o = vec3<i32>(c & 1, (c >> 1u) & 1, (c >> 2u) & 1);
        let q = clamp(base + o, vec3<i32>(0), vec3<i32>(last));
        let texel = vec3<i32>(q.x, q.y, q.z + k * i32(d));
        let r = textureLoad(radiance_tex, texel, 0);
        if (r.a < 0.5) {
            continue;
        }
        let n = textureLoad(cache_normal_tex, texel, 0).xyz;
        let fo = vec3<f32>(o);
        let tri = (fo.x * f.x + (1.0 - fo.x) * (1.0 - f.x)) * (fo.y * f.y + (1.0 - fo.y) * (1.0 - f.y)) * (fo.z * f.z + (1.0 - fo.z) * (1.0 - f.z));
        let w = (tri + 1e-3) * clamp(dot(n, -dir) + 0.25, 0.0, 1.0);
        sum += r.rgb * w;
        wsum += w;
    }
    if (wsum < 1e-4) {
        return vec3<f32>(0.0);
    }
    return sum / wsum;
}

// Sky radiance along `d`: the environment's sky / equator / ground gradient, steepened so that
// an open surface gets about the ambient the lit shaders give it.
fn sky_radiance(d: vec3<f32>) -> vec3<f32> {
    let t = clamp(abs(d.y) * 1.5, 0.0, 1.0);
    if (d.y >= 0.0) {
        return mix(p.equator.rgb, p.sky.rgb, t);
    }
    return mix(p.equator.rgb, p.ground.rgb, t);
}

// ---- probes ---------------------------------------------------------------------------------

fn probes_per_side() -> i32 {
    return i32(p.info.z);
}

fn pmod(a: i32, n: i32) -> i32 {
    return ((a % n) + n) % n;
}

// Storage index (in probes, ×4 for vec4s) of world cell `g` of level k (toroidal).
fn probe_index(k: i32, g: vec3<i32>) -> i32 {
    let n = probes_per_side();
    return ((k * n + pmod(g.z, n)) * n + pmod(g.y, n)) * n + pmod(g.x, n);
}

fn probe_position(k: i32, g: vec3<i32>) -> vec3<f32> {
    return (vec3<f32>(g) + 0.5) * p.levels[k].probe.w;
}

// Mean incoming radiance over the hemisphere around `n` (clamped-cosine convolution of the SH,
// with a little windowing against ringing).
fn probe_irradiance(i: i32, n: vec3<f32>) -> vec3<f32> {
    let r = probes[i * 4];
    let g = probes[i * 4 + 1];
    let b = probes[i * 4 + 2];
    let k1 = SH_Y1 * (2.0 / 3.0) * 0.9;
    let e = vec3<f32>(
        r.x * SH_Y0 + k1 * dot(r.yzw, n),
        g.x * SH_Y0 + k1 * dot(g.yzw, n),
        b.x * SH_Y0 + k1 * dot(b.yzw, n),
    );
    return max(e, vec3<f32>(0.0));
}

// 1 when nothing stands between `a` and `b` in the SDF, falling towards 0 as the segment grazes
// or crosses a surface.
fn segment_visibility(a: vec3<f32>, b: vec3<f32>, steps: i32) -> f32 {
    let delta = b - a;
    let len = length(delta);
    if (len < 1e-4 || steps <= 0) {
        return 1.0;
    }
    let dir = delta / len;
    var vis = 1.0;
    var t = 0.0;
    for (var i = 0; i < steps; i++) {
        let pos = a + dir * t;
        let k = level_of(pos, 0);
        if (k < 0) {
            return vis;
        }
        let v = voxel(k);
        let dist = sdf_sample(k, pos).x;
        vis = min(vis, clamp(dist / (0.5 * v), 0.0, 1.0));
        if (vis <= 0.0) {
            return 0.0;
        }
        t += max(dist, 0.5 * v);
        if (t >= len) {
            break;
        }
    }
    return vis * vis;
}

// Finest probe level whose grid has all eight neighbours of `pos` (-1 = none).
fn probe_level_of(pos: vec3<f32>) -> i32 {
    let n = f32(probes_per_side());
    for (var k = 0; k < level_count(); k++) {
        let q = pos / p.levels[k].probe.w - 0.5 - p.levels[k].probe.xyz;
        if (all(q >= vec3<f32>(0.0)) && all(q <= vec3<f32>(n - 1.001))) {
            return k;
        }
    }
    return -1;
}

// How far `pos` is inside level k's probe grid, in cells (blend zone near its faces).
fn probe_border(k: i32, pos: vec3<f32>) -> f32 {
    let n = f32(probes_per_side());
    let q = pos / p.levels[k].probe.w - 0.5 - p.levels[k].probe.xyz;
    let m = min(q, vec3<f32>(n - 1.0) - q);
    return min(m.x, min(m.y, m.z));
}

// Irradiance of level k's probes at `pos` with normal `n`: trilinear weights × back-face
// weight × SDF visibility from the surface to each probe. w = summed weight.
fn probe_gather(k: i32, pos: vec3<f32>, n: vec3<f32>, steps: i32) -> vec4<f32> {
    let spacing = p.levels[k].probe.w;
    let lk = max(level_of(pos, 0), 0);
    let bias = voxel(lk);
    let start = pos + n * bias;
    let q = start / spacing - 0.5;
    let base = vec3<i32>(floor(q));
    let f = q - floor(q);
    var sum = vec3<f32>(0.0);
    var wsum = 0.0;
    var fallback = vec3<f32>(0.0);
    var fsum = 0.0;
    for (var c = 0; c < 8; c++) {
        let o = vec3<i32>(c & 1, (c >> 1u) & 1, (c >> 2u) & 1);
        let g = base + o;
        let i = probe_index(k, g);
        let pstate = probes[i * 4 + 3];
        if (pstate.w <= 0.0 || any(vec3<i32>(pstate.xyz) != g)) {
            continue;
        }
        let fo = vec3<f32>(o);
        let tri = (fo.x * f.x + (1.0 - fo.x) * (1.0 - f.x)) * (fo.y * f.y + (1.0 - fo.y) * (1.0 - f.y)) * (fo.z * f.z + (1.0 - fo.z) * (1.0 - f.z));
        let pp = probe_position(k, g);
        // A gather point can sit exactly on a probe (the grids align): no direction then.
        let rel = pp - pos;
        let to_probe = rel / max(length(rel), 1e-5);
        let back = (dot(to_probe, n) + 1.0) * 0.5;
        let wb = back * back + 0.05;
        let e = probe_irradiance(i, n);
        fallback += e * (tri + 1e-3);
        fsum += tri + 1e-3;
        let vis = segment_visibility(start, pp, steps);
        let w = (tri + 1e-3) * wb * (vis + 1e-3);
        sum += e * w;
        wsum += w;
    }
    if (wsum < 1e-3 && fsum > 0.0) {
        // Every probe is hidden (a crevice smaller than the grid): their plain average.
        return vec4<f32>(fallback / fsum, 1e-3);
    }
    if (wsum <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(sum / wsum, wsum);
}

// Irradiance at a surface point: the finest probe level that covers it, blended into the next
// one near its border; the sky beyond the last level.
fn gi_irradiance(pos: vec3<f32>, n: vec3<f32>, steps: i32) -> vec3<f32> {
    let k = probe_level_of(pos);
    if (k < 0) {
        return sky_radiance(normalize(n + vec3<f32>(0.0, 0.0001, 0.0)));
    }
    let a = probe_gather(k, pos, n, steps);
    let border = probe_border(k, pos);
    if (border < 2.0 && k + 1 < level_count()) {
        let b = probe_gather(k + 1, pos, n, steps);
        if (b.w > 0.0) {
            return mix(b.rgb, a.rgb, clamp(border * 0.5, 0.0, 1.0));
        }
    }
    return a.rgb;
}
