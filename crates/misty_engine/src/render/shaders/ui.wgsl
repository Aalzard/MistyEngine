// Screen-space UI: textured, vertex-coloured triangles drawn over the final image.
// Vertex colours are sRGB; the target is an sRGB texture, so we output linear values.

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) mask: f32,
    @location(4) sdf: vec2<f32>,
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) mask: f32,
    @location(3) sdf: vec2<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(v.pos, 0.0, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    o.mask = v.mask;
    o.sdf = v.sdf;
    return o;
}

@group(0) @binding(0) var ui_texture: texture_2d<f32>;
@group(0) @binding(1) var ui_sampler: sampler;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn shade(i: VOut) -> vec4<f32> {
    let t = textureSample(ui_texture, ui_sampler, i.uv);
    // Distance field glyphs: 0.5 is the outline; sdf.x moves the edge (bold, outline, shadow),
    // sdf.y widens it (soft shadow). The screen-space derivative keeps edges one pixel wide.
    let w = max(fwidth(t.r) * 0.7, 0.0001) + i.sdf.y;
    let field = smoothstep(i.sdf.x - w, i.sdf.x + w, t.r);
    // mask = 1: single-channel glyph coverage used as alpha.
    let coverage = mix(t, vec4<f32>(1.0, 1.0, 1.0, t.r), min(i.mask, 1.0));
    let s = select(coverage, vec4<f32>(1.0, 1.0, 1.0, field), i.mask > 1.5);
    return vec4<f32>(srgb_to_linear(i.color.rgb) * s.rgb, i.color.a * s.a);
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    return shade(i);
}

// Camera Far Plane video: only where nothing was drawn (the depth buffer still holds its clear
// value, the sky).
@group(1) @binding(0) var scene_depth: texture_depth_multisampled_2d;

@fragment
fn fs_far(i: VOut) -> @location(0) vec4<f32> {
    let c = shade(i);
    let size = vec2<i32>(textureDimensions(scene_depth));
    let p = min(vec2<i32>(i.pos.xy), size - vec2<i32>(1));
    let d = textureLoad(scene_depth, p, 0);
    if (d < 0.99999) {
        discard;
    }
    return c;
}
