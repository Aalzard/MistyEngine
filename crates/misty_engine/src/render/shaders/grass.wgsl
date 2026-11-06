// Grass: appended to mesh.wgsl, grass_field.wgsl and grass_blade.wgsl (lit by `shade_grass`,
// receives shadows). Blades are textured from the built-in grass atlas: eight photographed
// specimens (four leaf blades, two timothy heads, two open panicles) with alpha-to-coverage
// edges. Colour comes from soft lush / dry patches across the field, per tuft and per blade
// jitter and a root-to-tip gradient; gusts that roll across the field bend the blades and
// lift their sheen, so the waves show. Sunlight shines through backlit blades.

const CELL_AVG = array<vec3<f32>, 8>(
    vec3<f32>(0.168, 0.224, 0.021),
    vec3<f32>(0.245, 0.326, 0.017),
    vec3<f32>(0.288, 0.299, 0.019),
    vec3<f32>(0.151, 0.208, 0.026),
    vec3<f32>(0.372, 0.279, 0.076),
    vec3<f32>(0.369, 0.301, 0.090),
    vec3<f32>(0.232, 0.197, 0.061),
    vec3<f32>(0.326, 0.257, 0.085),
);

struct GrassOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    // x = atlas u, y = atlas v, z = 0 at the root .. 1 at the tip, w = distance fade 0..1.
    @location(3) uv: vec4<f32>,
    // Atlas cell.
    @location(4) @interpolate(flat) cell: u32,
    // x = 0 close up .. 1 where blades get smaller than a few pixels (softer shading),
    // y = height above the ground in metres (canopy shade), z = gust 0..1, w = real / drawn
    // width (thin blades).
    @location(5) misc: vec4<f32>,
    // Across the blade (world), for the normal detail.
    @location(6) across: vec3<f32>,
    // Last frame's clip position (xyw): where the wind had the vertex a frame ago.
    @location(7) clip_prev: vec3<f32>,
};

@vertex
fn vs_grass(@builtin(vertex_index) vi: u32, b: BladeIn) -> GrassOut {
    let v = grass_blade(vi, b);
    let face = normalize(cross(v.across, v.tangent));

    var out: GrassOut;
    out.clip = view.view_proj * vec4<f32>(v.p, 1.0);
    // Motion vector: the blade as the wind had it last frame (terrains don't move).
    out.clip_prev = prev_clip(grass_blade_at(vi, b, grass.origin.w - grass.screen.w).p);
    out.world = v.p;
    // Rounded across the blade, so a card is not one flat shade.
    out.normal = normalize(face + v.across * v.side * 0.8);
    // Far blades are a few pixels wide: their differences turn into flicker, so they move
    // towards the local average (patches stay, single blades blend in).
    let far = smoothstep(4.0, 35.0, v.dist);
    let phase = b.shape.w;
    let dry_mix = mix(b.shape.z, 0.3 + 0.4 * b.shape.z, far * 0.6);
    let patches = grass_patches(b.position.xz, grass.lush.w);
    var base = grass_base_color(patches, dry_mix, grass.healthy.rgb, grass.dry.rgb, grass.lush.rgb, grass.amounts.xy);
    // Tufts and single blades: warmer / cooler hue and lighter / darker value.
    let cv = grass.look.w * (1.0 - 0.5 * far);
    let tint = b.extra.w;
    let hue = (fract(phase * 13.7) - 0.5) * 1.2 + tint * 0.4;
    let value = (fract(phase * 7.31 + 0.37) - 0.5) * 0.7 + tint * 0.3;
    base *= vec3<f32>(1.0 + 0.24 * hue * cv, 1.0 + 0.04 * hue * cv, 1.0 - 0.35 * hue * cv)
        * max(1.0 + 0.45 * value * cv, 0.3);
    let seed_color = grass.seed.rgb * (1.0 + 0.12 * tint)
        * mix(vec3<f32>(1.0), vec3<f32>(1.06, 1.0, 0.85), patches.y * grass.amounts.y);
    out.color = select(base, seed_color, v.seed_head);
    out.misc = vec4<f32>(far, v.above, v.gust, v.thin);
    out.uv = vec4<f32>((f32(v.cell) + v.side + 0.5) / 8.0, 1.0 - v.t, v.t, v.fade);
    out.cell = v.cell;
    out.across = v.across * v.side * 2.0;
    return out;
}

// Lighting of a blade: hemisphere ambient (joined by SSAO and the canopy), the lights with
// cheap hard-filtered shadows, cloud shadows, light through the blade, a soft sheen, fog.
// Probes, screen-space GI / reflections, lightmaps and rectangle lights are left out: on dense
// grass the full `shade` cost several times the rest of the frame and changed little.
fn shade_grass(
    world: vec3<f32>,
    albedo: vec3<f32>,
    n: vec3<f32>,
    occlusion: f32,
    transmission: vec3<f32>,
    smoothness: f32,
    alpha: f32,
) -> MeshTargets {
    let v = normalize(view.camera_pos.xyz - world);
    var ambient: vec3<f32>;
    if (n.y >= 0.0) {
        ambient = mix(lights.equator.rgb, lights.sky.rgb, n.y);
    } else {
        ambient = mix(lights.equator.rgb, lights.ground.rgb, -n.y);
    }
    let ssao = screen_ao(world);
    var color = ambient * lights.info.z * albedo * indirect_occlusion(occlusion, ssao, albedo);
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
        atten *= fast_shadow_factor(i, li, world, n, l) * direct_occlusion(ssao);
        if (kind == 0u) {
            atten *= grass_near_shadow(world, l);
        }
        // Forward scattering towards the eye when looking into the light, plus diffuse light
        // through the back face.
        let back = pow(clamp(dot(v, -l), 0.0, 1.0), 5.0);
        let through = max(dot(-n, l), 0.0);
        color += transmission * li.color.rgb * atten * (back * 2.5 + through * 0.6);
        let n_dot_l = dot(n, l);
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
    let vf = vfog_eval(world);
    color = vfog_blend(color, vf, 1.0);
    var out: MeshTargets;
    out.color = vec4<f32>(color, alpha);
    out.glow = vec4<f32>(0.0, 0.0, 0.0, alpha);
    out.velocity = vec2<f32>(0.0);
    if (ao_debug_view()) {
        out.color = vec4<f32>(vec3<f32>(ssao), alpha);
    }
    return out;
}

@fragment
fn fs_grass(in: GrassOut, @builtin(front_facing) front: bool) -> MeshTargets {
    let tex = textureSample(grass_atlas, grass_sampler, in.uv.xy);
    // Two texels across the blade: the alpha slope is the blade's thickness profile, used as
    // a normal map (rounded edges, a crease along the midrib) and a thickness map.
    let across_uv = vec2<f32>(2.0 / f32(textureDimensions(grass_atlas).x), 0.0);
    let next = textureSample(grass_atlas, grass_sampler, in.uv.xy + across_uv).a;
    let far = in.misc.x;
    // Sharpened alpha keeps thin shapes solid under minification (alpha to coverage).
    let sharp = clamp((tex.a - 0.5) / max(fwidth(tex.a), 1e-4) + 0.5, 0.0, 1.0);
    // Far away the filtered alpha goes straight to coverage (soft, no crawling edges).
    var coverage = mix(sharp, clamp(tex.a * 1.6, 0.0, 1.0), mix(0.35, 1.0, far)) * in.misc.w;
    // Alpha to coverage turns partial coverage into a fixed dot pattern; with TAA a noise that
    // changes every frame lets the history average it into smooth translucency instead.
    if (grass.screen.y > 0.5 && coverage < 0.98) {
        let n = grass_hash(in.clip.xy + fract(grass.origin.w * 13.7) * vec2<f32>(71.3, 37.9));
        coverage = clamp(coverage + (n - 0.5) * 0.3, 0.0, 1.0);
    }
    if (coverage < 0.01) {
        discard;
    }
    let t = in.uv.z;
    let cell = in.cell;
    let seed_head = cell >= 4u;
    let avg = CELL_AVG[cell];
    // The photo's detail (veins, spikelets, browned tips) on top of the chosen colour.
    let detail = clamp(tex.rgb / max(avg, vec3<f32>(0.01)), vec3<f32>(0.25), vec3<f32>(2.5));
    let lum = dot(tex.rgb, vec3<f32>(0.2126, 0.7152, 0.0722))
        / max(dot(avg, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.01);
    var color = in.color;
    if (!seed_head) {
        // Some tips dry out; blades are a little paler towards the tip.
        let dry_tip = smoothstep(0.55, 1.0, t) * grass.look.y * smoothstep(0.3, 0.9, fract(f32(cell) * 0.37 + in.color.g * 9.0));
        color = mix(color, grass.dry.rgb, dry_tip);
        color *= mix(vec3<f32>(0.8, 0.84, 0.8), vec3<f32>(1.06, 1.05, 1.0), t);
    } else {
        // Stems below the head stay green-ish.
        color = mix(mix(grass.healthy.rgb, grass.dry.rgb, 0.5), color, smoothstep(0.35, 0.6, t));
    }
    // Photo detail close up (veins, spikelets, browned tips), smoother far away.
    let textured = mix(vec3<f32>(lum), detail, 0.75);
    var albedo = clamp(color * mix(textured, vec3<f32>(1.0), far * 0.6), vec3<f32>(0.0), vec3<f32>(1.0));
    let near = 1.0 - far;
    let slope = next - tex.a;
    let thick = smoothstep(0.45, 1.0, tex.a);
    if (!seed_head) {
        // Leaf detail: fine parallel veins, a lighter midrib where the profile peaks and
        // a few dark speckles (fungus, dust).
        let ul = fract(in.uv.x * 8.0);
        let veins = pow(0.5 + 0.5 * cos(ul * 6.2831853 * 11.0), 6.0);
        let midrib = thick * (1.0 - smoothstep(0.0, 0.08, abs(slope)));
        let size = vec2<f32>(textureDimensions(grass_atlas));
        let speck = step(0.985, grass_hash(floor(in.uv.xy * size * vec2<f32>(0.5, 0.125))));
        albedo *= 1.0 + near * (0.12 * midrib - 0.1 * veins - 0.35 * speck);
    }

    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    // Normal detail from the thickness profile (rounded edges, the midrib crease).
    let side_dir = select(-in.across, in.across, front);
    n = normalize(n - side_dir * clamp(slope * 4.0, -0.6, 0.6) * near);
    // Lean the shading normal up: thin blades light more like the canopy than like cards
    // (more so far away, where the canopy reads as one surface).
    n = normalize(mix(n, vec3<f32>(0.0, 1.0, 0.0), mix(0.45, 0.8, far)));

    // Gusts flatten the blades and turn their waxy sides to the light: the waves show as
    // lighter bands moving across the field.
    let gust = in.misc.z * grass.amounts.z;
    albedo *= 1.0 + 0.16 * gust;
    // Waxy blades: a soft sheen close up, matte far away (no sparkle).
    let smoothness = (mix(select(0.25, 0.15, seed_head), 0.05, far) + 0.12 * gust) * in.uv.w;
    // Canopy shade: light reaches the ground only through the blades above, so everything
    // low in the grass (short undergrowth, the feet of tall blades) is darker.
    let canopy = smoothstep(0.0, 0.45, in.misc.y);
    let occlusion = mix(0.2, 1.0, canopy);
    // Sunlight is blocked down there too (not just sky light).
    let lit = albedo * mix(0.42, 1.0, canopy);
    // Thin edges let more light through than the thick midrib.
    let transmission = albedo * grass.seed.w * select(1.0, 0.6, seed_head) * mix(0.15, 1.0, canopy)
        * mix(1.35, 0.8, thick);
    var out = shade_grass(in.world, lit, n, occlusion, transmission, smoothness, coverage);
    out.velocity = velocity_of(in.clip, in.clip_prev);
    return out;
}
