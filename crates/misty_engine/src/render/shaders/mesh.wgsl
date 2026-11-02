// Lit meshes: Lambert diffuse + normalised Blinn-Phong specular, hemisphere ambient,
// directional/point/spot lights and one directional shadow map with 3x3 PCF.

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
    // rgb = linear colour * intensity, w = casts the shadow map (1/0).
    color: vec4<f32>,
    // x = cos(outer half angle), y = cos(inner half angle).
    spot: vec4<f32>,
};

struct Lights {
    // x = light count, y = shadow map enabled, z = ambient intensity, w unused.
    info: vec4<f32>,
    shadow_view_proj: mat4x4<f32>,
    sky: vec4<f32>,
    equator: vec4<f32>,
    ground: vec4<f32>,
    items: array<LightItem, 8>,
};

@group(1) @binding(0) var<uniform> lights: Lights;
@group(1) @binding(1) var<storage, read> draws: array<Draw>;
@group(1) @binding(2) var shadow_map: texture_depth_2d;
@group(1) @binding(3) var shadow_sampler: sampler_comparison;

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

fn shadow_factor(world: vec3<f32>, n: vec3<f32>, l: vec3<f32>) -> f32 {
    // Normal offset reduces acne on surfaces facing away from the light.
    let offset = n * 0.02 * (1.0 - max(dot(n, l), 0.0));
    let p = lights.shadow_view_proj * vec4<f32>(world + offset, 1.0);
    let ndc = p.xyz / p.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || ndc.z > 1.0) {
        return 1.0;
    }
    let texel = 1.0 / vec2<f32>(textureDimensions(shadow_map));
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) * texel, ndc.z - 0.0015);
        }
    }
    return sum / 9.0;
}

@fragment
fn fs_main(in: MeshOut) -> @location(0) vec4<f32> {
    let d = draws[in.draw];
    let albedo = srgb_to_linear(d.color.rgb);
    let metallic = d.params.x;
    let smoothness = d.params.y;
    let n = normalize(in.normal);
    var v: vec3<f32>;
    if (view.camera_pos.w > 0.5) {
        v = normalize(-(view.inv_view_proj * vec4<f32>(0.0, 0.0, 1.0, 0.0)).xyz);
    } else {
        v = normalize(view.camera_pos.xyz - in.world);
    }

    // Hemisphere ambient from the sky colours.
    var ambient: vec3<f32>;
    if (n.y >= 0.0) {
        ambient = mix(lights.equator.rgb, lights.sky.rgb, n.y);
    } else {
        ambient = mix(lights.equator.rgb, lights.ground.rgb, -n.y);
    }
    let diffuse_color = albedo * (1.0 - metallic);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    var color = ambient * lights.info.z * (diffuse_color + f0 * 0.25);

    let spec_power = exp2(10.0 * smoothness + 1.0);
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
        let ndl = max(dot(n, l), 0.0);
        if (ndl <= 0.0 || atten <= 0.0) {
            continue;
        }
        if (li.color.w > 0.5 && lights.info.y > 0.5 && d.params.z > 0.5) {
            atten *= shadow_factor(in.world, n, l);
        }
        let h = normalize(l + v);
        let spec = pow(max(dot(n, h), 0.0), spec_power) * (spec_power + 8.0) / 25.13;
        color += (diffuse_color + f0 * spec * smoothness) * li.color.rgb * ndl * atten;
    }
    color += d.emission.rgb;
    return vec4<f32>(color, d.color.a);
}
