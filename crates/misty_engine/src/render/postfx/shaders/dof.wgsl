// Depth of Field (render/postfx/dof.rs). A circle of confusion (CoC) per pixel from the depth:
// Gaussian blurs what lies past a distance range, Bokeh follows a thin lens (focus distance,
// focal length, f-number). Both work on a half-resolution copy:
//   prefilter   output -> half: colour + signed CoC in half-res pixels (negative = near).
//   Bokeh:      tiles (largest near / far CoC per 8×8 half-res texels) -> dilate (the near
//               CoC spread to neighbouring tiles, so a blurred foreground spills over the
//               sharp background behind it) -> gather: rings of samples inside a polygonal
//               aperture; the far field takes samples whose blur reaches the pixel up to the
//               pixel's own CoC (a sharp foreground stays sharp in front of a blurred
//               background), the near field takes every foreground sample whose blur reaches
//               it, with its coverage.
//   Gaussian:   separable CoC-aware Gaussian (horizontal, vertical).
//   composite   full resolution: sharp -> far by the pixel's CoC, then the near field over it.
// Runs after the temporal resolve, so it sees a converged, jitter-free image.

// Signed CoC radius in output pixels of a surface at eye distance `eye`.
fn dof_coc(eye: f32) -> f32 {
    let r = fx.dof.w;
    if (fx.dof.x > 1.5) {
        // Thin lens: c = A·F·(d − P) / (d·(P − F)) = scale · (1 − P / d).
        return clamp(fx.dof.z * (1.0 - fx.dof.y / max(eye, 1e-4)), -r, r);
    }
    return clamp((eye - fx.dof.y) / max(fx.dof.z - fx.dof.y, 1e-4), 0.0, 1.0) * r;
}

fn dof_coc_at(uv: vec2<f32>) -> f32 {
    return dof_coc(fx_eye_depth_at(uv));
}

// Output -> half resolution: the 2×2 output pixels' average colour; the CoC of the nearest
// near-field one when any is near (foreground edges keep their spill), else their average.
@fragment
fn fs_dof_prefilter(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<f32>(floor(in.pos.xy)) * 2.0;
    var color = vec3<f32>(0.0);
    var nearest = 0.0;
    var sum = 0.0;
    for (var i = 0u; i < 4u; i++) {
        let o = vec2<f32>(f32(i & 1u), f32(i >> 1u));
        let uv = (p + o + 0.5) / fx.size.xy;
        color += textureSampleLevel(fx_src, fx_sampler, uv, 0.0).rgb;
        let coc = dof_coc_at(uv);
        nearest = min(nearest, coc);
        sum += coc;
    }
    let coc = select(sum * 0.25, nearest, nearest < 0.0);
    return vec4<f32>(color * 0.25, coc * 0.5);
}

// Largest near (x) and far (y) CoC of an 8×8 block of the prefiltered image.
@fragment
fn fs_dof_tiles(in: FullscreenOut) -> @location(0) vec4<f32> {
    let tile = vec2<i32>(in.pos.xy) * 8;
    let last = vec2<i32>(textureDimensions(fx_src)) - vec2<i32>(1);
    var near = 0.0;
    var far = 0.0;
    for (var y = 0; y < 8; y++) {
        for (var x = 0; x < 8; x++) {
            let coc = textureLoad(fx_src, min(tile + vec2<i32>(x, y), last), 0).a;
            near = max(near, -coc);
            far = max(far, coc);
        }
    }
    return vec4<f32>(near, far, 0.0, 1.0);
}

// The tiles' near CoC spread over their neighbours (as far as the largest CoC reaches).
@fragment
fn fs_dof_dilate(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    let last = vec2<i32>(textureDimensions(fx_src)) - vec2<i32>(1);
    let reach = i32(fx.dof_tiles.x);
    var near = 0.0;
    for (var y = -reach; y <= reach; y++) {
        for (var x = -reach; x <= reach; x++) {
            near = max(near, textureLoad(fx_src, clamp(p + vec2<i32>(x, y), vec2<i32>(0), last), 0).x);
        }
    }
    let own = textureLoad(fx_src, p, 0);
    return vec4<f32>(near, own.y, 0.0, 1.0);
}

// Radius (0..1) of the aperture polygon in direction `angle`: `blades` straight sides,
// rounded towards a circle by the curvature.
fn dof_aperture(angle: f32) -> f32 {
    let blades = fx.dof_shape.x;
    if (blades < 2.5) {
        return 1.0;
    }
    let side = 6.2831853 / blades;
    let a = angle - fx.dof_shape.z;
    let local = a - side * floor(a / side) - side * 0.5;
    let polygon = cos(side * 0.5) / cos(local);
    return mix(polygon, 1.0, fx.dof_shape.y);
}

struct DofFields {
    @location(0) far: vec4<f32>,
    @location(1) near: vec4<f32>,
};

// Bokeh gather at half resolution: far field (rgb, a = 1) and near field (rgb, a = coverage).
@fragment
fn fs_dof_gather(in: FullscreenOut) -> DofFields {
    let p = vec2<i32>(in.pos.xy);
    let dims = vec2<f32>(textureDimensions(fx_src));
    let uv = (vec2<f32>(p) + 0.5) / dims;
    let centre = textureLoad(fx_src, p, 0);
    let tiles = textureLoad(fx_aux0, p / 8, 0);
    let own_far = max(centre.a, 0.0);
    let radius = max(own_far, tiles.x);
    var out: DofFields;
    if (radius < 0.5) {
        out.far = vec4<f32>(centre.rgb, 1.0);
        out.near = vec4<f32>(0.0);
        return out;
    }
    let rings = i32(fx.dof_shape.w);
    // Samples in the disc and the area each stands for.
    let count = f32(1 + 4 * rings * (rings + 1));
    let area = radius * radius / count;
    var far = vec4<f32>(0.0);
    var near = vec4<f32>(0.0);
    var coverage = 0.0;
    var largest = 0.0;
    for (var k = 0; k <= rings; k++) {
        let n = max(8 * k, 1);
        let ring = f32(k) / f32(max(rings, 1));
        for (var i = 0; i < n; i++) {
            let angle = (f32(i) + 0.5 * f32(k & 1)) / f32(n) * 6.2831853;
            let offset = vec2<f32>(cos(angle), sin(angle)) * (dof_aperture(angle) * ring * radius);
            let d = length(offset);
            let s = textureSampleLevel(fx_src, fx_sampler, uv + offset / dims, 0.0);
            // Far: background blur reaches the pixel, up to the pixel's own blur.
            let fw = clamp(min(max(s.a, 0.0), own_far) - d + 1.0, 0.0, 1.0);
            far += vec4<f32>(s.rgb * fw, fw);
            // Near: a foreground sample spreads its colour over its whole CoC; its share of
            // this pixel is the gather area per sample over the area it spreads over.
            let nc = max(-s.a, 0.0);
            let nw = clamp(nc - d + 1.0, 0.0, 1.0);
            near += vec4<f32>(s.rgb * nw, nw);
            coverage += nw * min(area / max(nc * nc, 1e-4), 1.0);
            largest = max(largest, nc * step(1e-4, nw));
        }
    }
    out.far = vec4<f32>(select(centre.rgb, far.rgb / far.a, far.a > 1e-4), 1.0);
    // (A near field of less than a pixel's blur stays the sharp image.)
    let alpha = clamp(coverage, 0.0, 1.0) * smoothstep(0.25, 1.0, largest);
    out.near = vec4<f32>(select(vec3<f32>(0.0), near.rgb / near.a, near.a > 1e-4), alpha);
    return out;
}

// Gaussian: one direction of a CoC-aware Gaussian (taps only count where their own blur
// reaches the pixel; the kernel is the pixel's own CoC). a = the pixel's CoC, kept.
fn dof_blur(p: vec2<i32>, dir: vec2<f32>) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(fx_src));
    let uv = (vec2<f32>(p) + 0.5) / dims;
    let centre = textureLoad(fx_src, p, 0);
    let radius = max(centre.a, 0.0);
    if (radius < 0.5) {
        return centre;
    }
    let taps = i32(fx.dof_shape.w);
    let sigma = radius * 0.5;
    var sum = vec4<f32>(centre.rgb, 1.0);
    for (var i = -taps; i <= taps; i++) {
        if (i == 0) {
            continue;
        }
        let d = f32(i) / f32(taps) * radius;
        let s = textureSampleLevel(fx_src, fx_sampler, uv + dir * d / dims, 0.0);
        let w = exp(-d * d / (2.0 * sigma * sigma)) * clamp(max(s.a, 0.0) - abs(d) + 1.0, 0.0, 1.0);
        sum += vec4<f32>(s.rgb * w, w);
    }
    return vec4<f32>(sum.rgb / sum.a, centre.a);
}

@fragment
fn fs_dof_blur_h(in: FullscreenOut) -> @location(0) vec4<f32> {
    return dof_blur(vec2<i32>(in.pos.xy), vec2<f32>(1.0, 0.0));
}

@fragment
fn fs_dof_blur_v(in: FullscreenOut) -> @location(0) vec4<f32> {
    let c = dof_blur(vec2<i32>(in.pos.xy), vec2<f32>(0.0, 1.0));
    return vec4<f32>(c.rgb, 1.0);
}

// Full resolution: the sharp image, the far field by the pixel's own CoC, the near field by
// its coverage. aux0 = far field, aux1 = near field (black for Gaussian).
@fragment
fn fs_dof_composite(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    let uv = (vec2<f32>(p) + 0.5) / fx.size.xy;
    let sharp = textureLoad(fx_src, p, 0).rgb;
    let coc = dof_coc_at(uv);
    let far = textureSampleLevel(fx_aux0, fx_sampler, uv, 0.0).rgb;
    let near = textureSampleLevel(fx_aux1, fx_sampler, uv, 0.0);
    var c = mix(sharp, far, smoothstep(0.5, 2.0, coc));
    c = mix(c, near.rgb, near.a);
    return vec4<f32>(c, 1.0);
}
