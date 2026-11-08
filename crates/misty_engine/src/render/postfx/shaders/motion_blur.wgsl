// Motion Blur (render/postfx/motion_blur.rs) from the main pass's motion vectors, after the
// temporal resolve, at the output resolution:
//   tiles      largest blur of each 16×16 output-pixel tile;
//   neighbours largest of the 3×3 tiles around (blur crosses tile edges);
//   gather     samples along the neighbourhood's largest blur, weighted by depth and each
//              sample's own blur (a reconstruction filter after McGuire et al. 2012): a moving
//              object smears over the still background, the background behind it shows
//              through its smear, and a still object in front stays sharp.
// The camera's share of a pixel's motion is found by reprojecting its depth with last frame's
// camera; it is clamped on its own (Camera Clamp) or dropped (Camera Motion Blur off).

// Blur vector in output pixels of the surface under output uv `uv`.
fn mb_velocity(uv: vec2<f32>) -> vec2<f32> {
    let p = fx_internal(uv);
    let v = textureLoad(fx_velocity, p, 0).xy;
    let z = textureLoad(fx_depth, p, 0);
    var camera = v;
    if (z < 1.0) {
        let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let w = fx.inv_view_proj * vec4<f32>(ndc, z, 1.0);
        let c = fx.prev_view_proj * vec4<f32>(w.xyz / w.w, 1.0);
        camera = select(vec2<f32>(0.0), (c.xy / c.w - ndc) * vec2<f32>(0.5, -0.5), c.w > 1e-6);
    }
    let scale = fx.size.xy * fx.mb.x;
    var camera_px = camera * scale;
    let object_px = (v - camera) * scale;
    if (fx.mb.w < 0.0) {
        camera_px = vec2<f32>(0.0);
    } else {
        camera_px *= min(1.0, fx.mb.w / max(length(camera_px), 1e-4));
    }
    let blur = object_px + camera_px;
    return blur * min(1.0, fx.mb.y / max(length(blur), 1e-4));
}

// Largest blur (by length) of a 16×16 output-pixel tile, sampled every other pixel.
@fragment
fn fs_mb_tiles(in: FullscreenOut) -> @location(0) vec4<f32> {
    let tile = floor(in.pos.xy) * 16.0;
    var best = vec2<f32>(0.0);
    for (var y = 0; y < 8; y++) {
        for (var x = 0; x < 8; x++) {
            let uv = (tile + vec2<f32>(f32(x), f32(y)) * 2.0 + 1.0) / fx.size.xy;
            let v = mb_velocity(min(uv, vec2<f32>(0.9999)));
            if (dot(v, v) > dot(best, best)) {
                best = v;
            }
        }
    }
    return vec4<f32>(best, 0.0, 1.0);
}

// Largest blur of the 3×3 tiles around.
@fragment
fn fs_mb_neighbours(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    let last = vec2<i32>(textureDimensions(fx_src)) - vec2<i32>(1);
    var best = vec2<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let v = textureLoad(fx_src, clamp(p + vec2<i32>(x, y), vec2<i32>(0), last), 0).xy;
            if (dot(v, v) > dot(best, best)) {
                best = v;
            }
        }
    }
    return vec4<f32>(best, 0.0, 1.0);
}

// 1 when depth `a` is in front of `b` (soft over a few percent of the distance).
fn mb_in_front(a: f32, b: f32) -> f32 {
    return clamp(1.0 - (a - b) / max(0.02 * b, 0.05), 0.0, 1.0);
}

fn mb_cone(d: f32, len: f32) -> f32 {
    return clamp(1.0 - d / len, 0.0, 1.0);
}

fn mb_cylinder(d: f32, len: f32) -> f32 {
    return 1.0 - smoothstep(0.95 * len, 1.05 * len, d);
}

// aux0 = the neighbourhood's largest blur per tile.
@fragment
fn fs_mb_gather(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    let uv = (vec2<f32>(p) + 0.5) / fx.size.xy;
    let color = textureLoad(fx_src, p, 0).rgb;
    let largest = textureLoad(fx_aux0, p / 16, 0).xy;
    if (length(largest) < 0.5) {
        return vec4<f32>(color, 1.0);
    }
    let own = max(length(mb_velocity(uv)), 0.5);
    let z = fx_eye_depth_at(uv);
    let n = i32(fx.mb.z);
    let jitter = fx_noise(in.pos.xy) - 0.5;
    var sum = vec4<f32>(color / own, 1.0 / own);
    for (var i = 0; i < n; i++) {
        let t = (f32(i) + 0.5 + jitter) / f32(n) - 0.5;
        let offset = largest * t;
        let suv = uv + offset / fx.size.xy;
        if (any(suv < vec2<f32>(0.0)) || any(suv > vec2<f32>(1.0))) {
            continue;
        }
        let d = length(offset);
        let len = max(length(mb_velocity(suv)), 0.5);
        let zs = fx_eye_depth_at(suv);
        let w = mb_in_front(zs, z) * mb_cone(d, len)
            + mb_in_front(z, zs) * mb_cone(d, own)
            + mb_cylinder(d, len) * mb_cylinder(d, own) * 2.0;
        let s = textureSampleLevel(fx_src, fx_sampler, suv, 0.0).rgb;
        sum += vec4<f32>(s * w, w);
    }
    return vec4<f32>(sum.rgb / sum.a, 1.0);
}
