// Bloom of the emission buffer: 13-tap downsample chain, tent upsample (additive).

@group(1) @binding(0) var src: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

fn uv_of(ndc: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src, samp, uv, 0.0).rgb;
}

// 13-tap filter (box-weighted groups) that avoids flicker when downsampling.
fn downsample13(uv: vec2<f32>) -> vec3<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    let a = tap(uv + t * vec2<f32>(-2.0, -2.0));
    let b = tap(uv + t * vec2<f32>(0.0, -2.0));
    let c = tap(uv + t * vec2<f32>(2.0, -2.0));
    let d = tap(uv + t * vec2<f32>(-1.0, -1.0));
    let e = tap(uv + t * vec2<f32>(1.0, -1.0));
    let f = tap(uv + t * vec2<f32>(-2.0, 0.0));
    let g = tap(uv);
    let h = tap(uv + t * vec2<f32>(2.0, 0.0));
    let i = tap(uv + t * vec2<f32>(-1.0, 1.0));
    let j = tap(uv + t * vec2<f32>(1.0, 1.0));
    let k = tap(uv + t * vec2<f32>(-2.0, 2.0));
    let l = tap(uv + t * vec2<f32>(0.0, 2.0));
    let m = tap(uv + t * vec2<f32>(2.0, 2.0));
    return (d + e + i + j) * 0.125 + (a + b + f + g) * 0.03125 + (b + c + g + h) * 0.03125
        + (f + g + k + l) * 0.03125 + (g + h + l + m) * 0.03125;
}

// The source is the emission buffer, so everything in it glows.
@fragment
fn fs_prefilter(in: FullscreenOut) -> @location(0) vec4<f32> {
    return vec4<f32>(min(downsample13(uv_of(in.ndc)), vec3<f32>(64.0)), 1.0);
}

@fragment
fn fs_downsample(in: FullscreenOut) -> @location(0) vec4<f32> {
    return vec4<f32>(downsample13(uv_of(in.ndc)), 1.0);
}

// 3x3 tent filter from the smaller level, blended additively into the larger one.
@fragment
fn fs_upsample(in: FullscreenOut) -> @location(0) vec4<f32> {
    let uv = uv_of(in.ndc);
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    var c = tap(uv) * 4.0;
    c += (tap(uv + vec2<f32>(t.x, 0.0)) + tap(uv - vec2<f32>(t.x, 0.0))
        + tap(uv + vec2<f32>(0.0, t.y)) + tap(uv - vec2<f32>(0.0, t.y))) * 2.0;
    c += tap(uv + t) + tap(uv - t) + tap(uv + vec2<f32>(t.x, -t.y)) + tap(uv + vec2<f32>(-t.x, t.y));
    return vec4<f32>(c / 16.0, 1.0);
}
