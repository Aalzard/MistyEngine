// Standard shader with optional features (render/material_v2.rs). Appended after mesh.wgsl and
// its includes only for the variants whose MAT_FEATURES is not 0 (that line is replaced in
// the copy), so the base shaders never see these bindings. Group 2 is the material: the six
// Standard bindings, then the feature values and maps (1×1 white / flat when unassigned).
//
//   Detail maps      detail albedo (× 2 in sRGB terms, grey = no change) and detail normal on
//                    their own tiling, masked by the detail mask's R.
//   Parallax         parallax occlusion mapping: the height map's R (white = high) marched
//                    along the view ray in tangent space, then every map is read at the hit.
//   Clear coat       a dielectric layer on top (mesh.wgsl `feature_light`, `coat_ambient`).
//   Translucency     wrap lighting tinted by the colour, plus light through the surface
//                    (Surface.transmission) scaled down by the thickness map's R.
//   Anisotropy       stretched highlights along a uv direction (or radial).
//   Specular         dielectric reflectance colour × strength (KHR_materials_specular).

struct LitParams {
    // xy = detail tiling, zw = detail offset (on the mesh uv).
    detail_st: vec4<f32>,
    // x = detail normal scale, y = detail albedo on, z = detail normal on.
    detail: vec4<f32>,
    // x = height scale (uv units), y = fewest steps, z = most steps.
    parallax: vec4<f32>,
    // x = strength, y = roughness (alpha), z = coat normal scale (0 = the geometry's normal).
    coat: vec4<f32>,
    // rgb = linear colour, w = transmission strength.
    translucency: vec4<f32>,
    // x = wrap (0..1), y = thickness (× map R).
    subsurface: vec4<f32>,
    // x = strength (-1..1: negative stretches across the direction), y = cos, z = sin of the
    // direction's angle in the uv plane, w = 1 when radial (round the uv centre).
    aniso: vec4<f32>,
    // rgb = specular colour × strength.
    specular: vec4<f32>,
};

@group(2) @binding(6) var<uniform> lit: LitParams;
@group(2) @binding(7) var detail_albedo_map: texture_2d<f32>;
@group(2) @binding(8) var detail_normal_map: texture_2d<f32>;
@group(2) @binding(9) var detail_mask_map: texture_2d<f32>;
@group(2) @binding(10) var height_map: texture_2d<f32>;
@group(2) @binding(11) var coat_normal_map: texture_2d<f32>;
@group(2) @binding(12) var thickness_map: texture_2d<f32>;

// Tangent-space normal from a normal map's XY (Z rebuilt, BC5 maps store only X and Y).
fn lit_ts_normal(xy01: vec2<f32>, scale: f32) -> vec3<f32> {
    let xy = (xy01 * 2.0 - 1.0) * scale;
    let z = sqrt(saturate(1.0 - dot(xy, xy)));
    return vec3<f32>(xy, max(z, 1e-3));
}

// Tangent frame (t = +u, b = up in the image, n) with `n` the side's unit normal.
fn lit_frame(in: MeshOut, n: vec3<f32>) -> mat3x3<f32> {
    var t0 = in.tangent.xyz - n * dot(n, in.tangent.xyz);
    if (dot(t0, t0) < 1e-10) {
        // No tangents: any direction across the normal.
        t0 = cross(n, select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9));
    }
    let t = normalize(t0);
    let sign = select(1.0, -1.0, in.tangent.w < 0.0);
    let b = cross(n, t) * sign;
    return mat3x3<f32>(t, b, n);
}

fn lit_view_dir(world: vec3<f32>) -> vec3<f32> {
    if (view.camera_pos.w > 0.5) {
        return normalize(-(view.inv_view_proj * vec4<f32>(0.0, 0.0, 1.0, 0.0)).xyz);
    }
    return normalize(view.camera_pos.xyz - world);
}

// Parallax occlusion mapping: march the view ray down through the height field (more steps
// at grazing angles) and return the uv where it enters, interpolated between the last two
// steps.
fn lit_parallax(in: MeshOut, frame: mat3x3<f32>, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec2<f32> {
    let d = draws[in.draw];
    let v = lit_view_dir(in.world);
    let vt = vec3<f32>(dot(v, frame[0]), dot(v, frame[1]), dot(v, frame[2]));
    if (vt.z <= 0.0 || lit.parallax.x == 0.0) {
        return uv;
    }
    // The bitangent points up in the image, v runs down it.
    let shift = vec2<f32>(vt.x, -vt.y) / max(vt.z, 0.08) * lit.parallax.x * d.uv_st.xy;
    let steps = clamp(mix(lit.parallax.z, lit.parallax.y, vt.z), 1.0, 64.0);
    let count = i32(ceil(steps));
    let layer = 1.0 / f32(count);
    var depth = 0.0;
    var cur = uv;
    var map_depth = 1.0 - textureSampleGrad(height_map, material_sampler, cur, dx, dy).r;
    var prev = uv;
    var prev_map = map_depth;
    var prev_depth = 0.0;
    for (var i = 0; i < count; i++) {
        if (depth >= map_depth) {
            break;
        }
        prev = cur;
        prev_map = map_depth;
        prev_depth = depth;
        cur -= shift * layer;
        depth += layer;
        map_depth = 1.0 - textureSampleGrad(height_map, material_sampler, cur, dx, dy).r;
    }
    let after = map_depth - depth;
    let before = prev_map - prev_depth;
    let denom = after - before;
    let w = select(0.0, after / denom, abs(denom) > 1e-6);
    return mix(cur, prev, clamp(w, 0.0, 1.0));
}

// Standard surface plus the variant's features.
fn lit_surface(in: MeshOut, front: bool) -> Surface {
    let d = draws[in.draw];
    // Explicit gradients (taken here, in uniform control flow), with the material mip bias.
    let k = exp2(view.view_tan.z);
    let dx = dpdx(in.uv) * k;
    let dy = dpdy(in.uv) * k;
    var n0 = normalize(in.normal);
    if (!front) {
        n0 = -n0;
    }
    let frame = lit_frame(in, n0);
    var uv = in.uv;
    if ((MAT_FEATURES & FEAT_PARALLAX) != 0u) {
        uv = lit_parallax(in, frame, uv, dx, dy);
    }
    var t: MaterialSamples;
    t.base = textureSampleGrad(albedo_map, material_sampler, uv, dx, dy);
    t.normal = textureSampleGrad(normal_map, material_sampler, uv, dx, dy).xyz;
    t.metal_gloss = textureSampleGrad(metallic_map, material_sampler, uv, dx, dy);
    t.occlusion = textureSampleGrad(occlusion_map, material_sampler, uv, dx, dy).g;
    t.emission = textureSampleGrad(emission_map, material_sampler, uv, dx, dy).rgb;
    var s = standard_surface_from(in, front, t);

    // The mesh uv before the material's tiling (detail tiling and radial anisotropy use it).
    let tiling = select(vec2<f32>(1.0), d.uv_st.xy, abs(d.uv_st.xy) > vec2<f32>(1e-6));
    let raw = (uv - d.uv_st.zw) / tiling;
    if ((MAT_FEATURES & FEAT_DETAIL) != 0u) {
        let scale = lit.detail_st.xy / tiling;
        let duv = raw * lit.detail_st.xy + lit.detail_st.zw;
        let mask = textureSampleGrad(detail_mask_map, material_sampler, uv, dx, dy).r;
        let da = textureSampleGrad(detail_albedo_map, material_sampler, duv, dx * scale, dy * scale).rgb;
        // × 2 in sRGB terms (4.59 in linear): mid grey leaves the albedo unchanged.
        s.albedo *= mix(vec3<f32>(1.0), da * 4.5947938, mask * lit.detail.y);
        var base = vec3<f32>(0.0, 0.0, 1.0);
        if (d.params.w != 0.0) {
            base = lit_ts_normal(t.normal.xy, d.params.w);
        }
        if (lit.detail.z > 0.5) {
            let dn = textureSampleGrad(detail_normal_map, material_sampler, duv, dx * scale, dy * scale).xy;
            let detail = lit_ts_normal(dn, lit.detail.x * mask);
            // Whiteout blend: the detail's slopes added to the base's.
            base = normalize(vec3<f32>(base.xy + detail.xy, base.z * detail.z));
        }
        s.normal = normalize(frame * base);
    }
    if ((MAT_FEATURES & FEAT_CLEARCOAT) != 0u) {
        s.coat = saturate(lit.coat.x);
        s.coat_roughness = max(lit.coat.y, 0.002);
        s.coat_normal = n0;
        if (lit.coat.z != 0.0) {
            let cn = textureSampleGrad(coat_normal_map, material_sampler, uv, dx, dy).xy;
            s.coat_normal = normalize(frame * lit_ts_normal(cn, lit.coat.z));
        }
    }
    if ((MAT_FEATURES & FEAT_TRANSLUCENCY) != 0u) {
        let thick = saturate(lit.subsurface.y * textureSampleGrad(thickness_map, material_sampler, uv, dx, dy).r);
        s.transmission = lit.translucency.rgb * lit.translucency.w * (1.0 - thick);
        s.wrap = saturate(lit.subsurface.x);
        s.wrap_color = lit.translucency.rgb;
    }
    if ((MAT_FEATURES & FEAT_ANISOTROPY) != 0u) {
        var dir = lit.aniso.yz;
        if (lit.aniso.w > 0.5) {
            // Radial: away from the uv centre, turned by the angle.
            let p = raw - vec2<f32>(0.5);
            let len = length(p);
            let r = select(vec2<f32>(1.0, 0.0), p / len, len > 1e-5);
            dir = vec2<f32>(r.x * lit.aniso.y - r.y * lit.aniso.z, r.x * lit.aniso.z + r.y * lit.aniso.y);
        }
        // u runs along the tangent, v down the image (against the bitangent).
        var w = frame[0] * dir.x - frame[1] * dir.y;
        if (lit.aniso.x < 0.0) {
            w = cross(s.normal, w);
        }
        w -= s.normal * dot(s.normal, w);
        s.aniso_dir = select(frame[0], normalize(w), dot(w, w) > 1e-10);
        s.anisotropy = saturate(abs(lit.aniso.x));
    }
    if ((MAT_FEATURES & FEAT_SPECULAR) != 0u) {
        s.specular = lit.specular.rgb;
    }
    return s;
}

@fragment
fn fs_lit(in: MeshOut, @builtin(front_facing) front: bool) -> MeshTargets {
    var out = shade(in.world, lit_surface(in, front));
    out.velocity = mesh_velocity(in);
    return out;
}

// Cutout with features: holes where alpha is below the cutoff.
@fragment
fn fs_lit_cutout(in: MeshOut, @builtin(front_facing) front: bool) -> MeshTargets {
    let s = lit_surface(in, front);
    let d = draws[in.draw];
    if (s.alpha < d.surface.y || lod_dithered_out(d.extra.z, in.clip.xy)) {
        discard;
    }
    var out = shade(in.world, s);
    out.color.a = 1.0;
    out.velocity = mesh_velocity(in);
    return out;
}
