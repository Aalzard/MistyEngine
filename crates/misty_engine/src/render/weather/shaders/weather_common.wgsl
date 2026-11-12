// ---- Weather (render/weather): the view group's weather uniform and top-down occlusion map ----
// Included by the lit shaders (wet / snowy surfaces, lightning light) and the weather passes.
// With no weather `weather.info.x` is 0 and every function returns at once.

struct WeatherRegion {
    // World → the volume's frame (rotation + translation, metres).
    inverse: mat4x4<f32>,
    // xyz = box half extents (sphere: x = radius), w = 1 for a sphere.
    shape: vec4<f32>,
    // x = blend distance, y = weight.
    blend: vec4<f32>,
    // x = rain drops / m³, y = snow flakes / m³, z = splash rate.
    precip: vec4<f32>,
    // x = wetness, y = puddles, z = snow cover.
    surface: vec4<f32>,
};

struct WeatherGlobals {
    // x = 1 when weather draws, y = weather clock (s), z = area volume count, w = 1 when the
    // occlusion map is valid.
    info: vec4<f32>,
    // Global volumes: x = rain / m³, y = snow / m³, z = splash rate, w = rain intensity.
    precip: vec4<f32>,
    // Global volumes: x = wetness, y = puddles, z = snow cover, w = 1 for ripples.
    surface: vec4<f32>,
    // rgb = linear snow colour, w = puddle pattern size (m).
    snow: vec4<f32>,
    // x = 1 with a snow line, y = its height, z = puddle coverage setting (0..1),
    // w = wetness on (1) / off (0).
    snow_line: vec4<f32>,
    // rgb = lightning light (linear, × brightness), w = brightness.
    flash: vec4<f32>,
    // xyz = unit direction towards the flash.
    flash_dir: vec4<f32>,
    // Occlusion map: x = min world x, y = top height, z = min world z, w = world size (m).
    occ: vec4<f32>,
    // x = depth range (m), y = texels per side, z = sun dimming, w = ambient dimming.
    occ2: vec4<f32>,
    regions: array<WeatherRegion, 4>,
};

@group(0) @binding(24) var<uniform> weather: WeatherGlobals;
// First surface seen from above (device depth: 0 = the map's top, 1 = its bottom).
@group(0) @binding(25) var weather_occlusion: texture_depth_2d;

fn weather_on() -> bool {
    return weather.info.x > 0.5;
}

// 0..1 weight of area volume `i` at `p` (smooth fade over its blend distance).
fn weather_region_weight(i: u32, p: vec3<f32>) -> f32 {
    let r = weather.regions[i];
    let q = (r.inverse * vec4<f32>(p, 1.0)).xyz;
    var inside: f32;
    if (r.shape.w > 0.5) {
        inside = r.shape.x - length(q);
    } else {
        let d = r.shape.xyz - abs(q);
        inside = min(d.x, min(d.y, d.z));
    }
    if (inside <= 0.0) {
        return 0.0;
    }
    if (r.blend.x <= 1e-4) {
        return r.blend.y;
    }
    return r.blend.y * smoothstep(0.0, 1.0, inside / r.blend.x);
}

// Rain and snow per m³ and the splash rate at `p` (global volumes, then the areas in
// priority order).
fn weather_precip_at(p: vec3<f32>) -> vec3<f32> {
    var a = weather.precip.xyz;
    let n = u32(weather.info.z);
    for (var i = 0u; i < n; i++) {
        let w = weather_region_weight(i, p);
        if (w > 0.0) {
            a = mix(a, weather.regions[i].precip.xyz, w);
        }
    }
    return a;
}

// Wetness, puddles and snow cover at `p`.
fn weather_surface_at(p: vec3<f32>) -> vec3<f32> {
    var a = weather.surface.xyz;
    let n = u32(weather.info.z);
    for (var i = 0u; i < n; i++) {
        let w = weather_region_weight(i, p);
        if (w > 0.0) {
            a = mix(a, weather.regions[i].surface.xyz, w);
        }
    }
    return a;
}

// Height of the first surface above `xz` seen from the sky (the occlusion map), or a very low
// value outside the map.
fn weather_cover_height(xz: vec2<f32>) -> f32 {
    if (weather.info.w < 0.5) {
        return -1.0e9;
    }
    let uv = vec2<f32>((xz.x - weather.occ.x) / weather.occ.w, 1.0 - (xz.y - weather.occ.z) / weather.occ.w);
    if (any(uv < vec2<f32>(0.0)) || any(uv >= vec2<f32>(1.0))) {
        return -1.0e9;
    }
    let size = i32(weather.occ2.y);
    let t = clamp(vec2<i32>(uv * f32(size)), vec2<i32>(0), vec2<i32>(size - 1));
    let d = textureLoad(weather_occlusion, t, 0);
    if (d >= 1.0) {
        return -1.0e9;
    }
    return weather.occ.y - d * weather.occ2.x;
}

// 1 = open to the sky at `p`, 0 = under a roof, a tree or anything else casting shadows
// (soft across a texel: four taps).
fn weather_exposure(p: vec3<f32>) -> f32 {
    if (weather.info.w < 0.5) {
        return 1.0;
    }
    let size = weather.occ2.y;
    let texel = weather.occ.w / size;
    let uv = vec2<f32>((p.x - weather.occ.x) / weather.occ.w, 1.0 - (p.z - weather.occ.z) / weather.occ.w) * size - 0.5;
    if (any(uv < vec2<f32>(0.0)) || any(uv >= vec2<f32>(size - 1.0))) {
        return 1.0;
    }
    let i = vec2<i32>(floor(uv));
    let f = fract(uv);
    // A surface counts as the cover itself within this much below it.
    let bias = 0.12 + texel * 1.5;
    var e = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let o = vec2<i32>(k & 1, k >> 1);
        let d = textureLoad(weather_occlusion, i + o, 0);
        let top = weather.occ.y - d * weather.occ2.x;
        e[k] = select(0.0, 1.0, d >= 1.0 || p.y >= top - bias);
    }
    return mix(mix(e.x, e.y, f.x), mix(e.z, e.w, f.x), f.y);
}

// ---- noise ----
fn weather_hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn weather_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    // Quintic: no creases along the cell edges.
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = weather_hash2(i);
    let b = weather_hash2(i + vec2<f32>(1.0, 0.0));
    let c = weather_hash2(i + vec2<f32>(0.0, 1.0));
    let d = weather_hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn weather_fbm(p: vec2<f32>) -> f32 {
    // Each octave turned, and the first one bent by a slow warp: thresholded value noise
    // otherwise shows its square cells (snow patches and puddles with straight edges).
    let r = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
    let bend = vec2<f32>(weather_noise(p * 0.47 + 3.7), weather_noise(p * 0.53 + 19.2)) - 0.5;
    let p0 = r * (p + bend * 1.6);
    let q = r * p0 * 2.13 + 7.1;
    // The broad octave is two lattices at 45° to each other: no straight edges or facets
    // where it is thresholded.
    let turned = mat2x2<f32>(0.7071, 0.7071, -0.7071, 0.7071) * p0 * 1.09 + 31.7;
    let broad = (weather_noise(p0) + weather_noise(turned)) * 0.5;
    return broad * 0.55 + weather_noise(q) * 0.3 + weather_noise(r * q * 2.05 + 3.3) * 0.15;
}

// Settled snow on a surface (0..1) with normal `n` at `world`: on upward faces, patchy while
// thin, above the snow line, not under cover.
fn weather_snow_amount(world: vec3<f32>, n: vec3<f32>, cover: f32, exposure: f32) -> f32 {
    if (cover <= 0.0) {
        return 0.0;
    }
    let breakup = weather_fbm(world.xz * 0.6) * 0.5 + weather_fbm(world.xz * 3.1) * 0.25;
    // Thin cover settles first on flat ground, thick cover reaches steeper faces.
    let slope = smoothstep(0.85 - cover * 0.55, 1.0 - cover * 0.35, n.y + (breakup - 0.35) * 0.3);
    let patchy = smoothstep(0.0, 0.25, cover * 1.4 - (1.0 - breakup) * 0.7 + 0.2);
    var above_line = 1.0;
    if (weather.snow_line.x > 0.5) {
        above_line = smoothstep(weather.snow_line.y - 2.0, weather.snow_line.y + 2.0, world.y + breakup * 3.0);
    }
    return clamp(slope * patchy * above_line * exposure, 0.0, 1.0);
}

// Foliage (grass blades, tree leaves and bark): albedo and smoothness with snow and wetness.
fn weather_foliage(world: vec3<f32>, n: vec3<f32>, albedo: vec3<f32>, smoothness: f32) -> vec4<f32> {
    if (!weather_on()) {
        return vec4<f32>(albedo, smoothness);
    }
    let a = weather_surface_at(world);
    if (a.x <= 0.0 && a.z <= 0.0) {
        return vec4<f32>(albedo, smoothness);
    }
    let exposure = weather_exposure(world);
    let snow = weather_snow_amount(world, normalize(n + vec3<f32>(0.0, 0.35, 0.0)), a.z, exposure) * 0.85;
    let wet = a.x * mix(0.35, 1.0, exposure) * (1.0 - snow);
    var c = albedo * mix(1.0, 0.7, wet);
    var g = mix(smoothness, max(smoothness, 0.5), wet);
    c = mix(c, weather.snow.rgb * 0.9, snow);
    g = mix(g, 0.25, snow);
    return vec4<f32>(c, g);
}

// Lightning light on a surface: added as emission (an unshadowed flash from the sky, dimmed
// under cover).
fn weather_flash_light(world: vec3<f32>, n: vec3<f32>, albedo: vec3<f32>, exposure: f32) -> vec3<f32> {
    if (weather.flash.w <= 0.0) {
        return vec3<f32>(0.0);
    }
    let wrap = 0.3 + 0.7 * max(dot(n, weather.flash_dir.xyz), 0.0);
    let sky = 0.5 + 0.5 * max(n.y, 0.0);
    return albedo * weather.flash.rgb * wrap * sky * mix(0.12, 1.0, exposure);
}
