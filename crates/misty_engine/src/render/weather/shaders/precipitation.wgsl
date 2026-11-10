// Weather composite (render/weather): drawn after the temporal resolve into the HDR image,
// before depth of field, motion blur, bloom and tone mapping. COMMON (view uniform, fog) and
// weather_common.wgsl come first.
//
// - fs_screen: haze that lowers visibility, distant rain / snow sheets, curtains of rain under
//   far storm cells, ground spray in heavy rain, and the lightning flash in the sky.
// - vs_rain / fs_rain: rain streaks. Every drop is procedural (a hash of its index) in a box
//   that follows the camera, anchored to the world and moved by the accumulated wind drift and
//   fall: no simulation state and no CPU work per drop. A drop is a motion-blurred streak that
//   shows the scene behind it refracted (the opaque scene copy), with a sky rim, sun and light
//   glints; it stops at the first surface from above (the occlusion map) and at the depth
//   buffer.
// - vs_snow / fs_snow: flakes of three shapes that tumble, swirl and drift, large and soft
//   near the camera (depth of field), streaked in strong wind.
// - vs_splash / fs_splash: crown splashes and ground mist spawned from the depth buffer by the
//   splash compute pass (splash_sim.wgsl).
// - vs_bolt / fs_bolt: lightning bolt channels.
// - fs_lens: drops on the lens.

struct Precip {
    // This frame's view-projection without TAA jitter, and its inverse.
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz = camera position, w = weather clock (s).
    camera: vec4<f32>,
    // xyz = camera right, w = tan(half vertical field of view).
    right: vec4<f32>,
    // xyz = camera up, w = 1 for orthographic cameras.
    up: vec4<f32>,
    // xyz = camera forward, w = camera near plane.
    forward: vec4<f32>,
    // xy = colour target size (px), zw = scene copy size (px).
    sizes: vec4<f32>,
    // x = near-layer drops, y = far-layer drops, z = most drops / m³ near the camera,
    // w = drop diameter (m).
    rain: vec4<f32>,
    // x = fall speed (m/s), y = streak seconds, z = opacity, w = refraction 0..1.
    rain2: vec4<f32>,
    // x = near-layer flakes, y = far-layer flakes, z = most flakes / m³, w = flake size (m).
    snow: vec4<f32>,
    // x = fall speed, y = swirl, z = opacity, w = streak seconds.
    snow2: vec4<f32>,
    // xyz = wind at the camera (m/s), w = accumulated rain fall (m).
    wind: vec4<f32>,
    // xyz = accumulated wind drift (m), w = accumulated snow fall (m).
    drift: vec4<f32>,
    // x = haze extinction (1/m), y = ground mist 0..1, z = 1 for far sheets, w = splash slots.
    haze: vec4<f32>,
    // x = camera far plane, y = 1 when the scene depth copy is valid, z = lens drops 0..1,
    // w = rain intensity at the camera.
    misc: vec4<f32>,
    // x = bolt segments, y = 1 for the glow (bloom) pass, z = 1 = splash rings, w = 1 = mist.
    bolt: vec4<f32>,
};

struct WLightItem {
    pos_range: vec4<f32>,
    dir_type: vec4<f32>,
    color: vec4<f32>,
    spot: vec4<f32>,
    shadow: vec4<f32>,
    area_x: vec4<f32>,
    area_y: vec4<f32>,
};

// The first part of the lit passes' `Lights` uniform (mesh.wgsl).
struct WLights {
    info: vec4<f32>,
    shadow_view_proj: mat4x4<f32>,
    sky: vec4<f32>,
    equator: vec4<f32>,
    ground: vec4<f32>,
    items: array<WLightItem, 8>,
};

struct Splash {
    // xyz = where it landed, w = birth time (weather clock).
    pos: vec4<f32>,
    // xyz = surface normal, w = size (m); negative = a mist puff.
    info: vec4<f32>,
};

struct BoltSegment {
    // xyz = start, w = width (m).
    a: vec4<f32>,
    // xyz = end, w = brightness 0..1.
    b: vec4<f32>,
};

@group(1) @binding(0) var<uniform> pr: Precip;
@group(1) @binding(1) var scene_color: texture_2d<f32>;
@group(1) @binding(2) var scene_depth: texture_2d<f32>;
@group(1) @binding(3) var wsampler: sampler;
@group(1) @binding(4) var<uniform> wlights: WLights;
@group(1) @binding(5) var<storage, read> splashes: array<Splash>;
@group(1) @binding(6) var<storage, read> bolt_segments: array<BoltSegment>;

const NEAR_BOX: vec3<f32> = vec3<f32>(32.0, 16.0, 32.0);
const FAR_BOX: vec3<f32> = vec3<f32>(64.0, 32.0, 64.0);
const PI_W: f32 = 3.14159265;

// ---- helpers ----

fn pcg(n: u32) -> u32 {
    var h = n * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}

fn rand4(n: u32) -> vec4<f32> {
    let a = pcg(n);
    let b = pcg(a ^ 0x9E3779B9u);
    let c = pcg(b ^ 0x85EBCA6Bu);
    let d = pcg(c ^ 0xC2B2AE35u);
    return vec4<f32>(vec4<u32>(a, b, c, d) >> vec4<u32>(8u)) / 16777216.0;
}

// Device depth (0 near .. 1 far) → distance along the view axis.
fn linear_depth(d: f32) -> f32 {
    if (pr.up.w > 0.5) {
        return mix(pr.forward.w, pr.misc.x, d);
    }
    let n = pr.forward.w;
    let f = pr.misc.x;
    return n * f / max(f - d * (f - n), 1e-6);
}

// Scene depth (view-axis metres) behind output pixel `px`; very far when there is none.
fn scene_distance(px: vec2<f32>) -> f32 {
    if (pr.misc.y < 0.5) {
        return 1.0e7;
    }
    let ip = vec2<i32>(px * pr.sizes.zw / pr.sizes.xy);
    let d = textureLoad(scene_depth, clamp(ip, vec2<i32>(0), vec2<i32>(pr.sizes.zw) - 1), 0).r;
    if (d >= 1.0) {
        return 1.0e7;
    }
    return linear_depth(d);
}

fn henyey(cos_t: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI_W * pow(max(1.0 + g2 - 2.0 * g * cos_t, 1e-4), 1.5));
}

// Ambient light around a drop (the lit passes' hemisphere, dimmed by the weather).
fn ambient_light(n_up: f32) -> vec3<f32> {
    let hemi = mix(wlights.equator.rgb, wlights.sky.rgb, clamp(n_up, 0.0, 1.0));
    return hemi * wlights.info.z;
}

// Sun light scattered towards the eye by a drop or flake at `world` (`g` = forward scattering).
fn sun_scatter(world: vec3<f32>, v: vec3<f32>, g: f32) -> vec3<f32> {
    if (view.sun_dir.w < 0.5) {
        return vec3<f32>(0.0);
    }
    let cos_t = dot(-view.sun_dir.xyz, v);
    let open = mix(0.25, 1.0, weather_exposure(world));
    return view.sun_color.rgb * (1.0 - weather.occ2.z) * henyey(-cos_t, g) * open;
}

// Point and spot lights scattered by a drop at `world` (rain shining in street lamps).
fn local_scatter(world: vec3<f32>, v: vec3<f32>, g: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let count = min(u32(wlights.info.x), 8u);
    for (var i = 0u; i < count; i++) {
        let li = wlights.items[i];
        let kind = u32(li.dir_type.w + 0.5);
        if (kind != 1u && kind != 2u) {
            continue;
        }
        let to = li.pos_range.xyz - world;
        let d2 = dot(to, to);
        let range = li.pos_range.w;
        if (d2 >= range * range) {
            continue;
        }
        let d = sqrt(d2);
        let l = to / max(d, 1e-4);
        let fall = clamp(1.0 - pow(d / range, 4.0), 0.0, 1.0);
        var att = fall * fall / (d2 + 1.0);
        if (kind == 2u) {
            att *= smoothstep(li.spot.x, li.spot.y, dot(-l, li.dir_type.xyz));
        }
        sum += li.color.rgb * att * (0.05 + henyey(dot(l, -v) * -1.0, g) * 3.0);
    }
    return sum;
}

fn flash_scatter(v: vec3<f32>, g: f32) -> vec3<f32> {
    return weather.flash.rgb * (0.08 + henyey(-dot(-weather.flash_dir.xyz, v), g) * 1.5);
}

// Haze between the camera and `dist` metres.
fn haze_amount(dist: f32) -> f32 {
    return 1.0 - exp(-dist * pr.haze.x);
}

// Light the haze scatters towards the eye along `dir`.
fn haze_light(dir: vec3<f32>) -> vec3<f32> {
    var c = ambient_light(0.5) * 1.15;
    if (view.sun_dir.w > 0.5) {
        let mie = henyey(dot(dir, view.sun_dir.xyz), 0.6);
        c += view.sun_color.rgb * (1.0 - weather.occ2.z) * (0.05 + mie * 0.35);
    }
    c += weather.flash.rgb * 0.35 * (0.3 + 0.7 * max(dot(dir, weather.flash_dir.xyz), 0.0));
    return c;
}

fn cull_vertex() -> vec4<f32> {
    return vec4<f32>(2.0, 2.0, 2.0, 1.0);
}

// The box a layer's particles live in: centred ahead of the camera.
fn layer_centre(box: vec3<f32>) -> vec3<f32> {
    var fwd = vec3<f32>(pr.forward.x, 0.0, pr.forward.z);
    let l = length(fwd);
    fwd = select(vec3<f32>(0.0, 0.0, 1.0), fwd / max(l, 1e-4), l > 1e-3);
    return pr.camera.xyz + fwd * box.x * 0.3 + vec3<f32>(0.0, box.y * 0.1, 0.0);
}

// World position of a particle: its hashed spot in the box plus the accumulated drift and
// fall, wrapped into the box around `centre` (anchored to the world: the camera moving or
// turning doesn't move the drops).
fn wrap_in_box(unwrapped: vec3<f32>, centre: vec3<f32>, box: vec3<f32>) -> vec3<f32> {
    return centre + (fract((unwrapped - centre) / box) - 0.5) * box;
}

// 0 at the box faces .. 1 inside (no popping where particles wrap round).
fn box_fade(p: vec3<f32>, centre: vec3<f32>, box: vec3<f32>) -> f32 {
    let e = 0.5 - abs(p - centre) / box;
    return smoothstep(0.0, 0.1, min(e.x, min(e.y, e.z)));
}

fn to_screen(c: vec4<f32>) -> vec2<f32> {
    return c.xy / c.w * pr.sizes.xy * 0.5;
}

fn from_screen(s: vec2<f32>, clip: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(s / (pr.sizes.xy * 0.5) * clip.w, clip.z, clip.w);
}

// Pixels per metre at view depth `w`.
fn px_per_metre(w: f32) -> f32 {
    if (pr.up.w > 0.5) {
        return pr.sizes.y / max(2.0 * pr.right.w, 1e-4);
    }
    return pr.sizes.y / max(2.0 * pr.right.w * w, 1e-4);
}

// Corner `k` of a quad as (across -1..1, along 0..1).
fn quad_corner(k: u32) -> vec2<f32> {
    var c = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    return c[k];
}

// ---- screen: haze, sheets, curtains, mist, sky flash ----

fn ray_dir(px: vec2<f32>) -> vec3<f32> {
    let uv = px / pr.sizes.xy;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let a = pr.inv_view_proj * vec4<f32>(ndc, 0.0, 1.0);
    let b = pr.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    return normalize(b.xyz / b.w - a.xyz / a.w);
}

// Streaks of rain on a cylinder `radius` metres round the camera, seen along `dir`.
fn rain_sheet(dir: vec3<f32>, radius: f32, seed: f32) -> f32 {
    let flat_len = max(length(dir.xz), 1e-3);
    let h = dir.y / flat_len * radius;
    let theta = atan2(dir.x, dir.z);
    // Wind slants the streaks: the wind across the line of sight shears them.
    let tangent = vec2<f32>(cos(theta), -sin(theta));
    let shear = dot(pr.wind.xz, tangent) / max(pr.rain2.x, 0.5);
    let cols = round(6.2831853 * radius * 9.0);
    let u = theta / 6.2831853 * cols + h * shear * 9.0;
    let cell = floor(u);
    let fx = fract(u) - 0.5;
    let r = rand4(u32(i32(cell) + 100000) * 7919u + u32(seed));
    let width = 0.1 + r.y * 0.12;
    let across = exp(-pow((fx - (r.x - 0.5) * 0.6) / width, 2.0));
    let speed = 1.0 + r.z * 0.3;
    let v = (h + pr.wind.w * speed) / (1.2 + r.w * 1.6) + r.x * 13.0;
    let s = fract(v);
    let dash = smoothstep(0.0, 0.05, s) * (1.0 - smoothstep(0.12, 0.45, s));
    return across * dash;
}

// Snow flakes on a cylinder round the camera.
fn snow_sheet(dir: vec3<f32>, radius: f32, seed: f32) -> f32 {
    let flat_len = max(length(dir.xz), 1e-3);
    let h = dir.y / flat_len * radius;
    let theta = atan2(dir.x, dir.z);
    let cols = round(6.2831853 * radius * 3.0);
    let drift = dot(pr.drift.xz, vec2<f32>(cos(theta), -sin(theta)));
    let u = theta / 6.2831853 * cols + drift * 3.0 / 6.2831853;
    let v = (h + pr.drift.w) * 3.0;
    let cell = floor(vec2<f32>(u, v));
    let r = rand4(u32(i32(cell.x) + 50000) * 9781u + u32(i32(cell.y) + 50000) * 6271u + u32(seed));
    let f = fract(vec2<f32>(u, v)) - 0.25 - r.xy * 0.5;
    let size = 0.06 + r.z * 0.1;
    return exp(-dot(f, f) / (size * size)) * step(0.35, r.w);
}

// Where the ray meets area volume `i`: (enter, leave) distances (enter > leave = missed).
fn region_span(i: u32, o: vec3<f32>, d: vec3<f32>) -> vec2<f32> {
    let r = weather.regions[i];
    let q0 = (r.inverse * vec4<f32>(o, 1.0)).xyz;
    let qd = (r.inverse * vec4<f32>(d, 0.0)).xyz;
    if (r.shape.w > 0.5) {
        let b = dot(q0, qd);
        let c = dot(q0, q0) - r.shape.x * r.shape.x;
        let disc = b * b - c;
        if (disc < 0.0) {
            return vec2<f32>(1.0, 0.0);
        }
        let s = sqrt(disc);
        return vec2<f32>(-b - s, -b + s);
    }
    let inv = 1.0 / select(qd, vec3<f32>(1e-6), abs(qd) < vec3<f32>(1e-6));
    let t0 = (-r.shape.xyz - q0) * inv;
    let t1 = (r.shape.xyz - q0) * inv;
    let lo = min(t0, t1);
    let hi = max(t0, t1);
    return vec2<f32>(max(lo.x, max(lo.y, lo.z)), min(hi.x, min(hi.y, hi.z)));
}

@fragment
fn fs_screen(in: FullscreenOut) -> @location(0) vec4<f32> {
    let px = in.pos.xy;
    let dir = ray_dir(px);
    let axial = scene_distance(px);
    let sky = axial >= 1.0e6;
    // View-axis depth → distance along the ray.
    let along = max(dot(dir, pr.forward.xyz), 1e-3);
    let dist = select(axial / along, 1.0e5, sky);
    let light = haze_light(dir);
    var col = vec3<f32>(0.0);
    var alpha = 0.0;
    // Haze (visibility): the sky greys out too.
    let haze = select(haze_amount(dist), haze_amount(min(dist, 3.0 / max(pr.haze.x, 1e-6))), sky);
    col += light * haze;
    alpha = haze;
    // Distant sheets of rain and snow beyond the particle boxes.
    if (pr.haze.z > 0.5) {
        let radii = vec3<f32>(17.0, 27.0, 44.0);
        for (var k = 0; k < 3; k++) {
            let rr = radii[k];
            let t = rr / max(length(dir.xz), 0.05);
            if (t < dist && t < 400.0) {
                let p = pr.camera.xyz + dir * t;
                let amt = weather_precip_at(p);
                if (amt.x > 0.0) {
                    let s = rain_sheet(dir, rr, f32(k) * 131.0) * clamp(amt.x / 30.0, 0.0, 1.5) * 0.11 * pr.rain2.z;
                    col += light * 1.6 * s * (1.0 - alpha);
                    alpha += s * 0.5 * (1.0 - alpha);
                }
                if (amt.y > 0.0) {
                    let s = snow_sheet(dir, rr, f32(k) * 71.0) * clamp(amt.y / 12.0, 0.0, 1.5) * 0.35 * pr.snow2.z;
                    col += light * 1.3 * s * (1.0 - alpha);
                    alpha += s * (1.0 - alpha);
                }
            }
        }
    }
    // Curtains of rain or snow under distant area volumes (storm cells seen from outside).
    let n = u32(weather.info.z);
    for (var i = 0u; i < n; i++) {
        let span = region_span(i, pr.camera.xyz, dir);
        let t0 = max(span.x, 46.0);
        let t1 = min(span.y, min(dist, 4000.0));
        if (t1 > t0) {
            let rg = weather.regions[i];
            let dens = (rg.precip.x + rg.precip.y * 1.5) * rg.blend.y;
            let streaks = 0.75 + 0.25 * weather_noise(vec2<f32>(atan2(dir.x, dir.z) * 60.0, pr.wind.w * 0.05));
            let c = (1.0 - exp(-(t1 - t0) * dens * 0.00045)) * streaks;
            col += light * 0.85 * c * (1.0 - alpha);
            alpha += c * (1.0 - alpha);
        }
    }
    // Ground spray where heavy rain hits open ground.
    if (pr.haze.y > 0.0 && !sky) {
        let p = pr.camera.xyz + dir * dist;
        let amt = weather_precip_at(p);
        if (amt.x > 0.0) {
            let near_far = smoothstep(2.0, 12.0, dist) * (1.0 - smoothstep(60.0, 140.0, dist));
            let grazing = 1.0 - abs(dir.y);
            let drift = pr.drift.xz * 0.6 + vec2<f32>(0.0, pr.camera.w * 0.4);
            let puffs = weather_fbm(p.xz * 0.35 + drift) * weather_fbm(p.xz * 1.3 - drift * 1.7);
            let m = pr.haze.y * clamp(amt.x / 35.0, 0.0, 1.0) * near_far * (0.4 + 0.6 * grazing) * puffs * weather_exposure(p + vec3<f32>(0.0, 0.3, 0.0)) * 0.55;
            col += light * 1.2 * m * (1.0 - alpha);
            alpha += m * (1.0 - alpha);
        }
    }
    // The flash lighting the clouds.
    if (sky && weather.flash.w > 0.0) {
        let towards = max(dot(dir, weather.flash_dir.xyz), 0.0);
        col += weather.flash.rgb * (0.12 + 0.88 * pow(towards, 5.0)) * 0.5 * smoothstep(-0.1, 0.25, dir.y);
    }
    return vec4<f32>(col, clamp(alpha, 0.0, 1.0));
}

// ---- rain ----

struct DropOut {
    @builtin(position) clip: vec4<f32>,
    // x = across the streak -1..1, y = along it 0..1.
    @location(0) quad: vec2<f32>,
    // x = opacity, y = streak width (px), z = view depth (m), w = refraction offset (px).
    @location(1) @interpolate(flat) params: vec4<f32>,
    @location(2) @interpolate(flat) world: vec3<f32>,
    // Across direction in pixels (x right, y up).
    @location(3) @interpolate(flat) perp: vec2<f32>,
};

fn culled_drop() -> DropOut {
    var out: DropOut;
    out.clip = cull_vertex();
    out.quad = vec2<f32>(0.0);
    out.params = vec4<f32>(0.0);
    out.world = vec3<f32>(0.0);
    out.perp = vec2<f32>(0.0);
    return out;
}

@vertex
fn vs_rain(@builtin(vertex_index) vi: u32) -> DropOut {
    let id = vi / 6u;
    let corner = quad_corner(vi % 6u);
    let near_layer = f32(id) < pr.rain.x;
    let box = select(FAR_BOX, NEAR_BOX, near_layer);
    let salt = select(0x51ED27u, 0xA341u, near_layer);
    let r = rand4(id ^ salt);
    let r2 = rand4(id * 3u + 0x1F12u);
    // Speed spread in 1/32 steps: the wrapped fall never jumps (see `WRAP`).
    let spread = (floor(r2.x * 9.0) - 4.0) / 32.0;
    let speed = pr.rain2.x * (1.0 + spread);
    let centre = layer_centre(box);
    let unwrapped = r.xyz * box + vec3<f32>(pr.drift.x, -pr.wind.w * (1.0 + spread), pr.drift.z);
    let p = wrap_in_box(unwrapped, centre, box);
    // The far layer leaves the near box to the near layer.
    if (!near_layer) {
        let q = abs(p - layer_centre(NEAR_BOX)) - NEAR_BOX * 0.45;
        if (max(q.x, max(q.y, q.z)) < 0.0) {
            return culled_drop();
        }
    }
    // Thinned to the local amount (areas, priorities).
    let amount = weather_precip_at(p).x;
    if (r.w * pr.rain.z >= amount) {
        return culled_drop();
    }
    // Stopped by the first surface above (roofs, trees).
    if (p.y < weather_cover_height(p.xz)) {
        return culled_drop();
    }
    let vel = vec3<f32>(pr.wind.x * (0.9 + r2.y * 0.2), -speed, pr.wind.z * (0.9 + r2.z * 0.2));
    let tail = p - vel * pr.rain2.y;
    let ch = pr.view_proj * vec4<f32>(p, 1.0);
    let ct = pr.view_proj * vec4<f32>(tail, 1.0);
    if (ch.w < pr.forward.w + 0.05 || ct.w < pr.forward.w + 0.05) {
        return culled_drop();
    }
    let sh = to_screen(ch);
    let st = to_screen(ct);
    var d = st - sh;
    let len = length(d);
    d = select(vec2<f32>(0.0, -1.0), d / max(len, 1e-4), len > 1e-3);
    let perp = vec2<f32>(-d.y, d.x);
    let width_px = pr.rain.w * (0.8 + r2.w * 0.5) * px_per_metre(ch.w);
    let w = max(width_px, 1.25);
    // Thinner than a pixel: fade instead of shrinking (no aliasing).
    let coverage = width_px / w;
    let s = mix(sh, st, corner.y) + perp * corner.x * w * 0.5 + d * (corner.y * 2.0 - 1.0) * w * 0.5;
    var out: DropOut;
    out.clip = from_screen(s, mix(ch, ct, corner.y));
    out.quad = corner;
    // Near drops fade in from the lens, far ones out into the sheets.
    let dist = ch.w;
    let fade = smoothstep(0.25, 1.2, dist) * (1.0 - smoothstep(select(52.0, 14.0, near_layer), select(64.0, 17.0, near_layer), dist));
    let fade_in = select(smoothstep(11.0, 15.0, dist), 1.0, near_layer);
    out.params = vec4<f32>(pr.rain2.z * coverage * fade * fade_in * box_fade(p, centre, box), w, dist, pr.rain2.w * (2.0 + w * 1.5));
    out.world = p;
    out.perp = perp;
    return out;
}

@fragment
fn fs_rain(in: DropOut) -> @location(0) vec4<f32> {
    let x = in.quad.x;
    let y = in.quad.y;
    let across = exp(-x * x * 2.8);
    let ends = smoothstep(0.0, 0.3, y) * (1.0 - smoothstep(0.7, 1.0, y));
    let behind = scene_distance(in.clip.xy) - in.params.z;
    if (behind <= 0.0) {
        discard;
    }
    let soft = clamp(behind / 0.25, 0.0, 1.0);
    let a = in.params.x * across * ends * soft;
    if (a < 0.002) {
        discard;
    }
    // A drop is a tiny lens: the scene behind is seen squeezed and flipped across it.
    let uv = in.clip.xy / pr.sizes.xy;
    let flip = vec2<f32>(in.perp.x, -in.perp.y) * (-x) * in.params.w / pr.sizes.xy;
    let refracted = textureSampleLevel(scene_color, wsampler, uv + flip + vec2<f32>(0.0, -0.004 * pr.rain2.w), 0.0).rgb;
    let v = normalize(pr.camera.xyz - in.world);
    // Fresnel rim of the round drop reflects the sky.
    let ndv = sqrt(max(1.0 - x * x, 0.0));
    let fresnel = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
    var light = ambient_light(0.6) * (0.06 + fresnel * 1.4);
    light += sun_scatter(in.world, v, 0.75) * 0.9;
    light += local_scatter(in.world, v, 0.7);
    light += flash_scatter(v, 0.6);
    let col = refracted * 0.88 + light;
    return vec4<f32>(col * a, a);
}

// ---- snow ----

struct FlakeOut {
    @builtin(position) clip: vec4<f32>,
    // Quad position -1..1 (x along the motion streak, y across).
    @location(0) quad: vec2<f32>,
    // x = opacity, y = softness, z = view depth, w = shape (0..1).
    @location(1) @interpolate(flat) params: vec4<f32>,
    @location(2) @interpolate(flat) world: vec3<f32>,
    // x = stretch along the motion (≥ 1), y = rotation (radians).
    @location(3) @interpolate(flat) shape: vec2<f32>,
};

fn culled_flake() -> FlakeOut {
    var out: FlakeOut;
    out.clip = cull_vertex();
    out.quad = vec2<f32>(0.0);
    out.params = vec4<f32>(0.0);
    out.world = vec3<f32>(0.0);
    out.shape = vec2<f32>(1.0, 0.0);
    return out;
}

@vertex
fn vs_snow(@builtin(vertex_index) vi: u32) -> FlakeOut {
    let id = vi / 6u;
    let k = quad_corner(vi % 6u);
    let corner = vec2<f32>(k.y * 2.0 - 1.0, k.x);
    let near_layer = f32(id) < pr.snow.x;
    let box = select(FAR_BOX, NEAR_BOX, near_layer);
    let salt = select(0x7E57u, 0x5A0Bu, near_layer);
    let r = rand4(id ^ salt);
    let r2 = rand4(id * 5u + 0x3C41u);
    let spread = (floor(r2.x * 9.0) - 4.0) / 32.0;
    let t = pr.camera.w;
    let centre = layer_centre(box);
    let unwrapped = r.xyz * box + vec3<f32>(pr.drift.x, -pr.drift.w * (1.0 + spread), pr.drift.z);
    var p = wrap_in_box(unwrapped, centre, box);
    // Swirling: each flake loops on its own slow circles.
    let swirl = pr.snow2.y;
    let f1 = 0.6 + r2.y * 0.9;
    let f2 = 1.3 + r2.z * 1.4;
    let ph = r.w * 6.2831853;
    let loop_pos = vec3<f32>(sin(t * f1 + ph) * 0.45 + sin(t * f2 + ph * 2.0) * 0.15, sin(t * f2 * 0.7 + ph) * 0.12, cos(t * f1 * 0.9 + ph) * 0.45);
    p += loop_pos * swirl;
    if (!near_layer) {
        let q = abs(p - layer_centre(NEAR_BOX)) - NEAR_BOX * 0.45;
        if (max(q.x, max(q.y, q.z)) < 0.0) {
            return culled_flake();
        }
    }
    let amount = weather_precip_at(p).y;
    if (r2.w * pr.snow.z >= amount) {
        return culled_flake();
    }
    if (p.y < weather_cover_height(p.xz)) {
        return culled_flake();
    }
    let c = pr.view_proj * vec4<f32>(p, 1.0);
    if (c.w < pr.forward.w + 0.05) {
        return culled_flake();
    }
    let dist = c.w;
    // Size: mostly small flakes, a few large clumps.
    var size = pr.snow.w * (0.4 + pow(r.y, 2.5) * 1.6);
    let ppm = px_per_metre(dist);
    // Depth of field: flakes very near the lens are big soft discs.
    let blur = clamp((1.6 - dist) / 1.3, 0.0, 1.0);
    size *= 1.0 + blur * 2.0;
    var size_px = size * ppm;
    let min_px = 1.6;
    let coverage = min(size_px / min_px, 1.0);
    size_px = max(size_px, min_px);
    // Motion streak in strong wind (blizzard), from this frame's screen motion.
    let vel = vec3<f32>(pr.wind.x, -pr.snow2.x * (1.0 + spread), pr.wind.z) + vec3<f32>(cos(t * f1 + ph), 0.0, -sin(t * f1 * 0.9 + ph)) * swirl * 0.4 * f1;
    let c2 = pr.view_proj * vec4<f32>(p - vel * pr.snow2.w, 1.0);
    var mdir = vec2<f32>(1.0, 0.0);
    var stretch = 1.0;
    if (c2.w > pr.forward.w) {
        let m = to_screen(c2) - to_screen(c);
        let ml = length(m);
        if (ml > size_px * 0.5) {
            mdir = m / ml;
            stretch = 1.0 + ml / size_px;
        }
    }
    let side = vec2<f32>(-mdir.y, mdir.x);
    let s = to_screen(c) + mdir * corner.x * size_px * 0.5 * stretch + side * corner.y * size_px * 0.5;
    var out: FlakeOut;
    out.clip = from_screen(s, c);
    out.quad = corner;
    let fade = smoothstep(0.15, 0.6, dist) * (1.0 - smoothstep(select(50.0, 14.0, near_layer), select(62.0, 17.0, near_layer), dist));
    let fade_in = select(smoothstep(11.0, 15.0, dist), 1.0, near_layer);
    let energy = 1.0 / ((1.0 + blur * 2.0) * (1.0 + blur * 2.0) * stretch);
    out.params = vec4<f32>(pr.snow2.z * coverage * fade * fade_in * box_fade(p, centre, box) * energy, blur, dist, r.z);
    out.world = p;
    // Tumbling: spin plus a squash as the flake turns edge-on.
    let spin = t * (0.8 + r2.y * 2.0) * select(-1.0, 1.0, r2.z > 0.5) + ph;
    out.shape = vec2<f32>(stretch, spin);
    return out;
}

// Flake shapes: 0 = soft round, 1 = six-armed star, 2 = clump of crystals.
fn flake_shape(q: vec2<f32>, kind: f32, soft: f32) -> f32 {
    let r = length(q);
    let edge = mix(0.12, 0.5, soft);
    if (kind < 0.55 || soft > 0.5) {
        return 1.0 - smoothstep(1.0 - edge * 1.6, 1.0, r);
    }
    if (kind < 0.85) {
        let a = atan2(q.y, q.x);
        let arms = pow(abs(cos(a * 3.0)), 6.0);
        let star = mix(0.35, 1.0, arms);
        let core = 1.0 - smoothstep(0.25, 0.45, r);
        return max(core, (1.0 - smoothstep(star - 0.25, star, r)) * (1.0 - smoothstep(0.85, 1.0, r)));
    }
    var c = 0.0;
    for (var i = 0; i < 3; i++) {
        let o = vec2<f32>(cos(f32(i) * 2.1 + 0.4), sin(f32(i) * 2.1 + 0.4)) * 0.38;
        c = max(c, 1.0 - smoothstep(0.35, 0.6, length(q - o)));
    }
    return c;
}

@fragment
fn fs_snow(in: FlakeOut) -> @location(0) vec4<f32> {
    let behind = scene_distance(in.clip.xy) - in.params.z;
    if (behind <= 0.0) {
        discard;
    }
    // Undo the stretch for the shape (a streak is a smeared flake).
    var q = in.quad;
    let st = in.shape.x;
    if (st > 1.0) {
        let along = abs(q.x) * st;
        q.x = sign(q.x) * max(along - (st - 1.0), 0.0);
    }
    let tumble = mix(0.45, 1.0, abs(cos(in.shape.y * 0.7)));
    let cs = cos(in.shape.y);
    let sn = sin(in.shape.y);
    let rq = vec2<f32>(q.x * cs - q.y * sn, (q.x * sn + q.y * cs) / tumble);
    let a = flake_shape(rq, in.params.w, in.params.y) * in.params.x * clamp(behind / 0.15, 0.0, 1.0);
    if (a < 0.003) {
        discard;
    }
    let v = normalize(pr.camera.xyz - in.world);
    // Ice scatters light every way, a little more forwards.
    var light = ambient_light(0.7) * 0.95;
    light += sun_scatter(in.world, v, 0.45) * 2.0 + view.sun_color.rgb * view.sun_dir.w * (1.0 - weather.occ2.z) * 0.08 * weather_exposure(in.world);
    light += local_scatter(in.world, v, 0.4) * 1.5;
    light += flash_scatter(v, 0.3);
    let col = light * 0.92;
    return vec4<f32>(col * a, a);
}

// ---- splashes and spray ----

struct SplashOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) quad: vec2<f32>,
    // x = opacity, y = kind (0 ring, 1 droplet, 2 mist), z = view depth, w = age 0..1.
    @location(1) @interpolate(flat) params: vec4<f32>,
    @location(2) @interpolate(flat) world: vec3<f32>,
};

fn culled_splash() -> SplashOut {
    var out: SplashOut;
    out.clip = cull_vertex();
    out.quad = vec2<f32>(0.0);
    out.params = vec4<f32>(0.0);
    out.world = vec3<f32>(0.0);
    return out;
}

const SPLASH_PARTS: u32 = 7u;

@vertex
fn vs_splash(@builtin(vertex_index) vi: u32, @builtin(instance_index) inst: u32) -> SplashOut {
    let part = vi / 6u;
    let k = quad_corner(vi % 6u);
    let corner = vec2<f32>(k.y * 2.0 - 1.0, k.x);
    let sp = splashes[inst];
    let mist = sp.info.w < 0.0;
    let life = select(0.34, 1.6, mist);
    let age = pr.camera.w - sp.pos.w;
    if (age < 0.0 || age > life || sp.pos.w <= 0.0) {
        return culled_splash();
    }
    let t = age / life;
    let size = abs(sp.info.w);
    let n = sp.info.xyz;
    var out: SplashOut;
    out.quad = corner;
    if (mist) {
        if (part != 0u || pr.bolt.w < 0.5) {
            return culled_splash();
        }
        // A soft puff rising a little and drifting with the wind.
        let p = sp.pos.xyz + n * (0.25 + t * 0.4) + pr.wind.xyz * age * 0.35;
        let c = pr.view_proj * vec4<f32>(p, 1.0);
        if (c.w < pr.forward.w + 0.05) {
            return culled_splash();
        }
        let r = size * (0.6 + t * 0.8) * px_per_metre(c.w);
        out.clip = from_screen(to_screen(c) + corner * r, c);
        out.params = vec4<f32>(pr.haze.y * 0.09 * sin(t * PI_W), 2.0, c.w, t);
        out.world = p;
        return out;
    }
    // Tangent frame on the surface.
    let up_ref = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.z) > 0.9);
    let ta = normalize(cross(n, up_ref));
    let tb = cross(n, ta);
    if (part == 0u) {
        if (pr.bolt.z < 0.5) {
            return culled_splash();
        }
        // The crown ring spreading on the surface.
        let radius = size * (0.25 + t * 1.6);
        let p = sp.pos.xyz + (ta * corner.x + tb * corner.y) * radius + n * 0.005;
        let c = pr.view_proj * vec4<f32>(p, 1.0);
        out.clip = c;
        out.params = vec4<f32>((1.0 - t) * (1.0 - t), 0.0, c.w, t);
        out.world = sp.pos.xyz;
        return out;
    }
    // Droplets thrown up and out, falling back (drawn as short streaks).
    let seed = rand4(inst * 13u + part * 101u);
    let a = (f32(part) + seed.x * 0.6) * 6.2831853 / f32(SPLASH_PARTS - 1u);
    let out_speed = (0.5 + seed.y * 0.7) * size * 9.0;
    let up_speed = (0.9 + seed.z * 0.9) * size * 14.0;
    let v0 = (ta * cos(a) + tb * sin(a)) * out_speed + n * up_speed;
    let g = vec3<f32>(0.0, -9.81, 0.0);
    let p = sp.pos.xyz + v0 * age + 0.5 * g * age * age;
    let v = v0 + g * age;
    let c = pr.view_proj * vec4<f32>(p, 1.0);
    let c2 = pr.view_proj * vec4<f32>(p - v * 0.016, 1.0);
    if (c.w < pr.forward.w + 0.05 || c2.w < pr.forward.w + 0.05) {
        return culled_splash();
    }
    let s0 = to_screen(c);
    let s1 = to_screen(c2);
    var d = s1 - s0;
    let l = length(d);
    d = select(vec2<f32>(0.0, 1.0), d / max(l, 1e-4), l > 1e-3);
    let side = vec2<f32>(-d.y, d.x);
    let dpx = size * 0.12 * px_per_metre(c.w);
    let w = max(dpx, 1.2);
    let s = mix(s0, s1, k.y) + side * k.x * w * 0.5 + d * (k.y * 2.0 - 1.0) * w * 0.5;
    out.clip = from_screen(s, c);
    out.quad = vec2<f32>(k.x, k.y);
    out.params = vec4<f32>((dpx / w) * (1.0 - t * t), 1.0, c.w, t);
    out.world = p;
    return out;
}

@fragment
fn fs_splash(in: SplashOut) -> @location(0) vec4<f32> {
    let behind = scene_distance(in.clip.xy) - in.params.z;
    let kind = u32(in.params.y + 0.5);
    // Rings lie on the surface: let them through a little behind it.
    if (behind < select(-0.02, -0.06, kind == 0u)) {
        discard;
    }
    var shape: f32;
    if (kind == 0u) {
        let r = length(in.quad);
        let ring = 1.0 - smoothstep(0.0, 0.18, abs(r - 0.8));
        shape = ring * (1.0 - smoothstep(0.9, 1.0, r));
    } else if (kind == 1u) {
        shape = exp(-in.quad.x * in.quad.x * 2.5) * smoothstep(0.0, 0.2, in.quad.y) * (1.0 - smoothstep(0.8, 1.0, in.quad.y));
    } else {
        shape = exp(-dot(in.quad, in.quad) * 2.2);
    }
    let a = shape * in.params.x * select(0.85, 0.5, kind == 0u);
    if (a < 0.003) {
        discard;
    }
    let v = normalize(pr.camera.xyz - in.world);
    var light = ambient_light(0.8) * select(0.6, 1.1, kind == 2u);
    light += sun_scatter(in.world, v, 0.6) * select(1.2, 0.6, kind == 2u);
    light += local_scatter(in.world, v, 0.6);
    light += flash_scatter(v, 0.4);
    return vec4<f32>(light * a, a);
}

// ---- lightning bolts ----

struct BoltOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) across: f32,
    // x = brightness, y = view depth, z = glow width share.
    @location(1) @interpolate(flat) params: vec4<f32>,
};

@vertex
fn vs_bolt(@builtin(vertex_index) vi: u32) -> BoltOut {
    let seg = bolt_segments[vi / 6u];
    let k = quad_corner(vi % 6u);
    var out: BoltOut;
    let ca = pr.view_proj * vec4<f32>(seg.a.xyz, 1.0);
    let cb = pr.view_proj * vec4<f32>(seg.b.xyz, 1.0);
    if (ca.w < pr.forward.w || cb.w < pr.forward.w || seg.b.w <= 0.0) {
        out.clip = cull_vertex();
        out.across = 0.0;
        out.params = vec4<f32>(0.0);
        return out;
    }
    let sa = to_screen(ca);
    let sb = to_screen(cb);
    var d = sb - sa;
    let l = length(d);
    d = select(vec2<f32>(0.0, 1.0), d / max(l, 1e-4), l > 1e-3);
    let side = vec2<f32>(-d.y, d.x);
    let c = mix(ca, cb, k.y);
    // The channel and a glow around it, at least a few pixels wide.
    let core_px = max(seg.a.w * px_per_metre(c.w), 1.5);
    let w = core_px * 7.0;
    let s = mix(sa, sb, k.y) + side * k.x * w * 0.5 + d * (k.y * 2.0 - 1.0) * core_px * 0.5;
    out.clip = from_screen(s, c);
    out.across = k.x;
    out.params = vec4<f32>(seg.b.w, c.w, 1.0 / 7.0, 0.0);
    return out;
}

@fragment
fn fs_bolt(in: BoltOut) -> @location(0) vec4<f32> {
    if (scene_distance(in.clip.xy) < in.params.y) {
        discard;
    }
    let x = in.across / in.params.z;
    let core = exp(-x * x * 1.5);
    let glow = exp(-in.across * in.across * 4.0);
    // Hazy air hides far bolts.
    let seen = exp(-in.params.y * pr.haze.x * 0.35);
    let hot = vec3<f32>(0.82, 0.86, 1.0);
    let k = select(1.0, 0.15, pr.bolt.y > 0.5);
    let c = hot * in.params.x * (core * 60.0 + glow * 4.0) * seen * k;
    return vec4<f32>(c, 0.0);
}

// ---- drops on the lens ----

@fragment
fn fs_lens(in: FullscreenOut) -> @location(0) vec4<f32> {
    let px = in.pos.xy;
    let uv = px / pr.sizes.xy;
    let aspect = pr.sizes.x / pr.sizes.y;
    let t = pr.camera.w;
    // No new drops under a roof.
    let open = weather_exposure(pr.camera.xyz + vec3<f32>(0.0, 0.5, 0.0));
    var col = vec3<f32>(0.0);
    var alpha = 0.0;
    for (var layer = 0; layer < 2; layer++) {
        let scale = select(9.0, 15.0, layer == 1);
        let g = vec2<f32>(uv.x * aspect, uv.y) * scale;
        let cell = floor(g);
        let r = rand4(u32(i32(cell.x) + 1000) * 7307u + u32(i32(cell.y) + 1000) * 1663u + u32(layer) * 17u);
        // Each cell holds a drop for a while, then a new one lands somewhere else.
        let period = 3.0 + r.x * 5.0;
        let phase = t / period + r.y;
        let gen = floor(phase);
        let life = fract(phase);
        let r2 = rand4(u32(gen) * 31u + u32(i32(cell.x) + 1000) * 7919u + u32(i32(cell.y) + 1000) * 104729u);
        if (r2.w > pr.misc.z * open) {
            continue;
        }
        // Heavy drops slide down late in their life.
        let slide = max(life - 0.6, 0.0) * r2.z * 1.4;
        let centre = cell + vec2<f32>(0.2 + r2.x * 0.6, 0.2 + r2.y * 0.6 + slide);
        let radius = 0.12 + r2.z * 0.22;
        var q = (g - centre) / radius;
        q.y *= 1.0 - slide * 0.6;
        let d = length(q);
        if (d >= 1.0) {
            continue;
        }
        let fade = smoothstep(0.0, 0.08, life) * (1.0 - smoothstep(0.85, 1.0, life));
        // The drop flips and magnifies what's behind it.
        let lens = q * sqrt(1.0 - d * d);
        let suv = uv - lens * radius / scale * vec2<f32>(1.0 / aspect, 1.0) * 1.6;
        var c = textureSampleLevel(scene_color, wsampler, suv, 0.0).rgb * 0.9;
        // Dark rim, bright highlight.
        c *= mix(1.0, 0.45, smoothstep(0.75, 1.0, d));
        c += ambient_light(1.0) * 1.5 * (1.0 - smoothstep(0.0, 0.25, length(q - vec2<f32>(-0.35, -0.4))));
        let a = fade * (1.0 - smoothstep(0.92, 1.0, d));
        col = mix(col, c, a);
        alpha = max(alpha, a);
    }
    return vec4<f32>(col * alpha, alpha);
}
