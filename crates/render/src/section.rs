//! Plane cross sections: triangle → directed segment → closed loop.
//!
//! **Classification.** A vertex is *above* when `v.z >= z`, otherwise *below*.
//! A vertex lying exactly on the plane is therefore above, which is a symbolic
//! perturbation: no triangle ever has a vertex "on" the plane, every crossing
//! triangle has exactly two crossing edges, and the vertex case cannot produce
//! two hits at one point and lose the segment.
//!
//! **Canonical crossing.** The crossing on an edge is always computed from its
//! above end towards its below end. Two triangles sharing an edge therefore
//! compute bit-identical points, which lets stitching join segments exactly.
//!
//! **Direction.** For a triangle wound counter-clockwise from outside, the
//! segment runs from the crossing on the edge that goes above→below to the
//! crossing on the edge that goes below→above. Outer loops come out
//! counter-clockwise and holes clockwise, with the solid on the left.

use slicecraft_geom::{Mesh, Point2, Triangle, Vec3};
use std::collections::HashMap;

/// Ends of open chains closer than this are joined (mm).
pub const JOIN_TOLERANCE: f64 = 0.01;
/// An open chain whose own ends are closer than this is closed (mm).
pub const CLOSE_GAP: f64 = 1.0;
/// Above this many open chains, tolerant joining is skipped: it is quadratic,
/// and a file this broken needs repair, not guessing.
pub const MAX_OPEN_CHAINS_TO_JOIN: usize = 4096;

/// A directed segment on the plane; the solid is on its left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub a: Point2,
    pub b: Point2,
}

fn crossing(above: Vec3, below: Vec3, z: f64) -> Point2 {
    let t = (above.z - z) / (above.z - below.z);
    Point2::new(above.x + (below.x - above.x) * t, above.y + (below.y - above.y) * t)
}

/// The segment where `t` crosses the plane at `z`, if it does.
pub fn triangle_segment(t: &Triangle, z: f64) -> Option<Segment> {
    let v = t.vertices();
    let up = v.map(|p| p.z >= z);
    if up == [true; 3] || up == [false; 3] {
        return None;
    }
    let mut start = None; // on the above → below edge
    let mut end = None; // on the below → above edge
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (Some(&pi), Some(&pj), Some(&ui), Some(&uj)) =
            (v.get(i), v.get(j), up.get(i), up.get(j))
        else {
            continue;
        };
        match (ui, uj) {
            (true, false) => start = Some(crossing(pi, pj, z)),
            (false, true) => end = Some(crossing(pj, pi, z)),
            _ => {}
        }
    }
    let (a, b) = (start?, end?);
    if !(a.is_finite() && b.is_finite()) || a == b {
        return None;
    }
    Some(Segment { a, b })
}

/// Every segment of a mesh at `z`, plus how many triangles were examined.
pub fn segments(mesh: &Mesh, z: f64) -> (Vec<Segment>, usize) {
    let mut out = Vec::new();
    let mut considered = 0usize;
    for t in mesh.triangles_crossing(z) {
        considered += 1;
        if let Some(s) = triangle_segment(t, z) {
            out.push(s);
        }
    }
    (out, considered)
}

type Key = (u64, u64);

fn key(p: Point2) -> Key {
    // `+ 0.0` folds -0.0 into 0.0 so both spellings of zero meet.
    ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits())
}

/// The result of stitching one plane's segments.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stitched {
    /// Closed rings, last point not repeated.
    pub loops: Vec<Vec<Point2>>,
    /// Chains that could not be closed and were dropped (a mesh defect).
    pub dropped_chains: usize,
    /// Chains that could not be closed, kept when asked for (else empty).
    pub open: Vec<Vec<Point2>>,
}

/// Join directed segments into closed loops.
///
/// First pass: exact endpoint matching, which closes every loop of a watertight
/// mesh. Second pass, for damaged meshes: join open chains whose ends are within
/// `JOIN_TOLERANCE` (in either direction), then close chains whose own gap is
/// under `CLOSE_GAP`. Whatever is still open is dropped and counted.
pub fn stitch(segs: &[Segment]) -> Stitched {
    stitch_with(segs, CLOSE_GAP, false)
}

/// Like [`stitch`]: chains whose own gap is up to `close_gap` mm are closed
/// (0 closes none), and with `keep_open` what stays open is returned in
/// `open` instead of being dropped.
pub fn stitch_with(segs: &[Segment], close_gap: f64, keep_open: bool) -> Stitched {
    let mut starts: HashMap<Key, Vec<usize>> = HashMap::with_capacity(segs.len());
    for (i, s) in segs.iter().enumerate() {
        starts.entry(key(s.a)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut loops = Vec::new();
    let mut open: Vec<Vec<Point2>> = Vec::new();

    for i in 0..segs.len() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        let Some(first) = segs.get(i) else { continue };
        if let Some(u) = used.get_mut(i) {
            *u = true;
        }
        let start_key = key(first.a);
        let mut chain = vec![first.a, first.b];
        // Each step consumes one unused segment, so this terminates.
        while let Some(&last) = chain.last() {
            let end_key = key(last);
            if end_key == start_key {
                chain.pop();
                loops.push(chain);
                break;
            }
            let next = starts.get(&end_key).and_then(|cands| {
                cands.iter().copied().find(|&j| !used.get(j).copied().unwrap_or(true))
            });
            match next {
                Some(j) => {
                    if let Some(u) = used.get_mut(j) {
                        *u = true;
                    }
                    if let Some(s) = segs.get(j) {
                        chain.push(s.b);
                    }
                }
                None => {
                    open.push(chain);
                    break;
                }
            }
        }
    }

    let mut dropped = 0usize;
    let mut kept = Vec::new();
    let close_gap = if close_gap.is_finite() { close_gap.max(0.0) } else { 0.0 };
    if !open.is_empty() {
        if open.len() <= MAX_OPEN_CHAINS_TO_JOIN {
            join_open_chains(&mut open);
        }
        for chain in open {
            match (chain.first(), chain.last()) {
                (Some(a), Some(b)) if chain.len() >= 3 && a.distance(*b) <= close_gap.max(1e-9) => {
                    let mut ring = chain;
                    if ring.first().zip(ring.last()).is_some_and(|(a, b)| a.same(*b)) {
                        ring.pop();
                    }
                    loops.push(ring);
                }
                _ if keep_open && chain.len() >= 2 => kept.push(chain),
                _ => dropped += 1,
            }
        }
    }
    loops.retain(|l| l.len() >= 3);
    Stitched { loops, dropped_chains: dropped, open: kept }
}

/// Greedily append chains whose start (or reversed end) meets another chain's end.
fn join_open_chains(open: &mut Vec<Vec<Point2>>) {
    let tol_sq = JOIN_TOLERANCE * JOIN_TOLERANCE;
    let mut merged = true;
    // Each successful merge removes one chain, so at most `len` rounds.
    while merged {
        merged = false;
        'outer: for i in 0..open.len() {
            let Some(end) = open.get(i).and_then(|c| c.last().copied()) else { continue };
            for j in 0..open.len() {
                if i == j {
                    continue;
                }
                let Some(other) = open.get(j) else { continue };
                let (Some(&s), Some(&e)) = (other.first(), other.last()) else { continue };
                let reverse = if end.distance_sq(s) <= tol_sq {
                    false
                } else if end.distance_sq(e) <= tol_sq {
                    true
                } else {
                    continue;
                };
                let mut tail = open.swap_remove(j);
                if reverse {
                    tail.reverse();
                }
                // swap_remove moved the last chain into slot j; fix `i` if it was that one.
                let target = if i == open.len() { j } else { i };
                if let Some(head) = open.get_mut(target) {
                    head.extend(tail.into_iter().skip(1));
                }
                merged = true;
                break 'outer;
            }
        }
    }
}

/// Outlines are simplified to this tolerance (mm) after slicing: a vertex
/// that moves the outline by less is dropped. Far finer than a nozzle can
/// draw; it only removes the dense facets of finely tessellated curves, which
/// otherwise turn into many thousands of tiny G-code moves.
pub const SIMPLIFY_TOLERANCE: f64 = 0.01;

fn distance_to_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (a.x + t * dx, a.y + t * dy);
    ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt()
}

/// Douglas–Peucker on an open polyline, without recursion: the ends are
/// always kept, and a point stays when dropping it would move the line by
/// more than `tol`.
pub fn simplify_line(points: &[Point2], tol: f64) -> Vec<Point2> {
    let n = points.len();
    if n < 3 || !(tol.is_finite() && tol > 0.0) {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((lo, hi)) = stack.pop() {
        if hi <= lo + 1 {
            continue;
        }
        let (Some(&a), Some(&b)) = (points.get(lo), points.get(hi)) else { continue };
        let mut far = (0.0, lo);
        for (k, p) in points.iter().enumerate().take(hi).skip(lo + 1) {
            let d = distance_to_segment(*p, a, b);
            if d > far.0 {
                far = (d, k);
            }
        }
        if far.0 > tol {
            if let Some(k) = keep.get_mut(far.1) {
                *k = true;
            }
            stack.push((lo, far.1));
            stack.push((far.1, hi));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// [`simplify_line`] for a closed ring (first point not repeated at the end).
/// The ring is split at its first point and the point farthest from it, so
/// both halves keep their shape. A ring that would fall below three points is
/// returned unchanged.
pub fn simplify_ring(points: &[Point2], tol: f64) -> Vec<Point2> {
    let n = points.len();
    if n < 4 {
        return points.to_vec();
    }
    let Some(&first) = points.first() else { return points.to_vec() };
    let mut far = (0.0, 0usize);
    for (k, p) in points.iter().enumerate() {
        let d = (p.x - first.x).powi(2) + (p.y - first.y).powi(2);
        if d > far.0 {
            far = (d, k);
        }
    }
    let split = far.1;
    if split == 0 || split >= n {
        return points.to_vec();
    }
    let mut a = simplify_line(&points[..=split], tol);
    let mut back: Vec<Point2> = points[split..].to_vec();
    back.push(first);
    let b = simplify_line(&back, tol);
    // a ends at the split point and b starts there; b ends at the first point
    a.extend(b.iter().skip(1).take(b.len().saturating_sub(2)));
    if a.len() < 3 { points.to_vec() } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    #[test]
    fn vertex_on_plane_still_yields_a_segment() {
        // One vertex exactly on the plane, one above, one below: the old
        // edge-walk found the vertex twice and dropped the segment.
        let t = Triangle { a: v(0.0, 0.0, 1.0), b: v(2.0, 0.0, 0.0), c: v(0.0, 2.0, 2.0) };
        let s = triangle_segment(&t, 1.0);
        assert!(s.is_some());
        if let Some(s) = s {
            assert!(s.a.distance(s.b) > 0.5);
        }
    }

    #[test]
    fn flat_triangle_on_plane_yields_nothing() {
        let t = Triangle { a: v(0.0, 0.0, 1.0), b: v(2.0, 0.0, 1.0), c: v(0.0, 2.0, 1.0) };
        assert_eq!(triangle_segment(&t, 1.0), None);
    }

    #[test]
    fn square_of_segments_stitches_into_one_ring() {
        let p = [
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 1.0),
        ];
        // shuffled order
        let segs = [
            Segment { a: p[2], b: p[3] },
            Segment { a: p[0], b: p[1] },
            Segment { a: p[3], b: p[0] },
            Segment { a: p[1], b: p[2] },
        ];
        let st = stitch(&segs);
        assert_eq!(st.loops.len(), 1);
        assert_eq!(st.loops.first().map(Vec::len), Some(4));
        assert_eq!(st.dropped_chains, 0);
    }

    #[test]
    fn small_gaps_are_joined_and_hopeless_chains_dropped() {
        let segs = [
            Segment { a: Point2::new(0.0, 0.0), b: Point2::new(5.0, 0.0) },
            Segment { a: Point2::new(5.0, 0.001), b: Point2::new(5.0, 5.0) },
            Segment { a: Point2::new(5.0, 5.0), b: Point2::new(0.0, 5.0) },
            Segment { a: Point2::new(0.0, 5.0), b: Point2::new(0.0, 0.2) },
            // a stray far away
            Segment { a: Point2::new(50.0, 50.0), b: Point2::new(60.0, 50.0) },
        ];
        let st = stitch(&segs);
        assert_eq!(st.loops.len(), 1);
        assert_eq!(st.dropped_chains, 1);
    }

    #[test]
    fn stitching_nothing_is_fine() {
        assert_eq!(stitch(&[]), Stitched::default());
    }

    #[test]
    fn dense_circles_simplify_and_keep_their_shape() {
        // a 10 mm circle as 3600 points: facets far below the tolerance
        let n = 3600;
        let ring: Vec<Point2> = (0..n)
            .map(|k| {
                let a = k as f64 / n as f64 * std::f64::consts::TAU;
                Point2::new(10.0 * a.cos(), 10.0 * a.sin())
            })
            .collect();
        let s = simplify_ring(&ring, SIMPLIFY_TOLERANCE);
        assert!(s.len() < n / 5, "{} points left", s.len());
        assert!(s.len() >= 3);
        // every original point stays within the tolerance of the result
        for p in &ring {
            let d = (0..s.len())
                .map(|k| distance_to_segment(*p, s[k], s[(k + 1) % s.len()]))
                .fold(f64::INFINITY, f64::min);
            assert!(d <= SIMPLIFY_TOLERANCE + 1e-9, "moved by {d}");
        }
        // area kept
        let area = |r: &[Point2]| {
            (0..r.len())
                .map(|k| r[k].x * r[(k + 1) % r.len()].y - r[(k + 1) % r.len()].x * r[k].y)
                .sum::<f64>()
                / 2.0
        };
        assert!((area(&s) - area(&ring)).abs() / area(&ring) < 1e-3);
    }

    #[test]
    fn small_rings_and_bad_tolerances_are_left_alone() {
        let tri = vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), Point2::new(0.0, 1.0)];
        assert_eq!(simplify_ring(&tri, 0.01), tri);
        let sq = vec![
            Point2::new(0.0, 0.0),
            Point2::new(0.5, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 1.0),
        ];
        assert_eq!(simplify_ring(&sq, 0.01).len(), 4);
        assert_eq!(simplify_line(&sq, f64::NAN), sq);
        assert!(simplify_line(&[], 0.01).is_empty());
    }
}
