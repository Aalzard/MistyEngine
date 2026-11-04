// FFT ocean (compute): animate the initial spectrum, inverse FFT rows then columns (radix-2 in
// workgroup memory), then write displacement / slope / foam textures and their mips.
//
// Eight real fields travel as four complex signals (two real fields per complex IFFT):
//   c0 = h + i·Dx    c1 = Dz + i·∂h/∂x    c2 = ∂h/∂z + i·∂Dx/∂x    c3 = ∂Dz/∂z + i·∂Dx/∂z

const N: u32 = 256u;
const LOG_N: u32 = 8u;
const PI: f32 = 3.14159265;
const GRAVITY: f32 = 9.81;

struct FftParams {
    // x = time, y = tile size (m), z = choppiness, w = frame delta.
    a: vec4<f32>,
    // x = foam threshold (Jacobian), y = foam gain, z = foam fade time (s).
    b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: FftParams;
// Per cell: h0(k).xy, conj(h0(-k)).xy.
@group(0) @binding(1) var<storage, read> h0: array<vec4<f32>>;
// Two vec4 per cell: (c0, c1), (c2, c3).
@group(0) @binding(2) var<storage, read_write> work: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> foam: array<f32>;
// x = Dx, y = height, z = Dz, w = Jacobian.
@group(0) @binding(4) var displacement: texture_storage_2d<rgba16float, write>;
// x = ∂h/∂x, y = ∂h/∂z (after the sideways motion), z = foam.
@group(0) @binding(5) var slope: texture_storage_2d<rgba16float, write>;

fn cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

// i·z
fn times_i(z: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(-z.y, z.x);
}

@compute @workgroup_size(8, 8)
fn spectrum(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= N || id.y >= N) {
        return;
    }
    let i = id.y * N + id.x;
    let half = f32(N / 2u);
    let k = (vec2<f32>(f32(id.x), f32(id.y)) - half) * (2.0 * PI / params.a.y);
    let kl = length(k);
    let w = sqrt(GRAVITY * kl);
    let phase = w * params.a.x;
    let e = vec2<f32>(cos(phase), sin(phase));
    let s = h0[i];
    let h = cmul(s.xy, e) + cmul(s.zw, vec2<f32>(e.x, -e.y));
    var u = vec2<f32>(0.0);
    if (kl > 1e-6) {
        u = k / kl;
    }
    let chop = params.a.z;
    // -i·u·h
    let dx = chop * u.x * vec2<f32>(h.y, -h.x);
    let dz = chop * u.y * vec2<f32>(h.y, -h.x);
    let dhdx = k.x * times_i(h);
    let dhdz = k.y * times_i(h);
    let dxdx = chop * k.x * u.x * h;
    let dzdz = chop * k.y * u.y * h;
    let dxdz = chop * k.y * u.x * h;
    let c0 = h + times_i(dx);
    let c1 = dz + times_i(dhdx);
    let c2 = dhdz + times_i(dxdx);
    let c3 = dzdz + times_i(dxdz);
    work[2u * i] = vec4<f32>(c0, c1);
    work[2u * i + 1u] = vec4<f32>(c2, c3);
}

var<workgroup> line_a: array<vec4<f32>, 256>;
var<workgroup> line_b: array<vec4<f32>, 256>;

fn bit_reverse(i: u32) -> u32 {
    return reverseBits(i) >> (32u - LOG_N);
}

// One inverse FFT of `line` (a row when `rows`, else a column); 128 threads, 2 elements each.
fn fft_line(line: u32, t: u32, rows: bool) {
    for (var e = 0u; e < 2u; e++) {
        let idx = t + e * (N / 2u);
        var cell = idx * N + line;
        if (rows) {
            cell = line * N + idx;
        }
        let r = bit_reverse(idx);
        line_a[r] = work[2u * cell];
        line_b[r] = work[2u * cell + 1u];
    }
    workgroupBarrier();
    for (var s = 1u; s <= LOG_N; s++) {
        let m = 1u << s;
        let h = m >> 1u;
        let j = t % h;
        let i0 = (t / h) * m + j;
        let i1 = i0 + h;
        let angle = 2.0 * PI * f32(j) / f32(m);
        let w = vec2<f32>(cos(angle), sin(angle));
        let a0 = line_a[i0];
        let a1 = line_a[i1];
        let b0 = line_b[i0];
        let b1 = line_b[i1];
        let ta = vec4<f32>(cmul(w, a1.xy), cmul(w, a1.zw));
        let tb = vec4<f32>(cmul(w, b1.xy), cmul(w, b1.zw));
        workgroupBarrier();
        line_a[i0] = a0 + ta;
        line_a[i1] = a0 - ta;
        line_b[i0] = b0 + tb;
        line_b[i1] = b0 - tb;
        workgroupBarrier();
    }
    for (var e = 0u; e < 2u; e++) {
        let idx = t + e * (N / 2u);
        var cell = idx * N + line;
        if (rows) {
            cell = line * N + idx;
        }
        work[2u * cell] = line_a[idx];
        work[2u * cell + 1u] = line_b[idx];
    }
}

@compute @workgroup_size(128)
fn fft_rows(@builtin(workgroup_id) g: vec3<u32>, @builtin(local_invocation_id) l: vec3<u32>) {
    fft_line(g.x, l.x, true);
}

@compute @workgroup_size(128)
fn fft_columns(@builtin(workgroup_id) g: vec3<u32>, @builtin(local_invocation_id) l: vec3<u32>) {
    fft_line(g.x, l.x, false);
}

@compute @workgroup_size(8, 8)
fn finalize(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= N || id.y >= N) {
        return;
    }
    let i = id.y * N + id.x;
    // Wave numbers were stored from -N/2: undo the resulting (-1)^(x+y) factor.
    var sign = 1.0;
    if (((id.x + id.y) & 1u) == 1u) {
        sign = -1.0;
    }
    let a = work[2u * i] * sign;
    let b = work[2u * i + 1u] * sign;
    let h = a.x;
    let dx = a.y;
    let dz = a.z;
    let dhdx = a.w;
    let dhdz = b.x;
    let dxdx = b.y;
    let dzdz = b.z;
    let dxdz = b.w;
    let jacobian = (1.0 + dxdx) * (1.0 + dzdz) - dxdz * dxdz;
    // Foam builds where the surface folds (Jacobian below the threshold) and fades over time.
    let fresh = clamp((params.b.x - jacobian) * params.b.y, 0.0, 1.0);
    let fade = exp(-params.a.w / max(params.b.z, 0.01));
    let f = max(foam[i] * fade, fresh);
    foam[i] = f;
    let sx = dhdx / max(1.0 + dxdx, 0.2);
    let sz = dhdz / max(1.0 + dzdz, 0.2);
    let p = vec2<i32>(id.xy);
    textureStore(displacement, p, vec4<f32>(dx, h, dz, jacobian));
    textureStore(slope, p, vec4<f32>(sx, sz, f, 1.0));
}
