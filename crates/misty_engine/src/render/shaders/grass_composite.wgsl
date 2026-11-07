// Grass visibility buffer (grass_vis.rs): write the resolved blades into the main pass's
// MSAA targets, sample by sample, where the lit grass pass used to (colour + coverage alpha,
// the glow target's coverage alpha, the motion vector). Samples without a blade keep what is
// there (terrain, trees); everything drawn after the grass then blends and resolves as
// before.

@group(1) @binding(0) var vis_px: texture_2d<u32>;
@group(1) @binding(1) var vis_slots: texture_2d_array<u32>;

struct CompositeOut {
    @location(0) color: vec4<f32>,
    @location(1) glow: vec4<f32>,
    @location(2) velocity: vec2<f32>,
};

// Runs per sample (reads `sample_index`).
@fragment
fn fs_grass_composite(@builtin(position) pos: vec4<f32>, @builtin(sample_index) si: u32) -> CompositeOut {
    let px = vec2<i32>(pos.xy);
    let p = textureLoad(vis_px, px, 0).xy;
    let code = (p.x >> (3u * si)) & 7u;
    if (code == 0u) {
        discard;
    }
    let c = textureLoad(vis_slots, px, i32(code - 1u), 0).xy;
    let rg = unpack2x16float(c.x);
    let ba = unpack2x16float(c.y);
    var out: CompositeOut;
    out.color = vec4<f32>(rg, ba.x, ba.y);
    out.glow = vec4<f32>(0.0, 0.0, 0.0, ba.y);
    out.velocity = unpack2x16float(p.y);
    return out;
}
