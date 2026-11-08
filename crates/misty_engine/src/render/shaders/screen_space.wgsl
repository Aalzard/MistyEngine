// Screen-space lighting terms of the lit shaders (appended to mesh.wgsl): ambient occlusion
// from the AO passes (render/ao.rs) and contact shadows ray-marched through the prepass
// depth. With both effects off the header is zero and every term is exactly 1.

struct ScreenSpace {
    // x = 1 when AO is on, y = direct lighting strength, z = 1 for the AO debug view.
    ao: vec4<f32>,
    // x = 1 when contact shadows are on, y = ray length, z = opacity, w = sample count.
    contact: vec4<f32>,
    // x = camera distance where contact shadows have faded out, y = occluder thickness,
    // zw = resolution of the texels.
    misc: vec4<f32>,
    // xyz = camera forward (the linear depth axis).
    forward: vec4<f32>,
    // x = ambient occlusion (1 = none), y = linear depth of the prepass surface.
    texels: array<vec2<f32>>,
};

@group(1) @binding(16) var<storage, read> screen: ScreenSpace;

// Linear depth of a world point, as stored in the texels.
fn screen_depth(world: vec3<f32>) -> f32 {
    return dot(world - view.camera_pos.xyz, screen.forward.xyz);
}

// Pixel of a world point (xy); z = 1 when it is on screen.
fn screen_pixel(world: vec3<f32>) -> vec3<f32> {
    let c = view.view_proj * vec4<f32>(world, 1.0);
    if (c.w <= 0.0) {
        return vec3<f32>(0.0);
    }
    let ndc = c.xy / c.w;
    let px = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * screen.misc.zw;
    let inside = all(px >= vec2<f32>(0.0)) && all(px < screen.misc.zw);
    return vec3<f32>(px, select(0.0, 1.0, inside));
}

fn screen_texel(px: vec2<f32>) -> vec2<f32> {
    let p = vec2<u32>(px);
    return screen.texels[p.y * u32(screen.misc.z) + p.x];
}

// SSAO of the surface at `world`: 1 when AO is off or the prepass saw another surface there
// (transparent meshes).
fn screen_ao(world: vec3<f32>) -> f32 {
    if (screen.ao.x < 0.5) {
        return 1.0;
    }
    let px = screen_pixel(world);
    if (px.z < 0.5) {
        return 1.0;
    }
    let t = screen_texel(px.xy);
    let z = screen_depth(world);
    if (abs(t.y - z) > 0.02 * z + 0.05) {
        return 1.0;
    }
    return t.x;
}

// Multi-bounce: light inter-reflected by bright surfaces lifts their occlusion (cubic fit
// from Jimenez et al., "Practical Real-Time Strategies for Accurate Indirect Occlusion").
fn ao_multi_bounce(ao: f32, albedo: vec3<f32>) -> vec3<f32> {
    let a = 2.0404 * albedo - 0.3324;
    let b = -4.7951 * albedo + 0.6417;
    let c = 2.7552 * albedo + 0.6903;
    return max(vec3<f32>(ao), ((ao * a + b) * ao + c) * ao);
}

// Occlusion of ambient diffuse light: the material's occlusion map, joined by SSAO.
fn indirect_occlusion(material: f32, ssao: f32, albedo: vec3<f32>) -> vec3<f32> {
    if (screen.ao.x < 0.5) {
        return vec3<f32>(material);
    }
    return ao_multi_bounce(min(material, ssao), albedo);
}

// Occlusion of reflections, derived from the diffuse AO (Lagarde's approximation).
fn specular_occlusion(material: f32, ssao: f32, n_dot_v: f32, roughness: f32) -> f32 {
    if (screen.ao.x < 0.5) {
        return material;
    }
    let ao = min(material, ssao);
    return saturate(pow(n_dot_v + ao, exp2(-16.0 * roughness - 1.0)) - 1.0 + ao);
}

// Direct light is only darkened by AO when Direct Lighting Strength is above 0.
fn direct_occlusion(ssao: f32) -> f32 {
    return mix(1.0, ssao, screen.ao.y);
}

fn ao_debug_view() -> bool {
    return screen.ao.z > 0.5;
}

// Contact shadow of light `li` at `world`: march a short ray towards the light through the
// prepass depth; a sample behind a nearby surface (within the thickness) is in shadow.
fn contact_shadow(li: LightItem, world: vec3<f32>, n: vec3<f32>, l: vec3<f32>) -> f32 {
    if (screen.contact.x < 0.5 || li.spot.z < 0.5) {
        return 1.0;
    }
    let z0 = screen_depth(world);
    let fade_end = screen.misc.x;
    let fade = 1.0 - smoothstep(fade_end * 0.8, fade_end, z0);
    if (fade <= 0.0) {
        return 1.0;
    }
    let px0 = screen_pixel(world);
    let jitter = fract(52.9829189 * fract(dot(px0.xy, vec2<f32>(0.06711056, 0.00583715))));
    // Start just off the surface so it doesn't shadow itself.
    let start = world + n * (0.002 * z0 + 0.01);
    let steps = u32(screen.contact.w);
    for (var i = 0u; i < steps; i++) {
        let q = start + l * ((f32(i) + jitter) / f32(steps) * screen.contact.y);
        let px = screen_pixel(q);
        if (px.z < 0.5) {
            break;
        }
        let surface = screen_texel(px.xy).y;
        let behind = screen_depth(q) - surface;
        if (behind > 0.002 * surface + 0.005 && behind < screen.misc.y + 0.01 * surface) {
            // Grazing light (the terminator of round shapes) can't be told from self-hits.
            let grazing = smoothstep(0.05, 0.3, dot(n, l));
            // Contact hardening: darkest where the occluder is closest, fading out towards
            // the end of the ray (the soft shadow map takes over from there).
            let t = (f32(i) + jitter) / f32(steps);
            let near = 1.0 - 0.6 * t * t;
            return 1.0 - screen.contact.z * fade * grazing * near;
        }
    }
    return 1.0;
}

// Decals in the lit shaders (render/decals; here so every lit shader has it). The decal resolve
// (decals/decals.wgsl) wrote, after the texels above, what the decals put on the surface each
// pixel shows: albedo, normal, mask and emission, premultiplied by their opacity (forward.w = 1
// in the header). `shade` blends them into the surface before lighting. Only the surface the
// prepass saw takes them (not a transparent one in front); with no decals the flag is 0 and
// the surface is untouched.

fn decal_surface(world: vec3<f32>, s: Surface) -> Surface {
    if (screen.forward.w < 0.5) {
        return s;
    }
    let px = screen_pixel(world);
    if (px.z < 0.5) {
        return s;
    }
    let p = vec2<u32>(px.xy);
    let width = u32(screen.misc.z);
    let i = p.y * width + p.x;
    let z = screen_depth(world);
    if (abs(screen.texels[i].y - z) > 0.02 * z + 0.05) {
        return s;
    }
    let base = width * u32(screen.misc.w) + 2u * i;
    let a = bitcast<vec2<u32>>(screen.texels[base]);
    let b = bitcast<vec2<u32>>(screen.texels[base + 1u]);
    if ((a.x | a.y | b.x | b.y) == 0u) {
        return s;
    }
    var out = s;
    let albedo = unpack4x8unorm(a.x);
    out.albedo = s.albedo * (1.0 - albedo.a) + albedo.rgb * albedo.rgb;
    let n = unpack4x8snorm(a.y);
    let na = n.w * 0.5 + 0.5;
    if (na > 0.004) {
        out.normal = normalize(s.normal * (1.0 - na) + n.xyz);
    }
    let m = unpack4x8unorm(b.x);
    out.metallic = s.metallic * (1.0 - m.a) + m.r;
    out.smoothness = s.smoothness * (1.0 - m.a) + m.g;
    out.occlusion = s.occlusion * (1.0 - m.a) + m.b;
    if (b.y != 0u) {
        out.emission = s.emission + unpack4x8unorm(b.y).rgb * exp2(f32(b.y >> 24u) - 128.0);
    }
    return out;
}
