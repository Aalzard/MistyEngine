// Particle billboards and trail segments (instanced quads), drawn after opaque meshes.
// Colours arrive linear (HDR); the part above 1 also goes to the glow target for bloom.
// Fog: alpha-blended particles fade to the fog colour, additive ones fade out (the
// volumetric / height fog too, see fog_apply.wgsl).

@group(1) @binding(0) var particle_tex: texture_2d<f32>;
@group(1) @binding(2) var particle_sampler: sampler;

struct ParticleIn {
    // xyz = world position, w = size (world units).
    @location(0) pos_size: vec4<f32>,
    @location(1) color: vec4<f32>,
    // x = shape (0 soft dot, 1 textured, 2 soft strip), y = rotation (radians),
    // z = mode (0 billboard, 1 stretched, 2 horizontal, 3 vertical, 4 segment),
    // w = stretched length scale.
    @location(2) params: vec4<f32>,
    // Texture rectangle: offset xy, scale zw.
    @location(3) uv_rect: vec4<f32>,
    // Stretched: world velocity + speed scale; segment: vector to the end + end width.
    @location(4) axis: vec4<f32>,
};

struct ParticleOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) shape: f32,
    @location(3) world: vec3<f32>,
    // Across a strip, -1..1 (soft strip edges).
    @location(4) across: f32,
    // The quad's own 0..1 square (the soft dot, whatever the sheet rectangle).
    @location(5) quad: vec2<f32>,
};

fn safe_normalize(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let l = length(v);
    if (l < 1e-6) {
        return fallback;
    }
    return v / l;
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, in: ParticleIn) -> ParticleOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[vi];
    let mode = u32(in.params.z + 0.5);
    let center = in.pos_size.xyz;
    let size = in.pos_size.w;
    let cam = view.camera_pos.xyz;
    var world: vec3<f32>;
    var base_uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    if (mode == 4u) {
        // Trail segment from `center` to `center + axis`: x along, y across.
        let t = c.x * 0.5 + 0.5;
        let b = center + in.axis.xyz;
        let along = mix(center, b, t);
        let side = safe_normalize(cross(in.axis.xyz, cam - along), vec3<f32>(0.0, 1.0, 0.0));
        let half = mix(size, in.axis.w, t) * 0.5;
        world = along + side * (c.y * half);
        base_uv = vec2<f32>(t, 0.5 - c.y * 0.5);
    } else {
        // Camera right/up in world space (directions through the inverse view-projection).
        var right = normalize((view.inv_view_proj * vec4<f32>(1.0, 0.0, 0.0, 0.0)).xyz);
        var up = normalize((view.inv_view_proj * vec4<f32>(0.0, 1.0, 0.0, 0.0)).xyz);
        var half_up = size * 0.5;
        if (mode == 1u) {
            let speed = length(in.axis.xyz);
            if (speed > 1e-4) {
                up = in.axis.xyz / speed;
                right = safe_normalize(cross(up, cam - center), right);
                half_up = size * 0.5 * max(in.params.w, 0.0) + speed * in.axis.w * 0.5;
            }
        } else if (mode == 2u) {
            right = vec3<f32>(1.0, 0.0, 0.0);
            up = vec3<f32>(0.0, 0.0, 1.0);
        } else if (mode == 3u) {
            up = vec3<f32>(0.0, 1.0, 0.0);
            right = safe_normalize(cross(up, cam - center), right);
        }
        let a = in.params.y;
        let rx = right * cos(a) + up * sin(a);
        let ry = up * cos(a) - right * sin(a);
        world = center + rx * (c.x * size * 0.5) + ry * (c.y * half_up);
    }
    var out: ParticleOut;
    out.pos = view.view_proj * vec4<f32>(world, 1.0);
    out.uv = in.uv_rect.xy + base_uv * in.uv_rect.zw;
    out.color = in.color;
    out.shape = in.params.x;
    out.world = world;
    out.across = c.y;
    out.quad = base_uv;
    return out;
}

struct ParticleTargets {
    @location(0) color: vec4<f32>,
    @location(1) glow: vec4<f32>,
};

fn shade(in: ParticleOut) -> ParticleTargets {
    let tex = textureSample(particle_tex, particle_sampler, in.uv);
    var shape = tex;
    if (in.shape < 0.5) {
        let r = length(in.quad * 2.0 - 1.0);
        let f = clamp(1.0 - r, 0.0, 1.0);
        shape = vec4<f32>(1.0, 1.0, 1.0, f * f * (3.0 - 2.0 * f));
    } else if (in.shape > 1.5) {
        let f = clamp(1.0 - abs(in.across), 0.0, 1.0);
        shape = vec4<f32>(1.0, 1.0, 1.0, sqrt(f));
    }
    let c = vec4<f32>(in.color.rgb * shape.rgb, in.color.a * shape.a);
    let bright = max(max(c.r, c.g), c.b);
    var out: ParticleTargets;
    out.color = c;
    out.glow = vec4<f32>(c.rgb * clamp(bright - 1.0, 0.0, 1.0), c.a);
    return out;
}

@fragment
fn fs_main(in: ParticleOut) -> ParticleTargets {
    var out = shade(in);
    let fog = fog_visibility(fog_distance(in.world));
    let vf = vfog_eval(in.world);
    out.color = vec4<f32>(vfog_blend(mix(view.fog_color.rgb, out.color.rgb, fog), vf, 1.0), out.color.a);
    out.glow = vec4<f32>(out.glow.rgb * fog * vfog_glow(vf), out.glow.a);
    return out;
}

@fragment
fn fs_additive(in: ParticleOut) -> ParticleTargets {
    var out = shade(in);
    let fog = fog_visibility(fog_distance(in.world)) * vfog_glow(vfog_eval(in.world));
    out.color = vec4<f32>(out.color.rgb * fog, out.color.a);
    out.glow = vec4<f32>(out.glow.rgb * fog, out.glow.a);
    return out;
}
