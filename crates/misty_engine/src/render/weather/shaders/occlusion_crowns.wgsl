// Weather occlusion map, terrain tree crowns (render/weather): every crown near the camera
// drawn from straight above as the upper half of its ellipsoid, so rain stops in the crowns
// of terrain trees too (they are not meshes, so `occlusion.wgsl` doesn't see them).

struct OccView {
    view_proj: mat4x4<f32>,
};

struct Crown {
    // xyz = centre of the crown.
    centre: vec4<f32>,
    // xyz = radii.
    radii: vec4<f32>,
};

@group(0) @binding(0) var<uniform> occ: OccView;
@group(0) @binding(1) var<storage, read> crowns: array<Crown>;

const SECTORS: u32 = 10u;
const RINGS: u32 = 3u;

// Ring `k` of the dome as (radius share, height share).
fn ring(k: u32) -> vec2<f32> {
    var r = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.55, 0.835),
        vec2<f32>(0.85, 0.527),
        vec2<f32>(1.0, 0.0),
    );
    return r[k];
}

@vertex
fn vs_crown(@builtin(vertex_index) vi: u32, @builtin(instance_index) ci: u32) -> @builtin(position) vec4<f32> {
    let c = crowns[ci];
    let quad = vi / 6u;
    let corner = vi % 6u;
    let sector = quad % SECTORS;
    let band = quad / SECTORS;
    // Two triangles per band and sector: (inner, outer) x (this sector, the next).
    var du = array<u32, 6>(0u, 1u, 0u, 1u, 1u, 0u);
    var dr = array<u32, 6>(0u, 0u, 1u, 0u, 1u, 1u);
    let s = sector + du[corner];
    let k = min(band + dr[corner], RINGS);
    let a = f32(s) * 6.2831853 / f32(SECTORS);
    let rh = ring(k);
    let p = c.centre.xyz + vec3<f32>(cos(a) * rh.x * c.radii.x, rh.y * c.radii.y, sin(a) * rh.x * c.radii.z);
    return occ.view_proj * vec4<f32>(p, 1.0);
}
