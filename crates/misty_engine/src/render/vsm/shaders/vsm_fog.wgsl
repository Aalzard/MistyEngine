// Virtual shadow maps (render/vsm/fog.rs): the volumetric fog and sun shafts sample the sun's
// shadow through cascade-shaped maps; with the virtual shadow map on, each of those layers is
// filled from it instead of drawing the casters again. The cascades and the virtual map look
// along the same light direction from the same light view, so a cascade texel is one column of
// the virtual map: look its page up at the level whose texel matches the layer's and convert
// the depth to the layer's range.

@group(1) @binding(0) var vsm_table: texture_2d_array<u32>;
@group(1) @binding(1) var vsm_pool: texture_depth_2d;
@group(1) @binding(2) var<uniform> vsm: VsmParams;

struct FogLayer {
    // xy = top-left corner of the layer in light-space (x, -y) metres relative to the camera,
    // z = side (m), w = side (texels).
    rect: vec4<f32>,
    // x = camera depth minus the layer's near plane (m), y = 1 / the layer's depth range (m),
    // z = texel (m).
    depth: vec4<f32>,
};
@group(1) @binding(3) var<uniform> vsm_fog: array<FogLayer, 4>;

struct FogOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) layer: u32,
};

@vertex
fn vs_vsm_fog(@builtin(vertex_index) vi: u32, @builtin(instance_index) layer: u32) -> FogOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var out: FogOut;
    out.clip = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.layer = layer;
    return out;
}

@fragment
fn fs_vsm_fog(in: FogOut) -> @builtin(frag_depth) f32 {
    let f = vsm_fog[in.layer];
    let q = f.rect.xy + in.clip.xy / f.rect.w * f.rect.z;
    let levels = vsm.light_y.w;
    let l0 = vsm_level_of(f.depth.z, vsm.info.x, levels);
    for (var l = l0; l < i32(levels); l++) {
        let lv = vsm.levels[l];
        let t = (q - lv.xy) * lv.z;
        let page = floor(t / VSM_PAGE);
        if (any(page < vec2<f32>(0.0)) || any(page >= vec2<f32>(VSM_PAGES))) {
            continue;
        }
        let slot = vec2<i32>((page + vsm.wrap[l].xy) % VSM_PAGES);
        let phys = textureLoad(vsm_table, slot, l, 0).r & 0xFFFFu;
        if (phys == 0u) {
            continue;
        }
        let row = u32(vsm.pool.z);
        let origin = vec2<f32>(f32((phys - 1u) % row), f32((phys - 1u) / row)) * VSM_PAGE;
        let d = textureLoad(vsm_pool, vec2<i32>(origin + floor(t - page * VSM_PAGE)), 0);
        if (d >= 1.0) {
            return 1.0;
        }
        // Light-space depth relative to the camera, then into the layer's range.
        let rel = (d - vsm.info.z) / vsm.light_z.w;
        return clamp((f.depth.x + rel) * f.depth.y, 0.0, 1.0);
    }
    return 1.0;
}
