// Grass study: shade the visibility buffer (variants B, C, F). One thread per pixel: the
// blade id and the point on its card come from the visibility target (or, nearer, from the
// software rasteriser's buffer, variant C); the position from the depth; the rest of the
// blade is evaluated again at that point (`grass_blade_ts`) and lit once (`grass_surface`).

@group(2) @binding(0) var vis_tex: texture_2d<u32>;
@group(2) @binding(1) var depth_tex: texture_depth_2d;
@group(2) @binding(2) var out_color: texture_storage_2d<rgba16float, write>;
// Variant C: depth | id per pixel (u64 as two u32: x = low = id << 9 | t, y = high = !depth).
@group(2) @binding(3) var<storage, read> swr: array<vec2<u32>>;

override SWR_ON: bool = false;
// Variant F: one pixel of each 2×2 block (a different one each frame) is shaded; the others
// take its colour when they lie at about its depth, else shade themselves.
override QUARTER: bool = false;

struct Sample {
    id: u32,
    t: f32,
    side: f32,
    depth: f32,
    // From the software rasteriser (the centre line of a thin blade).
    soft: bool,
};

fn fetch(px: vec2<u32>) -> Sample {
    var s: Sample;
    let vis = textureLoad(vis_tex, px, 0).xy;
    s.id = vis.x;
    let ts = unpack2x16unorm(vis.y);
    s.t = ts.x;
    s.side = ts.y - 0.5;
    s.depth = textureLoad(depth_tex, px, 0);
    s.soft = false;
    if (SWR_ON) {
        let w = swr[px.y * u32(study.screen.x) + px.x];
        if (w.y != 0u) {
            let d = bitcast<f32>(~w.y);
            if (d < s.depth) {
                s.depth = d;
                s.id = (w.x >> 9u) + 1u;
                s.t = f32(w.x & 511u) / 511.0;
                s.side = 0.0;
                s.soft = true;
            }
        }
    }
    return s;
}

fn world_at(px: vec2<u32>, depth: f32) -> vec3<f32> {
    let uv = (vec2<f32>(px) + 0.5) * study.screen.zw;
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let w = study.inv_view_proj * ndc;
    return w.xyz / w.w;
}

fn shade_sample(px: vec2<u32>, s: Sample) -> vec3<f32> {
    count_add(11u, 1u);
    let b = load_blade(s.id - 1u);
    let v = grass_blade_ts(s.t, s.side, b, grass.origin.w);
    var a = grass_attrs(v, b);
    let world = world_at(px, s.depth);
    a.world = world;
    // Atlas footprint of a pixel: the card's width and height in pixels.
    let px_m = max(v.dist * grass.screen.x, 1e-5);
    let w_px = max(v.width * select(1.0, 1.0 / v.thin, v.thin < 1.0) / px_m, 0.5);
    let h_px = max(v.height / px_m, 0.5);
    let gx = vec2<f32>(v.repeat / w_px / 8.0, 0.0);
    let gy = vec2<f32>(0.0, 1.0 / h_px);
    let leaf = grass_atlas_uv(a.uv.xy);
    var tex = textureSampleGrad(grass_atlas, grass_sampler, leaf.xy, gx, gy);
    tex.a *= leaf.z;
    if (s.soft) {
        // The centre line: the card's average leaf colour and a full-thickness profile.
        tex = vec4<f32>(textureSampleLevel(grass_atlas, grass_sampler, vec2<f32>((f32(v.cell) + 0.5) / 8.0, leaf.y), 6.0).rgb, 1.0);
    }
    let across_uv = vec2<f32>(2.0 / f32(textureDimensions(grass_atlas).x), 0.0);
    let next = select(textureSampleGrad(grass_atlas, grass_sampler, leaf.xy + across_uv, gx, gy).a, 1.0, s.soft);
    let face = normalize(cross(v.across, v.tangent));
    let front = dot(face, study.camera.xyz - world) >= 0.0;
    return grass_surface(a, front, vec2<f32>(px), tex, next, leaf.xy);
}

var<workgroup> anchor_color: array<vec4<f32>, 16>;
var<workgroup> anchor_depth: array<f32, 16>;

@compute @workgroup_size(8, 8)
fn cs_resolve(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    let size = vec2<u32>(study.screen.xy);
    let inside = gid.x < size.x && gid.y < size.y;
    let px = min(gid.xy, size - 1u);
    if (!QUARTER) {
        if (!inside) {
            return;
        }
        let s = fetch(px);
        if (s.id == 0u) {
            return;
        }
        textureStore(out_color, px, vec4<f32>(shade_sample(px, s), 1.0));
        return;
    }
    // Quarter rate: the anchor of each 2×2 block shades first.
    let k = study.frame.x % 4u;
    let anchor = vec2<u32>(k & 1u, k >> 1u);
    let block = (lid.y / 2u) * 4u + lid.x / 2u;
    let s = fetch(px);
    let is_anchor = all((lid.xy & vec2<u32>(1u)) == anchor);
    if (is_anchor) {
        var c = vec4<f32>(0.0);
        if (s.id != 0u) {
            c = vec4<f32>(shade_sample(px, s), 1.0);
        }
        anchor_color[block] = c;
        anchor_depth[block] = s.depth;
    }
    workgroupBarrier();
    if (!inside || s.id == 0u) {
        return;
    }
    if (is_anchor) {
        textureStore(out_color, px, anchor_color[block]);
        return;
    }
    let ac = anchor_color[block];
    let ad = anchor_depth[block];
    // Similar depth (linearised: z ~ 1 - near / distance close to the far plane) = same turf.
    let lin = (1.0 - s.depth) / max(1.0 - ad, 1e-7);
    if (ac.w > 0.5 && abs(lin - 1.0) < 0.03) {
        textureStore(out_color, px, ac);
    } else {
        textureStore(out_color, px, vec4<f32>(shade_sample(px, s), 1.0));
    }
}
