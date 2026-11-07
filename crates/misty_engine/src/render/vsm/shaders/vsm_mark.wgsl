// Virtual shadow maps (render/vsm): page marking. After the main pass, every pixel within the
// shadow distance marks the page of the clipmap level its footprint picks and, up to the
// levels the soft-shadow search / filter and the grass's taps sample, the coarser ones, with
// the neighbouring pages when it lies within `VSM_MARGIN` texels of a page border. One bit per
// page table slot; the bits are read back and allocated on the CPU.

@group(1) @binding(0) var mark_depth: texture_depth_multisampled_2d;
// The shown set of pages, then the one filling in the background while the sun turns (its
// light_x.w = 0 when there is none); their bits follow each other.
@group(1) @binding(1) var<uniform> vsm_sets: array<VsmParams, 2>;
@group(1) @binding(2) var<storage, read_write> vsm_marks: array<atomic<u32>>;

fn vsm_mark_slot(which: u32, l: i32, page: vec2<f32>) {
    if (any(page < vec2<f32>(0.0)) || any(page >= vec2<f32>(VSM_PAGES))) {
        return;
    }
    let s = vec2<u32>((page + vsm_sets[which].wrap[l].xy) % VSM_PAGES);
    let i = s.y * 64u + s.x;
    let word = which * 12u * 128u + u32(l) * 128u + i / 32u;
    let bit = 1u << (i % 32u);
    if ((atomicLoad(&vsm_marks[word]) & bit) == 0u) {
        atomicOr(&vsm_marks[word], bit);
    }
}

fn vsm_mark_level(which: u32, l: i32, q: vec2<f32>) {
    let lv = vsm_sets[which].levels[l];
    let t = (q - lv.xy) * lv.z;
    let page = floor(t / VSM_PAGE);
    let f = t - page * VSM_PAGE;
    vsm_mark_slot(which, l, page);
    var dx = 0.0;
    var dy = 0.0;
    if (f.x < VSM_MARGIN) {
        dx = -1.0;
    } else if (f.x > VSM_PAGE - VSM_MARGIN) {
        dx = 1.0;
    }
    if (f.y < VSM_MARGIN) {
        dy = -1.0;
    } else if (f.y > VSM_PAGE - VSM_MARGIN) {
        dy = 1.0;
    }
    if (dx != 0.0) {
        vsm_mark_slot(which, l, page + vec2<f32>(dx, 0.0));
    }
    if (dy != 0.0) {
        vsm_mark_slot(which, l, page + vec2<f32>(0.0, dy));
    }
    if (dx != 0.0 && dy != 0.0) {
        vsm_mark_slot(which, l, page + vec2<f32>(dx, dy));
    }
}

@compute @workgroup_size(8, 8)
fn cs_vsm_mark(@builtin(global_invocation_id) id: vec3<u32>) {
    // Every other pixel each way: neighbours a pixel apart land on the same pages (a page is
    // 128 texels, a texel about a pixel), and the margin covers the rest.
    let size = textureDimensions(mark_depth);
    let xy = id.xy * 2u;
    if (any(xy >= size)) {
        return;
    }
    let px = vec2<i32>(xy);
    let d = textureLoad(mark_depth, px, 0);
    if (d >= 1.0) {
        return;
    }
    let uv = (vec2<f32>(xy) + 0.5) / vec2<f32>(size);
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    let w = view.inv_view_proj * ndc;
    let world = w.xyz / w.w;
    let rel = world - view.camera_pos.xyz;
    let dist = length(rel);
    for (var which = 0u; which < 2u; which++) {
        if (vsm_sets[which].light_x.w < 0.5 || dist > vsm_sets[which].misc.w) {
            continue;
        }
        let levels = vsm_sets[which].light_y.w;
        let finest = vsm_sets[which].info.x;
        let lv = vsm_level_of(max(dist * vsm_sets[which].info.y, vsm_sets[which].info.w), finest, levels);
        let taps = vsm_level_of(vsm_taps_footprint(dist), finest, levels);
        let hi = max(lv, max(i32(vsm_sets[which].misc.y), taps));
        let q = vec2<f32>(dot(rel, vsm_sets[which].light_x.xyz), -dot(rel, vsm_sets[which].light_y.xyz));
        for (var l = lv; l <= hi; l++) {
            vsm_mark_level(which, l, q);
        }
    }
}
