// The sky's glow (bloom feed) when the main pass ran without the glow target
// (render/glowless.rs; appended to sky.wgsl): what the sky's `fs_main` wrote to the glow
// target, times the share of the pixel's samples it shaded (those still at the far plane), as
// the main pass's resolve averaged it. Nothing else writes glow in such a frame (no emission,
// nothing blended over the sky).

// `texture_depth_multisampled_2d`, or `texture_depth_2d` without MSAA (glowless.rs).
@group(1) @binding(0) var glow_depth: DEPTH_TEXTURE;

// `fs_main`'s glow output (same steps, the colour left out where the glow doesn't need it).
fn sky_glow_of(pos: vec4<f32>, ndc: vec2<f32>) -> vec3<f32> {
    let dir = view_ray(ndc);
    let vf = vfog_sky(dir);
    if (view.background.w < 0.5) {
        return vec3<f32>(0.0);
    }
    var sun = sun_disc(dir);
    let night = sky_env.night_zenith.w > 0.0;
    var c = vec3<f32>(0.0);
    if (sky_is_hdri() || night) {
        c = sky_color(dir) + sun;
    }
    if (sky_is_hdri()) {
        sun = max(c - vec3<f32>(8.0), vec3<f32>(0.0));
    }
    let moon = moon_disc(dir) * smoothstep(-0.02, 0.01, dir.y);
    if (night) {
        let sky_lum = dot(c, vec3<f32>(0.3, 0.6, 0.1)) / max(view.sky_ground.w, 1e-3);
        let wash = pow(clamp(0.012 / max(sky_lum, 1e-4), 0.0, 1.0), sky_env.night[2].x);
        let stars = star_field(dir);
        let frame_seed = u32(fract(view.motion.x * 977.0 + view.motion.y * 433.0) * 65535.0);
        let jitter = star_hash(vec3<u32>(vec2<u32>(pos.xy), frame_seed + 77u)).x;
        let glow = aurora(dir, jitter) * mix(1.0, wash, 0.4) + meteors(dir);
        sun += stars.rgb * stars.a * wash + glow * 0.4;
    }
    sun += moon.rgb * 0.3;
    var visible = 1.0;
    if (view.fog_color.w > 0.5) {
        let band = pow(1.0 - max(dir.y, 0.0), 8.0);
        let amount = (1.0 - fog_visibility(view.sky_misc.y)) * band;
        visible = 1.0 - amount;
    }
    return sun * 0.15 * visible * vfog_glow(vf);
}

@fragment
fn fs_sky_glow(in: FullscreenOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    var far = 0u;
    for (var i = 0u; i < GLOW_SAMPLES; i++) {
        if (textureLoad(glow_depth, px, i32(i)) >= 1.0) {
            far += 1u;
        }
    }
    if (far == 0u) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let glow = sky_glow_of(in.pos, in.ndc);
    return vec4<f32>(glow * (f32(far) / f32(GLOW_SAMPLES)), 1.0);
}
