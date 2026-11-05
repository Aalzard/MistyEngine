// Lit meshes, close to the Standard (metallic) shader:
// Lambert diffuse + GGX specular, sky reflections with Fresnel, hemisphere ambient,
// directional/point/spot lights, hard or soft shadows for all three light types (shadows.wgsl),
// rectangle area lights through linearly transformed cosines (area/area_light.wgsl).
// Albedo texture × colour and tangent-space normal maps (OpenGL convention, green = up),
// metallic (R) / smoothness (A), occlusion (G) and emission maps.
// Rendering modes: Opaque, Cutout (fs_cutout discards below the cutoff), Fade (straight alpha)
// and Transparent (premultiplied: only the diffuse fades). fs_unlit is the Unlit shaders.
// Fog (Lighting window) blends the result towards the fog colour; volumetric / height fog
// (Post-process Volume, fog_apply.wgsl) is applied on top.

const PI: f32 = 3.14159265;
const MAX_LOCAL_SHADOWS: u32 = 16u;

// `struct Draw` and `draw_uv` come from draw.wgsl.

struct LightItem {
    // xyz = position, w = range.
    pos_range: vec4<f32>,
    // xyz = direction the light travels, w = type (0 dir, 1 point, 2 spot, 3 rectangle).
    dir_type: vec4<f32>,
    // rgb = linear colour * intensity.
    color: vec4<f32>,
    // x = cos(outer half angle), y = cos(inner half angle), z = contact shadows.
    spot: vec4<f32>,
    // x = shadow mode (0 none, 1 hard, 2 soft), y = shadow layer (-1 = directional map).
    shadow: vec4<f32>,
    // Rectangle (area_light.wgsl): xyz = half-width axis, w = 1 when two-sided.
    area_x: vec4<f32>,
    // Rectangle: xyz = half-height axis.
    area_y: vec4<f32>,
};

struct Lights {
    // x = light count, y = directional shadow texel size (world), z = ambient intensity,
    // w = reflection intensity.
    info: vec4<f32>,
    shadow_view_proj: mat4x4<f32>,
    sky: vec4<f32>,
    equator: vec4<f32>,
    ground: vec4<f32>,
    items: array<LightItem, 8>,
    // Spot: one layer each. Point: six consecutive layers (+X, -X, +Y, -Y, +Z, -Z).
    local_view_proj: array<mat4x4<f32>, MAX_LOCAL_SHADOWS>,
};

@group(1) @binding(0) var<uniform> lights: Lights;
@group(1) @binding(1) var<storage, read> draws: array<Draw>;
// Sun cascades, one layer each (shadows.wgsl).
@group(1) @binding(2) var shadow_map: texture_depth_2d_array;
@group(1) @binding(3) var shadow_sampler: sampler_comparison;
@group(1) @binding(4) var local_shadow_map: texture_depth_2d_array;
// Instance index → draw slot: instanced batches list their draws here.
@group(1) @binding(5) var<storage, read> slots: array<u32>;

// Material textures (1×1 white / flat normal when unassigned).
// Baked lightmaps (render/lightmap_gpu.rs): one layer per lightmap; info x = debug view
// (1 Baked Lightmap, 2 UV Charts), y = layer count, z = bit mask of loaded layers.
@group(1) @binding(25) var lightmaps: texture_2d_array<f32>;
@group(1) @binding(26) var<uniform> lightmap_info: vec4<u32>;

@group(2) @binding(0) var albedo_map: texture_2d<f32>;
@group(2) @binding(1) var normal_map: texture_2d<f32>;
@group(2) @binding(2) var material_sampler: sampler;
// Linear maps (1×1 white when unassigned) and the sRGB emission map (white).
@group(2) @binding(3) var metallic_map: texture_2d<f32>;
@group(2) @binding(4) var occlusion_map: texture_2d<f32>;
@group(2) @binding(5) var emission_map: texture_2d<f32>;

struct MeshIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // xyz = tangent along +u, w = bitangent sign.
    @location(3) tangent: vec4<f32>,
    @location(4) uv2: vec2<f32>,
};

struct MeshOut {
    // Invariant: the depth prepass reuses this vertex stage and tests for equal depth.
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) draw: u32,
    @location(4) tangent: vec4<f32>,
    // Lightmap coordinates (uv2 × the draw's lightmap scale-offset).
    @location(5) lightmap_uv: vec2<f32>,
};

@vertex
fn vs_main(in: MeshIn, @builtin(instance_index) instance: u32) -> MeshOut {
    let draw_index = slots[instance];
    let d = draws[draw_index];
    let world = d.model * vec4<f32>(in.position, 1.0);
    var out: MeshOut;
    out.clip = view.view_proj * world;
    out.world = world.xyz;
    out.normal = (d.normal * vec4<f32>(in.normal, 0.0)).xyz;
    out.uv = draw_uv(d, in.uv);
    out.draw = draw_index;
    out.tangent = vec4<f32>((d.model * vec4<f32>(in.tangent.xyz, 0.0)).xyz, in.tangent.w * d.emission.w);
    out.lightmap_uv = in.uv2 * d.lightmap_st.xy + d.lightmap_st.zw;
    return out;
}

// Surface normal with the material's normal map applied (bitangent points up in the image).
fn mapped_normal(in: MeshOut, d: Draw, sampled: vec3<f32>) -> vec3<f32> {
    let n = normalize(in.normal);
    if (d.params.w == 0.0) {
        return n;
    }
    let t = normalize(in.tangent.xyz - n * dot(n, in.tangent.xyz));
    let b = cross(n, t) * in.tangent.w;
    var m = sampled * 2.0 - 1.0;
    m = vec3<f32>(m.xy * d.params.w, max(m.z, 1e-3));
    return normalize(t * m.x + b * m.y + n * m.z);
}

// GGX normal distribution.
fn d_ggx(n_dot_h: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

struct MeshTargets {
    @location(0) color: vec4<f32>,
    // Emission only, the bloom source.
    @location(1) glow: vec4<f32>,
};

// Lit surface inputs of `shade` (also used by the terrain shader).
struct Surface {
    albedo: vec3<f32>,
    alpha: f32,
    metallic: f32,
    smoothness: f32,
    normal: vec3<f32>,
    // Linear emission.
    emission: vec3<f32>,
    receive_shadows: bool,
    // Ambient occlusion (1 = none), scales ambient light and reflections.
    occlusion: f32,
    // Transparent mode: diffuse × alpha, specular kept (premultiplied blending).
    premultiply: bool,
    // Draw slot (its reflection probe list).
    draw: u32,
    // Lightmap coordinates (used when the draw is lightmapped).
    lightmap_uv: vec2<f32>,
};

// Standard shader inputs of a fragment; back faces (Cull Off) are lit from their side.
fn standard_surface(in: MeshOut, front: bool) -> Surface {
    let d = draws[in.draw];
    // Sample before any non-uniform branch (implicit derivatives).
    let base = textureSample(albedo_map, material_sampler, in.uv);
    let sampled_normal = textureSample(normal_map, material_sampler, in.uv).xyz;
    let metal_gloss = textureSample(metallic_map, material_sampler, in.uv);
    let occlusion = textureSample(occlusion_map, material_sampler, in.uv).g;
    let emission = textureSample(emission_map, material_sampler, in.uv).rgb;
    let from_albedo = d.surface.w > 0.5;
    var s: Surface;
    s.albedo = srgb_to_linear(d.color.rgb) * base.rgb;
    // Smoothness in the albedo alpha: the texture alpha is not transparency.
    s.alpha = d.color.a * select(base.a, 1.0, from_albedo);
    s.metallic = d.params.x * metal_gloss.r;
    s.smoothness = d.params.y * select(metal_gloss.a, base.a, from_albedo);
    var side = in;
    if (!front) {
        side.normal = -in.normal;
    }
    s.normal = mapped_normal(side, d, sampled_normal);
    s.emission = d.emission.rgb * emission;
    s.receive_shadows = d.params.z > 0.5;
    s.occlusion = mix(1.0, occlusion, d.surface.z);
    s.premultiply = d.surface.x > 2.5;
    s.draw = in.draw;
    s.lightmap_uv = in.lightmap_uv;
    return s;
}

// Baked light of a lightmapped surface: rgb = irradiance (× albedo = diffuse light), a = 1;
// zero when the draw has no loaded lightmap.
fn lightmap_sample(s: Surface) -> vec4<f32> {
    let slot = draws[s.draw].lightmap.x;
    if (slot < 0.5) {
        return vec4<f32>(0.0);
    }
    let layer = u32(slot + 0.5) - 1u;
    if (layer >= lightmap_info.y || (lightmap_info.z & (1u << layer)) == 0u) {
        return vec4<f32>(0.0);
    }
    let c = textureSampleLevel(lightmaps, probe_sampler, s.lightmap_uv, i32(layer), 0.0).rgb;
    return vec4<f32>(c, 1.0);
}

// Scene view Baked Lightmap / UV Charts views.
fn lightmap_debug_color(s: Surface, lm: vec4<f32>) -> vec3<f32> {
    if (lm.w < 0.5) {
        return vec3<f32>(0.08);
    }
    if (lightmap_info.x == 1u) {
        return lm.rgb;
    }
    // UV Charts: the texel grid, tinted per lightmap layer.
    let size = vec2<f32>(textureDimensions(lightmaps).xy);
    let t = vec2<i32>(floor(s.lightmap_uv * size));
    let checker = f32((t.x + t.y) & 1);
    let layer = draws[s.draw].lightmap.x;
    let hue = fract(vec3<f32>(0.0, 0.33, 0.67) + layer * 0.37);
    let tint = 0.5 + 0.5 * cos(6.2831853 * hue);
    return tint * mix(0.35, 0.8, checker);
}

@fragment
fn fs_main(in: MeshOut, @builtin(front_facing) front: bool) -> MeshTargets {
    return shade(in.world, standard_surface(in, front));
}

// Cutout: holes where alpha is below the cutoff.
@fragment
fn fs_cutout(in: MeshOut, @builtin(front_facing) front: bool) -> MeshTargets {
    let s = standard_surface(in, front);
    let d = draws[in.draw];
    if (s.alpha < d.surface.y || lod_dithered_out(d.extra.z, in.clip.xy)) {
        discard;
    }
    var out = shade(in.world, s);
    out.color.a = 1.0;
    return out;
}

// Unlit/Color, Unlit/Texture, Unlit/Transparent: colour × albedo texture, fogged.
@fragment
fn fs_unlit(in: MeshOut) -> MeshTargets {
    let d = draws[in.draw];
    let base = textureSample(albedo_map, material_sampler, in.uv);
    var color = srgb_to_linear(d.color.rgb) * base.rgb;
    let alpha = d.color.a * base.a;
    // Sprite-Lit-Default: 2D lights of the draw's Sorting Layer (none in the scene = unlit).
    if (d.extra.w > 0.5 && lights2d.info.x > 0.5) {
        color *= light2d_sum(in.world.xy, u32(d.extra.w + 0.5) - 1u);
    }
    // Light probe gizmos: an unlit sphere flagged for light probes shows the baked light.
    let light_probe = light_probe_block(in.draw);
    if (light_probe != PROBE_NONE) {
        color *= light_probe_irradiance(light_probe, normalize(in.normal));
    }
    let fog = fog_visibility(fog_distance(in.world));
    let vf = vfog_eval(in.world);
    var out: MeshTargets;
    out.color = vec4<f32>(vfog_blend(mix(view.fog_color.rgb, color, fog), vf, 1.0), alpha);
    out.glow = vec4<f32>(0.0, 0.0, 0.0, alpha);
    return out;
}

// Ambient, sky reflections, lights with shadows, emission and fog for one surface point.
fn shade(world: vec3<f32>, s: Surface) -> MeshTargets {
    let albedo = s.albedo;
    let metallic = clamp(s.metallic, 0.0, 1.0);
    let smoothness = clamp(s.smoothness, 0.0, 1.0);
    let perceptual = 1.0 - smoothness;
    let roughness = max(perceptual * perceptual, 0.002);
    let n = s.normal;
    var v: vec3<f32>;
    if (view.camera_pos.w > 0.5) {
        v = normalize(-(view.inv_view_proj * vec4<f32>(0.0, 0.0, 1.0, 0.0)).xyz);
    } else {
        v = normalize(view.camera_pos.xyz - world);
    }
    let n_dot_v = max(dot(n, v), 1e-4);

    var diffuse_color = albedo * (1.0 - metallic);
    var alpha = s.alpha;
    if (s.premultiply) {
        // Reflective parts stay visible on glass: alpha grows with reflectivity.
        let one_minus_reflectivity = (1.0 - metallic) * 0.96;
        diffuse_color *= s.alpha;
        alpha = 1.0 - one_minus_reflectivity + s.alpha * one_minus_reflectivity;
    }
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let grazing = clamp(smoothness + (1.0 - (1.0 - metallic) * 0.96), 0.0, 1.0);

    // Ambient diffuse from the environment (sky / equator / ground), or from the baked light
    // probes for moving objects that blend them (probes.wgsl).
    var ambient: vec3<f32>;
    var ambient_scale = lights.info.z;
    let light_probe = light_probe_block(s.draw);
    if (light_probe != PROBE_NONE) {
        ambient = light_probe_irradiance(light_probe, n);
        ambient_scale = 1.0;
    } else if (n.y >= 0.0) {
        ambient = mix(lights.equator.rgb, lights.sky.rgb, n.y);
    } else {
        ambient = mix(lights.equator.rgb, lights.ground.rgb, -n.y);
    }
    // Screen-space AO (screen_space.wgsl) joins the material occlusion on ambient light.
    let ssao = screen_ao(world);
    var color = ambient * ambient_scale * diffuse_color * indirect_occlusion(s.occlusion, ssao, diffuse_color);
    // Lightmapped: baked ambient / bounce light replaces the realtime ambient and probe diffuse.
    let lm = lightmap_sample(s);
    let lightmapped = lm.w > 0.5;
    if (lightmapped) {
        color = lm.rgb * diffuse_color * indirect_occlusion(s.occlusion, ssao, diffuse_color);
    }

    // Reflection of the sky: sharp when smooth, blurred towards the ambient colour when rough.
    let r = reflect(-v, n);
    let env = mix(sky_color(r), ambient * 1.6, perceptual);
    let fresnel_env = mix(f0, vec3<f32>(grazing), pow(1.0 - n_dot_v, 5.0));
    let surface_reduction = 1.0 / (roughness * roughness + 1.0);
    let spec_occlusion = specular_occlusion(s.occlusion, ssao, n_dot_v, roughness);
    // Reflection probes replace the sky inside their boxes (probes.wgsl).
    let probe = probe_lighting(s.draw, world, n, r, perceptual);
    var reflection: vec3<f32>;
    if (probe.weight > 0.0) {
        let sky_spec = env * fresnel_env * surface_reduction * lights.info.w * smoothness;
        let probe_spec = probe_specular(probe, f0, n_dot_v, perceptual) * lights.info.w;
        let probe_diffuse = probe.diffuse * diffuse_color * indirect_occlusion(s.occlusion, ssao, diffuse_color);
        // Lightmaps and light probes keep their diffuse; reflection probes then only add reflections.
        if (!lightmapped && light_probe == PROBE_NONE) {
            color = mix(color, probe_diffuse, probe.weight);
        }
        reflection = mix(sky_spec, probe_spec, probe.weight) * spec_occlusion;
    } else {
        reflection = env * fresnel_env * surface_reduction * lights.info.w * smoothness * spec_occlusion;
    }
    // Screen-space GI and reflections (screen_trace.wgsl); unchanged while both are off.
    let traced = screen_trace_sample(world);
    color = screen_gi_apply(traced, color, diffuse_color, s.occlusion, lightmapped);
    let reflection_weight = fresnel_env * surface_reduction * lights.info.w * smoothness;
    color += screen_reflection_apply(traced, reflection, reflection_weight);

    let count = u32(lights.info.x);
    for (var i = 0u; i < count; i++) {
        let li = lights.items[i];
        let kind = u32(li.dir_type.w);
        if (kind == 3u) {
            color += rect_shade(i, li, world, s.receive_shadows, n, v, perceptual, f0, diffuse_color, ssao);
            continue;
        }
        var l: vec3<f32>;
        var atten = 1.0;
        if (kind == 0u) {
            l = -normalize(li.dir_type.xyz);
            // Volumetric clouds shade the sun (clouds_apply.wgsl).
            atten = cloud_shadow(world);
        } else {
            let to_light = li.pos_range.xyz - world;
            let dist = length(to_light);
            l = to_light / max(dist, 1e-4);
            let x = dist / max(li.pos_range.w, 1e-4);
            let window = clamp(1.0 - x * x * x * x, 0.0, 1.0);
            atten = window * window / (1.0 + 25.0 * x * x);
            if (kind == 2u) {
                let cos_angle = dot(-l, normalize(li.dir_type.xyz));
                atten *= smoothstep(li.spot.x, li.spot.y, cos_angle);
            }
        }
        let n_dot_l = max(dot(n, l), 0.0);
        if (n_dot_l <= 0.0 || atten <= 0.0) {
            continue;
        }
        if (s.receive_shadows) {
            atten *= shadow_factor(i, li, world, n, l);
            atten *= contact_shadow(li, world, n, l);
        }
        atten *= direct_occlusion(ssao);
        let h = normalize(l + v);
        let n_dot_h = max(dot(n, h), 0.0);
        let l_dot_h = max(dot(l, h), 0.0);
        let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - l_dot_h, 5.0);
        // Kelemen visibility term; scaled by PI to match the un-normalised diffuse.
        let vis = 0.25 / max(l_dot_h * l_dot_h, 0.1);
        let spec = fresnel * (d_ggx(n_dot_h, roughness) * vis * PI);
        color += (diffuse_color + spec) * li.color.rgb * n_dot_l * atten;
    }
    color = cascade_debug(world, color);
    color += s.emission;
    if (probes.header.y > 0.5) {
        color = probe_debug_color(probe, n);
    }
    if (lightmap_info.x > 0u) {
        color = lightmap_debug_color(s, lm);
    }
    let fog = fog_visibility(fog_distance(world));
    // Premultiplied colour gets premultiplied fog.
    let fog_color = select(view.fog_color.rgb, view.fog_color.rgb * alpha, s.premultiply);
    color = mix(fog_color, color, fog);
    let vf = vfog_eval(world);
    color = vfog_blend(color, vf, select(1.0, alpha, s.premultiply));
    var out: MeshTargets;
    out.color = vec4<f32>(color, alpha);
    out.glow = vec4<f32>(s.emission * fog * vfog_glow(vf), alpha);
    if (ao_debug_view()) {
        out.color = vec4<f32>(vec3<f32>(ssao), alpha);
        out.glow = vec4<f32>(0.0, 0.0, 0.0, alpha);
    }
    return out;

}

// ---- 2D lights (render/light2d.rs, light2d/mod.rs) ---------------------------------------------

struct Light2DItem {
    // xy = position, z = inner radius, w = outer radius.
    pos_radius: vec4<f32>,
    // rgb = linear colour * intensity, w = kind (0 global, 1 point, 2 spot).
    color_kind: vec4<f32>,
    // xy = spot direction, z = cos(outer half angle), w = cos(inner half angle).
    dir_angle: vec4<f32>,
    // x = falloff exponent, y = shadow row (-1 none), z = shadow strength, w = softness (bins).
    params: vec4<f32>,
    // x = Sorting Layer mask (bit = layer value + 16).
    mask: vec4<u32>,
};

struct Lights2D {
    // x = light count, y = bins per shadow row.
    info: vec4<f32>,
    items: array<Light2DItem, 16>,
    // Per shadowed light: distance where the shadow starts, by direction (polar map).
    shadows: array<f32, 4096>,
};

@group(1) @binding(30) var<storage, read> lights2d: Lights2D;

// Fraction of light reaching `dist` along direction `d` of shadow row `row` (1 = lit).
fn light2d_shadow(row: u32, d: vec2<f32>, dist: f32, soft: f32) -> f32 {
    let bins = lights2d.info.y;
    let t = (atan2(d.y, d.x) + PI) / (2.0 * PI);
    let center = i32(min(floor(t * bins), bins - 1.0));
    let n = i32(bins);
    let k = i32(round(soft));
    var lit = 0.0;
    var total = 0.0;
    for (var i = -k; i <= k; i++) {
        let b = u32((center + i + n) % n);
        let occ = lights2d.shadows[row * u32(bins) + b];
        // A short blend along the ray hides the map's stair steps.
        let edge = 0.04 + soft * 0.02 * dist;
        lit += 1.0 - smoothstep(occ, occ + edge, dist);
        total += 1.0;
    }
    return lit / total;
}

// Sum of the 2D lights reaching a world XY point on Sorting Layer bit `layer`.
fn light2d_sum(p: vec2<f32>, layer: u32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let count = u32(lights2d.info.x);
    for (var i = 0u; i < count; i++) {
        let l = lights2d.items[i];
        if (((l.mask.x >> layer) & 1u) == 0u) {
            continue;
        }
        if (l.color_kind.w < 0.5) {
            sum += l.color_kind.rgb;
            continue;
        }
        let d = p - l.pos_radius.xy;
        let dist = length(d);
        let outer = l.pos_radius.w;
        if (dist >= outer) {
            continue;
        }
        let t = clamp((outer - dist) / max(outer - l.pos_radius.z, 1e-4), 0.0, 1.0);
        var a = pow(t, l.params.x);
        if (l.color_kind.w > 1.5 && dist > 1e-5) {
            let c = dot(d / dist, l.dir_angle.xy);
            let s = clamp((c - l.dir_angle.z) / max(l.dir_angle.w - l.dir_angle.z, 1e-4), 0.0, 1.0);
            a *= s * s * (3.0 - 2.0 * s);
        }
        if (l.params.y >= 0.0 && a > 0.0) {
            let lit = light2d_shadow(u32(l.params.y), d, dist, l.params.w);
            a *= 1.0 - l.params.z * (1.0 - lit);
        }
        sum += l.color_kind.rgb * a;
    }
    return sum;
}
