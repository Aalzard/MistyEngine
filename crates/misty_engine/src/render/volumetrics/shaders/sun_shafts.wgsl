// Screen-space sun shafts (included after common.wgsl, fullscreen passes at half resolution):
// bright, unoccluded sky around the sun → three radial blurs towards the sun (each pass
// shorter, together about 500 taps) → added onto the HDR scene.

struct Shafts {
    // xy = sun position (uv), z = fade (0 when the sun is off screen or behind), w = threshold.
    sun: vec4<f32>,
    // rgb = linear tint × sun colour × intensity, w = streak length (fraction of the way to the sun).
    color: vec4<f32>,
    // x = source radius (fraction of the screen height), y = aspect (width / height).
    shape: vec4<f32>,
};

@group(1) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(1) var source_sampler: sampler;
@group(1) @binding(2) var scene_depth: texture_depth_multisampled_2d;
@group(1) @binding(3) var<uniform> shafts: Shafts;

const TAPS: i32 = 8;

fn screen_uv(ndc: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

// Sky brighter than the threshold near the sun; geometry (depth < 1) blocks it.
@fragment
fn fs_prefilter(in: FullscreenOut) -> @location(0) vec4<f32> {
    let uv = screen_uv(in.ndc);
    let size = vec2<f32>(textureDimensions(scene_depth));
    let texel = vec2<i32>(clamp(uv * size, vec2<f32>(0.0), size - 1.0));
    if (textureLoad(scene_depth, texel, 0) < 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let c = textureSampleLevel(source, source_sampler, uv, 0.0).rgb;
    let bright = max(c - vec3<f32>(shafts.sun.w), vec3<f32>(0.0));
    let offset = (uv - shafts.sun.xy) * vec2<f32>(shafts.shape.y, 1.0);
    let r = clamp(1.0 - length(offset) / max(shafts.shape.x, 1e-3), 0.0, 1.0);
    return vec4<f32>(bright * r * r, 1.0);
}

// Average of TAPS samples from `uv` towards the sun over `scale` of the streak length.
fn radial(uv: vec2<f32>, scale: f32) -> vec3<f32> {
    let step = (shafts.sun.xy - uv) * shafts.color.w * scale / f32(TAPS);
    var sum = vec3<f32>(0.0);
    var weight = 1.0;
    var total = 0.0;
    for (var i = 0; i < TAPS; i++) {
        sum += textureSampleLevel(source, source_sampler, uv + step * f32(i), 0.0).rgb * weight;
        total += weight;
        weight *= 0.9;
    }
    return sum / total;
}

@fragment
fn fs_blur_long(in: FullscreenOut) -> @location(0) vec4<f32> {
    return vec4<f32>(radial(screen_uv(in.ndc), 1.0), 1.0);
}

@fragment
fn fs_blur_mid(in: FullscreenOut) -> @location(0) vec4<f32> {
    return vec4<f32>(radial(screen_uv(in.ndc), 1.0 / 8.0), 1.0);
}

@fragment
fn fs_blur_short(in: FullscreenOut) -> @location(0) vec4<f32> {
    return vec4<f32>(radial(screen_uv(in.ndc), 1.0 / 64.0), 1.0);
}

// Added onto the HDR scene (additive blending).
@fragment
fn fs_composite(in: FullscreenOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(source, source_sampler, screen_uv(in.ndc), 0.0).rgb;
    return vec4<f32>(c * shafts.color.rgb * shafts.sun.z, 1.0);
}
