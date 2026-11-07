// Virtual shadow maps (render/vsm): page marking. After the main pass, every pixel within the
// shadow distance marks the page of the clipmap level its footprint picks and, up to the
// levels the soft-shadow search / filter and the grass's taps sample, the coarser ones, with
// the neighbouring pages when it lies within `VSM_MARGIN` texels of a page border. One bit per
// page table slot; the bits are read back and allocated on the CPU.

@group(1) @binding(0) var mark_depth: texture_depth_multisampled_2d;
@group(1) @binding(1) var<uniform> vsm: VsmParams;
@group(1) @binding(2) var<storage, read_write> vsm_marks: array<atomic<u32>>;

fn vsm_mark_slot(l: i32, page: vec2<f32>) {
    if (any(page < vec2<f32>(0.0)) || any(page >= vec2<f32>(VSM_PAGES))) {
        return;
    }
    let s = vec2<u32>((page + vsm.wrap[l].xy) % VSM_PAGES);
    let i = s.y * 64u + s.x;
    let word = u32(l) * 128u + i / 32u;
    let bit = 1u << (i % 32u);
    if ((atomicLoad(&vsm_marks[word]) & bit) == 0u) {
        atomicOr(&vsm_marks[word], bit);
    }
}

fn vsm_mark_level(l: i32, q: vec2<f32>) {
    let lv = vsm.levels[l];
    let t = (q - lv.xy) * lv.z;
    let page = floor(t / VSM_PAGE);
    let f = t - page * VSM_PAGE;
    vsm_mark_slot(l, page);
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
        vsm_mark_slot(l, page + vec2<f32>(dx, 0.0));
    }
    if (dy != 0.0) {
        vsm_mark_slot(l, page + vec2<f32>(0.0, dy));
    }
    if (dx != 0.0 && dy != 0.0) {
        vsm_mark_slot(l, page + vec2<f32>(dx, dy));
    }
}

@compute @workgroup_size(8, 8)
fn cs_vsm_mark(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(mark_depth);
    if (any(id.xy >= size) || vsm.light_x.w < 0.5) {
        return;
    }
    let px = vec2<i32>(id.xy);
    let d = textureLoad(mark_depth, px, 0);
    if (d >= 1.0) {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size);
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    let w = view.inv_view_proj * ndc;
    let world = w.xyz / w.w;
    let rel = world - view.camera_pos.xyz;
    let dist = length(rel);
    if (dist > vsm.misc.w) {
        return;
    }
    let levels = vsm.light_y.w;
    let finest = vsm.info.x;
    let lv = vsm_level_of(max(dist * vsm.info.y, vsm.info.w), finest, levels);
    let taps = vsm_level_of(vsm_taps_footprint(dist), finest, levels);
    let hi = max(lv, max(i32(vsm.misc.y), taps));
    let q = vec2<f32>(dot(rel, vsm.light_x.xyz), -dot(rel, vsm.light_y.xyz));
    for (var l = lv; l <= hi; l++) {
        vsm_mark_level(l, q);
    }
}
