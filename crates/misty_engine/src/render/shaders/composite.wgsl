// Final image: HDR scene + bloom, with hue-preserving highlight compression, to 8-bit sRGB.

@group(1) @binding(0) var hdr: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var bloom: texture_2d<f32>;

const BLOOM_INTENSITY: f32 = 0.6;

// Over-bright colours keep their hue and whiten gradually instead of clipping per channel.
fn compress(c: vec3<f32>) -> vec3<f32> {
    let m = max(c.r, max(c.g, c.b));
    if (m <= 1.0) {
        return c;
    }
    let white = clamp((m - 1.0) / (m + 2.0), 0.0, 1.0) * 0.6;
    return mix(c / m, vec3<f32>(1.0), white);
}

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(in.ndc.x * 0.5 + 0.5, 0.5 - in.ndc.y * 0.5);
    let scene = textureSampleLevel(hdr, samp, uv, 0.0).rgb;
    let glow = textureSampleLevel(bloom, samp, uv, 0.0).rgb;
    return vec4<f32>(compress(scene + glow * BLOOM_INTENSITY), 1.0);
}
