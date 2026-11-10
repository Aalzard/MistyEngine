// Terrain trees, lit: appended to mesh.wgsl (lights, shadows, screen-space AO, fog) and
// tree_vertex.wgsl. Bark (opaque, normal mapped), leaf clusters (alpha to coverage, both
// sides, light shining through) and far impostors (baked albedo and normals, lit here).
//
// The look aims for the soft, "one piece" crowns of a painted landscape: leaf normals lean to
// the crown's dome, the crown darkens inwards and downwards (baked occlusion), sun shadows on
// leaves are sampled wide, and backlit clusters glow.

struct TreeOut {
    // Invariant: the leaves' depth pass and the lit leaves must agree to the bit (the lit ones
    // test for equal depth).
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // x = occlusion, y = cluster variation, z = 1 on leaves, w = LOD fade.
    @location(3) extra: vec4<f32>,
    // x = tree colour variation, y = height share.
    @location(4) misc: vec2<f32>,
    @location(5) clip_prev: vec3<f32>,
};

@vertex
fn vs_tree(v: TreeVertexIn, inst: TreeInstance) -> TreeOut {
    let p = tree_point(v, inst);
    var out: TreeOut;
    out.clip = view.view_proj * vec4<f32>(p.world, 1.0);
    out.clip_prev = prev_clip(p.prev);
    out.world = p.world;
    out.normal = p.normal;
    out.uv = v.uv;
    out.extra = vec4<f32>(v.extra.xyz, inst.scale.w);
    out.misc = vec2<f32>(inst.scale.z, v.wind.w);
    return out;
}

// Per-cluster colour variation (impostor.rs `leaf_variation`).
fn tree_leaf_variation(c: vec3<f32>, v: f32, amount: f32) -> vec3<f32> {
    let k = (v - 0.5) * amount;
    return c * vec3<f32>(1.0 + 0.32 * k, 1.0 + 0.14 * k, 1.0 - 0.2 * k) * (1.0 + 0.3 * k);
}

// Each tree a little warmer or cooler, lighter or darker.
fn tree_tint(c: vec3<f32>, v: f32) -> vec3<f32> {
    let k = (v - 0.5) * tree.motion.w;
    return c * vec3<f32>(1.0 + 0.18 * k, 1.0 + 0.06 * k, 1.0 - 0.16 * k) * (1.0 + 0.22 * k);
}

// Share of the sunlight that reaches the fragment beyond the shadow maps' own shade (crown
// shading: set by the leaves and impostors before `shade_tree`, 1 = all).
var<private> tree_sun_reach: f32 = 1.0;

// Crown shading (tree.shade.x): inner clusters (low baked occlusion `ao`) and low ones
// (`height` share of the tree) sit in the shade of the outer dome under a high sun. Returns
// (sun reach, occlusion for the sky light).
fn tree_crown_shade(ao: f32, height: f32) -> vec2<f32> {
    let k = tree.shade.x;
    if (k <= 0.0) {
        return vec2<f32>(1.0, ao);
    }
    let outer = smoothstep(0.3, 0.92, ao);
    let low = smoothstep(0.15, 0.85, height);
    let sun = mix(1.0, outer * mix(0.5, 1.0, low), k);
    let sky = pow(ao * mix(1.0, mix(0.55, 1.0, low), k), 1.0 + k);
    return vec2<f32>(sun, sky);
}

// Sun visibility over a wide disk (soft dappled light in the crown, no crisp speckle).
fn tree_shadow(i: u32, li: LightItem, world: vec3<f32>, n: vec3<f32>, l: vec3<f32>, radius: f32) -> f32 {
    if (li.shadow.x < 0.5) {
        return 1.0;
    }
    let n_dot_l = max(dot(n, l), 0.0);
    if (u32(li.dir_type.w) != 0u) {
        return local_shadow(i, li, world, n, n_dot_l, 1.0);
    }
    let t = normalize(cross(l, vec3<f32>(0.0, 1.0, 0.0) + vec3<f32>(1e-3, 0.0, 0.0)));
    let b = cross(l, t);
    // A little towards the light: a cluster doesn't shade itself.
    let p = world + l * radius * 0.6;
    var sum = 0.0;
    sum += sun_shadow(i, p + (t + b * 0.5) * radius, n, n_dot_l, 1.0);
    sum += sun_shadow(i, p + (-t - b * 0.5) * radius, n, n_dot_l, 1.0);
    sum += sun_shadow(i, p + (b - t * 0.5) * radius, n, n_dot_l, 1.0);
    sum += sun_shadow(i, p + (-b + t * 0.5) * radius, n, n_dot_l, 1.0);
    return sum * 0.25;
}

// Lighting of a tree surface: hemisphere ambient (baked crown occlusion and SSAO), the lights
// with wide soft shadows, cloud shadows, light through the leaves, fog.
fn shade_tree(world: vec3<f32>, albedo: vec3<f32>, n: vec3<f32>, occlusion: f32, transmission: vec3<f32>, wrap: f32, smoothness: f32, shadow_radius: f32) -> MeshTargets {
    let v = normalize(view.camera_pos.xyz - world);
    var ambient: vec3<f32>;
    if (n.y >= 0.0) {
        ambient = mix(lights.equator.rgb, lights.sky.rgb, n.y);
    } else {
        ambient = mix(lights.equator.rgb, lights.ground.rgb, -n.y);
    }
    let ssao = mix(1.0, screen_ao(world), 0.6);
    // Misty GI (when on) replaces the hemisphere: other crowns' shade and bounce light.
    let indirect = misty_ambient_world(ambient * lights.info.z, world, n);
    var color = indirect * albedo * indirect_occlusion(occlusion, ssao, albedo);
    let perceptual = 1.0 - smoothness;
    let roughness = max(perceptual * perceptual, 0.002);
    let count = u32(lights.info.x);
    for (var i = 0u; i < count; i++) {
        let li = lights.items[i];
        let kind = u32(li.dir_type.w);
        if (kind == 3u) {
            continue;
        }
        var l: vec3<f32>;
        var atten = 1.0;
        if (kind == 0u) {
            l = -normalize(li.dir_type.xyz);
            atten = cloud_shadow(world);
        } else {
            let to_light = li.pos_range.xyz - world;
            let dist = length(to_light);
            l = to_light / max(dist, 1e-4);
            let x = dist / max(li.pos_range.w, 1e-4);
            let window = clamp(1.0 - x * x * x * x, 0.0, 1.0);
            atten = window * window / (1.0 + 25.0 * x * x);
            if (kind == 2u) {
                atten *= smoothstep(li.spot.x, li.spot.y, dot(-l, normalize(li.dir_type.xyz)));
            }
        }
        if (atten <= 0.0) {
            continue;
        }
        var shadow = tree_shadow(i, li, world, n, l, shadow_radius);
        if (kind == 0u && li.spot.z > 0.5) {
            shadow *= far_sun_shadow(world, n, l);
        }
        // Light through the leaves: forward scattering towards the eye against the light,
        // plus diffuse light through the back of the cluster. It passes several layers of
        // leaves, so the crown's own shadow only dims it.
        // Soft: a wide forward lobe, and diffuse light through the back wrapping round.
        let back = pow(clamp(dot(v, -l), 0.0, 1.0), 2.5);
        let through = clamp((dot(-n, l) + 0.35) / 1.35, 0.0, 1.0);
        // (Under a canopy a small plant's shade is not its own: nothing comes through.)
        let floor = select(0.35, 0.0, tree.shade.y > 0.5);
        color += transmission * li.color.rgb * atten * mix(floor, 1.0, shadow) * (back * 1.6 + through * through * 0.7);
        atten *= shadow * direct_occlusion(ssao);
        if (kind == 0u) {
            atten *= tree_sun_reach;
        }
        // Wrapped diffuse: leaves scatter light around the crown's terminator.
        let n_dot_l = (dot(n, l) + wrap) / (1.0 + wrap);
        if (n_dot_l <= 0.0) {
            continue;
        }
        let h = normalize(l + v);
        let n_dot_h = max(dot(n, h), 0.0);
        let l_dot_h = max(dot(l, h), 0.0);
        let fresnel = 0.04 + 0.96 * pow(1.0 - l_dot_h, 5.0);
        let vis = 0.25 / max(l_dot_h * l_dot_h, 0.1);
        let spec = fresnel * d_ggx(n_dot_h, roughness) * vis * PI * smoothness;
        color += (albedo + vec3<f32>(spec)) * li.color.rgb * n_dot_l * atten;
    }
    color = cascade_debug(world, color);
    let fog = fog_visibility(fog_distance(world));
    color = mix(view.fog_color.rgb, color, fog);
    color = vfog_blend(color, vfog_eval(world), 1.0);
    var out: MeshTargets;
    out.color = vec4<f32>(color, 1.0);
    out.glow = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    out.velocity = vec2<f32>(0.0);
    if (ao_debug_view()) {
        out.color = vec4<f32>(vec3<f32>(ssao * occlusion), 1.0);
    }
    return out;
}

// Normal mapping without stored tangents (screen-space derivatives of position and uv).
fn tree_normal_mapped(n: vec3<f32>, world: vec3<f32>, uv: vec2<f32>, sampled: vec3<f32>, strength: f32) -> vec3<f32> {
    let dp1 = dpdx(world);
    let dp2 = dpdy(world);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    let dp2perp = cross(dp2, n);
    let dp1perp = cross(n, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-20));
    var m = sampled * 2.0 - 1.0;
    m = vec3<f32>(m.xy * strength, max(m.z, 1e-3));
    // Bitangent up the image (OpenGL normal maps); v grows down the image.
    return normalize(t * inv * m.x - b * inv * m.y + n * m.z);
}

// Normal maps carry x, y in rg (z rebuilt here) and two material maps in b and a
// (render/trees.rs `pack_channel`): the `sampled` value tree_normal_mapped expects.
fn tree_unpack_normal(s: vec4<f32>) -> vec3<f32> {
    let xy = s.xy * 2.0 - 1.0;
    return vec3<f32>(s.xy, sqrt(max(1.0 - dot(xy, xy), 0.0)) * 0.5 + 0.5);
}

// Offset parallax (one step, limited): the bark's height (albedo alpha, 0 = furrow, 1 = top
// of the plates, 0.5 = the mesh surface) shifts the texture against the view, about 2.5 cm
// deep, fading out by 20 m. The uv offset of a world offset comes from the derivatives.
fn tree_parallax(world: vec3<f32>, n: vec3<f32>, uv: vec2<f32>, height: f32) -> vec2<f32> {
    let dp1 = dpdx(world);
    let dp2 = dpdy(world);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    let det = duv1.x * duv2.y - duv1.y * duv2.x;
    let ok = abs(det) > 1e-14;
    let inv = select(0.0, 1.0 / det, ok);
    // World direction of +u and +v (per unit of uv).
    let pu = (dp1 * duv2.y - dp2 * duv1.y) * inv;
    let pv = (dp2 * duv1.x - dp1 * duv2.x) * inv;
    let to_eye = view.camera_pos.xyz - world;
    let v = normalize(to_eye);
    let fade = 1.0 - smoothstep(8.0, 20.0, length(to_eye));
    let o = (v - n * dot(v, n)) * (height - 0.5) * 0.025 * fade / max(dot(v, n), 0.4);
    let duv = vec2<f32>(dot(o, pu) / max(dot(pu, pu), 1e-12), dot(o, pv) / max(dot(pv, pv), 1e-12));
    return select(vec2<f32>(0.0), clamp(duv, vec2<f32>(-0.05), vec2<f32>(0.05)), ok);
}

@fragment
fn fs_tree_bark(in: TreeOut) -> MeshTargets {
    // Sample (and take derivatives) before any discard. Gradients of the unshifted uv: the
    // parallax shift doesn't change the mip.
    let bias = exp2(view.view_tan.z);
    let gx = dpdx(in.uv) * bias;
    let gy = dpdy(in.uv) * bias;
    let n0 = normalize(in.normal);
    let height = textureSampleGrad(tree_albedo, tree_sampler, in.uv, gx, gy).a;
    let uv = in.uv + tree_parallax(in.world, n0, in.uv, height);
    let base = textureSampleGrad(tree_albedo, tree_sampler, uv, gx, gy);
    let material = textureSampleGrad(tree_normal_map, tree_sampler, uv, gx, gy);
    let n = tree_normal_mapped(n0, in.world, in.uv, tree_unpack_normal(material), 1.0);
    if (tree_faded(in.extra.w, in.clip.xy)) {
        discard;
    }
    // Occlusion map: the ambient light, and a little cavity darkening of the sunlit bark.
    let cavity = material.a;
    let albedo = base.rgb * tree.bark.rgb * mix(1.0, cavity, 0.2);
    var out = shade_tree(in.world, albedo, n, in.extra.x * mix(1.0, cavity, 0.5), vec3<f32>(0.0), 0.15, 1.0 - material.b, 0.05);
    out.velocity = velocity_of(in.clip, in.clip_prev);
    return out;
}

// Coverage of a leaf fragment: an alpha test at 0.5 with a thin antialiased rim (`far`
// 0..1 widens it a little). Broad partial coverage is avoided on purpose: the hardware
// dithers alpha to coverage into a visible checker pattern; TAA smooths the hard edges.
fn leaf_coverage(alpha: f32, clip: vec4<f32>, far: f32) -> f32 {
    let w = max(fwidth(alpha), 1e-4) * (0.5 + 0.5 * far);
    return clamp((alpha - 0.5) / w + 0.5, 0.0, 1.0) * step(0.35, alpha);
}

// The leaves' depth, drawn before the trees (render/trees.rs `draw_main`): the coverage of
// `fs_tree_leaves` (the same inputs, so the same samples pass), only its alpha written. The lit
// leaves then test for equal depth, so each sample is shaded once, by the leaf it shows.
@fragment
fn fs_tree_leaves_depth(in: TreeOut) -> MeshTargets {
    let tex = textureSampleBias(tree_albedo, tree_sampler, in.uv, view.view_tan.z);
    let far = 0.3 * smoothstep(20.0, 90.0, length(in.world - view.camera_pos.xyz));
    let coverage = leaf_coverage(tex.a, in.clip, far);
    if (coverage < 0.01 || tree_faded(in.extra.w, in.clip.xy)) {
        discard;
    }
    var out: MeshTargets;
    out.color = vec4<f32>(0.0, 0.0, 0.0, coverage);
    out.glow = vec4<f32>(0.0);
    return out;
}

@fragment
fn fs_tree_leaves(in: TreeOut, @builtin(front_facing) front: bool) -> MeshTargets {
    let tex = textureSampleBias(tree_albedo, tree_sampler, in.uv, view.view_tan.z);
    let material = textureSampleBias(tree_normal_map, tree_sampler, in.uv, view.view_tan.z);
    let sampled = tree_unpack_normal(material);
    // Mostly sharp (alpha to coverage only antialiases the edges): wide partial coverage
    // reads as a dither pattern; a little softer far away.
    let far = 0.3 * smoothstep(20.0, 90.0, length(in.world - view.camera_pos.xyz));
    let coverage = leaf_coverage(tex.a, in.clip, far);
    // The dome normal with the leaves' own shape (fold, midrib, each leaf's tilt; either side
    // of the card): enough that single leaves catch the light differently and read as leaves,
    // less far away where they are a few pixels (the crown lights as one soft volume there).
    var n = normalize(in.normal);
    let detail = tree_normal_mapped(n, in.world, in.uv, sampled, select(-1.0, 1.0, front));
    n = normalize(mix(n, detail, mix(0.6, 0.3, far / 0.3)));
    if (coverage < 0.01 || tree_faded(in.extra.w, in.clip.xy)) {
        discard;
    }
    var albedo = tex.rgb * tree.leaf.rgb;
    albedo = tree_leaf_variation(albedo, in.extra.y, tree.motion.w);
    albedo = tree_tint(albedo, in.misc.x);
    // Inner, lower clusters are a little paler and bluer (sky light only), the outer warmer.
    let ao = in.extra.x;
    // Translucency map: thin blades glow, veins and twigs less.
    let transmission = albedo * vec3<f32>(1.05, 1.0, 0.6) * tree.leaf.w * mix(0.35, 1.0, ao) * material.a;
    // Roughness map: waxy upper sides with a soft sheen and small glints where single leaves
    // face the sun (the leaves' own normals break the dome's highlight up), matte undersides;
    // duller far away (no sparkle).
    let smoothness = (1.0 - material.b) * mix(0.85, 0.5, far / 0.3);
    let crown = tree_crown_shade(ao, in.misc.y);
    tree_sun_reach = crown.x;
    // A sharper terminator with crown shading: sunlit tops, dark undersides.
    let wrap = mix(0.45, 0.2, tree.shade.x);
    var out = shade_tree(in.world, albedo, n, crown.y, transmission, wrap, smoothness, 0.35);
    out.color.a = coverage;
    out.glow.a = coverage;
    out.velocity = velocity_of(in.clip, in.clip_prev);
    return out;
}

struct ImpostorOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) uv0: vec2<f32>,
    @location(2) uv1: vec2<f32>,
    // x = blend towards the second view, y = tree colour variation, z = LOD fade.
    @location(3) misc: vec3<f32>,
    @location(4) right: vec3<f32>,
    @location(5) toward: vec3<f32>,
    @location(6) clip_prev: vec3<f32>,
};

@vertex
fn vs_tree_impostor(@builtin(vertex_index) vi: u32, inst: TreeInstance) -> ImpostorOut {
    let to_eye = view.camera_pos.xyz - inst.foot.xyz;
    let q = impostor_point(vi, inst, to_eye.xz);
    var out: ImpostorOut;
    out.clip = view.view_proj * vec4<f32>(q.world, 1.0);
    out.clip_prev = prev_clip(q.world);
    out.world = q.world;
    out.uv0 = q.uv0;
    out.uv1 = q.uv1;
    out.misc = vec3<f32>(q.blend, inst.scale.z, inst.scale.w);
    out.right = q.right;
    out.toward = q.toward;
    return out;
}

@fragment
fn fs_tree_impostor(in: ImpostorOut) -> MeshTargets {
    // The two nearest baked views, blended (both sampled: smooth derivatives, no popping
    // while walking round a tree).
    let tex = mix(
        textureSampleBias(tree_albedo, tree_sampler, in.uv0, view.view_tan.z),
        textureSampleBias(tree_albedo, tree_sampler, in.uv1, view.view_tan.z),
        in.misc.x,
    );
    let nt = mix(
        textureSampleBias(tree_normal_map, tree_sampler, in.uv0, view.view_tan.z),
        textureSampleBias(tree_normal_map, tree_sampler, in.uv1, view.view_tan.z),
        in.misc.x,
    );
    let coverage = leaf_coverage(tex.a, in.clip, 0.3);
    if (coverage < 0.01 || tree_faded(in.misc.z, in.clip.xy)) {
        discard;
    }
    let m = nt.xyz * 2.0 - 1.0;
    let n = normalize(in.right * m.x + vec3<f32>(0.0, 1.0, 0.0) * m.y + in.toward * m.z);
    // sRGB textures: the samples are linear. The bake holds the textures' colours with the
    // cluster variation; tints apply here.
    var albedo = tex.rgb * tree.leaf.rgb;
    albedo = tree_tint(albedo, in.misc.y);
    let ao = nt.a;
    let transmission = albedo * vec3<f32>(1.05, 1.0, 0.6) * tree.leaf.w * mix(0.35, 1.0, ao);
    // Height share of the tree at this texel (the quad's v runs down the impostor).
    let height = (tree.impostor.z + (1.0 - in.uv0.y) * tree.impostor.y) / max(tree.bark.w, 1e-3);
    let crown = tree_crown_shade(ao, height);
    tree_sun_reach = crown.x;
    let wrap = mix(0.45, 0.2, tree.shade.x);
    var out = shade_tree(in.world, albedo, n, crown.y, transmission, wrap, 0.12, 0.6);
    out.color.a = coverage;
    out.glow.a = coverage;
    out.velocity = velocity_of(in.clip, in.clip_prev);
    return out;
}
