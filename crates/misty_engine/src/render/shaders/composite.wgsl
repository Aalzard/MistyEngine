// Final image: HDR scene + bloom, vignette, colour adjustments and tonemapping, to 8-bit sRGB.
// Parameters live in the view uniform (render/post.rs); CPU copies of the colour math are in
// post.rs for tests. Defaults: bloom 0.6, no grading, hue-preserving highlight compression.

@group(1) @binding(0) var hdr: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var bloom: texture_2d<f32>;

// Over-bright colours keep their hue and whiten gradually instead of clipping per channel.
fn compress(c: vec3<f32>) -> vec3<f32> {
    let m = max(c.r, max(c.g, c.b));
    if (m <= 1.0) {
        return c;
    }
    let white = clamp((m - 1.0) / (m + 2.0), 0.0, 1.0) * 0.6;
    return mix(c / m, vec3<f32>(1.0), white);
}

// Hable's filmic curve.
fn filmic(x: vec3<f32>) -> vec3<f32> {
    let a = 0.15;
    let b = 0.50;
    let c = 0.10;
    let d = 0.20;
    let e = 0.02;
    let f = 0.30;
    return (x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f) - e / f;
}

// Neutral: filmic curve scaled so mid grey stays about where it is.
fn tonemap_neutral(c: vec3<f32>) -> vec3<f32> {
    return clamp(filmic(max(c, vec3<f32>(0.0)) * 2.9) / filmic(vec3<f32>(11.2)), vec3<f32>(0.0), vec3<f32>(1.0));
}

// ACES: Stephen Hill's fit of the RRT + sRGB ODT.
fn tonemap_aces(c: vec3<f32>) -> vec3<f32> {
    let input = mat3x3<f32>(
        vec3<f32>(0.59719, 0.07600, 0.02840),
        vec3<f32>(0.35458, 0.90834, 0.13383),
        vec3<f32>(0.04823, 0.01566, 0.83777),
    );
    let output = mat3x3<f32>(
        vec3<f32>(1.60475, -0.10208, -0.00327),
        vec3<f32>(-0.53108, 1.10813, -0.07276),
        vec3<f32>(-0.07367, -0.00605, 1.07602),
    );
    let v = input * max(c, vec3<f32>(0.0));
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.432951) + 0.238081;
    return clamp(output * (a / b), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// AgX base contrast: polynomial fit of the sigmoid on the 0..1 log2 encoding.
fn agx_sigmoid(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    return 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x - 0.00232;
}

// AgX: inset (slight desaturation so bright colours go to white), log2 encoding from -12.47 to
// +4.03 EV, sigmoid, optional Punchy look, outset and display decoding (2.2).
fn tonemap_agx(c: vec3<f32>, punchy: bool) -> vec3<f32> {
    let inset = mat3x3<f32>(
        vec3<f32>(0.84247906, 0.04232824, 0.04237565),
        vec3<f32>(0.0784336, 0.8784686, 0.0784336),
        vec3<f32>(0.07922375, 0.07916613, 0.87914297),
    );
    let outset = mat3x3<f32>(
        vec3<f32>(1.196879, -0.05289685, -0.05297164),
        vec3<f32>(-0.09802088, 1.1519031, -0.09804345),
        vec3<f32>(-0.09902974, -0.09896118, 1.1510737),
    );
    let lo = -12.47393;
    let hi = 4.026069;
    var v = inset * max(c, vec3<f32>(0.0));
    v = clamp((clamp(log2(max(v, vec3<f32>(1e-10))), vec3<f32>(lo), vec3<f32>(hi)) - lo) / (hi - lo), vec3<f32>(0.0), vec3<f32>(1.0));
    v = agx_sigmoid(v);
    if (punchy) {
        let y = luminance(v);
        v = pow(max(v, vec3<f32>(0.0)), vec3<f32>(1.35));
        v = vec3<f32>(y) + (v - y) * 1.4;
    }
    v = outset * v;
    return clamp(pow(max(v, vec3<f32>(0.0)), vec3<f32>(2.2)), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Khronos PBR Neutral: small toe offset; above 0.76 highlights roll off and desaturate a little.
fn tonemap_pbr_neutral(color: vec3<f32>) -> vec3<f32> {
    let start = 0.76;
    var c = max(color, vec3<f32>(0.0));
    let x = min(c.r, min(c.g, c.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    c -= offset;
    let peak = max(c.r, max(c.g, c.b));
    if (peak < start) {
        return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    c *= new_peak / peak;
    let g = 1.0 - 1.0 / (0.15 * (peak - new_peak) + 1.0);
    return clamp(mix(c, vec3<f32>(new_peak), g), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Exposure, colour filter / white balance, contrast (log space around mid grey), saturation,
// tonemapping.
fn grade(color: vec3<f32>) -> vec3<f32> {
    var c = color * view.grade.y * view.color_filter.rgb;
    if (view.grade.z != 1.0) {
        let mid = log2(0.18);
        c = exp2((log2(max(c, vec3<f32>(1e-6))) - mid) * view.grade.z + mid);
    }
    if (view.grade.w != 1.0) {
        let y = luminance(c);
        c = max(vec3<f32>(y) + (c - y) * view.grade.w, vec3<f32>(0.0));
    }
    let mode = view.grade.x;
    if (mode > 3.5) {
        return tonemap_pbr_neutral(c);
    }
    if (mode > 2.5) {
        return tonemap_agx(c, view.color_filter.w > 0.5);
    }
    if (mode > 1.5) {
        return tonemap_aces(c);
    }
    if (mode > 0.5) {
        return tonemap_neutral(c);
    }
    return compress(c);
}

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(in.ndc.x * 0.5 + 0.5, 0.5 - in.ndc.y * 0.5);
    let scene = textureSampleLevel(hdr, samp, uv, 0.0).rgb;
    let glow = textureSampleLevel(bloom, samp, uv, 0.0).rgb;
    var c = scene + glow * view.bloom.x * view.bloom_tint.rgb;
    if (view.vignette.x > 0.0) {
        let d = abs(uv - 0.5) * view.vignette.x;
        let v = pow(clamp(1.0 - dot(d, d), 0.0, 1.0), view.vignette.y);
        c *= mix(view.vignette_color.rgb, vec3<f32>(1.0), v);
    }
    return vec4<f32>(grade(c), 1.0);
}
