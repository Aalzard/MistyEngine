//! Mesh Builder operations. Each takes the elements it works on and returns the elements to
//! select afterwards. Winding stays consistent: every new face uses each shared edge in the
//! direction opposite to its neighbour.

use std::collections::{HashMap, HashSet};

use glam::{Mat4, Vec3};

use super::{EditableMesh, Face, edge_key};

/// Selected elements of an Editable Mesh (vertex indices, undirected edges, face indices).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub verts: Vec<u32>,
    pub edges: Vec<[u32; 2]>,
    pub faces: Vec<usize>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.verts.is_empty() && self.edges.is_empty() && self.faces.is_empty()
    }

    pub fn faces(faces: Vec<usize>) -> Self {
        Self {
            faces,
            ..Default::default()
        }
    }

    pub fn edges(edges: Vec<[u32; 2]>) -> Self {
        Self {
            edges,
            ..Default::default()
        }
    }

    pub fn verts(verts: Vec<u32>) -> Self {
        Self {
            verts,
            ..Default::default()
        }
    }

    /// Every vertex the selection touches (sorted, unique).
    pub fn all_verts(&self, m: &EditableMesh) -> Vec<u32> {
        let mut out = self.verts.clone();
        out.extend(self.edges.iter().flatten());
        for f in &self.faces {
            if let Some(face) = m.faces.get(*f) {
                out.extend(&face.verts);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Drop elements that no longer exist (after undo or a topology change).
    pub fn retain_valid(&mut self, m: &EditableMesh) {
        let n = m.positions.len() as u32;
        self.verts.retain(|v| *v < n);
        let edges: HashSet<[u32; 2]> = m.edges().into_iter().collect();
        self.edges.retain(|e| edges.contains(e));
        self.faces.retain(|f| *f < m.faces.len());
    }
}

/// Faces touching any of `verts` (all of their corners must be in `verts` when `whole`).
pub fn faces_with_verts(m: &EditableMesh, verts: &[u32], whole: bool) -> Vec<usize> {
    let set: HashSet<u32> = verts.iter().copied().collect();
    (0..m.faces.len())
        .filter(|f| {
            let v = &m.faces[*f].verts;
            if whole {
                v.iter().all(|x| set.contains(x))
            } else {
                v.iter().any(|x| set.contains(x))
            }
        })
        .collect()
}

/// Move, rotate or scale vertices by `matrix` (local space).
pub fn transform_vertices(m: &mut EditableMesh, verts: &[u32], matrix: &Mat4) {
    for v in verts {
        if let Some(p) = m.positions.get_mut(*v as usize) {
            *p = matrix.transform_point3(*p);
        }
    }
}

/// Face containing the directed edge a → b.
fn face_with_directed(m: &EditableMesh, a: u32, b: u32) -> Option<usize> {
    m.faces.iter().position(|f| f.edges().any(|e| e == (a, b)))
}

fn push_vertex(m: &mut EditableMesh, p: Vec3) -> u32 {
    m.positions.push(p);
    m.positions.len() as u32 - 1
}

/// Extrude faces as one region along their averaged normals; returns the moved faces.
pub fn extrude_faces(m: &mut EditableMesh, faces: &[usize], distance: f32) -> Vec<usize> {
    let sel: Vec<usize> = unique_faces(m, faces);
    if sel.is_empty() {
        return Vec::new();
    }
    let directed: HashSet<(u32, u32)> = sel.iter().flat_map(|f| m.faces[*f].edges()).collect();
    let mut normal_sum: HashMap<u32, Vec3> = HashMap::new();
    for f in &sel {
        let n = m.face_normal(*f);
        for v in &m.faces[*f].verts {
            *normal_sum.entry(*v).or_insert(Vec3::ZERO) += n;
        }
    }
    // Keep flat regions flat: scale the averaged normal so its projection on each face
    // normal is the full distance.
    let mut moved: HashMap<u32, u32> = HashMap::new();
    let mut verts: Vec<u32> = normal_sum.keys().copied().collect();
    verts.sort_unstable();
    for v in verts {
        let sum = normal_sum[&v];
        let dir = sum.normalize_or(Vec3::Y);
        let min_dot = sel
            .iter()
            .filter(|f| m.faces[**f].verts.contains(&v))
            .map(|f| m.face_normal(*f).dot(dir))
            .fold(1.0f32, f32::min)
            .max(0.25);
        let p = m.position(v) + dir * (distance / min_dot);
        let nv = push_vertex(m, p);
        moved.insert(v, nv);
    }
    let mut sides = Vec::new();
    for f in &sel {
        let face = m.faces[*f].clone();
        for (a, b) in face.edges() {
            if directed.contains(&(b, a)) {
                continue; // interior edge of the region
            }
            sides.push(Face {
                smoothing: 0,
                ..face.with_verts(vec![a, b, moved[&b], moved[&a]])
            });
        }
    }
    for f in &sel {
        for v in &mut m.faces[*f].verts {
            *v = moved[v];
        }
    }
    m.faces.extend(sides);
    sel
}

/// Extrude open (border) edges outward in their face's plane; returns the new outer edges.
pub fn extrude_edges(m: &mut EditableMesh, edges: &[[u32; 2]], distance: f32) -> Vec<[u32; 2]> {
    let ef = m.edge_faces();
    // Border edges, directed as their face uses them.
    let mut border = Vec::new();
    for e in edges {
        if ef.get(e).is_some_and(|f| f.len() == 1) {
            let f = ef[e][0];
            let (a, b) = if m.faces[f].edges().any(|d| d == (e[0], e[1])) {
                (e[0], e[1])
            } else {
                (e[1], e[0])
            };
            border.push((f, a, b));
        }
    }
    let mut dirs: HashMap<u32, Vec3> = HashMap::new();
    for (f, a, b) in &border {
        let n = m.face_normal(*f);
        let out = (m.position(*b) - m.position(*a))
            .cross(n)
            .normalize_or_zero();
        *dirs.entry(*a).or_insert(Vec3::ZERO) += out;
        *dirs.entry(*b).or_insert(Vec3::ZERO) += out;
    }
    let mut moved: HashMap<u32, u32> = HashMap::new();
    let mut keys: Vec<u32> = dirs.keys().copied().collect();
    keys.sort_unstable();
    for v in keys {
        let p = m.position(v) + dirs[&v].normalize_or_zero() * distance;
        let nv = push_vertex(m, p);
        moved.insert(v, nv);
    }
    let mut out = Vec::new();
    for (f, a, b) in border {
        let (a2, b2) = (moved[&a], moved[&b]);
        let face = m.faces[f].with_verts(vec![b, a, a2, b2]);
        m.faces.push(face);
        out.push(edge_key(a2, b2));
    }
    out
}

/// Inset each face: a smaller copy inside it, joined by a ring of quads. Returns the inner
/// faces (same indices as the faces given).
pub fn inset_faces(m: &mut EditableMesh, faces: &[usize], amount: f32) -> Vec<usize> {
    let sel = unique_faces(m, faces);
    let mut ring = Vec::new();
    for &f in &sel {
        let face = m.faces[f].clone();
        let n = m.face_normal(f);
        let pts: Vec<Vec3> = face.verts.iter().map(|v| m.position(*v)).collect();
        let count = pts.len();
        // Never past the centre: limit to most of the smallest centre-to-edge distance.
        let center = pts.iter().sum::<Vec3>() / count as f32;
        let mut limit = f32::MAX;
        for i in 0..count {
            let (a, b) = (pts[i], pts[(i + 1) % count]);
            let e = (b - a).normalize_or_zero();
            let to_c = center - a;
            limit = limit.min((to_c - e * to_c.dot(e)).length());
        }
        let amount = amount.min(limit * 0.95);
        let mut inner = Vec::with_capacity(count);
        for i in 0..count {
            let (p, v, q) = (pts[(i + count - 1) % count], pts[i], pts[(i + 1) % count]);
            let in1 = n.cross((v - p).normalize_or_zero());
            let in2 = n.cross((q - v).normalize_or_zero());
            let bis = (in1 + in2).normalize_or(in1);
            let len = amount / bis.dot(in1).max(0.2);
            inner.push(push_vertex(m, v + bis * len));
        }
        for i in 0..count {
            let j = (i + 1) % count;
            ring.push(face.with_verts(vec![face.verts[i], face.verts[j], inner[j], inner[i]]));
        }
        m.faces[f].verts = inner;
    }
    m.faces.extend(ring);
    sel
}

/// Chamfer edges shared by two faces: each becomes a strip of width ~`amount` (a quad, plus a
/// corner triangle where more than three faces meet). Edges whose faces changed under an
/// earlier edge are skipped. Returns the new strip faces.
pub fn bevel_edges(m: &mut EditableMesh, edges: &[[u32; 2]], amount: f32) -> Vec<usize> {
    let mut strips = Vec::new();
    for e in edges {
        let (a, b) = (e[0], e[1]);
        let (Some(f1), Some(f2)) = (face_with_directed(m, a, b), face_with_directed(m, b, a))
        else {
            continue;
        };
        let around = |m: &EditableMesh, f: usize, v: u32| {
            let verts = &m.faces[f].verts;
            let n = verts.len();
            let i = verts.iter().position(|x| *x == v).unwrap_or(0);
            (verts[(i + n - 1) % n], verts[(i + 1) % n])
        };
        // f1: x1 → a → b → y1; f2: y2 → b → a → x2.
        let (x1, _) = around(m, f1, a);
        let (_, y1) = around(m, f1, b);
        let (_, x2) = around(m, f2, a);
        let (y2, _) = around(m, f2, b);
        let toward = |m: &mut EditableMesh, from: u32, to: u32| {
            let (p, q) = (m.position(from), m.position(to));
            let d = q - p;
            let len = d.length();
            push_vertex(m, p + d / len.max(1e-6) * amount.min(len * 0.45))
        };
        let a1 = toward(m, a, x1);
        let b1 = toward(m, b, y1);
        let a2 = toward(m, a, x2);
        let b2 = toward(m, b, y2);
        // Neighbours across the side edges, found before rewiring f1 and f2.
        let g1 = face_with_directed(m, a, x1);
        let g2 = face_with_directed(m, x2, a);
        let h1 = face_with_directed(m, y1, b);
        let h2 = face_with_directed(m, b, y2);
        replace_in(&mut m.faces[f1], &[(a, a1), (b, b1)]);
        replace_in(&mut m.faces[f2], &[(a, a2), (b, b2)]);
        let strip = m.faces[f1].with_verts(vec![b1, a1, a2, b2]);
        let strip = Face {
            smoothing: 0,
            ..strip
        };
        m.faces.push(strip);
        strips.push(m.faces.len() - 1);
        // Corner at a.
        match (g1, g2) {
            (Some(g), Some(h)) if g == h => splice(&mut m.faces[g], a, &[a2, a1]),
            (g, h) => {
                if let Some(g) = g {
                    splice(&mut m.faces[g], a, &[a, a1]);
                }
                if let Some(h) = h {
                    splice(&mut m.faces[h], a, &[a2, a]);
                }
                if g.is_some() && h.is_some() {
                    let tri = m.faces[f1].with_verts(vec![a1, a, a2]);
                    m.faces.push(tri);
                }
            }
        }
        // Corner at b.
        match (h1, h2) {
            (Some(g), Some(h)) if g == h => splice(&mut m.faces[g], b, &[b1, b2]),
            (g, h) => {
                if let Some(g) = g {
                    splice(&mut m.faces[g], b, &[b1, b]);
                }
                if let Some(h) = h {
                    splice(&mut m.faces[h], b, &[b, b2]);
                }
                if g.is_some() && h.is_some() {
                    let tri = m.faces[f1].with_verts(vec![b, b1, b2]);
                    m.faces.push(tri);
                }
            }
        }
    }
    let count = m.faces.len();
    let map = m.compact();
    let _ = map;
    strips.retain(|f| *f < count.min(m.faces.len()));
    strips
}

fn replace_in(face: &mut Face, pairs: &[(u32, u32)]) {
    for v in &mut face.verts {
        if let Some((_, to)) = pairs.iter().find(|(from, _)| from == v) {
            *v = *to;
        }
    }
}

/// Replace corner `v` of `face` by `with` (in order).
fn splice(face: &mut Face, v: u32, with: &[u32]) {
    if let Some(i) = face.verts.iter().position(|x| *x == v) {
        face.verts.splice(i..=i, with.iter().copied());
    }
}

/// Split each face into quads around its centre (edge midpoints are shared with neighbours,
/// which get the midpoint as an extra corner). Returns the new faces.
pub fn subdivide_faces(m: &mut EditableMesh, faces: &[usize]) -> Vec<usize> {
    let sel = unique_faces(m, faces);
    let selected: HashSet<usize> = sel.iter().copied().collect();
    let mut mids: HashMap<[u32; 2], u32> = HashMap::new();
    let mut out = Vec::new();
    let mut new_faces = Vec::new();
    for &f in &sel {
        let face = m.faces[f].clone();
        let n = face.verts.len();
        let center = push_vertex(m, m.face_center(f));
        let mut mid = Vec::with_capacity(n);
        for (a, b) in face.edges() {
            let key = edge_key(a, b);
            let v = match mids.get(&key) {
                Some(v) => *v,
                None => {
                    let v = push_vertex(m, (m.position(a) + m.position(b)) * 0.5);
                    mids.insert(key, v);
                    v
                }
            };
            mid.push(v);
        }
        for i in 0..n {
            let quad = vec![face.verts[i], mid[i], center, mid[(i + n - 1) % n]];
            if i == 0 {
                m.faces[f].verts = quad;
                out.push(f);
            } else {
                new_faces.push(face.with_verts(quad));
            }
        }
    }
    // Unselected neighbours of split edges keep the midpoint as a corner (no cracks).
    insert_midpoints(m, &mids, &selected);
    let first = m.faces.len();
    m.faces.extend(new_faces);
    out.extend(first..m.faces.len());
    out
}

/// Put split points into faces (other than `skip`) that use a split edge.
fn insert_midpoints(m: &mut EditableMesh, mids: &HashMap<[u32; 2], u32>, skip: &HashSet<usize>) {
    for (i, face) in m.faces.iter_mut().enumerate() {
        if skip.contains(&i) {
            continue;
        }
        let mut verts = Vec::with_capacity(face.verts.len() + 2);
        let n = face.verts.len();
        for k in 0..n {
            let (a, b) = (face.verts[k], face.verts[(k + 1) % n]);
            verts.push(a);
            if let Some(mid) = mids.get(&edge_key(a, b)) {
                verts.push(*mid);
            }
        }
        face.verts = verts;
    }
}

/// Collapse vertices to their centre; returns the merged vertex (after compaction).
pub fn merge_vertices(m: &mut EditableMesh, verts: &[u32]) -> Option<u32> {
    let mut verts: Vec<u32> = verts.to_vec();
    verts.sort_unstable();
    verts.dedup();
    let &keep = verts.first()?;
    let center = verts.iter().map(|v| m.position(*v)).sum::<Vec3>() / verts.len() as f32;
    m.positions[keep as usize] = center;
    let set: HashSet<u32> = verts.iter().copied().collect();
    for f in &mut m.faces {
        for v in &mut f.verts {
            if set.contains(v) {
                *v = keep;
            }
        }
    }
    let map = m.compact();
    map.get(keep as usize).copied().filter(|v| *v != u32::MAX)
}

/// Merge vertices (of `verts`, or all when empty) closer than `distance`; returns how many
/// vertices were removed.
pub fn weld_vertices(m: &mut EditableMesh, verts: &[u32], distance: f32) -> usize {
    let candidates: Vec<u32> = if verts.is_empty() {
        (0..m.positions.len() as u32).collect()
    } else {
        let mut v = verts.to_vec();
        v.sort_unstable();
        v.dedup();
        v
    };
    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut reps: Vec<u32> = Vec::new();
    for &v in &candidates {
        let p = m.position(v);
        match reps
            .iter()
            .find(|r| m.position(**r).distance(p) <= distance)
        {
            Some(r) => {
                remap.insert(v, *r);
            }
            None => reps.push(v),
        }
    }
    if remap.is_empty() {
        return 0;
    }
    for f in &mut m.faces {
        for v in &mut f.verts {
            if let Some(r) = remap.get(v) {
                *v = *r;
            }
        }
    }
    let before = m.positions.len();
    m.compact();
    before - m.positions.len()
}

/// Remove faces (and vertices nobody uses any more).
pub fn delete_faces(m: &mut EditableMesh, faces: &[usize]) {
    let set: HashSet<usize> = faces.iter().copied().collect();
    let mut i = 0;
    m.faces.retain(|_| {
        let keep = !set.contains(&i);
        i += 1;
        keep
    });
    m.compact();
}

/// Reverse the winding (and so the normal) of faces.
pub fn flip_faces(m: &mut EditableMesh, faces: &[usize]) {
    for f in unique_faces(m, faces) {
        m.faces[f].verts.reverse();
    }
}

/// Join two border edges with a new face; returns it.
pub fn bridge_edges(m: &mut EditableMesh, e1: [u32; 2], e2: [u32; 2]) -> Option<usize> {
    let ef = m.edge_faces();
    let border = |e: &[u32; 2]| -> Option<(usize, u32, u32)> {
        let faces = ef.get(e)?;
        if faces.len() != 1 {
            return None;
        }
        let f = faces[0];
        Some(if m.faces[f].edges().any(|d| d == (e[0], e[1])) {
            (f, e[0], e[1])
        } else {
            (f, e[1], e[0])
        })
    };
    let (f1, a, b) = border(&e1)?;
    let (_, c, d) = border(&e2)?;
    let mut verts = vec![b, a, d, c];
    verts.dedup();
    if verts.first() == verts.last() {
        verts.pop();
    }
    let mut unique = verts.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() < 3 || unique.len() != verts.len() {
        return None;
    }
    let face = Face {
        smoothing: 0,
        ..m.faces[f1].with_verts(verts)
    };
    m.faces.push(face);
    Some(m.faces.len() - 1)
}

/// Edge loop cut: walk the ring of quads across `edge` and split each one, `t` of the way
/// along the ring's edges (measured from `edge[0]`'s side). Returns the new loop edges.
pub fn loop_cut(m: &mut EditableMesh, edge: [u32; 2], t: f32) -> Vec<[u32; 2]> {
    let ef = m.edge_faces();
    let key = edge_key(edge[0], edge[1]);
    let Some(start_faces) = ef.get(&key).cloned() else {
        return Vec::new();
    };
    // (face, index of the crossed edge in the face, its start vertex)
    let mut ring: Vec<(usize, usize, u32)> = Vec::new();
    let mut visited: HashSet<usize> = HashSet::new();
    for &f0 in &start_faces {
        if visited.contains(&f0) {
            continue;
        }
        let (mut f, mut e, mut start) = (f0, key, edge[0]);
        loop {
            if visited.contains(&f) || m.faces[f].verts.len() != 4 {
                break;
            }
            let v = &m.faces[f].verts;
            let Some(i) = (0..4).find(|i| edge_key(v[*i], v[(i + 1) % 4]) == e) else {
                break;
            };
            visited.insert(f);
            ring.push((f, i, start));
            // The opposite edge's start is the corner next to this edge's start.
            let next_start = if v[i] == start {
                v[(i + 3) % 4]
            } else {
                v[(i + 2) % 4]
            };
            let opposite = edge_key(v[(i + 2) % 4], v[(i + 3) % 4]);
            let Some(next) = ef
                .get(&opposite)
                .and_then(|fs| fs.iter().copied().find(|x| *x != f))
            else {
                break;
            };
            (f, e, start) = (next, opposite, next_start);
        }
    }
    if ring.is_empty() {
        return Vec::new();
    }
    let t = t.clamp(0.01, 0.99);
    let mut cuts: HashMap<[u32; 2], u32> = HashMap::new();
    let mut cut = |m: &mut EditableMesh, a: u32, b: u32, from: u32| -> u32 {
        let k = edge_key(a, b);
        if let Some(v) = cuts.get(&k) {
            return *v;
        }
        let (s, e) = if from == a { (a, b) } else { (b, a) };
        let v = push_vertex(m, m.position(s).lerp(m.position(e), t));
        cuts.insert(k, v);
        v
    };
    let mut new_faces = Vec::new();
    let mut out = Vec::new();
    let in_ring: HashSet<usize> = ring.iter().map(|r| r.0).collect();
    for &(f, i, start) in &ring {
        let v = m.faces[f].verts.clone();
        let (v0, v1, v2, v3) = (v[i], v[(i + 1) % 4], v[(i + 2) % 4], v[(i + 3) % 4]);
        let opp_start = if v0 == start { v3 } else { v2 };
        let m1 = cut(m, v0, v1, start);
        let m2 = cut(m, v2, v3, opp_start);
        let face = m.faces[f].clone();
        m.faces[f].verts = vec![v0, m1, m2, v3];
        new_faces.push(face.with_verts(vec![m1, v1, v2, m2]));
        out.push(edge_key(m1, m2));
    }
    insert_midpoints(m, &cuts, &in_ring);
    m.faces.extend(new_faces);
    out
}

/// Connect: one edge → a loop cut through its middle; several → in every face with exactly
/// two of them, split both at their midpoints and join the midpoints. Returns new edges.
pub fn connect_edges(m: &mut EditableMesh, edges: &[[u32; 2]]) -> Vec<[u32; 2]> {
    if edges.len() == 1 {
        return loop_cut(m, edges[0], 0.5);
    }
    let set: HashSet<[u32; 2]> = edges.iter().copied().collect();
    let targets: Vec<usize> = (0..m.faces.len())
        .filter(|f| {
            m.faces[*f]
                .edges()
                .filter(|(a, b)| set.contains(&edge_key(*a, *b)))
                .count()
                == 2
        })
        .collect();
    if targets.is_empty() {
        return Vec::new();
    }
    let mut mids: HashMap<[u32; 2], u32> = HashMap::new();
    for f in &targets {
        for (a, b) in m.faces[*f].clone().edges() {
            let k = edge_key(a, b);
            if set.contains(&k) && !mids.contains_key(&k) {
                let v = push_vertex(m, (m.position(a) + m.position(b)) * 0.5);
                mids.insert(k, v);
            }
        }
    }
    let target_set: HashSet<usize> = targets.iter().copied().collect();
    let mut out = Vec::new();
    let mut extra = Vec::new();
    for &f in &targets {
        let face = m.faces[f].clone();
        let mut list = Vec::new();
        let mut marks = Vec::new();
        for (a, b) in face.edges() {
            list.push(a);
            if set.contains(&edge_key(a, b)) {
                marks.push(list.len());
                list.push(mids[&edge_key(a, b)]);
            }
        }
        let (p, q) = (marks[0], marks[1]);
        let first: Vec<u32> = list[p..=q].to_vec();
        let mut second: Vec<u32> = list[q..].to_vec();
        second.extend_from_slice(&list[..=p]);
        out.push(edge_key(list[p], list[q]));
        m.faces[f].verts = first;
        extra.push(face.with_verts(second));
    }
    insert_midpoints(m, &mids, &target_set);
    m.faces.extend(extra);
    out
}

/// Give faces their own vertices, so they come loose from the rest of the mesh.
pub fn detach_faces(m: &mut EditableMesh, faces: &[usize]) -> Vec<usize> {
    let sel = unique_faces(m, faces);
    let mut copies: HashMap<u32, u32> = HashMap::new();
    for &f in &sel {
        let verts = m.faces[f].verts.clone();
        let mut new = Vec::with_capacity(verts.len());
        for v in verts {
            let nv = match copies.get(&v) {
                Some(n) => *n,
                None => {
                    let n = push_vertex(m, m.position(v));
                    copies.insert(v, n);
                    n
                }
            };
            new.push(nv);
        }
        m.faces[f].verts = new;
    }
    m.compact();
    sel
}

/// Materials element of faces.
pub fn set_material(m: &mut EditableMesh, faces: &[usize], material: u32) {
    for f in unique_faces(m, faces) {
        m.faces[f].material = material;
    }
}

/// Smoothing group of faces (0 = hard).
pub fn set_smoothing(m: &mut EditableMesh, faces: &[usize], group: u32) {
    for f in unique_faces(m, faces) {
        m.faces[f].smoothing = group;
    }
}

/// Move the vertices so the bounds centre is at the origin; returns the old centre (the object
/// moves by it to stay in place).
pub fn center_pivot(m: &mut EditableMesh) -> Vec3 {
    let (lo, hi) = m.bounds();
    let c = (lo + hi) * 0.5;
    for p in &mut m.positions {
        *p -= c;
    }
    c
}

fn unique_faces(m: &EditableMesh, faces: &[usize]) -> Vec<usize> {
    let mut v: Vec<usize> = faces
        .iter()
        .copied()
        .filter(|f| *f < m.faces.len())
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}
