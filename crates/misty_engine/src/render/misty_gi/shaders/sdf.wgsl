// Global SDF compose (appended to gi_common.wgsl): every voxel of a level marked for recompose
// takes the distance to the closest shape of the level's list, with that shape's albedo and
// emission. Distances are capped at 8 voxels: shapes beyond the level's (grown) box are not in
// its list, so a larger value could step over them.

fn sd_box(q: vec3<f32>, e: vec3<f32>) -> f32 {
    let d = abs(q) - e;
    return length(max(d, vec3<f32>(0.0))) + min(max(d.x, max(d.y, d.z)), 0.0);
}

fn sd_ellipsoid(q: vec3<f32>, r: vec3<f32>) -> f32 {
    let rr = max(r, vec3<f32>(1e-4));
    let k0 = length(q / rr);
    let k1 = length(q / (rr * rr));
    if (k1 < 1e-6) {
        return -min(rr.x, min(rr.y, rr.z));
    }
    return k0 * (k0 - 1.0) / k1;
}

fn sd_capsule(q: vec3<f32>, r: f32, h: f32) -> f32 {
    var c = q;
    c.y -= clamp(c.y, -h, h);
    return length(c) - r;
}

fn sd_cylinder(q: vec3<f32>, r: f32, h: f32) -> f32 {
    let d = vec2<f32>(length(q.xz) - r, abs(q.y) - h);
    return min(max(d.x, d.y), 0.0) + length(max(d, vec2<f32>(0.0)));
}

// Baked mesh SDF (mesh space; outside its box: distance to the box plus the value at the box).
fn sd_mesh(inst: Instance, q: vec3<f32>) -> f32 {
    let lo = inst.bounds_min.xyz;
    let hi = inst.bounds_max.xyz;
    let c = clamp(q, lo, hi);
    let outside = length(q - c);
    let rel = (c - lo) / max(hi - lo, vec3<f32>(1e-6));
    let texel = inst.shape.xyz + 0.5 + rel * (MESH_BRICK - 1.0);
    let uvw = texel / vec3<f32>(textureDimensions(mesh_atlas));
    let inside = textureSampleLevel(mesh_atlas, linear_sampler, uvw, 0.0).x;
    return (inside + outside) * inst.shape.w;
}

// Normalised terrain height at texel (x, z) of its atlas region.
fn terrain_texel(inst: Instance, x: i32, z: i32) -> f32 {
    let cols = i32(inst.bounds_min.z);
    let rows = i32(inst.bounds_min.w);
    let t = vec2<i32>(clamp(x, 0, cols - 1), clamp(z, 0, rows - 1)) + vec2<i32>(inst.bounds_min.xy);
    return textureLoad(terrain_heights, t, 0).x;
}

// Heightfield distance: vertical distance over the slope's stretch; outside the terrain's
// footprint, at least the horizontal distance to it.
fn sd_terrain(inst: Instance, q: vec3<f32>) -> f32 {
    let size = inst.bounds_max.xyz;
    let cols = inst.bounds_min.z;
    let rows = inst.bounds_min.w;
    let uv = clamp(q.xz / size.xz, vec2<f32>(0.0), vec2<f32>(1.0));
    let g = uv * vec2<f32>(cols - 1.0, rows - 1.0);
    let b = vec2<i32>(floor(g));
    let f = g - floor(g);
    let h00 = terrain_texel(inst, b.x, b.y);
    let h10 = terrain_texel(inst, b.x + 1, b.y);
    let h01 = terrain_texel(inst, b.x, b.y + 1);
    let h11 = terrain_texel(inst, b.x + 1, b.y + 1);
    let h = mix(mix(h00, h10, f.x), mix(h01, h11, f.x), f.y) * size.y;
    let cell = size.xz / vec2<f32>(cols - 1.0, rows - 1.0);
    let gx = (mix(h10, h11, f.y) - mix(h00, h01, f.y)) * size.y / cell.x;
    let gz = (mix(h01, h11, f.x) - mix(h00, h10, f.x)) * size.y / cell.y;
    let vertical = (q.y - h) / sqrt(1.0 + gx * gx + gz * gz);
    let out = max(max(-q.xz, q.xz - size.xz), vec2<f32>(0.0));
    let horizontal = length(out);
    if (horizontal > 0.0) {
        return max(vertical, horizontal);
    }
    return vertical;
}

fn shape_distance(inst: Instance, pos: vec3<f32>) -> f32 {
    let q = (inst.inv * vec4<f32>(pos, 1.0)).xyz;
    let kind = u32(inst.extent.w);
    let e = inst.extent.xyz;
    switch (kind) {
        case 0u: {
            return sd_box(q, e);
        }
        case 1u: {
            return sd_ellipsoid(q, e);
        }
        case 2u: {
            return sd_capsule(q, e.x, e.y);
        }
        case 3u: {
            return sd_cylinder(q, e.x, e.y);
        }
        case 4u: {
            return sd_mesh(inst, q);
        }
        default: {
            return sd_terrain(inst, q);
        }
    }
}

fn aabb_distance(pos: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>) -> f32 {
    let d = max(max(lo - pos, pos - hi), vec3<f32>(0.0));
    return length(d);
}

@compute @workgroup_size(4, 4, 4)
fn cs_sdf(@builtin(global_invocation_id) id: vec3<u32>) {
    let d = u32(dim());
    let k = i32(id.z / d);
    if (id.x >= d || id.y >= d || k >= level_count()) {
        return;
    }
    let level = p.levels[k];
    if (level.flags.x < 0.5) {
        return;
    }
    let v = level.sdf.w;
    let local = vec3<f32>(f32(id.x), f32(id.y), f32(id.z - u32(k) * d)) + 0.5;
    let pos = level.sdf.xyz + local * v;
    var best = 8.0 * v;
    var albedo = vec3<f32>(0.0);
    var emission = vec3<f32>(0.0);
    let first = u32(level.flags.y);
    let count = u32(level.flags.z);
    for (var i = 0u; i < count; i++) {
        let inst = instances[lists[first + i]];
        if (aabb_distance(pos, inst.aabb_min.xyz, inst.aabb_max.xyz) >= best) {
            continue;
        }
        let s = shape_distance(inst, pos);
        if (s < best) {
            best = s;
            albedo = inst.albedo.rgb;
            emission = inst.emission.rgb;
        }
    }
    textureStore(sdf_out, vec3<i32>(id), vec4<f32>(best, albedo));
    textureStore(emission_out, vec3<i32>(id), vec4<f32>(emission, 0.0));
}
