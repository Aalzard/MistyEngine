// Cloud shadow lookup shared by the forward shaders (clouds_apply.wgsl) and the froxel fog.

struct CloudApply {
    // x = 1 when clouds are drawn, y = 1 when they cast shadows, z = shadow opacity,
    // w = 1 / shadow map world size.
    mode: vec4<f32>,
    // xy = shadow map origin (world xz of its corner), z = cloud base altitude,
    // w = height of the sun direction (clamped above the horizon).
    shadow: vec4<f32>,
    // xyz = direction towards the sun.
    sun: vec4<f32>,
};

// Sunlight left after the clouds above `world` (1 = no cloud). The point slides along the sun
// direction onto the cloud base, where the shadow map was traced.
fn cloud_shadow_lookup(map: texture_2d<f32>, s: sampler, c: CloudApply, world: vec3<f32>) -> f32 {
    if (c.mode.y < 0.5) {
        return 1.0;
    }
    let q = world.xz + c.sun.xz / c.shadow.w * (c.shadow.z - world.y);
    let uv = (q - c.shadow.xy) * c.mode.w;
    let edge = min(min(uv.x, uv.y), min(1.0 - uv.x, 1.0 - uv.y));
    if (edge <= 0.0) {
        return 1.0;
    }
    let t = textureSampleLevel(map, s, uv, 0.0).r;
    // Fade out towards the edges of the map instead of a hard border.
    return mix(1.0, t, c.mode.z * smoothstep(0.0, 0.08, edge));
}
