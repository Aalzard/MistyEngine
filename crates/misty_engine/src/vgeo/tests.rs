use std::collections::{HashMap, HashSet};

use glam::Vec3;

use super::cluster::edge;
use super::*;

fn rock_mesh(subdivisions: u32) -> (Vec<Vec3>, Vec<u32>) {
    let data = rock::rock(subdivisions, 3);
    let positions = data.vertices.iter().map(|v| v.position.into()).collect();
    (positions, data.indices)
}

/// A flat grid with an open border and a UV seam down the middle (duplicated vertices).
fn seamed_grid(n: usize) -> (Vec<Vec3>, Vec<u32>) {
    let mut positions = Vec::new();
    let mut index = HashMap::new();
    let mut indices = Vec::new();
    let mut vertex = |x: usize, z: usize, side: usize, positions: &mut Vec<Vec3>| {
        // Column n/2 has two vertices (left and right of the seam).
        let side = if x == n / 2 { side } else { 0 };
        *index.entry((x, z, side)).or_insert_with(|| {
            let h = ((x * 7 + z * 13) % 5) as f32 * 0.01;
            positions.push(Vec3::new(x as f32, h, z as f32));
            positions.len() as u32 - 1
        })
    };
    for z in 0..n {
        for x in 0..n {
            let side = if x < n / 2 { 0 } else { 1 };
            let a = vertex(x, z, side, &mut positions);
            let b = vertex(x + 1, z, side, &mut positions);
            let c = vertex(x, z + 1, side, &mut positions);
            let d = vertex(x + 1, z + 1, side, &mut positions);
            indices.extend([a, c, b, b, c, d]);
        }
    }
    (positions, indices)
}

/// Directed edges (position ids) of a set of triangles.
fn directed(mesh: &VirtualMesh, clusters: &[usize], weld: &[u32]) -> HashMap<(u32, u32), u32> {
    let mut out = HashMap::new();
    for &c in clusters {
        for t in mesh.triangles(c).as_chunks::<3>().0 {
            for k in 0..3 {
                let e = (weld[t[k] as usize], weld[t[(k + 1) % 3] as usize]);
                *out.entry(e).or_default() += 1;
            }
        }
    }
    out
}

/// Undirected edges used by exactly one triangle of the set.
fn border(mesh: &VirtualMesh, clusters: &[usize], weld: &[u32]) -> HashSet<(u32, u32)> {
    let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
    for (e, n) in directed(mesh, clusters, weld) {
        *uses.entry(edge(e.0, e.1)).or_default() += n;
    }
    uses.into_iter()
        .filter(|(_, n)| *n == 1)
        .map(|(e, _)| e)
        .collect()
}

#[test]
fn clusters_hold_every_source_triangle_once_and_at_most_128() {
    let (positions, indices) = rock_mesh(4);
    let mesh = VirtualMesh::build(&positions, &indices);
    assert_eq!(mesh.source_triangles, indices.len() / 3);
    let mut seen: HashMap<[u32; 3], u32> = HashMap::new();
    for (i, c) in mesh.clusters.iter().enumerate() {
        assert!(c.triangles as usize <= MAX_TRIANGLES && c.triangles > 0);
        if c.level == 0 {
            for t in mesh.triangles(i).as_chunks::<3>().0 {
                *seen.entry(*t).or_default() += 1;
            }
        }
    }
    let source: HashSet<[u32; 3]> = indices.as_chunks::<3>().0.iter().copied().collect();
    assert_eq!(seen.len(), source.len());
    assert!(seen.iter().all(|(t, n)| *n == 1 && source.contains(t)));
    // Clusters are mostly full: the GPU runs 3 x 128 vertices per cluster whatever it holds.
    let fill = mesh.source_triangles as f32 / (mesh.finest() * MAX_TRIANGLES) as f32;
    assert!(
        fill >= 0.8,
        "{:.0} % full ({} clusters)",
        fill * 100.0,
        mesh.finest()
    );
}

#[test]
fn hierarchy_halves_down_to_a_few_roots() {
    let (positions, indices) = rock_mesh(5);
    let mesh = VirtualMesh::build(&positions, &indices);
    let roots: Vec<&Cluster> = mesh
        .clusters
        .iter()
        .filter(|c| c.parent_error >= ROOT_ERROR)
        .collect();
    let root_tris: u32 = roots.iter().map(|c| c.triangles).sum();
    assert!(mesh.levels >= 5, "{} levels", mesh.levels);
    assert!(
        (root_tris as usize) * 20 < mesh.source_triangles,
        "{root_tris} root triangles of {}",
        mesh.source_triangles
    );
    // Sorted by parent error, roots last.
    assert!(
        mesh.clusters
            .windows(2)
            .all(|w| w[0].parent_error <= w[1].parent_error)
    );
    assert!(
        mesh.stuck_groups as usize * 4 <= mesh.groups.len(),
        "{} of {}",
        mesh.stuck_groups,
        mesh.groups.len()
    );
}

#[test]
fn errors_and_spheres_grow_towards_the_roots() {
    let (positions, indices) = rock_mesh(4);
    let mesh = VirtualMesh::build(&positions, &indices);
    let mut by_group: HashMap<u32, (Sphere, f32)> = HashMap::new();
    for c in &mesh.clusters {
        assert!(c.parent_error > c.error, "{c:?}");
        assert!(c.parent.contains(&c.lod), "{c:?}");
        assert!(mesh.bounds.contains(&c.bounds) && mesh.bounds.contains(&c.lod));
        if c.level > 0 {
            // Every cluster a group made shares the group's values...
            let v = by_group.entry(c.group).or_insert((c.lod, c.error));
            assert_eq!(*v, (c.lod, c.error));
        }
    }
    for c in &mesh.clusters {
        // ...and they are exactly what its children replace them with, and the group's record.
        if c.parent_group != u32::MAX {
            assert_eq!(by_group[&c.parent_group], (c.parent, c.parent_error));
            let g = mesh.groups[c.parent_group as usize];
            assert_eq!((g.sphere, g.error), (c.parent, c.parent_error));
        }
        if c.group != u32::MAX {
            let g = mesh.groups[c.group as usize];
            assert_eq!((g.sphere, g.error), (c.lod, c.error));
        }
    }
    // Groups by error (the GPU's binary search), every one used.
    assert!(mesh.groups.windows(2).all(|w| w[0].error <= w[1].error));
    assert_eq!(by_group.len(), mesh.groups.len());
}

/// The cut of `view` with the decision of each group made by `fine` (`None` = no group).
fn cut_with(
    mesh: &VirtualMesh,
    own: impl Fn(u32) -> bool,
    parent: impl Fn(u32) -> bool,
) -> Vec<usize> {
    (0..mesh.clusters.len())
        .filter(|&i| {
            let c = &mesh.clusters[i];
            let fine_own = c.group == u32::MAX || own(c.group);
            let fine_parent = c.parent_group != u32::MAX && parent(c.parent_group);
            fine_own && !fine_parent
        })
        .collect()
}

#[test]
fn one_decision_per_group_is_crack_free_whatever_the_rounding() {
    let (positions, indices) = rock_mesh(4);
    let mesh = VirtualMesh::build(&positions, &indices);
    let weld = weld(&positions);
    let base = LodView {
        camera: Vec3::new(2.0, 0.8, 3.0),
        scale: 1080.0 / (2.0 * 30f32.to_radians().tan()),
        orthographic: false,
        threshold: 1.0,
    };
    let mut caught = 0;
    // Put the threshold exactly on a group's projected error: the worst case, where two
    // evaluations rounding differently disagree.
    for g in (0..mesh.groups.len()).step_by(7) {
        let gr = mesh.groups[g];
        let view = LodView {
            threshold: base.projected(gr.sphere, gr.error),
            ..base
        };
        let decide = |g: u32, bias: f32| {
            let gr = mesh.groups[g as usize];
            view.projected(gr.sphere, gr.error) <= view.threshold * (1.0 + bias)
        };
        for bias in [-1e-6f32, 1e-6] {
            // One decision per group, read by both sides of its border: closed either way.
            let shared = cut_with(&mesh, |g| decide(g, bias), |g| decide(g, bias));
            watertight(&mesh, &shared, &weld)
                .unwrap_or_else(|e| panic!("group {g}, bias {bias}: {e}"));
            // The same group evaluated twice (own error vs children's parent error) with
            // different rounding: overlaps or holes. What the per-group decision rules out.
            let split = cut_with(&mesh, |g| decide(g, bias), |g| decide(g, -bias));
            caught += watertight(&mesh, &split, &weld).is_err() as u32;
        }
    }
    assert!(caught > 0, "the test should see the two-evaluation failure");
}

/// The rock with the triangles on the +x side moved to a second part (their own vertices: a
/// seam between the parts), as a glTF mesh of two primitives combined. Returns the positions,
/// indices and the parts' first vertices.
fn two_part_rock() -> (Vec<Vec3>, Vec<u32>, Vec<u32>) {
    let (positions, indices) = rock_mesh(4);
    let tris = indices.as_chunks::<3>().0;
    let side = |t: &[u32; 3]| t.iter().map(|&v| positions[v as usize].x).sum::<f32>() > 0.0;
    let mut out_pos = positions.clone();
    let mut out_idx = Vec::new();
    for t in tris.iter().filter(|t| !side(t)) {
        out_idx.extend_from_slice(t);
    }
    let start = out_pos.len() as u32;
    let mut copy: HashMap<u32, u32> = HashMap::new();
    for t in tris.iter().filter(|t| side(t)) {
        for &v in t {
            let n = *copy.entry(v).or_insert_with(|| {
                out_pos.push(positions[v as usize]);
                out_pos.len() as u32 - 1
            });
            out_idx.push(n);
        }
    }
    (out_pos, out_idx, vec![0, start])
}

#[test]
fn parts_share_one_hierarchy_with_one_part_per_cluster() {
    let (positions, indices, starts) = two_part_rock();
    let mesh = VirtualMesh::build_parts(&positions, &indices, &starts);
    assert_eq!(mesh.materials, 2);
    let part = |v: u32| (v >= starts[1]) as u32;
    let mut seen = [0usize; 2];
    for (i, c) in mesh.clusters.iter().enumerate() {
        assert!(
            mesh.triangles(i).iter().all(|&v| part(v) == c.material),
            "cluster {i} mixes parts"
        );
        seen[c.material as usize] += 1;
    }
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
    // One hierarchy: groups across the seam, a few roots, and every cut closed (by position).
    assert!(mesh.levels >= 4, "{} levels", mesh.levels);
    let weld = weld(&positions);
    for distance in [1.0f32, 4.0, 20.0, 200.0] {
        let view = LodView {
            camera: Vec3::new(0.3, 0.5, 1.0).normalize() * distance,
            scale: 1080.0 / (2.0 * 30f32.to_radians().tan()),
            orthographic: false,
            threshold: 1.0,
        };
        watertight(&mesh, &mesh.select(&view), &weld)
            .unwrap_or_else(|e| panic!("distance {distance}: {e}"));
    }
    // The file keeps the parts.
    let bytes = file::write(&[Some((1, 0.0, &mesh))]);
    let back = file::read(&bytes).expect("readable");
    let e = back[0].as_ref().expect("mesh");
    assert_eq!(e.mesh.materials, 2);
    assert_eq!(e.mesh.clusters, mesh.clusters);
}

#[test]
fn simplified_groups_keep_their_border_exactly() {
    for (positions, indices) in [rock_mesh(4), seamed_grid(40)] {
        let mesh = VirtualMesh::build(&positions, &indices);
        let weld = weld(&positions);
        let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut made: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, c) in mesh.clusters.iter().enumerate() {
            if c.parent_group != u32::MAX {
                children.entry(c.parent_group).or_default().push(i);
            }
            if c.group != u32::MAX {
                made.entry(c.group).or_default().push(i);
            }
        }
        assert!(!made.is_empty());
        for (g, kids) in &children {
            let before = border(&mesh, kids, &weld);
            let after = border(&mesh, &made[g], &weld);
            assert_eq!(before, after, "group {g}");
        }
    }
}

/// Is the selected surface closed and consistently wound (every directed edge matched by its
/// reverse exactly once)? A crack or an overlap between levels breaks this.
fn watertight(mesh: &VirtualMesh, selected: &[usize], weld: &[u32]) -> Result<(), String> {
    let edges = directed(mesh, selected, weld);
    for (&(a, b), &n) in &edges {
        let back = edges.get(&(b, a)).copied().unwrap_or(0);
        if n != 1 || back != 1 {
            return Err(format!("edge {a}-{b}: {n} forward, {back} back"));
        }
    }
    Ok(())
}

#[test]
fn every_cut_is_crack_free_and_covers_the_surface_once() {
    let (positions, indices) = rock_mesh(5);
    let mesh = VirtualMesh::build(&positions, &indices);
    let weld = weld(&positions);
    // The source mesh is closed, so every cut must be too.
    let all0: Vec<usize> = (0..mesh.clusters.len())
        .filter(|&i| mesh.clusters[i].level == 0)
        .collect();
    watertight(&mesh, &all0, &weld).expect("source");
    let mut sizes = HashSet::new();
    for (i, distance) in [0.5f32, 2.0, 3.0, 5.0, 8.0, 15.0, 40.0, 120.0, 1000.0]
        .into_iter()
        .enumerate()
    {
        // Cameras around the rock at various distances: near parts finer than far ones.
        let dir = Vec3::new((i as f32 * 1.7).cos(), 0.4, (i as f32 * 1.7).sin()).normalize();
        for threshold in [0.5, 1.0, 4.0] {
            let view = LodView {
                camera: dir * distance,
                scale: 1080.0 / (2.0 * 30f32.to_radians().tan()),
                orthographic: false,
                threshold,
            };
            let selected = mesh.select(&view);
            watertight(&mesh, &selected, &weld)
                .unwrap_or_else(|e| panic!("distance {distance}, {threshold} px: {e}"));
            sizes.insert(
                selected
                    .iter()
                    .map(|&c| mesh.clusters[c].triangles)
                    .sum::<u32>(),
            );
        }
    }
    assert!(sizes.len() > 5, "the cuts should differ: {sizes:?}");
    // No error allowed: the source triangles. Any error: the roots.
    let exact = LodView {
        camera: Vec3::new(0.0, 0.0, 3.0),
        scale: 1000.0,
        orthographic: false,
        threshold: 0.0,
    };
    assert_eq!(mesh.select(&exact), all0);
    let coarse = LodView {
        threshold: 1e30,
        ..exact
    };
    let roots: Vec<usize> = (0..mesh.clusters.len())
        .filter(|&i| mesh.clusters[i].parent_error >= ROOT_ERROR)
        .collect();
    assert_eq!(mesh.select(&coarse), roots);
}

#[test]
fn seams_and_open_borders_survive_simplification() {
    let (positions, indices) = seamed_grid(40);
    let mesh = VirtualMesh::build(&positions, &indices);
    let weld = weld(&positions);
    let open = |sel: &[usize]| border(&mesh, sel, &weld);
    let all0: Vec<usize> = (0..mesh.clusters.len())
        .filter(|&i| mesh.clusters[i].level == 0)
        .collect();
    let roots: Vec<usize> = (0..mesh.clusters.len())
        .filter(|&i| mesh.clusters[i].parent_error >= ROOT_ERROR)
        .collect();
    assert!(mesh.levels > 1);
    // The grid's outline is the same at every level, and the seam's two sides keep their own
    // vertices (each triangle at the seam still uses its side's copy).
    assert_eq!(open(&all0), open(&roots));
    let seam_x = 20.0;
    for &c in &roots {
        for t in mesh.triangles(c).as_chunks::<3>().0 {
            let xs: Vec<f32> = t.iter().map(|&v| positions[v as usize].x).collect();
            let left = xs.iter().any(|&x| x < seam_x);
            let right = xs.iter().any(|&x| x > seam_x);
            assert!(!(left && right), "a triangle crosses the seam");
        }
    }
}

#[test]
fn simplified_lod_levels_shrink() {
    let data = rock::rock(4, 1);
    let half = rock::simplified(&data, 0.5);
    let tenth = rock::simplified(&data, 0.1);
    let n = data.indices.len() / 3;
    assert!(half.indices.len() / 3 <= n / 2 + 8);
    assert!(tenth.indices.len() / 3 <= n / 10 + 8);
    assert!(tenth.vertices.len() < data.vertices.len() / 5);
}

#[test]
fn normal_cones_only_cull_clusters_facing_away() {
    let (positions, indices) = rock_mesh(4);
    let mesh = VirtualMesh::build(&positions, &indices);
    let r = mesh.bounds.radius;
    let mut culled = 0;
    let mut tested = 0;
    for k in 0..48 {
        // Eyes all around the rock, near (inside a few radii) and far.
        let a = k as f32 * 2.399;
        let y = 1.0 - 2.0 * (k as f32 + 0.5) / 48.0;
        let dir = Vec3::new(
            a.cos() * (1.0 - y * y).sqrt(),
            y,
            a.sin() * (1.0 - y * y).sqrt(),
        );
        let eye = mesh.bounds.center + dir * r * if k % 2 == 0 { 1.6 } else { 12.0 };
        for (i, c) in mesh.clusters.iter().enumerate() {
            tested += 1;
            if !c.backfacing(eye) {
                continue;
            }
            culled += 1;
            for t in mesh.triangles(i).as_chunks::<3>().0 {
                let [p0, p1, p2] = t.map(|v| positions[v as usize]);
                let n = (p1 - p0).cross(p2 - p0);
                for p in [p0, p1, p2] {
                    assert!(
                        n.dot(p - eye) >= -1e-6 * n.length() * (p - eye).length(),
                        "cluster {i} has a triangle facing the eye {eye}"
                    );
                }
            }
        }
    }
    // A closed rock shows about half its surface, but its bumps spread each cluster's
    // normals widely: about an eighth of all clusters (every level, near and far eyes).
    let share = culled as f32 / tested as f32;
    eprintln!("normal cones cull {share:.3} of the clusters");
    assert!(share > 0.08, "only {share:.2} of the clusters culled");
}

#[test]
fn vgeo_files_round_trip_and_reject_damage() {
    let (positions, indices) = rock_mesh(3);
    let mesh = VirtualMesh::build(&positions, &indices);
    let hash = file::content_hash(&positions, &indices);
    let bytes = file::write(&[None, Some((hash, 12.5, &mesh))]);
    let back = file::read(&bytes).expect("readable");
    assert_eq!(back.len(), 2);
    assert!(back[0].is_none());
    let e = back[1].as_ref().expect("second mesh");
    assert_eq!((e.hash, e.build_ms), (hash, 12.5));
    assert_eq!(e.mesh.clusters, mesh.clusters);
    assert_eq!(e.mesh.indices, mesh.indices);
    assert_eq!(e.mesh.bounds, mesh.bounds);
    assert_eq!(e.mesh.groups, mesh.groups);
    assert_eq!(
        (e.mesh.levels, e.mesh.source_triangles, e.mesh.materials),
        (mesh.levels, mesh.source_triangles, mesh.materials)
    );
    // Truncated, another version or another source: not used.
    assert!(file::read(&bytes[..bytes.len() - 3]).is_none());
    let mut other = bytes.clone();
    other[8] ^= 1;
    assert!(file::read(&other).is_none());
    let mut moved = positions.clone();
    moved[0].x += 1e-3;
    assert_ne!(file::content_hash(&moved, &indices), hash);
}
