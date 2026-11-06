// Grass in the depth + normal prepass of the screen-space effects: appended to
// grass_field.wgsl and grass_blade.wgsl. Near blades write the same depth and shading normal
// as the lit pass (alpha tested), so ambient occlusion, contact shadows, the shadow mask and
// SSGI / SSR see the blades instead of the ground behind them.

struct GrassPrepassOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    // xy = atlas uv, z = 0 close up .. 1 far.
    @location(1) uv: vec3<f32>,
    // Last frame's clip position (xyw): where the wind had the vertex a frame ago.
    @location(2) clip_prev: vec3<f32>,
};

struct GrassPrepassTargets {
    @location(0) normal: vec4<f32>,
    @location(1) velocity: vec2<f32>,
};

@vertex
fn vs_grass_prepass(@builtin(vertex_index) vi: u32, b: BladeIn) -> GrassPrepassOut {
    let v = grass_blade(vi, b);
    let face = normalize(cross(v.across, v.tangent));
    var out: GrassPrepassOut;
    out.clip = view.view_proj * vec4<f32>(v.p, 1.0);
    // (Only while the wind's vectors are wanted: the clock moved and TAA is on.)
    var prev = v.p;
    if (grass.screen.w > 0.0) {
        prev = grass_blade_at(vi, b, grass.origin.w - grass.screen.w).p;
    }
    out.clip_prev = prev_clip(prev);
    out.normal = normalize(face + v.across * v.side * 0.8);
    out.uv = vec3<f32>((f32(v.cell) + v.side + 0.5) / 8.0, 1.0 - v.t, smoothstep(4.0, 35.0, v.dist));
    return out;
}

@fragment
fn fs_grass_prepass(in: GrassPrepassOut, @builtin(front_facing) front: bool) -> GrassPrepassTargets {
    if (textureSample(grass_atlas, grass_sampler, in.uv.xy).a < 0.5) {
        discard;
    }
    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    // The lit pass leans blade normals up the same way (grass.wgsl).
    n = normalize(mix(n, vec3<f32>(0.0, 1.0, 0.0), mix(0.5, 0.8, in.uv.z)));
    var out: GrassPrepassTargets;
    // w = 1 + smoothness (a matte sheen); grass sends light through SSGI.
    out.normal = vec4<f32>(n, 1.1);
    out.velocity = velocity_of(in.clip, in.clip_prev);
    return out;
}
