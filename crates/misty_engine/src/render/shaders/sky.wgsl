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

// A fixed star field (cube-face cells, one possible star each, its size at least about a
// pixel so it holds still under TAA), fading in at night and out towards the horizon.
fn star_field(dir: vec3<f32>) -> vec3<f32> {
    let k = sky_env.night_horizon.w;
    if (k <= 0.0 || dir.y < -0.02) {
        return vec3<f32>(0.0);
    }
    let a = abs(dir);
    var uv: vec2<f32>;
    var face: u32;
    if (a.x >= a.y && a.x >= a.z) {
        uv = dir.zy / a.x;
        face = select(0u, 1u, dir.x < 0.0);
    } else if (a.y >= a.z) {
        uv = dir.xz / a.y;
        face = select(2u, 3u, dir.y < 0.0);
    } else {
        uv = dir.xy / a.z;
        face = select(4u, 5u, dir.z < 0.0);
    }
    let cells = 220.0;
    let g = (uv * 0.5 + 0.5) * cells;
    let cell = floor(g);
    let h = star_hash(vec3<u32>(vec2<u32>(cell), face));
    if (h.x > 0.45) {
        return vec3<f32>(0.0);
    }
    let pos = vec2<f32>(0.25) + 0.5 * h.yz;
    // Angular distance from the star (a cell is about 90° / cells across).
    let cell_angle = 1.5707963 / cells;
    let dist = length(g - cell - pos) * cell_angle;
    let px = pixel_angle();
    let sigma = max(px * 0.75, 0.00012);
    // Most stars are faint, a few bright.
    let mag = pow(h.w, 14.0) * 6.0 + pow(h.w, 3.0) * 0.25 + 0.02;
    // The same energy whatever the pixel size: a wider blob is dimmer.
    let energy = (0.0006 / sigma) * (0.0006 / sigma);
    let glow = exp(-(dist * dist) / (sigma * sigma)) * mag * min(energy, 4.0);
    let tint = mix(vec3<f32>(1.0, 0.82, 0.62), vec3<f32>(0.72, 0.84, 1.0), fract(h.x * 7.31));
    let horizon = smoothstep(-0.02, 0.2, dir.y);
    return tint * glow * k * horizon * view.sky_ground.w;
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
    // The moon (hidden below the horizon) in front of the stars.
    let moon = moon_disc(dir) * smoothstep(-0.02, 0.01, dir.y);
    c += moon.rgb + star_field(dir) * (1.0 - moon.a);
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
