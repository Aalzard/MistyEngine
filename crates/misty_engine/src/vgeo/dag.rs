//! The cluster hierarchy: group neighbouring clusters, simplify each group with its border
//! locked, split the result into new clusters, repeat.

use std::collections::HashMap;

use glam::Vec3;

use super::cluster::{self, edge, morton};
use super::simplify::simplify;
use super::{Cluster, GROUP_SIZE, ROOT_ERROR, Sphere, VirtualMesh, weld};

/// A cluster while the hierarchy is built.
struct Node {
    tris: Vec<[u32; 3]>,
    lod: Sphere,
    error: f32,
    parent: Sphere,
    parent_error: f32,
    level: u32,
    group: u32,
    parent_group: u32,
}

/// One simplified group: its member clusters, sphere, error and new clusters' triangles.
type Made = (Vec<usize>, Sphere, f32, Vec<Vec<[u32; 3]>>);

/// A level stops the hierarchy when it keeps more than this share of the triangles.
const MIN_LEVEL_REDUCTION: f32 = 0.95;

pub(super) fn build(positions: &[Vec3], indices: &[u32]) -> VirtualMesh {
    let weld = weld(positions);
    let tris: Vec<[u32; 3]> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .copied()
        .filter(|t| {
            let w = t.map(|v| weld[v as usize]);
            w[0] != w[1] && w[1] != w[2] && w[0] != w[2]
        })
        .collect();
    let source_triangles = tris.len();
    let points = |tris: &[[u32; 3]]| -> Vec<Vec3> {
        tris.iter()
            .flat_map(|t| t.map(|v| positions[v as usize]))
            .collect()
    };

    let mut nodes: Vec<Node> = cluster::build(&tris, positions, &weld)
        .into_iter()
        .map(|members| {
            let tris: Vec<[u32; 3]> = members.iter().map(|&t| tris[t]).collect();
            let lod = Sphere::around(&points(&tris));
            Node {
                tris,
                lod,
                error: 0.0,
                parent: lod,
                parent_error: ROOT_ERROR,
                level: 0,
                group: u32::MAX,
                parent_group: u32::MAX,
            }
        })
        .collect();

    let mut current: Vec<usize> = (0..nodes.len()).collect();
    let (mut groups_made, mut stuck) = (0u32, 0u32);
    let mut level = 0;
    while current.len() > 1 {
        let groups = group(&current, &nodes, positions, &weld);
        let before: usize = current.iter().map(|&i| nodes[i].tris.len()).sum();
        let mut made: Vec<Made> = Vec::new();
        let mut after = 0;
        let mut stuck_here = 0;
        for members in groups {
            let members: Vec<usize> = members.iter().map(|&m| current[m]).collect();
            let group_tris: Vec<[u32; 3]> = members
                .iter()
                .flat_map(|&m| nodes[m].tris.iter().copied())
                .collect();
            let border = border_positions(&group_tris, &weld);
            let target = group_tris.len() / 2;
            let s = simplify(
                &group_tris,
                positions,
                &weld,
                |p| border.contains_key(&p),
                target,
            );
            if s.tris.len() as f32 > 0.85 * group_tris.len() as f32 {
                stuck_here += 1;
            }
            let child_error = members.iter().map(|&m| nodes[m].error).fold(0.0, f32::max);
            let spheres: Vec<Sphere> = members.iter().map(|&m| nodes[m].lod).collect();
            let sphere = Sphere::merge(&spheres);
            // Strictly coarser than every child, so the cut is unique even for flat regions.
            let error = child_error + s.error.max(1e-7 * (1.0 + sphere.radius));
            let split: Vec<Vec<[u32; 3]>> = cluster::build(&s.tris, positions, &weld)
                .into_iter()
                .map(|c| c.iter().map(|&t| s.tris[t]).collect())
                .collect();
            after += s.tris.len();
            made.push((members, sphere, error, split));
        }
        if after as f32 > MIN_LEVEL_REDUCTION * before as f32 {
            break;
        }
        level += 1;
        stuck += stuck_here;
        let mut next = Vec::new();
        for (members, sphere, error, split) in made {
            let id = groups_made;
            groups_made += 1;
            for &m in &members {
                nodes[m].parent = sphere;
                nodes[m].parent_error = error;
                nodes[m].parent_group = id;
            }
            for tris in split {
                next.push(nodes.len());
                nodes.push(Node {
                    tris,
                    lod: sphere,
                    error,
                    parent: sphere,
                    parent_error: ROOT_ERROR,
                    level,
                    group: id,
                    parent_group: u32::MAX,
                });
            }
        }
        current = next;
    }

    // Order by parent error (roots last), then lay out the triangles.
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by(|&a, &b| nodes[a].parent_error.total_cmp(&nodes[b].parent_error));
    let mut indices_out = Vec::with_capacity(nodes.iter().map(|n| n.tris.len() * 3).sum());
    let mut clusters = Vec::with_capacity(nodes.len());
    let mut all = Vec::new();
    for i in order {
        let n = &nodes[i];
        let first = indices_out.len() as u32;
        for t in &n.tris {
            indices_out.extend_from_slice(t);
        }
        let bounds = Sphere::around(&points(&n.tris));
        let (cone_axis, cone_cutoff) = super::normal_cone(&n.tris, positions);
        all.push(bounds);
        all.push(n.lod);
        clusters.push(Cluster {
            first,
            triangles: n.tris.len() as u32,
            bounds,
            lod: n.lod,
            error: n.error,
            parent: n.parent,
            parent_error: n.parent_error,
            level: n.level,
            group: n.group,
            parent_group: n.parent_group,
            cone_axis,
            cone_cutoff,
        });
    }
    VirtualMesh {
        clusters,
        indices: indices_out,
        bounds: Sphere::merge(&all),
        levels: level + 1,
        source_triangles,
        groups: groups_made,
        stuck_groups: stuck,
    }
}

/// Positions on the border of a set of triangles: on an edge used by one of them only (shared
/// with another group, or an open edge of the mesh) or by more than two (non-manifold).
pub(super) fn border_positions(tris: &[[u32; 3]], weld: &[u32]) -> HashMap<u32, ()> {
    let mut uses: HashMap<(u32, u32), u32> = HashMap::with_capacity(tris.len() * 2);
    for t in tris {
        for k in 0..3 {
            let e = edge(weld[t[k] as usize], weld[t[(k + 1) % 3] as usize]);
            *uses.entry(e).or_default() += 1;
        }
    }
    let mut border = HashMap::new();
    for ((a, b), n) in uses {
        if n != 2 {
            border.insert(a, ());
            border.insert(b, ());
        }
    }
    border
}

/// Group the clusters `current` (indices into `nodes`) into sets of about `GROUP_SIZE`
/// neighbours sharing the most border edges. Returns positions into `current`.
fn group(current: &[usize], nodes: &[Node], positions: &[Vec3], weld: &[u32]) -> Vec<Vec<usize>> {
    let n = current.len();
    // Shared edges between clusters.
    let mut owner: HashMap<(u32, u32), u32> = HashMap::new();
    let mut adjacency: Vec<HashMap<u32, u32>> = vec![HashMap::new(); n];
    for (ci, &node) in current.iter().enumerate() {
        for t in &nodes[node].tris {
            for k in 0..3 {
                let e = edge(weld[t[k] as usize], weld[t[(k + 1) % 3] as usize]);
                match owner.get(&e) {
                    Some(&o) if o as usize != ci => {
                        *adjacency[ci].entry(o).or_default() += 1;
                        *adjacency[o as usize].entry(ci as u32).or_default() += 1;
                    }
                    Some(_) => {}
                    None => {
                        owner.insert(e, ci as u32);
                    }
                }
            }
        }
    }
    let centers: Vec<Vec3> = current
        .iter()
        .map(|&i| {
            let t = &nodes[i].tris;
            t.iter().map(|t| positions[t[0] as usize]).sum::<Vec3>() / t.len().max(1) as f32
        })
        .collect();
    let (lo, hi) = centers.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(l, h), c| (l.min(*c), h.max(*c)),
    );
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&c| morton(centers[c], lo, hi));

    let mut group_of = vec![u32::MAX; n];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for &seed in &order {
        if group_of[seed] != u32::MAX {
            continue;
        }
        let id = groups.len() as u32;
        let mut members = vec![seed];
        group_of[seed] = id;
        while members.len() < GROUP_SIZE {
            // The ungrouped neighbour sharing the most edges with the group.
            let mut shared: HashMap<u32, u32> = HashMap::new();
            for &m in &members {
                for (&o, &k) in &adjacency[m] {
                    if group_of[o as usize] == u32::MAX {
                        *shared.entry(o).or_default() += k;
                    }
                }
            }
            let Some((&best, _)) = shared.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            else {
                break;
            };
            group_of[best as usize] = id;
            members.push(best as usize);
        }
        groups.push(members);
    }
    // A lone cluster joins the neighbouring group it shares the most edges with (its border
    // would be locked all around otherwise and it could not simplify).
    for g in 0..groups.len() {
        if groups[g].len() != 1 {
            continue;
        }
        let c = groups[g][0];
        let best = adjacency[c]
            .iter()
            .filter(|(o, _)| {
                let og = group_of[**o as usize] as usize;
                og != g && groups[og].len() < 2 * GROUP_SIZE
            })
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(o, _)| group_of[*o as usize]);
        if let Some(to) = best {
            groups[g].clear();
            groups[to as usize].push(c);
            group_of[c] = to;
        }
    }
    groups.retain(|g| !g.is_empty());
    groups
}
