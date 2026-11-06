// Grass study: MSAA resolve (variant A's 4× targets to one sample), TAA and tonemapping.
// Every variant ends here, so their images compare after the same temporal filter.

struct FullOut {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> FullOut {
    var out: FullOut;
    let p = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    out.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

@group(2) @binding(0) var ms_color: texture_multisampled_2d<f32>;
@group(2) @binding(1) var ms_depth: texture_depth_multisampled_2d;

struct ResolveOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_msaa_resolve(in: FullOut) -> ResolveOut {
    let p = vec2<i32>(in.clip.xy);
    var c = vec4<f32>(0.0);
    var d = 1.0;
    for (var s = 0; s < 4; s++) {
        c += textureLoad(ms_color, p, s);
        d = min(d, textureLoad(ms_depth, p, s));
    }
    var out: ResolveOut;
    out.color = c * 0.25;
    out.depth = d;
    return out;
}

@group(3) @binding(0) var taa_color: texture_2d<f32>;
@group(3) @binding(1) var taa_depth: texture_depth_2d;
@group(3) @binding(2) var taa_history: texture_2d<f32>;
@group(3) @binding(3) var taa_out: texture_storage_2d<rgba16float, write>;
@group(3) @binding(4) var ldr_out: texture_storage_2d<rgba8unorm, write>;

fn rgb_to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.25 * c.r + 0.5 * c.g + 0.25 * c.b,
        0.5 * c.r - 0.5 * c.b,
        -0.25 * c.r + 0.5 * c.g - 0.25 * c.b,
    );
}

fn ycocg_to_rgb(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

// Exposure in `study.params` is not used; the scene values are already scaled.
@compute @workgroup_size(8, 8)
fn cs_taa(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = vec2<u32>(study.screen.xy);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let p = vec2<i32>(gid.xy);
    let cur = textureLoad(taa_color, p, 0).rgb;
    var result = cur;
    if (study.frame.y > 0u) {
        // Neighbourhood box (variance clip) in YCoCg.
        var m1 = vec3<f32>(0.0);
        var m2 = vec3<f32>(0.0);
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let q = clamp(p + vec2<i32>(x, y), vec2<i32>(0), vec2<i32>(size) - 1);
                let c = rgb_to_ycocg(textureLoad(taa_color, q, 0).rgb);
                m1 += c;
                m2 += c * c;
            }
        }
        m1 /= 9.0;
        let sigma = sqrt(max(m2 / 9.0 - m1 * m1, vec3<f32>(0.0)));
        let lo = m1 - sigma * 1.25;
        let hi = m1 + sigma * 1.25;
        // Reproject with the camera (the study's wind clock is fixed: nothing else moves).
        let depth = textureLoad(taa_depth, p, 0);
        let uv = (vec2<f32>(gid.xy) + 0.5) * study.screen.zw;
        let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
        let w = study.inv_view_proj * ndc;
        let prev = study.prev_view_proj * vec4<f32>(w.xyz / w.w, 1.0);
        let puv = vec2<f32>(prev.x / prev.w * 0.5 + 0.5, 0.5 - prev.y / prev.w * 0.5);
        if (study.frame.x > 0u && all(puv >= vec2<f32>(0.0)) && all(puv <= vec2<f32>(1.0))) {
            let h = rgb_to_ycocg(textureSampleLevel(taa_history, lin_clamp, puv, 0.0).rgb);
            let clipped = ycocg_to_rgb(clamp(h, lo, hi));
            result = mix(clipped, cur, 0.1);
        }
    }
    textureStore(taa_out, p, vec4<f32>(result, 1.0));
    textureStore(ldr_out, p, vec4<f32>(to_srgb(aces(result)), 1.0));
}
