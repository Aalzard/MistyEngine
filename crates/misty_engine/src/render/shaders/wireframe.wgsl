// Wireframe draw modes (render/wireframe.rs), appended to the source of each wire pipeline:
// the scene's own vertex stages rasterise their triangles as edges (polygon mode Line). The
// edges' opacity is `view.view_tan.w` (0 outside the wireframe modes); their colour is the
// pass's blend constant (colour = constant × opacity + scene × (1 − opacity)), so the shared
// prelude and the view uniform stay as they are.

fn wire_out() -> vec4<f32> {
    return vec4<f32>(view.view_tan.w);
}

@fragment
fn fs_wire() -> @location(0) vec4<f32> {
    return wire_out();
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
