// Probe update (appended to gi_common.wgsl): one workgroup per probe updated this frame (one
// in `stride`), one thread per ray. Rays are a spherical Fibonacci set turned by this frame's
// rotation; a hit reads the radiance cache, a miss the sky. The rays are projected onto SH L1
// and blended into the probe's history (hysteresis); a probe whose world cell changed (the grid
// scrolled) or that is new starts afresh. Probes inside geometry are marked off (state −1).
//
// Settled levels (nothing they see changed for a while) trace their converged probes only one
// turn in SETTLED_RATE; with every level settled the dispatch holds only those probes.
//
// Interval merge (the radiance-cascades idea on the probe levels): a probe's rays trace only
// half its level's width; beyond that they take the coarser levels' radiance in the same
// direction at the interval's end. Long rays are traced once by sparse coarse probes and shared
// by every finer probe, instead of marching through every level from each one.

const MAX_RAYS: u32 = 32u;
// A settled level (flags.w = 1: nothing it sees changed for a while, misty_gi/mod.rs
// `SETTLE_FRAMES`) traces a probe that has had this many updates in its cell only one turn in
// `SETTLED_RATE`; the others keep their history as it is.
const SETTLED_UPDATES: f32 = 32.0;
const SETTLED_RATE: u32 = 8u;

var<workgroup> wg_skip: u32;

var<workgroup> sh_r: array<vec4<f32>, 32>;
var<workgroup> sh_g: array<vec4<f32>, 32>;
var<workgroup> sh_b: array<vec4<f32>, 32>;

fn fibonacci_dir(i: u32, n: u32) -> vec3<f32> {
    let z = 1.0 - (2.0 * f32(i) + 1.0) / f32(n);
    let r = sqrt(max(1.0 - z * z, 0.0));
    let phi = f32(i) * 2.39996323;
    return vec3<f32>(r * cos(phi), r * sin(phi), z);
}

// Radiance along `dir` at `q` from the probes of the first level from `start` up whose grid
// holds `q` (trilinear SH L1, evaluated in the direction). w = 0 when no level holds `q`.
fn far_field(start: i32, q: vec3<f32>, dir: vec3<f32>) -> vec4<f32> {
    let n = f32(probes_per_side());
    for (var k = start; k < level_count(); k++) {
        let spacing = p.levels[k].probe.w;
        let qq = q / spacing - 0.5;
        let rel = qq - p.levels[k].probe.xyz;
        if (any(rel < vec3<f32>(0.0)) || any(rel > vec3<f32>(n - 1.001))) {
            continue;
        }
        let base = vec3<i32>(floor(qq));
        let f = qq - floor(qq);
        var sum = vec3<f32>(0.0);
        var wsum = 0.0;
        for (var c = 0; c < 8; c++) {
            let o = vec3<i32>(c & 1, (c >> 1u) & 1, (c >> 2u) & 1);
            let g = base + o;
            let i = probe_index(k, g);
            let st = probes[i * 4 + 3];
            if (st.w <= 0.0 || any(vec3<i32>(st.xyz) != g)) {
                continue;
            }
            let fo = vec3<f32>(o);
            let tri = (fo.x * f.x + (1.0 - fo.x) * (1.0 - f.x)) * (fo.y * f.y + (1.0 - fo.y) * (1.0 - f.y)) * (fo.z * f.z + (1.0 - fo.z) * (1.0 - f.z));
            let r = probes[i * 4];
            let gg = probes[i * 4 + 1];
            let b = probes[i * 4 + 2];
            let l = vec3<f32>(r.x * SH_Y0 + SH_Y1 * dot(r.yzw, dir), gg.x * SH_Y0 + SH_Y1 * dot(gg.yzw, dir), b.x * SH_Y0 + SH_Y1 * dot(b.yzw, dir));
            sum += max(l, vec3<f32>(0.0)) * (tri + 1e-3);
            wsum += tri + 1e-3;
        }
        if (wsum > 1e-3) {
            return vec4<f32>(sum / wsum, 1.0);
        }
        return vec4<f32>(0.0);
    }
    return vec4<f32>(0.0);
}

// Radiance a ray of a level-k probe brings back (see the interval merge above).
fn probe_ray(pp: vec3<f32>, dir: vec3<f32>, k: i32, t_min: f32) -> vec3<f32> {
    let last = level_count() - 1;
    var reach = 1e4;
    if (k < last) {
        reach = 0.5 * dim() * voxel(k);
    }
    let h = trace(pp, dir, reach, 64, 0.5, t_min);
    if (h.hit) {
        return cache_radiance(h.level, h.pos, dir);
    }
    if (k < last && h.t >= reach) {
        let far = far_field(k + 1, pp + dir * reach, dir);
        if (far.w > 0.0) {
            return far.rgb;
        }
    }
    return sky_radiance(dir);
}

@compute @workgroup_size(32)
fn cs_probes(@builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let n = probes_per_side();
    let per_level = n * n * n;
    let stride = u32(p.info2.y);
    // Every level settled (flags.w = 2): the dispatch holds only the probes whose refresh turn
    // this is, one in SETTLED_RATE (the others keep their history).
    let reduced = p.levels[0].flags.w > 1.5;
    var group = wid.x;
    if (reduced) {
        let turn = u32(p.info.w) / stride;
        group = wid.x * SETTLED_RATE + (SETTLED_RATE - turn % SETTLED_RATE) % SETTLED_RATE;
    }
    let slot = i32(group * stride + u32(p.info2.z));
    let k = slot / per_level;
    if (k >= level_count()) {
        return;
    }
    // The two finest levels (what the camera is close to) follow changes fastest; coarser
    // levels update at half that rate.
    if (!reduced && k >= 2 && ((u32(p.info.w) / stride) & 1u) == 1u) {
        return;
    }
    let s = slot - k * per_level;
    let sc = vec3<i32>(s % n, (s / n) % n, s / (n * n));
    // The world cell stored in this slot: the grid's first cell plus the toroidal offset.
    let first = vec3<i32>(p.levels[k].probe.xyz);
    let g = first + vec3<i32>(pmod(sc.x - first.x, n), pmod(sc.y - first.y, n), pmod(sc.z - first.z, n));
    let pp = probe_position(k, g);
    let index = probe_index(k, g);
    // Settled level: most turns keep the converged history (one turn in SETTLED_RATE traces).
    if (lid == 0u) {
        var skip = 0u;
        if (p.levels[k].flags.w > 0.5 && !reduced) {
            let st = probes[index * 4 + 3];
            var turn = u32(p.info.w) / stride;
            if (k >= 2) {
                turn = turn / 2u;
            }
            let settled = all(vec3<i32>(st.xyz) == g) && st.w >= SETTLED_UPDATES;
            if (settled && (turn + wid.x) % SETTLED_RATE != 0u) {
                skip = 1u;
            }
        }
        wg_skip = skip;
    }
    if (workgroupUniformLoad(&wg_skip) == 1u) {
        return;
    }
    let rays = min(u32(p.info2.x), MAX_RAYS);
    // Inside geometry: nothing to see, and its light would leak into the surface.
    let inside = sdf_at(pp) < 0.0;

    var c_r = vec4<f32>(0.0);
    var c_g = vec4<f32>(0.0);
    var c_b = vec4<f32>(0.0);
    if (lid < rays && !inside) {
        let base = fibonacci_dir(lid, rays);
        let dir = normalize(vec3<f32>(dot(p.rot0.xyz, base), dot(p.rot1.xyz, base), dot(p.rot2.xyz, base)));
        let radiance = probe_ray(pp, dir, k, voxel(max(level_of(pp, 0), 0)));
        let y = vec4<f32>(SH_Y0, SH_Y1 * dir);
        c_r = radiance.r * y;
        c_g = radiance.g * y;
        c_b = radiance.b * y;
    }
    sh_r[lid] = c_r;
    sh_g[lid] = c_g;
    sh_b[lid] = c_b;
    workgroupBarrier();
    if (lid != 0u) {
        return;
    }
    var r = vec4<f32>(0.0);
    var gg = vec4<f32>(0.0);
    var b = vec4<f32>(0.0);
    for (var i = 0u; i < rays; i++) {
        r += sh_r[i];
        gg += sh_g[i];
        b += sh_b[i];
    }
    let norm = 4.0 * PI / f32(rays);
    r *= norm;
    gg *= norm;
    b *= norm;
    let pstate = probes[index * 4 + 3];
    if (inside) {
        probes[index * 4 + 3] = vec4<f32>(vec3<f32>(g), -1.0);
        return;
    }
    let same = all(vec3<i32>(pstate.xyz) == g) && pstate.w > 0.0 && p.info3.w < 0.5;
    // New probes converge fast: 1/n blend until the hysteresis takes over.
    let updates = select(0.0, pstate.w, same);
    var alpha = max(1.0 / (updates + 1.0), 1.0 - p.info2.w);
    // A big change (a light switched or moved) blends in faster than the steady hysteresis.
    let before = probes[index * 4].x + probes[index * 4 + 1].x + probes[index * 4 + 2].x;
    let now = r.x + gg.x + b.x;
    if (abs(now - before) > 0.25 * max(before, 0.05)) {
        alpha = max(alpha, 0.5);
    }
    probes[index * 4] = mix(probes[index * 4], r, select(1.0, alpha, same));
    probes[index * 4 + 1] = mix(probes[index * 4 + 1], gg, select(1.0, alpha, same));
    probes[index * 4 + 2] = mix(probes[index * 4 + 2], b, select(1.0, alpha, same));
    probes[index * 4 + 3] = vec4<f32>(vec3<f32>(g), min(updates + 1.0, 255.0));
}
