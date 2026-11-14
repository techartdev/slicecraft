//! Mesh repair and mesh tools: weld, degenerate removal, consistent normals,
//! hole filling, connected parts, and the largest flat face.
//!
//! Every routine works on an indexed copy of the triangle soup: vertices are
//! welded on a grid (`WELD` mm), so seams with last-bit differences join.

use crate::{GeomError, Mesh, Triangle, Vec3};
use std::collections::{HashMap, VecDeque};

/// Vertices closer than this are the same vertex, mm.
pub const WELD: f64 = 1e-5;
/// Largest hole (in boundary edges) `fill_holes` closes.
pub const MAX_HOLE_EDGES: usize = 10_000;

/// What a repair changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RepairReport {
    pub degenerate_removed: usize,
    pub flipped: usize,
    pub holes_filled: usize,
    pub triangles_added: usize,
    /// The whole mesh was inside out and was turned.
    pub turned_inside_out: bool,
}

/// An indexed mesh: welded vertices and triangles as index triples.
struct Indexed {
    verts: Vec<Vec3>,
    tris: Vec<[usize; 3]>,
}

fn key(v: Vec3) -> (i64, i64, i64) {
    ((v.x / WELD).round() as i64, (v.y / WELD).round() as i64, (v.z / WELD).round() as i64)
}

fn index(mesh: &Mesh) -> Indexed {
    let mut map: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut verts = Vec::new();
    let mut tris = Vec::with_capacity(mesh.len());
    for t in mesh.triangles() {
        let mut ids = [0usize; 3];
        for (slot, v) in ids.iter_mut().zip(t.vertices()) {
            *slot = *map.entry(key(v)).or_insert_with(|| {
                verts.push(v);
                verts.len() - 1
            });
        }
        tris.push(ids);
    }
    Indexed { verts, tris }
}

impl Indexed {
    fn vert(&self, i: usize) -> Vec3 {
        self.verts.get(i).copied().unwrap_or(Vec3::ZERO)
    }

    fn mesh(&self) -> Result<Mesh, GeomError> {
        Mesh::new(
            self.tris
                .iter()
                .map(|[a, b, c]| Triangle { a: self.vert(*a), b: self.vert(*b), c: self.vert(*c) })
                .collect(),
        )
    }

    /// Directed edge → triangles using it.
    fn edges(&self) -> HashMap<(usize, usize), Vec<usize>> {
        let mut m: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (ti, t) in self.tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                m.entry((a.min(b), a.max(b))).or_default().push(ti);
            }
        }
        m
    }

    fn signed_volume(&self) -> f64 {
        self.tris
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (self.vert(*a), self.vert(*b), self.vert(*c));
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }
}

/// Weld, drop degenerate faces, make the winding consistent and outward, and
/// close holes. Returns the repaired mesh and what changed.
pub fn repair(mesh: &Mesh, fill: bool) -> Result<(Mesh, RepairReport), GeomError> {
    let mut report = RepairReport::default();
    let mut m = index(mesh);
    // degenerate: repeated vertices or no area
    let before = m.tris.len();
    let verts = m.verts.clone();
    let vert = |i: usize| verts.get(i).copied().unwrap_or(Vec3::ZERO);
    m.tris.retain(|t| {
        t[0] != t[1]
            && t[1] != t[2]
            && t[0] != t[2]
            && (vert(t[1]) - vert(t[0])).cross(vert(t[2]) - vert(t[0])).length() > 1e-12
    });
    report.degenerate_removed = before - m.tris.len();
    let (flipped, turned) = orient(&mut m);
    report.flipped = flipped;
    report.turned_inside_out = turned;
    if fill {
        let (holes, added) = fill_holes(&mut m);
        report.holes_filled = holes;
        report.triangles_added = added;
    }
    if m.signed_volume() < 0.0 {
        for t in &mut m.tris {
            t.swap(1, 2);
        }
        report.turned_inside_out = true;
    }
    Ok((m.mesh()?, report))
}

/// Flip faces so neighbours agree: across a shared edge the two faces must
/// run the edge in opposite directions. Each connected part is walked breadth
/// first from its first face, then turned as a whole if its volume came out
/// negative (so the starting face may itself be the wrong one). Returns how
/// many faces end up different from how they came.
fn orient(m: &mut Indexed) -> (usize, bool) {
    let original = m.tris.clone();
    let edges = m.edges();
    let n = m.tris.len();
    let mut seen = vec![false; n];
    let mut turned = false;
    for start in 0..n {
        if seen.get(start).copied().unwrap_or(true) {
            continue;
        }
        if let Some(s) = seen.get_mut(start) {
            *s = true;
        }
        let mut part = vec![start];
        let mut queue = VecDeque::from([start]);
        while let Some(t) = queue.pop_front() {
            let Some(&tri) = m.tris.get(t) else { continue };
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                let Some(users) = edges.get(&(a.min(b), a.max(b))) else { continue };
                // only manifold edges carry a reliable orientation
                if users.len() != 2 {
                    continue;
                }
                for &u in users {
                    if u == t || seen.get(u).copied().unwrap_or(true) {
                        continue;
                    }
                    let Some(other) = m.tris.get_mut(u) else { continue };
                    // same direction a→b in the neighbour means it is flipped
                    if (0..3).any(|j| other[j] == a && other[(j + 1) % 3] == b) {
                        other.swap(1, 2);
                    }
                    if let Some(s) = seen.get_mut(u) {
                        *s = true;
                    }
                    part.push(u);
                    queue.push_back(u);
                }
            }
        }
        // a closed part must enclose positive volume
        let volume: f64 = part
            .iter()
            .filter_map(|&t| m.tris.get(t))
            .map(|[a, b, c]| {
                let (a, b, c) = (m.vert(*a), m.vert(*b), m.vert(*c));
                a.dot(b.cross(c))
            })
            .sum();
        if volume < 0.0 {
            turned = true;
            for &t in &part {
                if let Some(tri) = m.tris.get_mut(t) {
                    tri.swap(1, 2);
                }
            }
        }
    }
    (m.tris.iter().zip(&original).filter(|(now, was)| now != was).count(), turned)
}

/// Close each boundary loop with a fan from its centre. Returns the number
/// of holes closed and triangles added.
fn fill_holes(m: &mut Indexed) -> (usize, usize) {
    // boundary edges, in the direction their face runs them
    let mut count: HashMap<(usize, usize), usize> = HashMap::new();
    for t in &m.tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    // a hole's loop runs opposite to its faces' boundary edges
    let mut next: HashMap<usize, usize> = HashMap::new();
    for t in &m.tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if count.get(&(a.min(b), a.max(b))) == Some(&1) {
                next.insert(b, a);
            }
        }
    }
    let (mut holes, mut added) = (0, 0);
    while let Some((&start, _)) = next.iter().next() {
        let mut ring = vec![start];
        let mut at = start;
        let mut closed = false;
        while let Some(n) = next.remove(&at) {
            if n == start {
                closed = true;
                break;
            }
            ring.push(n);
            at = n;
            if ring.len() > MAX_HOLE_EDGES {
                break;
            }
        }
        if !closed || ring.len() < 3 {
            continue;
        }
        let centre =
            ring.iter().fold(Vec3::ZERO, |acc, i| acc + m.vert(*i)).scale(1.0 / ring.len() as f64);
        m.verts.push(centre);
        let c = m.verts.len() - 1;
        for k in 0..ring.len() {
            let (Some(&a), Some(&b)) = (ring.get(k), ring.get((k + 1) % ring.len())) else {
                continue;
            };
            m.tris.push([a, b, c]);
            added += 1;
        }
        holes += 1;
    }
    (holes, added)
}

/// The mesh's connected parts (faces sharing an edge belong together), largest first.
pub fn parts(mesh: &Mesh) -> Result<Vec<Mesh>, GeomError> {
    let m = index(mesh);
    let n = m.tris.len();
    // union-find over faces joined by shared edges
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while let Some(&up) = p.get(x) {
            if up == x {
                break;
            }
            let grand = p.get(up).copied().unwrap_or(up);
            if let Some(slot) = p.get_mut(x) {
                *slot = grand;
            }
            x = up;
        }
        x
    }
    for users in m.edges().values() {
        if let Some((&first, rest)) = users.split_first() {
            for &u in rest {
                let (a, b) = (find(&mut parent, first), find(&mut parent, u));
                if a != b
                    && let Some(slot) = parent.get_mut(a)
                {
                    *slot = b;
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<Triangle>> = HashMap::new();
    for (ti, t) in m.tris.iter().enumerate() {
        let root = find(&mut parent, ti);
        groups.entry(root).or_default().push(Triangle {
            a: m.vert(t[0]),
            b: m.vert(t[1]),
            c: m.vert(t[2]),
        });
    }
    let mut out: Vec<Mesh> = groups.into_values().map(Mesh::new).collect::<Result<_, _>>()?;
    out.sort_by_key(|p| std::cmp::Reverse(p.len()));
    Ok(out)
}

/// The outward normal of the mesh's largest flat area: triangles are grouped
/// by normal direction (within about 1°) and their areas summed.
pub fn largest_flat_face(mesh: &Mesh) -> Option<Vec3> {
    let mut groups: HashMap<(i64, i64, i64), (f64, Vec3)> = HashMap::new();
    for t in mesh.triangles() {
        let cross = (t.b - t.a).cross(t.c - t.a);
        let area = cross.length() / 2.0;
        let Some(n) = cross.normalized() else { continue };
        let q = |x: f64| (x * 60.0).round() as i64;
        let e = groups.entry((q(n.x), q(n.y), q(n.z))).or_insert((0.0, Vec3::ZERO));
        e.0 += area;
        e.1 = e.1 + n.scale(area);
    }
    groups.into_values().max_by(|a, b| a.0.total_cmp(&b.0)).and_then(|(_, n)| n.normalized())
}

/// Rotate `mesh` (about the origin) so that direction `from` points straight down.
pub fn rotate_to_down(mesh: &Mesh, from: Vec3) -> Result<Mesh, GeomError> {
    let Some(f) = from.normalized() else { return Ok(mesh.clone()) };
    let down = Vec3::new(0.0, 0.0, -1.0);
    let axis = f.cross(down);
    let s = axis.length();
    let c = f.dot(down);
    let rot = |v: Vec3| -> Vec3 {
        if s < 1e-12 {
            // already down, or straight up: turn half way round X
            return if c > 0.0 { v } else { Vec3::new(v.x, -v.y, -v.z) };
        }
        let k = axis.scale(1.0 / s);
        // Rodrigues: v cosθ + (k × v) sinθ + k (k·v)(1 − cosθ)
        v.scale(c) + k.cross(v).scale(s) + k.scale(k.dot(v) * (1.0 - c))
    };
    mesh.map_vertices(rot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Triangle {
        Triangle {
            a: Vec3::new(a[0], a[1], a[2]),
            b: Vec3::new(b[0], b[1], b[2]),
            c: Vec3::new(c[0], c[1], c[2]),
        }
    }

    /// A closed tetrahedron, outward faces.
    fn tetra(o: f64) -> Vec<Triangle> {
        let (a, b, c, d) = ([o, 0.0, 0.0], [o + 1.0, 0.0, 0.0], [o, 1.0, 0.0], [o, 0.0, 1.0]);
        vec![tri(a, c, b), tri(a, b, d), tri(a, d, c), tri(b, c, d)]
    }

    #[test]
    fn repair_fixes_flipped_faces_holes_and_degenerates() {
        let mut t = tetra(0.0);
        // flip one face, drop another (a hole), add a degenerate one
        if let Some(f) = t.get_mut(1) {
            std::mem::swap(&mut f.b, &mut f.c);
        }
        t.remove(3);
        t.push(tri([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        let mesh = Mesh::new(t).unwrap_or_default();
        let (fixed, report) = repair(&mesh, true).unwrap_or_default();
        assert_eq!(report.degenerate_removed, 1);
        assert_eq!(report.flipped, 1);
        assert_eq!(report.holes_filled, 1);
        let r = fixed.report();
        assert!(r.is_watertight(), "{r:?}");
        assert!(r.volume > 0.0);
        // an inside-out mesh is turned
        let inverted: Vec<Triangle> =
            tetra(0.0).into_iter().map(|t| Triangle { a: t.a, b: t.c, c: t.b }).collect();
        let (fixed, report) =
            repair(&Mesh::new(inverted).unwrap_or_default(), false).unwrap_or_default();
        assert!(report.turned_inside_out && fixed.report().volume > 0.0);
    }

    #[test]
    fn parts_and_flat_faces() {
        let mut two = tetra(0.0);
        two.extend(tetra(5.0));
        let mesh = Mesh::new(two).unwrap_or_default();
        let p = parts(&mesh).unwrap_or_default();
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|m| m.len() == 4));
        // the tetrahedron's largest face is the slanted one: (1,1,1)/√3
        let n = largest_flat_face(&Mesh::new(tetra(0.0)).unwrap_or_default());
        assert!(n.is_some_and(|n| (n.x - n.z).abs() < 1e-9 && n.x > 0.5), "{n:?}");
        // turning it down puts that face on the bed
        let down =
            rotate_to_down(&Mesh::new(tetra(0.0)).unwrap_or_default(), n.unwrap_or(Vec3::ZERO))
                .unwrap_or_default();
        let after = largest_flat_face(&down);
        assert!(after.is_some_and(|n| (n.z + 1.0).abs() < 1e-9), "{after:?}");
    }
}
