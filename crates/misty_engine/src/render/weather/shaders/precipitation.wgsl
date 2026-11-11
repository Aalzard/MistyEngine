// Weather composite (render/weather): drawn after the temporal resolve into the HDR image,
// before depth of field, motion blur, bloom and tone mapping. COMMON (view uniform, fog) and
// weather_common.wgsl come first.
//
// - fs_screen: haze that lowers visibility, distant rain / snow sheets, curtains of rain under
//   far storm cells, ground spray in heavy rain, and the lightning flash in the sky.
// - vs_rain / fs_rain: rain. Every drop is procedural (a hash of its index) in a box that
//   follows the camera, anchored to the world and moved by the accumulated wind drift and
//   fall: no simulation state and no CPU work per drop. A drop is a lens drawn out by the
//   exposure: wide, soft and see-through near the camera (depth of field), a thin streak of
//   the light around it further out, with glints only from the sun, lamps and lightning; it
//   reads the blurred scene chain (scene_blur.rs), and stops at the first surface from above
//   (the occlusion map) and at the depth buffer.
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
    // x = wind turbulence 0..1, y = depth of field (blur of a drop 1 m away as a share of the
    // picture height), z = the widest level of the blurred scene, w = rain veil 0..1.
    rain3: vec4<f32>,
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
// The opaque scene blurred into a small mip chain (scene_blur.rs): level 0 = half size.
@group(1) @binding(7) var scene_blur: texture_2d<f32>;
@group(1) @binding(8) var blur_sampler: sampler;

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

// Sun light scattered towards the eye by a drop or flake at `world` (`v` = towards the eye,
// `g` = forward scattering: brightest looking towards the sun through the drop).
fn sun_scatter(world: vec3<f32>, v: vec3<f32>, g: f32) -> vec3<f32> {
    if (view.sun_dir.w < 0.5) {
        return vec3<f32>(0.0);
    }
    let cos_t = dot(view.sun_dir.xyz, -v);
    let open = mix(0.25, 1.0, weather_exposure(world));
    return view.sun_color.rgb * (1.0 - weather.occ2.z) * henyey(cos_t, g) * open;
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
        // A lamp seen through the drop is brightest; from the side a drop still glints.
        sum += li.color.rgb * att * (0.02 + henyey(dot(l, -v), g) * 1.0);
    }
    return sum;
}

fn flash_scatter(v: vec3<f32>, g: f32) -> vec3<f32> {
    return weather.flash.rgb * (0.08 + henyey(dot(weather.flash_dir.xyz, -v), g) * 1.5);
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
    // Lightning lights the wet air round the strike, not the whole sky evenly.
    c += weather.flash.rgb * 0.14 * (0.12 + 0.88 * pow(max(dot(dir, weather.flash_dir.xyz), 0.0), 3.0));
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

// Smooth noise whose columns wrap after `period` cells (no seam where the angle wraps).
fn sheet_noise(u: f32, v: f32, period: f32, seed: f32) -> f32 {
    let i = floor(vec2<f32>(u, v));
    let f = fract(vec2<f32>(u, v));
    let w = f * f * (3.0 - 2.0 * f);
    let x0 = i.x - floor(i.x / period) * period;
    let x1 = (i.x + 1.0) - floor((i.x + 1.0) / period) * period;
    let a = weather_hash2(vec2<f32>(x0 + seed, i.y));
    let b = weather_hash2(vec2<f32>(x1 + seed, i.y));
    let c = weather_hash2(vec2<f32>(x0 + seed, i.y + 1.0));
    let d = weather_hash2(vec2<f32>(x1 + seed, i.y + 1.0));
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

// A soft curtain of rain on a cylinder `radius` metres round the camera, seen along `dir`:
// fine falling streaks that run into each other (at this distance a drop is far thinner than
// a pixel, so there are no single lines, only a streaky veil).
fn rain_sheet(dir: vec3<f32>, radius: f32, seed: f32) -> f32 {
    let flat_len = max(length(dir.xz), 1e-3);
    let h = dir.y / flat_len * radius;
    let theta = atan2(dir.x, dir.z);
    // Wind slants the streaks: the wind across the line of sight shears them.
    let tangent = vec2<f32>(cos(theta), -sin(theta));
    let shear = dot(pr.wind.xz, tangent) / max(pr.rain2.x, 0.5);
    // Columns a few pixels wide whatever the distance and the picture size.
    let cols = max(round(6.2831853 * pr.sizes.y / max(2.0 * pr.right.w, 0.1) / 3.2), 16.0);
    let u = (theta / 6.2831853 + 0.5 + h * shear / (6.2831853 * radius)) * cols;
    let fall = h + pr.wind.w * (1.0 + seed * 0.07);
    let fine = sheet_noise(u, fall / 1.7, cols, seed * 37.0);
    let broad = sheet_noise(u * round(cols * 0.37) / cols, fall / 4.3 + 11.0, round(cols * 0.37), seed * 91.0);
    return smoothstep(0.42, 0.95, fine * 0.65 + broad * 0.35);
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
    // In a gale far flakes stream past too fast to be seen one by one: they thin into the haze.
    let calm = 1.0 / (1.0 + length(pr.wind.xz) * 0.2);
    return exp(-dot(f, f) / (size * size)) * step(0.35, r.w) * calm;
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
    let uv = px / pr.sizes.xy;
    let dir = ray_dir(px);
    let axial = scene_distance(px);
    let sky = axial >= 1.0e6;
    // View-axis depth → distance along the ray.
    let along = max(dot(dir, pr.forward.xyz), 1e-3);
    let dist = select(axial / along, 1.0e5, sky);
    let light = haze_light(dir);
    var col = vec3<f32>(0.0);
    var alpha = 0.0;
    // Heavy rain washes the colour out of everything a little way off (the air between is
    // full of grey water): the opaque scene's colour is pulled towards its own grey.
    if (pr.rain3.w > 0.0 && !sky) {
        let here = textureSampleLevel(scene_color, wsampler, uv, 0.0).rgb;
        let grey = vec3<f32>(dot(here, vec3<f32>(0.2126, 0.7152, 0.0722)));
        alpha = pr.rain3.w * 0.45 * smoothstep(3.0, 45.0, dist);
        col = grey * alpha;
    }
    // Haze (visibility): the sky greys out too.
    let haze = select(haze_amount(dist), haze_amount(min(dist, 3.0 / max(pr.haze.x, 1e-6))), sky);
    col = col * (1.0 - haze) + light * haze;
    alpha += haze * (1.0 - alpha);
    var flash_sky = vec3<f32>(0.0);
    if (sky) {
        flash_sky = sky_flash(dir);
    }
    // Distant sheets of rain and snow beyond the particle boxes.
    if (pr.haze.z > 0.5) {
        // Rain takes the light around it: a grey veil over dark trees, nothing over the sky.
        let around = light_around(uv, haze, light) + flash_sky;
        let radii = vec3<f32>(15.0, 26.0, 44.0);
        for (var k = 0; k < 3; k++) {
            let rr = radii[k];
            let t = rr / max(length(dir.xz), 0.05);
            if (t < dist && t < 400.0) {
                let p = pr.camera.xyz + dir * t;
                let amt = weather_precip_at(p);
                if (amt.x > 0.0) {
                    // Bands of denser rain drift across with the wind.
                    let bands = 0.55 + 0.9 * weather_noise(p.xz * 0.045 + pr.drift.xz * 0.045);
                    let s = rain_sheet(dir, rr, f32(k) + 1.0) * clamp(amt.x / 30.0, 0.0, 1.5) * 0.3 * pr.rain2.z * bands;
                    // Against the sky the veil is a little darker than it (grey shafts).
                    let lit = around * (1.0 - 0.3 * haze);
                    col += lit * s * (1.0 - alpha);
                    alpha += s * (1.0 - alpha);
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
    // The flash lighting the clouds from inside.
    col += flash_sky;
    return vec4<f32>(col, clamp(alpha, 0.0, 1.0));
}

// ---- rain ----
//
// A falling drop is a small lens. In one exposure it draws a streak as long as its fall and
// as wide as itself; what shows through it is the scene behind, squeezed and flipped, and out
// of focus when the drop is near the lens. So drops are never white lines:
// - near the camera (under ~3 m) a drop is a wide, soft, see-through smear that bends the
//   background (depth of field: the lens is focused far away);
// - further out it is a thin streak carrying the average light around it: lighter than dark
//   trees, the same as the sky (so it vanishes there), and bright only where the sun, a lamp
//   or lightning shines through it;
// - far away the streaks merge into the soft sheets of `fs_screen`.

struct DropOut {
    @builtin(position) clip: vec4<f32>,
    // x = across the streak -1..1, y = along it 0..1.
    @location(0) quad: vec2<f32>,
    // x = opacity, y = softness 0 (sharp) .. 1 (fully out of focus), z = view depth (m),
    // w = lens offset (px) at the drop's edge.
    @location(1) @interpolate(flat) params: vec4<f32>,
    @location(2) @interpolate(flat) world: vec3<f32>,
    // Across direction in pixels (x right, y up).
    @location(3) @interpolate(flat) perp: vec2<f32>,
    // x = blur level of the scene seen through the drop, y = bead phase, z = beads along the
    // streak, w = thickness 0 (hairline) .. 1 (a drop wide enough to see through).
    @location(4) @interpolate(flat) look: vec4<f32>,
};

fn culled_drop() -> DropOut {
    var out: DropOut;
    out.clip = cull_vertex();
    out.quad = vec2<f32>(0.0);
    out.params = vec4<f32>(0.0);
    out.world = vec3<f32>(0.0);
    out.perp = vec2<f32>(0.0);
    out.look = vec4<f32>(0.0);
    return out;
}

// Circle of confusion (px) of a point `dist` metres away for a lens focused far off.
fn defocus_px(dist: f32) -> f32 {
    let c = pr.sizes.y * pr.rain3.y * max(1.0 / max(dist, 0.05) - 1.0 / 14.0, 0.0);
    return min(c, pr.sizes.y * 0.07);
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
    // Drop size: mostly small drops, a few big ones (x the weather's drop size). Big drops
    // fall faster: the speed spread is in 1/32 steps so the wrapped fall never jumps (WRAP).
    let big = r2.w * r2.w;
    let size = 0.55 + big * 1.5;
    let spread = (floor(big * 12.99) - 4.0) / 32.0;
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
    // Well outside the picture (further than a streak is long): nothing more to work out.
    let ch = pr.view_proj * vec4<f32>(p, 1.0);
    if (ch.w < pr.forward.w - 1.5 || abs(ch.x) > abs(ch.w) * 1.15 + 1.5 || abs(ch.y) > abs(ch.w) * 1.15 + 1.5) {
        return culled_drop();
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
    // Wind: small drops are carried further than big ones, and turbulence swings every
    // drop's slant on its own slow eddies.
    let carried = 1.15 - big * 0.4 + (r2.x - 0.5) * 0.5;
    let t = pr.camera.w;
    let eddy = vec2<f32>(
        sin(p.x * 0.31 + p.z * 0.17 + t * 1.3 + r2.z * 6.2831853),
        cos(p.z * 0.29 - p.x * 0.13 + t * 1.1 + r2.y * 6.2831853),
    ) * pr.rain3.x * (0.8 + length(pr.wind.xz) * 0.25);
    // A drop doesn't fall dead straight: it sways on small loops of its own (air drag on a
    // wobbling drop), so no two streaks lie along the same line.
    let sway_f = vec2<f32>(5.0 + r2.z * 6.0, 4.0 + r2.x * 7.0);
    let sway_ph = vec2<f32>(r.w, r2.y) * 6.2831853;
    let sway_amp = (0.012 + 0.03 * (1.0 - big)) * (0.6 + pr.rain3.x * 1.6);
    let sway_vel = vec2<f32>(cos(t * sway_f.x + sway_ph.x) * sway_f.x, -sin(t * sway_f.y + sway_ph.y) * sway_f.y) * sway_amp;
    let vel = vec3<f32>(pr.wind.x * carried + eddy.x + sway_vel.x, -speed, pr.wind.z * carried + eddy.y + sway_vel.y);
    let tail = p - vel * pr.rain2.y;
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
    let dist = ch.w;
    // Drawn a little thicker than a real drop (a streak also holds the drop's wobble).
    // The nearest drops are drawn larger still: they are the ones the eye picks out, and
    // they give the rain its depth (big near, fine far).
    let near_boost = 1.7 + 2.3 * (1.0 - smoothstep(0.6, 5.0, dist));
    let drop_px = pr.rain.w * size * near_boost * px_per_metre(dist);
    // Depth: near drops are big; about half of them are out of focus (wide and soft), the
    // rest stay sharp and clearly seen, so the rain has a near, a middle and a far.
    let coc = defocus_px(dist) * mix(0.12, 1.0, smoothstep(0.35, 0.65, r.z));
    let width_px = drop_px + coc;
    let w = max(width_px, 2.2);
    // Thinner than a pixel: fade instead of shrinking (no aliasing).
    let coverage = width_px / w;
    // Out of focus the drop's light spreads over the blur disc (some is kept so the big
    // near drops still read as water).
    let soft = coc / max(width_px, 1e-3);
    let spread_out = mix(drop_px / max(width_px, 1e-3), 1.0, 0.45);
    let s = mix(sh, st, corner.y) + perp * corner.x * w * 0.5 + d * (corner.y * 2.0 - 1.0) * w * 0.5;
    var out: DropOut;
    out.clip = from_screen(s, mix(ch, ct, corner.y));
    out.quad = corner;
    // Near drops fade in from the lens, far ones out into the sheets.
    let fade = smoothstep(0.2, 0.7, dist) * (1.0 - smoothstep(select(34.0, 14.0, near_layer), select(52.0, 17.0, near_layer), dist));
    let fade_in = select(smoothstep(11.0, 15.0, dist), 1.0, near_layer);
    // Every drop its own brightness (how it catches the light as it wobbles).
    let own = 0.55 + r2.y * r2.y * 1.2;
    let alpha = pr.rain2.z * coverage * spread_out * own * fade * fade_in * box_fade(p, centre, box);
    // The lens: the image across the drop is flipped and squeezed.
    let lens = w * 0.5 * (1.0 + 2.5 * pr.rain2.w);
    out.params = vec4<f32>(alpha, soft, dist, lens);
    out.world = p;
    out.perp = perp;
    // Blur level: texels of the half-size level 0 are two pixels wide.
    let lod = clamp(log2(max(max(coc, w * 0.6), 2.0) * 0.5), 0.0, pr.rain3.z);
    out.look = vec4<f32>(lod, r.w * 6.2831853, 1.0 + r2.z * 3.0, clamp((w - 1.5) / 4.0, 0.0, 1.0));
    return out;
}

// What the scene looks like through water at `uv` at blur `level`.
fn through_water(uv: vec2<f32>, level: f32) -> vec3<f32> {
    return textureSampleLevel(scene_blur, blur_sampler, clamp(uv, vec2<f32>(0.002), vec2<f32>(0.998)), level).rgb;
}

// The light all round a point on screen: what a small drop gathers from everything behind
// it. The widest blur of the scene (pulled towards the middle of the picture), seen through
// the same haze as the pixel behind the drop (`hazed` 0..1, `haze_col` its light): the
// blurred scene is the scene before the haze, and a drop in front of the hazed sky must
// match it, or every streak would shine against it.
fn light_around(uv: vec2<f32>, hazed: f32, haze_col: vec3<f32>) -> vec3<f32> {
    let wide = mix(uv, vec2<f32>(0.5, 0.42), 0.35);
    let c = textureSampleLevel(scene_blur, blur_sampler, wide, pr.rain3.z).rgb;
    // Half of what a drop gathers is the sky, which the haze has already greyed.
    let sky_haze = select(0.0, 0.6, pr.haze.x > 0.0);
    return mix(c, haze_col, max(hazed, sky_haze));
}

// The lightning flash on the cloud base along `dir`: brightest round the strike, in the
// shapes of the clouds (lit from inside), not an even white sky.
fn sky_flash(dir: vec3<f32>) -> vec3<f32> {
    if (weather.flash.w <= 0.0) {
        return vec3<f32>(0.0);
    }
    let towards = max(dot(dir, weather.flash_dir.xyz), 0.0);
    let up = max(dir.y, 0.04);
    let cells = weather_fbm(dir.xz / up * 0.55 + pr.drift.xz * 0.002);
    let glow = 0.04 + 0.96 * pow(towards, 4.0);
    let shaped = mix(0.3, 1.5, smoothstep(0.25, 0.75, cells));
    return weather.flash.rgb * glow * shaped * 0.32 * smoothstep(-0.1, 0.25, dir.y);
}

// Haze over the scene `axial` metres (view-axis depth) behind a pixel seen along `dir` (0..1).
fn haze_behind(axial: f32, dir: vec3<f32>) -> f32 {
    if (pr.haze.x <= 0.0) {
        return 0.0;
    }
    let along = max(dot(dir, pr.forward.xyz), 1e-3);
    return haze_amount(min(axial / along, 3.0 / pr.haze.x));
}

@fragment
fn fs_rain(in: DropOut) -> @location(0) vec4<f32> {
    let x = in.quad.x;
    let y = in.quad.y;
    let scene_at = scene_distance(in.clip.xy);
    let behind = scene_at - in.params.z;
    if (behind <= 0.0) {
        discard;
    }
    let soft = in.params.y;
    let thick = in.look.w;
    // A bead, not a line: full and round at the head (where the drop is when the shutter
    // closes), thinning and fading up its tail. Blurred drops fade from the middle.
    let taper = mix(1.0, 0.45, smoothstep(0.1, 1.0, y));
    let ax = abs(x) / taper;
    let across = 1.0 - smoothstep(1.0 - mix(0.7, 1.0, soft), 1.0, ax);
    let ends = smoothstep(0.0, mix(0.1, 0.3, soft), y) * (1.0 - smoothstep(0.25, 1.0, y) * 0.85) * (1.0 - smoothstep(0.85, 1.0, y));
    // A falling drop wobbles: its streak is beaded, not even.
    let bead = 1.0 + 0.14 * (1.0 - soft) * sin(y * in.look.z * 6.2831853 + in.look.y);
    let a = clamp(in.params.x * across * ends * bead * clamp(behind / 0.25, 0.0, 1.0), 0.0, 0.92);
    if (a < 0.003) {
        discard;
    }
    let uv = in.clip.xy / pr.sizes.xy;
    // Across the drop the scene behind is flipped (a cylinder lens along the streak).
    let offset = vec2<f32>(in.perp.x, -in.perp.y) * (-x) * in.params.w / pr.sizes.xy;
    let v = normalize(pr.camera.xyz - in.world);
    let haze_col = haze_light(-v);
    let hazed = haze_behind(scene_at, -v);
    // In front of the sky a drop is a touch darker than it (half of what it gathers is the
    // ground), and it lights up with the clouds in a flash.
    let over_sky = smoothstep(0.88, 0.95, hazed);
    let lit_sky = sky_flash(-v) * over_sky;
    let seen = mix(through_water(uv + offset, in.look.x), haze_col, hazed) + lit_sky;
    let around = light_around(uv, hazed, haze_col) * (1.0 - 0.35 * over_sky) + lit_sky;
    // Fresnel: the middle of a drop is clear, its rim mirrors the light around it.
    let ndv = sqrt(max(1.0 - x * x, 0.0));
    let fresnel = 0.03 + 0.97 * pow(1.0 - ndv, 4.0);
    // A hairline streak can't show an image: it carries the average light instead.
    let clear = mix(0.25, 1.0 - fresnel, thick);
    var col = seen * clear + around * (1.0 - clear) * 1.2;
    // Every drop carries a highlight of the sky (a bright line off its middle): it is what
    // shows the rain against dark ground. At night there is no sky to mirror.
    let shine = exp(-(x - 0.4) * (x - 0.4) * 14.0) * mix(0.35, 1.0, thick);
    col += haze_col * shine * (1.0 - over_sky) * 0.55;
    // Light shining through the drop: the sun ahead and lamps. Lightning lasts a
    // thousandth of a second: it freezes the drop where it is (the head of the streak).
    var glint = sun_scatter(in.world, v, 0.8) * 0.8;
    glint += local_scatter(in.world, v, 0.7);
    glint += flash_scatter(v, 0.6) * 2.0 * (1.0 - smoothstep(0.02, 0.14, y));
    // Glints sit in the drop's bright core (a thin line down a thick streak); out of focus
    // the core spreads over the whole blur.
    col += glint * mix(mix(1.0, exp(-x * x * 5.0) * 1.6, thick), 1.0, soft);
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
    // Depth of field (the same lens as the rain's): flakes within a few metres are bigger,
    // softer and fainter the nearer they are.
    let coc = defocus_px(dist);
    let sharp_px = size * ppm;
    var size_px = sharp_px + coc;
    let blur = coc / max(size_px, 1e-3);
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
    // Blurred flakes spread their light; streaks keep more (the eye follows them).
    let kept = sharp_px / max(sharp_px + coc, 1e-3);
    let energy = mix(kept * kept, 1.0, 0.18) / max(sqrt(stretch), 1.0);
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
    // A flake smeared by its motion is a soft dash, not a pill with hard ends.
    let dash = 1.0 - smoothstep(0.1, 1.0, length(in.quad));
    let shape = mix(flake_shape(rq, in.params.w, in.params.y), dash, clamp((st - 1.0) / 1.2, 0.0, 1.0));
    let a = shape * in.params.x * clamp(behind / 0.15, 0.0, 1.0);
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
//
// Where a drop lands it throws up a thin crown of water that opens and falls back in a few
// hundredths of a second, a few droplets on their own arcs, and a ring that runs out over
// the wet surface. All of it is clear water: it shows the light around it, never white.
// Every splash differs (size, how long it lasts, how many droplets, how high).

struct SplashOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) quad: vec2<f32>,
    // x = opacity, y = kind (0 ring, 1 droplet, 2 mist, 3 crown), z = view depth, w = age 0..1.
    @location(1) @interpolate(flat) params: vec4<f32>,
    @location(2) @interpolate(flat) world: vec3<f32>,
    // x = the splash's own random number.
    @location(3) @interpolate(flat) own: vec2<f32>,
};

fn culled_splash() -> SplashOut {
    var out: SplashOut;
    out.clip = cull_vertex();
    out.quad = vec2<f32>(0.0);
    out.params = vec4<f32>(0.0);
    out.world = vec3<f32>(0.0);
    out.own = vec2<f32>(0.0);
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
    let own = rand4(inst * 7919u + 17u);
    // Big drops make splashes that last longer.
    let life = select(0.2 + own.x * 0.22, 1.6, mist);
    let age = pr.camera.w - sp.pos.w;
    if (age < 0.0 || age > life || sp.pos.w <= 0.0) {
        return culled_splash();
    }
    let t = age / life;
    let size = abs(sp.info.w) * (0.6 + own.x * 1.1);
    let n = sp.info.xyz;
    var out: SplashOut;
    out.quad = corner;
    out.own = vec2<f32>(own.y, own.z);
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
        let r = abs(sp.info.w) * (0.6 + t * 0.8) * px_per_metre(c.w);
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
        // The ring running out over the surface (it outlives the crown a little).
        let radius = size * (0.6 + t * 3.2);
        let p = sp.pos.xyz + (ta * corner.x + tb * corner.y) * radius + n * 0.005;
        let c = pr.view_proj * vec4<f32>(p, 1.0);
        out.clip = c;
        out.params = vec4<f32>((1.0 - t) * (1.0 - t), 0.0, c.w, t);
        out.world = sp.pos.xyz;
        return out;
    }
    if (part == 1u) {
        // The crown: a cup of water standing on the surface, seen from the side. It opens
        // fast, stands a moment and drops back.
        let c0 = pr.view_proj * vec4<f32>(sp.pos.xyz, 1.0);
        if (c0.w < pr.forward.w + 0.05) {
            return culled_splash();
        }
        let grow = 1.0 - (1.0 - min(t * 2.5, 1.0)) * (1.0 - min(t * 2.5, 1.0));
        let h = size * (1.3 + own.y * 2.0) * grow * (1.0 - smoothstep(0.55, 1.0, t) * 0.6);
        let w = size * (0.9 + t * 1.5);
        // Upright on the surface, turned to the camera.
        let to_eye = normalize(pr.camera.xyz - sp.pos.xyz);
        var side = cross(n, to_eye);
        let sl = length(side);
        if (sl < 0.05) {
            return culled_splash();
        }
        side = side / sl;
        let ppm = px_per_metre(c0.w);
        // Too small to see as a shape: leave it to the droplets.
        if (w * ppm < 1.5) {
            return culled_splash();
        }
        let p = sp.pos.xyz + side * corner.x * w + n * (corner.y * 0.5 + 0.5) * h;
        let c = pr.view_proj * vec4<f32>(p, 1.0);
        out.clip = c;
        out.params = vec4<f32>((1.0 - smoothstep(0.45, 1.0, t)) * smoothstep(1.5, 5.0, w * ppm) * 0.85, 3.0, c.w, t);
        out.world = sp.pos.xyz;
        return out;
    }
    // Droplets thrown up and out, falling back: two to five of them, each its own arc.
    let seed = rand4(inst * 13u + part * 101u);
    if (f32(part) - 2.0 >= 2.0 + own.w * 3.2) {
        return culled_splash();
    }
    let a = (f32(part) + seed.x * 0.9) * 6.2831853 / f32(SPLASH_PARTS - 2u) + own.z * 6.2831853;
    let scale = size / 0.02;
    let out_speed = (0.12 + seed.y * 0.4) * scale;
    let up_speed = (0.35 + seed.z * 0.75) * sqrt(scale);
    let v0 = (ta * cos(a) + tb * sin(a)) * out_speed + n * up_speed;
    let g = vec3<f32>(0.0, -9.81, 0.0);
    let p = sp.pos.xyz + v0 * age + 0.5 * g * age * age;
    // Back on the ground: gone.
    if (dot(p - sp.pos.xyz, n) < -0.002) {
        return culled_splash();
    }
    let v = v0 + g * age;
    let c = pr.view_proj * vec4<f32>(p, 1.0);
    let c2 = pr.view_proj * vec4<f32>(p - v * 0.012, 1.0);
    if (c.w < pr.forward.w + 0.05 || c2.w < pr.forward.w + 0.05) {
        return culled_splash();
    }
    let s0 = to_screen(c);
    let s1 = to_screen(c2);
    var d = s1 - s0;
    let l = length(d);
    d = select(vec2<f32>(0.0, 1.0), d / max(l, 1e-4), l > 1e-3);
    let side = vec2<f32>(-d.y, d.x);
    let dpx = size * (0.18 + seed.w * 0.24) * px_per_metre(c.w);
    let w = max(dpx, 1.2);
    let s = mix(s0, s1, k.y) + side * k.x * w * 0.5 + d * (k.y * 2.0 - 1.0) * w * 0.5;
    out.clip = from_screen(s, c);
    out.quad = vec2<f32>(k.x, k.y);
    out.params = vec4<f32>(min(dpx / w, 1.0) * (1.0 - t * t) * (0.5 + seed.w * 0.5), 1.0, c.w, t);
    out.world = p;
    return out;
}

@fragment
fn fs_splash(in: SplashOut) -> @location(0) vec4<f32> {
    let scene_at = scene_distance(in.clip.xy);
    let behind = scene_at - in.params.z;
    let kind = u32(in.params.y + 0.5);
    // Rings and crowns stand on the surface: let them through a little behind it.
    if (behind < select(-0.02, -0.06, kind == 0u || kind == 3u)) {
        discard;
    }
    var shape: f32;
    if (kind == 0u) {
        // A thin ring, a little broken (water films are never even).
        let r = length(in.quad);
        let ang = atan2(in.quad.y, in.quad.x);
        let broken = 0.55 + 0.45 * sin(ang * 3.0 + in.own.x * 40.0) * sin(ang * 5.0 + in.own.y * 17.0);
        let ring = 1.0 - smoothstep(0.0, 0.1, abs(r - 0.85));
        shape = ring * broken * 0.3;
    } else if (kind == 1u) {
        let along = in.quad.y * 2.0 - 1.0;
        shape = exp(-(in.quad.x * in.quad.x + along * along) * 2.2);
    } else if (kind == 3u) {
        // The cup in profile: thin walls leaning out, brightest at the two edges where the
        // sheet of water is seen edge-on, a ragged rim breaking into points.
        let hgt = in.quad.y * 0.5 + 0.5;
        let half = 0.45 + 0.5 * hgt;
        let x = in.quad.x / half;
        let ax = abs(x);
        let rim = 0.84 + 0.16 * sin(x * 5.0 + in.own.x * 30.0) * sin(x * 2.0 + in.own.y * 11.0);
        let inside = (1.0 - smoothstep(0.85, 1.0, ax)) * (1.0 - smoothstep(rim - 0.25, rim, hgt));
        let wall = 0.4 + 0.6 * smoothstep(0.45, 0.95, ax);
        shape = inside * wall * (0.5 + 0.5 * hgt) * 0.75;
    } else {
        shape = exp(-dot(in.quad, in.quad) * 2.2);
    }
    let a = shape * in.params.x * 0.75;
    if (a < 0.003) {
        discard;
    }
    let v = normalize(pr.camera.xyz - in.world);
    if (kind == 2u) {
        // Spray is fine droplets: it scatters like mist.
        var fog = ambient_light(0.8) * 1.1;
        fog += sun_scatter(in.world, v, 0.6) * 0.6;
        fog += local_scatter(in.world, v, 0.6);
        fog += flash_scatter(v, 0.4) * 0.25;
        return vec4<f32>(fog * a, a);
    }
    // Clear water: the light around it, plus what shines through.
    let uv = in.clip.xy / pr.sizes.xy;
    let haze_col = haze_light(-v);
    var light = light_around(uv, haze_behind(scene_at, -v), haze_col) * 1.1 + ambient_light(1.0) * 0.06;
    light += sun_scatter(in.world, v, 0.6) * 1.0;
    light += local_scatter(in.world, v, 0.6) * 1.5;
    light += flash_scatter(v, 0.4) * 0.25;
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
        // A few drops at a time, even in a downpour (the glass would be wiped otherwise).
        if (r2.w > pr.misc.z * open * 0.22) {
            continue;
        }
        // Heavy drops run down late in their life (staying inside their cell).
        let radius = 0.06 + r2.z * r2.z * 0.24;
        let slide = max(life - 0.6, 0.0) * r2.z * 0.5;
        let centre = cell + vec2<f32>(0.32 + r2.x * 0.36, 0.31 + r2.y * 0.18 + slide);
        var q = (g - centre) / radius;
        // Drops aren't round: a little lumpy, longer when they run.
        q *= 1.0 + 0.05 * sin(atan2(q.y, q.x) * 2.0 + r2.x * 20.0);
        q.y *= 1.0 - slide * 1.5;
        let d = length(q);
        if (d >= 1.0) {
            continue;
        }
        let fade = smoothstep(0.0, 0.08, life) * (1.0 - smoothstep(0.85, 1.0, life));
        // A drop on the glass is a tiny fisheye: the view upside down, far out of focus.
        let cuv = centre / scale * vec2<f32>(1.0 / aspect, 1.0);
        let suv = cuv - (uv - cuv) * 3.0;
        var c = through_water(suv, 2.0 + r2.z * 1.5);
        // A darker rim, and a soft highlight of the light above.
        c *= mix(1.0, 0.72, smoothstep(0.6, 1.0, d));
        let shine = 1.0 - smoothstep(0.0, 0.32, length(q - vec2<f32>(-0.3, -0.4)));
        c += through_water(vec2<f32>(0.5, 0.25), pr.rain3.z) * 0.5 * shine;
        let a = fade * (1.0 - smoothstep(0.7, 1.0, d)) * 0.85;
        col = mix(col, c, a);
        alpha = max(alpha, a);
    }
    return vec4<f32>(col * alpha, alpha);
}
