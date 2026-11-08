// GPU sort of a Visual Effect system's alive list (bitonic, keys + particle indices).
//
// `cs_sort_args` turns the alive count into indirect dispatches (the padded power of two `p2`,
// at least one 512-element block); `cs_sort_init` writes keys (distance from the camera,
// inverted so the farthest comes first, or the age so the youngest comes first) and the
// particle indices, padding with 0xffffffff; `cs_sort_local` sorts every 512-element block in
// workgroup memory; then each larger stage k runs global compare-exchange steps for the long
// distances (`cs_sort_step`) and finishes the short ones in workgroup memory
// (`cs_sort_merge`). Stages larger than `p2` get zero workgroups from `cs_sort_args`.

struct SortParticle {
    position: vec3<f32>,
    age: f32,
    velocity: vec3<f32>,
    lifetime: f32,
    color: vec4<f32>,
    size: f32,
    angle: f32,
    seed: u32,
    alive: u32,
};

struct SortParams {
    world: mat4x4<f32>,
    // xyz = camera position.
    camera: vec4<f32>,
    // 0 = distance (back to front), 1 = age (youngest first).
    mode: u32,
    world_space: u32,
    // Length of the key / value buffers (a power of two, at least 512).
    size: u32,
    stages: u32,
};

struct SortPass {
    k: u32,
    j: u32,
    pad0: u32,
    pad1: u32,
};

@group(0) @binding(0) var<uniform> params: SortParams;
@group(0) @binding(1) var<storage, read> particles: array<SortParticle>;
@group(0) @binding(2) var<storage, read> alive: array<u32>;
@group(0) @binding(3) var<storage, read> counters: array<u32>;
@group(0) @binding(4) var<storage, read_write> keys: array<u32>;
@group(0) @binding(5) var<storage, read_write> vals: array<u32>;
@group(1) @binding(0) var<uniform> pass_params: SortPass;

@compute @workgroup_size(256)
fn cs_sort_init(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.size) {
        return;
    }
    let n = counters[5];
    if (i >= n) {
        keys[i] = 0xffffffffu;
        vals[i] = 0xffffffffu;
        return;
    }
    let index = alive[i];
    let p = particles[index];
    var key = 0u;
    if (params.mode == 0u) {
        var pos = p.position;
        if (params.world_space == 0u) {
            pos = (params.world * vec4<f32>(pos, 1.0)).xyz;
        }
        // Farther first: invert the (positive) float's bits.
        key = ~bitcast<u32>(max(distance(pos, params.camera.xyz), 1e-6));
    } else {
        key = bitcast<u32>(max(p.age, 0.0));
    }
    keys[i] = min(key, 0xfffffffeu);
    vals[i] = index;
}

var<workgroup> wk: array<u32, 512>;
var<workgroup> wv: array<u32, 512>;

fn local_compare(base: u32, t: u32, k: u32, j: u32) {
    let i = (t / j) * 2u * j + (t % j);
    let l = i + j;
    let ascending = ((base + i) & k) == 0u;
    let a = wk[i];
    let b = wk[l];
    if ((a > b) == ascending && a != b) {
        wk[i] = b;
        wk[l] = a;
        let v = wv[i];
        wv[i] = wv[l];
        wv[l] = v;
    }
}

@compute @workgroup_size(256)
fn cs_sort_local(@builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    let base = wid.x * 512u;
    let t = lid.x;
    wk[t] = keys[base + t];
    wv[t] = vals[base + t];
    wk[t + 256u] = keys[base + t + 256u];
    wv[t + 256u] = vals[base + t + 256u];
    workgroupBarrier();
    for (var k = 2u; k <= 512u; k = k << 1u) {
        for (var j = k >> 1u; j > 0u; j = j >> 1u) {
            local_compare(base, t, k, j);
            workgroupBarrier();
        }
    }
    keys[base + t] = wk[t];
    vals[base + t] = wv[t];
    keys[base + t + 256u] = wk[t + 256u];
    vals[base + t + 256u] = wv[t + 256u];
}

@compute @workgroup_size(256)
fn cs_sort_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = pass_params.k;
    let j = pass_params.j;
    let t = gid.x;
    let i = (t / j) * 2u * j + (t % j);
    let l = i + j;
    if (l >= params.size) {
        return;
    }
    let ascending = (i & k) == 0u;
    let a = keys[i];
    let b = keys[l];
    if ((a > b) == ascending && a != b) {
        keys[i] = b;
        keys[l] = a;
        let v = vals[i];
        vals[i] = vals[l];
        vals[l] = v;
    }
}

@compute @workgroup_size(256)
fn cs_sort_merge(@builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    let k = pass_params.k;
    let base = wid.x * 512u;
    let t = lid.x;
    wk[t] = keys[base + t];
    wv[t] = vals[base + t];
    wk[t + 256u] = keys[base + t + 256u];
    wv[t + 256u] = vals[base + t + 256u];
    workgroupBarrier();
    for (var j = 256u; j > 0u; j = j >> 1u) {
        local_compare(base, t, k, j);
        workgroupBarrier();
    }
    keys[base + t] = wk[t];
    vals[base + t] = wv[t];
    keys[base + t + 256u] = wk[t + 256u];
    vals[base + t + 256u] = wv[t + 256u];
}
