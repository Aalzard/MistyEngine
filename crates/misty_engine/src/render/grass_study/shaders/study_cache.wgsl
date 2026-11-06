// Grass study, variant G: a world-space cache of the sun's visibility over the turf around
// the camera (two clip levels, three canopy height bands each). Each frame updates one slice
// of rows of each level (`study.frame.z` slices per full refresh); the blades then fetch it
// instead of tapping the cascades, the near grass map and the cloud shadow per fragment.

@group(2) @binding(0) var cache_out: texture_storage_2d_array<rgba8unorm, write>;
// Ground height under each texel of both levels (the engine would read its heightmap).
@group(2) @binding(1) var<storage, read> cache_ground: array<f32>;

@compute @workgroup_size(8, 8)
fn cs_cache(@builtin(global_invocation_id) gid: vec3<u32>) {
    let level = gid.z;
    let c = select(study.cache1, study.cache0, level == 0u);
    let n = u32(c.w);
    let slices = max(study.frame.z, 1u);
    let rows = n / slices;
    let x = gid.x;
    let y = gid.y + (study.frame.x % slices) * rows;
    if (x >= n || gid.y >= rows) {
        return;
    }
    let xz = c.xy + (vec2<f32>(f32(x), f32(y)) + 0.5) * c.z;
    let ground = cache_ground[level * n * n + y * n + x];
    let local = xz - grass.origin.xz;
    let relief = grass_canopy(local);
    let top = (grass.canopy.x + (grass.canopy.y - grass.canopy.x) * relief.x) * 0.95;
    let l = -study.sun_dir.xyz;
    let up = vec3<f32>(0.0, 1.0, 0.0);
    var v = vec3<f32>(0.0);
    let heights = vec3<f32>(0.08, 0.5, 0.95);
    for (var i = 0; i < 3; i++) {
        let p = vec3<f32>(xz.x, ground + top * heights[i], xz.y);
        v[i] = sun_visibility(p, up, l);
    }
    textureStore(cache_out, vec2<u32>(x, y), i32(level), vec4<f32>(v, 1.0));
}
