// Terrain layer blending: splat weights, planar or triplanar projection (cliffs), height-based
// blending and Color Harmony. Layer textures are 2D arrays (one slice per layer).

@group(3) @binding(1) var splat_map: texture_2d<f32>;
@group(3) @binding(2) var splat_sampler: sampler;
@group(3) @binding(3) var layer_albedo: texture_2d_array<f32>;
@group(3) @binding(4) var layer_normal: texture_2d_array<f32>;
@group(3) @binding(5) var layer_height: texture_2d_array<f32>;
@group(3) @binding(6) var layer_sampler: sampler;

struct LayerSample {
    albedo: vec3<f32>,
    // World-space normal.
    normal: vec3<f32>,
    height: f32,
};

// One projection of layer `i`: tiled uv `p` (metres) with its screen gradients. A second
// copy, rotated and a little larger, is blended in by a noise of a few tiles, so the tile's
// repeat does not show as a grid.
fn layer_projection(i: u32, p: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>) -> LayerSample {
    let t = terrain.tiles[i];
    let a = layer_tile(i, (p + t.zw) / t.xy, gx / t.xy, gy / t.xy);
    let w = smoothstep(0.3, 0.7, grass_noise(p / (max(t.x, 0.1) * 2.3) + 3.7));
    if (w < 0.01) {
        return a;
    }
    let c = cos(0.9);
    let sn = sin(0.9);
    let r = mat2x2<f32>(c, sn, -sn, c);
    let k = 0.87 / t.xy;
    let b = layer_tile(i, r * p * k + vec2<f32>(0.37, 0.11), r * gx * k, r * gy * k);
    var out: LayerSample;
    out.albedo = mix(a.albedo, b.albedo, w);
    // The copy's tangent-space normal turned back into the first copy's frame.
    let bn = vec3<f32>(transpose(r) * b.normal.xy, b.normal.z);
    out.normal = mix(a.normal, bn, w);
    out.height = mix(a.height, b.height, w);
    return out;
}

// Layer `i` sampled at tile uv `uv` with its screen gradients.
fn layer_tile(i: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> LayerSample {
    var s: LayerSample;
    // Upscaled targets sample the output resolution's mip (view_tan.z = log2 render scale).
    let bias = exp2(view.view_tan.z);
    let gx = dx * bias;
    let gy = dy * bias;
    s.albedo = textureSampleGrad(layer_albedo, layer_sampler, uv, i, gx, gy).rgb;
    // Macro variation: the same texture ~7× larger, relative to its average colour (the last
    // mip), breaks up the tiling without changing the mean.
    let macro_uv = uv * 0.137 + vec2<f32>(0.31, 0.67);
    let broad = textureSampleGrad(layer_albedo, layer_sampler, macro_uv, i, gx * 0.137, gy * 0.137).rgb;
    let mean = textureSampleLevel(layer_albedo, layer_sampler, uv, i, 12.0).rgb;
    let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
    s.albedo *= clamp(mix(1.0, dot(broad, luma) / max(dot(mean, luma), 0.02), 0.6), 0.5, 1.6);
    let m = textureSampleGrad(layer_normal, layer_sampler, uv, i, gx, gy).xyz * 2.0 - 1.0;
    // Tangent-space normal here; `layer_sample` turns it into world space.
    s.normal = vec3<f32>(m.xy * terrain.surface[i].z, max(m.z, 1e-3));
    s.height = textureSampleGrad(layer_height, layer_sampler, uv, i, gx, gy).r;
    return s;
}

// Layer `i` at terrain-local point `p` with geometric normal `n`; `pw` = weights of the
// x, y (top) and z projections. Normal maps are combined with a whiteout blend per axis.
fn layer_sample(i: u32, p: vec3<f32>, dx: vec3<f32>, dy: vec3<f32>, n: vec3<f32>, pw: vec3<f32>) -> LayerSample {
    var out: LayerSample;
    out.albedo = vec3<f32>(0.0);
    out.normal = vec3<f32>(0.0);
    out.height = 0.0;
    if (pw.y > 0.0) {
        // Top: u = +x, v = +z (green points to -z).
        let s = layer_projection(i, p.xz, dx.xz, dy.xz);
        let wn = vec3<f32>(s.normal.x + n.x, s.normal.z * n.y, -s.normal.y + n.z);
        out.albedo += s.albedo * pw.y;
        out.normal += wn * pw.y;
        out.height += s.height * pw.y;
    }
    if (pw.x > 0.0) {
        // Side facing x: u = +z, v = down (green points up).
        let s = layer_projection(i, vec2<f32>(p.z, -p.y), vec2<f32>(dx.z, -dx.y), vec2<f32>(dy.z, -dy.y));
        let wn = vec3<f32>(s.normal.z * n.x, s.normal.y + n.y, s.normal.x + n.z);
        out.albedo += s.albedo * pw.x;
        out.normal += wn * pw.x;
        out.height += s.height * pw.x;
    }
    if (pw.z > 0.0) {
        // Side facing z: u = +x, v = down.
        let s = layer_projection(i, vec2<f32>(p.x, -p.y), vec2<f32>(dx.x, -dx.y), vec2<f32>(dy.x, -dy.y));
        let wn = vec3<f32>(s.normal.x + n.x, s.normal.y + n.y, s.normal.z * n.z);
        out.albedo += s.albedo * pw.z;
        out.normal += wn * pw.z;
        out.height += s.height * pw.z;
    }
    out.normal = normalize(out.normal);
    return out;
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    let d = mx - mn;
    var h = 0.0;
    if (d > 1e-6) {
        if (mx == c.r) {
            h = ((c.g - c.b) / d) / 6.0;
            h = h - floor(h);
        } else if (mx == c.g) {
            h = ((c.b - c.r) / d + 2.0) / 6.0;
        } else {
            h = ((c.r - c.g) / d + 4.0) / 6.0;
        }
    }
    return vec3<f32>(h, select(0.0, d / mx, mx > 1e-6), mx);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let h = fract(c.x) * 6.0;
    let k = (vec3<f32>(5.0, 3.0, 1.0) + h) % vec3<f32>(6.0);
    return c.z - c.z * c.y * clamp(min(k, 4.0 - k), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Color Harmony adjustment of one layer colour (see terrain/harmony.rs).
fn harmonize(c: vec3<f32>, a: vec4<f32>) -> vec3<f32> {
    if (a.w < 0.5) {
        return c;
    }
    let hsv = rgb_to_hsv(c);
    return hsv_to_rgb(vec3<f32>(hsv.x + a.x, clamp(hsv.y * a.y, 0.0, 1.0), hsv.z * a.z));
}

// Lit surface of the terrain at a fragment.
fn terrain_surface(in: MeshOut) -> Surface {
    // Everything needing implicit derivatives happens first (uniform control flow).
    let dx = dpdx(in.world);
    let dy = dpdy(in.world);
    var wv = textureSample(splat_map, splat_sampler, in.uv);
    let res = terrain.grid.x;
    let nuv = (in.uv * (res - 1.0) + 0.5) / res;
    let normal_texel = textureSample(terrain_normals, splat_sampler, nuv);
    let n = normalize(normal_texel.xyz);
    // Grass density (normals w, terrain.rs).
    let grass_cover = max(normal_texel.w, 0.0);

    // Channels past the layer count don't exist; normalise what's left.
    let count = u32(terrain.size.w);
    let mask = vec4<f32>(
        select(0.0, 1.0, count > 0u),
        select(0.0, 1.0, count > 1u),
        select(0.0, 1.0, count > 2u),
        select(0.0, 1.0, count > 3u),
    );
    wv = wv * mask;
    let sum = wv.x + wv.y + wv.z + wv.w;
    if (sum > 1e-4) {
        wv = wv / sum;
    } else {
        wv = vec4<f32>(mask.x, 0.0, 0.0, 0.0);
    }

    // Projection weights: top only on gentle slopes, triplanar past the cliff angle.
    var pw = vec3<f32>(0.0, 1.0, 0.0);
    if (terrain.cliff.x > 0.5) {
        let steep = length(n.xz);
        let t = smoothstep(terrain.cliff.y - 0.08, terrain.cliff.y + 0.04, steep);
        var b = pow(abs(n), vec3<f32>(terrain.cliff.z));
        b = b / max(b.x + b.y + b.z, 1e-5);
        pw = mix(pw, b, t);
        // Drop negligible projections (saves their texture reads).
        pw = select(pw, vec3<f32>(0.0), pw < vec3<f32>(0.02));
        pw = pw / (pw.x + pw.y + pw.z);
    }

    let local = in.world - draws[in.draw].model[3].xyz;
    var w = array<f32, 4>(wv.x, wv.y, wv.z, wv.w);
    var layers: array<LayerSample, 4>;
    for (var i = 0u; i < 4u; i++) {
        if (w[i] > 0.002) {
            layers[i] = layer_sample(i, local, dx, dy, n, pw);
        } else {
            w[i] = 0.0;
        }
    }

    // Height blend: layers poke through each other where their height maps are high.
    if (terrain.cliff.w < 0.999) {
        var top = -1.0;
        for (var i = 0u; i < 4u; i++) {
            if (w[i] > 0.0) {
                top = max(top, layers[i].height + w[i]);
            }
        }
        let base = top - terrain.cliff.w;
        var total = 0.0;
        for (var i = 0u; i < 4u; i++) {
            if (w[i] > 0.0) {
                w[i] = max(layers[i].height + w[i] - base, 0.0);
                total += w[i];
            }
        }
        for (var i = 0u; i < 4u; i++) {
            w[i] = w[i] / max(total, 1e-5);
        }
    }

    var s: Surface;
    s.albedo = vec3<f32>(0.0);
    s.normal = vec3<f32>(0.0);
    s.metallic = 0.0;
    s.smoothness = 0.0;
    for (var i = 0u; i < 4u; i++) {
        if (w[i] > 0.0) {
            s.albedo += harmonize(layers[i].albedo, terrain.harmony[i]) * w[i];
            s.normal += layers[i].normal * w[i];
            s.metallic += terrain.surface[i].x * w[i];
            s.smoothness += terrain.surface[i].y * w[i];
        }
    }
    // Under grass the ground takes the grass's own colour in deep shade, so the gaps
    // between blades read as more grass rather than soil; far away (thin blades) it moves
    // towards the lit canopy colour, and beyond the draw distance it stands in for the grass.
    let tint = terrain.grass_healthy.w * grass_cover;
    if (tint > 0.0) {
        let patches = grass_patches(local.xz, terrain.grass_dry.w);
        let dist = distance(in.world, view.camera_pos.xyz);
        let far = smoothstep(0.15, 1.0, dist / terrain.grass_lush.w);
        // The turf's own base colour: close up the green of the stems' feet deep in the
        // canopy's shade (not pale litter in the sun between the strips of blades), far away
        // the drier, lit colour of the canopy top it stands in for.
        let grass = grass_swathe_tint(
            grass_base_color(patches, mix(0.15, 0.35, far), terrain.grass_healthy.rgb, terrain.grass_dry.rgb, terrain.grass_lush.rgb, terrain.grass_amounts.xy),
            grass_swathe(local.xz, terrain.grass_dry.w),
        ) * grass_far_tint(dist);
        // Far away the canopy reads darker than lit soil (blades shade each other) and
        // mottled: tufts, dips and clumps a few metres across.
        let mottle = 0.3 * grass_noise(local.xz / 1.3 + 8.7) + 0.4 * grass_noise(local.xz / 6.0 + 5.1)
            + 0.3 * grass_noise(local.xz / 21.0 + 2.3);
        let shade = mix(0.3, 0.66, far) * mix(1.0, 0.7 + 0.6 * mottle, smoothstep(8.0, 50.0, dist));
        // The soil texture's light and dark spots stay as litter and shadowed stems.
        let soil = dot(s.albedo, vec3<f32>(0.2126, 0.7152, 0.0722));
        let litter = clamp(0.7 + 0.9 * soil, 0.75, 1.25);
        // Close up the real ground shows between the blades (soil, thatch, pebbles, moss),
        // only nudged towards the turf's colour and darker and damper under it; worn spots
        // are bare dirt. Far away, where single blades are lost, the ground takes the
        // canopy's colour and stands in for it.
        // Past a few metres single gaps are too small to read as ground: the canopy colour
        // takes over quickly, so the pale litter does not mottle the middle distance.
        let mid = smoothstep(4.0, 25.0, dist);
        let worn = grass_worn(local.xz) * (1.0 - mid);
        let take = tint * mix(0.85, 1.0, mid) * (1.0 - 0.9 * worn);
        let under = mix(0.4, 1.0, max(worn, mid));
        s.albedo = mix(s.albedo * under, grass * shade * litter, take);
        s.smoothness *= 1.0 - take;
        // The soil's relief stays close up; far away it would only shimmer under the turf.
        s.normal = normalize(mix(s.normal, n, 0.8 * tint * far));
    }
    if (count == 0u) {
        // No layers yet: plain light grey.
        s.albedo = vec3<f32>(0.5);
        s.normal = n;
    } else {
        s.normal = normalize(s.normal);
    }
    s.alpha = 1.0;
    s.emission = vec3<f32>(0.0);
    s.receive_shadows = true;
    s.occlusion = 1.0;
    s.premultiply = false;
    s.draw = in.draw;
    return s;
}
