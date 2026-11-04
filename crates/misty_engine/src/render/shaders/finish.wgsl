// Final-image pass after the composite (render/finish.rs): FXAA and/or contrast-adaptive
// sharpening. Reads the composite's 8-bit image and writes the one that is displayed.

@group(1) @binding(0) var src: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

// 0 = soft, 1 = strongest.
const SHARPNESS: f32 = 0.6;

fn luma(c: vec3<f32>) -> f32 {
    return sqrt(dot(c, vec3<f32>(0.299, 0.587, 0.114)));
}

fn texel(p: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(textureDimensions(src)) - vec2<i32>(1);
    return textureLoad(src, clamp(p, vec2<i32>(0), last), 0).rgb;
}

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src, samp, uv, 0.0).rgb;
}

// FXAA (the compact "lite" variant): find the local edge from the diagonal neighbours and blend
// along it; pixels without enough contrast are left alone.
fn fxaa(p: vec2<i32>) -> vec3<f32> {
    let inv = 1.0 / vec2<f32>(textureDimensions(src));
    let uv = (vec2<f32>(p) + 0.5) * inv;
    let m = texel(p);
    let nw = luma(texel(p + vec2<i32>(-1, -1)));
    let ne = luma(texel(p + vec2<i32>(1, -1)));
    let sw = luma(texel(p + vec2<i32>(-1, 1)));
    let se = luma(texel(p + vec2<i32>(1, 1)));
    let lm = luma(m);
    let lo = min(lm, min(min(nw, ne), min(sw, se)));
    let hi = max(lm, max(max(nw, ne), max(sw, se)));
    if (hi - lo < max(0.0312, hi * 0.125)) {
        return m;
    }
    var dir = vec2<f32>(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
    let reduce = max((nw + ne + sw + se) * (0.25 / 8.0), 1.0 / 128.0);
    let scale = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * scale, vec2<f32>(-8.0), vec2<f32>(8.0)) * inv;
    let a = 0.5 * (tap(uv + dir * (1.0 / 3.0 - 0.5)) + tap(uv + dir * (2.0 / 3.0 - 0.5)));
    let b = a * 0.5 + 0.25 * (tap(uv - dir * 0.5) + tap(uv + dir * 0.5));
    let lb = luma(b);
    if (lb < lo || lb > hi) {
        return a;
    }
    return b;
}

// Contrast-adaptive sharpening: a negative-lobe cross filter whose strength falls where the
// neighbourhood is already contrasty, so edges sharpen without ringing.
fn sharpen(p: vec2<i32>, c: vec3<f32>) -> vec3<f32> {
    let n = texel(p + vec2<i32>(0, -1));
    let w = texel(p + vec2<i32>(-1, 0));
    let e = texel(p + vec2<i32>(1, 0));
    let s = texel(p + vec2<i32>(0, 1));
    let lo = min(c, min(min(n, w), min(e, s)));
    let hi = max(c, max(max(n, w), max(e, s)));
    let amp = sqrt(clamp(min(lo, 1.0 - hi) / max(hi, vec3<f32>(1e-4)), vec3<f32>(0.0), vec3<f32>(1.0)));
    let weight = amp * (-1.0 / mix(8.0, 5.0, SHARPNESS));
    let sum = (n + w + e + s) * weight + c;
    return clamp(sum / (1.0 + 4.0 * weight), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_fxaa(in: FullscreenOut) -> @location(0) vec4<f32> {
    return vec4<f32>(fxaa(vec2<i32>(in.pos.xy)), 1.0);
}

@fragment
fn fs_sharpen(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    return vec4<f32>(sharpen(p, texel(p)), 1.0);
}

@fragment
fn fs_both(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    return vec4<f32>(sharpen(p, fxaa(p)), 1.0);
}
