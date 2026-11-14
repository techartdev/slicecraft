//! Planar regions: polygons with holes, and the operations a slicer needs on
//! them — build from loops, offset, union, difference, intersection.
//!
//! The arithmetic is done by `i_overlay` on a fixed 1 µm integer grid, through
//! its fallible fixed-scale entry points only: its infallible float entry points
//! panic on out-of-range input, and a slicer must not. Every coordinate that
//! reaches this module comes from a validated mesh (`MAX_COORD`), so the grid
//! cannot overflow; if it ever would, the caller gets `GeomError::RegionRange`.

use crate::{Bounds2, GeomError, Point2};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::scale::FixedScaleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};

/// Integer grid steps per millimetre for region arithmetic (1 µm).
pub const GRID_SCALE: f64 = 1000.0;

/// Miter joins sharper than this interior angle are clipped (radians, 30°).
const MITER_MIN_ANGLE: f64 = std::f64::consts::PI / 6.0;

/// A closed ring of points; the closing edge from last to first is implied.
pub type Contour = Vec<Point2>;

/// One filled area: a counter-clockwise outer ring and clockwise holes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Polygon {
    pub outer: Contour,
    pub holes: Vec<Contour>,
}

impl Polygon {
    /// Area enclosed by the outer ring minus the holes.
    pub fn area(&self) -> f64 {
        contour_area(&self.outer).abs()
            - self.holes.iter().map(|h| contour_area(h).abs()).sum::<f64>()
    }

    /// Every ring, outer first.
    pub fn contours(&self) -> impl Iterator<Item = &Contour> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }
}

/// A set of disjoint polygons on one plane.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Region {
    pub polygons: Vec<Polygon>,
}

type Pt = [f64; 2];
type Shapes = Vec<Vec<Vec<Pt>>>;

fn to_pt(p: &Point2) -> Pt {
    [p.x, p.y]
}

/// How loops from a cross-section become solid areas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillMode {
    /// Overlapping bodies merge; holes are holes (non-zero winding).
    #[default]
    Merge,
    /// Every crossing of an outline toggles inside and outside (even-odd).
    Alternate,
    /// Everything inside any outline is solid, holes included.
    Outlines,
}

/// Twice-signed shoelace area halved: positive for counter-clockwise rings.
pub fn signed_area(ring: &[Point2]) -> f64 {
    let n = ring.len();
    let mut a = 0.0;
    for i in 0..n {
        if let (Some(p), Some(q)) = (ring.get(i), ring.get((i + 1) % n)) {
            a += p.x * q.y - q.x * p.y;
        }
    }
    a / 2.0
}

fn ring_to_pts(ring: &[Point2]) -> Vec<Pt> {
    ring.iter().map(to_pt).collect()
}

/// Signed area of a ring; positive when counter-clockwise.
pub fn contour_area(ring: &[Point2]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let (Some(a), Some(b)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
        sum += a.x * b.y - b.x * a.y;
    }
    sum / 2.0
}

/// Length of a ring including its closing edge.
pub fn contour_length(ring: &[Point2]) -> f64 {
    let open: f64 = ring.windows(2).filter_map(|w| Some(w.first()?.distance(*w.get(1)?))).sum();
    match (ring.first(), ring.last()) {
        (Some(a), Some(b)) if ring.len() > 2 => open + a.distance(*b),
        _ => open,
    }
}

impl Region {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }

    pub fn area(&self) -> f64 {
        self.polygons.iter().map(Polygon::area).sum()
    }

    /// Every ring of every polygon.
    pub fn contours(&self) -> impl Iterator<Item = &Contour> {
        self.polygons.iter().flat_map(Polygon::contours)
    }

    pub fn bounds(&self) -> Option<Bounds2> {
        Bounds2::of(self.polygons.iter().flat_map(|p| p.outer.iter()))
    }

    /// The filled area of a set of closed loops under the non-zero winding rule.
    ///
    /// Non-zero rather than even-odd so that overlapping bodies in one mesh merge
    /// instead of cancelling, and a mesh with every facet inverted still fills.
    pub fn from_loops(loops: &[Contour]) -> Result<Self, GeomError> {
        Self::from_loops_mode(loops, FillMode::Merge)
    }

    /// Like [`Region::from_loops`], with a choice of how nested and
    /// overlapping loops become solid.
    pub fn from_loops_mode(loops: &[Contour], mode: FillMode) -> Result<Self, GeomError> {
        let rule = match mode {
            FillMode::Merge | FillMode::Outlines => FillRule::NonZero,
            FillMode::Alternate => FillRule::EvenOdd,
        };
        let oriented: Vec<Contour>;
        let loops = if mode == FillMode::Outlines {
            // every loop counter-clockwise: holes become solid too
            oriented = loops
                .iter()
                .map(|l| {
                    let mut l = l.clone();
                    if crate::region::signed_area(&l) < 0.0 {
                        l.reverse();
                    }
                    l
                })
                .collect();
            oriented.as_slice()
        } else {
            loops
        };
        Self::from_loops_rule(loops, rule)
    }

    fn from_loops_rule(loops: &[Contour], rule: FillRule) -> Result<Self, GeomError> {
        let subj: Vec<Vec<Pt>> =
            loops.iter().filter(|l| l.len() >= 3).map(|l| ring_to_pts(l)).collect();
        if subj.is_empty() {
            return Ok(Self::new());
        }
        let clip: Vec<Vec<Pt>> = Vec::new();
        let shapes = subj
            .overlay_with_fixed_scale(&clip, OverlayRule::Subject, rule, GRID_SCALE)
            .map_err(|_| GeomError::RegionRange)?;
        Ok(Self::from_shapes(shapes))
    }

    /// Grow (`delta > 0`) or shrink (`delta < 0`) every boundary by `delta` mm.
    pub fn offset(&self, delta: f64) -> Result<Self, GeomError> {
        if !delta.is_finite() {
            return Err(GeomError::NonFinite);
        }
        if self.is_empty() || delta == 0.0 {
            return Ok(self.clone());
        }
        let style = OutlineStyle::new(delta).line_join(LineJoin::Miter(MITER_MIN_ANGLE));
        let shapes = self
            .to_shapes()
            .outline_fixed_scale(&style, GRID_SCALE)
            .map_err(|_| GeomError::RegionRange)?;
        Ok(Self::from_shapes(shapes))
    }

    pub fn union(&self, other: &Region) -> Result<Self, GeomError> {
        self.boolean(other, OverlayRule::Union)
    }

    pub fn difference(&self, other: &Region) -> Result<Self, GeomError> {
        if self.is_empty() || other.is_empty() {
            return Ok(self.clone());
        }
        self.boolean(other, OverlayRule::Difference)
    }

    pub fn intersection(&self, other: &Region) -> Result<Self, GeomError> {
        if self.is_empty() || other.is_empty() {
            return Ok(Self::new());
        }
        self.boolean(other, OverlayRule::Intersect)
    }

    /// True when `p` is inside the region or within `ON_EDGE` of its boundary.
    pub fn contains(&self, p: Point2) -> bool {
        let mut inside = false;
        for ring in self.contours() {
            let n = ring.len();
            for i in 0..n {
                let (Some(&a), Some(&b)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
                if distance_to_segment(p, a, b) <= ON_EDGE {
                    return true;
                }
                // even-odd: holes are separate rings, so parity is exact
                if (a.y > p.y) != (b.y > p.y) {
                    let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                    if x > p.x {
                        inside = !inside;
                    }
                }
            }
        }
        inside
    }

    /// True when the straight move from `a` to `b` stays inside the region:
    /// it crosses no boundary edge and its midpoint is inside. Touching the
    /// boundary, as a move along an edge does, counts as inside.
    pub fn segment_inside(&self, a: Point2, b: Point2) -> bool {
        for ring in self.contours() {
            let n = ring.len();
            for i in 0..n {
                let (Some(&c), Some(&d)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
                if segments_cross(a, b, c, d) {
                    return false;
                }
            }
        }
        self.contains(Point2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0))
    }

    /// True when the straight move from `a` to `b` crosses the region's
    /// boundary: it enters or leaves the region. Touching is not crossing.
    pub fn crosses_boundary(&self, a: Point2, b: Point2) -> bool {
        let (lo_x, hi_x) = (a.x.min(b.x), a.x.max(b.x));
        let (lo_y, hi_y) = (a.y.min(b.y), a.y.max(b.y));
        self.contours().any(|ring| {
            let n = ring.len();
            (0..n).any(|i| {
                let (Some(&c), Some(&d)) = (ring.get(i), ring.get((i + 1) % n)) else {
                    return false;
                };
                // cheap reject before the orientation test
                if c.x.max(d.x) < lo_x
                    || c.x.min(d.x) > hi_x
                    || c.y.max(d.y) < lo_y
                    || c.y.min(d.y) > hi_y
                {
                    return false;
                }
                segments_cross(a, b, c, d)
            })
        })
    }

    /// Everything inside an outer outline, holes filled. Polygons that lay
    /// in a hole merge into the one around them.
    pub fn without_holes(&self) -> Result<Self, GeomError> {
        let filled = Region {
            polygons: self
                .polygons
                .iter()
                .map(|p| Polygon { outer: p.outer.clone(), holes: Vec::new() })
                .collect(),
        };
        filled.union(&Region::new())
    }

    /// Drop polygons whose area is below `min_area` mm².
    pub fn without_slivers(mut self, min_area: f64) -> Self {
        self.polygons.retain(|p| p.area() >= min_area);
        self
    }

    fn boolean(&self, other: &Region, rule: OverlayRule) -> Result<Self, GeomError> {
        let a = self.to_shapes();
        let b = other.to_shapes();
        let shapes = a
            .overlay_with_fixed_scale(&b, rule, FillRule::NonZero, GRID_SCALE)
            .map_err(|_| GeomError::RegionRange)?;
        Ok(Self::from_shapes(shapes))
    }

    fn to_shapes(&self) -> Shapes {
        self.polygons
            .iter()
            .map(|p| p.contours().map(|c| ring_to_pts(c)).collect::<Vec<_>>())
            .collect()
    }

    fn from_shapes(shapes: Shapes) -> Self {
        let mut polygons = Vec::with_capacity(shapes.len());
        for shape in shapes {
            let mut rings = shape
                .into_iter()
                .map(|ring| ring.into_iter().map(|[x, y]| Point2::new(x, y)).collect::<Contour>());
            let Some(outer) = rings.next() else { continue };
            if outer.len() < 3 {
                continue;
            }
            let holes: Vec<Contour> = rings.filter(|h| h.len() >= 3).collect();
            polygons.push(Polygon { outer, holes });
        }
        Self { polygons }
    }
}

/// Distance within which a point counts as on a boundary, mm.
const ON_EDGE: f64 = 1e-6;

fn distance_to_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (qx, qy) = (a.x + t * dx - p.x, a.y + t * dy - p.y);
    (qx * qx + qy * qy).sqrt()
}

/// Proper crossing: each segment has its ends strictly on both sides of the
/// other. Touching at an end or running along each other is not a crossing.
fn segments_cross(a: Point2, b: Point2, c: Point2, d: Point2) -> bool {
    let orient =
        |p: Point2, q: Point2, r: Point2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let eps = 1e-9;
    let (d1, d2) = (orient(c, d, a), orient(c, d, b));
    let (d3, d4) = (orient(a, b, c), orient(a, b, d));
    ((d1 > eps && d2 < -eps) || (d1 < -eps && d2 > eps))
        && ((d3 > eps && d4 < -eps) || (d3 < -eps && d4 > eps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filling_holes_keeps_the_outside() {
        let sq = |x0: f64, x1: f64| {
            vec![Point2::new(x0, x0), Point2::new(x1, x0), Point2::new(x1, x1), Point2::new(x0, x1)]
        };
        let mut hole = sq(2.0, 8.0);
        hole.reverse();
        // a ring with an island in its hole
        let r = Region::from_loops(&[sq(0.0, 10.0), hole, sq(4.0, 6.0)]);
        assert!(r.is_ok());
        let Ok(r) = r else { return };
        assert_eq!(r.polygons.len(), 2);
        let Ok(filled) = r.without_holes() else { return };
        assert_eq!(filled.polygons.len(), 1);
        assert!(filled.polygons.iter().all(|p| p.holes.is_empty()));
        assert!((filled.area() - 100.0).abs() < 1e-6, "{}", filled.area());
    }

    #[test]
    fn fill_modes_treat_nested_loops_differently() {
        let sq = |h: f64, ccw: bool| {
            let mut v = vec![
                Point2::new(-h, -h),
                Point2::new(h, -h),
                Point2::new(h, h),
                Point2::new(-h, h),
            ];
            if !ccw {
                v.reverse();
            }
            v
        };
        let area = |loops: &[Contour], m: FillMode| {
            Region::from_loops_mode(loops, m).map_or(-1.0, |r| r.area())
        };
        // a proper hole: clockwise inside counter-clockwise
        let holed = [sq(5.0, true), sq(2.0, false)];
        assert!((area(&holed, FillMode::Merge) - 84.0).abs() < 1e-6);
        assert!((area(&holed, FillMode::Outlines) - 100.0).abs() < 1e-6);
        // two bodies, one inside the other, both counter-clockwise
        let nested = [sq(5.0, true), sq(2.0, true)];
        assert!((area(&nested, FillMode::Merge) - 100.0).abs() < 1e-6);
        assert!((area(&nested, FillMode::Alternate) - 84.0).abs() < 1e-6);
        assert!(signed_area(&sq(1.0, true)) > 0.0 && signed_area(&sq(1.0, false)) < 0.0);
    }

    fn square(x0: f64, y0: f64, s: f64) -> Contour {
        vec![
            Point2::new(x0, y0),
            Point2::new(x0 + s, y0),
            Point2::new(x0 + s, y0 + s),
            Point2::new(x0, y0 + s),
        ]
    }

    fn reversed(mut c: Contour) -> Contour {
        c.reverse();
        c
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn contains_and_segment_inside_respect_holes_and_edges() {
        // a 10 mm square with a 4 mm hole in the middle
        let mut hole = square(3.0, 3.0, 4.0);
        hole.reverse();
        let r = Region::from_loops(&[square(0.0, 0.0, 10.0), hole]).unwrap_or_default();
        assert!(r.contains(Point2::new(1.0, 1.0)));
        assert!(!r.contains(Point2::new(5.0, 5.0)), "in the hole");
        assert!(!r.contains(Point2::new(11.0, 1.0)));
        assert!(r.contains(Point2::new(0.0, 5.0)), "on the edge counts");
        // along the left edge: inside; across the hole: not
        assert!(r.segment_inside(Point2::new(0.0, 1.0), Point2::new(0.0, 9.0)));
        assert!(!r.segment_inside(Point2::new(1.0, 5.0), Point2::new(9.0, 5.0)));
        // two separate squares: a link between them leaves the region
        let two =
            Region::from_loops(&[square(0.0, 0.0, 2.0), square(2.5, 0.0, 2.0)]).unwrap_or_default();
        assert!(!two.segment_inside(Point2::new(2.0, 1.0), Point2::new(2.5, 1.0)));
        assert!(
            two.crosses_boundary(Point2::new(1.0, 1.0), Point2::new(3.5, 1.0)),
            "from one into the other"
        );
        assert!(
            !two.crosses_boundary(Point2::new(5.0, 1.0), Point2::new(5.0, 9.0)),
            "outside both"
        );
        assert!(!two.crosses_boundary(Point2::new(0.5, 0.5), Point2::new(1.5, 1.5)), "inside one");
    }

    #[test]
    fn loops_with_a_hole_make_one_polygon() {
        let outer = square(0.0, 0.0, 10.0);
        let hole = reversed(square(3.0, 3.0, 4.0));
        let r = Region::from_loops(&[outer, hole]);
        assert!(r.is_ok());
        let Ok(r) = r else { return };
        assert_eq!(r.polygons.len(), 1);
        assert_eq!(r.polygons.first().map(|p| p.holes.len()), Some(1));
        assert!(close(r.area(), 100.0 - 16.0));
    }

    #[test]
    fn inverted_loops_still_fill() {
        let r = Region::from_loops(&[reversed(square(0.0, 0.0, 10.0))]);
        assert!(r.as_ref().is_ok_and(|r| close(r.area(), 100.0)));
    }

    #[test]
    fn overlapping_bodies_merge() {
        let r = Region::from_loops(&[square(0.0, 0.0, 10.0), square(5.0, 0.0, 10.0)]);
        assert!(r.as_ref().is_ok_and(|r| r.polygons.len() == 1 && close(r.area(), 150.0)));
    }

    #[test]
    fn inward_offset_shrinks_square() {
        let Ok(r) = Region::from_loops(&[square(0.0, 0.0, 10.0)]) else { return };
        let shrunk = r.offset(-1.0);
        assert!(shrunk.as_ref().is_ok_and(|s| close(s.area(), 64.0)));
        let gone = r.offset(-6.0);
        assert!(gone.as_ref().is_ok_and(Region::is_empty));
    }

    #[test]
    fn booleans() {
        let Ok(a) = Region::from_loops(&[square(0.0, 0.0, 10.0)]) else { return };
        let Ok(b) = Region::from_loops(&[square(5.0, 5.0, 10.0)]) else { return };
        assert!(a.intersection(&b).is_ok_and(|r| close(r.area(), 25.0)));
        assert!(a.difference(&b).is_ok_and(|r| close(r.area(), 75.0)));
        assert!(a.union(&b).is_ok_and(|r| close(r.area(), 175.0)));
        assert!(a.intersection(&Region::new()).is_ok_and(|r| r.is_empty()));
        assert!(a.difference(&Region::new()).is_ok_and(|r| close(r.area(), 100.0)));
    }

    #[test]
    fn non_finite_offset_is_refused() {
        let Ok(a) = Region::from_loops(&[square(0.0, 0.0, 10.0)]) else { return };
        assert_eq!(a.offset(f64::NAN).err(), Some(GeomError::NonFinite));
    }

    #[test]
    fn contour_measures() {
        let s = square(0.0, 0.0, 2.0);
        assert!(close(contour_area(&s), 4.0));
        assert!(close(contour_length(&s), 8.0));
        assert!(close(contour_area(&reversed(s)), -4.0));
    }
}
