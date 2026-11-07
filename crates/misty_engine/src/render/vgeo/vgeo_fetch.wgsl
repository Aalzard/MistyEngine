// Misty Virtual Geometry: vertex pulling of the visible clusters (render/vgeo/mod.rs).
// Instance i is entry i of the visible list. Two ways to draw:
//   indexed (`vg_fetch_indexed`): one indexed sub-draw per cluster, its local indices into
//     its own vertex list, base vertex = entry × 384: the post-transform vertex cache works.
//   pulled (`vg_fetch`): 384 vertices per cluster, the index read from storage; triangles
//     past a cluster's count collapse onto one vertex (no area, dropped). No vertex reuse.

struct VgCluster {
    bounds: vec4<f32>,
    lod: vec4<f32>,
    parent: vec4<f32>,
    error: f32,
    parent_error: f32,
    first: u32,
    triangles: u32,
    vertex_first: u32,
    local_first: u32,
    pad0: u32,
    pad1: u32,
};

// Source vertices, 14 floats each: position, normal, uv, tangent, uv2 (render/mesh.rs Vertex).
@group(3) @binding(0) var<storage, read> vg_vertices: array<f32>;
@group(3) @binding(1) var<storage, read> vg_indices: array<u32>;
@group(3) @binding(2) var<storage, read> vg_clusters: array<VgCluster>;
// (draw, cluster) per drawn cluster, written by vgeo_cull.wgsl.
@group(3) @binding(3) var<storage, read> vg_visible: array<vec2<u32>>;
// Per cluster, its vertices' source ids (indexed path).
@group(3) @binding(4) var<storage, read> vg_cluster_vertices: array<u32>;

const VG_STRIDE: u32 = 14u;

struct VgVertex {
    draw: u32,
    cluster: u32,
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    tangent: vec4<f32>,
    uv2: vec2<f32>,
};

fn vg_fetch(vertex: u32, instance: u32) -> VgVertex {
    let item = vg_visible[instance];
    let c = vg_clusters[item.y];
    let corner = min(vertex, c.triangles * 3u - 1u);
    return vg_vertex(vg_indices[c.first + corner], item);
}

fn vg_fetch_indexed(vertex: u32, instance: u32) -> VgVertex {
    let item = vg_visible[instance];
    let c = vg_clusters[item.y];
    let local = vertex - instance * 384u;
    return vg_vertex(vg_cluster_vertices[c.vertex_first + local], item);
}

fn vg_vertex(source: u32, item: vec2<u32>) -> VgVertex {
    var out: VgVertex;
    out.draw = item.x;
    out.cluster = item.y;
    let b = source * VG_STRIDE;
    out.position = vec3<f32>(vg_vertices[b], vg_vertices[b + 1u], vg_vertices[b + 2u]);
    out.normal = vec3<f32>(vg_vertices[b + 3u], vg_vertices[b + 4u], vg_vertices[b + 5u]);
    out.uv = vec2<f32>(vg_vertices[b + 6u], vg_vertices[b + 7u]);
    out.tangent = vec4<f32>(
        vg_vertices[b + 8u],
        vg_vertices[b + 9u],
        vg_vertices[b + 10u],
        vg_vertices[b + 11u],
    );
    out.uv2 = vec2<f32>(vg_vertices[b + 12u], vg_vertices[b + 13u]);
    return out;
}
