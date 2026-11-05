// Temporal anti-aliasing (render/taa.rs). The camera is jittered by a sub-pixel Halton offset
// every frame; this pass blends the resolved HDR image into a history reprojected from the
// depth buffer (camera motion), so edges, thin grass and shading noise converge to a smooth,
// stable image. History that no longer matches (moving or animated objects, disocclusion) is
// clipped to the current pixel's 3×3 colour range, which keeps ghosting short.

struct TaaParams {
    // Current frame without jitter: pixel + depth -> world.
    inv_view_proj: mat4x4<f32>,
    // Previous frame without jitter: world -> last frame's pixel.
    prev_view_proj: mat4x4<f32>,
    // x = history valid (0/1), y = blend weight of the current frame, zw = 1 / size.
    info: vec4<f32>,
};

@group(1) @binding(0) var taa_current: texture_2d<f32>;
@group(1) @binding(1) var taa_depth: texture_depth_multisampled_2d;
@group(1) @binding(2) var taa_history: texture_2d<f32>;
@group(1) @binding(3) var taa_sampler: sampler;
@group(1) @binding(4) var<uniform> taa: TaaParams;

fn taa_load(p: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(textureDimensions(taa_current)) - vec2<i32>(1);
    return textureLoad(taa_current, clamp(p, vec2<i32>(0), last), 0).rgb;
}

fn taa_depth_at(p: vec2<i32>) -> f32 {
    let last = vec2<i32>(textureDimensions(taa_depth)) - vec2<i32>(1);
    return textureLoad(taa_depth, clamp(p, vec2<i32>(0), last), 0);
}

// Tonemapped-space blending keeps bright highlights from dominating (no flicker).
fn taa_compress(c: vec3<f32>) -> vec3<f32> {
    return c / (1.0 + max(c.r, max(c.g, c.b)));
}

fn taa_expand(c: vec3<f32>) -> vec3<f32> {
    return c / max(1.0 - max(c.r, max(c.g, c.b)), 1e-4);
}

fn to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.25 * c.r + 0.5 * c.g + 0.25 * c.b,
        0.5 * c.r - 0.5 * c.b,
        -0.25 * c.r + 0.5 * c.g - 0.25 * c.b,
    );
}

fn from_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z);
}

// Five-tap Catmull-Rom history fetch: sharp where bilinear would soften a little more each frame.
fn taa_history_at(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(taa_history));
    let pos = uv * size;
    let centre = floor(pos - 0.5) + 0.5;
    let f = pos - centre;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let tc0 = (centre - 1.0) / size;
    let tc3 = (centre + 2.0) / size;
    let tc12 = (centre + w2 / w12) / size;
    var c = vec3<f32>(0.0);
    var w = 0.0;
    c += textureSampleLevel(taa_history, taa_sampler, vec2<f32>(tc12.x, tc0.y), 0.0).rgb * (w12.x * w0.y);
    c += textureSampleLevel(taa_history, taa_sampler, vec2<f32>(tc0.x, tc12.y), 0.0).rgb * (w0.x * w12.y);
    c += textureSampleLevel(taa_history, taa_sampler, vec2<f32>(tc12.x, tc12.y), 0.0).rgb * (w12.x * w12.y);
    c += textureSampleLevel(taa_history, taa_sampler, vec2<f32>(tc3.x, tc12.y), 0.0).rgb * (w3.x * w12.y);
    c += textureSampleLevel(taa_history, taa_sampler, vec2<f32>(tc12.x, tc3.y), 0.0).rgb * (w12.x * w3.y);
    w = w12.x * w0.y + w0.x * w12.y + w12.x * w12.y + w3.x * w12.y + w12.x * w3.y;
    return max(c / max(w, 1e-4), vec3<f32>(0.0));
}

@fragment
fn fs_taa(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    let current = taa_load(p);
    if (taa.info.x < 0.5) {
        return vec4<f32>(current, 1.0);
    }

    // Neighbourhood statistics (YCoCg of the compressed colour) and the nearest depth
    // around the pixel, so thin foreground edges reproject with the foreground's motion.
    var m1 = vec3<f32>(0.0);
    var m2 = vec3<f32>(0.0);
    var nearest = 1.0;
    var nearest_p = p;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let q = p + vec2<i32>(x, y);
            let c = to_ycocg(taa_compress(taa_load(q)));
            m1 += c;
            m2 += c * c;
            let d = taa_depth_at(q);
            if (d < nearest) {
                nearest = d;
                nearest_p = q;
            }
        }
    }
    let mean = m1 / 9.0;
    let sigma = sqrt(max(m2 / 9.0 - mean * mean, vec3<f32>(0.0)));

    // Reproject the pixel centre through the (unjittered) cameras.
    let texel = taa.info.zw;
    let uv = (vec2<f32>(p) + 0.5) * texel;
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, nearest, 1.0);
    let world = taa.inv_view_proj * ndc;
    let prev_clip = taa.prev_view_proj * vec4<f32>(world.xyz / world.w, 1.0);
    let prev_ndc = prev_clip.xy / max(prev_clip.w, 1e-6);
    let prev_uv = vec2<f32>(prev_ndc.x * 0.5 + 0.5, 0.5 - prev_ndc.y * 0.5);
    if (any(prev_uv < vec2<f32>(0.0)) || any(prev_uv > vec2<f32>(1.0)) || prev_clip.w <= 0.0) {
        return vec4<f32>(current, 1.0);
    }

    // Clip the history towards the box of what the pixel can be now.
    let history = to_ycocg(taa_compress(taa_history_at(prev_uv)));
    let lo = mean - sigma * 1.0;
    let hi = mean + sigma * 1.0;
    let centre = 0.5 * (hi + lo);
    let extent = max(0.5 * (hi - lo), vec3<f32>(1e-5));
    let offset = history - centre;
    let units = abs(offset / extent);
    let most = max(units.x, max(units.y, units.z));
    var clipped = history;
    if (most > 1.0) {
        clipped = centre + offset / most;
    }

    // Faster refresh while the camera moves fast (less smearing), slow when still.
    let motion = length((prev_uv - uv) / texel);
    let weight = clamp(taa.info.y + motion * 0.01, taa.info.y, 0.5);
    let now = to_ycocg(taa_compress(current));
    let blended = mix(from_ycocg(clipped), from_ycocg(now), weight);
    return vec4<f32>(taa_expand(clamp(blended, vec3<f32>(0.0), vec3<f32>(0.999))), 1.0);
}
