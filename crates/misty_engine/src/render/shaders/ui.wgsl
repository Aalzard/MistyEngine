// Screen-space UI: textured, vertex-coloured triangles drawn over the final image.
// Vertex colours are sRGB; the target is an sRGB texture, so we output linear values.

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) mask: f32,
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) mask: f32,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(v.pos, 0.0, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    o.mask = v.mask;
    return o;
}

@group(0) @binding(0) var ui_texture: texture_2d<f32>;
@group(0) @binding(1) var ui_sampler: sampler;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(ui_texture, ui_sampler, i.uv);
    // mask = 1: single-channel glyph coverage used as alpha.
    let s = mix(t, vec4<f32>(1.0, 1.0, 1.0, t.r), i.mask);
    return vec4<f32>(srgb_to_linear(i.color.rgb) * s.rgb, i.color.a * s.a);
}
