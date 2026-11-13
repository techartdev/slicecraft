#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! Slicing at the geometric level: a mesh and a Z plane become closed loops,
//! loops become regions, regions become toolpaths, and toolpaths become
//! something a front end can draw.

pub mod paths;
pub mod section;

use slicecraft_geom::region::{contour_area, contour_length};
use slicecraft_geom::{GeomError, Mesh, Region};
use std::fmt;

pub use paths::{LayerPaths, PathKind, ToolPath};
pub use slicecraft_geom::{Bounds2, Point2};

/// A closed loop on one layer; the closing edge from last to first is implied.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop2 {
    pub points: Vec<Point2>,
}

impl Loop2 {
    /// Signed area; positive is counter-clockwise (an outer boundary).
    pub fn signed_area(&self) -> f64 {
        contour_area(&self.points)
    }

    /// Perimeter including the closing edge.
    pub fn length(&self) -> f64 {
        contour_length(&self.points)
    }
}

/// One plane through a mesh: its Z, its closed loops and how clean the cut was.
#[derive(Debug, Clone, Default)]
pub struct Layer {
    pub z: f64,
    pub loops: Vec<Loop2>,
    pub triangles_considered: usize,
    /// Open chains that could not be closed (a mesh defect), dropped.
    pub dropped_chains: usize,
}

impl Layer {
    pub fn bounds(&self) -> Option<Bounds2> {
        Bounds2::of(self.loops.iter().flat_map(|l| l.points.iter()))
    }

    /// The filled area enclosed by the loops.
    pub fn region(&self) -> Result<Region, GeomError> {
        let rings: Vec<Vec<Point2>> = self.loops.iter().map(|l| l.points.clone()).collect();
        Region::from_loops(&rings)
    }
}

/// Intersect one mesh with the plane at `z` and stitch the result into loops.
pub fn cross_section(mesh: &Mesh, z: f64) -> Layer {
    let (segs, considered) = section::segments(mesh, z);
    let stitched = section::stitch(&segs);
    Layer {
        z,
        loops: stitched.loops.into_iter().map(|points| Loop2 { points }).collect(),
        triangles_considered: considered,
        dropped_chains: stitched.dropped_chains,
    }
}

/// Cross sections through the middle of each `layer_height` slab of the mesh,
/// at most `max_layers` of them. Heights are computed, not accumulated, so the
/// thousandth layer is where it should be.
pub fn layers(mesh: &Mesh, layer_height: f64, max_layers: usize) -> Vec<Layer> {
    if !(layer_height.is_finite() && layer_height > 0.0) || mesh.is_empty() {
        return Vec::new();
    }
    let bounds = mesh.bounds();
    let (bottom, top) = (bounds.min().z, bounds.max().z);
    let mut out = Vec::new();
    for i in 0..max_layers {
        let z = bottom + (i as f64 + 0.5) * layer_height;
        if z >= top {
            break;
        }
        out.push(cross_section(mesh, z));
    }
    out
}

/// What a front end draws: one closed polyline per layer loop.
#[derive(Debug, Clone, Default)]
pub struct DisplayList {
    pub polylines: Vec<Vec<Point2>>,
}

impl DisplayList {
    pub fn from_layers(layers: &[Layer]) -> Self {
        let mut polylines = Vec::new();
        for layer in layers {
            for l in &layer.loops {
                let mut ring = l.points.clone();
                if let Some(&first) = ring.first() {
                    ring.push(first);
                }
                polylines.push(ring);
            }
        }
        Self { polylines }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    EmptyMesh,
    Geometry(GeomError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::EmptyMesh => f.write_str("the model has no triangles"),
            RenderError::Geometry(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<GeomError> for RenderError {
    fn from(e: GeomError) -> Self {
        RenderError::Geometry(e)
    }
}

/// Test meshes shared by this crate's tests and, through `pub`, by the crates
/// above it. Built from code, so no file asset is involved.
pub mod fixtures {
    use slicecraft_geom::{Mesh, Triangle, Vec3};

    /// An axis-aligned box from `min` with the given size, outward-facing.
    pub fn cuboid(min: Vec3, size: Vec3) -> Option<Mesh> {
        let (x0, y0, z0) = (min.x, min.y, min.z);
        let (x1, y1, z1) = (min.x + size.x, min.y + size.y, min.z + size.z);
        let c = [
            Vec3::new(x0, y0, z0),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x0, y1, z0),
            Vec3::new(x0, y0, z1),
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x0, y1, z1),
        ];
        let faces = [
            (0, 2, 1),
            (0, 3, 2),
            (4, 5, 6),
            (4, 6, 7),
            (0, 1, 5),
            (0, 5, 4),
            (1, 2, 6),
            (1, 6, 5),
            (2, 3, 7),
            (2, 7, 6),
            (3, 0, 4),
            (3, 4, 7),
        ];
        let mut tris = Vec::with_capacity(faces.len());
        for (a, b, cc) in faces {
            tris.push(Triangle { a: *c.get(a)?, b: *c.get(b)?, c: *c.get(cc)? });
        }
        Mesh::new(tris).ok()
    }

    /// A cube of side `s` with its corner at the origin.
    pub fn cube(s: f64) -> Option<Mesh> {
        cuboid(Vec3::ZERO, Vec3::new(s, s, s))
    }

    /// A box with a square through-hole along Z: outer side `s`, hole side `h`.
    pub fn hollow_box(s: f64, h: f64) -> Option<Mesh> {
        let mut tris: Vec<Triangle> = Vec::new();
        let (o0, o1) = (0.0, s);
        let (i0, i1) = ((s - h) / 2.0, (s + h) / 2.0);
        let top = s;
        // outer walls (outward) and inner walls (facing into the hole)
        let ring = |a: f64, b: f64| {
            [Vec3::new(a, a, 0.0), Vec3::new(b, a, 0.0), Vec3::new(b, b, 0.0), Vec3::new(a, b, 0.0)]
        };
        let outer = ring(o0, o1);
        let inner = ring(i0, i1);
        let up = |p: Vec3| Vec3::new(p.x, p.y, top);
        for k in 0..4 {
            let (p, q) = (outer[k], outer[(k + 1) % 4]);
            tris.push(Triangle { a: p, b: q, c: up(q) });
            tris.push(Triangle { a: p, b: up(q), c: up(p) });
            let (p, q) = (inner[k], inner[(k + 1) % 4]);
            tris.push(Triangle { a: q, b: p, c: up(p) });
            tris.push(Triangle { a: q, b: up(p), c: up(q) });
            // bottom (facing down) and top (facing up) annulus quads
            let (op, oq, ip, iq) = (outer[k], outer[(k + 1) % 4], inner[k], inner[(k + 1) % 4]);
            tris.push(Triangle { a: op, b: iq, c: oq });
            tris.push(Triangle { a: op, b: ip, c: iq });
            tris.push(Triangle { a: up(op), b: up(oq), c: up(iq) });
            tris.push(Triangle { a: up(op), b: up(iq), c: up(ip) });
        }
        Mesh::new(tris).ok()
    }

    /// A regular octahedron of "radius" `r` centred at `(r, r, r)`: every slice
    /// through its equator passes exactly through four vertices.
    pub fn octahedron(r: f64) -> Option<Mesh> {
        let c = Vec3::new(r, r, r);
        let px = c + Vec3::new(r, 0.0, 0.0);
        let nx = c + Vec3::new(-r, 0.0, 0.0);
        let py = c + Vec3::new(0.0, r, 0.0);
        let ny = c + Vec3::new(0.0, -r, 0.0);
        let pz = c + Vec3::new(0.0, 0.0, r);
        let nz = c + Vec3::new(0.0, 0.0, -r);
        let t = |a, b, cc| Triangle { a, b, c: cc };
        Mesh::new(vec![
            t(px, py, pz),
            t(py, nx, pz),
            t(nx, ny, pz),
            t(ny, px, pz),
            t(py, px, nz),
            t(nx, py, nz),
            t(ny, nx, nz),
            t(px, ny, nz),
        ])
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn mid_cube_section_is_one_ccw_square() {
        let Some(mesh) = cube(10.0) else { return };
        let layer = cross_section(&mesh, 5.0);
        assert_eq!(layer.loops.len(), 1);
        assert_eq!(layer.dropped_chains, 0);
        let l = &layer.loops[0];
        assert!((l.signed_area() - 100.0).abs() < 1e-9, "area {}", l.signed_area());
        assert!((l.length() - 40.0).abs() < 1e-9);
    }

    #[test]
    fn hollow_box_has_outer_and_hole() {
        let Some(mesh) = hollow_box(20.0, 8.0) else { return };
        let layer = cross_section(&mesh, 7.3);
        assert_eq!(layer.loops.len(), 2);
        let mut areas: Vec<f64> = layer.loops.iter().map(Loop2::signed_area).collect();
        areas.sort_by(f64::total_cmp);
        assert!((areas[0] + 64.0).abs() < 1e-9, "hole is clockwise: {areas:?}");
        assert!((areas[1] - 400.0).abs() < 1e-9);
        let region = layer.region();
        assert!(region.is_ok_and(|r| (r.area() - 336.0).abs() < 1e-3));
    }

    #[test]
    fn slice_through_vertices_closes() {
        // regression: a plane through mesh vertices used to drop segments
        let Some(mesh) = octahedron(5.0) else { return };
        let layer = cross_section(&mesh, 5.0);
        assert_eq!(layer.loops.len(), 1);
        assert_eq!(layer.dropped_chains, 0);
        // the equator is a square with diagonal 10: area 50
        assert!((layer.loops[0].signed_area() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn plane_outside_mesh_yields_nothing() {
        let Some(mesh) = cube(10.0) else { return };
        assert!(cross_section(&mesh, 50.0).loops.is_empty());
        assert!(cross_section(&mesh, -1.0).loops.is_empty());
    }

    #[test]
    fn layers_are_capped_mid_slab_and_ordered() {
        let Some(mesh) = cube(1.0) else { return };
        let ls = layers(&mesh, 0.1, 5);
        assert_eq!(ls.len(), 5);
        assert!((ls[0].z - 0.05).abs() < 1e-12);
        assert!(ls.windows(2).all(|w| w[0].z < w[1].z));
        assert!(layers(&mesh, 0.0, 5).is_empty());
        assert_eq!(layers(&mesh, 0.1, 1000).len(), 10);
    }

    #[test]
    fn display_list_closes_rings() {
        let Some(mesh) = cube(2.0) else { return };
        let ls = layers(&mesh, 1.0, 4);
        let dl = DisplayList::from_layers(&ls);
        assert_eq!(dl.polylines.len(), 2);
        assert!(dl.polylines.iter().all(|p| p.len() >= 5 && p.first() == p.last()));
    }
}
