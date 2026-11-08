// Decal resolve (render/decals): a deferred-style screen-space pass over the screen-space
// prepass (depth + world normals) before the main pass. Each 8×8 tile lists the decals whose
// screen rectangle covers it, then every pixel walks that list in draw order (lowest priority
// first), projects its world position into each decal box and blends the decal's albedo,
// normal, mask (metallic, smoothness, occlusion) and emission. The result goes into the
// screen-space texel buffer after the AO texels (4 words per pixel, premultiplied by the
// decal opacity), where the lit shaders pick it up before lighting (decal_apply.wgsl), like
// Unity's DBuffer decals in a forward renderer.

const MAX_DECALS: u32 = 1024u;
const LAYER_SIZE: f32 = 512.0;

struct Decal {
    // World -> the decal's unit box (-0.5..0.5 on each axis).
    world_to_box: mat4x4<f32>,
    // xyz = the box's world X (unit), w = base map layer (-1 = none).
    right: vec4<f32>,
    // xyz = the box's world Y (unit), w = normal map layer (-1 = none).
    up: vec4<f32>,
    // xyz = the projection direction (box +Z, unit), w = mask map layer (-1 = none).
    forward: vec4<f32>,
    // rgb = linear tint, a = tint alpha × opacity.
    color: vec4<f32>,
    // xy = tiling, zw = offset.
    uv: vec4<f32>,
    // x = normal strength, y = metallic, z = smoothness, w = affects flags (1 albedo,
    // 2 normal, 4 mask).
    surface: vec4<f32>,
    // rgb = linear emission × intensity, w = texels per metre of the layers on this decal.
    emission: vec4<f32>,
    // x / y = cosines where the angle fade starts / ends (x > 1.5: no angle fade),
    // z = draw distance, w = distance where the distance fade starts.
    fade: vec4<f32>,
    // Screen rectangle in pixels (min x, min y, max x, max y).
    rect: vec4<f32>,
};

struct DecalParams {
    // xy = texel size of the screen-space buffer, z = decals.
    size: vec4<f32>,
    // x = world size of a pixel one metre away (perspective) or anywhere (orthographic),
    // y = 1 when orthographic.
    pixel: vec4<f32>,
    // x = first word of the decal texels in the screen-space buffer.
    words: vec4<u32>,
};

@group(1) @binding(0) var dc_depth: texture_depth_2d;
@group(1) @binding(1) var dc_normals: texture_2d<f32>;
@group(1) @binding(2) var dc_layers: texture_2d_array<f32>;
@group(1) @binding(3) var dc_sampler: sampler;
@group(1) @binding(4) var<uniform> dc: DecalParams;
@group(1) @binding(5) var<storage, read> decals: array<Decal>;
@group(1) @binding(6) var<storage, read_write> dc_out: array<u32>;

var<workgroup> tile_list: array<atomic<u32>, 32>;

fn dc_layer(layer: f32, uv: vec2<f32>, lod: f32) -> vec4<f32> {
    return textureSampleLevel(dc_layers, dc_sampler, uv, i32(layer + 0.5), lod);
}

@compute @workgroup_size(8, 8)
fn cs_decals(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) li: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    if (li < 32u) {
        atomicStore(&tile_list[li], 0u);
    }
    workgroupBarrier();
    let count = min(u32(dc.size.z), MAX_DECALS);
    let lo = vec2<f32>(wid.xy * 8u);
    let hi = lo + 8.0;
    for (var i = li; i < count; i += 64u) {
        let r = decals[i].rect;
        if (r.x < hi.x && r.z >= lo.x && r.y < hi.y && r.w >= lo.y) {
            atomicOr(&tile_list[i / 32u], 1u << (i % 32u));
        }
    }
    workgroupBarrier();
    let dims = vec2<u32>(dc.size.xy);
    if (any(gid.xy >= dims)) {
        return;
    }
    let pixel = gid.y * dims.x + gid.x;
    let word = dc.words.x + pixel * 4u;
    var albedo = vec4<f32>(0.0);
    var normal = vec4<f32>(0.0);
    var mask = vec4<f32>(0.0);
    var emission = vec3<f32>(0.0);
    let z = textureLoad(dc_depth, gid.xy, 0);
    let encoded = textureLoad(dc_normals, gid.xy, 0);
    if (z < 1.0 && encoded.w != 0.0) {
        let ndc = vec2<f32>(
            (f32(gid.x) + 0.5) / f32(dims.x) * 2.0 - 1.0,
            1.0 - (f32(gid.y) + 0.5) / f32(dims.y) * 2.0,
        );
        let world = unproject(ndc, z);
        let n = normalize(encoded.xyz);
        var footprint = dc.pixel.x;
        if (dc.pixel.y < 0.5) {
            footprint *= length(world - view.camera_pos.xyz);
        }
        let distance = length(world - view.camera_pos.xyz);
        let words = (count + 31u) / 32u;
        for (var w = 0u; w < words; w++) {
            var bits = atomicLoad(&tile_list[w]);
            while (bits != 0u) {
                let b = firstTrailingBit(bits);
                bits &= bits - 1u;
                let d = decals[w * 32u + b];
                let local = (d.world_to_box * vec4<f32>(world, 1.0)).xyz;
                if (any(abs(local) > vec3<f32>(0.5))) {
                    continue;
                }
                // Fades: angle between the surface and the projection, camera distance.
                let facing = dot(n, -d.forward.xyz);
                var fade = select(smoothstep(d.fade.y, d.fade.x, facing), 1.0, d.fade.x > 1.5);
                fade *= 1.0 - smoothstep(d.fade.w, d.fade.z, distance);
                if (fade <= 0.0) {
                    continue;
                }
                let uv = vec2<f32>(local.x + 0.5, 0.5 - local.y) * d.uv.xy + d.uv.zw;
                // Mip from the pixel's footprint on the decal (wider at grazing angles).
                let lod = log2(max(footprint * d.emission.w / max(abs(facing), 0.25), 1e-4));
                var base = d.color;
                if (d.right.w >= 0.0) {
                    let t = dc_layer(d.right.w, uv, lod);
                    base *= vec4<f32>(pow(t.rgb, vec3<f32>(2.2)), t.a);
                }
                let a = clamp(base.a * fade, 0.0, 1.0);
                if (a <= 0.0) {
                    continue;
                }
                let flags = u32(d.surface.w + 0.5);
                if ((flags & 1u) != 0u) {
                    albedo = vec4<f32>(albedo.rgb * (1.0 - a) + base.rgb * a, albedo.a * (1.0 - a) + a);
                }
                if ((flags & 2u) != 0u) {
                    var ts = vec3<f32>(0.0, 0.0, 1.0);
                    if (d.up.w >= 0.0) {
                        ts = dc_layer(d.up.w, uv, lod).xyz * 2.0 - 1.0;
                    }
                    // The decal's tangent frame on this surface: its X and Y laid onto it.
                    let t = normalize(d.right.xyz - n * dot(n, d.right.xyz) + vec3<f32>(1e-5, 0.0, 0.0));
                    var bt = cross(n, t);
                    bt = select(-bt, bt, dot(bt, d.up.xyz) >= 0.0);
                    let dn = normalize(t * ts.x + bt * ts.y + n * max(ts.z, 1e-3));
                    let na = a * d.surface.x;
                    normal = vec4<f32>(normal.xyz * (1.0 - na) + dn * na, normal.w * (1.0 - na) + na);
                }
                if ((flags & 4u) != 0u) {
                    var m = vec3<f32>(d.surface.y, d.surface.z, 1.0);
                    if (d.forward.w >= 0.0) {
                        let t = dc_layer(d.forward.w, uv, lod);
                        m = vec3<f32>(m.x * t.r, m.y * t.a, t.g);
                    }
                    mask = vec4<f32>(mask.rgb * (1.0 - a) + m * a, mask.a * (1.0 - a) + a);
                }
                emission = emission * (1.0 - a) + d.emission.rgb * a;
            }
        }
    }
    dc_out[word] = pack4x8unorm(vec4<f32>(sqrt(max(albedo.rgb, vec3<f32>(0.0))), albedo.a));
    dc_out[word + 1u] = pack4x8snorm(vec4<f32>(normal.xyz, normal.w * 2.0 - 1.0));
    dc_out[word + 2u] = pack4x8unorm(mask);
    // Emission: rgb over a power of two, the exponent (+ 128) in the top byte; 0 = none.
    let peak = max(emission.r, max(emission.g, emission.b));
    var e = 0u;
    if (peak > 1e-4) {
        let exponent = clamp(ceil(log2(peak)), -100.0, 100.0);
        let rgb = pack4x8unorm(vec4<f32>(emission / exp2(exponent), 0.0)) & 0x00ffffffu;
        e = rgb | (u32(exponent + 128.0) << 24u);
    }
    dc_out[word + 3u] = e;
}
