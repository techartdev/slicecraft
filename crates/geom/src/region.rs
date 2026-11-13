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
        let subj: Vec<Vec<Pt>> =
            loops.iter().filter(|l| l.len() >= 3).map(|l| ring_to_pts(l)).collect();
        if subj.is_empty() {
            return Ok(Self::new());
        }
        let clip: Vec<Vec<Pt>> = Vec::new();
        let shapes = subj
            .overlay_with_fixed_scale(&clip, OverlayRule::Subject, FillRule::NonZero, GRID_SCALE)
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

#[cfg(test)]
mod tests {
    use super::*;

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
