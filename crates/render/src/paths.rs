//! Toolpaths: what the nozzle draws on one layer, and the generators that make
//! them from a layer region — perimeter shells, line infill, path ordering.
//!
//! Generators take plain numbers, not a profile, so they can be tested against
//! analytic expectations and reused by any front end.

use slicecraft_geom::region::contour_length;
use slicecraft_geom::{GeomError, Point2, Region};

/// Most scanlines one infill call may cast. A 300 mm bed at a 0.1 mm pitch
/// needs ~4,300; the cap only stops absurd input from allocating without bound.
pub const MAX_SCANLINES: usize = 1_000_000;
/// Above this many paths in one group, ordering keeps input order (with
/// alternating direction) instead of the quadratic nearest-neighbour search.
pub const MAX_PATHS_TO_ORDER: usize = 6000;

/// What a path is for; drives speed, colour and comments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathKind {
    Skirt,
    Brim,
    OuterPerimeter,
    InnerPerimeter,
    SolidInfill,
    SparseInfill,
    /// The body of support.
    Support,
    /// Dense support: under or on the model, or at its foot on the bed.
    SupportInterface,
}

impl PathKind {
    pub const ALL: [PathKind; 8] = [
        PathKind::Skirt,
        PathKind::Brim,
        PathKind::OuterPerimeter,
        PathKind::InnerPerimeter,
        PathKind::SolidInfill,
        PathKind::SparseInfill,
        PathKind::Support,
        PathKind::SupportInterface,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PathKind::Skirt => "skirt",
            PathKind::Brim => "brim",
            PathKind::OuterPerimeter => "outer perimeter",
            PathKind::InnerPerimeter => "inner perimeter",
            PathKind::SolidInfill => "solid infill",
            PathKind::SparseInfill => "sparse infill",
            PathKind::Support => "support",
            PathKind::SupportInterface => "dense support",
        }
    }
}

/// One extruded path. A closed path returns to its first point.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolPath {
    pub kind: PathKind,
    pub points: Vec<Point2>,
    pub closed: bool,
}

impl ToolPath {
    pub fn length(&self) -> f64 {
        if self.closed {
            contour_length(&self.points)
        } else {
            self.points.windows(2).filter_map(|w| Some(w.first()?.distance(*w.get(1)?))).sum()
        }
    }

    pub fn start(&self) -> Option<Point2> {
        self.points.first().copied()
    }

    /// Where the nozzle is when the path is done.
    pub fn end(&self) -> Option<Point2> {
        if self.closed { self.points.first().copied() } else { self.points.last().copied() }
    }
}

/// Everything printed on one layer, in print order.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerPaths {
    pub index: usize,
    /// Nozzle height for this layer (top of the layer), mm.
    pub z: f64,
    /// Layer thickness, mm.
    pub height: f64,
    pub paths: Vec<ToolPath>,
    /// `inside_travel[k]`: the travel to `paths[k]` crosses no model outline,
    /// so it needs no retraction. Missing entries mean "retract as usual".
    pub inside_travel: Vec<bool>,
    /// `travel_via[k]`: waypoints the travel to `paths[k]` follows (combing).
    /// Empty or missing means a straight move.
    pub travel_via: Vec<Vec<Point2>>,
}

/// Perimeter shells of `region`, outermost first, and the area left inside the
/// innermost shell (its inner edge, not its centreline).
pub fn perimeters(
    region: &Region,
    count: u32,
    width: f64,
) -> Result<(Vec<ToolPath>, Region), GeomError> {
    let mut paths = Vec::new();
    if count == 0 {
        return Ok((paths, region.clone()));
    }
    for k in 0..count {
        let shell = region.offset(-(width * 0.5 + f64::from(k) * width))?;
        if shell.is_empty() {
            break;
        }
        let kind = if k == 0 { PathKind::OuterPerimeter } else { PathKind::InnerPerimeter };
        for ring in shell.contours() {
            if ring.len() >= 3 {
                paths.push(ToolPath { kind, points: ring.clone(), closed: true });
            }
        }
    }
    let inner = region.offset(-(f64::from(count) * width))?;
    Ok((paths, inner))
}

/// Parallel lines `spacing` apart at `angle` radians, clipped to `region`.
///
/// Lines sit on a global grid (`k · spacing` in the rotated frame), so the same
/// angle on another layer lands on the same lines. Each edge contributes its
/// crossings in one pass, so the cost is edges + crossings, not lines × edges.
pub fn line_infill(
    region: &Region,
    spacing: f64,
    angle: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, GeomError> {
    if !(spacing.is_finite() && spacing > 0.0 && angle.is_finite()) {
        return Err(GeomError::NonFinite);
    }
    if region.is_empty() {
        return Ok(Vec::new());
    }
    // Work in a frame where the lines are horizontal.
    let rings: Vec<Vec<Point2>> =
        region.contours().map(|r| r.iter().map(|p| p.rotated(-angle)).collect()).collect();
    let Some(b) = slicecraft_geom::Bounds2::of(rings.iter().flatten()) else {
        return Ok(Vec::new());
    };
    let k_min = (b.min_y / spacing).ceil();
    let k_max = (b.max_y / spacing).floor();
    if !(k_min.is_finite() && k_max.is_finite()) || k_max < k_min {
        return Ok(Vec::new());
    }
    let count = (k_max - k_min) as usize + 1;
    if count > MAX_SCANLINES {
        return Err(GeomError::RegionRange);
    }
    let mut hits: Vec<Vec<f64>> = vec![Vec::new(); count];
    for ring in &rings {
        let n = ring.len();
        for i in 0..n {
            let (Some(&p), Some(&q)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
            if p.y == q.y {
                continue;
            }
            let (lo, hi) = if p.y < q.y { (p, q) } else { (q, p) };
            // half-open [lo.y, hi.y): a vertex on a scanline is counted once
            let first = (lo.y / spacing).ceil().max(k_min);
            let mut k = first;
            while k <= k_max {
                let y = k * spacing;
                if y >= hi.y {
                    break;
                }
                let x = lo.x + (y - lo.y) * (hi.x - lo.x) / (hi.y - lo.y);
                if let Some(row) = hits.get_mut((k - k_min) as usize) {
                    row.push(x);
                }
                k += 1.0;
            }
        }
    }
    let min_len = spacing.min(0.05);
    let mut out = Vec::new();
    for (row_index, mut xs) in hits.into_iter().enumerate() {
        if xs.len() < 2 {
            continue;
        }
        xs.sort_by(f64::total_cmp);
        let y = (k_min + row_index as f64) * spacing;
        for pair in xs.chunks_exact(2) {
            let &[x0, x1] = pair else { continue };
            if x1 - x0 < min_len {
                continue;
            }
            out.push(ToolPath {
                kind,
                points: vec![Point2::new(x0, y).rotated(angle), Point2::new(x1, y).rotated(angle)],
                closed: false,
            });
        }
    }
    Ok(out)
}

/// Most rings one concentric fill may draw.
/// Most lines `zigzag` joins in one call; past it the lines stay separate.
pub const MAX_ZIGZAG_LINES: usize = 4000;

/// Join parallel fill lines into continuous zigzags: each line's end links to
/// the nearest unused line end within `max_link` when the link stays inside
/// `region`. Fewer, longer paths mean fewer travel moves and retractions.
pub fn zigzag(lines: Vec<ToolPath>, region: &Region, max_link: f64) -> Vec<ToolPath> {
    if lines.len() < 2 || lines.len() > MAX_ZIGZAG_LINES || max_link.is_nan() || max_link <= 0.0 {
        return lines;
    }
    let link2 = max_link * max_link;
    let ends: Vec<Option<(Point2, Point2)>> =
        lines.iter().map(|l| Some((*l.points.first()?, *l.points.last()?))).collect();
    let mut used = vec![false; lines.len()];
    let mut out = Vec::new();
    for start in 0..lines.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let Some(first) = lines.get(start) else { continue };
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let kind = first.kind;
        let mut chain = first.points.clone();
        while let Some(&end) = chain.last() {
            let mut best: Option<(f64, usize, bool)> = None;
            for (j, e) in ends.iter().enumerate() {
                let (Some((a, b)), false) = (e, used.get(j).copied().unwrap_or(true)) else {
                    continue;
                };
                for (point, reversed) in [(*a, false), (*b, true)] {
                    let d2 = end.distance_sq(point);
                    if d2 <= link2 && best.is_none_or(|(bd, _, _)| d2 < bd) {
                        best = Some((d2, j, reversed));
                    }
                }
            }
            let Some((_, j, reversed)) = best else { break };
            let Some(next) = lines.get(j) else { break };
            let Some(&join_at) = (if reversed { next.points.last() } else { next.points.first() })
            else {
                break;
            };
            if !region.segment_inside(end, join_at) {
                // the nearest end is across a boundary: stop this chain here
                break;
            }
            if let Some(u) = used.get_mut(j) {
                *u = true;
            }
            if reversed {
                chain.extend(next.points.iter().rev().copied());
            } else {
                chain.extend(next.points.iter().copied());
            }
        }
        out.push(ToolPath { kind, points: chain, closed: false });
    }
    out
}

/// Largest outline `comb` walks along; bigger ones fall back to a straight move.
pub const MAX_COMB_RING: usize = 20_000;

/// A route from `a` to `b` that stays inside `region`: the straight line, with
/// each stretch that leaves the region replaced by a walk along the outline it
/// crossed, the shorter way round. Returns the waypoints between `a` and `b`
/// (empty when the straight line stays inside), or `None` when there is no
/// such route or it is longer than `max_len`.
pub fn comb(region: &Region, a: Point2, b: Point2, max_len: f64) -> Option<Vec<Point2>> {
    if !region.contains(a) || !region.contains(b) {
        return None;
    }
    let rings: Vec<&Vec<Point2>> = region.contours().collect();
    // every crossing of a→b with an outline edge: (t along a→b, ring, edge, point)
    let mut hits: Vec<(f64, usize, usize, Point2)> = Vec::new();
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    for (r, ring) in rings.iter().enumerate() {
        let n = ring.len();
        for i in 0..n {
            let (Some(&p), Some(&q)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
            let (ex, ey) = (q.x - p.x, q.y - p.y);
            let den = dx * ey - dy * ex;
            if den.abs() < 1e-12 {
                continue;
            }
            let t = ((p.x - a.x) * ey - (p.y - a.y) * ex) / den;
            let u = ((p.x - a.x) * dy - (p.y - a.y) * dx) / den;
            if t > 1e-9 && t < 1.0 - 1e-9 && (0.0..1.0).contains(&u) {
                hits.push((t, r, i, Point2::new(a.x + t * dx, a.y + t * dy)));
            }
        }
    }
    if hits.is_empty() {
        return Some(Vec::new());
    }
    if !hits.len().is_multiple_of(2) {
        return None;
    }
    hits.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut via = Vec::new();
    for pair in hits.chunks_exact(2) {
        let [(_, r0, i0, p0), (_, r1, i1, p1)] = pair else { return None };
        if r0 != r1 {
            return None;
        }
        let ring = rings.get(*r0)?;
        let n = ring.len();
        if n > MAX_COMB_RING {
            return None;
        }
        // forward: vertices i0+1 ..= i1; backward: i0, i0-1, ..., i1+1
        let forward: Vec<Point2> =
            (1..=((i1 + n - i0) % n)).filter_map(|k| ring.get((i0 + k) % n).copied()).collect();
        let backward: Vec<Point2> =
            (0..((i0 + n - i1) % n)).filter_map(|k| ring.get((i0 + n - k) % n).copied()).collect();
        let len = |pts: &[Point2]| {
            let mut at = *p0;
            let mut total = 0.0;
            for q in pts.iter().chain(std::iter::once(p1)) {
                total += at.distance(*q);
                at = *q;
            }
            total
        };
        via.push(*p0);
        via.extend(if len(&forward) <= len(&backward) { forward } else { backward });
        via.push(*p1);
    }
    let mut total = 0.0;
    let mut at = a;
    for q in via.iter().chain(std::iter::once(&b)) {
        total += at.distance(*q);
        at = *q;
    }
    (total <= max_len).then_some(via)
}

pub const MAX_RINGS: usize = 10_000;

/// Rings following the outline inward, `spacing` apart; the first ring sits
/// half a spacing inside the region's edge.
pub fn concentric_infill(
    region: &Region,
    spacing: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, GeomError> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(GeomError::NonFinite);
    }
    let mut out = Vec::new();
    let mut ring = region.offset(-spacing / 2.0)?;
    for _ in 0..MAX_RINGS {
        if ring.is_empty() {
            return Ok(out);
        }
        for c in ring.contours() {
            if c.len() >= 3 {
                out.push(ToolPath { kind, points: c.clone(), closed: true });
            }
        }
        ring = ring.offset(-spacing)?;
    }
    Err(GeomError::RegionRange)
}

/// Rotate a closed path so it starts at the vertex nearest `target`.
pub fn start_near(path: &mut ToolPath, target: Point2) {
    if !path.closed {
        return;
    }
    let best = path
        .points
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.distance_sq(target).total_cmp(&b.1.distance_sq(target)))
        .map(|(i, _)| i);
    if let Some(k) = best {
        path.points.rotate_left(k);
    }
}

/// Order paths to shorten travel, starting from `from`: nearest next path,
/// open paths may be reversed, closed paths start at their nearest vertex.
/// Returns the ordered paths and where the nozzle ends.
pub fn order_paths(mut paths: Vec<ToolPath>, from: Point2) -> (Vec<ToolPath>, Point2) {
    let mut at = from;
    if paths.len() > MAX_PATHS_TO_ORDER {
        for (i, p) in paths.iter_mut().enumerate() {
            if !p.closed && i % 2 == 1 {
                p.points.reverse();
            }
            if let Some(e) = p.end() {
                at = e;
            }
        }
        return (paths, at);
    }
    let mut out = Vec::with_capacity(paths.len());
    while !paths.is_empty() {
        let mut best = (f64::INFINITY, 0usize, Choice::Forward);
        for (i, p) in paths.iter().enumerate() {
            let (d, c) = nearest_entry(p, at);
            if d < best.0 {
                best = (d, i, c);
            }
        }
        let mut p = paths.swap_remove(best.1);
        match best.2 {
            Choice::Forward => {}
            Choice::Reverse => p.points.reverse(),
            Choice::RotateTo(k) => p.points.rotate_left(k),
        }
        if let Some(e) = p.end() {
            at = e;
        }
        out.push(p);
    }
    (out, at)
}

#[derive(Debug, Clone, Copy)]
enum Choice {
    Forward,
    Reverse,
    RotateTo(usize),
}

fn nearest_entry(p: &ToolPath, at: Point2) -> (f64, Choice) {
    if p.closed {
        let mut best = (f64::INFINITY, Choice::RotateTo(0));
        for (k, q) in p.points.iter().enumerate() {
            let d = q.distance_sq(at);
            if d < best.0 {
                best = (d, Choice::RotateTo(k));
            }
        }
        best
    } else {
        let s = p.points.first().map_or(f64::INFINITY, |q| q.distance_sq(at));
        let e = p.points.last().map_or(f64::INFINITY, |q| q.distance_sq(at));
        if e < s { (e, Choice::Reverse) } else { (s, Choice::Forward) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_region(s: f64) -> Option<Region> {
        Region::from_loops(&[vec![
            Point2::new(0.0, 0.0),
            Point2::new(s, 0.0),
            Point2::new(s, s),
            Point2::new(0.0, s),
        ]])
        .ok()
    }

    #[test]
    fn comb_walks_around_a_hole_and_stays_inside() {
        let sq = |x0: f64, s: f64| {
            vec![
                Point2::new(x0, x0),
                Point2::new(x0 + s, x0),
                Point2::new(x0 + s, x0 + s),
                Point2::new(x0, x0 + s),
            ]
        };
        let mut hole = sq(4.0, 2.0);
        hole.reverse();
        let r = Region::from_loops(&[sq(0.0, 10.0), hole]).unwrap_or_default();
        let (a, b) = (Point2::new(1.0, 5.0), Point2::new(9.0, 5.0));
        let via = comb(&r, a, b, 100.0);
        assert!(via.as_ref().is_some_and(|v| !v.is_empty()), "{via:?}");
        let mut pts = vec![a];
        pts.extend(via.unwrap_or_default());
        pts.push(b);
        for w in pts.windows(2) {
            if let [p, q] = w {
                assert!(r.segment_inside(*p, *q), "left the region: {p:?} {q:?}");
            }
        }
        // a straight line inside needs no waypoints; too short a limit refuses
        assert_eq!(comb(&r, Point2::new(1.0, 1.0), Point2::new(9.0, 1.0), 100.0), Some(vec![]));
        assert!(comb(&r, a, b, 8.5).is_none());
        // a point outside: no route
        assert!(comb(&r, Point2::new(-1.0, 5.0), b, 100.0).is_none());
    }

    #[test]
    fn zigzag_joins_lines_inside_a_square_and_not_across_a_gap() {
        let sq = |x0: f64, s: f64| {
            vec![
                Point2::new(x0, 0.0),
                Point2::new(x0 + s, 0.0),
                Point2::new(x0 + s, s),
                Point2::new(x0, s),
            ]
        };
        let one = Region::from_loops(&[sq(0.0, 10.0)]).unwrap_or_default();
        let lines = line_infill(&one, 1.0, 0.0, PathKind::Support).unwrap_or_default();
        let n = lines.len();
        assert!(n >= 9);
        let joined = zigzag(lines, &one, 1.5);
        assert_eq!(joined.len(), 1, "one zigzag through the whole square");
        assert_eq!(joined.first().map(|p| p.points.len()), Some(2 * n));
        // two squares 0.5 mm apart: the lines join within each, never across
        let two = Region::from_loops(&[sq(0.0, 4.0), sq(4.5, 4.0)]).unwrap_or_default();
        let lines = line_infill(&two, 1.0, std::f64::consts::FRAC_PI_2, PathKind::Support)
            .unwrap_or_default();
        let joined = zigzag(lines, &two, 1.5);
        assert_eq!(joined.len(), 2);
        for p in &joined {
            for w in p.points.windows(2) {
                if let [a, b] = w {
                    assert!(two.segment_inside(*a, *b), "a link left the region: {a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn two_perimeters_on_a_square() {
        let Some(r) = square_region(20.0) else { return };
        let Ok((paths, inner)) = perimeters(&r, 2, 0.5) else { return };
        assert_eq!(paths.len(), 2);
        assert_eq!(paths.first().map(|p| p.kind), Some(PathKind::OuterPerimeter));
        // outer shell centreline is 0.25 mm in: 19.5 mm sides
        assert!(paths.first().is_some_and(|p| (p.length() - 78.0).abs() < 1e-2));
        // inner shell 0.75 mm in: 18.5 mm sides
        assert!(paths.get(1).is_some_and(|p| (p.length() - 74.0).abs() < 1e-2));
        assert!((inner.area() - 18.0 * 18.0).abs() < 1e-2);
    }

    #[test]
    fn thin_region_gets_fewer_shells() {
        let Some(r) = square_region(1.0) else { return };
        let Ok((paths, inner)) = perimeters(&r, 5, 0.4) else { return };
        assert!(paths.len() < 5);
        assert!(inner.is_empty());
    }

    #[test]
    fn horizontal_infill_covers_square() {
        let Some(r) = square_region(10.0) else { return };
        let Ok(lines) = line_infill(&r, 1.0, 0.0, PathKind::SolidInfill) else { return };
        // scanlines at y = 0..=9 (y = 10 is outside the half-open top edge)
        assert_eq!(lines.len(), 10);
        let total: f64 = lines.iter().map(ToolPath::length).sum();
        assert!((total - 100.0).abs() < 1e-6);
    }

    #[test]
    fn diagonal_infill_stays_inside() {
        let Some(r) = square_region(10.0) else { return };
        let Ok(lines) = line_infill(&r, 0.7, std::f64::consts::FRAC_PI_4, PathKind::SparseInfill)
        else {
            return;
        };
        assert!(!lines.is_empty());
        for l in &lines {
            for p in &l.points {
                assert!(p.x > -1e-6 && p.x < 10.0 + 1e-6 && p.y > -1e-6 && p.y < 10.0 + 1e-6);
            }
        }
    }

    #[test]
    fn infill_skips_holes() {
        let Ok(r) = Region::from_loops(&[
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 10.0),
                Point2::new(0.0, 10.0),
            ],
            vec![
                Point2::new(3.0, 3.0),
                Point2::new(3.0, 7.0),
                Point2::new(7.0, 7.0),
                Point2::new(7.0, 3.0),
            ],
        ]) else {
            return;
        };
        let Ok(lines) = line_infill(&r, 1.0, 0.0, PathKind::SolidInfill) else { return };
        let total: f64 = lines.iter().map(ToolPath::length).sum();
        // rows y=3..=6 lose 4 mm each through the hole
        assert!((total - (100.0 - 16.0)).abs() < 1e-6);
    }

    #[test]
    fn bad_spacing_is_refused() {
        let Some(r) = square_region(10.0) else { return };
        assert!(line_infill(&r, 0.0, 0.0, PathKind::SolidInfill).is_err());
        assert!(line_infill(&r, f64::NAN, 0.0, PathKind::SolidInfill).is_err());
        assert!(line_infill(&r, 1e-9, 0.0, PathKind::SolidInfill).is_err());
    }

    #[test]
    fn concentric_rings_shrink_to_nothing() {
        let Some(r) = square_region(10.0) else { return };
        let Ok(rings) = concentric_infill(&r, 1.0, PathKind::SolidInfill) else { return };
        // rings at 0.5, 1.5, 2.5, 3.5, 4.5 mm in
        assert_eq!(rings.len(), 5);
        assert!(rings.iter().all(|p| p.closed));
        assert!(rings.first().is_some_and(|p| (p.length() - 36.0).abs() < 1e-2));
    }

    #[test]
    fn start_near_rotates_closed_paths_only() {
        let mut ring = ToolPath {
            kind: PathKind::OuterPerimeter,
            points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), Point2::new(1.0, 1.0)],
            closed: true,
        };
        start_near(&mut ring, Point2::new(2.0, 2.0));
        assert_eq!(ring.start(), Some(Point2::new(1.0, 1.0)));
        let mut open = ToolPath { closed: false, ..ring.clone() };
        let before = open.start();
        start_near(&mut open, Point2::new(0.0, 0.0));
        assert_eq!(open.start(), before);
    }

    #[test]
    fn ordering_picks_nearest_and_reverses() {
        let a = ToolPath {
            kind: PathKind::SolidInfill,
            points: vec![Point2::new(10.0, 0.0), Point2::new(20.0, 0.0)],
            closed: false,
        };
        let b = ToolPath {
            kind: PathKind::SolidInfill,
            points: vec![Point2::new(5.0, 1.0), Point2::new(0.0, 1.0)],
            closed: false,
        };
        let (ordered, end) = order_paths(vec![a, b], Point2::new(0.0, 0.0));
        assert_eq!(ordered.first().and_then(|p| p.start()), Some(Point2::new(0.0, 1.0)));
        assert_eq!(ordered.get(1).and_then(|p| p.start()), Some(Point2::new(10.0, 0.0)));
        assert_eq!(end, Point2::new(20.0, 0.0));
    }

    #[test]
    fn closed_path_starts_at_nearest_vertex() {
        let ring = ToolPath {
            kind: PathKind::OuterPerimeter,
            points: vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 1.0),
                Point2::new(0.0, 1.0),
            ],
            closed: true,
        };
        let (ordered, end) = order_paths(vec![ring], Point2::new(5.0, 5.0));
        assert_eq!(ordered.first().and_then(|p| p.start()), Some(Point2::new(1.0, 1.0)));
        assert_eq!(end, Point2::new(1.0, 1.0));
    }
}
