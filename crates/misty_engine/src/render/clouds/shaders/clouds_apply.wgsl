// Volumetric clouds in the forward shaders (included after common.wgsl, before fog_apply.wgsl,
// which folds them into `vfog_eval` / `vfog_sky`). The cloud image is traced without the scene
// depth at reduced resolution, so it upsamples cleanly; each fragment then hides the clouds
// that lie behind it by comparing its own distance with the clouds' distance.

@group(0) @binding(8) var cloud_image: texture_2d_array<f32>;
@group(0) @binding(9) var cloud_shadow_map: texture_2d<f32>;
@group(0) @binding(10) var<uniform> clouds: CloudApply;

// Sunlight left after the clouds above `world` (1 without clouds or cloud shadows).
fn cloud_shadow(world: vec3<f32>) -> f32 {
    return cloud_shadow_lookup(cloud_shadow_map, vfog_sampler, clouds, world);
}

// rgb = cloud light in front of a fragment at `world` (`dist` from the eye), a = transmittance.
fn clouds_at(world: vec3<f32>, dist: f32) -> vec4<f32> {
    if (clouds.mode.x < 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let clip = view.view_proj * vec4<f32>(world, 1.0);
    if (clip.w <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // The image was traced without the TAA jitter: sample it where the fragment is without
    // it, so the clouds hold still under the jitter instead of swinging with it.
    let ndc = clip.xy / clip.w - view.motion.xy;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let c = textureSampleLevel(cloud_image, vfog_sampler, uv, 0, 0.0);
    let d = textureSampleLevel(cloud_image, vfog_sampler, uv, 1, 0.0).r * 1000.0;
    // Geometry in front of (or inside) the clouds hides them softly.
    let k = clamp((dist - d * 0.7) / (d * 0.3 + 1.0), 0.0, 1.0);
    return vec4<f32>(c.rgb * k, mix(1.0, c.a, k));
}

// Fog `f` (between the eye and the fragment) over clouds `c` behind it.
fn with_clouds(f: vec4<f32>, c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * f.a + f.rgb, c.a * f.a);
}
