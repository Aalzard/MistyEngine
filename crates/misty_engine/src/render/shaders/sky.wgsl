// Procedural gradient sky: zenith blue, light horizon, dark ground.

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4<f32> {
    if (view.background.w < 0.5) {
        return vec4<f32>(view.background.rgb, 1.0);
    }
    let dir = normalize(unproject(in.ndc, 1.0) - unproject(in.ndc, 0.0));
    let zenith = srgb_to_linear(vec3<f32>(0.40, 0.55, 0.76));
    let horizon = srgb_to_linear(vec3<f32>(0.76, 0.79, 0.83));
    let ground = srgb_to_linear(vec3<f32>(0.32, 0.30, 0.29));
    var c: vec3<f32>;
    if (dir.y >= 0.0) {
        c = mix(horizon, zenith, pow(dir.y, 0.6));
    } else {
        c = mix(horizon, ground, pow(min(-dir.y * 6.0, 1.0), 0.5));
    }
    return vec4<f32>(c, 1.0);
}
