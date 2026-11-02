// Procedural gradient sky (see `sky_gradient` in common.wgsl).

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4<f32> {
    if (view.background.w < 0.5) {
        return vec4<f32>(view.background.rgb, 1.0);
    }
    let dir = normalize(unproject(in.ndc, 1.0) - unproject(in.ndc, 0.0));
    return vec4<f32>(sky_gradient(dir), 1.0);
}
