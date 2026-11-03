// Shadows: appended to mesh.wgsl (uses its `lights`, `shadow_map`, `local_shadow_map` and
// `shadow_sampler`). The sun uses up to four cascades blended across their borders; spot and
// point lights use one perspective layer per face.
// Hard = one bilinear compare. Soft = percentage-closer soft shadows: a blocker search, a
// penumbra estimate from the light's size (sun angle or source radius) and a filter of that
// size on a Vogel disk rotated per pixel. Shadows stay sharp where a caster touches the
// receiver and widen with the distance between them.

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
    // Per light: [size, depth bias, normal bias, -], [near, far, tan(half fov), -].
    // Size = tan(half angular diameter) for the sun, the source radius (m) otherwise.
    light_params: array<vec4<f32>, 16>,
};

@group(1) @binding(6) var<uniform> shadow_params: ShadowParams;

const GOLDEN_ANGLE: f32 = 2.39996323;
// Widest blocker search / filter radius, in texels.
const MAX_FILTER_TEXELS: f32 = 48.0;
// Narrowest soft filter radius, in texels (anti-aliases contact shadows).
const MIN_FILTER_TEXELS: f32 = 1.5;
// Blockers further above the receiver than this (m) are not searched for (sun).
const MAX_BLOCKER_DISTANCE: f32 = 60.0;

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

// Interleaved gradient noise of the pixel a world point lands on (rotates the sample disk).
fn shadow_noise(world: vec3<f32>) -> f32 {
    let clip = view.view_proj * vec4<f32>(world, 1.0);
    let ndc = clip.xy / clip.w;
    let px = floor((ndc * vec2<f32>(0.5, -0.5) + 0.5) * shadow_params.misc.xy);
    return fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
}

// Sample `i` of `n` on a unit Vogel disk turned by `phi`.
fn vogel(i: u32, n: u32, phi: f32) -> vec2<f32> {
    let r = sqrt((f32(i) + 0.5) / f32(n));
    let a = f32(i) * GOLDEN_ANGLE + phi;
    return vec2<f32>(cos(a), sin(a)) * r;
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

// Average depth of the texels closer to the light than the receiver within `radius` (uv),
// and how many there were.
fn find_blockers(map: texture_depth_2d_array, uv: vec2<f32>, layer: i32, z: f32, slope: vec2<f32>, radius: f32, phi: f32) -> vec2<f32> {
    let size = vec2<f32>(textureDimensions(map));
    let n = u32(shadow_params.settings.z);
    var sum = 0.0;
    var count = 0.0;
    for (var i = 0u; i < n; i++) {
        let o = vogel(i, n, phi) * radius;
        let t = vec2<i32>(clamp((uv + o) * size, vec2<f32>(0.0), size - 1.0));
        let d = textureLoad(map, t, layer, 0);
        if (d < z + dot(slope, o)) {
            sum += d;
            count += 1.0;
        }
    }
    return vec2<f32>(sum / max(count, 1.0), count);
}

// Lit fraction over a disk of `radius` (uv): bilinear compares on the receiver's plane.
fn filter_disk(map: texture_depth_2d_array, uv: vec2<f32>, layer: i32, z: f32, slope: vec2<f32>, radius: f32, phi: f32) -> f32 {
    let n = u32(shadow_params.settings.w);
    var lit = 0.0;
    for (var i = 0u; i < n; i++) {
        let o = vogel(i, n, phi) * radius;
        lit += textureSampleCompareLevel(map, shadow_sampler, uv + o, layer, z + dot(slope, o));
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
    let slope = receiver_slope(m, p, n, n_dot_l);
    // Blockers up to MAX_BLOCKER_DISTANCE above the receiver can reach this far.
    let reach = lp.x * min(z * info.z, MAX_BLOCKER_DISTANCE) / info.w;
    let search = clamp(reach, 2.0 * texel, MAX_FILTER_TEXELS * texel);
    let blockers = find_blockers(shadow_map, sc.xy, layer, z, slope, search, phi);
    if (blockers.y < 0.5) {
        return 1.0;
    }
    // Penumbra radius = blocker-to-receiver distance × tan(half the sun's angle).
    let distance = max(z - blockers.x, 0.0) * info.z;
    let radius = clamp(distance * lp.x / info.w, MIN_FILTER_TEXELS * texel, MAX_FILTER_TEXELS * texel);
    return filter_disk(shadow_map, sc.xy, layer, z, slope, radius, phi);
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

fn local_shadow(light: u32, li: LightItem, world: vec3<f32>, n: vec3<f32>, n_dot_l: f32, mode: f32) -> f32 {
    var layer = i32(li.shadow.y);
    if (layer < 0) {
        return 1.0;
    }
    let to_point = world - li.pos_range.xyz;
    if (u32(li.dir_type.w) == 1u) {
        // Pick the cube face by the major axis.
        let a = abs(to_point);
        if (a.x >= a.y && a.x >= a.z) {
            layer += select(1, 0, to_point.x > 0.0);
        } else if (a.y >= a.z) {
            layer += select(3, 2, to_point.y > 0.0);
        } else {
            layer += select(5, 4, to_point.z > 0.0);
        }
    }
    let lp = shadow_params.light_params[light * 2u];
    let clip = shadow_params.light_params[light * 2u + 1u];
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
    // `bias` texels of depth, converted to the map's non-linear depth.
    let z = sc.z - lp.y * texel_world * near * far / ((far - near) * d * d);
    if (mode < 1.5) {
        return textureSampleCompareLevel(local_shadow_map, shadow_sampler, sc.xy, layer, z);
    }
    let phi = shadow_noise(world) * 6.2831853;
    let slope = receiver_slope(m, p, n, n_dot_l);
    // The source's radius seen at the receiver, doubled: finds blockers down to a third of the way.
    let uv_per_metre = 1.0 / (2.0 * tan_half * d);
    let search = clamp(2.0 * lp.x * uv_per_metre, 2.0 * texel, MAX_FILTER_TEXELS * texel);
    let blockers = find_blockers(local_shadow_map, sc.xy, layer, z, slope, search, phi);
    if (blockers.y < 0.5) {
        return 1.0;
    }
    // Similar triangles: penumbra = radius × (receiver - blocker) / blocker.
    let db = linear_depth(blockers.x, near, far);
    let radius_uv = lp.x * max(d - db, 0.0) / max(db, 1e-3) * uv_per_metre;
    let radius = clamp(radius_uv, MIN_FILTER_TEXELS * texel, MAX_FILTER_TEXELS * texel);
    return filter_disk(local_shadow_map, sc.xy, layer, z, slope, radius, phi);
}

// Visibility of light `i` (0 = fully shadowed).
fn shadow_factor(i: u32, li: LightItem, world: vec3<f32>, n: vec3<f32>, l: vec3<f32>) -> f32 {
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
