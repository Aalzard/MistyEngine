// Indirect dispatch arguments of a Visual Effect sort (see sort.wgsl).

struct SortParams {
    world: mat4x4<f32>,
    camera: vec4<f32>,
    mode: u32,
    world_space: u32,
    size: u32,
    stages: u32,
};

@group(0) @binding(0) var<storage, read> arg_counters: array<u32>;
@group(0) @binding(1) var<storage, read_write> args: array<u32>;
@group(0) @binding(2) var<uniform> arg_params: SortParams;

fn padded(n: u32) -> u32 {
    var p = 1u;
    while (p < n) {
        p = p << 1u;
    }
    return p;
}

@compute @workgroup_size(1)
fn cs_sort_args() {
    // Counters: dead, alive[2], spawned, reaped, last_alive (word 5).
    let p2 = padded(arg_counters[5]);
    let m = max(p2, 512u);
    args[0] = m / 256u;
    args[1] = 1u;
    args[2] = 1u;
    args[3] = 0u;
    args[4] = m / 512u;
    args[5] = 1u;
    args[6] = 1u;
    args[7] = 0u;
    for (var s = 0u; s < arg_params.stages; s = s + 1u) {
        let k = 1024u << s;
        var groups = 0u;
        if (k <= p2) {
            groups = p2 / 512u;
        }
        let b = 8u + s * 8u;
        args[b] = groups;
        args[b + 1u] = 1u;
        args[b + 2u] = 1u;
        args[b + 3u] = 0u;
        args[b + 4u] = groups;
        args[b + 5u] = 1u;
        args[b + 6u] = 1u;
        args[b + 7u] = 0u;
    }
}

