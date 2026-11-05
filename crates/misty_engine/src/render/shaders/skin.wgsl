// Skinned Mesh Renderer deformation: blend shapes, then linear blend skinning with up to four
// bones per vertex. Reads the bind-pose vertices and writes posed ones in the same layout
// (position, normal, uv, tangent, lightmap uv: 14 floats), which every pass then draws as a vertex buffer.

struct Params {
    vertex_count: u32,
    joint_count: u32,
    shape_count: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> src: array<f32>;
// Two entries per vertex: joint indices, then the weights' bits.
@group(0) @binding(2) var<storage, read> influences: array<vec4<u32>>;
// [shape][vertex]: position offset xyz, normal offset xyz.
@group(0) @binding(3) var<storage, read> deltas: array<f32>;
@group(0) @binding(4) var<storage, read> shape_weights: array<f32>;
@group(0) @binding(5) var<storage, read> joints: array<mat4x4<f32>>;
@group(0) @binding(6) var<storage, read_write> dst: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let v = id.x;
    if v >= params.vertex_count {
        return;
    }
    let b = v * 14u;
    var p = vec3<f32>(src[b], src[b + 1u], src[b + 2u]);
    var n = vec3<f32>(src[b + 3u], src[b + 4u], src[b + 5u]);
    let uv = vec2<f32>(src[b + 6u], src[b + 7u]);
    var t = vec3<f32>(src[b + 8u], src[b + 9u], src[b + 10u]);
    let tw = src[b + 11u];

    for (var s = 0u; s < params.shape_count; s++) {
        let w = shape_weights[s];
        if w != 0.0 {
            let d = (s * params.vertex_count + v) * 6u;
            p += w * vec3<f32>(deltas[d], deltas[d + 1u], deltas[d + 2u]);
            n += w * vec3<f32>(deltas[d + 3u], deltas[d + 4u], deltas[d + 5u]);
        }
    }

    if params.joint_count > 0u {
        let last = params.joint_count - 1u;
        let j = influences[v * 2u];
        let w = bitcast<vec4<f32>>(influences[v * 2u + 1u]);
        let m = joints[min(j.x, last)] * w.x + joints[min(j.y, last)] * w.y
            + joints[min(j.z, last)] * w.z + joints[min(j.w, last)] * w.w;
        p = (m * vec4<f32>(p, 1.0)).xyz;
        n = (m * vec4<f32>(n, 0.0)).xyz;
        t = (m * vec4<f32>(t, 0.0)).xyz;
    }
    if dot(n, n) > 1e-12 {
        n = normalize(n);
    } else {
        n = vec3<f32>(0.0, 1.0, 0.0);
    }
    if dot(t, t) > 1e-12 {
        t = normalize(t);
    } else {
        t = vec3<f32>(1.0, 0.0, 0.0);
    }

    dst[b] = p.x;
    dst[b + 1u] = p.y;
    dst[b + 2u] = p.z;
    dst[b + 3u] = n.x;
    dst[b + 4u] = n.y;
    dst[b + 5u] = n.z;
    dst[b + 6u] = uv.x;
    dst[b + 7u] = uv.y;
    dst[b + 8u] = t.x;
    dst[b + 9u] = t.y;
    dst[b + 10u] = t.z;
    dst[b + 11u] = tw;
    // Lightmap uv, unchanged.
    dst[b + 12u] = src[b + 12u];
    dst[b + 13u] = src[b + 13u];
}
