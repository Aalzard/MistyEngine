// Grass study, variant C: a compute "software rasteriser" for blades thinner than
// `study.params.x` pixels (the hardware visibility raster skips them). One thread per drawn
// blade: its centre line (four segments, or the across line of a flat fan) is walked pixel
// by pixel; each pixel keeps the nearest blade with a 64-bit atomic max of
// (!depth << 32 | id << 9 | t). Coverage: the card's average alpha at that height × its width
// in pixels, as a per-pixel per-frame noise threshold (TAA averages it).

@group(2) @binding(0) var depth_tex: texture_depth_2d;
@group(2) @binding(1) var<storage, read_write> swr: array<atomic<u64>>;
// Indices of the blades drawn this frame (the chunks' prefixes).
@group(2) @binding(2) var<storage, read> drawn: array<u32>;

fn to_screen(p: vec3<f32>) -> vec4<f32> {
    let c = study.view_proj * vec4<f32>(p, 1.0);
    let ndc = c.xyz / c.w;
    return vec4<f32>((ndc.x * 0.5 + 0.5) * study.screen.x, (0.5 - ndc.y * 0.5) * study.screen.y, ndc.z, c.w);
}

fn plot(px: vec2<f32>, z: f32, id: u32, t: f32, coverage: f32) {
    if (px.x < 0.0 || px.y < 0.0 || px.x >= study.screen.x || px.y >= study.screen.y || z <= 0.0 || z >= 1.0) {
        return;
    }
    let p = vec2<u32>(px);
    if (coverage <= frame_noise(vec2<f32>(p) * 1.37 + 3.1)) {
        return;
    }
    if (z >= textureLoad(depth_tex, p, 0)) {
        return;
    }
    count_add(12u, 1u);
    let hi = ~bitcast<u32>(z);
    let lo = (id << 9u) | u32(clamp(t, 0.0, 1.0) * 511.0);
    atomicMax(&swr[p.y * u32(study.screen.x) + p.x], (u64(hi) << 32u) | u64(lo));
}

fn segment(a: vec4<f32>, b: vec4<f32>, ta: f32, tb: f32, id: u32, cov_a: f32, cov_b: f32) {
    let d = b.xy - a.xy;
    let n = u32(clamp(ceil(max(abs(d.x), abs(d.y))), 1.0, 48.0));
    for (var i = 0u; i <= n; i++) {
        let f = f32(i) / f32(n);
        plot(floor(mix(a.xy, b.xy, f)), mix(a.z, b.z, f), id, mix(ta, tb, f), mix(cov_a, cov_b, f));
    }
}

// Average alpha of the card's leaves at height t (a small mip of the atlas cell).
fn mean_alpha(cell: u32, t: f32) -> f32 {
    return textureSampleLevel(grass_atlas, grass_sampler, vec2<f32>((f32(cell) + 0.5) / 8.0, 1.0 - t), 7.0).a;
}

@compute @workgroup_size(64)
fn cs_swr(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let k = gid.x + gid.y * groups.x * 64u;
    if (k >= arrayLength(&drawn)) {
        return;
    }
    let i = drawn[k];
    let b = load_blade(i);
    if (grass_outside(b, study.view_proj, true) || study_skip(b)) {
        return;
    }
    let root = grass_blade_ts(0.0, 0.0, b, grass.origin.w);
    let w = blade_pixels(root);
    if (w >= study.params.x || root.dist <= 3.0) {
        return;
    }
    count_add(13u, 1u);
    let id = i;
    let wcov = clamp(w, 0.0, 1.0);
    if (root.height <= 0.0) {
        // A flat fan: its across line on the ground.
        let half_w = root.across * root.width * 0.5;
        let a = to_screen(root.root - half_w);
        let c = to_screen(root.root + half_w);
        let cov = clamp(mean_alpha(root.cell, 0.3) * 1.6, 0.0, 1.0);
        segment(a, c, 0.0, 0.0, id, cov, cov);
        return;
    }
    var prev = to_screen(root.p);
    var prev_t = 0.0;
    var prev_cov = clamp(mean_alpha(root.cell, 0.0) * 1.6, 0.0, 1.0) * wcov;
    for (var s = 1u; s <= 4u; s++) {
        let t = f32(s) / 4.0;
        let v = grass_blade_ts(t, 0.0, b, grass.origin.w);
        let cur = to_screen(v.p);
        let cov = clamp(mean_alpha(root.cell, t) * 1.6, 0.0, 1.0) * wcov;
        segment(prev, cur, prev_t, t, id, prev_cov, cov);
        prev = cur;
        prev_t = t;
        prev_cov = cov;
    }
}
