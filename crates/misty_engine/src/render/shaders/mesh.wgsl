// Lit meshes, close to the Standard (metallic) shader:
// Lambert diffuse + GGX specular, sky reflections with Fresnel, hemisphere ambient,
// directional/point/spot lights, hard or soft shadows for all three light types.
// Albedo texture × colour and tangent-space normal maps (OpenGL convention, green = up),
// metallic (R) / smoothness (A), occlusion (G) and emission maps.
// Rendering modes: Opaque, Cutout (fs_cutout discards below the cutoff), Fade (straight alpha)
// and Transparent (premultiplied: only the diffuse fades). fs_unlit is the Unlit shaders.
// Fog (Lighting window) blends the result towards the fog colour; volumetric / height fog
// (Post-process Volume, fog_apply.wgsl) is applied on top.

const PI: f32 = 3.14159265;
const MAX_LOCAL_SHADOWS: u32 = 16u;

struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    // sRGB base colour + alpha.
    color: vec4<f32>,
    // x = metallic, y = smoothness, z = receive shadows, w = normal map scale (0 = no normal map).
    params: vec4<f32>,
    // rgb = emission (linear), w = -1 when the model matrix mirrors (flips tangent handedness).
    emission: vec4<f32>,
    // xy = tiling, zw = offset.
    uv_st: vec4<f32>,
    // x = rendering mode (0 opaque, 1 cutout, 2 fade, 3 transparent), y = alpha cutoff,
    // z = occlusion strength, w = 1 when smoothness comes from the albedo alpha.
    surface: vec4<f32>,
};

struct LightItem {
    // xyz = position, w = range.
    pos_range: vec4<f32>,
    // xyz = direction the light travels, w = type (0 dir, 1 point, 2 spot).
    dir_type: vec4<f32>,
    // rgb = linear colour * intensity.
    color: vec4<f32>,
    // x = cos(outer half angle), y = cos(inner half angle).
    spot: vec4<f32>,
    // x = shadow mode (0 none, 1 hard, 2 soft), y = shadow layer (-1 = directional map).
    shadow: vec4<f32>,
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
@group(1) @binding(2) var shadow_map: texture_depth_2d;
@group(1) @binding(3) var shadow_sampler: sampler_comparison;
@group(1) @binding(4) var local_shadow_map: texture_depth_2d_array;
// Instance index → draw slot: instanced batches list their draws here.
@group(1) @binding(5) var<storage, read> slots: array<u32>;

// Material textures (1×1 white / flat normal when unassigned).
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
};

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) draw: u32,
    @location(4) tangent: vec4<f32>,
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
    out.uv = in.uv * d.uv_st.xy + d.uv_st.zw;
    out.draw = draw_index;
    out.tangent = vec4<f32>((d.model * vec4<f32>(in.tangent.xyz, 0.0)).xyz, in.tangent.w * d.emission.w);
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

// Shadow-map coordinates (uv, depth) of a world point, or depth > 1 when outside.
fn shadow_coords(m: mat4x4<f32>, p: vec3<f32>) -> vec3<f32> {
    let c = m * vec4<f32>(p, 1.0);
    let ndc = c.xyz / c.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (c.w <= 0.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) {
        return vec3<f32>(0.0, 0.0, 2.0);
    }
    return vec3<f32>(uv, ndc.z);
}

// Hard = one bilinear compare; Soft = 5x5 PCF over a wider footprint.
fn filter_directional(sc: vec3<f32>, mode: f32, bias: f32) -> f32 {
    if (sc.z > 1.0) {
        return 1.0;
    }
    let z = sc.z - bias;
    if (mode < 1.5) {
        return textureSampleCompareLevel(shadow_map, shadow_sampler, sc.xy, z);
    }
    let texel = 1.5 / vec2<f32>(textureDimensions(shadow_map));
    var sum = 0.0;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, sc.xy + vec2<f32>(f32(x), f32(y)) * texel, z);
        }
    }
    return sum / 25.0;
}

fn filter_local(sc: vec3<f32>, layer: i32, mode: f32, bias: f32) -> f32 {
    if (sc.z > 1.0) {
        return 1.0;
    }
    let z = sc.z - bias;
    if (mode < 1.5) {
        return textureSampleCompareLevel(local_shadow_map, shadow_sampler, sc.xy, layer, z);
    }
    let texel = 1.5 / vec2<f32>(textureDimensions(local_shadow_map));
    var sum = 0.0;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            sum += textureSampleCompareLevel(local_shadow_map, shadow_sampler, sc.xy + vec2<f32>(f32(x), f32(y)) * texel, layer, z);
        }
    }
    return sum / 25.0;
}

fn shadow_factor(li: LightItem, world: vec3<f32>, n: vec3<f32>, l: vec3<f32>) -> f32 {
    let mode = li.shadow.x;
    if (mode < 0.5) {
        return 1.0;
    }
    // Normal offset reduces acne on surfaces facing away from the light.
    let slope = 1.0 - max(dot(n, l), 0.0);
    let kind = u32(li.dir_type.w);
    if (kind == 0u) {
        // Push the lookup out along the normal by about one shadow texel (more on slopes).
        let p = world + n * lights.info.y * (1.5 + 3.0 * slope);
        return filter_directional(shadow_coords(lights.shadow_view_proj, p), mode, 0.0003);
    }
    var layer = i32(li.shadow.y);
    if (layer < 0) {
        return 1.0;
    }
    let to_point = world - li.pos_range.xyz;
    if (kind == 1u) {
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
    // One 1024² texel of a ~92° frustum at this distance is about 0.002 × distance.
    let texel = length(to_point) * 0.002;
    let offset = n * texel * (2.0 + 4.0 * slope);
    let sc = shadow_coords(lights.local_view_proj[layer], world + offset);
    return filter_local(sc, layer, mode, 0.0002);
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
    return s;
}

@fragment
fn fs_main(in: MeshOut, @builtin(front_facing) front: bool) -> MeshTargets {
    return shade(in.world, standard_surface(in, front));
}

// Cutout: holes where alpha is below the cutoff.
@fragment
fn fs_cutout(in: MeshOut, @builtin(front_facing) front: bool) -> MeshTargets {
    let s = standard_surface(in, front);
    if (s.alpha < draws[in.draw].surface.y) {
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
    let color = srgb_to_linear(d.color.rgb) * base.rgb;
    let alpha = d.color.a * base.a;
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

    // Ambient diffuse from the environment (sky / equator / ground).
    var ambient: vec3<f32>;
    if (n.y >= 0.0) {
        ambient = mix(lights.equator.rgb, lights.sky.rgb, n.y);
    } else {
        ambient = mix(lights.equator.rgb, lights.ground.rgb, -n.y);
    }
    var color = ambient * lights.info.z * diffuse_color * s.occlusion;

    // Reflection of the sky: sharp when smooth, blurred towards the ambient colour when rough.
    let r = reflect(-v, n);
    let env = mix(sky_color(r), ambient * 1.6, perceptual);
    let fresnel_env = mix(f0, vec3<f32>(grazing), pow(1.0 - n_dot_v, 5.0));
    let surface_reduction = 1.0 / (roughness * roughness + 1.0);
    color += env * fresnel_env * surface_reduction * lights.info.w * smoothness * s.occlusion;

    let count = u32(lights.info.x);
    for (var i = 0u; i < count; i++) {
        let li = lights.items[i];
        let kind = u32(li.dir_type.w);
        var l: vec3<f32>;
        var atten = 1.0;
        if (kind == 0u) {
            l = -normalize(li.dir_type.xyz);
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
            atten *= shadow_factor(li, world, n, l);
        }
        let h = normalize(l + v);
        let n_dot_h = max(dot(n, h), 0.0);
        let l_dot_h = max(dot(l, h), 0.0);
        let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - l_dot_h, 5.0);
        // Kelemen visibility term; scaled by PI to match the un-normalised diffuse.
        let vis = 0.25 / max(l_dot_h * l_dot_h, 0.1);
        let spec = fresnel * (d_ggx(n_dot_h, roughness) * vis * PI);
        color += (diffuse_color + spec) * li.color.rgb * n_dot_l * atten;
    }
    color += s.emission;
    let fog = fog_visibility(fog_distance(world));
    // Premultiplied colour gets premultiplied fog.
    let fog_color = select(view.fog_color.rgb, view.fog_color.rgb * alpha, s.premultiply);
    color = mix(fog_color, color, fog);
    let vf = vfog_eval(world);
    color = vfog_blend(color, vf, select(1.0, alpha, s.premultiply));
    var out: MeshTargets;
    out.color = vec4<f32>(color, alpha);
    out.glow = vec4<f32>(s.emission * fog * vfog_glow(vf), alpha);
    return out;

}
