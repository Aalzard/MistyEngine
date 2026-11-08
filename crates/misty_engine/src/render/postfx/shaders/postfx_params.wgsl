// Uniform of the Depth of Field, Motion Blur and Auto Exposure passes (render/postfx, the
// Rust copy is `FxParams` in params.rs). One per view, written every render they run.

struct FxParams {
    // This frame's view-projection without jitter, inverted: output uv + depth -> world.
    inv_view_proj: mat4x4<f32>,
    // Last frame's view-projection without jitter: world -> last frame's clip space.
    prev_view_proj: mat4x4<f32>,
    // x = near, y = far, z = 1 when orthographic.
    camera: vec4<f32>,
    // xy = output (post-processing) size, zw = internal (render) size, in pixels.
    size: vec4<f32>,
    // Depth of Field: x = mode (1 Gaussian, 2 Bokeh), y = focus distance (Bokeh) or blur
    // start (Gaussian), z = CoC scale in output pixels (Bokeh) or blur end (Gaussian),
    // w = largest CoC radius in output pixels.
    dof: vec4<f32>,
    // x = aperture blades, y = blade curvature (1 = round), z = rotation (radians),
    // w = gather rings (Bokeh) or taps per side (Gaussian).
    dof_shape: vec4<f32>,
    // x = tiles each side the near CoC is spread over.
    dof_tiles: vec4<f32>,
    // Motion Blur: x = shutter share, y = longest blur in output pixels, z = samples,
    // w = longest camera blur in output pixels (negative = no camera blur).
    mb: vec4<f32>,
    // Auto Exposure histogram: x = log2 luminance of the first bin, y = log2 range,
    // z = low and w = high share of the pixels kept (0..1).
    ae: vec4<f32>,
    // x = limit min, y = limit max, z = compensation (EV), w = metering (0 average,
    // 1 spot, 2 centre weighted).
    ae_limits: vec4<f32>,
    // x = share of the way to a brighter target taken this frame, y = to a darker one,
    // z = 1 to jump straight to the target.
    ae_adapt: vec4<f32>,
};
