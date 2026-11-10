// Skybox: procedural sky (see `sky_color` in common.wgsl) with a sun disc, a solid colour, or
// an HDRI (its background cube, rotated and tinted). The moon and the stars are drawn here too
// (render/moon.rs), before the clouds and fog are blended over the sky.
// The camera's solid background replaces it when the skybox is off. Fog hazes the horizon;
// volumetric / height fog covers both at the max fog distance (fog_apply.wgsl).

struct SkyTargets {
    @location(0) color: vec4<f32>,
    // The sun disc glows through bloom.
    @location(1) glow: vec4<f32>,
    // Motion vector of the sky (camera rotation).
    @location(2) velocity: vec2<f32>,
};

// Sun disc (and halo in High Quality) seen in `dir`; zero when there is no visible sun.
fn sun_disc(dir: vec3<f32>) -> vec3<f32> {
    let mode = view.sun_color.w;
    // An HDRI has its own sun in the image.
    if (view.sun_dir.w < 0.5 || mode < 0.5 || view.sky_solid.w > 0.5 || sky_is_hdri()) {
        return vec3<f32>(0.0);
    }
    let cos_a = dot(dir, view.sun_dir.xyz);
    let r = view.sky_misc.x;
    var light = smoothstep(cos(r), cos(r * 0.75), cos_a) * 6.0;
    if (mode > 1.5) {
        let c = max(cos_a, 0.0);
        light += pow(c, 600.0) * 0.6 + pow(c, 24.0) * 0.06;
    }
    // Low suns turn orange; the ground hides them.
    let warm = mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.45, 0.15), sky_sunset());
    return view.sun_color.rgb * warm * light * smoothstep(-0.02, 0.01, dir.y);
}

// The moon's texture (render/moon.rs; sampled with `sky_sampler`).
@group(0) @binding(11) var moon_tex: texture_2d<f32>;

// Angle one pixel covers at the centre of the view (radians).
fn pixel_angle() -> f32 {
    if (view.view_tan.y <= 0.0) {
        return 0.001;
    }
    return 2.0 * view.view_tan.y / max(view.motion.w, 1.0);
}

// Height of the built-in texture's alpha step (moon/texture_gen.rs HEIGHT_SCALE), in radians
// of the moon's radius.
const MOON_HEIGHT_STEP: f32 = 0.00012 * 255.0;

// Moon-local direction (x east, y north, z towards the viewer) → world.
fn moon_world(v: vec3<f32>) -> vec3<f32> {
    return v.x * sky_env.moon_right.xyz + v.y * sky_env.moon_up.xyz - v.z * sky_env.moon_dir.xyz;
}

// The moon seen in `dir`: rgb = radiance added over the sky (the sky's in-scatter is in front
// of it, so its dark side shows the sky), a = how much of the pixel it covers (hides stars).
// A sphere lit from `moon_light`: the phase, terminator and earthshine come from the lighting,
// one texture serves every phase.
fn moon_disc(dir: vec3<f32>) -> vec4<f32> {
    let r = sky_env.moon_dir.w;
    if (r <= 0.0) {
        return vec4<f32>(0.0);
    }
    let m = sky_env.moon_dir.xyz;
    let px = pixel_angle();
    if (dot(dir, m) < cos(r + 2.0 * px)) {
        return vec4<f32>(0.0);
    }
    let sr = sin(r);
    let p = vec2<f32>(dot(dir, sky_env.moon_right.xyz), dot(dir, sky_env.moon_up.xyz)) / sr;
    let d = length(p);
    let edge = px / sr;
    let cover = 1.0 - smoothstep(1.0 - edge, 1.0 + edge * 0.5, d);
    if (cover <= 0.0) {
        return vec4<f32>(0.0);
    }
    let q = p / max(d, 1.0);
    let z = sqrt(max(1.0 - dot(q, q), 0.0));
    let lon = atan2(q.x, z);
    let lat = asin(clamp(q.y, -1.0, 1.0));
    let flags = sky_env.moon_up.w;
    let disc_image = flags > 1.5;
    let has_height = fract(flags * 0.5) > 0.25;
    var uv: vec2<f32>;
    if (disc_image) {
        uv = vec2<f32>(0.5 + 0.5 * q.x, 0.5 - 0.5 * q.y);
    } else {
        uv = vec2<f32>(0.5 + lon / 6.2831853, 0.5 - lat / 3.1415927);
    }
    // Mip level from texels per pixel across the disc.
    let texels = sky_env.moon_right.w;
    let lod = max(log2(max(texels * px / (2.0 * r), 1e-6)), 0.0);
    let t = textureSampleLevel(moon_tex, sky_sampler, uv, lod);
    let albedo = t.rgb;
    let n0 = moon_world(vec3<f32>(q.x, q.y, z));
    var n = n0;
    if (has_height && !disc_image) {
        // Crater relief from the height map: slopes along longitude and latitude.
        let size = vec2<f32>(texels * 2.0, texels);
        let step = exp2(lod) / size;
        let hx = textureSampleLevel(moon_tex, sky_sampler, uv + vec2<f32>(step.x, 0.0), lod).a
            - textureSampleLevel(moon_tex, sky_sampler, uv - vec2<f32>(step.x, 0.0), lod).a;
        let hy = textureSampleLevel(moon_tex, sky_sampler, uv - vec2<f32>(0.0, step.y), lod).a
            - textureSampleLevel(moon_tex, sky_sampler, uv + vec2<f32>(0.0, step.y), lod).a;
        let dlon = hx * MOON_HEIGHT_STEP / (2.0 * step.x * 6.2831853);
        let dlat = hy * MOON_HEIGHT_STEP / (2.0 * step.y * 3.1415927);
        let east = vec3<f32>(cos(lon), 0.0, -sin(lon));
        let north = vec3<f32>(-sin(lat) * sin(lon), cos(lat), -sin(lat) * cos(lon));
        let tilt = east * (dlon / max(cos(lat), 0.05)) + north * dlat;
        n = normalize(n0 - moon_world(tilt));
    }
    let s = sky_env.moon_light.xyz;
    // Lommel–Seeliger with some Lambert: the full moon is evenly bright to its limb.
    let lit = max(dot(n, s), 0.0);
    let mu = max(dot(n, -m), 0.0);
    let shade = mix(lit, 2.0 * lit / (lit + mu + 1e-3), 0.75);
    // A soft terminator (the smooth sphere's), so the relief does not light the night side.
    let day = smoothstep(-0.04, 0.06, dot(n0, s));
    let earthshine = sky_env.moon_light.w;
    let radiance = albedo * (shade * day + earthshine) * sky_env.moon_color.rgb;
    return vec4<f32>(radiance * cover, cover);
}

fn star_hash(p: vec3<u32>) -> vec4<f32> {
    var v = p * vec3<u32>(1664525u, 1013904223u, 2654435761u) + vec3<u32>(1013904223u, 1664525u, 374761393u);
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v = v ^ (v >> vec3<u32>(16u));
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    let w = v.x ^ v.y ^ v.z;
    return vec4<f32>(vec4<u32>(v, w * 747796405u) >> vec4<u32>(8u)) / 16777216.0;
}

// ---- Night sky: stars, Milky Way, aurora, meteors (render/moon.rs `night_params`) ----

// Direction on the celestial sphere (the stars and the Milky Way turn with the sky).
fn celestial(dir: vec3<f32>) -> vec3<f32> {
    let n = sky_env.night;
    return vec3<f32>(dot(dir, n[3].xyz), dot(dir, n[4].xyz), dot(dir, n[5].xyz));
}

// A fixed star field on the celestial sphere (cube-face cells, one possible star each, at
// least about a pixel wide so TAA keeps it still): a power law of brightness, colour
// temperatures, twinkle stronger near the horizon. rgb = radiance, a = its bloom share.
fn star_field(dir: vec3<f32>) -> vec4<f32> {
    let k = sky_env.night_horizon.w;
    if (k <= 0.0 || dir.y < -0.02) {
        return vec4<f32>(0.0);
    }
    let n = sky_env.night;
    let e = celestial(dir);
    let a = abs(e);
    var uv: vec2<f32>;
    var face: u32;
    if (a.x >= a.y && a.x >= a.z) {
        uv = e.zy / a.x;
        face = select(0u, 1u, e.x < 0.0);
    } else if (a.y >= a.z) {
        uv = e.xz / a.y;
        face = select(2u, 3u, e.y < 0.0);
    } else {
        uv = e.xy / a.z;
        face = select(4u, 5u, e.z < 0.0);
    }
    let cells = 220.0;
    let g = (uv * 0.5 + 0.5) * cells;
    let cell = floor(g);
    let h = star_hash(vec3<u32>(vec2<u32>(cell), face));
    if (h.x > min(0.3 * n[0].x, 0.95)) {
        return vec4<f32>(0.0);
    }
    let pos = vec2<f32>(0.25) + 0.5 * h.yz;
    // Angular distance from the star (a cell is about 90° / cells across).
    let cell_angle = 1.5707963 / cells;
    let dist = length(g - cell - pos) * cell_angle;
    // About a pixel wide whatever the zoom, so a star keeps its brightness.
    let sigma = max(pixel_angle() * 0.6, 0.00005);
    // Magnitudes: many faint stars; the spread adds a few very bright ones.
    let spread = n[0].z;
    let lum = 0.012 + 0.22 * pow(h.w, 6.0) + spread * 5.0 * pow(h.w, 28.0);
    var glow = exp(-(dist * dist) / (sigma * sigma)) * lum;
    // A soft halo on the bright ones (kept inside the cell; bloom spreads the rest).
    let halo = min(sigma * (2.0 + 4.0 * n[0].w), cell_angle * 0.12);
    glow += n[0].w * 0.2 * smoothstep(0.3, 2.0, lum) * lum * exp(-(dist * dist) / (halo * halo));
    // Colour temperature: orange (cool stars) to blue-white (hot stars).
    let t = fract(h.x * 11.31 + h.y * 3.7);
    let temp = mix(vec3<f32>(1.0, 0.7, 0.42), vec3<f32>(0.66, 0.79, 1.0), t);
    let tint = mix(vec3<f32>(1.0), temp, n[1].x);
    // Twinkle: two incommensurate waves per star, stronger through the air near the horizon.
    if (n[1].y > 0.0) {
        let tt = n[1].w * n[1].z;
        let w = sin(tt * (5.0 + 7.0 * h.y) + h.z * 40.0) * 0.6
            + sin(tt * (13.0 + 9.0 * h.z) + h.y * 31.0) * 0.4;
        let air = mix(0.35, 1.0, pow(1.0 - clamp(dir.y, 0.0, 1.0), 3.0));
        glow *= max(1.0 + n[1].y * air * w, 0.0);
    }
    let horizon = smoothstep(-0.02, 0.2, dir.y);
    let c = tint * glow * k * horizon * view.sky_ground.w;
    return vec4<f32>(c, n[0].w * smoothstep(0.5, 3.0, lum));
}

fn hash31(p: vec3<f32>) -> f32 {
    let q = vec3<u32>(vec3<i32>(floor(p)) + vec3<i32>(4096));
    return star_hash(q).x;
}

// Smooth 3D value noise in 0..1.
fn vnoise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash31(i), hash31(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let b = mix(hash31(i + vec3<f32>(0.0, 1.0, 0.0)), hash31(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let c = mix(hash31(i + vec3<f32>(0.0, 0.0, 1.0)), hash31(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let d = mix(hash31(i + vec3<f32>(0.0, 1.0, 1.0)), hash31(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}

fn fbm3(p: vec3<f32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    var q = p;
    for (var o = 0; o < octaves; o += 1) {
        sum += vnoise3(q) * amp;
        norm += amp;
        amp *= 0.5;
        q = q * 2.07 + vec3<f32>(1.7, 9.2, 4.1);
    }
    return sum / norm;
}

// The Milky Way in galactic coordinates: a band widening and brightening towards the core in
// Sagittarius, star clouds, and the dark dust lanes of the Great Rift along its middle.
fn milky_way(dir: vec3<f32>) -> vec3<f32> {
    let n = sky_env.night;
    let k = n[3].w;
    if (k <= 0.0 || dir.y < -0.02) {
        return vec3<f32>(0.0);
    }
    let g = vec3<f32>(dot(dir, n[6].xyz), dot(dir, n[7].xyz), dot(dir, n[8].xyz));
    let b = asin(clamp(g.z, -1.0, 1.0));
    let l = atan2(g.y, g.x);
    let width = n[6].w;
    let high = n[2].y > 0.5;
    let centre = exp(-(l * l) / (2.0 * 0.75 * 0.75));
    let w = 0.1 * width * (1.0 + 1.3 * centre);
    let band = exp(-(b * b) / (w * w));
    let core = n[4].w * exp(-(l * l + (b * 2.4) * (b * 2.4)) / (2.0 * 0.2 * 0.2));
    if (band + core < 0.002) {
        return vec3<f32>(0.0);
    }
    let octaves = select(3, 5, high);
    let clouds = fbm3(g * 7.0, octaves);
    var lum = band * (0.45 + 0.75 * centre) * (0.35 + 1.3 * clouds * clouds) + core * (0.5 + clouds);
    // Dust lanes just off the plane, broken up.
    let lane = (b - 0.025 * sin(l * 2.0 + 0.6)) / (0.05 * width * (1.0 + centre));
    let rift = exp(-lane * lane) * smoothstep(0.38, 0.7, fbm3(g * 5.0 + vec3<f32>(3.1), octaves));
    lum *= 1.0 - clamp(n[5].w * rift * 0.9, 0.0, 0.95);
    if (high) {
        // Fine grain of unresolved stars.
        lum *= 0.8 + 0.4 * vnoise3(g * 90.0);
    }
    let warm = clamp(centre * 0.8 + core * 0.4, 0.0, 1.0);
    let col = mix(vec3<f32>(0.72, 0.8, 1.0), vec3<f32>(1.0, 0.85, 0.64), warm) * n[9].rgb;
    let horizon = smoothstep(-0.02, 0.25, dir.y);
    return col * lum * k * 0.03 * horizon * view.sky_ground.w;
}

fn hash11(x: f32) -> f32 {
    return star_hash(vec3<u32>(u32(i32(floor(x)) + 65536), 17u, 3u)).x;
}

fn vnoise1(x: f32) -> f32 {
    let i = floor(x);
    let f = x - i;
    return mix(hash11(i), hash11(i + 1.0), f * f * (3.0 - 2.0 * f));
}

// The aurora: a slab of glowing curtains between `height` and `height + thickness` km above
// the ground, centred `distance` km away towards its direction, ray-marched (emission only):
// folds that wave and drift, vertical rays, a sharp lower edge, green below and the top colour
// above, pulsing. `jitter` (0..1 per pixel) hides the step pattern (TAA smooths it).
fn aurora(dir: vec3<f32>, jitter: f32) -> vec3<f32> {
    let n = sky_env.night;
    let k = n[9].w;
    if (k <= 0.0 || dir.y < 0.01) {
        return vec3<f32>(0.0);
    }
    let h0 = n[10].w;
    let thick = n[11].w;
    let fwd = vec2<f32>(n[12].x, n[12].y);
    let right = vec2<f32>(fwd.y, -fwd.x);
    let dist = n[12].z;
    let curtains = i32(n[12].w);
    let wavy = n[13].x;
    let rays = n[13].y;
    let time = n[1].w * n[13].z;
    let t0 = h0 / dir.y;
    let t1 = (h0 + thick) / dir.y;
    let d2 = dir.xz;
    // Skip rays that pass outside the display (across: ±(curtains·45 + folds) km, along: ±1500).
    let reach = f32(curtains) * 25.0 + 120.0 * wavy + 40.0;
    let qa = vec2<f32>(dot(d2 * t0, right), dot(d2 * t0, fwd) - dist);
    let qb = vec2<f32>(dot(d2 * t1, right), dot(d2 * t1, fwd) - dist);
    if (min(qa.y, qb.y) > reach || max(qa.y, qb.y) < -reach
        || min(qa.x, qb.x) > 1500.0 || max(qa.x, qb.x) < -1500.0) {
        return vec3<f32>(0.0);
    }
    let steps = select(10, 24, n[2].y > 0.5);
    let dt = (t1 - t0) / f32(steps);
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < steps; i += 1) {
        let t = t0 + (f32(i) + jitter) * dt;
        let hf = (dir.y * t - h0) / thick;
        let p = d2 * t;
        let q = vec2<f32>(dot(p, right), dot(p, fwd) - dist);
        let along = smoothstep(1500.0, 900.0, abs(q.x));
        if (along <= 0.0) {
            continue;
        }
        var dens = 0.0;
        for (var c = 0; c < curtains; c += 1) {
            let fc = f32(c);
            let base = (fc - f32(curtains - 1) * 0.5) * 50.0;
            let fold = wavy * (70.0 * sin(q.x * 0.0035 + time * 0.07 + fc * 1.7)
                + 28.0 * sin(q.x * 0.0105 - time * 0.16 + fc * 4.1)
                + 11.0 * sin(q.x * 0.031 + time * 0.37 + fc * 2.3));
            let dv = q.y - base - fold;
            let sheet = exp(-dv * dv / (18.0 * 18.0));
            if (sheet < 0.01) {
                continue;
            }
            // Vertical rays: brightness varying along the curtain, drifting.
            let x = q.x * 0.05 * max(rays, 0.05) + fc * 13.7 - time * 0.4;
            let r = vnoise1(x) * 0.65 + vnoise1(x * 2.7 + 5.0) * 0.35;
            let ray = mix(1.0, pow(r, 2.2) * 2.6, clamp(rays, 0.0, 1.0));
            dens += sheet * ray * (0.6 + 0.4 * vnoise1(q.x * 0.002 + fc * 7.0 + time * 0.05));
        }
        let profile = smoothstep(0.0, 0.05, hf) * exp(-hf * 2.4);
        let col = mix(n[10].rgb, n[11].rgb, smoothstep(0.2, 0.85, hf));
        acc += col * dens * profile * along;
    }
    let pulse = 1.0 + n[13].w * (0.3 * sin(time * 0.9) + 0.2 * sin(time * 2.3 + 1.0)
        + 0.12 * sin(time * 6.1 + 2.0));
    return acc * dt * 0.012 * k * max(pulse, 0.0) * view.sky_ground.w;
}

// Shooting stars: one may cross the sky in each `60 / rate` seconds; a bright head and a
// fading tail along a great circle.
fn meteors(dir: vec3<f32>) -> vec3<f32> {
    let n = sky_env.night;
    let rate = n[2].z;
    let bright = n[2].w;
    if (rate <= 0.0 || bright <= 0.0 || dir.y < 0.0) {
        return vec3<f32>(0.0);
    }
    let period = 60.0 / rate;
    let now = n[1].w;
    let px = pixel_angle();
    var acc = 0.0;
    for (var s = 0; s < 2; s += 1) {
        let slot = floor(now / period) - f32(s);
        if (slot < 0.0) {
            continue;
        }
        let h = star_hash(vec3<u32>(u32(slot), 101u, 7u));
        let start = h.x * period * 0.7;
        let duration = 0.45 + 0.6 * h.y;
        let age = (now - slot * period - start) / duration;
        if (age < 0.0 || age > 1.0) {
            continue;
        }
        let h2 = star_hash(vec3<u32>(u32(slot), 202u, 9u));
        let az = h.z * 6.2831853;
        let alt = 0.35 + 0.8 * h.w;
        let s0 = vec3<f32>(sin(az) * cos(alt), sin(alt), cos(az) * cos(alt));
        // Heading across the sky, mostly downwards.
        let east = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), s0) + vec3<f32>(1e-4, 0.0, 0.0));
        let down = cross(s0, east);
        let ang = (h2.x - 0.5) * 2.4;
        let tangent = normalize(east * sin(ang) - down * cos(ang));
        let normal = cross(s0, tangent);
        let path = 0.18 + 0.25 * h2.y;
        let head = path * age;
        let tail = min(0.1, head);
        let along = atan2(dot(dir, tangent), dot(dir, s0));
        let across = dot(dir, normal);
        if (along > head || along < head - tail) {
            continue;
        }
        let w = max(px * 0.8, 0.0003);
        let fade = sin(age * 3.1415927);
        let toward_head = (along - (head - tail)) / max(tail, 1e-4);
        acc += exp(-(across * across) / (w * w)) * toward_head * toward_head * fade
            * (0.6 + 0.8 * h2.z);
    }
    return vec3<f32>(1.0, 0.94, 0.82) * acc * bright * 3.0 * view.sky_ground.w;
}

// The full-screen triangle on the far plane: the sky drawn after the opaque scene with a depth
// test shades only the samples nothing covers (render/mod.rs `main_pass`).
@vertex
fn vs_sky_far(@builtin(vertex_index) i: u32) -> FullscreenOut {
    // `vs_fullscreen`'s triangle (an entry point can't be called).
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    let ndc = uv * 2.0 - 1.0;
    var out: FullscreenOut;
    out.pos = vec4<f32>(ndc, 1.0, 1.0);
    out.ndc = ndc;
    return out;
}

@fragment
fn fs_main(in: FullscreenOut) -> SkyTargets {
    var out: SkyTargets;
    out.glow = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let dir = view_ray(in.ndc);
    out.velocity = sky_velocity(dir, in.ndc);
    let vf = vfog_sky(dir);
    if (view.background.w < 0.5) {
        out.color = vec4<f32>(vfog_blend(view.background.rgb, vf, 1.0), 1.0);
        return out;
    }
    var sun = sun_disc(dir);
    var c = sky_color(dir) + sun;
    if (sky_is_hdri()) {
        // The HDRI's very bright texels (its sun) glow through bloom like the procedural disc.
        sun = max(c - vec3<f32>(8.0), vec3<f32>(0.0));
    }
    // The moon (hidden below the horizon) in front of the stars and the Milky Way; a bright
    // (moonlit, twilight, light-polluted) sky washes the faint ones out (Star Fade).
    let moon = moon_disc(dir) * smoothstep(-0.02, 0.01, dir.y);
    if (sky_env.night_zenith.w > 0.0) {
        let sky_lum = dot(c, vec3<f32>(0.3, 0.6, 0.1)) / max(view.sky_ground.w, 1e-3);
        let wash = pow(clamp(0.012 / max(sky_lum, 1e-4), 0.0, 1.0), sky_env.night[2].x);
        let stars = star_field(dir);
        let behind = (stars.rgb + milky_way(dir)) * wash * (1.0 - moon.a);
        // The aurora is bright: the sky dims it only a little. Meteors are in front of the moon.
        // Per pixel and per frame (the TAA jitter changes every frame): TAA blends the steps.
        let seed = in.pos.xy + view.motion.xy * vec2<f32>(4129.0, 2311.0);
        let jitter = fract(52.9829189 * fract(dot(seed, vec2<f32>(0.06711056, 0.00583715))));
        let glow = aurora(dir, jitter) * mix(1.0, wash, 0.4) + meteors(dir);
        c += behind + glow;
        sun += stars.rgb * stars.a * wash + glow * 0.4;
    }
    c += moon.rgb;
    sun += moon.rgb * 0.3;
    var visible = 1.0;
    if (view.fog_color.w > 0.5) {
        // Fog thickens towards the horizon and fills the view below it.
        let band = pow(1.0 - max(dir.y, 0.0), 8.0);
        let amount = (1.0 - fog_visibility(view.sky_misc.y)) * band;
        c = mix(c, view.fog_color.rgb, amount);
        visible = 1.0 - amount;
    }
    out.color = vec4<f32>(vfog_blend(c, vf, 1.0), 1.0);
    out.glow = vec4<f32>(sun * 0.15 * visible * vfog_glow(vf), 1.0);
    return out;
}
