// Lit meshes, close to the Standard (metallic) shader:
// Lambert diffuse + GGX specular, sky reflections with Fresnel, hemisphere ambient,
// directional/point/spot lights, hard or soft shadows for all three light types.

const PI: f32 = 3.14159265;
const MAX_LOCAL_SHADOWS: u32 = 16u;

struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    // sRGB base colour + alpha.
    color: vec4<f32>,
    // x = metallic, y = smoothness, z = receive shadows, w unused.
    params: vec4<f32>,
    // rgb = emission (linear).
    emission: vec4<f32>,
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
    // x = light count, y unused, z = ambient intensity, w unused.
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

struct MeshIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) draw: u32,
};

@vertex
fn vs_main(in: MeshIn, @builtin(instance_index) draw_index: u32) -> MeshOut {
    let d = draws[draw_index];
    let world = d.model * vec4<f32>(in.position, 1.0);
    var out: MeshOut;
    out.clip = view.view_proj * world;
    out.world = world.xyz;
    out.normal = (d.normal * vec4<f32>(in.normal, 0.0)).xyz;
    out.uv = in.uv;
    out.draw = draw_index;
    return out;
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
        let p = world + n * 0.02 * slope;
        return filter_directional(shadow_coords(lights.shadow_view_proj, p), mode, 0.0015);
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
    let offset = n * (0.01 + 0.03 * slope) * max(length(to_point) * 0.1, 0.2);
    let sc = shadow_coords(lights.local_view_proj[layer], world + offset);
    return filter_local(sc, layer, mode, 0.0005);
}

// GGX normal distribution.
fn d_ggx(n_dot_h: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

@fragment
fn fs_main(in: MeshOut) -> @location(0) vec4<f32> {
    let d = draws[in.draw];
    let albedo = srgb_to_linear(d.color.rgb);
    let metallic = clamp(d.params.x, 0.0, 1.0);
    let smoothness = clamp(d.params.y, 0.0, 1.0);
    let perceptual = 1.0 - smoothness;
    let roughness = max(perceptual * perceptual, 0.002);
    let n = normalize(in.normal);
    var v: vec3<f32>;
    if (view.camera_pos.w > 0.5) {
        v = normalize(-(view.inv_view_proj * vec4<f32>(0.0, 0.0, 1.0, 0.0)).xyz);
    } else {
        v = normalize(view.camera_pos.xyz - in.world);
    }
    let n_dot_v = max(dot(n, v), 1e-4);

    let diffuse_color = albedo * (1.0 - metallic);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let grazing = clamp(smoothness + (1.0 - (1.0 - metallic) * 0.96), 0.0, 1.0);

    // Ambient diffuse from the sky hemisphere.
    var ambient: vec3<f32>;
    if (n.y >= 0.0) {
        ambient = mix(lights.equator.rgb, lights.sky.rgb, n.y);
    } else {
        ambient = mix(lights.equator.rgb, lights.ground.rgb, -n.y);
    }
    var color = ambient * lights.info.z * diffuse_color;

    // Reflection of the sky: sharp when smooth, blurred towards the ambient colour when rough.
    let r = reflect(-v, n);
    let env = mix(sky_gradient(r), ambient * 1.6, perceptual);
    let fresnel_env = mix(f0, vec3<f32>(grazing), pow(1.0 - n_dot_v, 5.0));
    let surface_reduction = 1.0 / (roughness * roughness + 1.0);
    color += env * fresnel_env * surface_reduction * lights.info.z * smoothness;

    let count = u32(lights.info.x);
    for (var i = 0u; i < count; i++) {
        let li = lights.items[i];
        let kind = u32(li.dir_type.w);
        var l: vec3<f32>;
        var atten = 1.0;
        if (kind == 0u) {
            l = -normalize(li.dir_type.xyz);
        } else {
            let to_light = li.pos_range.xyz - in.world;
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
        if (d.params.z > 0.5) {
            atten *= shadow_factor(li, in.world, n, l);
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
    color += d.emission.rgb;
    return vec4<f32>(color, d.color.a);
}
