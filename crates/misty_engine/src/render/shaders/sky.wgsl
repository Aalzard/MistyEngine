// Skybox: procedural sky (see `sky_color` in common.wgsl) with a sun disc, or a solid colour.
// The camera's solid background replaces it when the skybox is off. Fog hazes the horizon.

struct SkyTargets {
    @location(0) color: vec4<f32>,
    // The sun disc glows through bloom.
    @location(1) glow: vec4<f32>,
};

// Sun disc (and halo in High Quality) seen in `dir`; zero when there is no visible sun.
fn sun_disc(dir: vec3<f32>) -> vec3<f32> {
    let mode = view.sun_color.w;
    if (view.sun_dir.w < 0.5 || mode < 0.5 || view.sky_solid.w > 0.5) {
        return vec3<f32>(0.0);
    }
    let cos_a = dot(dir, view.sun_dir.xyz);
    let r = view.sky_misc.x;
    var light = smoothstep(cos(r), cos(r * 0.75), cos_a) * 6.0;
    if (mode > 1.5) {
        let c = max(cos_a, 0.0);
        light += pow(c, 600.0) * 0.6 + pow(c, 24.0) * 0.06;
    }
    // Low suns turn orange; the ground hides them.
    let warm = mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.45, 0.15), sky_sunset());
    return view.sun_color.rgb * warm * light * smoothstep(-0.02, 0.01, dir.y);
}

@fragment
fn fs_main(in: FullscreenOut) -> SkyTargets {
    var out: SkyTargets;
    out.glow = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    if (view.background.w < 0.5) {
        out.color = vec4<f32>(view.background.rgb, 1.0);
        return out;
    }
    let dir = normalize(unproject(in.ndc, 1.0) - unproject(in.ndc, 0.0));
    let sun = sun_disc(dir);
    var c = sky_color(dir) + sun;
    var visible = 1.0;
    if (view.fog_color.w > 0.5) {
        // Fog thickens towards the horizon and fills the view below it.
        let band = pow(1.0 - max(dir.y, 0.0), 8.0);
        let amount = (1.0 - fog_visibility(view.sky_misc.y)) * band;
        c = mix(c, view.fog_color.rgb, amount);
        visible = 1.0 - amount;
    }
    out.color = vec4<f32>(c, 1.0);
    out.glow = vec4<f32>(sun * 0.15 * visible, 1.0);
    return out;
}
