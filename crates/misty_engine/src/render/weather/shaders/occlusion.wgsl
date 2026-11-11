// Weather occlusion map (render/weather): the shadow-casting meshes around the camera drawn
// from straight above into a depth map, so rain and snow stop at the first surface (roofs,
// tree crowns) and surfaces under cover stay dry. Prepended: draw.wgsl (`struct Draw`).

struct OccView {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> occ: OccView;
@group(0) @binding(1) var<storage, read> draws: array<Draw>;

@vertex
fn vs_main(@location(0) position: vec3<f32>, @builtin(instance_index) draw: u32) -> @builtin(position) vec4<f32> {
    return occ.view_proj * draws[draw].model * vec4<f32>(position, 1.0);
}
