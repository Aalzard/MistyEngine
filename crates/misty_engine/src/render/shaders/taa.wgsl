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
    // Temporal upscaling: xy = internal (render) size, zw = this frame's jitter in internal
    // pixels (a sample at pixel centre c saw the scene at c - jitter).
    upscale: vec4<f32>,
    // Upscaler (render/taa.rs): x = kernel exponent, y = weight gain, z = weight cap,
    // w = sharpening gain.
    tune: vec4<f32>,
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

// ---- Temporal upscaling (internal resolution → output-resolution history) ----

// Reconstruction kernel, d in output pixels: exp(-4.6 d²), σ ≈ 0.33 px (KERNEL in taa.rs).
fn taau_kernel(d: vec2<f32>) -> f32 {
    return exp(-taa.tune.x * dot(d, d));
}

// One output pixel rebuilt from the 3×3 internal samples nearest to it. Inputs (colour,
// depth, motion vectors) are internal-resolution; the history and the output are
// output-resolution.
@fragment
fn fs_taau(in: FullscreenOut) -> @location(0) vec4<f32> {
    let out_size = taa.texel.zw;
    let in_size = taa.upscale.xy;
    let jitter = taa.upscale.zw;
    let uv = in.pos.xy * taa.texel.xy;
    // Output pixels per internal pixel (> 1 when upscaling).
    let ratio = out_size / in_size;
    // This output pixel's centre in (unjittered) internal-pixel coordinates, and the
    // internal pixel whose jittered sample lies nearest to it.
    let pin = uv * in_size;
    let centre = vec2<i32>(floor(pin + jitter));

    var sharp = vec3<f32>(0.0);
    var sharp_w = 0.0;
    var smooth_c = vec3<f32>(0.0);
    var smooth_w = 0.0;
    var m1 = vec3<f32>(0.0);
    var m2 = vec3<f32>(0.0);
    var nearest = 1.0;
    var nearest_p = centre;
    // Weight of the closest sample in output pixels: how well this frame covers the pixel.
    var coverage = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let q = centre + vec2<i32>(x, y);
            let c = to_ycocg(taa_compress(taa_load(q)));
            // Where this sample saw the scene, relative to the output pixel's centre.
            let d_in = vec2<f32>(q) + 0.5 - jitter - pin;
            let w_sharp = taau_kernel(d_in * ratio);
            // Fallback upsample: σ ≈ 0.43 internal px (smooth, no blocks).
            let w_smooth = exp(-2.67 * dot(d_in, d_in));
            sharp += c * w_sharp;
            sharp_w += w_sharp;
            smooth_c += c * w_smooth;
            smooth_w += w_smooth;
            coverage = max(coverage, w_sharp);
            m1 += c;
            m2 += c * c;
            let d = taa_depth_at(q);
            if (d < nearest) {
                nearest = d;
                nearest_p = q;
            }
        }
    }
    let spatial = smooth_c / max(smooth_w, 1e-5);
    let depth = taa_depth_at(centre);
    let z = taa_linear(depth);
    if (taa.info.x < 0.5) {
        return vec4<f32>(taa_expand(clamp(from_ycocg(spatial), vec3<f32>(0.0), vec3<f32>(0.999))), z);
    }
    // This frame's estimate of the pixel: the sharp kernel's (only well defined where a
    // sample is close; elsewhere its weight below keeps it out of the history).
    let now = select(spatial, sharp / sharp_w, sharp_w > 1e-4);
    let mean = m1 / 9.0;
    let sigma = sqrt(max(m2 / 9.0 - mean * mean, vec3<f32>(0.0)));

    // Reproject through the motion of the nearest surface around the pixel.
    let last = vec2<i32>(in_size) - vec2<i32>(1);
    let velocity = textureLoad(taa_velocity, clamp(nearest_p, vec2<i32>(0), last), 0).xy;
    let prev_uv = uv + velocity;
    if (any(prev_uv < vec2<f32>(0.0)) || any(prev_uv > vec2<f32>(1.0))) {
        return vec4<f32>(taa_expand(clamp(from_ycocg(spatial), vec3<f32>(0.0), vec3<f32>(0.999))), z);
    }
    // Motion in output pixels.
    let motion = length(velocity * out_size);
    let history = to_ycocg(taa_compress(taa_history_at(prev_uv)));

    // Disocclusion: the history there saw another depth than this surface had last frame.
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let world = taa.inv_view_proj * ndc;
    let prev_clip = taa.prev_view_proj * vec4<f32>(world.xyz / world.w, 1.0);
    let expected = select(prev_clip.w, z, taa.info.w > 0.5);
    let seen = taa_history_tap(prev_uv).a;
    let own = all(nearest_p == centre);
    if (own && abs(seen - expected) > 0.1 * max(expected, 0.1) + 0.02) {
        return vec4<f32>(taa_expand(clamp(from_ycocg(spatial), vec3<f32>(0.0), vec3<f32>(0.999))), z);
    }

    // Clip the history to the box of the internal neighbourhood (wider at rest: the jitter
    // walking over thin blades stays in; tight in motion: nothing smears).
    let gamma = mix(1.25, 1.0, smoothstep(0.25, 2.0, motion));
    let lo = mean - sigma * gamma;
    let hi = mean + sigma * gamma;
    let box_centre = 0.5 * (hi + lo);
    let extent = max(0.5 * (hi - lo), vec3<f32>(1e-5));
    let offset = history - box_centre;
    let units = abs(offset / extent);
    let most = max(units.x, max(units.y, units.z));
    var clipped = history;
    if (most > 1.0) {
        clipped = box_centre + offset / most;
    }

    // Share of this frame: the setting's, scaled by how close this frame's nearest sample is
    // to the pixel's centre (× √(output pixels per internal pixel): a sample landing right
    // on the pixel counts more, without single samples making thin blades flicker, see
    // WEIGHT_GAIN in taa.rs); more while moving fast (the resampled history softens, fresh
    // samples keep it crisp).
    let pixels = ratio.x * ratio.y;
    let base = taa.info.y;
    let moving = smoothstep(2.0, 12.0, motion);
    let share = mix(base, max(base, 0.3), moving);
    let weight = clamp(share * coverage * pow(pixels, taa.tune.y), share * 0.05, taa.tune.z);
    let blended = mix(from_ycocg(clipped), from_ycocg(now), weight);
    return vec4<f32>(taa_expand(clamp(blended, vec3<f32>(0.0), vec3<f32>(0.999))), z);
}

// ---- History sharpening (the resolved history → the HDR image) ----

@group(1) @binding(0) var sharpen_src: texture_2d<f32>;
@group(1) @binding(1) var<uniform> sharpen: TaaParams;
@group(1) @binding(2) var sharpen_sampler: sampler;

// An upscaled target's frame without a temporal resolve: the internal image stretched over
// the output with a bilinear filter.
@fragment
fn fs_taa_stretch(in: FullscreenOut) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(in.ndc.x * 0.5 + 0.5, 0.5 - in.ndc.y * 0.5);
    return vec4<f32>(textureSampleLevel(sharpen_src, sharpen_sampler, uv, 0.0).rgb, 1.0);
}

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
    let k = sharpen.info.z * sharpen.tune.w;
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
