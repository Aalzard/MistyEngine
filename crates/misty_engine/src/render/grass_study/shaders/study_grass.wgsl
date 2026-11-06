// Grass study: the blades' vertex and fragment stages. The forward passes (variant A and
// those built on it) are a copy of render/shaders/grass.wgsl (main a91ac1e) with the study
// lighting; the visibility passes (B, C, F) rasterise only a blade id and the point on its
// card, and `grass_surface` shades it later (study_resolve.wgsl), once per pixel.

const CELL_AVG = array<vec3<f32>, 8>(
    vec3<f32>(0.176, 0.236, 0.038),
    vec3<f32>(0.185, 0.244, 0.040),
    vec3<f32>(0.175, 0.238, 0.037),
    vec3<f32>(0.179, 0.239, 0.039),
    vec3<f32>(0.372, 0.279, 0.076),
    vec3<f32>(0.369, 0.301, 0.090),
    vec3<f32>(0.232, 0.197, 0.061),
    vec3<f32>(0.326, 0.257, 0.085),
);

// Single-sample targets: alpha to coverage can't work, so coverage becomes a per-pixel,
// per-frame noise threshold that TAA averages (A1, the visibility buffer).
override STOCHASTIC: bool = false;
// The visibility raster leaves blades thinner than `study.params.x` pixels to the software
// rasteriser (variant C).
override SWR: bool = false;
// Samples per pixel of the multisampled visibility raster (B4: 4, B2: 2).
override MS_SAMPLES: u32 = 4u;

// What a blade's vertex stage hands the lighting (GrassOut without the clip positions).
struct GrassAttrs {
    world: vec3<f32>,
    normal: vec3<f32>,
    color: vec3<f32>,
    uv: vec4<f32>,
    cell: u32,
    misc: vec4<f32>,
    across: vec3<f32>,
    canopy: vec4<f32>,
};

struct GrassOut {
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) uv: vec4<f32>,
    @location(4) @interpolate(flat) cell: u32,
    @location(5) misc: vec4<f32>,
    @location(6) across: vec3<f32>,
    @location(7) clip_prev: vec3<f32>,
    @location(8) canopy: vec4<f32>,
};

struct MeshTargets {
    @location(0) color: vec4<f32>,
    @location(1) glow: vec4<f32>,
    @location(2) velocity: vec2<f32>,
};

fn canopy_top(b: BladeIn) -> vec4<f32> {
    let relief = vec3<f32>(b.field.w, b.slope);
    let span = grass.canopy.y - grass.canopy.x;
    let top = (grass.canopy.x + span * relief.x) * 0.95;
    return vec4<f32>(max(top, 0.02), span * relief.yz, relief.x);
}

const GRASS_SKIP = vec4<f32>(2.0, 2.0, 2.0, 1.0);

// Projected width of the blade in pixels (before the one-pixel minimum).
fn blade_pixels(v: BladeVertex) -> f32 {
    return v.width / max(v.dist * grass.screen.x, 1e-6);
}

fn grass_attrs(v: BladeVertex, b: BladeIn) -> GrassAttrs {
    var out: GrassAttrs;
    let face = normalize(cross(v.across, v.tangent));
    out.world = v.p;
    out.normal = normalize(face + v.across * v.side * 0.8);
    let far = smoothstep(5.0, 55.0, v.dist);
    let phase = b.shape.w;
    let dry_mix = mix(b.shape.z, 0.2 + 0.4 * b.shape.z, far * 0.7);
    let patches = b.field.xy;
    var base = grass_base_color(patches, dry_mix, grass.healthy.rgb, grass.dry.rgb, grass.lush.rgb, grass.amounts.xy);
    base = grass_swathe_tint(base, b.field.z) * grass_far_tint(v.dist);
    let cv = grass.look.w * (1.0 - 0.5 * far);
    let tint = b.extra.w;
    let hue = (fract(phase * 13.7) - 0.5) * 0.3 + tint * 1.0;
    let value = (fract(phase * 7.31 + 0.37) - 0.5) * 0.2 + tint * 0.5;
    base *= vec3<f32>(1.0 + 0.22 * hue * cv, 1.0 + 0.02 * hue * cv, 1.0 - 0.32 * hue * cv)
        * max(1.0 + 0.4 * value * cv, 0.3);
    let seed_color = grass.seed.rgb * (1.0 + 0.12 * tint)
        * mix(vec3<f32>(1.0), vec3<f32>(1.06, 1.0, 0.85), patches.y * grass.amounts.y);
    out.color = select(base, seed_color, v.seed_head);
    let top = canopy_top(b);
    out.misc = vec4<f32>(far, v.above / top.x, v.gust, v.thin);
    out.canopy = vec4<f32>(normalize(vec3<f32>(-top.y * 8.0, 1.0, -top.z * 8.0)), top.w);
    out.uv = vec4<f32>(grass_card_u(v.cell, v.side, v.repeat), grass_card_v(v.t, phase), v.t, v.fade);
    out.cell = v.cell;
    out.across = v.across * v.side * 2.0;
    return out;
}

fn grass_vertex(vi: u32, b: BladeIn) -> GrassOut {
    var out: GrassOut;
    let v = grass_blade(vi, b);
    let a = grass_attrs(v, b);
    out.clip = study.view_proj * vec4<f32>(v.p, 1.0);
    var prev = v.p;
    if (grass.screen.w > 0.0) {
        prev = grass_blade_at(vi, b, grass.origin.w - grass.screen.w).p;
    }
    let pc = study.prev_view_proj * vec4<f32>(prev, 1.0);
    out.clip_prev = pc.xyw;
    out.world = a.world;
    out.normal = a.normal;
    out.color = a.color;
    out.uv = a.uv;
    out.cell = a.cell;
    out.misc = a.misc;
    out.across = a.across;
    out.canopy = a.canopy;
    return out;
}

fn study_skip(b: BladeIn) -> bool {
    let root = grass.origin.xyz + b.position.xyz;
    return study_split_skip(b, distance(root, grass.camera.xyz));
}

@vertex
fn vs_grass(@builtin(vertex_index) vi: u32, b: BladeIn) -> GrassOut {
    if (grass_outside(b, study.view_proj, true) || study_skip(b)) {
        var out: GrassOut;
        out.clip = GRASS_SKIP;
        return out;
    }
    return grass_vertex(vi, b);
}

@vertex
fn vs_grass_depth(@builtin(vertex_index) vi: u32, b: BladeIn) -> GrassOut {
    if (grass_flat(b) || grass_outside(b, study.view_proj, true) || study_skip(b)) {
        var out: GrassOut;
        out.clip = GRASS_SKIP;
        return out;
    }
    return grass_vertex(vi, b);
}

fn grass_coverage(misc: vec4<f32>, alpha: f32, fw: f32, pixel: vec2<f32>) -> f32 {
    let far = misc.x;
    let sharp = clamp((alpha - 0.5) / max(fw, 1e-4) + 0.5, 0.0, 1.0);
    var coverage = mix(sharp, clamp(alpha * 1.6, 0.0, 1.0), mix(0.5, 1.0, far)) * misc.w;
    if (study.frame.y > 0u && coverage < 0.98) {
        let n = study_hash(pixel + fract(f32(study.frame.x % 64u) * 0.137) * vec2<f32>(71.3, 37.9));
        coverage = clamp(coverage + (n - 0.5) * 0.3, 0.0, 1.0);
    }
    return coverage;
}

// Single-sample: keep the fragment with probability `coverage`.
fn stochastic_keep(coverage: f32, pixel: vec2<f32>) -> bool {
    return coverage > frame_noise(pixel * 1.37 + 3.1);
}

@fragment
fn fs_grass_depth(in: GrassOut, @builtin(sample_mask) mask: u32) -> MeshTargets {
    count_quad(0u, mask);
    let auv = grass_atlas_uv(in.uv.xy);
    let alpha = textureSampleGrad(grass_atlas, grass_sampler, auv.xy, grass_atlas_dx(in.uv.xy), grass_atlas_dy(in.uv.xy)).a * auv.z;
    let coverage = grass_coverage(in.misc, alpha, fwidth(alpha), in.clip.xy);
    if (coverage < 0.01 || (STOCHASTIC && !stochastic_keep(coverage, in.clip.xy))) {
        discard;
    }
    count_add(2u, 1u);
    var out: MeshTargets;
    out.color = vec4<f32>(0.0, 0.0, 0.0, coverage);
    out.glow = vec4<f32>(0.0);
    return out;
}

// The lit colour of a blade surface point: grass.wgsl `fs_grass` after the coverage test.
// `tex` = the atlas texel, `next` = the alpha two texels across.
fn grass_surface(a: GrassAttrs, front: bool, pixel: vec2<f32>, tex: vec4<f32>, next: f32, auv: vec2<f32>) -> vec3<f32> {
    let far = a.misc.x;
    let t = a.uv.z;
    let cell = a.cell;
    let seed_head = cell >= 4u;
    let avg = CELL_AVG[cell];
    let detail = clamp(tex.rgb / max(avg, vec3<f32>(0.01)), vec3<f32>(0.25), vec3<f32>(2.5));
    let lum = dot(tex.rgb, vec3<f32>(0.2126, 0.7152, 0.0722))
        / max(dot(avg, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.01);
    var color = a.color;
    if (!seed_head) {
        let dry_tip = smoothstep(0.45, 1.0, t) * grass.look.y * (0.45 + 0.55 * smoothstep(0.2, 0.9, fract(f32(cell) * 0.37 + a.color.g * 9.0)));
        color = mix(color, grass.dry.rgb, dry_tip);
        color *= mix(vec3<f32>(0.82, 0.95, 0.78), vec3<f32>(1.1, 1.08, 1.0), t);
    } else {
        color = mix(mix(grass.healthy.rgb, grass.dry.rgb, 0.5), color, smoothstep(0.35, 0.6, t));
    }
    let textured = mix(vec3<f32>(lum), detail, 0.5);
    var albedo = clamp(color * mix(textured, vec3<f32>(1.0), 0.6 + far * 0.4), vec3<f32>(0.0), vec3<f32>(1.0));
    let near = 1.0 - far;
    let slope = next - tex.a;
    let thick = smoothstep(0.45, 1.0, tex.a);
    if (!seed_head) {
        let ul = fract(auv.x * 8.0);
        let veins = pow(0.5 + 0.5 * cos(ul * 6.2831853 * 34.0), 6.0);
        let midrib = thick * (1.0 - smoothstep(0.0, 0.08, abs(slope)));
        let size = vec2<f32>(textureDimensions(grass_atlas));
        let speck = step(0.985, grass_hash(floor(auv * size * vec2<f32>(0.5, 0.125))));
        albedo *= 1.0 + near * (0.05 * midrib - 0.03 * veins - 0.05 * speck);
    }
    var n = normalize(a.normal);
    if (!front) {
        n = -n;
    }
    let side_dir = select(-a.across, a.across, front);
    n = normalize(n - side_dir * clamp(slope * 1.5, -0.3, 0.3) * near);
    n = normalize(mix(n, normalize(a.canopy.xyz), mix(0.7, 0.8, far)));
    let gust = a.misc.z * grass.amounts.z;
    albedo *= 1.0 + 0.16 * gust;
    let smoothness = (mix(select(0.08, 0.06, seed_head), 0.04, far) + 0.05 * gust) * a.uv.w;
    let canopy = smoothstep(0.0, 0.85, a.misc.y);
    let dip = mix(0.84, 1.06, a.canopy.w);
    let occlusion = mix(0.4, 1.0, canopy) * dip;
    let lit = albedo * mix(0.62, 1.0, canopy) * dip;
    let transmission = albedo * grass.seed.w * select(1.0, 0.6, seed_head) * mix(0.15, 1.0, canopy)
        * mix(1.35, 0.8, thick);
    return study_shade(a.world, pixel, lit, n, occlusion, transmission, smoothness, clamp(a.misc.y, 0.0, 1.0) * 2.0);
}

@fragment
fn fs_grass(in: GrassOut, @builtin(front_facing) front: bool, @builtin(sample_mask) mask: u32) -> MeshTargets {
    count_quad(4u, mask);
    let leaf = grass_atlas_uv(in.uv.xy);
    let auv = leaf.xy;
    let gx = grass_atlas_dx(in.uv.xy);
    let gy = grass_atlas_dy(in.uv.xy);
    var tex = textureSampleGrad(grass_atlas, grass_sampler, auv, gx, gy);
    tex.a *= leaf.z;
    let across_uv = vec2<f32>(2.0 / f32(textureDimensions(grass_atlas).x), 0.0);
    let next = textureSampleGrad(grass_atlas, grass_sampler, auv + across_uv, gx, gy).a;
    let coverage = grass_coverage(in.misc, tex.a, fwidth(tex.a), in.clip.xy);
    if (coverage < 0.01 || (STOCHASTIC && !stochastic_keep(coverage, in.clip.xy))) {
        discard;
    }
    count_add(6u, 1u);
    count_add(7u, countOneBits(mask));
    var a: GrassAttrs;
    a.world = in.world;
    a.normal = in.normal;
    a.color = in.color;
    a.uv = in.uv;
    a.cell = in.cell;
    a.misc = in.misc;
    a.across = in.across;
    a.canopy = in.canopy;
    let color = grass_surface(a, front, in.clip.xy, tex, next, auv);
    var out: MeshTargets;
    out.color = vec4<f32>(color, select(coverage, 1.0, STOCHASTIC));
    out.glow = vec4<f32>(0.0, 0.0, 0.0, coverage);
    var v = vec2<f32>(0.0);
    if (in.clip_prev.z > 1e-6) {
        let cur = (in.clip.xy * study.screen.zw) * 2.0 - 1.0;
        let prev = in.clip_prev.xy / in.clip_prev.z * vec2<f32>(1.0, -1.0);
        v = (cur * vec2<f32>(1.0, -1.0) - prev) * 0.5;
    }
    out.velocity = v;
    return out;
}

// ---- Visibility buffer (B, C, F) ----

struct VisOut {
    @builtin(position) clip: vec4<f32>,
    // xy = card coordinate, z = t, w = side 0..1.
    @location(0) uv: vec4<f32>,
    // x = far, y = real / drawn width.
    @location(1) misc: vec2<f32>,
    @location(2) @interpolate(flat) id: u32,
};

// Every blade (standing and the flat fans: the nearest one wins the pixel).
@vertex
fn vs_vis(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32, b: BladeIn) -> VisOut {
    var out: VisOut;
    if (grass_outside(b, study.view_proj, true) || study_skip(b)) {
        out.clip = GRASS_SKIP;
        return out;
    }
    let v = grass_blade(vi, b);
    if (SWR && blade_pixels(v) < study.params.x && v.dist > 3.0) {
        out.clip = GRASS_SKIP;
        return out;
    }
    out.clip = study.view_proj * vec4<f32>(v.p, 1.0);
    out.uv = vec4<f32>(grass_card_u(v.cell, v.side, v.repeat), grass_card_v(v.t, b.shape.w), v.t, v.side + 0.5);
    out.misc = vec2<f32>(smoothstep(5.0, 55.0, v.dist), v.thin);
    out.id = ii + 1u;
    return out;
}

@fragment
fn fs_vis(in: VisOut, @builtin(sample_mask) mask: u32) -> @location(0) vec2<u32> {
    count_quad(8u, mask);
    let auv = grass_atlas_uv(in.uv.xy);
    let alpha = textureSampleGrad(grass_atlas, grass_sampler, auv.xy, grass_atlas_dx(in.uv.xy), grass_atlas_dy(in.uv.xy)).a * auv.z;
    let coverage = grass_coverage(vec4<f32>(in.misc.x, 0.0, 0.0, in.misc.y), alpha, fwidth(alpha), in.clip.xy);
    if (coverage < 0.01 || !stochastic_keep(coverage, in.clip.xy)) {
        discard;
    }
    count_add(10u, 1u);
    let packed = pack2x16unorm(clamp(in.uv.zw, vec2<f32>(0.0), vec2<f32>(1.0)));
    return vec2<u32>(in.id, packed);
}

// Core WebGPU can't multisample integer targets: the id goes into an R32Float target as
// raw bits (with an exponent bit set, so it is never a denormal that could be flushed) and
// the point on the card into an Rg16Float target.
struct Vis4Out {
    @location(0) id: f32,
    @location(1) ts: vec2<f32>,
    @builtin(sample_mask) mask: u32,
};

// Variant B4: the visibility raster at 4× MSAA. Integer targets can't use alpha to coverage,
// so the coverage becomes an output sample mask the same way (n of 4 samples, rotated per
// pixel), and each sample keeps its own nearest blade.
@fragment
fn fs_vis4(in: VisOut, @builtin(sample_mask) mask_in: u32) -> Vis4Out {
    count_quad(8u, mask_in);
    let auv = grass_atlas_uv(in.uv.xy);
    let alpha = textureSampleGrad(grass_atlas, grass_sampler, auv.xy, grass_atlas_dx(in.uv.xy), grass_atlas_dy(in.uv.xy)).a * auv.z;
    let coverage = grass_coverage(vec4<f32>(in.misc.x, 0.0, 0.0, in.misc.y), alpha, fwidth(alpha), in.clip.xy);
    let ms = f32(MS_SAMPLES);
    let n = u32(clamp(round(coverage * ms), 0.0, ms));
    if (n == 0u) {
        discard;
    }
    count_add(10u, 1u);
    let start = min(u32(study_hash(floor(in.clip.xy) * 0.731 + 5.3) * ms), MS_SAMPLES - 1u);
    let bits = (1u << n) - 1u;
    let all = (1u << MS_SAMPLES) - 1u;
    var out: Vis4Out;
    out.mask = ((bits << start) | (bits >> (MS_SAMPLES - start))) & all;
    out.id = bitcast<f32>(in.id | 0x40000000u);
    out.ts = in.uv.zw;
    return out;
}
