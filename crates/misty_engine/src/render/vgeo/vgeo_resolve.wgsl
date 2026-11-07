// Misty Virtual Geometry material resolve (render/vgeo/vis.rs), after MESH_SRC: a full-screen
// triangle in the main pass, drawn twice per batch (material texture set). Each pixel reads
// the ids its MSAA samples got from the visibility raster (vgeo_vis.wgsl), rebuilds the
// surface of one triangle at the pixel centre (perspective-correct barycentrics and their
// screen derivatives, so textures get explicit gradients) and lights it with mesh.wgsl's
// Standard shading, written only to the samples it owns (`sample_mask`):
//   layer 0  the object (draw) of the pixel's first covered sample;
//   layer 1  every other covered sample of the batch of the next object found (silhouettes:
//            a third object in one pixel takes the second's shading).
// A surface is continuous across its triangles, so one shade per object per pixel keeps the
// look of per-triangle MSAA shading while running about once per covered pixel instead of
// once per triangle per 2×2 quad.

struct VgResolve {
    // Word offsets in `vg_pool` of the vertices, local indices, clusters, cluster vertices.
    offsets: vec4<u32>,
    // x = MSAA samples.
    info: vec4<u32>,
};

// The whole virtual geometry pool (render/vgeo/mod.rs `Pool`), one binding.
@group(3) @binding(0) var<storage, read> vg_pool: array<u32>;
@group(3) @binding(1) var vg_vis_ids: texture_multisampled_2d<f32>;
@group(3) @binding(2) var vg_vis_draws: texture_multisampled_2d<f32>;
@group(3) @binding(3) var<uniform> vr: VgResolve;

// Words per pool vertex and per cluster record (GpuVertex / GpuCluster in render/vgeo/mod.rs).
const VR_VERTEX_WORDS: u32 = 8u;
const VR_CLUSTER_WORDS: u32 = 20u;

struct VgResolveIn {
    @builtin(position) pos: vec4<f32>,
    // batch × 2 + layer.
    @location(0) @interpolate(flat) pass_id: u32,
};

@vertex
fn vs_cluster_resolve(@builtin(vertex_index) i: u32, @builtin(instance_index) pass_id: u32) -> VgResolveIn {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VgResolveIn;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.pass_id = pass_id;
    return out;
}

fn vr_octahedral(w: u32) -> vec3<f32> {
    let e = unpack2x16snorm(w);
    var n = vec3<f32>(e.x, e.y, 1.0 - abs(e.x) - abs(e.y));
    let t = max(-n.z, 0.0);
    n.x += select(t, -t, n.x >= 0.0);
    n.y += select(t, -t, n.y >= 0.0);
    return normalize(n);
}

struct VrCorner {
    position: vec3<f32>,
    normal: vec3<f32>,
    tangent: vec4<f32>,
    uv: vec2<f32>,
    uv2: vec2<f32>,
};

// Corner `corner` (0 .. 3 × triangles) of cluster `cluster`, as vgeo_fetch.wgsl's vg_vertex.
fn vr_corner(cluster: u32, corner: u32) -> VrCorner {
    let cb = vr.offsets.z + cluster * VR_CLUSTER_WORDS;
    let vertex_first = vg_pool[cb + 15u];
    let li = vg_pool[cb + 16u] + corner;
    let local = (vg_pool[vr.offsets.y + (li >> 1u)] >> ((li & 1u) * 16u)) & 0xffffu;
    let source = vg_pool[vr.offsets.w + vertex_first + local];
    let b = vr.offsets.x + source * VR_VERTEX_WORDS;
    var v: VrCorner;
    v.position = vec3<f32>(bitcast<f32>(vg_pool[b]), bitcast<f32>(vg_pool[b + 1u]), bitcast<f32>(vg_pool[b + 2u]));
    v.normal = vr_octahedral(vg_pool[b + 3u]);
    let t = vg_pool[b + 4u];
    v.tangent = vec4<f32>(vr_octahedral(t), select(1.0, -1.0, (t & 1u) != 0u));
    v.uv = vec2<f32>(bitcast<f32>(vg_pool[b + 5u]), bitcast<f32>(vg_pool[b + 6u]));
    v.uv2 = unpack2x16unorm(vg_pool[b + 7u]);
    return v;
}

// Perspective-correct barycentrics of a pixel inside a triangle (clip positions c0..c2) and
// their change one pixel right (ddx) and down (ddy).
struct VrBary {
    l: vec3<f32>,
    ddx: vec3<f32>,
    ddy: vec3<f32>,
};

fn vr_bary(c0: vec4<f32>, c1: vec4<f32>, c2: vec4<f32>, ndc: vec2<f32>, size: vec2<f32>) -> VrBary {
    let inv_w = 1.0 / vec3<f32>(c0.w, c1.w, c2.w);
    let n0 = c0.xy * inv_w.x;
    let n1 = c1.xy * inv_w.y;
    let n2 = c2.xy * inv_w.z;
    let inv_det = 1.0 / determinant(mat2x2<f32>(n2 - n1, n0 - n1));
    // Change of barycentric / w per unit of NDC x and y.
    let gx = vec3<f32>(n1.y - n2.y, n2.y - n0.y, n0.y - n1.y) * inv_det * inv_w;
    let gy = vec3<f32>(n2.x - n1.x, n0.x - n2.x, n1.x - n0.x) * inv_det * inv_w;
    let gx_sum = gx.x + gx.y + gx.z;
    let gy_sum = gy.x + gy.y + gy.z;
    let delta = ndc - n0;
    let interp_inv_w = inv_w.x + delta.x * gx_sum + delta.y * gy_sum;
    let interp_w = 1.0 / interp_inv_w;
    var b: VrBary;
    b.l = interp_w * (vec3<f32>(inv_w.x, 0.0, 0.0) + delta.x * gx + delta.y * gy);
    // One pixel right / down in NDC (y up).
    let px = 2.0 / size.x;
    let py = -2.0 / size.y;
    b.ddx = (b.l * interp_inv_w + gx * px) / (interp_inv_w + gx_sum * px) - b.l;
    b.ddy = (b.l * interp_inv_w + gy * py) / (interp_inv_w + gy_sum * py) - b.l;
    return b;
}

fn vr_mix3(b: vec3<f32>, a0: vec3<f32>, a1: vec3<f32>, a2: vec3<f32>) -> vec3<f32> {
    return a0 * b.x + a1 * b.y + a2 * b.z;
}

fn vr_mix2(b: vec3<f32>, a0: vec2<f32>, a1: vec2<f32>, a2: vec2<f32>) -> vec2<f32> {
    return a0 * b.x + a1 * b.y + a2 * b.z;
}

struct VgResolveTargets {
    @location(0) color: vec4<f32>,
    @location(1) glow: vec4<f32>,
    @location(2) velocity: vec2<f32>,
    @builtin(sample_mask) mask: u32,
};

fn vr_batch(draw_word: u32) -> u32 {
    return (draw_word >> 20u) & 0x1ffu;
}

@fragment
fn fs_cluster_resolve(in: VgResolveIn) -> VgResolveTargets {
    let batch = in.pass_id >> 1u;
    let layer = in.pass_id & 1u;
    let p = vec2<i32>(in.pos.xy);
    // Pick this layer's object and the samples it owns.
    var first = 0u;
    var key = 0u;
    var rep = 0u;
    var mask = 0u;
    for (var s = 0u; s < vr.info.x; s++) {
        let id = bitcast<u32>(textureLoad(vg_vis_ids, p, i32(s)).r);
        if (id == 0u) {
            continue;
        }
        let obj = bitcast<u32>(textureLoad(vg_vis_draws, p, i32(s)).r);
        if (first == 0u) {
            first = obj;
        }
        if (layer == 0u) {
            if (obj == first) {
                mask |= 1u << s;
                rep = select(rep, id, rep == 0u);
            }
        } else if (obj != first) {
            if (key == 0u) {
                key = obj;
                rep = id;
            }
            if (vr_batch(obj) == vr_batch(key)) {
                mask |= 1u << s;
            }
        }
    }
    key = select(key, first, layer == 0u);
    if (mask == 0u || vr_batch(key) != batch) {
        discard;
    }

    let draw = key & 0xfffffu;
    let cluster = (rep >> 7u) & 0x1fffffu;
    let corner = (rep & 127u) * 3u;
    let d = draws[draw];
    let v0 = vr_corner(cluster, corner);
    let v1 = vr_corner(cluster, corner + 1u);
    let v2 = vr_corner(cluster, corner + 2u);
    let w0 = (d.model * vec4<f32>(v0.position, 1.0)).xyz;
    let w1 = (d.model * vec4<f32>(v1.position, 1.0)).xyz;
    let w2 = (d.model * vec4<f32>(v2.position, 1.0)).xyz;
    let size = view.motion.zw;
    let ndc = vec2<f32>(in.pos.x / size.x * 2.0 - 1.0, 1.0 - in.pos.y / size.y * 2.0);
    let b = vr_bary(
        view.view_proj * vec4<f32>(w0, 1.0),
        view.view_proj * vec4<f32>(w1, 1.0),
        view.view_proj * vec4<f32>(w2, 1.0),
        ndc,
        size,
    );

    var m: MeshOut;
    m.clip = vec4<f32>(in.pos.xy, 0.0, 1.0);
    m.world = vr_mix3(b.l, w0, w1, w2);
    m.normal = vr_mix3(
        b.l,
        (d.normal * vec4<f32>(v0.normal, 0.0)).xyz,
        (d.normal * vec4<f32>(v1.normal, 0.0)).xyz,
        (d.normal * vec4<f32>(v2.normal, 0.0)).xyz,
    );
    let uv = vr_mix2(b.l, v0.uv, v1.uv, v2.uv);
    m.uv = draw_uv(d, uv);
    let uv_dx = draw_uv(d, uv + vr_mix2(b.ddx, v0.uv, v1.uv, v2.uv)) - m.uv;
    let uv_dy = draw_uv(d, uv + vr_mix2(b.ddy, v0.uv, v1.uv, v2.uv)) - m.uv;
    m.draw = draw;
    let tangent = vr_mix3(
        b.l,
        (d.model * vec4<f32>(v0.tangent.xyz, 0.0)).xyz,
        (d.model * vec4<f32>(v1.tangent.xyz, 0.0)).xyz,
        (d.model * vec4<f32>(v2.tangent.xyz, 0.0)).xyz,
    );
    m.tangent = vec4<f32>(tangent, v0.tangent.w * d.emission.w);
    m.lightmap_uv = vr_mix2(b.l, v0.uv2, v1.uv2, v2.uv2) * d.lightmap_st.xy + d.lightmap_st.zw;
    m.clip_prev = vr_mix3(
        b.l,
        prev_clip((d.prev_model * vec4<f32>(v0.position, 1.0)).xyz),
        prev_clip((d.prev_model * vec4<f32>(v1.position, 1.0)).xyz),
        prev_clip((d.prev_model * vec4<f32>(v2.position, 1.0)).xyz),
    );

    let shaded = shade(m.world, standard_surface_grad(m, true, uv_dx, uv_dy));
    var out: VgResolveTargets;
    out.color = shaded.color;
    out.glow = shaded.glow;
    out.velocity = mesh_velocity(m);
    out.mask = mask;
    return out;
}
