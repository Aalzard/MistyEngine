// Bindings and helpers of the Depth of Field and Motion Blur passes (render/postfx). They run
// after the temporal resolve at the output resolution; depth and motion vectors are the main
// pass's (internal resolution, multisampled: sample 0 is read, like TAA does).

@group(1) @binding(0) var fx_src: texture_2d<f32>;
@group(1) @binding(1) var fx_sampler: sampler;
@group(1) @binding(2) var fx_depth: texture_depth_multisampled_2d;
@group(1) @binding(3) var fx_velocity: texture_multisampled_2d<f32>;
@group(1) @binding(4) var<uniform> fx: FxParams;
@group(1) @binding(5) var fx_aux0: texture_2d<f32>;
@group(1) @binding(6) var fx_aux1: texture_2d<f32>;

fn fx_uv(ndc: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

// Internal pixel under an output uv.
fn fx_internal(uv: vec2<f32>) -> vec2<i32> {
    let last = vec2<i32>(fx.size.zw) - vec2<i32>(1);
    return clamp(vec2<i32>(uv * fx.size.zw), vec2<i32>(0), last);
}

fn fx_device_depth(uv: vec2<f32>) -> f32 {
    return textureLoad(fx_depth, fx_internal(uv), 0);
}

// Distance along the view axis of a device depth (0 = near plane, 1 = far plane).
fn fx_eye_depth(z: f32) -> f32 {
    let n = fx.camera.x;
    let f = fx.camera.y;
    if (fx.camera.z > 0.5) {
        return n + z * (f - n);
    }
    return n * f / max(f - z * (f - n), 1e-6);
}

fn fx_eye_depth_at(uv: vec2<f32>) -> f32 {
    return fx_eye_depth(fx_device_depth(uv));
}

// Interleaved gradient noise (0..1) of a pixel: decorrelates neighbouring samples.
fn fx_noise(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}
