// Auto Exposure (render/postfx/exposure.rs): a log2 luminance histogram of the resolved HDR
// image (every other pixel, weighted by the metering mode), then one workgroup averages the
// bins between the low and high percentages, turns that into a target EV (relative to mid
// grey, within the limits), eases the view's exposure towards it and stores the multiplier
// the composite applies. The histogram is cleared for the next frame as it is read.

const BINS: u32 = 128u;

struct Exposure {
    // × the scene colour in the composite (1 = the engine's fixed exposure).
    multiplier: f32,
    // Adapted EV of the scene (the multiplier is 2^(compensation − ev)).
    ev: f32,
    // This frame's target EV.
    target_ev: f32,
    // 1 once adapted (0 = the next frame jumps to its target).
    valid: f32,
};

@group(0) @binding(0) var ae_src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> fx: FxParams;
@group(0) @binding(2) var<storage, read_write> ae_bins: array<atomic<u32>, 128>;
@group(0) @binding(3) var<storage, read_write> ae_exposure: Exposure;

var<workgroup> local_bins: array<atomic<u32>, 128>;
var<workgroup> counts: array<f32, 128>;

// Metering weight (0..1) of a pixel at uv.
fn ae_weight(uv: vec2<f32>) -> f32 {
    let mode = fx.ae_limits.w;
    let aspect = fx.size.x / fx.size.y;
    let d = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let r = length(d);
    if (mode > 1.5) {
        return exp(-r * r / (2.0 * 0.25 * 0.25));
    }
    if (mode > 0.5) {
        return 1.0 - smoothstep(0.09, 0.11, r);
    }
    return 1.0;
}

fn ae_bin_log(i: u32) -> f32 {
    return fx.ae.x + (f32(i) + 0.5) / f32(BINS) * fx.ae.y;
}

@compute @workgroup_size(16, 16)
fn cs_histogram(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    if (li < BINS) {
        atomicStore(&local_bins[li], 0u);
    }
    workgroupBarrier();
    let dims = textureDimensions(ae_src);
    let p = gid.xy * 2u;
    if (all(p < dims)) {
        let c = textureLoad(ae_src, p, 0).rgb;
        let lum = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
        let t = (log2(max(lum, 1e-8)) - fx.ae.x) / fx.ae.y;
        // (NaN and infinite pixels land in the end bins.)
        let bin = u32(clamp(t * f32(BINS), 0.0, f32(BINS - 1u)));
        let uv = (vec2<f32>(p) + 0.5) / vec2<f32>(dims);
        let w = u32(ae_weight(uv) * 64.0 + 0.5);
        if (w > 0u) {
            atomicAdd(&local_bins[bin], w);
        }
    }
    workgroupBarrier();
    if (li < BINS) {
        let v = atomicLoad(&local_bins[li]);
        if (v > 0u) {
            atomicAdd(&ae_bins[li], v);
        }
    }
}

@compute @workgroup_size(128)
fn cs_average(@builtin(local_invocation_index) li: u32) {
    counts[li] = f32(atomicLoad(&ae_bins[li]));
    atomicStore(&ae_bins[li], 0u);
    workgroupBarrier();
    if (li != 0u) {
        return;
    }
    var total = 0.0;
    for (var i = 0u; i < BINS; i++) {
        total += counts[i];
    }
    let low = total * fx.ae.z;
    let high = total * fx.ae.w;
    var below = 0.0;
    var sum = 0.0;
    var kept = 0.0;
    for (var i = 0u; i < BINS; i++) {
        let c = counts[i];
        let part = max(min(below + c, high) - max(below, low), 0.0);
        sum += part * ae_bin_log(i);
        kept += part;
        below += c;
    }
    let mid_grey = log2(0.18);
    let average = select(mid_grey, sum / kept, kept > 0.0);
    let goal = clamp(average - mid_grey, fx.ae_limits.x, fx.ae_limits.y);
    var ev = ae_exposure.ev;
    if (fx.ae_adapt.z > 0.5 || ae_exposure.valid < 0.5) {
        ev = goal;
    } else {
        ev += (goal - ev) * select(fx.ae_adapt.y, fx.ae_adapt.x, goal > ev);
    }
    ae_exposure.ev = ev;
    ae_exposure.target_ev = goal;
    ae_exposure.valid = 1.0;
    ae_exposure.multiplier = exp2(fx.ae_limits.z - ev);
}
