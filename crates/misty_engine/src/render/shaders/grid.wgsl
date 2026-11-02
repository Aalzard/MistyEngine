// Infinite editor grid on one world plane, with 10x level-of-detail blending and distance fade.

struct GridOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// Anti-aliased line intensity for a grid with the given spacing.
fn grid_lines(c: vec2<f32>, spacing: f32) -> f32 {
    let g = c / spacing;
    let w = max(fwidth(g), vec2<f32>(1e-5));
    let l = abs(fract(g - 0.5) - 0.5) / w;
    let line = 1.0 - min(min(l.x, l.y), 1.0);
    // Fade lines that get denser than a pixel to avoid moire.
    return line * (1.0 - smoothstep(0.3, 0.9, max(w.x, w.y)));
}

fn axis_component(v: vec3<f32>, axis: u32) -> f32 {
    if (axis == 0u) { return v.x; }
    if (axis == 1u) { return v.y; }
    return v.z;
}

fn plane_coords(p: vec3<f32>, axis: u32) -> vec2<f32> {
    if (axis == 0u) { return p.zy; }
    if (axis == 1u) { return p.xz; }
    return p.xy;
}

@fragment
fn fs_main(in: FullscreenOut) -> GridOut {
    let axis = u32(view.grid.x);
    let near = unproject(in.ndc, 0.0);
    let far = unproject(in.ndc, 1.0);
    let dir = far - near;

    let denom = axis_component(dir, axis);
    let t = -axis_component(near, axis) / select(denom, 1e-6, abs(denom) < 1e-6);
    let p = near + dir * t;
    let coords = plane_coords(p, axis);

    // LOD: blend between 10^n and 10^(n+1) spacing based on camera height (or ortho size).
    let lod = log2(max(view.grid.w * 0.15, 1.0)) / log2(10.0);
    let base = pow(10.0, floor(lod));
    let blend = fract(lod);
    let fine = grid_lines(coords, base) * (1.0 - blend);
    let coarse = grid_lines(coords, base * 10.0);
    var alpha = max(fine * 0.45, coarse * 0.7);

    // Fade with distance and at grazing angles.
    let dist = length(p - view.camera_pos.xyz);
    alpha *= 1.0 - smoothstep(view.grid.y * 0.3, view.grid.y, dist);
    let facing = abs(axis_component(normalize(dir), axis));
    alpha *= smoothstep(0.0, 0.12, facing);

    let valid = t > 0.0 && t < 1.0 && view.grid.z > 0.5;
    let clip = view.view_proj * vec4<f32>(p, 1.0);

    var out: GridOut;
    out.color = vec4<f32>(srgb_to_linear(vec3<f32>(0.55)), select(0.0, alpha, valid));
    out.depth = select(1.0, clamp(clip.z / clip.w, 0.0, 1.0), valid);
    return out;
}
