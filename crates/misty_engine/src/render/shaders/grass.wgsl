// Grass: appended to mesh.wgsl, grass_field.wgsl and grass_blade.wgsl (lit by `shade_grass`,
// receives shadows). Blades are textured from the built-in grass atlas: four clumps of thin
// field-grass blades (tools/grass_atlas.py) and four photographed seed heads (two timothy,
// two open panicles) with alpha-to-coverage
// edges. Colour comes from soft lush / dry patches across the field, per tuft and per blade
// jitter and a root-to-tip gradient; gusts that roll across the field bend the blades and
// lift their sheen, so the waves show. Sunlight shines through backlit blades.

const CELL_AVG = array<vec3<f32>, 8>(
    vec3<f32>(0.176, 0.236, 0.038),
    vec3<f32>(0.185, 0.244, 0.040),
    vec3<f32>(0.175, 0.238, 0.037),
    vec3<f32>(0.179, 0.239, 0.039),
    vec3<f32>(0.372, 0.279, 0.076),
    vec3<f32>(0.369, 0.301, 0.090),
    vec3<f32>(0.232, 0.197, 0.061),
    vec3<f32>(0.326, 0.257, 0.085),
);

struct GrassOut {
    // Invariant: the grass depth pass reuses this vertex stage and the lit pass tests for
    // equal depth.
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    // x = atlas u, y = atlas v, z = 0 at the root .. 1 at the tip, w = distance fade 0..1.
    @location(3) uv: vec4<f32>,
    // Atlas cell.
    @location(4) @interpolate(flat) cell: u32,
    // x = 0 close up .. 1 where blades get smaller than a few pixels (softer shading),
    // y = height above the ground as a share of the local canopy top (canopy shade),
    // z = gust 0..1, w = real / drawn width (thin blades).
    @location(5) misc: vec4<f32>,
    // Across the blade (world), for the normal detail.
    @location(6) across: vec3<f32>,
    // Last frame's clip position (xyw): where the wind had the vertex a frame ago.
    @location(7) clip_prev: vec3<f32>,
    // xyz = normal of the rolling canopy top here (lit crests, shaded dips), w = its
    // relief 0 (dip) .. 1 (crest).
    @location(8) canopy: vec4<f32>,
};

// The canopy top at a blade's root: x = its height (m), yz = its slope (m per m), w = its
// relief 0..1 (`grass_canopy` there, worked out on the CPU with the blade).
fn canopy_top(b: BladeIn) -> vec4<f32> {
    let relief = vec3<f32>(b.field.w, b.slope);
    let span = grass.canopy.y - grass.canopy.x;
    // Tufts stand a little above the tallest setting; the undergrowth under the tips.
    let top = (grass.canopy.x + span * relief.x) * 0.95;
    return vec4<f32>(max(top, 0.02), span * relief.yz, relief.x);
}

// Every vertex of a skipped blade lands on one point outside the view: nothing is rasterised.
const GRASS_SKIP = vec4<f32>(2.0, 2.0, 2.0, 1.0);

// The lit pass leaves the blade out: shrunk away by the LOD while the ground cover replaces
// the flat fans, or a flower shrunk away (flowers never lie down as fans).
fn grass_skip(b: BladeIn) -> bool {
    return grass_flat(b) && (grass.draw.y != 0u || grass_is_flower(b));
}

// The lit grass: every blade in view, the flat fans of the blades the LOD shrank away too
// (`grass_flat`: they test against the depth the standing blades wrote without writing their
// own, see `vs_grass_depth`).
@vertex
fn vs_grass(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> GrassOut {
    let b = grass_fetch(ii);
    if (grass_skip(b) || grass_outside(b, view.view_proj, true)) {
        var out: GrassOut;
        out.clip = GRASS_SKIP;
        return out;
    }
    return grass_vertex(vi, b);
}

// The grass depth pass: the standing blades only. The flat fans of the blades the LOD shrank
// away lie on the ground under them; the lit pass draws them against this depth without
// writing any of their own (where fans overlap, the later one shows). Leaving them out here
// halves what the depth pass rasterises.
@vertex
fn vs_grass_depth(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> GrassOut {
    let b = grass_fetch(ii);
    if (grass_flat(b) || grass_outside(b, view.view_proj, true)) {
        var out: GrassOut;
        out.clip = GRASS_SKIP;
        return out;
    }
    return grass_vertex(vi, b);
}

fn grass_vertex(vi: u32, b: BladeIn) -> GrassOut {
    let v = grass_blade(vi, b);
    // Motion vector: the blade as the wind had it last frame (terrains don't move).
    // (Only while the wind's vectors are wanted: the clock moved and TAA is on.)
    var prev = v.p;
    if (grass.screen.w > 0.0) {
        prev = grass_blade_at(vi, b, grass.origin.w - grass.screen.w).p;
    }
    return grass_out(v, prev, b);
}

// GrassOut.across of a card vertex: across the blade, signed by the side (the blade's
// rounding); flowers: the card's plain across direction (their normal maps' +x).
fn grass_across(v: BladeVertex) -> vec3<f32> {
    return select(v.across * v.side * 2.0, v.across, v.cell >= 8u);
}

// Shading normal of a flower card's vertex: plant cards a little rounded across, the head flat.
fn grass_flower_normal(v: BladeVertex) -> vec3<f32> {
    let face = normalize(cross(v.across, v.tangent));
    let round = select(0.5, 0.0, (v.cell & 1u) == 1u);
    return normalize(face + v.across * v.side * round);
}

// The vertex stage's output for card vertex `v` of blade `b` (`prev` = where the wind had it
// last frame).
fn grass_out(v: BladeVertex, prev: vec3<f32>, b: BladeIn) -> GrassOut {
    var out: GrassOut;
    let face = normalize(cross(v.across, v.tangent));

    out.clip = view.view_proj * vec4<f32>(v.p, 1.0);
    out.clip_prev = prev_clip(prev);
    out.world = v.p;
    // Rounded across the blade, so a card is not one flat shade.
    out.normal = normalize(face + v.across * v.side * 0.8);
    // Far blades are a few pixels wide: their differences turn into flicker, so they move
    // towards the local average (patches stay, single blades blend in).
    let far = smoothstep(5.0, 55.0, v.dist);
    let phase = b.shape.w;
    // Far away a grazing view sees mostly the tips (paler, drier) and the seed stalks.
    let dry_mix = mix(b.shape.z, 0.2 + 0.4 * b.shape.z, far * 0.7);
    let patches = b.field.xy;
    var base = grass_base_color(patches, dry_mix, grass.healthy.rgb, grass.dry.rgb, grass.lush.rgb, grass.amounts.xy);
    base = grass_swathe_tint(base, b.field.z) * grass_far_tint(v.dist);
    // Tufts and single blades: warmer / cooler hue and lighter / darker value.
    let cv = grass.look.w * (1.0 - 0.5 * far);
    let tint = b.extra.w;
    // Mostly per clump (each tuft its own green or yellow-green), a little per blade.
    let hue = (fract(phase * 13.7) - 0.5) * 0.3 + tint * 1.0;
    let value = (fract(phase * 7.31 + 0.37) - 0.5) * 0.2 + tint * 0.5;
    base *= vec3<f32>(1.0 + 0.22 * hue * cv, 1.0 + 0.02 * hue * cv, 1.0 - 0.32 * hue * cv)
        * max(1.0 + 0.4 * value * cv, 0.3);
    let seed_color = grass.seed.rgb * (1.0 + 0.12 * tint)
        * mix(vec3<f32>(1.0), vec3<f32>(1.06, 1.0, 0.85), patches.y * grass.amounts.y);
    out.color = select(base, seed_color, v.seed_head);
    if (v.cell >= 8u) {
        // Flowers: their atlas colour, each flower a little warmer or cooler, lighter or
        // darker (the tint in the shading).
        let k = tint * (1.0 - 0.5 * far);
        out.color = vec3<f32>(1.0 + 0.06 * k, 1.0 + 0.02 * k, 1.0 - 0.06 * k)
            * (1.0 + 0.12 * (fract(phase * 7.31 + 0.37) - 0.5));
    }
    // Height within the local canopy (a tuft's tip stands above 1, a dip's below).
    let top = canopy_top(b);
    out.misc = vec4<f32>(far, v.above / top.x, v.gust, v.thin);
    // The canopy's slope, exaggerated: the turf is rougher than its smoothed height field,
    // and the sun side of every bump reads lighter than the dip behind it.
    out.canopy = vec4<f32>(normalize(vec3<f32>(-top.y * 8.0, 1.0, -top.z * 8.0)), top.w);
    out.uv = vec4<f32>(v.card, v.t, v.fade);
    out.cell = v.cell;
    out.across = grass_across(v);
    if (v.cell >= 8u) {
        out.normal = grass_flower_normal(v);
    }
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
    // Broad and gentle: the canopy depth darkens the turf, not every blade's neighbours.
    let ssao = mix(1.0, screen_ao(world), 0.7);
    // Misty GI (when on) replaces the hemisphere: sky occlusion under trees and bounce light.
    let indirect = misty_ambient(ambient * lights.info.z, world);
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
        atten *= fast_shadow_factor(i, li, world, n, l) * direct_occlusion(ssao);
        if (kind == 0u) {
            // Blades shade each other near the camera (the near grass shadow map), softly
            // and never fully dark inside the canopy: light scatters through the blades.
            // No screen-space contact shadow on the blades: the canopy's own soft occlusion
            // gives the depth, and per-blade contact darkening read as crisp noise.
            atten *= mix(0.3, 1.0, grass_near_shadow(world, l));
        }
        // Forward scattering towards the eye when looking into the light, plus diffuse light
        // through the back face.
        let back = pow(clamp(dot(v, -l), 0.0, 1.0), 5.0);
        let through = max(dot(-n, l), 0.0);
        color += transmission * li.color.rgb * atten * (back * 2.5 + through * 0.8);
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

// Coverage of a blade fragment from its atlas alpha (alpha to coverage): the same in the
// depth pass and the lit pass, so both cover the same samples.
fn grass_coverage(in: GrassOut, alpha: f32) -> f32 {
    let far = in.misc.x;
    // Sharpened alpha keeps thin shapes solid under minification (alpha to coverage).
    let sharp = clamp((alpha - 0.5) / max(fwidth(alpha), 1e-4) + 0.5, 0.0, 1.0);
    // Far away the filtered alpha goes straight to coverage (soft, no crawling edges).
    var coverage = mix(sharp, clamp(alpha * 1.6, 0.0, 1.0), mix(0.5, 1.0, far)) * in.misc.w;
    // Alpha to coverage turns partial coverage into a fixed dot pattern; with TAA a noise that
    // changes every frame lets the history average it into smooth translucency instead.
    if (grass.screen.y > 0.5 && coverage < 0.98) {
        let n = grass_hash(in.clip.xy + fract(grass.origin.w * 13.7) * vec2<f32>(71.3, 37.9));
        coverage = clamp(coverage + (n - 0.5) * 0.3, 0.0, 1.0);
    }
    return coverage;
}

// Grass depth, drawn right before the lit grass: only the alpha (coverage) is written.
@fragment
fn fs_grass_depth(in: GrassOut) -> MeshTargets {
    let auv = grass_atlas_uv(in.uv.xy);
    let alpha = textureSampleGrad(grass_atlas, grass_sampler, auv.xy, i32(auv.w), grass_atlas_dx(in.uv.xy), grass_atlas_dy(in.uv.xy)).a * auv.z;
    let coverage = grass_coverage(in, alpha);
    if (coverage < 0.01) {
        discard;
    }
    var out: MeshTargets;
    out.color = vec4<f32>(0.0, 0.0, 0.0, coverage);
    out.glow = vec4<f32>(0.0);
    return out;
}

@fragment
fn fs_grass(in: GrassOut, @builtin(front_facing) front: bool) -> MeshTargets {
    let leaf = grass_atlas_uv(in.uv.xy);
    let gx = grass_atlas_dx(in.uv.xy);
    let gy = grass_atlas_dy(in.uv.xy);
    var tex = textureSampleGrad(grass_atlas, grass_sampler, leaf.xy, i32(leaf.w), gx, gy);
    tex.a *= leaf.z;
    let coverage = grass_coverage(in, tex.a);
    if (coverage < 0.01) {
        discard;
    }
    var out = grass_lit(in, front, tex, leaf.xy, gx, gy, coverage);
    out.velocity = velocity_of(in.clip, in.clip_prev);
    return out;
}

// The lit blade at a point of its card (`in`, the atlas texel `tex` at `auv` with the atlas
// gradients `gx`, `gy`, and its coverage): the lit grass fragment, and the grass visibility
// buffer's resolve for each blade it sees (grass_vis.wgsl). No velocity.
fn grass_lit(
    in: GrassOut,
    front: bool,
    tex: vec4<f32>,
    auv: vec2<f32>,
    gx: vec2<f32>,
    gy: vec2<f32>,
    coverage: f32,
) -> MeshTargets {
    if (in.cell >= 8u) {
        return grass_flower_lit(in, front, tex, auv, gx, gy, coverage);
    }
    // Two texels across the blade: the alpha slope is the blade's thickness profile, used as
    // a normal map (rounded edges, a crease along the midrib) and a thickness map.
    let across_uv = vec2<f32>(2.0 / f32(textureDimensions(grass_atlas).x), 0.0);
    let next = textureSampleGrad(grass_atlas, grass_sampler, auv + across_uv, 0, gx, gy).a;
    let far = in.misc.x;
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
        // Every tip yellows towards straw (some more), from a greener base.
        let dry_tip = smoothstep(0.45, 1.0, t) * grass.look.y * (0.45 + 0.55 * smoothstep(0.2, 0.9, fract(f32(cell) * 0.37 + in.color.g * 9.0)));
        color = mix(color, grass.dry.rgb, dry_tip);
        color *= mix(vec3<f32>(0.82, 0.95, 0.78), vec3<f32>(1.1, 1.08, 1.0), t);
    } else {
        // Stems below the head stay green-ish.
        color = mix(mix(grass.healthy.rgb, grass.dry.rgb, 0.5), color, smoothstep(0.35, 0.6, t));
    }
    // Photo detail close up (veins, spikelets, browned tips), smoother far away.
    let textured = mix(vec3<f32>(lum), detail, 0.5);
    var albedo = clamp(color * mix(textured, vec3<f32>(1.0), 0.6 + far * 0.4), vec3<f32>(0.0), vec3<f32>(1.0));
    let near = 1.0 - far;
    let slope = next - tex.a;
    let thick = smoothstep(0.45, 1.0, tex.a);
    if (!seed_head) {
        // Leaf detail: fine parallel veins, a lighter midrib where the profile peaks and
        // a few dark speckles (fungus, dust).
        let ul = fract(auv.x * 8.0);
        let veins = pow(0.5 + 0.5 * cos(ul * 6.2831853 * 34.0), 6.0);
        let midrib = thick * (1.0 - smoothstep(0.0, 0.08, abs(slope)));
        let size = vec2<f32>(textureDimensions(grass_atlas));
        let speck = step(0.985, grass_hash(floor(auv * size * vec2<f32>(0.5, 0.125))));
        albedo *= 1.0 + near * (0.05 * midrib - 0.03 * veins - 0.05 * speck);
    }

    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    // Normal detail from the thickness profile (rounded edges, the midrib crease).
    let side_dir = select(-in.across, in.across, front);
    n = normalize(n - side_dir * clamp(slope * 1.5, -0.3, 0.3) * near);
    // Lean the shading normal towards the canopy top's: thin blades light more like the
    // rolling turf than like cards (more so far away, where the canopy reads as one
    // surface), so the sun side of every tuft and bump is lighter than the dip behind it.
    // Not too far: seen at a grazing angle the turf shows the blades' sides, not a lawn
    // top lit by the whole sky (which turned the far field lime).
    // Strongly: the turf is lit as one soft volume, not as thousands of cards.
    n = normalize(mix(n, normalize(in.canopy.xyz), mix(0.7, 0.8, far)));

    // Gusts flatten the blades and turn their waxy sides to the light: the waves show as
    // lighter bands moving across the field.
    let gust = in.misc.z * grass.amounts.z;
    albedo *= 1.0 + 0.16 * gust;
    // Waxy blades: a soft sheen close up, matte far away (no sparkle).
    // Low and broad: a faint waxy sheen, no glints.
    let smoothness = (mix(select(0.08, 0.06, seed_head), 0.04, far) + 0.05 * gust) * in.uv.w;
    // Canopy shade: light reaches the ground only through the blades above, so everything
    // low in the local canopy (undergrowth, the feet of the stems, a dip's blades) is
    // darker than the tips of the tufts standing above it.
    let canopy = smoothstep(0.0, 0.85, in.misc.y);
    // Dips of the rolling turf lie in the shade of the crests around them.
    let dip = mix(0.84, 1.06, in.canopy.w);
    // Soft depth: broad darkening towards the feet of the stems and deep in the canopy.
    let occlusion = mix(0.4, 1.0, canopy) * dip;
    // Sunlight is blocked down there too (not just sky light), though the short turf's
    // feet are never far from the light.
    let lit = albedo * mix(0.62, 1.0, canopy) * dip;
    // Thin edges let more light through than the thick midrib.
    let transmission = albedo * grass.seed.w * select(1.0, 0.6, seed_head) * mix(0.15, 1.0, canopy)
        * mix(1.35, 0.8, thick);
    return shade_grass(in.world, lit, n, occlusion, transmission, smoothness, coverage);
}

// A lit flower card (`grass_lit` for cells 8..): the flower atlas's colour and its material
// layer (normal map, roughness, translucency), the flower's tint, shade low in the turf and
// light through the petals. Heads stand above the turf and light as themselves; stems lean a
// little towards the canopy's shading so they sit in the grass.
fn grass_flower_lit(
    in: GrassOut,
    front: bool,
    tex: vec4<f32>,
    auv: vec2<f32>,
    gx: vec2<f32>,
    gy: vec2<f32>,
    coverage: f32,
) -> MeshTargets {
    let material = textureSampleGrad(grass_atlas, grass_sampler, auv, 2, gx, gy);
    let far = in.misc.x;
    let head = (in.cell & 1u) == 1u;
    // Tangent frame: +x across the card, +y up the image (towards the tip / head).
    let n0 = normalize(in.normal);
    let across = normalize(in.across - n0 * dot(in.across, n0));
    let up = cross(n0, across);
    let m = vec3<f32>(material.xy * 2.0 - 1.0, 0.0);
    let mz = sqrt(max(1.0 - dot(m.xy, m.xy), 0.05));
    // Seen from the back, the relief mirrors through the card.
    let detail = mix(1.0, 0.4, far);
    var n = normalize(across * m.x * detail + up * m.y * detail + n0 * select(-mz, mz, front));
    if (!head) {
        n = normalize(mix(n, normalize(in.canopy.xyz), 0.25 + 0.3 * far));
    }
    let albedo = clamp(tex.rgb * in.color, vec3<f32>(0.0), vec3<f32>(1.0));
    // Shade low in the turf (stems, low leaves); the heads stand in the light.
    let canopy = smoothstep(0.0, 0.85, in.misc.y);
    let dip = mix(0.88, 1.04, in.canopy.w);
    let occlusion = mix(0.45, 1.0, canopy) * dip;
    let lit = albedo * mix(0.6, 1.0, canopy) * dip;
    let roughness = material.z;
    let smoothness = clamp(1.0 - roughness, 0.0, 0.85) * mix(1.0, 0.5, far) * in.uv.w;
    let transmission = albedo * material.w * grass.seed.w * 1.3 * mix(0.25, 1.0, canopy);
    return shade_grass(in.world, lit, n, occlusion, transmission, smoothness, coverage);
}
