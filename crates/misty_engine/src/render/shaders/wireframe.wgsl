// Wireframe draw modes (render/wireframe.rs), appended to the source of each wire pipeline:
// the scene's own vertex stages rasterise their triangles as edges (polygon mode Line) in the
// colour of `view.wire`, alpha blended in the main pass.

@fragment
fn fs_wire() -> @location(0) vec4<f32> {
    return view.wire;
}

struct WireBackground {
    @location(0) color: vec4<f32>,
    @location(1) glow: vec4<f32>,
};

// The Wireframe mode's flat background, painted over the scene's colour (its depth stays for
// the edges to test against) before the grid and the edges.
@fragment
fn fs_wire_background() -> WireBackground {
    var out: WireBackground;
    out.color = vec4<f32>(view.background.rgb, 1.0);
    out.glow = vec4<f32>(0.0);
    return out;
}
