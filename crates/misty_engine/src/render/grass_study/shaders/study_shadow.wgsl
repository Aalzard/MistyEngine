// Grass study: shadow casters into the sun cascades (blades: a copy of grass_shadow.wgsl;
// the ground: depth only).

@group(2) @binding(0) var<uniform> caster_vp: mat4x4<f32>;

struct GrassShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_grass_shadow(@builtin(vertex_index) vi: u32, b: BladeIn) -> GrassShadowOut {
    var out: GrassShadowOut;
    if (grass_flat(b) || grass_outside(b, caster_vp, false)) {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let v = grass_blade(vi, b);
    out.clip = caster_vp * vec4<f32>(v.p, 1.0);
    out.uv = vec2<f32>(grass_card_u(v.cell, v.side, v.repeat), grass_card_v(v.t, b.shape.w));
    return out;
}

@fragment
fn fs_grass_shadow(in: GrassShadowOut) {
    let leaf = grass_atlas_uv(in.uv);
    if (textureSampleLevel(grass_atlas, grass_sampler, leaf.xy, 1.0).a * leaf.z < 0.3) {
        discard;
    }
}

@vertex
fn vs_ground_shadow(@location(0) p: vec3<f32>) -> @builtin(position) vec4<f32> {
    return caster_vp * vec4<f32>(p, 1.0);
}
