// Grass visibility buffer (grass_vis.rs), appended to the lit grass shader (grass.wgsl).
//
// The lit grass pass shaded every blade fragment that passed the depth test, once per
// primitive per pixel under MSAA and in 2×2 quads: ~15 M shader lanes for ~2 M visible
// blade samples at 1080p. Here the same blades, in the same order and with the same depth
// test, coverage and alpha to coverage, only write which blade (and which triangle of its
// card) each sample shows; a compute pass then shades each distinct blade of a pixel once
// with the very same lighting (`grass_lit`), and the main pass writes the results into the
// samples they own (grass_composite.wgsl) before everything drawn after the grass.
//
// Core WebGPU can't multisample integer targets: the blade id goes into an R32Float target
// as raw bits (bit 30 set, so it is never a denormal or NaN), the triangle and facing into
// an Rgba16Float target whose alpha is the coverage (alpha to coverage reads target 0).

// Visibility id: bit 30 set, terrain (draw.z, 3 bits) at 24, page at 21, index in the page.
fn grass_vis_id(entry: u32) -> u32 {
    return 0x40000000u | ((grass.draw.z & 7u) << 24u) | ((entry >> 28u) << 21u)
        | (entry & 0x1fffffu);
}

struct VisOut {
    // Invariant: the same position as the grass depth pass, tested for equal depth.
    @builtin(position) @invariant clip: vec4<f32>,
    // As GrassOut.uv (card u, v; z, w unused here).
    @location(0) uv: vec4<f32>,
    // x = far (GrassOut.misc.x), y = real / drawn width (GrassOut.misc.w).
    @location(1) misc: vec2<f32>,
    @location(2) @interpolate(flat) id: u32,
    // The first vertex of the fragment's triangle (the strip's triangle n is vertices n, n+1,
    // n+2; flat values come from vertex n).
    @location(3) @interpolate(flat) tri: u32,
};

// The lit grass's blades and fans, as `vs_grass` draws them.
@vertex
fn vs_grass_vis(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VisOut {
    let b = grass_fetch(ii);
    if (grass_skip(b) || grass_outside(b, view.view_proj, true)) {
        var out: VisOut;
        out.clip = GRASS_SKIP;
        return out;
    }
    return vis_out(vi, ii, b);
}

// The standing blades only, as the grass depth pass draws them (`vs_grass_depth`): with
// depth writes they stand in for that pass (render/grass_vis.rs `raster_standing`).
@vertex
fn vs_grass_vis_standing(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VisOut {
    let b = grass_fetch(ii);
    if (grass_flat(b) || grass_outside(b, view.view_proj, true)) {
        var out: VisOut;
        out.clip = GRASS_SKIP;
        return out;
    }
    return vis_out(vi, ii, b);
}

// The flat fans only (the lit grass's blades `vs_grass_vis_standing` leaves out), drawn after
// every standing blade without depth writes.
@vertex
fn vs_grass_vis_fans(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VisOut {
    let b = grass_fetch(ii);
    if (!grass_flat(b) || grass_skip(b) || grass_outside(b, view.view_proj, true)) {
        var out: VisOut;
        out.clip = GRASS_SKIP;
        return out;
    }
    return vis_out(vi, ii, b);
}

fn vis_out(vi: u32, ii: u32, b: BladeIn) -> VisOut {
    var out: VisOut;
    let g = grass_vertex(vi, b);
    out.clip = g.clip;
    out.uv = g.uv;
    out.misc = vec2<f32>(g.misc.x, g.misc.w);
    out.id = grass_vis_id(grass_drawn[ii]);
    out.tri = vi;
    return out;
}

struct VisTargets {
    // x = first vertex of the triangle, y = 1 front / -1 back face, w = coverage.
    @location(0) card: vec4<f32>,
    // `grass_vis_id` as raw bits.
    @location(1) id: f32,
};

// The coverage of `fs_grass` (same inputs, same quad, so the same samples pass).
@fragment
fn fs_grass_vis(in: VisOut, @builtin(front_facing) front: bool) -> VisTargets {
    let leaf = grass_atlas_uv(in.uv.xy);
    var alpha = textureSampleGrad(grass_atlas, grass_sampler, leaf.xy, i32(leaf.w), grass_atlas_dx(in.uv.xy), grass_atlas_dy(in.uv.xy)).a;
    alpha *= leaf.z;
    var g: GrassOut;
    g.clip = in.clip;
    g.misc = vec4<f32>(in.misc.x, 0.0, 0.0, in.misc.y);
    let coverage = grass_coverage(g, alpha);
    if (coverage < 0.01) {
        discard;
    }
    var out: VisTargets;
    out.card = vec4<f32>(f32(in.tri), select(-1.0, 1.0, front), 0.0, coverage);
    out.id = bitcast<f32>(in.id);
    return out;
}

// ---- The resolve ----
//
// Two steps per terrain: `cs_grass_classify` (one thread per pixel) finds the distinct blade
// fragments among each pixel's samples, writes the pixel's sample → slot table and appends
// one job per slot of this terrain (compacted per workgroup); `cs_grass_shade` then shades
// one job per thread, so no thread waits for a neighbour that sees four blades.

// Pages 1–3 of the terrain's blades (page 0 is `grass_blades`, grass_pool.rs).
@group(3) @binding(2) var<storage, read> grass_page1: array<vec2<f32>>;
@group(3) @binding(3) var<storage, read> grass_page2: array<vec2<f32>>;
@group(3) @binding(4) var<storage, read> grass_page3: array<vec2<f32>>;
@group(3) @binding(5) var vis_card: texture_multisampled_2d<f32>;
@group(3) @binding(6) var vis_ids: texture_multisampled_2d<f32>;
// Per pixel: 3 bits per sample (0 = no blade, k = slot k - 1).
@group(3) @binding(7) var vis_table: texture_storage_2d<r32uint, write>;
// Per pixel and slot: colour rg, colour b + coverage (pack2x16float).
@group(3) @binding(8) var vis_slots: texture_storage_2d_array<rg32uint, write>;

struct VisFrame {
    // x = this dispatch's terrain (id bits 24–26), y unused, zw = target size.
    info: vec4<u32>,
};
@group(3) @binding(9) var<uniform> vis: VisFrame;
// Shading jobs: pixel index << 4 | slot << 2 | the slot's first sample.
@group(3) @binding(10) var<storage, read_write> vis_jobs: array<u32>;
// x = jobs appended, yzw = the shading dispatch's workgroups (indirect arguments).
@group(3) @binding(11) var<storage, read_write> vis_count: array<atomic<u32>, 4>;
// Per pixel: the motion vector of sample 0's blade (pack2x16float).
@group(3) @binding(12) var vis_vel: texture_storage_2d<r32uint, write>;

fn vis_word(page: u32, i: u32) -> vec2<f32> {
    switch page {
        case 1u: {
            return grass_page1[i];
        }
        case 2u: {
            return grass_page2[i];
        }
        case 3u: {
            return grass_page3[i];
        }
        default: {
            return grass_blades[i];
        }
    }
}

fn vis_blade(id: u32) -> BladeIn {
    let page = (id >> 21u) & 7u;
    let at = (id & 0x1fffffu) * 9u;
    var b: BladeIn;
    b.position = vec4<f32>(vis_word(page, at), vis_word(page, at + 1u));
    b.shape = vec4<f32>(vis_word(page, at + 2u), vis_word(page, at + 3u));
    b.extra = vec4<f32>(vis_word(page, at + 4u), vis_word(page, at + 5u));
    b.field = vec4<f32>(vis_word(page, at + 6u), vis_word(page, at + 7u));
    b.slope = vis_word(page, at + 8u);
    return b;
}

// Perspective-correct barycentrics of the triangle (clip positions) at framebuffer point
// `p`, as the rasteriser interpolates (also outside the triangle: a pixel centre a
// triangle covers only some samples of extrapolates its attributes the same way).
fn vis_bary(c0: vec4<f32>, c1: vec4<f32>, c2: vec4<f32>, p: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(vis.info.zw);
    let ndc = vec2<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0);
    let s0 = c0.xy / c0.w;
    let s1 = c1.xy / c1.w;
    let s2 = c2.xy / c2.w;
    let e1 = s1 - s0;
    let e2 = s2 - s0;
    let d = ndc - s0;
    let area = e1.x * e2.y - e1.y * e2.x;
    let l1 = (d.x * e2.y - d.y * e2.x) / area;
    let l2 = (e1.x * d.y - e1.y * d.x) / area;
    let l = vec3<f32>(1.0 - l1 - l2, l1, l2) / vec3<f32>(c0.w, c1.w, c2.w);
    return l / (l.x + l.y + l.z);
}

fn vis_vertex(now: BladeFrame, before: BladeFrame, b: BladeIn, vi: u32) -> GrassOut {
    let v = grass_blade_point(now, b, vi);
    var prev = v.p;
    if (grass.screen.w > 0.0) {
        prev = grass_blade_point(before, b, vi).p;
    }
    return grass_out(v, prev, b);
}

// The parts of `grass_out` that differ between a card's vertices (the rest left unset).
fn vis_varying(now: BladeFrame, before: BladeFrame, b: BladeIn, vi: u32) -> GrassOut {
    let v = grass_blade_point(now, b, vi);
    var prev = v.p;
    if (grass.screen.w > 0.0) {
        prev = grass_blade_point(before, b, vi).p;
    }
    var out: GrassOut;
    let face = normalize(cross(v.across, v.tangent));
    out.clip = view.view_proj * vec4<f32>(v.p, 1.0);
    out.clip_prev = prev_clip(prev);
    out.world = v.p;
    out.normal = normalize(face + v.across * v.side * 0.8);
    if (v.cell >= 8u) {
        out.normal = grass_flower_normal(v);
    }
    out.misc.y = v.above / canopy_top(b).x;
    out.uv = vec4<f32>(v.card, v.t, v.fade);
    out.across = grass_across(v);
    return out;
}

fn vis_card_uv(a: GrassOut, b: GrassOut, c: GrassOut, m: vec3<f32>) -> vec2<f32> {
    return a.uv.xy * m.x + b.uv.xy * m.y + c.uv.xy * m.z;
}

// Shade the blade `id` seen through triangle `card.x` at pixel `px`.
fn vis_shade(px: vec2<u32>, id: u32, card: vec4<f32>) -> MeshTargets {
    let b = vis_blade(id);
    let tri = u32(card.x);
    // The triangle's three vertices as the vertex stage made them (`grass_vertex`), the
    // blade's shared work done once per wind time.
    let now = grass_blade_frame(b, grass.origin.w);
    var before = now;
    if (grass.screen.w > 0.0) {
        before = grass_blade_frame(b, grass.origin.w - grass.screen.w);
    }
    // Vertex 0 in full; of the other two only what varies along the card (colour, canopy,
    // distance terms and the cell are the blade's: the same at every vertex).
    let v0 = vis_vertex(now, before, b, tri);
    let v1 = vis_varying(now, before, b, tri + 1u);
    let v2 = vis_varying(now, before, b, tri + 2u);
    let p = vec2<f32>(px) + 0.5;
    let m = vis_bary(v0.clip, v1.clip, v2.clip, p);
    var in = v0;
    in.clip = vec4<f32>(p, 0.0, 1.0);
    in.world = v0.world * m.x + v1.world * m.y + v2.world * m.z;
    in.normal = v0.normal * m.x + v1.normal * m.y + v2.normal * m.z;
    in.uv = v0.uv * m.x + v1.uv * m.y + v2.uv * m.z;
    in.misc.y = v0.misc.y * m.x + v1.misc.y * m.y + v2.misc.y * m.z;
    in.across = v0.across * m.x + v1.across * m.y + v2.across * m.z;
    in.clip_prev = v0.clip_prev * m.x + v1.clip_prev * m.y + v2.clip_prev * m.z;
    // Screen gradients of the card coordinate as the fragment's quad takes them (the
    // horizontal and vertical pixel pairs of its 2×2 quad).
    let q = vec2<f32>(px & vec2<u32>(~1u)) + 0.5;
    let ux0 = vis_card_uv(v0, v1, v2, vis_bary(v0.clip, v1.clip, v2.clip, vec2<f32>(q.x, p.y)));
    let ux1 = vis_card_uv(v0, v1, v2, vis_bary(v0.clip, v1.clip, v2.clip, vec2<f32>(q.x + 1.0, p.y)));
    let uy0 = vis_card_uv(v0, v1, v2, vis_bary(v0.clip, v1.clip, v2.clip, vec2<f32>(p.x, q.y)));
    let uy1 = vis_card_uv(v0, v1, v2, vis_bary(v0.clip, v1.clip, v2.clip, vec2<f32>(p.x, q.y + 1.0)));
    let dx = ux1 - ux0;
    let dy = uy1 - uy0;
    let gx = vec2<f32>(dx.x / 8.0, dx.y);
    let gy = vec2<f32>(dy.x / 8.0, dy.y);
    let leaf = grass_atlas_uv(in.uv.xy);
    var tex = textureSampleGrad(grass_atlas, grass_sampler, leaf.xy, i32(leaf.w), gx, gy);
    tex.a *= leaf.z;
    var out = grass_lit(in, card.y > 0.0, tex, leaf.xy, gx, gy, card.w);
    out.velocity = velocity_of(in.clip, in.clip_prev);
    return out;
}

fn vis_pack(c: vec4<f32>) -> vec2<u32> {
    let v = clamp(c, vec4<f32>(-65504.0), vec4<f32>(65504.0));
    return vec2<u32>(pack2x16float(v.xy), pack2x16float(v.zw));
}

var<workgroup> vis_group_jobs: atomic<u32>;
var<workgroup> vis_group_base: u32;

@compute @workgroup_size(8, 8)
fn cs_grass_classify(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {
    if (lid == 0u) {
        atomicStore(&vis_group_jobs, 0u);
    }
    workgroupBarrier();
    let inside = gid.x < vis.info.z && gid.y < vis.info.w;
    let px = min(gid.xy, vis.info.zw - 1u);
    let n = textureNumSamples(vis_ids);
    var ids: array<u32, 4>;
    var cards: array<vec4<f32>, 4>;
    var slot_of: array<u32, 4>;
    var first: array<u32, 4>;
    var slots = 0u;
    var table = 0u;
    for (var s = 0u; s < n; s++) {
        let id = bitcast<u32>(textureLoad(vis_ids, px, s).x);
        let card = textureLoad(vis_card, px, s);
        ids[s] = id;
        cards[s] = card;
        slot_of[s] = 4u;
        if (id == 0u || !inside) {
            continue;
        }
        // The same blade fragment (same triangle, facing and coverage) in an earlier sample
        // shares its shade.
        var slot = slots;
        for (var k = 0u; k < s; k++) {
            if (slot == slots && ids[k] == id && all(cards[k] == card)) {
                slot = slot_of[k];
            }
        }
        if (slot == slots) {
            first[slots] = s;
            slots += 1u;
        }
        slot_of[s] = slot;
        table |= (slot + 1u) << (3u * s);
    }
    // This terrain's slots: room in the workgroup's run of jobs, then one global add.
    var mine = 0u;
    for (var k = 0u; k < slots; k++) {
        mine += u32(((ids[first[k]] >> 24u) & 7u) == vis.info.x);
    }
    var at = 0u;
    if (mine > 0u) {
        at = atomicAdd(&vis_group_jobs, mine);
    }
    workgroupBarrier();
    if (lid == 0u) {
        let total = atomicLoad(&vis_group_jobs);
        if (total > 0u) {
            vis_group_base = atomicAdd(&vis_count[0], total);
        }
    }
    workgroupBarrier();
    if (!inside) {
        return;
    }
    textureStore(vis_table, px, vec4<u32>(table, 0u, 0u, 0u));
    let pixel = px.y * vis.info.z + px.x;
    var j = vis_group_base + at;
    for (var k = 0u; k < slots; k++) {
        let s = first[k];
        if (((ids[s] >> 24u) & 7u) == vis.info.x) {
            vis_jobs[j] = (pixel << 4u) | (k << 2u) | s;
            j += 1u;
        }
    }
}

// The shading dispatch's size from the jobs appended (x up to 65535 workgroups a row).
@compute @workgroup_size(1)
fn cs_grass_jobs() {
    let groups = (atomicLoad(&vis_count[0]) + 63u) / 64u;
    let x = min(groups, 65535u);
    atomicStore(&vis_count[1], x);
    atomicStore(&vis_count[2], select(0u, (groups + 65534u) / 65535u, groups > 0u));
    atomicStore(&vis_count[3], 1u);
}

@compute @workgroup_size(64)
fn cs_grass_shade(
    @builtin(workgroup_id) wg: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {
    let i = (wg.y * groups.x + wg.x) * 64u + lid;
    if (i >= atomicLoad(&vis_count[0])) {
        return;
    }
    let job = vis_jobs[i];
    let pixel = job >> 4u;
    let k = (job >> 2u) & 3u;
    let s = job & 3u;
    let px = vec2<u32>(pixel % vis.info.z, pixel / vis.info.z);
    let id = bitcast<u32>(textureLoad(vis_ids, px, s).x);
    let card = textureLoad(vis_card, px, s);
    let shaded = vis_shade(px, id, card);
    textureStore(vis_slots, px, k, vec4<u32>(vis_pack(vec4<f32>(shaded.color.rgb, card.w)), 0u, 0u));
    // (The slot whose first sample is sample 0 owns sample 0: TAA reads its motion.)
    if (s == 0u) {
        textureStore(vis_vel, px, vec4<u32>(pack2x16float(shaded.velocity), 0u, 0u, 0u));
    }
}

// Ready for the next terrain's jobs.
@compute @workgroup_size(1)
fn cs_grass_jobs_reset() {
    atomicStore(&vis_count[0], 0u);
}
