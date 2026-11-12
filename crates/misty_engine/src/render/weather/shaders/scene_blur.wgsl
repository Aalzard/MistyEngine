// One level of the rain's blurred scene chain (render/weather/scene_blur.rs): the level above
// (or the opaque scene copy) at half size through a 4-tap tent, so the chain is smooth
// enough to stand in for a wide blur. Very bright pixels (lamps, the sun's glints) are
// softly limited so a single texel can't flicker through the drops.

@group(1) @binding(0) var blur_src: texture_2d<f32>;
@group(1) @binding(1) var blur_sampler: sampler;

@fragment
fn fs_blur(in: FullscreenOut) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(in.ndc.x * 0.5 + 0.5, 0.5 - in.ndc.y * 0.5);
    let texel = 1.0 / vec2<f32>(textureDimensions(blur_src));
    var c = vec3<f32>(0.0);
    c += textureSampleLevel(blur_src, blur_sampler, uv + texel * vec2<f32>(-1.0, -1.0), 0.0).rgb;
    c += textureSampleLevel(blur_src, blur_sampler, uv + texel * vec2<f32>(1.0, -1.0), 0.0).rgb;
    c += textureSampleLevel(blur_src, blur_sampler, uv + texel * vec2<f32>(-1.0, 1.0), 0.0).rgb;
    c += textureSampleLevel(blur_src, blur_sampler, uv + texel * vec2<f32>(1.0, 1.0), 0.0).rgb;
    c *= 0.25;
    // NaNs and infinities never spread through the chain.
    c = clamp(c, vec3<f32>(0.0), vec3<f32>(64.0));
    return vec4<f32>(c, 1.0);
}
