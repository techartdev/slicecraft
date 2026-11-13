#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! f64 geometry shared by every SliceCraft layer.
//!
//! World units are millimetres. Z is up. Nothing here allocates without a
//! bound supplied by the caller, because every mesh comes from a file.

pub mod region;
pub mod repair;

use std::fmt;

pub use region::{Contour, Polygon, Region};

/// Tolerance for treating two coordinates as the same point.
pub const EPS: f64 = 1e-9;

/// Largest coordinate magnitude a mesh may carry, in millimetres. A metre-scale
/// printer is far inside this; anything larger is a unit mistake or hostile
/// input, and would overflow the integer grid the region operations use.
pub const MAX_COORD: f64 = 100_000.0;

/// A point or direction in model space, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn scale(self, k: f64) -> Self {
        Self::new(self.x * k, self.y * k, self.z * k)
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// Unit vector in the same direction, or `None` for a (near) zero vector.
    pub fn normalized(self) -> Option<Self> {
        let len = self.length();
        if len < EPS || !len.is_finite() { None } else { Some(self.scale(1.0 / len)) }
    }

    /// True when every coordinate is finite. Hostile input fails this.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// True when the point is finite and inside `±MAX_COORD` on every axis.
    pub fn is_in_range(self) -> bool {
        self.is_finite()
            && self.x.abs() <= MAX_COORD
            && self.y.abs() <= MAX_COORD
            && self.z.abs() <= MAX_COORD
    }

    /// Point at `z` on the segment, or `None` when the segment is parallel to the plane.
    pub fn at_z(self, other: Self, z: f64) -> Option<Self> {
        let dz = other.z - self.z;
        if dz.abs() < EPS {
            return None;
        }
        let t = (z - self.z) / dz;
        if !(-EPS..=1.0 + EPS).contains(&t) {
            return None;
        }
        Some(Self::new(self.x + (other.x - self.x) * t, self.y + (other.y - self.y) * t, z))
    }

    /// The XY projection.
    pub fn xy(self) -> Point2 {
        Point2::new(self.x, self.y)
    }
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;

    fn add(self, other: Self) -> Self::Output {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Vec3;

    fn sub(self, other: Self) -> Self::Output {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl std::ops::Mul<f64> for Vec3 {
    type Output = Vec3;

    fn mul(self, k: f64) -> Self::Output {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }
}

impl fmt::Display for Vec3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:.3}, {:.3}, {:.3})", self.x, self.y, self.z)
    }
}

/// A point on a slice plane, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point2 {
    pub x: f64,
    pub y: f64,
}

impl Point2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn distance_sq(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx * dx + dy * dy
    }

    pub fn same(self, other: Self) -> bool {
        self.distance(other) < 1e-6
    }

    /// Rotate about the origin by `angle` radians.
    pub fn rotated(self, angle: f64) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// An axis-aligned box on a slice plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds2 {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds2 {
    /// Bounds of a set of points, or `None` when there are none.
    pub fn of<'a>(points: impl IntoIterator<Item = &'a Point2>) -> Option<Self> {
        let mut b: Option<Bounds2> = None;
        for p in points {
            b = Some(match b {
                None => Bounds2 { min_x: p.x, min_y: p.y, max_x: p.x, max_y: p.y },
                Some(cur) => Bounds2 {
                    min_x: cur.min_x.min(p.x),
                    min_y: cur.min_y.min(p.y),
                    max_x: cur.max_x.max(p.x),
                    max_y: cur.max_y.max(p.y),
                },
            });
        }
        b
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }
}

/// An axis-aligned bounding box; starts empty and grows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds3 {
    min: Vec3,
    max: Vec3,
    empty: bool,
}

impl Default for Bounds3 {
    fn default() -> Self {
        Self::empty()
    }
}

impl Bounds3 {
    pub const fn empty() -> Self {
        Self { min: Vec3::ZERO, max: Vec3::ZERO, empty: true }
    }

    pub fn add(&mut self, p: Vec3) {
        if self.empty {
            self.min = p;
            self.max = p;
            self.empty = false;
            return;
        }
        self.min = Vec3::new(self.min.x.min(p.x), self.min.y.min(p.y), self.min.z.min(p.z));
        self.max = Vec3::new(self.max.x.max(p.x), self.max.y.max(p.y), self.max.z.max(p.z));
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }

    pub fn min(&self) -> Vec3 {
        self.min
    }

    pub fn max(&self) -> Vec3 {
        self.max
    }

    pub fn size(&self) -> Vec3 {
        if self.empty { Vec3::ZERO } else { self.max - self.min }
    }

    pub fn center(&self) -> Vec3 {
        if self.empty { Vec3::ZERO } else { (self.min + self.max).scale(0.5) }
    }
}

/// One mesh triangle, counter-clockwise when seen from outside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
}

impl Triangle {
    /// Unit outward normal, or `None` for a degenerate triangle.
    pub fn normal(&self) -> Option<Vec3> {
        (self.b - self.a).cross(self.c - self.a).normalized()
    }

    pub fn min_z(&self) -> f64 {
        self.a.z.min(self.b.z).min(self.c.z)
    }

    pub fn max_z(&self) -> f64 {
        self.a.z.max(self.b.z).max(self.c.z)
    }

    pub fn vertices(&self) -> [Vec3; 3] {
        [self.a, self.b, self.c]
    }
}

/// A triangle soup whose every coordinate is finite and inside `±MAX_COORD`.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    triangles: Vec<Triangle>,
}

impl Mesh {
    /// Builds a mesh, rejecting any non-finite or out-of-range coordinate.
    pub fn new(triangles: Vec<Triangle>) -> Result<Self, GeomError> {
        for t in &triangles {
            for v in t.vertices() {
                if !v.is_finite() {
                    return Err(GeomError::NonFinite);
                }
                if !v.is_in_range() {
                    return Err(GeomError::OutOfRange);
                }
            }
        }
        Ok(Self { triangles })
    }

    pub fn triangles(&self) -> &[Triangle] {
        &self.triangles
    }

    pub fn len(&self) -> usize {
        self.triangles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    pub fn bounds(&self) -> Bounds3 {
        let mut b = Bounds3::empty();
        for t in &self.triangles {
            b.add(t.a);
            b.add(t.b);
            b.add(t.c);
        }
        b
    }

    /// Triangles whose Z range straddles `z` may cross that plane.
    pub fn triangles_crossing(&self, z: f64) -> impl Iterator<Item = &Triangle> {
        self.triangles.iter().filter(move |t| t.min_z() <= z && t.max_z() >= z)
    }

    /// A new mesh with every vertex moved by `f`; the result is validated again.
    pub fn map_vertices(&self, f: impl Fn(Vec3) -> Vec3) -> Result<Self, GeomError> {
        Self::new(
            self.triangles.iter().map(|t| Triangle { a: f(t.a), b: f(t.b), c: f(t.c) }).collect(),
        )
    }

    /// Bounds of the mesh after `f` without building a second mesh.
    pub fn bounds_mapped(&self, f: impl Fn(Vec3) -> Vec3) -> Bounds3 {
        let mut b = Bounds3::empty();
        for t in &self.triangles {
            for v in t.vertices() {
                b.add(f(v));
            }
        }
        b
    }
}

/// Facts about a mesh that tell whether it will slice cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MeshReport {
    pub triangles: usize,
    /// Signed volume, mm³; negative means the faces point inward.
    pub volume: f64,
    pub area: f64,
    /// Edges used by exactly one triangle: holes in the surface.
    pub open_edges: usize,
    /// Edges used by more than two triangles: non-manifold geometry.
    pub non_manifold_edges: usize,
    pub degenerate_triangles: usize,
}

impl MeshReport {
    /// Closed, manifold and facing outward.
    pub fn is_watertight(&self) -> bool {
        self.open_edges == 0 && self.non_manifold_edges == 0 && self.volume > 0.0
    }
}

impl Mesh {
    /// Volume, area and edge topology. Corners are matched on a 0.01 µm grid,
    /// so a shared corner stored with last-bit differences (common in exported
    /// files, and harmless to slicing) is not reported as an open edge.
    pub fn report(&self) -> MeshReport {
        use std::collections::HashMap;
        type Key = (i64, i64, i64);
        const GRID: f64 = 1e5; // steps per mm; coordinates are within MAX_COORD
        let q = |c: f64| (c * GRID).round() as i64;
        let key = |v: Vec3| (q(v.x), q(v.y), q(v.z));
        let mut edges: HashMap<(Key, Key), u32> =
            HashMap::with_capacity(self.triangles.len() * 3 / 2);
        let mut r = MeshReport { triangles: self.triangles.len(), ..MeshReport::default() };
        for t in &self.triangles {
            r.volume += t.a.dot(t.b.cross(t.c)) / 6.0;
            let n = (t.b - t.a).cross(t.c - t.a);
            r.area += n.length() / 2.0;
            if n.length() < EPS {
                r.degenerate_triangles += 1;
            }
            for (p, q) in [(t.a, t.b), (t.b, t.c), (t.c, t.a)] {
                let (kp, kq) = (key(p), key(q));
                let e = if kp <= kq { (kp, kq) } else { (kq, kp) };
                *edges.entry(e).or_insert(0) += 1;
            }
        }
        for count in edges.values() {
            match count {
                1 => r.open_edges += 1,
                2 => {}
                _ => r.non_manifold_edges += 1,
            }
        }
        r
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeomError {
    NonFinite,
    OutOfRange,
    /// A region operation could not fit its input on the integer grid.
    RegionRange,
}

impl fmt::Display for GeomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeomError::NonFinite => f.write_str("geometry contains a non-finite coordinate"),
            GeomError::OutOfRange => write!(
                f,
                "geometry has a coordinate beyond ±{MAX_COORD} mm (wrong units or a damaged file)"
            ),
            GeomError::RegionRange => {
                f.write_str("a layer region is too large for the slicing grid")
            }
        }
    }
}

impl std::error::Error for GeomError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(a: Vec3, b: Vec3, c: Vec3) -> Triangle {
        Triangle { a, b, c }
    }

    #[test]
    fn bounds_start_empty_and_grow() {
        let mut b = Bounds3::empty();
        assert!(b.is_empty());
        b.add(Vec3::new(1.0, 2.0, 3.0));
        b.add(Vec3::new(-1.0, 0.0, 5.0));
        assert!(!b.is_empty());
        assert_eq!(b.size(), Vec3::new(2.0, 2.0, 2.0));
        assert_eq!(b.center(), Vec3::new(0.0, 1.0, 4.0));
    }

    #[test]
    fn degenerate_triangle_has_no_normal() {
        let t = tri(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(t.normal(), None);
    }

    #[test]
    fn mesh_rejects_non_finite() {
        let t =
            tri(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(Mesh::new(vec![t]).err(), Some(GeomError::NonFinite));
    }

    #[test]
    fn mesh_rejects_out_of_range() {
        let t = tri(Vec3::new(1e12, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(Mesh::new(vec![t]).err(), Some(GeomError::OutOfRange));
    }

    #[test]
    fn map_vertices_revalidates() {
        let t = tri(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        let Ok(m) = Mesh::new(vec![t]) else { return };
        assert!(m.map_vertices(|v| v + Vec3::new(5.0, 0.0, 0.0)).is_ok());
        assert_eq!(m.map_vertices(|v| v.scale(1e9)).err(), Some(GeomError::OutOfRange));
        let b = m.bounds_mapped(|v| v + Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(b.min().x, 5.0);
    }

    #[test]
    fn segment_plane_crossing_lands_on_plane() {
        let p = Vec3::new(0.0, 0.0, 0.0).at_z(Vec3::new(0.0, 0.0, 2.0), 1.0);
        assert_eq!(p, Some(Vec3::new(0.0, 0.0, 1.0)));
        assert_eq!(p.map(|q| q.z), Some(1.0));
        let parallel = Vec3::new(0.0, 0.0, 1.0).at_z(Vec3::new(1.0, 0.0, 1.0), 0.0);
        assert_eq!(parallel, None);
    }

    #[test]
    fn report_of_a_closed_tetrahedron_and_an_open_one() {
        let (o, x, y, z) = (
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let faces = vec![tri(o, y, x), tri(o, x, z), tri(o, z, y), tri(x, y, z)];
        let Ok(m) = Mesh::new(faces.clone()) else { return };
        let r = m.report();
        assert!((r.volume - 1.0 / 6.0).abs() < 1e-12);
        assert_eq!((r.open_edges, r.non_manifold_edges), (0, 0));
        assert!(r.is_watertight());
        // a corner moved by one ulp still closes the surface
        let nudged = Vec3::new(f64::from_bits(1.0f64.to_bits() + 1), 0.0, 0.0);
        let mut close_faces = faces.clone();
        if let Some(t) = close_faces.get_mut(3) {
            t.a = nudged;
        }
        let Ok(near) = Mesh::new(close_faces) else { return };
        assert_eq!(near.report().open_edges, 0);
        let Ok(open) = Mesh::new(faces[..3].to_vec()) else { return };
        assert_eq!(open.report().open_edges, 3);
        assert!(!open.report().is_watertight());
    }

    #[test]
    fn point_rotation_and_bounds() {
        let p = Point2::new(1.0, 0.0).rotated(std::f64::consts::FRAC_PI_2);
        assert!(p.distance(Point2::new(0.0, 1.0)) < 1e-12);
        let pts = [Point2::new(1.0, 2.0), Point2::new(-1.0, 5.0)];
        let b = Bounds2::of(pts.iter());
        assert_eq!(b.map(|b| (b.min_x, b.max_y)), Some((-1.0, 5.0)));
        assert!(Bounds2::of([].iter()).is_none());
    }
}
