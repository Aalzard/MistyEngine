// Temporal anti-aliasing (render/taa.rs). The camera is jittered by a sub-pixel Halton offset
// every frame; this pass blends the resolved HDR image into a history reprojected through the
// motion vectors of the main pass (camera, moving objects, grass in the wind), so edges, thin
// grass and shading noise converge to a smooth, stable image. The history is fetched with a
// Catmull-Rom filter, clipped to the variance of the current 3×3 neighbourhood (YCoCg) and
// rejected where the depth says another surface was there, so ghosts stay short; a second
// pass (`fs_taa_sharpen`) writes it to the image through a mild unsharp mask, so a still
// scene stays as crisp as the single frame.

struct TaaParams {
    // Current frame without jitter: pixel + depth -> world.
    inv_view_proj: mat4x4<f32>,
    // Previous frame without jitter: world -> last frame's clip (depth rejection).
    prev_view_proj: mat4x4<f32>,
    // x = history valid (0/1), y = blend weight of the current frame, z = history sharpness,
    // w = 1 when orthographic.
    info: vec4<f32>,
    // xy = 1 / size, zw = size.
    texel: vec4<f32>,
    // x = near, y = far.
    clip: vec4<f32>,
};

@group(1) @binding(0) var taa_current: texture_2d<f32>;
@group(1) @binding(1) var taa_depth: texture_depth_multisampled_2d;
@group(1) @binding(2) var taa_history: texture_2d<f32>;
@group(1) @binding(3) var taa_sampler: sampler;
@group(1) @binding(4) var<uniform> taa: TaaParams;
@group(1) @binding(5) var taa_velocity: texture_multisampled_2d<f32>;

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

// View depth of device depth `d` (DirectX-style 0..1).
fn taa_linear(d: f32) -> f32 {
    let n = taa.clip.x;
    let f = taa.clip.y;
    if (taa.info.w > 0.5) {
        return n + d * (f - n);
    }
    return n * f / max(f - d * (f - n), 1e-6);
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

// Bilinear history tap (rgb) and its depth (a).
fn taa_history_tap(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(taa_history, taa_sampler, uv, 0.0);
}

@fragment
fn fs_taa(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    let current = taa_load(p);
    let depth = taa_depth_at(p);
    let z = taa_linear(depth);
    if (taa.info.x < 0.5) {
        return vec4<f32>(current, z);
    }

    // Neighbourhood statistics (YCoCg of the compressed colour) and the nearest depth
    // around the pixel: thin foreground edges reproject with the foreground's motion.
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

    // Reproject through the motion vector of the nearest surface in the neighbourhood.
    let texel = taa.texel.xy;
    let uv = (vec2<f32>(p) + 0.5) * texel;
    let last = vec2<i32>(taa.texel.zw) - vec2<i32>(1);
    let velocity = textureLoad(taa_velocity, clamp(nearest_p, vec2<i32>(0), last), 0).xy;
    let prev_uv = uv + velocity;
    if (any(prev_uv < vec2<f32>(0.0)) || any(prev_uv > vec2<f32>(1.0))) {
        return vec4<f32>(current, z);
    }
    let motion = length(velocity * taa.texel.zw);

    // History: Catmull-Rom (the sharpening is a separate pass on the way to the image, so it
    // never feeds back into the accumulation).
    let history = to_ycocg(taa_compress(taa_history_at(prev_uv)));

    // Disocclusion: the history there saw a surface at another depth than this one would
    // have had last frame.
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let world = taa.inv_view_proj * ndc;
    let prev_clip = taa.prev_view_proj * vec4<f32>(world.xyz / world.w, 1.0);
    let expected = select(prev_clip.w, z, taa.info.w > 0.5);
    // (Only where the pixel followed its own motion: a background pixel that borrowed a
    // foreground edge's vector lands on foreground history by design, and the clip handles it.)
    let seen = taa_history_tap(prev_uv).a;
    let own = all(nearest_p == p);
    let disoccluded = own && abs(seen - expected) > 0.1 * max(expected, 0.1) + 0.02;

    // Clip the history towards the box of what the pixel can be now. A still view keeps a
    // slightly wider box (less flicker from the jitter walking across thin detail); motion
    // tightens it so nothing smears.
    let gamma = mix(1.25, 1.0, smoothstep(0.25, 2.0, motion));
    let lo = mean - sigma * gamma;
    let hi = mean + sigma * gamma;
    let centre = 0.5 * (hi + lo);
    let extent = max(0.5 * (hi - lo), vec3<f32>(1e-5));
    let offset = history - centre;
    let units = abs(offset / extent);
    let most = max(units.x, max(units.y, units.z));
    var clipped = history;
    if (most > 1.0) {
        clipped = centre + offset / most;
    }

    // Blend: the current frame's share from the setting, more while the view moves fast
    // (the resampled history softens, fresh pixels keep it crisp) and all of it where the
    // history was disoccluded.
    var weight = mix(taa.info.y, max(taa.info.y, 0.3), smoothstep(2.0, 12.0, motion));
    if (disoccluded) {
        weight = 1.0;
    }
    let now = to_ycocg(taa_compress(current));
    let blended = mix(from_ycocg(clipped), from_ycocg(now), weight);
    return vec4<f32>(taa_expand(clamp(blended, vec3<f32>(0.0), vec3<f32>(0.999))), z);
}

// ---- History sharpening (the resolved history → the HDR image) ----

@group(1) @binding(0) var sharpen_src: texture_2d<f32>;
@group(1) @binding(1) var<uniform> sharpen: TaaParams;

fn sharpen_load(p: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(textureDimensions(sharpen_src)) - vec2<i32>(1);
    return textureLoad(sharpen_src, clamp(p, vec2<i32>(0), last), 0).rgb;
}

// Unsharp mask of the converged history against its 4-neighbour mean (in tonemapped space,
// clamped to the neighbourhood so edges sharpen without halos). Reads the history, writes
// the image; the history itself stays unsharpened, so nothing builds up frame on frame.
@fragment
fn fs_taa_sharpen(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    let c = taa_compress(sharpen_load(p));
    let k = sharpen.info.z;
    if (k <= 0.0) {
        return vec4<f32>(taa_expand(c), 1.0);
    }
    let n = taa_compress(sharpen_load(p + vec2<i32>(0, -1)));
    let s = taa_compress(sharpen_load(p + vec2<i32>(0, 1)));
    let w = taa_compress(sharpen_load(p + vec2<i32>(-1, 0)));
    let e = taa_compress(sharpen_load(p + vec2<i32>(1, 0)));
    let lo = min(c, min(min(n, s), min(w, e)));
    let hi = max(c, max(max(n, s), max(w, e)));
    let mean = 0.25 * (n + s + w + e);
    let sharp = clamp(c + (c - mean) * k, lo, hi);
    return vec4<f32>(taa_expand(clamp(sharp, vec3<f32>(0.0), vec3<f32>(0.999))), 1.0);
}
