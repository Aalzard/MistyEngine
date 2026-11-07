// Misty Virtual Geometry visibility raster (render/vgeo/vis.rs): the visible clusters write
// only which triangle each MSAA sample shows, with the depth test of the main pass; the
// resolve (vgeo_resolve.wgsl) then shades each covered pixel once per object instead of every
// small triangle in 2×2 quads. After PREPASS_SRC and vgeo_fetch.wgsl; the module starts with
// `enable primitive_index;`.
//
// Ids are raw bits in R32Float targets (core WebGPU can't multisample integer formats), always
// with bit 30 set and bit 28 clear so they are never denormal, infinite or NaN (the grass
// visibility buffer uses the same rule, with bit 29 clear):
//   id   = 0x60000000 | cluster << 7 | triangle    (bit 29 set = virtual geometry; cluster
//          < 2^21, triangle < 128)
//   draw = 0x40000000 | batch << 20 | draw          (batch < 512, draw < 2^20)
// 0 = no virtual geometry in that sample.

struct VgInfo {
    // x = visible-list entries per view, y = entries per batch.
    regions: vec4<u32>,
};
@group(3) @binding(5) var<uniform> vg_info: VgInfo;

struct VgVisOut {
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) @interpolate(flat) ids: vec2<u32>,
};

@vertex
fn vs_cluster_vis(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VgVisOut {
    return vg_vis_out(vertex, instance);
}

fn vg_vis_out(vertex: u32, instance: u32) -> VgVisOut {
    let item = vg_visible[instance];
    let c = vg_clusters[item.y];
    let local = vertex - instance * 384u;
    let p = vg_vertex_position(vg_cluster_vertices[c.vertex_first + local]);
    let d = draws[item.x];
    var out: VgVisOut;
    out.clip = view.view_proj * (d.model * vec4<f32>(p, 1.0));
    let batch = (instance % vg_info.regions.x) / vg_info.regions.y;
    out.ids = vec2<u32>(0x60000000u | (item.y << 7u), 0x40000000u | (batch << 20u) | item.x);
    return out;
}

// Cutout batches: the main pass's alpha test, so holes never reach the resolve.
struct VgVisCutoutOut {
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) @interpolate(flat) ids: vec2<u32>,
    @location(1) uv: vec2<f32>,
    // x = colour alpha, y = cutoff, z = 1 when the albedo alpha holds smoothness.
    @location(2) @interpolate(flat) alpha: vec3<f32>,
};

@vertex
fn vs_cluster_vis_cutout(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VgVisCutoutOut {
    let base = vg_vis_out(vertex, instance);
    let item = vg_visible[instance];
    let c = vg_clusters[item.y];
    let v = vg_vertex(vg_cluster_vertices[c.vertex_first + vertex - instance * 384u], item);
    let d = draws[item.x];
    var out: VgVisCutoutOut;
    out.clip = base.clip;
    out.ids = base.ids;
    out.uv = draw_uv(d, v.uv);
    out.alpha = vec3<f32>(d.color.a, d.surface.y, d.surface.w);
    return out;
}

struct VgVisTargets {
    @location(0) id: f32,
    @location(1) draw: f32,
};

@fragment
fn fs_cluster_vis(in: VgVisOut, @builtin(primitive_index) triangle: u32) -> VgVisTargets {
    var out: VgVisTargets;
    out.id = bitcast<f32>(in.ids.x | (triangle & 127u));
    out.draw = bitcast<f32>(in.ids.y);
    return out;
}

@fragment
fn fs_cluster_vis_cutout(in: VgVisCutoutOut, @builtin(primitive_index) triangle: u32) -> VgVisTargets {
    let base = textureSampleBias(albedo_map, material_sampler, in.uv, view.view_tan.z).a;
    let alpha = in.alpha.x * select(base, 1.0, in.alpha.z > 0.5);
    if (alpha < in.alpha.y) {
        discard;
    }
    var out: VgVisTargets;
    out.id = bitcast<f32>(in.ids.x | (triangle & 127u));
    out.draw = bitcast<f32>(in.ids.y);
    return out;
}
