// Coloured line list for gizmos. Vertex colours are sRGB.

struct LineIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: LineIn) -> LineOut {
    var out: LineOut;
    out.pos = view.view_proj * vec4<f32>(in.position, 1.0);
    out.color = vec4<f32>(srgb_to_linear(in.color.rgb), in.color.a);
    return out;
}

@fragment
fn fs_main(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}
