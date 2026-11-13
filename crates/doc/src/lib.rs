#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! The document: what is on the bed, on what machine, with what settings.
//!
//! Units are millimetres, degrees and millimetres per second. Every number that
//! comes from a file or a person is validated on the way in, because a slicer
//! must never emit G-code from a profile that cannot be printed.

pub mod profile;
pub mod settings;

use slicecraft_geom::{Bounds3, GeomError, Mesh, Triangle, Vec3};
use std::fmt;
use std::sync::Arc;

pub use settings::{
    Flavor, InfillPattern, PerimeterOrder, PrintSettings, SeamMode, SettingKind, SettingSpec,
    SettingValue, SolidPattern, Tab,
};

/// Largest scale factor a placement may apply on any axis.
pub const MAX_SCALE: f64 = 1000.0;
/// Smallest scale factor a placement may apply on any axis.
pub const MIN_SCALE: f64 = 0.001;
/// Largest number of objects one document may hold.
pub const MAX_OBJECTS: usize = 1000;

/// Placement of an object on the bed.
///
/// The object's reference point is the centre of its own bounding box. The
/// model is scaled and rotated about that point (X, then Y, then Z), and
/// `translate` says where the point goes in X/Y. Models always sit on the bed:
/// whatever the rotation, the lowest point is dropped to z = 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub translate: Vec3,
    /// Degrees about X, Y and Z, applied in that order.
    pub rotate: Vec3,
    /// Scale per axis, in the model's own frame (1.0 = as imported).
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self { translate: Vec3::ZERO, rotate: Vec3::ZERO, scale: Vec3::new(1.0, 1.0, 1.0) }
    }
}

impl Transform {
    pub fn is_valid(&self) -> bool {
        let scale_ok = |k: f64| k.is_finite() && (MIN_SCALE..=MAX_SCALE).contains(&k);
        self.translate.is_in_range()
            && self.rotate.is_finite()
            && self.rotate.x.abs() <= 3600.0
            && self.rotate.y.abs() <= 3600.0
            && self.rotate.z.abs() <= 3600.0
            && scale_ok(self.scale.x)
            && scale_ok(self.scale.y)
            && scale_ok(self.scale.z)
    }

    /// Scale, then rotate X → Y → Z, about the origin.
    pub fn apply_linear(&self, v: Vec3) -> Vec3 {
        let p = Vec3::new(v.x * self.scale.x, v.y * self.scale.y, v.z * self.scale.z);
        let (sx, cx) = self.rotate.x.to_radians().sin_cos();
        let p = Vec3::new(p.x, p.y * cx - p.z * sx, p.y * sx + p.z * cx);
        let (sy, cy) = self.rotate.y.to_radians().sin_cos();
        let p = Vec3::new(p.x * cy + p.z * sy, p.y, -p.x * sy + p.z * cy);
        let (sz, cz) = self.rotate.z.to_radians().sin_cos();
        Vec3::new(p.x * cz - p.y * sz, p.x * sz + p.y * cz, p.z)
    }
}

/// Which way to mirror a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// One model on the bed. The mesh is shared, so copying a document (for undo,
/// or to slice on another thread) does not copy triangles.
#[derive(Debug, Clone)]
pub struct SceneObject {
    pub name: String,
    pub mesh: Arc<Mesh>,
    pub transform: Transform,
    pub visible: bool,
    /// Local reference point: the centre of the mesh's bounding box.
    anchor: Vec3,
}

impl SceneObject {
    pub fn new(name: impl Into<String>, mesh: Mesh) -> Self {
        Self::shared(name, Arc::new(mesh))
    }

    pub fn shared(name: impl Into<String>, mesh: Arc<Mesh>) -> Self {
        let anchor = mesh.bounds().center();
        Self { name: name.into(), mesh, transform: Transform::default(), visible: true, anchor }
    }

    /// Model-to-bed mapping before the drop: centre, scale, rotate, place.
    fn placed(&self, v: Vec3) -> Vec3 {
        let t = &self.transform;
        let p = t.apply_linear(v - self.anchor);
        Vec3::new(p.x + t.translate.x, p.y + t.translate.y, p.z)
    }

    fn drop_height(&self) -> f64 {
        let b = self.mesh.bounds_mapped(|v| self.placed(v));
        if b.is_empty() { 0.0 } else { b.min().z }
    }

    /// The model-to-bed mapping for one vertex, including the drop to the bed.
    /// Cost: one pass over the mesh to find the drop; use `world_mesh` for many.
    pub fn to_world(&self, v: Vec3) -> Vec3 {
        let drop = self.drop_height();
        self.placed(v) - Vec3::new(0.0, 0.0, drop)
    }

    /// Bounds on the bed, with every transform applied and the model on the bed.
    pub fn world_bounds(&self) -> Bounds3 {
        let b = self.mesh.bounds_mapped(|v| self.placed(v));
        if b.is_empty() {
            return b;
        }
        let lift = Vec3::new(0.0, 0.0, b.min().z);
        let mut out = Bounds3::empty();
        out.add(b.min() - lift);
        out.add(b.max() - lift);
        out
    }

    /// The mesh as it sits on the bed, ready to slice. A transform that turns
    /// the model inside out (an odd number of negative scales) cannot happen:
    /// scales are positive, and mirroring rewrites the mesh instead.
    pub fn world_mesh(&self) -> Result<Mesh, GeomError> {
        let drop = Vec3::new(0.0, 0.0, self.drop_height());
        self.mesh.map_vertices(|v| self.placed(v) - drop)
    }

    /// Mirror the model's own mesh across `axis`, keeping its outside outside.
    pub fn mirror(&mut self, axis: Axis) -> Result<(), GeomError> {
        let c = self.anchor;
        let flip = |v: Vec3| match axis {
            Axis::X => Vec3::new(2.0 * c.x - v.x, v.y, v.z),
            Axis::Y => Vec3::new(v.x, 2.0 * c.y - v.y, v.z),
            Axis::Z => Vec3::new(v.x, v.y, 2.0 * c.z - v.z),
        };
        // a reflection reverses winding; swap two corners to restore it
        let tris: Vec<Triangle> = self
            .mesh
            .triangles()
            .iter()
            .map(|t| Triangle { a: flip(t.a), b: flip(t.c), c: flip(t.b) })
            .collect();
        self.mesh = Arc::new(Mesh::new(tris)?);
        Ok(())
    }

    /// Size of the model as placed (after scale and rotation).
    pub fn size(&self) -> Vec3 {
        self.world_bounds().size()
    }

    /// Size of the model as imported.
    pub fn original_size(&self) -> Vec3 {
        self.mesh.bounds().size()
    }
}

/// A print bed and the machine limits around it.
#[derive(Debug, Clone, PartialEq)]
pub struct Bed {
    pub width: f64,
    pub depth: f64,
    pub height: f64,
}

impl Default for Bed {
    fn default() -> Self {
        Self { width: 220.0, depth: 220.0, height: 250.0 }
    }
}

impl Bed {
    /// Room for rounding at the bed edge, in millimetres.
    const EDGE_SLACK: f64 = 1e-6;

    pub fn is_valid(&self) -> bool {
        [self.width, self.depth, self.height]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0 && *v <= slicecraft_geom::MAX_COORD)
    }

    pub fn contains(&self, p: Vec3) -> bool {
        let e = Self::EDGE_SLACK;
        p.x >= -e
            && p.x <= self.width + e
            && p.y >= -e
            && p.y <= self.depth + e
            && p.z >= -e
            && p.z <= self.height + e
    }

    pub fn bounds(&self) -> Bounds3 {
        let mut b = Bounds3::empty();
        b.add(Vec3::ZERO);
        b.add(Vec3::new(self.width, self.depth, self.height));
        b
    }

    pub fn center(&self) -> Vec3 {
        Vec3::new(self.width / 2.0, self.depth / 2.0, 0.0)
    }
}

/// The whole document.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub bed: Bed,
    pub settings: PrintSettings,
    pub objects: Vec<SceneObject>,
}

impl Document {
    /// Replace the profile; the bed follows the profile's machine settings.
    pub fn set_settings(&mut self, settings: PrintSettings) {
        self.bed = Bed {
            width: settings.bed_width,
            depth: settings.bed_depth,
            height: settings.bed_height,
        };
        self.settings = settings;
    }

    /// Add an object, refusing past `MAX_OBJECTS`. Returns its index.
    pub fn add(&mut self, object: SceneObject) -> Result<usize, DocError> {
        if self.objects.len() >= MAX_OBJECTS {
            return Err(DocError::TooManyObjects);
        }
        self.objects.push(object);
        Ok(self.objects.len() - 1)
    }

    pub fn bounds(&self) -> Bounds3 {
        let mut b = Bounds3::empty();
        for o in &self.objects {
            let ob = o.world_bounds();
            if !ob.is_empty() {
                b.add(ob.min());
                b.add(ob.max());
            }
        }
        b
    }

    /// Visible objects that fall outside the bed, by name.
    pub fn outside_bed(&self) -> Vec<String> {
        let mut out = Vec::new();
        for o in self.objects.iter().filter(|o| o.visible) {
            let ob = o.world_bounds();
            if ob.is_empty() {
                continue;
            }
            // models are dropped to the bed when sliced, so judge them there
            let lift = Vec3::new(0.0, 0.0, ob.min().z);
            if !self.bed.contains(ob.min() - lift) || !self.bed.contains(ob.max() - lift) {
                out.push(o.name.clone());
            }
        }
        out
    }

    /// Put an object's reference point at the bed centre.
    pub fn center_object(&mut self, index: usize) -> Result<(), DocError> {
        let center = self.bed.center();
        let o = self.objects.get_mut(index).ok_or(DocError::NoSuchObject(index))?;
        o.transform.translate = center;
        Ok(())
    }

    /// Lay every object out in rows across the bed, `gap` mm apart, the whole
    /// block centred. Objects keep their rotation and scale.
    pub fn arrange(&mut self, gap: f64) -> Result<(), DocError> {
        if !(gap.is_finite() && gap >= 0.0) {
            return Err(DocError::Setting("arrange gap must be a non-negative number"));
        }
        /// An object's footprint, and its reference point relative to the
        /// footprint's corner.
        struct Item {
            index: usize,
            width: f64,
            depth: f64,
            offset: (f64, f64),
        }
        #[derive(Default)]
        struct Row {
            width: f64,
            depth: f64,
            /// (object index, x within the row, offset)
            members: Vec<(usize, f64, (f64, f64))>,
        }
        let mut items = Vec::new();
        for (index, o) in self.objects.iter().enumerate() {
            let b = o.world_bounds();
            if b.is_empty() {
                continue;
            }
            let t = o.transform.translate;
            items.push(Item {
                index,
                width: b.size().x,
                depth: b.size().y,
                offset: (t.x - b.min().x, t.y - b.min().y),
            });
        }
        items.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        let mut rows: Vec<Row> = Vec::new();
        for it in items {
            let fits = rows.last().is_some_and(|r| {
                r.members.is_empty() || r.width + gap + it.width <= self.bed.width
            });
            if !fits {
                rows.push(Row::default());
            }
            if let Some(row) = rows.last_mut() {
                let x = if row.members.is_empty() { 0.0 } else { row.width + gap };
                row.members.push((it.index, x, it.offset));
                row.width = x + it.width;
                row.depth = row.depth.max(it.depth);
            }
        }
        let total_depth: f64 =
            rows.iter().map(|r| r.depth).sum::<f64>() + gap * (rows.len().saturating_sub(1)) as f64;
        let mut y = (self.bed.depth - total_depth) / 2.0;
        for row in rows {
            let x0 = (self.bed.width - row.width) / 2.0;
            for (i, x, (ox, oy)) in row.members {
                if let Some(o) = self.objects.get_mut(i) {
                    o.transform.translate = Vec3::new(x0 + x + ox, y + oy, 0.0);
                }
            }
            y += row.depth + gap;
        }
        Ok(())
    }

    /// A model that cannot be sliced, and why.
    pub fn sliceable(&self) -> Result<(), DocError> {
        if !self.objects.iter().any(|o| o.visible) {
            return Err(DocError::Empty);
        }
        if !self.bed.is_valid() {
            return Err(DocError::Setting("the bed must have positive dimensions"));
        }
        self.settings.validate()?;
        for o in self.objects.iter().filter(|o| o.visible) {
            if o.mesh.is_empty() {
                return Err(DocError::EmptyObject);
            }
            if !o.transform.is_valid() {
                return Err(DocError::BadTransform);
            }
        }
        if let Some(name) = self.outside_bed().into_iter().next() {
            return Err(DocError::OutsideBed(name));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocError {
    Empty,
    EmptyObject,
    BadTransform,
    TooManyObjects,
    NoSuchObject(usize),
    OutsideBed(String),
    Setting(&'static str),
    InvalidSetting(String),
}

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocError::Empty => f.write_str("the document has no visible models"),
            DocError::EmptyObject => f.write_str("a model has no triangles"),
            DocError::BadTransform => f.write_str("a model has an invalid placement"),
            DocError::TooManyObjects => {
                write!(f, "a document may hold at most {MAX_OBJECTS} models")
            }
            DocError::NoSuchObject(i) => write!(f, "there is no model number {i}"),
            DocError::OutsideBed(name) => {
                write!(f, "model \"{name}\" does not fit on the bed where it is placed")
            }
            DocError::Setting(msg) => write!(f, "{msg}"),
            DocError::InvalidSetting(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for DocError {}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_geom::Triangle;

    fn one_triangle() -> Option<Mesh> {
        Mesh::new(vec![Triangle {
            a: Vec3::new(0.0, 0.0, 0.0),
            b: Vec3::new(10.0, 0.0, 0.0),
            c: Vec3::new(0.0, 10.0, 0.0),
        }])
        .ok()
    }

    fn square_plate(size: f64) -> Option<Mesh> {
        Mesh::new(vec![
            Triangle {
                a: Vec3::new(0.0, 0.0, 0.0),
                b: Vec3::new(size, 0.0, 0.0),
                c: Vec3::new(size, size, 1.0),
            },
            Triangle {
                a: Vec3::new(0.0, 0.0, 0.0),
                b: Vec3::new(size, size, 1.0),
                c: Vec3::new(0.0, size, 0.0),
            },
        ])
        .ok()
    }

    #[test]
    fn defaults_validate() {
        assert!(PrintSettings::default().validate().is_ok());
        assert_eq!(Bed::default().width, 220.0);
    }

    #[test]
    fn bad_settings_are_rejected() {
        let zero_layer = PrintSettings { layer_height: 0.0, ..PrintSettings::default() };
        assert!(zero_layer.validate().is_err());
        let too_much_infill =
            PrintSettings { infill_percentage: 101.0, ..PrintSettings::default() };
        assert!(too_much_infill.validate().is_err());
        let hot = PrintSettings { nozzle_temperature: f64::INFINITY, ..PrintSettings::default() };
        assert!(hot.validate().is_err());
        let nan_speed = PrintSettings { travel_speed: f64::NAN, ..PrintSettings::default() };
        assert!(nan_speed.validate().is_err());
        let shells = PrintSettings { perimeter_count: 10_000, ..PrintSettings::default() };
        assert!(shells.validate().is_err());
    }

    #[test]
    fn empty_document_is_not_sliceable() {
        assert_eq!(Document::default().sliceable().err(), Some(DocError::Empty));
    }

    #[test]
    fn object_outside_bed_is_reported_and_refused() {
        let mut doc = Document::default();
        let Some(mesh) = one_triangle() else { return };
        let mut obj = SceneObject::new("big", mesh);
        obj.transform.translate = Vec3::new(500.0, 0.0, 0.0);
        assert!(doc.add(obj).is_ok());
        assert_eq!(doc.outside_bed(), vec!["big".to_string()]);
        assert_eq!(doc.sliceable().err(), Some(DocError::OutsideBed("big".into())));
    }

    #[test]
    fn centred_object_is_sliceable() {
        let mut doc = Document::default();
        let Some(mesh) = one_triangle() else { return };
        assert!(doc.add(SceneObject::new("tri", mesh)).is_ok());
        assert!(doc.center_object(0).is_ok());
        assert!(doc.sliceable().is_ok());
        assert!(doc.center_object(7).is_err());
    }

    #[test]
    fn rotation_and_scale_are_applied() {
        let Some(mesh) = square_plate(10.0) else { return };
        let mut o = SceneObject::new("plate", mesh);
        o.transform.translate = Vec3::new(100.0, 100.0, 0.0);
        let b = o.world_bounds();
        assert!((b.min().x - 95.0).abs() < 1e-9 && (b.max().y - 105.0).abs() < 1e-9);
        o.transform.scale = Vec3::new(2.0, 2.0, 2.0);
        let b = o.world_bounds();
        assert!((b.size().x - 20.0).abs() < 1e-9);
        o.transform.rotate.z = 45.0;
        let b = o.world_bounds();
        // a 20 mm square turned 45° spans its diagonal
        assert!((b.size().x - 20.0 * 2f64.sqrt()).abs() < 1e-9);
        assert!(o.world_mesh().is_ok());
    }

    #[test]
    fn rotation_about_x_stands_a_plate_up_and_keeps_it_on_the_bed() {
        let Some(mesh) = square_plate(10.0) else { return };
        let mut o = SceneObject::new("plate", mesh);
        o.transform.rotate.x = 90.0;
        let b = o.world_bounds();
        assert!(b.min().z.abs() < 1e-9, "dropped to the bed");
        assert!((b.size().z - 10.0).abs() < 1e-9, "stands 10 mm tall: {:?}", b.size());
        o.transform.scale = Vec3::new(1.0, 3.0, 1.0);
        assert!((o.world_bounds().size().z - 30.0).abs() < 1e-9);
        let m = o.world_mesh();
        assert!(m.is_ok_and(|m| m.bounds().min().z.abs() < 1e-9));
    }

    #[test]
    fn mirroring_keeps_triangles_facing_out() {
        let Some(mesh) = one_triangle() else { return };
        let before = mesh.triangles().first().and_then(|t| t.normal());
        let mut o = SceneObject::new("tri", mesh);
        assert!(o.mirror(Axis::X).is_ok());
        let after = o.mesh.triangles().first().and_then(|t| t.normal());
        // a flat triangle facing up still faces up after a mirror in X
        assert_eq!(before.map(|n| n.z.signum()), after.map(|n| n.z.signum()));
        let b = o.mesh.bounds();
        assert!((b.min().x - 0.0).abs() < 1e-9 && (b.max().x - 10.0).abs() < 1e-9);
    }

    #[test]
    fn bad_transforms_are_invalid() {
        let t = Transform { scale: Vec3::new(1.0, 0.0, 1.0), ..Transform::default() };
        assert!(!t.is_valid());
        let t = Transform { rotate: Vec3::new(f64::NAN, 0.0, 0.0), ..Transform::default() };
        assert!(!t.is_valid());
        assert!(Transform::default().is_valid());
    }

    #[test]
    fn profile_drives_the_bed() {
        let mut doc = Document::default();
        let p = PrintSettings { bed_width: 300.0, ..PrintSettings::default() };
        doc.set_settings(p);
        assert_eq!(doc.bed.width, 300.0);
    }

    #[test]
    fn arrange_separates_objects() {
        let mut doc = Document::default();
        for i in 0..3 {
            let Some(mesh) = square_plate(30.0) else { return };
            assert!(doc.add(SceneObject::new(format!("p{i}"), mesh)).is_ok());
        }
        assert!(doc.arrange(5.0).is_ok());
        let bounds: Vec<Bounds3> = doc.objects.iter().map(SceneObject::world_bounds).collect();
        for (i, a) in bounds.iter().enumerate() {
            for b in bounds.iter().skip(i + 1) {
                let apart = a.max().x <= b.min().x + 1e-9
                    || b.max().x <= a.min().x + 1e-9
                    || a.max().y <= b.min().y + 1e-9
                    || b.max().y <= a.min().y + 1e-9;
                assert!(apart, "objects overlap after arrange");
            }
        }
        assert!(doc.outside_bed().is_empty());
    }

    #[test]
    fn flavor_names_round_trip() {
        for f in Flavor::ALL {
            assert_eq!(Flavor::from_name(f.name()), Some(f));
        }
        assert_eq!(Flavor::from_name("KLIPPER"), Some(Flavor::Klipper));
        assert_eq!(Flavor::from_name("nope"), None);
    }
}
