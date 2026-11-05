// Shadows: appended to mesh.wgsl (uses its `lights`, `shadow_map`, `local_shadow_map` and
// `shadow_sampler`). The sun uses up to four cascades blended across their borders; spot and
// point lights use one perspective layer per face, rectangle lights a hemicube in their own
// frame (or one wide map with soft shadows off).
// Hard = one bilinear compare. Soft = percentage-closer soft shadows: a blocker search, a
// penumbra estimate from the light's size and a filter of that size. Local lights project
// their real shape through the receiver (a sphere's disk, a rectangle's area), so a long
// rectangle softens its shadows more across its long side. Shadows stay sharp where a
// caster touches the receiver and widen with the distance between them.
// The render/shadow_denoise compute pass runs the same functions with a pattern that moves
// every frame and accumulates the result; `shadow_mask_lookup` then hands the lit shaders
// that smooth value instead (it returns -1 where it has none).

struct ShadowParams {
    cascades: array<mat4x4<f32>, 4>,
    // x = far split (view depth), y = texel size (world), z = depth range (world), w = map width (world).
    cascade_info: array<vec4<f32>, 4>,
    // x = cascade count, y = blend fraction, z = blocker samples, w = filter samples.
    settings: vec4<f32>,
    // xyz = camera forward, w = shadow distance.
    view_axis: vec4<f32>,
    // xy = target size (pixels), z = 1 to tint the cascades, w = camera near.
    misc: vec4<f32>,
    // x = widest filter radius (texels), y = narrowest, z = extra depth bias per metre of
    // filter radius (curved receivers under wide kernels).
    filtering: vec4<f32>,
    // Per light: [size, depth bias, normal bias, -], [near, far, tan(half fov), faces].
    // Size = tan(half angular diameter) for the sun, the source radius (m) for spot / point
    // lights and the softness multiplier for rectangles (their half axes give the shape).
    // Faces = shadow-map layers of a rectangle light (1 = one wide map, 5 / 6 = hemicube).
    light_params: array<vec4<f32>, 16>,
};

@group(1) @binding(6) var<uniform> shadow_params: ShadowParams;

const GOLDEN_ANGLE: f32 = 2.39996323;
// Blockers further above the receiver than this (m) are not searched for (sun).
const MAX_BLOCKER_DISTANCE: f32 = 60.0;

// Temporal shadow mask (render/shadow_denoise): the frame number (0 = the fixed pattern the
// lit shaders use), the radial offset of the samples, and the world radius of the last
// filter at the receiver (how far the screen-space denoiser may blur).
var<private> shadow_frame: u32 = 0u;
var<private> shadow_radial: f32 = 0.5;
var<private> shadow_radius_out: f32 = 0.0;

// Shadow-map coordinates (uv, depth) of a world point, or depth > 1 when outside.
fn shadow_coords(m: mat4x4<f32>, p: vec3<f32>) -> vec3<f32> {
    let c = m * vec4<f32>(p, 1.0);
    let ndc = c.xyz / c.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (c.w <= 0.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || ndc.z < 0.0) {
        return vec3<f32>(0.0, 0.0, 2.0);
    }
    return vec3<f32>(uv, ndc.z);
}

// Shadow-map uv of a world point, unclipped.
fn shadow_uv(m: mat4x4<f32>, p: vec3<f32>) -> vec2<f32> {
    let c = m * vec4<f32>(p, 1.0);
    let ndc = c.xy / max(c.w, 1e-6);
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

fn interleaved_gradient(px: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
}

// Interleaved gradient noise of the pixel a world point lands on (rotates the sample disk).
// With a frame number (temporal mask) the pattern moves every frame (Jimenez 2014) and the
// samples' radii are jittered too, so the accumulated frames fill the whole disk.
fn shadow_noise(world: vec3<f32>) -> f32 {
    let clip = view.view_proj * vec4<f32>(world, 1.0);
    let ndc = clip.xy / clip.w;
    let px = floor((ndc * vec2<f32>(0.5, -0.5) + 0.5) * shadow_params.misc.xy);
    if (shadow_frame == 0u) {
        shadow_radial = 0.5;
        return interleaved_gradient(px);
    }
    let f = f32(shadow_frame % 64u);
    shadow_radial = fract(interleaved_gradient(px.yx + vec2<f32>(29.0, 71.0)) + 0.61803399 * f);
    return interleaved_gradient(px + 5.588238 * f);
}

// Sample `i` of `n` on a unit Vogel disk turned by `phi`.
fn vogel(i: u32, n: u32, phi: f32) -> vec2<f32> {
    let r = sqrt((f32(i) + shadow_radial) / f32(n));
    let a = f32(i) * GOLDEN_ANGLE + phi;
    return vec2<f32>(cos(a), sin(a)) * r;
}

// A unit-disk point moved to the [-1, 1]² square, keeping areas (the inverse of Shirley and
// Chiu's concentric map), so rectangle lights are sampled over their whole area.
fn disk_to_square(d: vec2<f32>) -> vec2<f32> {
    let r = length(d);
    if (r < 1e-6) {
        return vec2<f32>(0.0);
    }
    let quarter = 0.78539816;
    // Rotate into the sector around +X, map, rotate back.
    let a = atan2(d.y, d.x);
    let k = round(a / (2.0 * quarter));
    let s = vec2<f32>(r, r * (a - k * 2.0 * quarter) / quarter);
    let c = cos(k * 2.0 * quarter);
    let n = sin(k * 2.0 * quarter);
    return vec2<f32>(s.x * c - s.y * n, s.x * n + s.y * c);
}

// Sample `i` of `n` over the source's shape (disk or square), mapped to shadow-map uv by `k`.
fn shape_sample(i: u32, n: u32, phi: f32, k: mat2x2<f32>, square: bool) -> vec2<f32> {
    var s = vogel(i, n, phi);
    if (square) {
        s = disk_to_square(s);
    }
    return k * s;
}

// Kernel axes (uv): the widest kept to `hi`, each at least `lo` long.
fn kernel_axes(a0: vec2<f32>, b0: vec2<f32>, lo: f32, hi: f32) -> mat2x2<f32> {
    var a = a0;
    var b = b0;
    let widest = max(length(a), length(b));
    if (widest > hi) {
        a *= hi / widest;
        b *= hi / widest;
    }
    let la = length(a);
    if (la < lo) {
        a = select(vec2<f32>(lo, 0.0), a * (lo / max(la, 1e-12)), la > 1e-9);
    }
    let lb = length(b);
    if (lb < lo) {
        let side = vec2<f32>(-a.y, a.x) * (lo / length(a));
        b = select(side, b * (lo / max(lb, 1e-12)), lb > 1e-9);
    }
    return mat2x2<f32>(a, b);
}

fn isotropic(r: f32) -> mat2x2<f32> {
    return mat2x2<f32>(vec2<f32>(r, 0.0), vec2<f32>(0.0, r));
}

// Depth change of the receiver's plane per unit of shadow-map uv, so wide kernels compare
// against the plane instead of one depth (no self-shadowing on slopes). Limited at grazing angles.
fn receiver_slope(m: mat4x4<f32>, p: vec3<f32>, n: vec3<f32>, n_dot_l: f32) -> vec2<f32> {
    let c = m * vec4<f32>(p, 1.0);
    let ndc = c.xyz / c.w;
    var t1 = cross(n, vec3<f32>(0.0, 1.0, 0.0));
    if (dot(t1, t1) < 1e-4) {
        t1 = cross(n, vec3<f32>(1.0, 0.0, 0.0));
    }
    t1 = normalize(t1);
    let t2 = cross(n, t1);
    // Derivatives of the projected point along the two tangents.
    let a4 = m * vec4<f32>(t1, 0.0);
    let b4 = m * vec4<f32>(t2, 0.0);
    let a = (a4.xyz - ndc * a4.w) / c.w;
    let b = (b4.xyz - ndc * b4.w) / c.w;
    let det = a.x * b.y - a.y * b.x;
    if (abs(det) < 1e-20) {
        return vec2<f32>(0.0);
    }
    let dzdx = (a.z * b.y - b.z * a.y) / det;
    let dzdy = (b.z * a.x - a.z * b.x) / det;
    // ndc → uv: u = x / 2 + 0.5, v = 0.5 - y / 2.
    let g = vec2<f32>(2.0 * dzdx, -2.0 * dzdy);
    let tan_now = sqrt(max(1.0 - n_dot_l * n_dot_l, 0.0)) / max(n_dot_l, 1e-4);
    return g * min(1.0, 8.0 / max(tan_now, 1e-4));
}

// Average depth of the texels closer to the light than the receiver over the kernel `k`
// (uv), and how many there were.
fn find_blockers(map: texture_depth_2d_array, uv: vec2<f32>, layer: i32, z: f32, slope: vec2<f32>, k: mat2x2<f32>, square: bool, phi: f32) -> vec2<f32> {
    let size = vec2<f32>(textureDimensions(map));
    let n = u32(shadow_params.settings.z);
    var sum = 0.0;
    var count = 0.0;
    for (var i = 0u; i < n; i++) {
        let o = shape_sample(i, n, phi, k, square);
        let t = vec2<i32>(clamp((uv + o) * size, vec2<f32>(0.0), size - 1.0));
        let d = textureLoad(map, t, layer, 0);
        if (d < z + dot(slope, o)) {
            sum += d;
            count += 1.0;
        }
    }
    return vec2<f32>(sum / max(count, 1.0), count);
}

// Lit fraction over the kernel `k` (uv): bilinear compares on the receiver's plane. Samples
// further from the receiver's own point get a little more bias (`bias_per_uv` = depth per uv
// of distance), so wide kernels don't shadow curved receivers.
fn filter_kernel(map: texture_depth_2d_array, uv: vec2<f32>, layer: i32, z: f32, slope: vec2<f32>, k: mat2x2<f32>, square: bool, phi: f32, bias_per_uv: f32) -> f32 {
    let n = u32(shadow_params.settings.w);
    var lit = 0.0;
    for (var i = 0u; i < n; i++) {
        let o = shape_sample(i, n, phi, k, square);
        let zc = z + dot(slope, o) - bias_per_uv * length(o);
        lit += textureSampleCompareLevel(map, shadow_sampler, uv + o, layer, zc);
    }
    return lit / f32(n);
}

// Sun shadow from cascade `c` (-1 when the point is outside it).
fn sun_cascade(c: u32, light: u32, world: vec3<f32>, n: vec3<f32>, n_dot_l: f32, mode: f32, phi: f32) -> f32 {
    let info = shadow_params.cascade_info[c];
    let lp = shadow_params.light_params[light * 2u];
    let m = shadow_params.cascades[c];
    // Normal offset: about `normal bias` texels, more on slopes.
    let p = world + n * info.y * lp.z * (1.0 + 2.0 * (1.0 - n_dot_l));
    let sc = shadow_coords(m, p);
    if (sc.z > 1.0) {
        return -1.0;
    }
    let layer = i32(c);
    let z = sc.z - lp.y * info.y / info.z;
    if (mode < 1.5) {
        return textureSampleCompareLevel(shadow_map, shadow_sampler, sc.xy, layer, z);
    }
    let texel = 1.0 / f32(textureDimensions(shadow_map).x);
    let widest = shadow_params.filtering.x * texel;
    let narrowest = shadow_params.filtering.y * texel;
    let slope = receiver_slope(m, p, n, n_dot_l);
    // Blockers up to MAX_BLOCKER_DISTANCE above the receiver can reach this far.
    let reach = lp.x * min(z * info.z, MAX_BLOCKER_DISTANCE) / info.w;
    let search = clamp(reach, 2.0 * texel, widest);
    let blockers = find_blockers(shadow_map, sc.xy, layer, z, slope, isotropic(search), false, phi);
    if (blockers.y < 0.5) {
        return 1.0;
    }
    // Penumbra radius = blocker-to-receiver distance × tan(half the sun's angle).
    let distance = max(z - blockers.x, 0.0) * info.z;
    let radius = clamp(distance * lp.x / info.w, narrowest, widest);
    shadow_radius_out = max(shadow_radius_out, radius * info.w);
    let bias_per_uv = shadow_params.filtering.z * info.w / info.z;
    return filter_kernel(shadow_map, sc.xy, layer, z, slope, isotropic(radius), false, phi, bias_per_uv);
}

// View depth of a world point (the axis the cascades are split along).
fn cascade_depth(world: vec3<f32>) -> f32 {
    return dot(world - view.camera_pos.xyz, shadow_params.view_axis.xyz);
}

// Cascade a view depth falls in, and how far into the blend band towards the next (0..1).
fn cascade_at(depth: f32) -> vec2<f32> {
    let count = u32(shadow_params.settings.x);
    var c = 0u;
    while (c + 1u < count && depth > shadow_params.cascade_info[c].x) {
        c++;
    }
    var start = shadow_params.misc.w;
    if (c > 0u) {
        start = shadow_params.cascade_info[c - 1u].x;
    }
    let end = shadow_params.cascade_info[c].x;
    let band = shadow_params.settings.y * (end - start);
    var t = 0.0;
    if (c + 1u < count && band > 0.0) {
        t = clamp((depth - (end - band)) / band, 0.0, 1.0);
    }
    return vec2<f32>(f32(c), t);
}

fn sun_shadow(light: u32, world: vec3<f32>, n: vec3<f32>, n_dot_l: f32, mode: f32) -> f32 {
    let count = u32(shadow_params.settings.x);
    let depth = cascade_depth(world);
    let distance = shadow_params.view_axis.w;
    if (count == 0u || depth > distance) {
        return 1.0;
    }
    let phi = shadow_noise(world) * 6.2831853;
    let at = cascade_at(depth);
    var c = u32(at.x);
    var s = sun_cascade(c, light, world, n, n_dot_l, mode, phi);
    // Outside its cascade (e.g. an orthographic view): the next ones are larger.
    while (s < 0.0 && c + 1u < count) {
        c++;
        s = sun_cascade(c, light, world, n, n_dot_l, mode, phi);
    }
    if (s < 0.0) {
        return 1.0;
    }
    if (at.y > 0.0 && c == u32(at.x)) {
        let next = sun_cascade(c + 1u, light, world, n, n_dot_l, mode, phi);
        if (next >= 0.0) {
            s = mix(s, next, at.y);
        }
    }
    // Fade out over the last 10% of the shadow distance.
    let fade = clamp((distance - depth) / (0.1 * distance), 0.0, 1.0);
    return mix(1.0, s, fade);
}

// View depth of a perspective shadow-map depth.
fn linear_depth(z: f32, near: f32, far: f32) -> f32 {
    return near * far / (far - z * (far - near));
}

// Cube face (0..5: +X, -X, +Y, -Y, +Z, -Z) of a direction given in the cube's own axes.
fn cube_face(q: vec3<f32>) -> i32 {
    let a = abs(q);
    if (a.x >= a.y && a.x >= a.z) {
        return select(1, 0, q.x > 0.0);
    }
    if (a.y >= a.z) {
        return select(3, 2, q.y > 0.0);
    }
    return select(5, 4, q.z > 0.0);
}

fn local_shadow(light: u32, li: LightItem, world: vec3<f32>, n: vec3<f32>, n_dot_l: f32, mode: f32) -> f32 {
    var layer = i32(li.shadow.y);
    if (layer < 0) {
        return 1.0;
    }
    let kind = u32(li.dir_type.w);
    let lp = shadow_params.light_params[light * 2u];
    let clip = shadow_params.light_params[light * 2u + 1u];
    let to_point = world - li.pos_range.xyz;
    if (kind == 1u) {
        // Point: pick the cube face by the major axis.
        layer += cube_face(to_point);
    } else if (kind == 3u && clip.w > 1.5) {
        // Rectangle hemicube: faces along its own right, up and forward axes.
        if (length(li.area_x.xyz) < 1e-6 || length(li.area_y.xyz) < 1e-6) {
            return 1.0;
        }
        let r = normalize(li.area_x.xyz);
        let u = normalize(li.area_y.xyz);
        let f = normalize(li.dir_type.xyz);
        let face = cube_face(vec3<f32>(dot(to_point, r), dot(to_point, u), dot(to_point, f)));
        if (face == 5 && clip.w < 5.5) {
            // Behind a one-sided rectangle: no light to shadow.
            return 1.0;
        }
        // Layers: forward first (what the fog samples), then +right, -right, +up, -up, back.
        layer += select(select(face + 1, 5, face == 5), 0, face == 4);
    }
    let near = clip.x;
    let far = clip.y;
    let tan_half = clip.z;
    let size = f32(textureDimensions(local_shadow_map).x);
    let texel = 1.0 / size;
    // World size of one texel at this distance.
    let texel_world = length(to_point) * 2.0 * tan_half / size;
    let p = world + n * texel_world * lp.z * (1.0 + 2.0 * (1.0 - n_dot_l));
    let m = lights.local_view_proj[layer];
    let sc = shadow_coords(m, p);
    if (sc.z > 1.0) {
        return 1.0;
    }
    let d = linear_depth(sc.z, near, far);
    // Depth (map units) of one metre at this distance.
    let depth_per_metre = near * far / ((far - near) * d * d);
    // `bias` texels of depth, converted to the map's non-linear depth.
    let z = sc.z - lp.y * texel_world * depth_per_metre;
    if (mode < 1.5) {
        return textureSampleCompareLevel(local_shadow_map, shadow_sampler, sc.xy, layer, z);
    }
    let phi = shadow_noise(world) * 6.2831853;
    let slope = receiver_slope(m, p, n, n_dot_l);
    // The source's half axes: a rectangle's own, else a disk of the source radius facing
    // the receiver.
    var a1: vec3<f32>;
    var a2: vec3<f32>;
    let square = kind == 3u;
    if (square) {
        a1 = li.area_x.xyz * lp.x;
        a2 = li.area_y.xyz * lp.x;
    } else {
        let w = normalize(to_point);
        var t = cross(w, vec3<f32>(0.0, 1.0, 0.0));
        if (dot(t, t) < 1e-4) {
            t = cross(w, vec3<f32>(1.0, 0.0, 0.0));
        }
        t = normalize(t);
        a1 = t * lp.x;
        a2 = cross(w, t) * lp.x;
    }
    // Through the receiver, a source point at `s` (shape units) lines up with the blocker
    // texel at uv(p + s · axes · (d - d_blocker) / d_blocker): `j1`, `j2` = the uv of each
    // axis at the receiver (linearised around p).
    let base = shadow_uv(m, p);
    let j1 = (shadow_uv(m, p + a1 * 0.01) - base) * 100.0;
    let j2 = (shadow_uv(m, p + a2 * 0.01) - base) * 100.0;
    let widest = shadow_params.filtering.x * texel;
    let narrowest = shadow_params.filtering.y * texel;
    // Blockers down to a third of the way to the light: (d - d / 3) / (d / 3) = 2.
    let search = kernel_axes(j1 * 2.0, j2 * 2.0, 2.0 * texel, widest);
    let blockers = find_blockers(local_shadow_map, sc.xy, layer, z, slope, search, square, phi);
    if (blockers.y < 0.5) {
        return 1.0;
    }
    // Similar triangles: penumbra = source size × (receiver - blocker) / blocker.
    let db = linear_depth(blockers.x, near, far);
    let ratio = max(d - db, 0.0) / max(db, 1e-3);
    let kernel = kernel_axes(j1 * ratio, j2 * ratio, narrowest, widest);
    let uv_per_metre = 1.0 / (2.0 * tan_half * d);
    let radius_uv = max(length(kernel[0]), length(kernel[1]));
    shadow_radius_out = max(shadow_radius_out, radius_uv / uv_per_metre);
    let bias_per_uv = shadow_params.filtering.z * depth_per_metre / uv_per_metre;
    return filter_kernel(local_shadow_map, sc.xy, layer, z, slope, kernel, square, phi, bias_per_uv);
}

// Visibility of light `i` (0 = fully shadowed): the temporal mask's smooth value when it
// covers this light and surface, else the shadow maps directly.
fn shadow_factor(i: u32, li: LightItem, world: vec3<f32>, n: vec3<f32>, l: vec3<f32>) -> f32 {
    let mode = li.shadow.x;
    if (mode < 0.5) {
        return 1.0;
    }
    let masked = shadow_mask_lookup(i, world);
    if (masked >= 0.0) {
        return masked;
    }
    return shadow_direct(i, li, world, n, l);
}

// Visibility of light `i` straight from the shadow maps.
fn shadow_direct(i: u32, li: LightItem, world: vec3<f32>, n: vec3<f32>, l: vec3<f32>) -> f32 {
    let mode = li.shadow.x;
    if (mode < 0.5) {
        return 1.0;
    }
    let n_dot_l = max(dot(n, l), 0.0);
    if (u32(li.dir_type.w) == 0u) {
        return sun_shadow(i, world, n, n_dot_l, mode);
    }
    return local_shadow(i, li, world, n, n_dot_l, mode);
}

// Visibility of light `i` from one hardware-filtered tap, whatever the light's mode.
fn fast_shadow_factor(i: u32, li: LightItem, world: vec3<f32>, n: vec3<f32>, l: vec3<f32>) -> f32 {
    if (li.shadow.x < 0.5) {
        return 1.0;
    }
    let n_dot_l = max(dot(n, l), 0.0);
    if (u32(li.dir_type.w) == 0u) {
        // Four hardware-filtered taps spread across the light (a soft, stable edge).
        var t = normalize(cross(l, vec3<f32>(0.0, 1.0, 0.0) + vec3<f32>(1e-3, 0.0, 0.0)));
        let b = cross(l, t);
        let r = 0.12 * (1.0 + 0.02 * length(world - view.camera_pos.xyz));
        var sum = 0.0;
        sum += sun_shadow(i, world + (t + b * 0.5) * r, n, n_dot_l, 1.0);
        sum += sun_shadow(i, world + (-t - b * 0.5) * r, n, n_dot_l, 1.0);
        sum += sun_shadow(i, world + (b - t * 0.5) * r, n, n_dot_l, 1.0);
        sum += sun_shadow(i, world + (-b + t * 0.5) * r, n, n_dot_l, 1.0);
        return sum * 0.25;
    }
    return local_shadow(i, li, world, n, n_dot_l, 1.0);
}

// Scene view ▸ Shadow Cascades: colour each cascade (blended across the bands).
fn cascade_debug(world: vec3<f32>, color: vec3<f32>) -> vec3<f32> {
    let count = u32(shadow_params.settings.x);
    let depth = cascade_depth(world);
    if (shadow_params.misc.z < 0.5 || count == 0u || depth > shadow_params.view_axis.w) {
        return color;
    }
    var tints = array<vec3<f32>, 4>(
        vec3<f32>(1.0, 0.25, 0.25),
        vec3<f32>(0.25, 1.0, 0.3),
        vec3<f32>(0.3, 0.45, 1.0),
        vec3<f32>(1.0, 0.85, 0.2),
    );
    let at = cascade_at(depth);
    let c = u32(at.x);
    var tint = tints[c];
    if (at.y > 0.0) {
        tint = mix(tint, tints[min(c + 1u, 3u)], at.y);
    }
    let luma = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    return tint * (0.15 + 0.85 * min(luma, 1.5));
}
