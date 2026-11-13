//! Infill patterns made of polylines: gyroid, honeycomb, wiggle, cubic and
//! octahedral, each clipped to the area it fills.
//!
//! Each pattern is laid out on a grid fixed to the bed origin, so layers line
//! up, and is generated over the area's bounding box, then clipped. Spacing is
//! the average distance between lines for the requested density; patterns made
//! of several line sets spread that material across the sets.

use crate::paths::{MAX_SCANLINES, PathKind, ToolPath};
use slicecraft_geom::{Bounds2, GeomError, Point2, Region};
use std::f64::consts::{FRAC_PI_3, PI, TAU};

/// Most polyline points one pattern may generate for one area.
pub const MAX_PATTERN_POINTS: usize = 4_000_000;
/// Height of a band in the edge index used for clipping, mm.
const BAND: f64 = 2.0;

/// The area's edges bucketed by horizontal band, for fast crossing tests.
struct EdgeIndex {
    min_y: f64,
    bands: Vec<Vec<(Point2, Point2)>>,
}

impl EdgeIndex {
    fn new(region: &Region, b: &Bounds2) -> Result<Self, GeomError> {
        let count = (((b.max_y - b.min_y) / BAND).floor() as usize).saturating_add(1);
        if count > MAX_SCANLINES {
            return Err(GeomError::RegionRange);
        }
        let mut bands = vec![Vec::new(); count];
        for ring in region.contours() {
            let n = ring.len();
            for i in 0..n {
                let (Some(&p), Some(&q)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
                let lo = ((p.y.min(q.y) - b.min_y) / BAND).floor().max(0.0) as usize;
                let hi = ((p.y.max(q.y) - b.min_y) / BAND).floor().max(0.0) as usize;
                for k in lo..=hi.min(count.saturating_sub(1)) {
                    if let Some(band) = bands.get_mut(k) {
                        band.push((p, q));
                    }
                }
            }
        }
        Ok(Self { min_y: b.min_y, bands })
    }

    /// Parameters in (0, 1) where `a→b` crosses an edge.
    fn crossings(&self, a: Point2, b: Point2, out: &mut Vec<f64>) {
        let lo = ((a.y.min(b.y) - self.min_y) / BAND).floor().max(0.0) as usize;
        let hi = ((a.y.max(b.y) - self.min_y) / BAND).floor().max(0.0) as usize;
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let mut seen_last: Option<(Point2, Point2)> = None;
        for k in lo..=hi.min(self.bands.len().saturating_sub(1)) {
            let Some(band) = self.bands.get(k) else { continue };
            for &(p, q) in band {
                // an edge spanning several bands is listed in each; skip repeats
                if lo != hi && seen_last == Some((p, q)) {
                    continue;
                }
                let (ex, ey) = (q.x - p.x, q.y - p.y);
                let den = dx * ey - dy * ex;
                if den.abs() < 1e-15 {
                    continue;
                }
                let t = ((p.x - a.x) * ey - (p.y - a.y) * ex) / den;
                let u = ((p.x - a.x) * dy - (p.y - a.y) * dx) / den;
                if t > 0.0 && t < 1.0 && (0.0..1.0).contains(&u) {
                    out.push(t);
                }
                seen_last = Some((p, q));
            }
        }
        out.sort_by(f64::total_cmp);
        out.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
    }
}

/// Clip polylines to `region`: the pieces inside, as open paths. Pieces
/// shorter than `min_len` are dropped.
pub fn clip_polylines(
    region: &Region,
    lines: Vec<Vec<Point2>>,
    kind: PathKind,
    min_len: f64,
) -> Result<Vec<ToolPath>, GeomError> {
    let Some(b) = region.bounds() else { return Ok(Vec::new()) };
    let index = EdgeIndex::new(region, &b)?;
    let mut out = Vec::new();
    let mut ts = Vec::new();
    for line in lines {
        let mut current: Vec<Point2> = Vec::new();
        let flush = |current: &mut Vec<Point2>, out: &mut Vec<ToolPath>| {
            if current.len() >= 2 {
                let len: f64 = current
                    .windows(2)
                    .map(|w| match w {
                        [p, q] => p.distance(*q),
                        _ => 0.0,
                    })
                    .sum();
                if len >= min_len {
                    out.push(ToolPath { kind, points: std::mem::take(current), closed: false });
                }
            }
            current.clear();
        };
        for w in line.windows(2) {
            let [a, b] = w else { continue };
            ts.clear();
            index.crossings(*a, *b, &mut ts);
            // pieces between crossings: inside or outside by their midpoint
            let mut cuts = Vec::with_capacity(ts.len() + 2);
            cuts.push(0.0);
            cuts.extend_from_slice(&ts);
            cuts.push(1.0);
            for c in cuts.windows(2) {
                let [t0, t1] = c else { continue };
                let at = |t: f64| Point2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                let inside = region.contains(at((t0 + t1) / 2.0));
                if inside {
                    let (p0, p1) = (at(*t0), at(*t1));
                    if current.last().is_none_or(|l| l.distance_sq(p0) > 1e-12) {
                        flush(&mut current, &mut out);
                        current.push(p0);
                    }
                    current.push(p1);
                } else {
                    flush(&mut current, &mut out);
                }
            }
        }
        flush(&mut current, &mut out);
    }
    Ok(out)
}

/// Join polylines whose ends meet within `tol` into longer ones (reversing
/// pieces as needed). Ends are found through a grid hash, so this is linear.
pub fn join_polylines(lines: Vec<Vec<Point2>>, tol: f64) -> Vec<Vec<Point2>> {
    use std::collections::HashMap;
    if lines.len() < 2 || tol.is_nan() || tol <= 0.0 {
        return lines;
    }
    let cell = |p: Point2| ((p.x / tol).floor() as i64, (p.y / tol).floor() as i64);
    let mut ends: HashMap<(i64, i64), Vec<(usize, bool)>> = HashMap::new();
    for (i, l) in lines.iter().enumerate() {
        if let (Some(a), Some(b)) = (l.first(), l.last()) {
            ends.entry(cell(*a)).or_default().push((i, false));
            ends.entry(cell(*b)).or_default().push((i, true));
        }
    }
    let mut used = vec![false; lines.len()];
    let find = |p: Point2,
                used: &[bool],
                ends: &HashMap<(i64, i64), Vec<(usize, bool)>>,
                lines: &[Vec<Point2>]| {
        let (cx, cy) = cell(p);
        let mut best: Option<(f64, usize, bool)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &(j, at_end) in ends.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    if used.get(j).copied().unwrap_or(true) {
                        continue;
                    }
                    let q = if at_end {
                        lines.get(j).and_then(|l| l.last())
                    } else {
                        lines.get(j).and_then(|l| l.first())
                    };
                    if let Some(q) = q {
                        let d = q.distance_sq(p);
                        if d <= tol * tol && best.is_none_or(|(bd, _, _)| d < bd) {
                            best = Some((d, j, at_end));
                        }
                    }
                }
            }
        }
        best
    };
    let mut out = Vec::new();
    for start in 0..lines.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let mut chain = lines.get(start).cloned().unwrap_or_default();
        // grow at the end, then at the front
        for _ in 0..2 {
            while let Some(&end) = chain.last() {
                let Some((_, j, at_end)) = find(end, &used, &ends, &lines) else { break };
                if let Some(u) = used.get_mut(j) {
                    *u = true;
                }
                let Some(next) = lines.get(j) else { break };
                if at_end {
                    chain.extend(next.iter().rev().skip(1).copied());
                } else {
                    chain.extend(next.iter().skip(1).copied());
                }
            }
            chain.reverse();
        }
        out.push(chain);
    }
    out
}

fn bounds_ok(b: &Bounds2, spacing: f64) -> bool {
    spacing.is_finite()
        && spacing > 0.0
        && ((b.max_x - b.min_x) / spacing) * ((b.max_y - b.min_y) / spacing)
            < MAX_PATTERN_POINTS as f64
}

/// Gyroid: the layer's cross-section of `sin x cos y + sin y cos z + sin z cos x = 0`
/// with period `2 · spacing`, solved for `y` along `x` (two branches per period).
pub fn gyroid(
    region: &Region,
    spacing: f64,
    z: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, GeomError> {
    let Some(b) = region.bounds() else { return Ok(Vec::new()) };
    if !bounds_ok(&b, spacing) {
        return Err(GeomError::RegionRange);
    }
    let k = PI / spacing; // radians per mm: period 2·spacing
    let zr = z * k;
    let (sz, cz) = zr.sin_cos();
    // sample along x finely enough to follow the curve
    let step = (spacing / 8.0).max(0.05);
    let xs: Vec<f64> = {
        let mut v = Vec::new();
        let mut x = (b.min_x / step).floor() * step - step;
        while x <= b.max_x + step {
            v.push(x);
            x += step;
        }
        v
    };
    let periods_lo = ((b.min_y * k) / TAU).floor() as i64 - 1;
    let periods_hi = ((b.max_y * k) / TAU).ceil() as i64 + 1;
    let mut lines = Vec::new();
    for branch in [1.0f64, -1.0] {
        for period in periods_lo..=periods_hi {
            let mut line: Vec<Point2> = Vec::new();
            for &x in &xs {
                let xr = x * k;
                // sin x cos y + cos z sin y = −sin z cos x
                let (a, bb, c) = (xr.sin(), cz, -sz * xr.cos());
                let r = (a * a + bb * bb).sqrt();
                if r < 1e-9 || (c / r).abs() > 1.0 {
                    if line.len() >= 2 {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.clear();
                    continue;
                }
                let delta = bb.atan2(a);
                let y = (delta + branch * (c / r).acos() + period as f64 * TAU) / k;
                if let Some(last) = line.last()
                    && (y - last.y).abs() > spacing * 1.5
                {
                    // the branch jumped: start a new line
                    lines.push(std::mem::take(&mut line));
                }
                line.push(Point2::new(x, y));
            }
            if line.len() >= 2 {
                lines.push(line);
            }
        }
    }
    // the two branches meet where the curve turns back: join them there
    let lines = join_polylines(lines, spacing * 0.5);
    clip_polylines(region, lines, kind, spacing * 0.25)
}

/// Wiggle: parallel sine waves, turned 90° on alternate layers.
pub fn wiggle(
    region: &Region,
    spacing: f64,
    layer: usize,
    kind: PathKind,
) -> Result<Vec<ToolPath>, GeomError> {
    let Some(b) = region.bounds() else { return Ok(Vec::new()) };
    if !bounds_ok(&b, spacing) {
        return Err(GeomError::RegionRange);
    }
    let turned = layer % 2 == 1;
    let (lo_u, hi_u, lo_v, hi_v) = if turned {
        (b.min_y, b.max_y, b.min_x, b.max_x)
    } else {
        (b.min_x, b.max_x, b.min_y, b.max_y)
    };
    let amplitude = spacing * 0.25;
    let wavelength = spacing * 2.0;
    let step = wavelength / 12.0;
    let mut lines = Vec::new();
    let mut v = (lo_v / spacing).floor() * spacing - spacing;
    while v <= hi_v + spacing {
        let mut line = Vec::new();
        let mut u = (lo_u / step).floor() * step - step;
        while u <= hi_u + step {
            let off = amplitude * (u / wavelength * TAU).sin();
            line.push(if turned { Point2::new(v + off, u) } else { Point2::new(u, v + off) });
            u += step;
        }
        lines.push(line);
        v += spacing;
    }
    clip_polylines(region, lines, kind, spacing * 0.25)
}

/// Straight lines at `angle`, `spacing` apart, shifted across by `shift`.
fn line_set(b: &Bounds2, spacing: f64, angle: f64, shift: f64) -> Vec<Vec<Point2>> {
    let (s, c) = angle.sin_cos();
    // corners in the rotated frame where the lines are horizontal
    let corners = [(b.min_x, b.min_y), (b.max_x, b.min_y), (b.max_x, b.max_y), (b.min_x, b.max_y)];
    let rot: Vec<(f64, f64)> =
        corners.iter().map(|&(x, y)| (x * c + y * s, -x * s + y * c)).collect();
    let (mut u0, mut u1, mut v0, mut v1) =
        (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for (u, v) in rot {
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    let mut lines = Vec::new();
    let first = ((v0 - shift) / spacing).floor() * spacing + shift;
    let mut v = first;
    while v <= v1 {
        let back = |u: f64| Point2::new(u * c - v * s, u * s + v * c);
        lines.push(vec![back(u0 - 1.0), back(u1 + 1.0)]);
        v += spacing;
    }
    lines
}

/// Cubic: three line sets at 0°, 60° and 120° that move across with height,
/// so the layers stack into cubes standing on a corner.
pub fn cubic(
    region: &Region,
    spacing: f64,
    z: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, GeomError> {
    let Some(b) = region.bounds() else { return Ok(Vec::new()) };
    if !bounds_ok(&b, spacing) {
        return Err(GeomError::RegionRange);
    }
    let set_spacing = spacing * 3.0;
    let shift = z / 2f64.sqrt();
    let mut lines = Vec::new();
    for k in 0..3 {
        lines.extend(line_set(&b, set_spacing, f64::from(k) * FRAC_PI_3, shift));
    }
    clip_polylines(region, lines, kind, spacing * 0.25)
}

/// Octahedral: two line sets at ±45° that move in opposite directions with
/// height, so the layers stack into octahedra and tetrahedra.
pub fn octahedral(
    region: &Region,
    spacing: f64,
    z: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, GeomError> {
    let Some(b) = region.bounds() else { return Ok(Vec::new()) };
    if !bounds_ok(&b, spacing) {
        return Err(GeomError::RegionRange);
    }
    let set_spacing = spacing * 2.0;
    let shift = z;
    let mut lines = line_set(&b, set_spacing, PI / 4.0, shift);
    lines.extend(line_set(&b, set_spacing, -PI / 4.0, -shift));
    clip_polylines(region, lines, kind, spacing * 0.25)
}

/// Honeycomb: regular hexagons whose cells are `2 · spacing` across, the
/// edges chained into long paths so no edge is printed twice.
pub fn honeycomb(
    region: &Region,
    spacing: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, GeomError> {
    let Some(b) = region.bounds() else { return Ok(Vec::new()) };
    if !bounds_ok(&b, spacing) {
        return Err(GeomError::RegionRange);
    }
    // pointy-top hexagons with side a: width √3·a, rows 1.5·a apart; the
    // average line spacing of a honeycomb is about 0.866·a·√3/1.5 ≈ a
    let a = spacing;
    let dx = 3f64.sqrt() * a;
    let dy = 1.5 * a;
    let (c0, c1) = ((b.min_x / dx).floor() as i64 - 1, (b.max_x / dx).ceil() as i64 + 1);
    let (r0, r1) = ((b.min_y / dy).floor() as i64 - 1, (b.max_y / dy).ceil() as i64 + 1);
    // each row draws one zigzag (the upper half of its hexagons); the vertical
    // edges join each row's low points to the row above
    let mut lines = Vec::new();
    for r in r0..=r1 {
        let shift = if r.rem_euclid(2) == 1 { dx / 2.0 } else { 0.0 };
        let cy = r as f64 * dy;
        let mut zig = Vec::new();
        for c in c0..=c1 {
            let cx = c as f64 * dx + shift;
            zig.push(Point2::new(cx - dx / 2.0, cy + a / 2.0));
            zig.push(Point2::new(cx, cy + a));
        }
        lines.push(zig);
        for c in c0..=c1 {
            let cx = c as f64 * dx + shift;
            // the right side of each hexagon, between its two zigzags
            lines.push(vec![
                Point2::new(cx + dx / 2.0, cy - a / 2.0),
                Point2::new(cx + dx / 2.0, cy + a / 2.0),
            ]);
        }
    }
    let lines = join_polylines(lines, a * 0.01);
    clip_polylines(region, lines, kind, spacing * 0.25)
}

/// What the shells and the infill area leave empty in `region`: `thin` is the
/// part too narrow for even the outer shell (thin walls of the model), `gaps`
/// adds the slivers between shells and infill. Both drop anything narrower
/// than `min_width`.
pub fn uncovered(
    region: &Region,
    count: u32,
    width: f64,
    inner: &Region,
    min_width: f64,
) -> Result<(Region, Region), GeomError> {
    if region.is_empty() || count == 0 {
        return Ok((Region::new(), Region::new()));
    }
    let half = width * 0.5;
    let mut covered = inner.clone();
    let mut outer_reach = Region::new();
    for k in 0..count {
        let centre = crate::paths::inner_shell(region, k, width)?;
        if centre.is_empty() {
            break;
        }
        let band = centre.offset(half)?.difference(&centre.offset(-half)?)?;
        if k == 0 {
            outer_reach = centre.offset(half)?;
        }
        covered = covered.union(&band)?;
    }
    let clean = |r: Region| -> Result<Region, GeomError> {
        let m = (min_width * 0.5).max(0.01);
        Ok(r.offset(-m)?.offset(m)?.without_slivers(width * width * 0.5))
    };
    let thin = clean(region.difference(&outer_reach)?)?;
    let gaps = clean(region.difference(&covered)?)?;
    Ok((thin, gaps))
}

/// Fill narrow areas with lines along their length: each island gets as many
/// lines as fit across it (at least one, down its middle).
pub fn fill_narrow(
    region: &Region,
    width: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, GeomError> {
    let mut out = Vec::new();
    for poly in &region.polygons {
        let one = Region { polygons: vec![poly.clone()] };
        let ring = &poly.outer;
        if ring.len() < 3 {
            continue;
        }
        // long axis from the edge-weighted second moments of the outline
        let n = ring.len();
        let (mut sw, mut mx, mut my) = (0.0, 0.0, 0.0);
        for i in 0..n {
            let (Some(a), Some(b)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
            let l = a.distance(*b);
            sw += l;
            mx += l * (a.x + b.x) / 2.0;
            my += l * (a.y + b.y) / 2.0;
        }
        if sw <= 0.0 {
            continue;
        }
        let (mx, my) = (mx / sw, my / sw);
        let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
        for i in 0..n {
            let (Some(a), Some(b)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
            let l = a.distance(*b);
            let (x, y) = ((a.x + b.x) / 2.0 - mx, (a.y + b.y) / 2.0 - my);
            sxx += l * x * x;
            syy += l * y * y;
            sxy += l * x * y;
        }
        let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
        let (s_, c) = angle.sin_cos();
        // extent across (v) and along (u) the long axis
        let (mut u0, mut u1, mut v0, mut v1) =
            (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for p in ring {
            let (u, v) = (p.x * c + p.y * s_, -p.x * s_ + p.y * c);
            u0 = u0.min(u);
            u1 = u1.max(u);
            v0 = v0.min(v);
            v1 = v1.max(v);
        }
        let across = v1 - v0;
        let lines = ((across / width).round() as usize).clamp(1, 64);
        let back = |u: f64, v: f64| Point2::new(u * c - v * s_, u * s_ + v * c);
        let mut polylines = Vec::with_capacity(lines);
        for j in 0..lines {
            let v = v0 + (j as f64 + 0.5) * across / lines as f64;
            polylines.push(vec![back(u0 - 1.0, v), back(u1 + 1.0, v)]);
        }
        out.extend(clip_polylines(&one, polylines, kind, width * 0.5)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f64) -> Region {
        Region::from_loops(&[vec![
            Point2::new(0.0, 0.0),
            Point2::new(s, 0.0),
            Point2::new(s, s),
            Point2::new(0.0, s),
        ]])
        .unwrap_or_default()
    }

    fn total(paths: &[ToolPath]) -> f64 {
        paths
            .iter()
            .flat_map(|p| p.points.windows(2))
            .map(|w| match w {
                [a, b] => a.distance(*b),
                _ => 0.0,
            })
            .sum()
    }

    fn all_inside(region: &Region, paths: &[ToolPath]) -> bool {
        paths.iter().flat_map(|p| p.points.windows(2)).all(|w| match w {
            [a, b] => region.segment_inside(*a, *b),
            _ => true,
        })
    }

    #[test]
    fn clipping_keeps_only_the_inside() {
        let mut hole = vec![
            Point2::new(4.0, 4.0),
            Point2::new(6.0, 4.0),
            Point2::new(6.0, 6.0),
            Point2::new(4.0, 6.0),
        ];
        hole.reverse();
        let r = Region::from_loops(&[
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 10.0),
                Point2::new(0.0, 10.0),
            ],
            hole,
        ])
        .unwrap_or_default();
        let line = vec![Point2::new(-5.0, 5.0), Point2::new(15.0, 5.0)];
        let out = clip_polylines(&r, vec![line], PathKind::SparseInfill, 0.0).unwrap_or_default();
        assert_eq!(out.len(), 2, "{out:?}");
        assert!((total(&out) - 8.0).abs() < 1e-9);
    }

    #[test]
    fn every_pattern_fills_about_the_density_asked_and_stays_inside() {
        // a 40 mm square at 2 mm spacing: straight lines would be 800 mm
        let r = square(40.0);
        let spacing = 2.0;
        let straight = 40.0 * 40.0 / spacing;
        let cases: Vec<(&str, Result<Vec<ToolPath>, GeomError>)> = vec![
            ("gyroid", gyroid(&r, spacing, 3.3, PathKind::SparseInfill)),
            ("wiggle", wiggle(&r, spacing, 1, PathKind::SparseInfill)),
            ("cubic", cubic(&r, spacing, 3.3, PathKind::SparseInfill)),
            ("octahedral", octahedral(&r, spacing, 3.3, PathKind::SparseInfill)),
            ("honeycomb", honeycomb(&r, spacing, PathKind::SparseInfill)),
        ];
        for (name, result) in cases {
            assert!(result.is_ok(), "{name}: {result:?}");
            let paths = result.unwrap_or_default();
            let len = total(&paths);
            assert!(len > straight * 0.7 && len < straight * 1.6, "{name}: {len} mm vs {straight}");
            assert!(all_inside(&r, &paths), "{name} left the area");
            // long paths, not dust
            assert!(paths.len() < 400, "{name}: {} paths", paths.len());
        }
    }

    #[test]
    fn gyroid_changes_with_height_and_cubic_moves() {
        let r = square(30.0);
        let a = gyroid(&r, 2.0, 0.2, PathKind::SparseInfill).unwrap_or_default();
        let b = gyroid(&r, 2.0, 1.2, PathKind::SparseInfill).unwrap_or_default();
        assert_ne!(a.first().map(|p| p.points.clone()), b.first().map(|p| p.points.clone()));
        let c0 = cubic(&r, 2.0, 0.2, PathKind::SparseInfill).unwrap_or_default();
        let c1 = cubic(&r, 2.0, 0.4, PathKind::SparseInfill).unwrap_or_default();
        assert_ne!(c0.first().map(|p| p.points.clone()), c1.first().map(|p| p.points.clone()));
    }

    #[test]
    fn a_thin_wall_gets_one_line_and_gaps_are_found() {
        // a 0.35 mm wall, 20 mm long: thinner than one 0.45 mm line
        let wall = Region::from_loops(&[vec![
            Point2::new(0.0, 0.0),
            Point2::new(20.0, 0.0),
            Point2::new(20.0, 0.35),
            Point2::new(0.0, 0.35),
        ]])
        .unwrap_or_default();
        let inner = wall.offset(-0.9).unwrap_or_default();
        let (thin, gaps) = uncovered(&wall, 2, 0.45, &inner, 0.2).unwrap_or_default();
        assert!(thin.area() > 6.0, "{}", thin.area());
        assert!(gaps.area() >= thin.area() - 1e-6);
        let lines = fill_narrow(&thin, 0.45, PathKind::GapFill).unwrap_or_default();
        assert_eq!(lines.len(), 1, "{lines:?}");
        let len = total(&lines);
        assert!(len > 18.0 && len < 21.0, "{len}");
        // a square that fits two loops leaves (almost) nothing
        let sq = square(10.0);
        let inner = sq.offset(-0.9).unwrap_or_default();
        let (thin, gaps) = uncovered(&sq, 2, 0.45, &inner, 0.2).unwrap_or_default();
        assert!(thin.is_empty() && gaps.area() < 0.5, "{} {}", thin.area(), gaps.area());
    }

    #[test]
    fn huge_areas_at_tiny_spacing_are_refused() {
        let r = square(1000.0);
        assert!(gyroid(&r, 0.001, 0.0, PathKind::SparseInfill).is_err());
        assert!(honeycomb(&r, f64::NAN, PathKind::SparseInfill).is_err());
    }
}
