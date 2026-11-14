//! Mesh operations: cut with a horizontal plane (capped), reduce the triangle
//! count, find self-intersections, and hollow.

use crate::repair::WELD;
use crate::{GeomError, Mesh, Point2, Triangle, Vec3};
use std::collections::HashMap;

/// Most triangles `self_intersections` checks; past it, the count is skipped.
pub const MAX_INTERSECTION_TRIANGLES: usize = 2_000_000;

// ------------------------------------------------------------ triangulation

fn area2(a: Point2, b: Point2, c: Point2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn ring_area(r: &[Point2]) -> f64 {
    let n = r.len();
    (0..n)
        .map(|i| {
            let (a, b) = (r[i], r[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

fn inside_tri(p: Point2, a: Point2, b: Point2, c: Point2) -> bool {
    let (d1, d2, d3) = (area2(a, b, p), area2(b, c, p), area2(c, a, p));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Triangles covering a polygon with holes: each hole is joined to the outline
/// by a bridge from its rightmost point to the nearest visible outline vertex,
/// then the single ring is ear-clipped. Rings may come either way round.
pub fn triangulate(outer: &[Point2], holes: &[Vec<Point2>]) -> Vec<[Point2; 3]> {
    let mut ring: Vec<Point2> = outer.to_vec();
    if ring_area(&ring) < 0.0 {
        ring.reverse();
    }
    // holes clockwise, rightmost first
    let mut hs: Vec<Vec<Point2>> = holes
        .iter()
        .filter(|h| h.len() >= 3)
        .map(|h| {
            let mut h = h.clone();
            if ring_area(&h) > 0.0 {
                h.reverse();
            }
            h
        })
        .collect();
    hs.sort_by(|a, b| {
        let mx = |h: &Vec<Point2>| h.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        mx(b).total_cmp(&mx(a))
    });
    for h in hs {
        let Some((hi, hp)) =
            h.iter().enumerate().max_by(|a, b| a.1.x.total_cmp(&b.1.x)).map(|(i, p)| (i, *p))
        else {
            continue;
        };
        // the nearest ring vertex to the right whose bridge crosses no edge
        let crosses = |a: Point2, b: Point2, ring: &[Point2]| {
            let n = ring.len();
            (0..n).any(|i| {
                let (c, d) = (ring[i], ring[(i + 1) % n]);
                if c == a || d == a || c == b || d == b {
                    return false;
                }
                let o = |p: Point2, q: Point2, r: Point2| area2(p, q, r);
                (o(a, b, c) > 0.0) != (o(a, b, d) > 0.0) && (o(c, d, a) > 0.0) != (o(c, d, b) > 0.0)
            })
        };
        let best = ring
            .iter()
            .enumerate()
            .filter(|(_, p)| !crosses(hp, **p, &ring))
            .min_by(|a, b| a.1.distance_sq(hp).total_cmp(&b.1.distance_sq(hp)))
            .map(|(i, _)| i);
        let Some(bi) = best else { continue };
        let mut joined = Vec::with_capacity(ring.len() + h.len() + 2);
        joined.extend_from_slice(&ring[..=bi]);
        for k in 0..=h.len() {
            joined.push(h[(hi + k) % h.len()]);
        }
        joined.push(ring[bi]);
        joined.extend_from_slice(&ring[bi + 1..]);
        ring = joined;
    }
    // ear clipping
    let mut idx: Vec<usize> = (0..ring.len()).collect();
    let mut out = Vec::with_capacity(ring.len());
    let mut guard = 0usize;
    while idx.len() > 3 && guard < ring.len() * ring.len() + 16 {
        guard += 1;
        let n = idx.len();
        let mut clipped = false;
        for i in 0..n {
            let (ia, ib, ic) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            let (a, b, c) = (ring[ia], ring[ib], ring[ic]);
            if area2(a, b, c) <= 1e-12 {
                continue;
            }
            let blocked = idx.iter().any(|&k| {
                k != ia
                    && k != ib
                    && k != ic
                    && ring[k] != a
                    && ring[k] != b
                    && ring[k] != c
                    && inside_tri(ring[k], a, b, c)
            });
            if blocked {
                continue;
            }
            out.push([a, b, c]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        let (a, b, c) = (ring[idx[0]], ring[idx[1]], ring[idx[2]]);
        if area2(a, b, c).abs() > 1e-12 {
            out.push([a, b, c]);
        }
    }
    out
}

// ------------------------------------------------------------ cut

/// Where the edge p–q crosses height `z`, computed from the lower end so the
/// two faces sharing the edge get the same bits.
fn crossing(p: Vec3, q: Vec3, z: f64) -> Vec3 {
    let (lo, hi) = if (p.z, p.x, p.y) <= (q.z, q.x, q.y) { (p, q) } else { (q, p) };
    let t = (z - lo.z) / (hi.z - lo.z);
    Vec3::new(lo.x + (hi.x - lo.x) * t, lo.y + (hi.y - lo.y) * t, z)
}

fn bits(p: Point2) -> (u64, u64) {
    (p.x.to_bits(), p.y.to_bits())
}

/// Chain cut segments into closed loops (segments that do not close are dropped).
fn loops(segments: &[(Point2, Point2)]) -> Vec<Vec<Point2>> {
    let mut adj: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segments.iter().enumerate() {
        adj.entry(bits(*a)).or_default().push(i);
        adj.entry(bits(*b)).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    let mut out = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (first, mut at) = segments[start];
        let mut ring = vec![first];
        let mut closed = false;
        for _ in 0..segments.len() {
            if bits(at) == bits(first) {
                closed = true;
                break;
            }
            ring.push(at);
            let next = adj.get(&bits(at)).and_then(|v| v.iter().copied().find(|&k| !used[k]));
            let Some(k) = next else { break };
            used[k] = true;
            let (a, b) = segments[k];
            at = if bits(a) == bits(at) { b } else { a };
        }
        if closed && ring.len() >= 3 {
            out.push(ring);
        }
    }
    out
}

fn ring_contains(r: &[Point2], p: Point2) -> bool {
    let n = r.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (r[i], r[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// Cut a closed mesh with the plane `z`: the part below and the part above,
/// each closed with a cap built from the cut itself (outlines and holes).
/// Either part may be empty.
pub fn cut(mesh: &Mesh, z: f64) -> Result<(Mesh, Mesh), GeomError> {
    let mut below = Vec::new();
    let mut above = Vec::new();
    let mut segments = Vec::new();
    for t in mesh.triangles() {
        let v = t.vertices();
        let side: Vec<bool> = v.iter().map(|p| p.z >= z).collect();
        let up = side.iter().filter(|s| **s).count();
        if up == 0 {
            below.push(*t);
            continue;
        }
        if up == 3 {
            above.push(*t);
            continue;
        }
        // split: the lone vertex and the two crossing points
        let lone_up = up == 1;
        let k = side.iter().position(|s| *s == lone_up).unwrap_or(0);
        let (a, b, c) = (v[k], v[(k + 1) % 3], v[(k + 2) % 3]);
        let (ab, ac) = (crossing(a, b, z), crossing(a, c, z));
        segments.push((Point2::new(ab.x, ab.y), Point2::new(ac.x, ac.y)));
        let lone = Triangle { a, b: ab, c: ac };
        let rest = [Triangle { a: ab, b, c }, Triangle { a: ab, b: c, c: ac }];
        if lone_up {
            above.push(lone);
            below.extend(rest);
        } else {
            below.push(lone);
            above.extend(rest);
        }
    }
    // outlines and holes by nesting depth: even depth is an outline
    let rings = loops(&segments);
    let depth: Vec<usize> = rings
        .iter()
        .enumerate()
        .map(|(i, r)| {
            rings
                .iter()
                .enumerate()
                .filter(|(j, o)| *j != i && r.first().is_some_and(|p| ring_contains(o, *p)))
                .count()
        })
        .collect();
    for (i, outer) in rings.iter().enumerate() {
        if !depth[i].is_multiple_of(2) {
            continue;
        }
        // its holes: rings one level deeper that sit inside it
        let holes: Vec<Vec<Point2>> = rings
            .iter()
            .enumerate()
            .filter(|(j, h)| {
                depth[*j] == depth[i] + 1 && h.first().is_some_and(|p| ring_contains(outer, *p))
            })
            .map(|(_, h)| h.clone())
            .collect();
        for [a, b, c] in triangulate(outer, &holes) {
            let (a, b, c) =
                (Vec3::new(a.x, a.y, z), Vec3::new(b.x, b.y, z), Vec3::new(c.x, c.y, z));
            below.push(Triangle { a, b, c });
            above.push(Triangle { a, b: c, c: b });
        }
    }
    let keep = |tris: Vec<Triangle>| -> Result<Mesh, GeomError> {
        let tris =
            tris.into_iter().filter(|t| (t.b - t.a).cross(t.c - t.a).length() > 1e-12).collect();
        Mesh::new(tris)
    };
    Ok((keep(below)?, keep(above)?))
}

// ------------------------------------------------------------ reduce

/// Fewer triangles: vertices are merged on a grid of `cell` mm (each cell's
/// vertices become their average) and collapsed triangles dropped.
pub fn reduce(mesh: &Mesh, cell: f64) -> Result<Mesh, GeomError> {
    if !(cell.is_finite() && cell > WELD) {
        return Ok(mesh.clone());
    }
    let key = |v: Vec3| {
        ((v.x / cell).floor() as i64, (v.y / cell).floor() as i64, (v.z / cell).floor() as i64)
    };
    let mut sums: HashMap<(i64, i64, i64), (Vec3, f64)> = HashMap::new();
    for t in mesh.triangles() {
        for v in t.vertices() {
            let e = sums.entry(key(v)).or_insert((Vec3::ZERO, 0.0));
            e.0 = e.0 + v;
            e.1 += 1.0;
        }
    }
    let at = |v: Vec3| sums.get(&key(v)).map_or(v, |(s, n)| s.scale(1.0 / n));
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for t in mesh.triangles() {
        let (ka, kb, kc) = (key(t.a), key(t.b), key(t.c));
        if ka == kb || kb == kc || ka == kc {
            continue;
        }
        // the same three cells once (either winding is a duplicate face)
        let mut sorted = [ka, kb, kc];
        sorted.sort();
        if !seen.insert(sorted) {
            continue;
        }
        out.push(Triangle { a: at(t.a), b: at(t.b), c: at(t.c) });
    }
    Mesh::new(out)
}

// ------------------------------------------------------------ intersections

fn tri_tri(a: &Triangle, b: &Triangle) -> bool {
    // shared corners are neighbours, not intersections
    let av = a.vertices();
    let bv = b.vertices();
    if av.iter().any(|p| bv.iter().any(|q| (*p - *q).length() < WELD)) {
        return false;
    }
    // does a segment cross a triangle's interior?
    let seg_hits = |p: Vec3, q: Vec3, t: &Triangle| {
        let (e1, e2) = (t.b - t.a, t.c - t.a);
        let d = q - p;
        let h = d.cross(e2);
        let det = e1.dot(h);
        if det.abs() < 1e-12 {
            return false;
        }
        let f = 1.0 / det;
        let s = p - t.a;
        let u = f * s.dot(h);
        if !(0.0..=1.0).contains(&u) {
            return false;
        }
        let qv = s.cross(e1);
        let v = f * d.dot(qv);
        if v < 0.0 || u + v > 1.0 {
            return false;
        }
        let w = f * e2.dot(qv);
        w > 1e-9 && w < 1.0 - 1e-9
    };
    (0..3).any(|k| seg_hits(av[k], av[(k + 1) % 3], b))
        || (0..3).any(|k| seg_hits(bv[k], bv[(k + 1) % 3], a))
}

/// Pairs of triangles that cross each other (not counting neighbours),
/// found through a grid of the triangles' boxes. `None` when the mesh is too
/// large to check.
pub fn self_intersections(mesh: &Mesh) -> Option<usize> {
    let tris = mesh.triangles();
    if tris.len() > MAX_INTERSECTION_TRIANGLES {
        return None;
    }
    let b = mesh.bounds();
    let size = b.size();
    let cell = (size.x.max(size.y).max(size.z) / (tris.len() as f64).cbrt().max(1.0)).max(0.05);
    let key = |v: f64, lo: f64| ((v - lo) / cell).floor() as i64;
    let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        let v = t.vertices();
        let lo = |f: fn(&Vec3) -> f64| v.iter().map(f).fold(f64::INFINITY, f64::min);
        let hi = |f: fn(&Vec3) -> f64| v.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
        let (x0, x1) = (key(lo(|p| p.x), b.min().x), key(hi(|p| p.x), b.min().x));
        let (y0, y1) = (key(lo(|p| p.y), b.min().y), key(hi(|p| p.y), b.min().y));
        let (z0, z1) = (key(lo(|p| p.z), b.min().z), key(hi(|p| p.z), b.min().z));
        if (x1 - x0 + 1) * (y1 - y0 + 1) * (z1 - z0 + 1) > 4096 {
            continue;
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    grid.entry((x, y, z)).or_default().push(i);
                }
            }
        }
    }
    let mut pairs = std::collections::HashSet::new();
    for cell_tris in grid.values() {
        for (k, &i) in cell_tris.iter().enumerate() {
            for &j in cell_tris.iter().skip(k + 1) {
                let key = (i.min(j), i.max(j));
                if pairs.contains(&key) {
                    continue;
                }
                if let (Some(a), Some(b)) = (tris.get(i), tris.get(j))
                    && tri_tri(a, b)
                {
                    pairs.insert(key);
                }
            }
        }
    }
    Some(pairs.len())
}

// ------------------------------------------------------------ hollow

/// A hollow version of a closed mesh: the outer surface plus an inner one
/// `wall` mm in, facing inward, so the slicer prints only the walls. The inner
/// surface moves each vertex along its averaged normal; very thin features
/// should not be hollowed with a thick wall.
pub fn hollow(mesh: &Mesh, wall: f64) -> Result<Mesh, GeomError> {
    if !(wall.is_finite() && wall > 0.0) {
        return Ok(mesh.clone());
    }
    let key = |v: Vec3| {
        ((v.x / WELD).round() as i64, (v.y / WELD).round() as i64, (v.z / WELD).round() as i64)
    };
    let mut normals: HashMap<(i64, i64, i64), Vec3> = HashMap::new();
    for t in mesh.triangles() {
        // area-weighted face normal into every corner
        let n = (t.b - t.a).cross(t.c - t.a);
        for v in t.vertices() {
            let e = normals.entry(key(v)).or_insert(Vec3::ZERO);
            *e = *e + n;
        }
    }
    let inward = |v: Vec3| {
        let n = normals.get(&key(v)).and_then(|n| n.normalized()).unwrap_or(Vec3::ZERO);
        v - n.scale(wall)
    };
    let mut tris = mesh.triangles().to_vec();
    for t in mesh.triangles() {
        tris.push(Triangle { a: inward(t.a), b: inward(t.c), c: inward(t.b) });
    }
    Mesh::new(tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(s: f64) -> Mesh {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x * s, y * s, z * s);
        let quads = [
            [p(0.0, 0.0, 0.0), p(0.0, 1.0, 0.0), p(1.0, 1.0, 0.0), p(1.0, 0.0, 0.0)],
            [p(0.0, 0.0, 1.0), p(1.0, 0.0, 1.0), p(1.0, 1.0, 1.0), p(0.0, 1.0, 1.0)],
            [p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0), p(1.0, 0.0, 1.0), p(0.0, 0.0, 1.0)],
            [p(0.0, 1.0, 0.0), p(0.0, 1.0, 1.0), p(1.0, 1.0, 1.0), p(1.0, 1.0, 0.0)],
            [p(0.0, 0.0, 0.0), p(0.0, 0.0, 1.0), p(0.0, 1.0, 1.0), p(0.0, 1.0, 0.0)],
            [p(1.0, 0.0, 0.0), p(1.0, 1.0, 0.0), p(1.0, 1.0, 1.0), p(1.0, 0.0, 1.0)],
        ];
        let mut tris = Vec::new();
        for q in quads {
            tris.push(Triangle { a: q[0], b: q[1], c: q[2] });
            tris.push(Triangle { a: q[0], b: q[2], c: q[3] });
        }
        Mesh::new(tris).unwrap_or_default()
    }

    #[test]
    fn triangulation_covers_a_square_with_a_hole() {
        let sq = |x: f64, s: f64| {
            vec![
                Point2::new(x, x),
                Point2::new(x + s, x),
                Point2::new(x + s, x + s),
                Point2::new(x, x + s),
            ]
        };
        let tris = triangulate(&sq(0.0, 10.0), &[sq(4.0, 2.0)]);
        let area: f64 = tris.iter().map(|[a, b, c]| area2(*a, *b, *c).abs() / 2.0).sum();
        assert!((area - 96.0).abs() < 1e-6, "{area}");
        assert!(tris.iter().all(|[a, b, c]| area2(*a, *b, *c) > 0.0), "counter-clockwise");
    }

    #[test]
    fn cutting_a_cube_gives_two_closed_halves() {
        let m = cube(10.0);
        let (lo, hi) = cut(&m, 4.0).unwrap_or_default();
        let (rl, rh) = (lo.report(), hi.report());
        assert!(rl.is_watertight() && rh.is_watertight(), "{rl:?} {rh:?}");
        assert!(
            (rl.volume - 400.0).abs() < 1e-6 && (rh.volume - 600.0).abs() < 1e-6,
            "{} {}",
            rl.volume,
            rh.volume
        );
    }

    #[test]
    fn reduce_intersections_and_hollow() {
        let m = cube(10.0);
        // a coarse grid merges nothing on a cube's corners; a huge one collapses it
        assert_eq!(reduce(&m, 1.0).map(|r| r.len()).unwrap_or(0), 12);
        assert_eq!(reduce(&m, 100.0).map(|r| r.len()).unwrap_or(1), 0);
        assert_eq!(self_intersections(&m), Some(0));
        // two cubes overlapping cross each other
        let mut tris = m.triangles().to_vec();
        tris.extend(
            cube(10.0)
                .map_vertices(|v| v + Vec3::new(5.0, 5.0, 5.0))
                .unwrap_or_default()
                .triangles(),
        );
        assert!(self_intersections(&Mesh::new(tris).unwrap_or_default()).is_some_and(|n| n > 0));
        let h = hollow(&m, 2.0).unwrap_or_default();
        assert_eq!(h.len(), 24);
        // outer 1000 mm³ minus an inner shell (inward-facing) gives less
        let v = h.report().volume;
        assert!(v > 0.0 && v < 1000.0, "{v}");
    }
}
