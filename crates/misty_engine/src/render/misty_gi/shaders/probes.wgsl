// Probe update (appended to gi_common.wgsl): one workgroup per probe updated this frame (one
// in `stride`), one thread per ray. Rays are a spherical Fibonacci set turned by this frame's
// rotation; a hit reads the radiance cache, a miss the sky. The rays are projected onto SH L1
// and blended into the probe's history (hysteresis); a probe whose world cell changed (the grid
// scrolled) or that is new starts afresh. Probes inside geometry are marked off (state −1).

const MAX_RAYS: u32 = 32u;

var<workgroup> sh_r: array<vec4<f32>, 32>;
var<workgroup> sh_g: array<vec4<f32>, 32>;
var<workgroup> sh_b: array<vec4<f32>, 32>;

fn fibonacci_dir(i: u32, n: u32) -> vec3<f32> {
    let z = 1.0 - (2.0 * f32(i) + 1.0) / f32(n);
    let r = sqrt(max(1.0 - z * z, 0.0));
    let phi = f32(i) * 2.39996323;
    return vec3<f32>(r * cos(phi), r * sin(phi), z);
}

@compute @workgroup_size(32)
fn cs_probes(@builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let n = probes_per_side();
    let per_level = n * n * n;
    let stride = u32(p.info2.y);
    let slot = i32(wid.x * stride + u32(p.info2.z));
    let k = slot / per_level;
    if (k >= level_count()) {
        return;
    }
    let s = slot - k * per_level;
    let sc = vec3<i32>(s % n, (s / n) % n, s / (n * n));
    // The world cell stored in this slot: the grid's first cell plus the toroidal offset.
    let first = vec3<i32>(p.levels[k].probe.xyz);
    let g = first + vec3<i32>(pmod(sc.x - first.x, n), pmod(sc.y - first.y, n), pmod(sc.z - first.z, n));
    let pp = probe_position(k, g);
    let index = probe_index(k, g);
    let rays = min(u32(p.info2.x), MAX_RAYS);
    // Inside geometry: nothing to see, and its light would leak into the surface.
    let inside = sdf_at(pp) < 0.0;

    var c_r = vec4<f32>(0.0);
    var c_g = vec4<f32>(0.0);
    var c_b = vec4<f32>(0.0);
    if (lid < rays && !inside) {
        let base = fibonacci_dir(lid, rays);
        let dir = normalize(vec3<f32>(dot(p.rot0.xyz, base), dot(p.rot1.xyz, base), dot(p.rot2.xyz, base)));
        let h = trace(pp, dir, 1e4, 64, 0.5, voxel(max(level_of(pp, 0), 0)));
        var radiance: vec3<f32>;
        if (h.hit) {
            radiance = cache_radiance(h.level, h.pos, dir);
        } else {
            radiance = sky_radiance(dir);
        }
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
