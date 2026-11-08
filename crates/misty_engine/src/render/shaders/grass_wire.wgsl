// The grass in the wireframe draw modes (render/wireframe.rs, appended to grass.wgsl): like
// Unity's detail meshes, only a sparse share of the standing blades and flowers near the
// camera, so the ground's own edges stay readable.

// Blades and flowers drawn within this many metres of the camera...
const WIRE_GRASS_REACH: f32 = 10.0;
// ...this share of them close up (by their drop-out key, so the same ones every frame),
// thinning out over the outer half of the reach (no hard edge where they stop).
const WIRE_GRASS_SHARE: f32 = 0.06;

@vertex
fn vs_grass_wire(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> GrassOut {
    let b = grass_fetch(ii);
    let seed_head = b.extra.x >= 2.0;
    var key = b.extra.x - select(0.0, 2.0, seed_head);
    if (grass_is_flower(b)) {
        key = key / FLOWER_KEY;
    }
    let dist = distance(grass.origin.xyz + b.position.xyz, grass.camera.xyz);
    let share = WIRE_GRASS_SHARE * (1.0 - smoothstep(0.5, 1.0, dist / WIRE_GRASS_REACH));
    if (key >= share || grass_flat(b)
        || grass_outside(b, view.view_proj, true)) {
        var out: GrassOut;
        out.clip = GRASS_SKIP;
        return out;
    }
    return grass_vertex(vi, b);
}
