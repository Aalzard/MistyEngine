// Wireframe without polygon mode Line (render/wireframe.rs): filled triangles that keep only
// the pixels within about half a pixel of an edge (each edge is drawn by the triangles on both
// of its sides, about a pixel wide in all). Needs SHADER_BARYCENTRICS.

@fragment
fn fs_wire_bary(@builtin(barycentric) b: vec3<f32>) -> @location(0) vec4<f32> {
    // Distance to each edge in pixels.
    let px = b / max(fwidth(b), vec3<f32>(1e-6));
    if (min(px.x, min(px.y, px.z)) > 0.5) {
        discard;
    }
    return wire_out();
}
