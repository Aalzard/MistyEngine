// Reflection probes (render/probes): appended to mesh.wgsl. Each draw lists up to four probes
// (most important first); a pixel blends those whose influence box contains it, fading over
// the blend distance, and the sky fills the rest (an HDRI sky through its own prefiltered cube
// and SH, like a probe at infinity; the procedural sky in mesh.wgsl). Specular reads the prefiltered cube level of
// the surface roughness along the box-projected reflection; diffuse evaluates the probe's SH9.

const PROBE_NONE: u32 = 0xffffffffu;

struct ProbeItem {
    // xyz = capture point, w = intensity.
    position: vec4<f32>,
    // w = blend distance.
    box_min: vec4<f32>,
    // w = 1 with box projection.
    box_max: vec4<f32>,
    // x = atlas cube, y = importance, z = index (debug colour).
    params: vec4<f32>,
    // Irradiance / π (see sh.rs).
    sh: array<vec4<f32>, 9>,
};

struct Probes {
    // x = count, y = debug view, z = highest roughness level.
    header: vec4<f32>,
    items: array<ProbeItem, 64>,
};

@group(1) @binding(20) var<uniform> probes: Probes;
@group(1) @binding(21) var probe_atlas: texture_cube_array<f32>;
@group(1) @binding(22) var probe_sampler: sampler;
@group(1) @binding(23) var<storage, read> probe_lists: array<vec4<u32>>;
// Split-sum BRDF: (N·V, perceptual roughness) → (scale, bias) of F0.
@group(1) @binding(24) var brdf_lut: texture_2d<f32>;

struct ProbeLight {
    // Blend of the probes' SH and prefiltered reflections (intensity applied).
    diffuse: vec3<f32>,
    specular: vec3<f32>,
    // How much the probes cover (the sky gets 1 - weight); 1 with an HDRI sky, which fills
    // the rest itself.
    weight: f32,
    // Probe coverage alone (debug view).
    coverage: f32,
    // Debug view colour.
    debug: vec3<f32>,
};

fn probes_active() -> bool {
    return probes.header.x > 0.5;
}

// Diffuse SH9 at normal `n` (basis as in sh.rs).
fn probe_sh(i: u32, n: vec3<f32>) -> vec3<f32> {
    let c = probes.items[i].sh;
    var r = c[0].rgb * 0.282095;
    r += c[1].rgb * (0.488603 * n.y) + c[2].rgb * (0.488603 * n.z) + c[3].rgb * (0.488603 * n.x);
    r += c[4].rgb * (1.092548 * n.x * n.y) + c[5].rgb * (1.092548 * n.y * n.z);
    r += c[6].rgb * (0.315392 * (3.0 * n.z * n.z - 1.0)) + c[7].rgb * (1.092548 * n.x * n.z);
    r += c[8].rgb * (0.546274 * (n.x * n.x - n.y * n.y));
    return max(r, vec3<f32>(0.0));
}

// 1 deep inside the box, fading to 0 at its faces over the blend distance.
fn probe_weight(p: ProbeItem, world: vec3<f32>) -> f32 {
    let d = min(world - p.box_min.xyz, p.box_max.xyz - world);
    let inside = min(d.x, min(d.y, d.z));
    if (inside < 0.0) {
        return 0.0;
    }
    if (p.box_min.w <= 1e-4) {
        return 1.0;
    }
    return clamp(inside / p.box_min.w, 0.0, 1.0);
}

// Lookup direction: the reflection ray hits the box, seen from the capture point.
fn probe_direction(p: ProbeItem, world: vec3<f32>, r: vec3<f32>) -> vec3<f32> {
    if (p.box_max.w < 0.5) {
        return r;
    }
    let safe = select(r, vec3<f32>(1e-6), abs(r) < vec3<f32>(1e-6));
    let far = max((p.box_max.xyz - world) / safe, (p.box_min.xyz - world) / safe);
    let t = min(far.x, min(far.y, far.z));
    return world + r * max(t, 0.0) - p.position.xyz;
}

fn probe_hue(i: f32) -> vec3<f32> {
    let h = fract(i * 0.618034) * 6.0;
    let k = abs(fract(vec3<f32>(h, h + 4.0, h + 2.0) / 6.0) * 6.0 - 3.0) - 1.0;
    return clamp(k, vec3<f32>(0.0), vec3<f32>(1.0)) * 0.8 + 0.2;
}

// Probes lighting a point of draw `draw`: normal `n`, reflection `r`.
fn probe_lighting(draw: u32, world: vec3<f32>, n: vec3<f32>, r: vec3<f32>, perceptual: f32) -> ProbeLight {
    var out = probe_blend(draw, world, n, r, perceptual);
    out.coverage = out.weight;
    // An HDRI sky is the probe of last resort: it fills whatever the probes leave.
    if (sky_is_hdri() && out.weight < 0.999) {
        let rest = 1.0 - out.weight;
        out.specular = out.specular * out.weight + sky_hdri_specular(r, perceptual) * rest;
        out.diffuse = out.diffuse * out.weight + sky_hdri_diffuse(n) * rest;
        out.weight = 1.0;
    }
    return out;
}

// The draw's probes alone: weighted averages and how much they cover.
fn probe_blend(draw: u32, world: vec3<f32>, n: vec3<f32>, r: vec3<f32>, perceptual: f32) -> ProbeLight {
    var out: ProbeLight;
    let count = u32(probes.header.x);
    if (count == 0u) {
        return out;
    }
    let list = probe_lists[draw];
    var total = 0.0;
    for (var k = 0u; k < 4u; k++) {
        let i = list[k];
        if (i == PROBE_NONE || i >= count) {
            break;
        }
        let p = probes.items[i];
        let w = probe_weight(p, world) * (1.0 - total);
        if (w <= 0.0) {
            continue;
        }
        let dir = probe_direction(p, world, r);
        let lod = perceptual * probes.header.z;
        let spec = textureSampleLevel(probe_atlas, probe_sampler, dir, i32(p.params.x), lod).rgb;
        out.specular += spec * (p.position.w * w);
        out.diffuse += probe_sh(i, n) * (p.position.w * w);
        out.debug += probe_hue(p.params.z) * w;
        total += w;
        if (total >= 0.999) {
            break;
        }
    }
    if (total > 0.0) {
        out.specular /= total;
        out.diffuse /= total;
        out.debug /= total;
    }
    out.weight = total;
    return out;
}

// Split-sum specular of the probes: prefiltered light × (F0 · A + B), with the multiple-scattering
// energy the single-scattering lobe loses on rough surfaces added back.
fn probe_specular(probe: ProbeLight, f0: vec3<f32>, n_dot_v: f32, perceptual: f32) -> vec3<f32> {
    let ab = textureSampleLevel(brdf_lut, probe_sampler, vec2<f32>(n_dot_v, perceptual), 0.0).rg;
    let energy = 1.0 + f0 * (1.0 / max(ab.x + ab.y, 1e-3) - 1.0);
    return probe.specular * (f0 * ab.x + ab.y) * energy;
}

// Debug view: each probe's colour where it lights, dark grey where the sky does.
fn probe_debug_color(probe: ProbeLight, n: vec3<f32>) -> vec3<f32> {
    let shade = 0.45 + 0.55 * clamp(n.y * 0.5 + 0.5, 0.0, 1.0);
    return mix(vec3<f32>(0.12), probe.debug, probe.coverage) * shade;
}
