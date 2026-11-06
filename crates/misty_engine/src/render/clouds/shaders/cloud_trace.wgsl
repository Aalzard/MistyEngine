// Cloud raymarch at reduced resolution (after cloud_common.wgsl). One thread per texel marches
// the camera ray through the layer with a blue-noise start offset, lights each sample with a
// short march towards the sun (multiple-scattering octaves, two-lobe phase for the silver
// lining) plus sky ambient, and writes layer 0 = (in-scattered light, transmittance) and
// layer 1 = (distance to the clouds in km, 0, 0, 1).

@group(0) @binding(6) var trace_out: texture_storage_2d_array<rgba16float, write>;

fn hg(g: f32, c: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI_C * pow(max(1.0 + g2 - 2.0 * g * c, 1e-4), 1.5));
}

// Forward lobe (silver lining around the sun) mixed with a soft back lobe.
fn cloud_phase(c: f32, octave: f32) -> f32 {
    let k = pow(0.5, octave);
    let forward = hg(0.8 * k, c);
    let back = hg(-0.2 * k, c);
    let w = 0.2 + 0.5 * cl.sun_dir.w;
    return mix(back, forward, w);
}

// Blue noise for this texel and frame (golden-ratio animated), 0..1.
fn jitter(texel: vec2<u32>, salt: u32) -> f32 {
    let size = textureDimensions(blue_noise);
    let n = textureLoad(blue_noise, (texel + vec2<u32>(salt * 17u, salt * 29u)) % size, 0).r;
    return fract(n + cl.eye.w * 0.61803399);
}

// Optical depth from `p` towards the sun (cone of growing steps).
fn sun_optical_depth(p: vec3<f32>, j: f32) -> f32 {
    let steps = i32(cl.trace.w);
    let reach = min((cl.layer.z - cl.layer.y) * 0.75, 1800.0);
    var od = 0.0;
    var last = 0.0;
    for (var i = 0; i < steps; i++) {
        let t = (f32(i) + j) / f32(steps);
        let dist = t * t * reach + 8.0;
        let q = p + cl.sun_dir.xyz * dist;
        let h = height_fraction(q);
        let cov = local_coverage(q);
        var d = base_density(q, h, cov);
        if (d > 0.0 && i < 3) {
            d = full_density(q, h, d);
        }
        od += d * (dist - last);
        last = dist;
    }
    return od * cl.shape.y;
}

// Sun and sky light scattered towards the eye at a sample with extinction `sigma`.
fn sample_light(p: vec3<f32>, h: f32, cov: f32, sigma: f32, cos_v: f32, j: f32) -> vec3<f32> {
    var sun = vec3<f32>(0.0);
    if (cl.sun_color.w > 0.5) {
        let od = sun_optical_depth(p, j);
        // Multiple-scattering octaves: each one less attenuated, with a wider phase.
        var a = 1.0;
        var b = 1.0;
        for (var o = 0; o < 3; o++) {
            sun += a * cloud_phase(cos_v, f32(o)) * exp(-od * b);
            a *= 0.5;
            b *= 0.3;
        }
        // Darker, crisper edges facing the sun ("powder").
        let powder = 1.0 - exp(-sigma * 60.0);
        sun *= mix(1.0, powder, 0.5 * clamp(1.0 - cos_v, 0.0, 1.0)) * 2.0;
        sun *= cl.sun_color.rgb * PI_C;
    }
    // Sky light is blocked by the cloud above: thick, dense, covered layers get dark bases,
    // darkest in their densest pockets (structure under an overcast sheet).
    let local = sigma / max(cl.shape.y, 1e-6);
    let above = cl.shape.y * cov * (1.0 - h) * (cl.layer.z - cl.layer.y) * 0.02 * (0.4 + 1.2 * local);
    let ambient = mix(cl.ambient_bottom.rgb, cl.ambient_top.rgb, sqrt(h)) / (1.0 + above * 0.5);
    return sun + ambient;
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(cl.trace.xy);
    if (any(id.xy >= size)) {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size);
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let dir = view_dir(ndc);
    let eye = cl.eye.xyz;

    var light = vec3<f32>(0.0);
    var trans = 1.0;
    var depth_sum = 0.0;
    var depth_weight = 0.0;
    let span = layer_span(eye, dir);
    if (span.y > span.x) {
        let steps = i32(cl.trace.z);
        let len = span.y - span.x;
        // Short spans (looking up) get small steps; long ones are capped at the step count.
        let dt = max(len / f32(steps), 12.0);
        let j = jitter(id.xy, 0u);
        let jl = jitter(id.xy, 1u);
        let cos_v = dot(dir, cl.sun_dir.xyz);
        var t = span.x + dt * j;
        for (var i = 0; i < steps; i++) {
            if (t > span.y || trans < 0.01) {
                break;
            }
            let p = eye + dir * t;
            let h = height_fraction(p);
            let cov = local_coverage(p);
            var d = base_density(p, h, cov);
            if (d > 0.0) {
                d = full_density(p, h, d);
            }
            // Far clouds thin out instead of aliasing.
            d *= 1.0 - smoothstep(cl.layer.w * 0.6, cl.layer.w, t);
            if (d > 0.002) {
                let sigma = d * cl.shape.y;
                let s = sample_light(p, h, cov.x, sigma, cos_v, jl);
                // Energy-conserving integration over the step.
                let step_t = exp(-sigma * dt);
                light += trans * s * (1.0 - step_t);
                depth_sum += t * trans * (1.0 - step_t);
                depth_weight += trans * (1.0 - step_t);
                trans *= step_t;
            }
            t += dt;
        }
    }
    var dist = cl.layer.w;
    if (depth_weight > 1e-4) {
        dist = depth_sum / depth_weight;
    }
    // Aerial perspective: distant clouds fade into the horizon haze.
    let fade = exp(-dist / max(cl.haze.w, 1.0));
    light = light * fade + cl.haze.rgb * (1.0 - trans) * (1.0 - fade);
    textureStore(trace_out, id.xy, 0, vec4<f32>(light, trans));
    textureStore(trace_out, id.xy, 1, vec4<f32>(dist / 1000.0, 0.0, 0.0, 1.0));
}
