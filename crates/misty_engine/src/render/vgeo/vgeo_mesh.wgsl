// Misty Virtual Geometry in the main pass: the visible clusters, lit by mesh.wgsl's fs_main
// (after MESH_SRC and vgeo_fetch.wgsl).

@vertex
fn vs_cluster(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> MeshOut {
    return vg_mesh_out(vg_fetch(vertex, instance));
}

@vertex
fn vs_cluster_indexed(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> MeshOut {
    return vg_mesh_out(vg_fetch_indexed(vertex, instance));
}

fn vg_mesh_out(v: VgVertex) -> MeshOut {
    let d = draws[v.draw];
    let world = d.model * vec4<f32>(v.position, 1.0);
    var out: MeshOut;
    out.clip = view.view_proj * world;
    out.world = world.xyz;
    out.normal = (d.normal * vec4<f32>(v.normal, 0.0)).xyz;
    out.uv = draw_uv(d, v.uv);
    out.draw = v.draw;
    out.tangent = vec4<f32>((d.model * vec4<f32>(v.tangent.xyz, 0.0)).xyz, v.tangent.w * d.emission.w);
    out.lightmap_uv = v.uv2 * d.lightmap_st.xy + d.lightmap_st.zw;
    out.clip_prev = prev_clip((d.prev_model * vec4<f32>(v.position, 1.0)).xyz);
    return out;
}

// Debug views (Scene view draw modes, render/vgeo/mod.rs `VgeoDebug`).
struct VgDebugInfo {
    // x = visible-list entries per main view, y = entries per batch, z = debug view
    // (1 clusters, 2 level of detail, 3 overdraw, 4 material slots).
    regions: vec4<u32>,
};
@group(3) @binding(5) var<uniform> vg_debug_info: VgDebugInfo;

struct VgDebugOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) color: vec3<f32>,
};

fn vg_hash_color(i: u32) -> vec3<f32> {
    var h = i * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h = (h >> 22u) ^ h;
    let c = vec3<f32>(f32(h & 255u), f32((h >> 8u) & 255u), f32((h >> 16u) & 255u)) / 255.0;
    return 0.15 + 0.85 * c;
}

// Level 0 (source triangles) blue, then green, yellow, orange, red, magenta for coarser ones.
fn vg_level_color(level: u32) -> vec3<f32> {
    let ramp = array<vec3<f32>, 7>(
        vec3<f32>(0.15, 0.35, 1.0),
        vec3<f32>(0.1, 0.8, 0.9),
        vec3<f32>(0.2, 0.85, 0.2),
        vec3<f32>(0.95, 0.9, 0.15),
        vec3<f32>(1.0, 0.55, 0.1),
        vec3<f32>(0.95, 0.15, 0.1),
        vec3<f32>(0.85, 0.2, 0.9),
    );
    return ramp[min(level, 6u)];
}

@vertex
fn vs_cluster_debug(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VgDebugOut {
    return vg_debug_out(vg_fetch(vertex, instance));
}

@vertex
fn vs_cluster_debug_indexed(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VgDebugOut {
    return vg_debug_out(vg_fetch_indexed(vertex, instance));
}

fn vg_debug_out(v: VgVertex) -> VgDebugOut {
    let d = draws[v.draw];
    let world = d.model * vec4<f32>(v.position, 1.0);
    let c = vg_clusters[v.cluster];
    var out: VgDebugOut;
    out.clip = view.view_proj * world;
    out.world = world.xyz;
    switch vg_debug_info.regions.z {
        case 2u: {
            out.color = vg_level_color(vg_level(c));
        }
        case 4u: {
            out.color = vg_hash_color(c.packed >> 14u);
        }
        default: {
            out.color = vg_hash_color(v.cluster);
        }
    }
    return out;
}

@fragment
fn fs_cluster_debug(in: VgDebugOut) -> MeshTargets {
    let n = normalize(cross(dpdy(in.world), dpdx(in.world)));
    let light = 0.35 + 0.65 * abs(dot(n, normalize(vec3<f32>(0.4, 0.8, 0.3))));
    var out: MeshTargets;
    out.color = vec4<f32>(in.color * light, 1.0);
    out.glow = vec4<f32>(0.0);
    out.velocity = vec2<f32>(0.0);
    return out;
}

// Overdraw: every rasterized cluster fragment adds the same small amount (additive blend, no
// depth test), so bright = many layers of triangles.
@fragment
fn fs_cluster_overdraw(in: VgDebugOut) -> MeshTargets {
    var out: MeshTargets;
    out.color = vec4<f32>(0.12, 0.05, 0.015, 1.0);
    out.glow = vec4<f32>(0.0);
    out.velocity = vec2<f32>(0.0);
    return out;
}
