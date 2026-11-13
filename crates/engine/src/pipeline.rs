//! The slicing pipeline: document → layer regions → shells → solid/sparse
//! areas → infill → ordering → G-code.
//!
//! Per-layer stages run on scoped worker threads. Workers are joined
//! explicitly, so a panic inside a dependency becomes `EngineError::Internal`
//! instead of taking the application down with the print.

use crate::EngineError;
use slicecraft_doc::{Document, InfillPattern, PerimeterOrder, SeamMode, SolidPattern};
use slicecraft_geom::{Mesh, Point2, Region, Vec3};
use slicecraft_io::{GcodeStats, write_gcode};
use slicecraft_render::paths::{
    concentric_infill, line_infill, order_paths, perimeters, start_near,
};
use slicecraft_render::{LayerPaths, PathKind, ToolPath, cross_section};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

/// Largest layer count a single job may produce.
pub const MAX_LAYERS: usize = 100_000;

/// Receives progress and may ask the job to stop.
pub trait SliceObserver {
    fn progress(&mut self, _stage: Stage, _done: usize, _total: usize) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// An observer that watches nothing.
pub struct Quiet;
impl SliceObserver for Quiet {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Sections,
    Support,
    Shells,
    Infill,
    Gcode,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Sections => "cutting layers",
            Stage::Support => "building support",
            Stage::Shells => "building perimeters",
            Stage::Infill => "filling",
            Stage::Gcode => "writing G-code",
        }
    }
}

/// Everything a slice produces.
#[derive(Debug, Clone)]
pub struct SliceOutput {
    pub layers: Vec<LayerPaths>,
    pub gcode: String,
    pub stats: SliceStats,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SliceStats {
    pub gcode: GcodeStats,
    /// Open loops dropped across all layers (a mesh defect when non-zero).
    pub dropped_chains: usize,
    pub warnings: Vec<String>,
}

/// One layer's place in the stack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerSlot {
    pub index: usize,
    /// Nozzle height (top of the layer).
    pub top: f64,
    pub height: f64,
}

impl LayerSlot {
    /// Where the layer is cut: the middle of its slab.
    pub fn cut_z(&self) -> f64 {
        self.top - self.height / 2.0
    }
}

/// Layer slots for a model `model_top` mm tall. Heights are computed from the
/// index, never accumulated.
pub fn layer_schedule(
    first: f64,
    height: f64,
    model_top: f64,
) -> Result<Vec<LayerSlot>, EngineError> {
    let mut out = Vec::new();
    for index in 0..=MAX_LAYERS {
        let (top, h) =
            if index == 0 { (first, first) } else { (first + index as f64 * height, height) };
        let slot = LayerSlot { index, top, height: h };
        if slot.cut_z() >= model_top {
            return Ok(out);
        }
        if index == MAX_LAYERS {
            return Err(EngineError::TooManyLayers);
        }
        out.push(slot);
    }
    Err(EngineError::TooManyLayers)
}

/// Map `f` over `items` on worker threads, in order, reporting progress.
pub(crate) fn par_map<T, R, F>(
    items: &[T],
    stage: Stage,
    observer: &mut dyn SliceObserver,
    f: F,
) -> Result<Vec<R>, EngineError>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> Result<R, EngineError> + Sync,
{
    let total = items.len();
    if total == 0 {
        return Ok(Vec::new());
    }
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 64);
    let chunk = total.div_ceil(threads).max(1);
    let done = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let f = &f;
    let (done_ref, stop_ref) = (&done, &stop);

    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .enumerate()
            .map(|(c, slice)| {
                scope.spawn(move || -> Result<Vec<R>, EngineError> {
                    let mut out = Vec::with_capacity(slice.len());
                    for (k, item) in slice.iter().enumerate() {
                        if stop_ref.load(Ordering::Relaxed) {
                            return Err(EngineError::Cancelled);
                        }
                        match f(c * chunk + k, item) {
                            Ok(r) => out.push(r),
                            Err(e) => {
                                stop_ref.store(true, Ordering::Relaxed);
                                return Err(e);
                            }
                        }
                        done_ref.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(out)
                })
            })
            .collect();

        // the calling thread reports progress and relays cancellation
        loop {
            let d = done.load(Ordering::Relaxed);
            observer.progress(stage, d, total);
            if observer.cancelled() {
                stop.store(true, Ordering::Relaxed);
            }
            if handles.iter().all(|h| h.is_finished()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(15));
        }

        let mut results = Vec::with_capacity(total);
        let mut first_err = None;
        for h in handles {
            match h.join() {
                Ok(Ok(part)) => results.extend(part),
                Ok(Err(e)) => {
                    // a cancellation caused by another worker's error is not the cause
                    if first_err.is_none() || first_err == Some(EngineError::Cancelled) {
                        first_err = Some(e);
                    }
                }
                Err(_) => first_err = Some(EngineError::Internal("a slicing worker failed")),
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(results),
        }
    })
}

/// The models as they sit on the bed, each dropped so its lowest point is z = 0.
pub fn bed_meshes(doc: &Document) -> Result<Vec<Mesh>, EngineError> {
    let mut out = Vec::new();
    for o in doc.objects.iter().filter(|o| o.visible) {
        let world = o.world_mesh()?;
        let drop = world.bounds().min().z;
        let mesh = if drop == 0.0 {
            world
        } else {
            world.map_vertices(|v| v - Vec3::new(0.0, 0.0, drop))?
        };
        out.push(mesh);
    }
    Ok(out)
}

/// Fill `region` with `pattern` at an average line spacing of `spacing`.
fn pattern_infill(
    region: &Region,
    pattern: InfillPattern,
    spacing: f64,
    angle: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, EngineError> {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_3};
    let mut out = Vec::new();
    match pattern {
        // the same material spread over two or three directions
        InfillPattern::Grid => {
            for k in 0..2 {
                out.extend(line_infill(
                    region,
                    spacing * 2.0,
                    angle + f64::from(k) * FRAC_PI_2,
                    kind,
                )?);
            }
        }
        InfillPattern::Triangles => {
            for k in 0..3 {
                out.extend(line_infill(
                    region,
                    spacing * 3.0,
                    angle + f64::from(k) * FRAC_PI_3,
                    kind,
                )?);
            }
        }
        InfillPattern::Concentric => out.extend(concentric_infill(region, spacing, kind)?),
        // not drawn yet: rectilinear stands in, and slicing says so
        InfillPattern::Rectilinear
        | InfillPattern::Lines
        | InfillPattern::Honeycomb
        | InfillPattern::Gyroid
        | InfillPattern::Wiggle
        | InfillPattern::Cubic
        | InfillPattern::Octahedral => {
            out.extend(line_infill(region, spacing, angle, kind)?);
        }
    }
    Ok(out)
}

/// Pseudo-random but repeatable index for scattered seams.
fn scatter(layer: usize, path: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let mut h = (layer as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (path as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    (h % len as u64) as usize
}

/// Loops around the first layer's outline: `count` of them, the first
/// `gap` mm outside it (0 = touching, a brim).
fn rings_around(
    outline: &Region,
    gap: f64,
    count: u32,
    w: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, EngineError> {
    let mut out = Vec::new();
    for k in 0..count {
        let ring = outline.offset(gap + w * 0.5 + f64::from(k) * w)?;
        for p in &ring.polygons {
            out.push(ToolPath { kind, points: p.outer.clone(), closed: true });
        }
    }
    Ok(out)
}

struct Shelled {
    shells: Vec<ToolPath>,
    inner: Region,
}

/// Slice a document into toolpaths and G-code.
pub fn slice_document(
    doc: &Document,
    observer: &mut dyn SliceObserver,
) -> Result<SliceOutput, EngineError> {
    doc.sliceable()?;
    let s = &doc.settings;
    let meshes = bed_meshes(doc)?;
    let top = meshes.iter().map(|m| m.bounds().max().z).fold(0.0f64, f64::max);
    let slots = layer_schedule(s.first_layer_height, s.layer_height, top)?;
    if slots.is_empty() {
        return Err(EngineError::NothingToSlice);
    }
    let w = s.line_width();

    // 1. regions
    let sections = par_map(&slots, Stage::Sections, observer, |_, slot| {
        let mut loops = Vec::new();
        let mut dropped = 0usize;
        for m in &meshes {
            let layer = cross_section(m, slot.cut_z());
            dropped += layer.dropped_chains;
            loops.extend(layer.loops.into_iter().map(|l| l.points));
        }
        Ok((Region::from_loops(&loops)?, dropped))
    })?;
    let dropped_chains: usize = sections.iter().map(|(_, d)| d).sum();
    let regions: Vec<Region> = sections.into_iter().map(|(r, _)| r).collect();

    // 2. support, from the model's layers
    let support_areas = crate::support::support_layers(&regions, &slots, s, w, observer)?;
    let support_first = match support_areas.first() {
        Some(l) if !l.is_empty() => l.area()?,
        _ => Region::new(),
    };
    let supported_layers = support_areas.iter().filter(|l| !l.is_empty()).count();
    let mut support_toolpaths = par_map(&support_areas, Stage::Support, observer, |i, layer| {
        crate::support::support_paths(layer, i, s, w)
    })?;
    drop(support_areas);

    // 3. shells
    let shelled = par_map(&regions, Stage::Shells, observer, |_, region| {
        let (shells, inner) = perimeters(region, s.perimeter_count, w)?;
        Ok(Shelled { shells, inner })
    })?;

    // 4. solid vs sparse, infill
    let empty = Region::new();
    let (below, above) = (s.solid_layers_bottom as usize, s.solid_layers_top as usize);
    let base_angle = s.infill_angle.to_radians();
    let overlap = s.infill_overlap / 100.0 * w;
    let sparse_spacing =
        if s.infill_percentage > 0.0 { Some(w * 100.0 / s.infill_percentage) } else { None };
    let n = regions.len();
    let infilled = par_map(&shelled, Stage::Infill, observer, |i, sh| {
        if sh.inner.is_empty() {
            return Ok(Vec::new());
        }
        // Solid is needed wherever the layers within reach above or below do not
        // all cover this area. Near the bottom or top of the stack nothing
        // covers it, so the whole layer is solid.
        let (lo, hi) = (i.checked_sub(below), i + above);
        let solid = match lo {
            Some(from) if hi < n => {
                let mut covered: Option<Region> = None;
                for j in (from..=hi).filter(|&j| j != i) {
                    let rj = regions.get(j).unwrap_or(&empty);
                    covered = Some(match covered {
                        None => rj.clone(),
                        Some(c) => c.intersection(rj)?,
                    });
                    if covered.as_ref().is_some_and(Region::is_empty) {
                        break;
                    }
                }
                match covered {
                    // no solid layers configured: nothing is forced solid
                    None => Region::new(),
                    Some(c) => sh.inner.difference(&c)?.without_slivers(w * w),
                }
            }
            _ => sh.inner.clone(),
        };
        let sparse = sh.inner.difference(&solid)?;
        let turn = s.infill_alternate && i % 2 == 1;
        let angle = if turn { base_angle + std::f64::consts::FRAC_PI_2 } else { base_angle };
        let solid_area = solid.offset(overlap)?;
        let mut paths = match s.solid_pattern {
            SolidPattern::Rectilinear => line_infill(&solid_area, w, angle, PathKind::SolidInfill)?,
            SolidPattern::Lines => line_infill(&solid_area, w, base_angle, PathKind::SolidInfill)?,
            SolidPattern::Concentric => concentric_infill(&solid, w, PathKind::SolidInfill)?,
        };
        if let Some(spacing) = sparse_spacing {
            let area = if s.infill_pattern == InfillPattern::Concentric {
                sparse
            } else {
                sparse.offset(overlap)?
            };
            let a = if s.infill_pattern == InfillPattern::Lines { base_angle } else { angle };
            paths.extend(pattern_infill(
                &area,
                s.infill_pattern,
                spacing,
                a,
                PathKind::SparseInfill,
            )?);
        }
        Ok(paths)
    })?;

    // 5. skirt and brim, around the first layer and its support
    let first = regions.first().cloned().unwrap_or_default();
    let first = if support_first.is_empty() { first } else { first.union(&support_first)? };
    let brim = if s.brim_enabled {
        rings_around(&first, 0.0, s.brim_loops, w, PathKind::Brim)?
    } else {
        Vec::new()
    };
    // a skirt goes around the brim, not through it
    let brim_reach = if s.brim_enabled { f64::from(s.brim_loops) * w } else { 0.0 };
    let skirt = if s.skirt_enabled {
        rings_around(&first, s.skirt_distance + brim_reach, s.skirt_loops, w, PathKind::Skirt)?
    } else {
        Vec::new()
    };

    // 6. order everything
    let mut at = Point2::new(0.0, 0.0);
    let mut layers = Vec::with_capacity(n);
    for (i, (slot, (sh, infill))) in slots.iter().zip(shelled.into_iter().zip(infilled)).enumerate()
    {
        observer.progress(Stage::Gcode, i, n);
        if observer.cancelled() {
            return Err(EngineError::Cancelled);
        }
        let mut groups: Vec<Vec<ToolPath>> = Vec::with_capacity(6);
        if i < s.skirt_layers as usize {
            groups.push(skirt.clone());
        }
        if i == 0 {
            groups.push(brim.clone());
        }
        // support first, so walls never wait on a column that is not there yet
        if let Some(sup) = support_toolpaths.get_mut(i) {
            groups.push(std::mem::take(sup));
        }
        let (outer, inner): (Vec<ToolPath>, Vec<ToolPath>) =
            sh.shells.into_iter().partition(|p| p.kind == PathKind::OuterPerimeter);
        let walls = match s.perimeter_order {
            PerimeterOrder::InsideOut => [inner, outer],
            PerimeterOrder::OutsideIn => [outer, inner],
        };
        let (solid, sparse): (Vec<ToolPath>, Vec<ToolPath>) =
            infill.into_iter().partition(|p| p.kind == PathKind::SolidInfill);
        if s.infill_before_perimeters {
            groups.push(solid);
            groups.push(sparse);
            groups.extend(walls);
        } else {
            groups.extend(walls);
            groups.push(solid);
            groups.push(sparse);
        }
        let seam_target = Point2::new(s.seam_x, s.seam_y);
        let mut paths = Vec::new();
        for g in groups {
            let (mut ordered, end) = order_paths(g, at);
            at = end;
            for (k, p) in ordered.iter_mut().enumerate() {
                let is_wall = matches!(p.kind, PathKind::OuterPerimeter | PathKind::InnerPerimeter);
                if !is_wall {
                    continue;
                }
                match s.seam_mode {
                    SeamMode::Nearest => {}
                    SeamMode::Aligned => start_near(p, seam_target),
                    SeamMode::Random => {
                        let n = p.points.len();
                        p.points.rotate_left(scatter(i, k, n));
                    }
                }
            }
            if s.seam_mode != SeamMode::Nearest
                && let Some(e) = ordered.last().and_then(ToolPath::end)
            {
                at = e;
            }
            paths.extend(ordered);
        }
        layers.push(LayerPaths {
            index: slot.index,
            z: slot.top,
            height: slot.height,
            paths,
            inside_travel: Vec::new(),
        });
    }

    // Travel that does not cross the model's outline (inside the part, or
    // outside it, between support pillars) needs no retraction: a string
    // there is hidden in infill or in support.
    if s.retract_only_outside {
        let flags = par_map(&layers, Stage::Gcode, observer, |i, layer| {
            let Some(area) = regions.get(i) else { return Ok(Vec::new()) };
            let mut flags = Vec::with_capacity(layer.paths.len());
            let mut prev: Option<Point2> = None;
            for p in &layer.paths {
                let inside = match (prev, p.start()) {
                    (Some(a), Some(b)) => !area.crosses_boundary(a, b),
                    _ => false,
                };
                flags.push(inside);
                prev = p.end();
            }
            Ok(flags)
        })?;
        for (layer, f) in layers.iter_mut().zip(flags) {
            layer.inside_travel = f;
        }
    }
    let out = write_gcode(&layers, s, &doc.bed);
    let mut warnings = Vec::new();
    if !s.infill_pattern.is_implemented() && s.infill_percentage > 0.0 {
        warnings.push(
            "the chosen infill pattern is not drawn yet; rectilinear was used instead".to_string(),
        );
    }
    if s.support_enabled && supported_layers == 0 {
        warnings.push(format!(
            "support is on, but nothing overhangs more than {:.0}° from vertical",
            s.support_angle
        ));
    }
    let unused = s.unused_changes();
    if !unused.is_empty() {
        warnings.push(format!(
            "{} changed setting(s) are not used by the slicer yet: {}",
            unused.len(),
            unused.join(", ")
        ));
    }
    if dropped_chains > 0 {
        warnings.push(format!(
            "{dropped_chains} open outline(s) were dropped: the model has holes or broken faces"
        ));
    }
    if layers.iter().all(|l| l.paths.is_empty()) {
        return Err(EngineError::NothingToSlice);
    }
    Ok(SliceOutput {
        layers,
        gcode: out.text,
        stats: SliceStats { gcode: out.stats, dropped_chains, warnings },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_has_first_layer_then_steps() {
        let Ok(s) = layer_schedule(0.3, 0.2, 1.0) else { return };
        // tops 0.3, 0.5, 0.7, 0.9, (1.1 cut at 1.0 is not below the top)
        assert_eq!(s.len(), 4);
        assert!((s[0].cut_z() - 0.15).abs() < 1e-12);
        assert!((s[3].top - 0.9).abs() < 1e-12);
        assert!(layer_schedule(0.3, 0.2, 0.0).is_ok_and(|v| v.is_empty()));
    }

    #[test]
    fn schedule_is_bounded() {
        assert_eq!(layer_schedule(0.01, 0.01, 1e9).err(), Some(EngineError::TooManyLayers));
    }

    #[test]
    fn par_map_keeps_order_and_reports_errors() {
        let items: Vec<usize> = (0..1000).collect();
        let r = par_map(&items, Stage::Sections, &mut Quiet, |i, v| Ok(i + v));
        assert!(r.is_ok_and(|v| v.iter().enumerate().all(|(i, x)| *x == 2 * i)));
        let r: Result<Vec<usize>, _> = par_map(&items, Stage::Sections, &mut Quiet, |i, _| {
            if i == 500 { Err(EngineError::NothingToSlice) } else { Ok(i) }
        });
        assert_eq!(r.err(), Some(EngineError::NothingToSlice));
    }

    struct CancelNow;
    impl SliceObserver for CancelNow {
        fn cancelled(&self) -> bool {
            true
        }
    }

    #[test]
    fn par_map_can_be_cancelled() {
        let items: Vec<usize> = (0..10_000).collect();
        let r = par_map(&items, Stage::Sections, &mut CancelNow, |_, v| {
            std::thread::sleep(Duration::from_micros(50));
            Ok(*v)
        });
        assert_eq!(r.err(), Some(EngineError::Cancelled));
    }
}
