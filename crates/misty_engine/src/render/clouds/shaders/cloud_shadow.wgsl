// Cloud shadow map (after cloud_common.wgsl): a square of the cloud base plane around the
// camera. Each texel marches from its point on the base up through the layer towards the sun
// and stores the sunlight transmittance. Surfaces and the fog look it up by sliding their
// position along the sun direction onto the base plane (clouds_apply.wgsl).

@group(0) @binding(6) var shadow_out: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = u32(cl.misc.y);
    if (any(id.xy >= vec2<u32>(size))) {
        return;
    }
    let xz = cl.shadow.xy + (vec2<f32>(id.xy) + 0.5) / f32(size) * cl.misc.z;
    let sun = cl.sun_dir.xyz;
    let up = max(sun.y, 0.08);
    let start = vec3<f32>(xz.x, cl.layer.y, xz.y);
    let len = (cl.layer.z - cl.layer.y) / up;
    let steps = i32(cl.misc.w);
    let dt = len / f32(steps);
    var od = 0.0;
    for (var i = 0; i < steps; i++) {
        let p = start + sun * (dt * (f32(i) + 0.5));
        let h = clamp((p.y - cl.layer.y) / max(cl.layer.z - cl.layer.y, 1.0), 0.0, 1.0);
        let cov = local_coverage(p);
        var d = base_density(p, h, cov);
        if (d > 0.0) {
            d = full_density(p, h, d);
        }
        od += d * dt;
    }
    let t = exp(-od * cl.shape.y);
    textureStore(shadow_out, id.xy, vec4<f32>(t, t, t, 1.0));
}
