// Misty Virtual Geometry: vertex pulling of the visible clusters (render/vgeo/mod.rs).
// Instance i is entry i of the visible list. Two ways to draw:
//   indexed (`vg_fetch_indexed`): one indexed sub-draw per cluster, its local indices into
//     its own vertex list, base vertex = entry × 384: the post-transform vertex cache works.
//   pulled (`vg_fetch`): 384 vertices per cluster, the index read from storage; triangles
//     past a cluster's count collapse onto one vertex (no area, dropped). No vertex reuse.

// The cluster record (render/vgeo/mod.rs `GpuCluster`, vgeo_cull.wgsl `Cluster`).
struct VgCluster {
    bounds: vec4<f32>,
    group: u32,
    parent_group: u32,
    parent_error: f32,
    // Triangles (8 bits) | level << 8 (6 bits) | material slot << 14.
    packed: u32,
    vertex_first: u32,
    local_first: u32,
    cone_axis: u32,
    cone_cutoff: f32,
};

fn vg_triangles(c: VgCluster) -> u32 {
    return c.packed & 255u;
}

fn vg_level(c: VgCluster) -> u32 {
    return (c.packed >> 8u) & 63u;
}

// Source vertices, 8 words each (render/vgeo/mod.rs `pack_vertex`): position (3 × f32),
// normal and tangent (octahedral 2 × snorm16, the bitangent sign in the tangent's lowest bit),
// uv (2 × f32), lightmap uv (2 × unorm16).
@group(3) @binding(0) var<storage, read> vg_vertices: array<u32>;
// Every cluster's triangles as 16-bit indices into its own vertex list, two per word (also
// the index buffer of the indexed path).
@group(3) @binding(1) var<storage, read> vg_local: array<u32>;
@group(3) @binding(2) var<storage, read> vg_clusters: array<VgCluster>;
// (draw, cluster) per drawn cluster, written by vgeo_cull.wgsl.
@group(3) @binding(3) var<storage, read> vg_visible: array<vec2<u32>>;
// Per cluster, its vertices' source ids (indexed path).
@group(3) @binding(4) var<storage, read> vg_cluster_vertices: array<u32>;

const VG_STRIDE: u32 = 8u;

struct VgVertex {
    draw: u32,
    cluster: u32,
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    tangent: vec4<f32>,
    uv2: vec2<f32>,
};

// Local index `i` (16 bits) of the pool.
fn vg_local_index(i: u32) -> u32 {
    return (vg_local[i >> 1u] >> ((i & 1u) * 16u)) & 0xffffu;
}

// Source vertex of corner `corner` (0 .. 3 × triangles) of cluster `c`.
fn vg_corner(c: VgCluster, corner: u32) -> u32 {
    return vg_cluster_vertices[c.vertex_first + vg_local_index(c.local_first + corner)];
}

fn vg_fetch(vertex: u32, instance: u32) -> VgVertex {
    let item = vg_visible[instance];
    let c = vg_clusters[item.y];
    let corner = min(vertex, vg_triangles(c) * 3u - 1u);
    return vg_vertex(vg_corner(c, corner), item);
}

fn vg_fetch_indexed(vertex: u32, instance: u32) -> VgVertex {
    let item = vg_visible[instance];
    let c = vg_clusters[item.y];
    let local = vertex - instance * 384u;
    return vg_vertex(vg_cluster_vertices[c.vertex_first + local], item);
}

// A direction from its octahedral encoding (render/vgeo/mod.rs `octahedral`).
fn vg_octahedral(w: u32) -> vec3<f32> {
    let e = unpack2x16snorm(w);
    var n = vec3<f32>(e.x, e.y, 1.0 - abs(e.x) - abs(e.y));
    let t = max(-n.z, 0.0);
    n.x += select(t, -t, n.x >= 0.0);
    n.y += select(t, -t, n.y >= 0.0);
    return normalize(n);
}

fn vg_vertex_position(source: u32) -> vec3<f32> {
    let b = source * VG_STRIDE;
    return vec3<f32>(
        bitcast<f32>(vg_vertices[b]),
        bitcast<f32>(vg_vertices[b + 1u]),
        bitcast<f32>(vg_vertices[b + 2u]),
    );
}

fn vg_vertex(source: u32, item: vec2<u32>) -> VgVertex {
    var out: VgVertex;
    out.draw = item.x;
    out.cluster = item.y;
    let b = source * VG_STRIDE;
    out.position = vg_vertex_position(source);
    out.normal = vg_octahedral(vg_vertices[b + 3u]);
    let t = vg_vertices[b + 4u];
    out.tangent = vec4<f32>(vg_octahedral(t), select(1.0, -1.0, (t & 1u) != 0u));
    out.uv = vec2<f32>(bitcast<f32>(vg_vertices[b + 5u]), bitcast<f32>(vg_vertices[b + 6u]));
    out.uv2 = unpack2x16unorm(vg_vertices[b + 7u]);
    return out;
}
