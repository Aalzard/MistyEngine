// Camera-facing particle billboards (instanced quads), drawn after opaque meshes.
// Colours arrive linear (HDR); the part above 1 also goes to the glow target for bloom.
// Fog: alpha-blended particles fade to the fog colour, additive ones fade out (the
// volumetric / height fog too, see fog_apply.wgsl).

@group(1) @binding(0) var particle_tex: texture_2d<f32>;
@group(1) @binding(2) var particle_sampler: sampler;

struct ParticleIn {
    // xyz = world position, w = size (world units).
    @location(0) pos_size: vec4<f32>,
    @location(1) color: vec4<f32>,
    // x = 1 when textured (else the built-in soft round shape).
    @location(2) params: vec4<f32>,
};

struct ParticleOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) textured: f32,
    @location(3) world: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, in: ParticleIn) -> ParticleOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[vi];
    // Camera right/up in world space (directions through the inverse view-projection).
    let right = normalize((view.inv_view_proj * vec4<f32>(1.0, 0.0, 0.0, 0.0)).xyz);
    let up = normalize((view.inv_view_proj * vec4<f32>(0.0, 1.0, 0.0, 0.0)).xyz);
    let half = in.pos_size.w * 0.5;
    let world = in.pos_size.xyz + (right * c.x + up * c.y) * half;
    var out: ParticleOut;
    out.pos = view.view_proj * vec4<f32>(world, 1.0);
    out.uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    out.color = in.color;
    out.textured = in.params.x;
    out.world = world;
    return out;
}

struct ParticleTargets {
    @location(0) color: vec4<f32>,
    @location(1) glow: vec4<f32>,
};

fn shade(in: ParticleOut) -> ParticleTargets {
    let tex = textureSample(particle_tex, particle_sampler, in.uv);
    var shape = tex;
    if (in.textured < 0.5) {
        let r = length(in.uv * 2.0 - 1.0);
        let f = clamp(1.0 - r, 0.0, 1.0);
        shape = vec4<f32>(1.0, 1.0, 1.0, f * f * (3.0 - 2.0 * f));
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

