//! The slicing pipeline: document → layer regions → shells → solid/sparse
//! areas → infill → ordering → G-code.
//!
//! Per-layer stages run on scoped worker threads. Workers are joined
//! explicitly, so a panic inside a dependency becomes `EngineError::Internal`
//! instead of taking the application down with the print.

use crate::EngineError;
use slicecraft_doc::{
    Document, InfillPattern, IslandOrder, PerimeterOrder, SeamMode, SolidPattern,
};
use slicecraft_geom::{Mesh, Point2, Region, Vec3};
use slicecraft_io::GcodeStats;
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
    role_meshes(doc, slicecraft_doc::ModelRole::Print)
}

/// The visible models of one role as they sit on the bed.
pub fn role_meshes(
    doc: &Document,
    role: slicecraft_doc::ModelRole,
) -> Result<Vec<Mesh>, EngineError> {
    let mut out = Vec::new();
    for o in doc.objects.iter().filter(|o| o.visible && o.role == role) {
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
/// Parallel fill lines, each island's lines joined into zigzags along its
/// edge: fewer travels and retractions, and short strokes in narrow areas
/// become one longer path.
fn linked_lines(
    region: &Region,
    spacing: f64,
    angle: f64,
    kind: PathKind,
    width: f64,
) -> Result<Vec<ToolPath>, EngineError> {
    let mut out = Vec::new();
    // dense lines get linked at the edge: they stop half a link short of it,
    // so the links add no material over unlinked lines
    let dense = spacing <= width * 1.5;
    for poly in &region.polygons {
        let one = Region { polygons: vec![poly.clone()] };
        let area = if dense { one.offset(-0.5 * width)? } else { one.clone() };
        let lines = line_infill(&area, spacing, angle, kind)?;
        // only neighbours a few line widths apart are linked: dense fill
        // becomes zigzags, sparse lines stay apart (a link there would be a
        // long extrusion along the wall)
        out.extend(slicecraft_render::paths::zigzag(lines, &one, width * 3.0));
    }
    Ok(out)
}

fn pattern_infill(
    region: &Region,
    pattern: InfillPattern,
    spacing: f64,
    angle: f64,
    kind: PathKind,
    (z, layer): (f64, usize),
    width: f64,
) -> Result<Vec<ToolPath>, EngineError> {
    use slicecraft_render::patterns;
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
        InfillPattern::Gyroid => out.extend(patterns::gyroid(region, spacing, z, kind)?),
        InfillPattern::Honeycomb => out.extend(patterns::honeycomb(region, spacing, kind)?),
        InfillPattern::Wiggle => out.extend(patterns::wiggle(region, spacing, layer, kind)?),
        InfillPattern::Cubic => out.extend(patterns::cubic(region, spacing, z, kind)?),
        InfillPattern::Octahedral => out.extend(patterns::octahedral(region, spacing, z, kind)?),
        InfillPattern::Rectilinear | InfillPattern::Lines => {
            out.extend(linked_lines(region, spacing, angle, kind, width)?);
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
            out.push(ToolPath { kind, points: p.outer.clone(), closed: true, tool: 0 });
        }
    }
    Ok(out)
}

struct Shelled {
    shells: Vec<ToolPath>,
    inner: Region,
}

/// Gap fill for one layer: single lines in thin walls (and, with gap fill on,
/// in gaps between the shells and the infill).
fn gap_fill(
    region: &Region,
    inner: &Region,
    s: &slicecraft_doc::PrintSettings,
    w: f64,
) -> Result<Vec<ToolPath>, EngineError> {
    use slicecraft_doc::ThinWalls;
    use slicecraft_render::patterns::uncovered;
    if s.thin_walls == ThinWalls::Off {
        return Ok(Vec::new());
    }
    let min_width = w * s.single_line_min_width / 100.0;
    let (thin, gaps) = uncovered(region, s.perimeter_count, w, inner, min_width)?;
    let area = if s.thin_walls == ThinWalls::GapFill { gaps } else { thin };
    let pieces = slicecraft_render::patterns::fill_narrow_with(
        &area,
        w,
        w * s.single_line_max_width / 100.0,
        PathKind::GapFill,
    )?;
    // pieces that meet end to end print as one line: fewer stops and travels
    let (open, closed): (Vec<ToolPath>, Vec<ToolPath>) =
        pieces.into_iter().partition(|p| !p.closed);
    let joined = slicecraft_render::patterns::join_polylines(
        open.into_iter().map(|p| p.points).collect(),
        w * 0.75,
    );
    let mut out = closed;
    out.extend(joined.into_iter().filter(|l| l.len() >= 2).map(|points| ToolPath {
        kind: PathKind::GapFill,
        points,
        closed: false,
        tool: 0,
    }));
    Ok(out)
}

/// Layer slots that follow height zones changing the layer height: each
/// layer takes the height the zones give at its bottom.
pub fn zoned_schedule(doc: &Document, model_top: f64) -> Result<Vec<LayerSlot>, EngineError> {
    let first = doc.settings.first_layer_height;
    let mut out = vec![LayerSlot { index: 0, top: first, height: first }];
    let mut top = first;
    while top < model_top {
        if out.len() >= MAX_LAYERS {
            return Err(EngineError::TooManyLayers);
        }
        let h = doc.settings_at(top + 1e-6).layer_height.clamp(0.01, 2.0);
        let slot = LayerSlot { index: out.len(), top: top + h, height: h };
        if slot.cut_z() >= model_top {
            break;
        }
        top += h;
        out.push(slot);
    }
    Ok(out)
}

/// Adaptive layer slots: each layer as tall as the surface allows. A surface
/// at angle θ from vertical leaves a stair-step of `h · cos θ`... measured as
/// `h · |n_z|`; each layer keeps that under a cusp height set by the quality.
pub fn adaptive_schedule(
    meshes: &[Mesh],
    s: &slicecraft_doc::PrintSettings,
    model_top: f64,
) -> Result<Vec<LayerSlot>, EngineError> {
    const BIN: f64 = 0.05;
    let (min_h, max_h) = (s.adaptive_min_height.min(s.adaptive_max_height), s.adaptive_max_height);
    let q = f64::from(s.adaptive_quality.clamp(1, 10));
    let cusp = max_h * 0.5 * (11.0 - q) / 10.0;
    let bins = ((model_top / BIN).ceil() as usize).saturating_add(2);
    if bins > MAX_LAYERS * 40 {
        return Err(EngineError::TooManyLayers);
    }
    // the tallest layer each height band allows
    let mut allowed = vec![max_h; bins];
    for m in meshes {
        for t in m.triangles() {
            let Some(n) = t.normal() else { continue };
            let nz = n.z.abs();
            if nz < 1e-3 {
                continue;
            }
            let h = (cusp / nz).clamp(min_h, max_h);
            let (lo, hi) =
                ((t.min_z() / BIN).floor().max(0.0) as usize, (t.max_z() / BIN).ceil() as usize);
            for b in allowed.iter_mut().take(hi.min(bins)).skip(lo) {
                if h < *b {
                    *b = h;
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut top = s.first_layer_height;
    out.push(LayerSlot { index: 0, top, height: s.first_layer_height });
    while top < model_top {
        if out.len() >= MAX_LAYERS {
            return Err(EngineError::TooManyLayers);
        }
        let (lo, hi) = ((top / BIN).floor() as usize, ((top + max_h) / BIN).ceil() as usize);
        let h = allowed.iter().take(hi.min(bins)).skip(lo).fold(max_h, |a, b| a.min(*b)).max(min_h);
        let slot = LayerSlot { index: out.len(), top: top + h, height: h };
        if slot.cut_z() >= model_top {
            break;
        }
        top += h;
        out.push(slot);
    }
    Ok(out)
}

/// Size compensation: outer contours grow by `outer`, holes by `hole`.
fn compensate(region: Region, outer: f64, hole: f64) -> Result<Region, EngineError> {
    if outer == 0.0 && hole == 0.0 {
        return Ok(region);
    }
    let outers = Region {
        polygons: region
            .polygons
            .iter()
            .map(|p| slicecraft_geom::Polygon { outer: p.outer.clone(), holes: Vec::new() })
            .collect(),
    }
    .union(&Region::new())?;
    let holes = Region::from_loops(
        &region
            .polygons
            .iter()
            .flat_map(|p| p.holes.iter().map(|h| h.iter().rev().copied().collect::<Vec<_>>()))
            .collect::<Vec<_>>(),
    )?;
    Ok(outers.offset(outer)?.difference(&holes.offset(hole)?)?)
}

/// Layer slots, each layer's part region, the open loops dropped while
/// stitching, each layer's region of models used as support, and the open
/// outlines kept to print as lines.
struct Sections {
    slots: Vec<LayerSlot>,
    regions: Vec<Region>,
    dropped_chains: usize,
    role_support: Vec<Region>,
    open: Vec<Vec<Vec<Point2>>>,
}

fn fill_mode(rule: slicecraft_doc::FillRule) -> slicecraft_geom::region::FillMode {
    use slicecraft_doc::FillRule;
    use slicecraft_geom::region::FillMode;
    match rule {
        FillRule::Merge => FillMode::Merge,
        FillRule::Alternate => FillMode::Alternate,
        FillRule::Outlines => FillMode::Outlines,
    }
}

/// The layer schedule and each layer's model region, plus the open loops
/// dropped while stitching and the support-role regions.
fn layer_regions(
    doc: &Document,
    observer: &mut dyn SliceObserver,
) -> Result<Sections, EngineError> {
    doc.sliceable()?;
    let s = &doc.settings;
    let meshes = bed_meshes(doc)?;
    let cutters = role_meshes(doc, slicecraft_doc::ModelRole::Cutter)?;
    let supports = role_meshes(doc, slicecraft_doc::ModelRole::Support)?;
    let top = meshes.iter().chain(&supports).map(|m| m.bounds().max().z).fold(0.0f64, f64::max);
    let slots = if s.adaptive_layers {
        adaptive_schedule(&meshes, s, top)?
    } else if doc
        .zones
        .iter()
        .any(|z| z.objects.is_empty() && z.overrides.contains_key("layer_height"))
    {
        zoned_schedule(doc, top)?
    } else {
        layer_schedule(s.first_layer_height, s.layer_height, top)?
    };
    if slots.is_empty() {
        return Err(EngineError::NothingToSlice);
    }
    use slicecraft_doc::OpenOutlines;
    let close_gap = match s.open_outline_mode {
        OpenOutlines::Drop => 0.0,
        OpenOutlines::CloseGaps | OpenOutlines::Lines => s.slice_close_gap,
    };
    let keep_open = s.open_outline_mode == OpenOutlines::Lines;
    let mode = fill_mode(s.fill_rule);
    let sections = par_map(&slots, Stage::Sections, observer, |_, slot| {
        let mut loops = Vec::new();
        let mut dropped = 0usize;
        let mut open = Vec::new();
        for m in &meshes {
            let layer =
                slicecraft_render::cross_section_with(m, slot.cut_z(), close_gap, keep_open);
            dropped += layer.dropped_chains;
            loops.extend(layer.loops.into_iter().map(|l| l.points));
            open.extend(layer.open);
        }
        let region = compensate(
            Region::from_loops_mode(&loops, mode)?,
            s.outer_size_offset,
            s.hole_size_offset,
        )?;
        Ok((region, dropped, open))
    })?;
    let dropped_chains: usize = sections.iter().map(|(_, d, _)| d).sum();
    let mut open_lines = Vec::with_capacity(sections.len());
    let mut regions: Vec<Region> = Vec::with_capacity(sections.len());
    for (r, _, o) in sections {
        regions.push(r);
        open_lines.push(o);
    }
    // models of the other roles, cut at the same heights
    let section_of = |meshes: &[Mesh], z: f64| -> Result<Region, EngineError> {
        let mut loops = Vec::new();
        for m in meshes {
            loops.extend(cross_section(m, z).loops.into_iter().map(|l| l.points));
        }
        Ok(Region::from_loops(&loops)?)
    };
    if !cutters.is_empty() {
        let cut = par_map(&slots, Stage::Sections, observer, |_, slot| {
            section_of(&cutters, slot.cut_z())
        })?;
        for (r, c) in regions.iter_mut().zip(cut) {
            if !c.is_empty() {
                *r = r.difference(&c)?;
            }
        }
    }
    let role_support = if supports.is_empty() {
        Vec::new()
    } else {
        par_map(&slots, Stage::Sections, observer, |_, slot| section_of(&supports, slot.cut_z()))?
    };
    Ok(Sections { slots, regions, dropped_chains, role_support, open: open_lines })
}

/// One layer of a support preview.
#[derive(Debug, Clone)]
pub struct SupportPreviewLayer {
    pub slot: LayerSlot,
    /// Everything the layer's support covers.
    pub area: Region,
}

/// The support a slice would print, without slicing the rest: what the
/// prepare view shows before slicing.
pub fn support_preview(
    doc: &Document,
    observer: &mut dyn SliceObserver,
) -> Result<Vec<SupportPreviewLayer>, EngineError> {
    let Sections { slots, regions, .. } = layer_regions(doc, observer)?;
    let s = &doc.settings;
    let layers = crate::support::support_layers(
        &regions,
        &slots,
        s,
        s.line_width(),
        &doc.support_edits,
        observer,
    )?;
    let mut out = Vec::new();
    for (slot, l) in slots.into_iter().zip(layers) {
        if !l.is_empty() {
            out.push(SupportPreviewLayer { slot, area: l.area()? });
        }
    }
    Ok(out)
}

/// A raft: its layers, ready to print, and its outline.
struct Raft {
    layers: Vec<(f64, Vec<ToolPath>)>,
    footprint: Region,
    /// Total thickness plus the air gap: how far the part is lifted.
    lift: f64,
}

/// A raft under `first` (the part's first layer and its support): base layers
/// at the raft density, then solid top layers turned a quarter each layer,
/// each with an outline.
fn raft_layers(
    first: &Region,
    s: &slicecraft_doc::PrintSettings,
    w: f64,
) -> Result<Raft, EngineError> {
    use slicecraft_render::paths::zigzag;
    // the margin, with holes filled: a raft is one solid plate
    let grown = first.offset(s.raft_margin.max(w))?;
    let footprint = Region {
        polygons: grown
            .polygons
            .into_iter()
            .map(|p| slicecraft_geom::Polygon { outer: p.outer, holes: Vec::new() })
            .collect(),
    }
    .union(&Region::new())?;
    let mut layers = Vec::new();
    let mut z = 0.0;
    let count = s.raft_base_layers + s.raft_top_layers;
    for k in 0..count {
        let base = k < s.raft_base_layers;
        let h = if k == 0 { s.first_layer_height } else { s.layer_height };
        z += h;
        let density = if base { s.raft_infill.max(1.0) } else { 100.0 };
        let spacing = w * 100.0 / density;
        let angle =
            if k % 2 == 0 { std::f64::consts::FRAC_PI_4 } else { -std::f64::consts::FRAC_PI_4 };
        let (outline, inner) = perimeters(&footprint, 1, w)?;
        let mut paths: Vec<ToolPath> =
            outline.into_iter().map(|p| ToolPath { kind: PathKind::Raft, ..p }).collect();
        for poly in &inner.offset(w * 0.15)?.polygons {
            let one = Region { polygons: vec![poly.clone()] };
            let lines = line_infill(&one, spacing, angle, PathKind::Raft)?;
            paths.extend(zigzag(lines, &one, spacing * 2.0));
        }
        layers.push((z, paths));
    }
    Ok(Raft { layers, footprint, lift: z + s.raft_gap })
}

/// Put the raft under the part's layers: raft layers first (the skirt on the
/// first), then every part layer lifted by the raft's thickness and gap.
fn with_raft(raft: Raft, layers: Vec<LayerPaths>, skirt: Vec<ToolPath>) -> Vec<LayerPaths> {
    let mut out = Vec::with_capacity(raft.layers.len() + layers.len());
    let mut below = 0.0;
    for (k, (z, mut paths)) in raft.layers.into_iter().enumerate() {
        if k == 0 {
            let mut first = skirt.clone();
            first.append(&mut paths);
            paths = first;
        }
        out.push(LayerPaths {
            index: k,
            z,
            height: z - below,
            paths,
            inside_travel: Vec::new(),
            travel_via: Vec::new(),
        });
        below = z;
    }
    let offset = out.len();
    for mut l in layers {
        l.index += offset;
        l.z += raft.lift;
        out.push(l);
    }
    out
}

/// The tool a set of settings prints with.
fn tool_of(s: &slicecraft_doc::PrintSettings) -> u8 {
    u8::try_from(s.tool_number.min(15)).unwrap_or(0)
}

/// Group each layer's paths into one run per tool (the tool that ended the
/// layer below first, so a layer changes tools as few times as it can), and
/// on every layer up to the last one that changes tools, print the ooze
/// shield and a share of the prime tower before each run.
fn arrange_tools(
    layers: &mut [LayerPaths],
    regions: &[Region],
    slots: &[LayerSlot],
    s: &slicecraft_doc::PrintSettings,
    w: f64,
    base_tool: u8,
) -> Result<(), EngineError> {
    let tools_on = |l: &LayerPaths| {
        let mut t: Vec<u8> = l.paths.iter().map(|p| p.tool).collect();
        t.sort_unstable();
        t.dedup();
        t
    };
    let single = |t: &Vec<u8>| t.is_empty() || t.as_slice() == [base_tool];
    let Some(last_multi) = layers.iter().rposition(|l| !single(&tools_on(l))) else {
        return Ok(());
    };
    // the shield, from the top down: around this layer's part, and leaning
    // in at most the shield's slope from the shield above
    let mut shields: Vec<Region> = vec![Region::new(); last_multi + 1];
    if s.ooze_shield_enabled {
        let slope = s.ooze_shield_angle.clamp(0.0, 89.0).to_radians().tan();
        let mut above: Option<Region> = None;
        for i in (0..=last_multi).rev() {
            let own = match regions.get(i) {
                Some(r) if !r.is_empty() => r.offset(s.ooze_shield_distance)?,
                _ => Region::new(),
            };
            let here = match &above {
                Some(a) => {
                    let h = slots.get(i + 1).map_or(s.layer_height, |sl| sl.height);
                    let leaned = if slope > 0.0 { a.offset(-h * slope)? } else { a.clone() };
                    if own.is_empty() { leaned } else { own.union(&leaned)? }
                }
                None => own,
            };
            if let Some(slot) = shields.get_mut(i) {
                *slot = here.clone();
            }
            above = Some(here);
        }
    }
    let half = s.prime_tower_size / 2.0;
    let (cx, cy) = (s.prime_tower_x, s.prime_tower_y);
    let tower_part =
        |j: usize, k: usize, i: usize, tool: u8| -> Result<Vec<ToolPath>, EngineError> {
            let k = k.max(1) as f64;
            let x0 = cx - half + 2.0 * half * j as f64 / k;
            let x1 = cx - half + 2.0 * half * (j as f64 + 1.0) / k;
            let rect = vec![
                Point2::new(x0, cy - half),
                Point2::new(x1, cy - half),
                Point2::new(x1, cy + half),
                Point2::new(x0, cy + half),
            ];
            let area = Region::from_loops(&[rect])?;
            let angle = if i.is_multiple_of(2) {
                std::f64::consts::FRAC_PI_4
            } else {
                -std::f64::consts::FRAC_PI_4
            };
            let mut out: Vec<ToolPath> = Vec::new();
            if j == 0 {
                // one wall around the whole tower keeps it straight
                let ring = vec![
                    Point2::new(cx - half, cy - half),
                    Point2::new(cx + half, cy - half),
                    Point2::new(cx + half, cy + half),
                    Point2::new(cx - half, cy + half),
                ];
                out.push(ToolPath { kind: PathKind::PrimeTower, points: ring, closed: true, tool });
            }
            let inner = area.offset(-0.5 * w)?;
            out.extend(
                line_infill(&inner, w, angle, PathKind::PrimeTower)?
                    .into_iter()
                    .map(|p| ToolPath { tool, ..p }),
            );
            Ok(out)
        };
    let mut current = layers.first().and_then(|l| l.paths.first()).map_or(base_tool, |p| p.tool);
    for (i, layer) in layers.iter_mut().enumerate() {
        let mut tools = tools_on(layer);
        if tools.is_empty() {
            continue;
        }
        if let Some(pos) = tools.iter().position(|&t| t == current) {
            let t = tools.remove(pos);
            tools.insert(0, t);
        }
        let paths = std::mem::take(&mut layer.paths);
        let mut out = Vec::with_capacity(paths.len() + 16);
        for (j, &t) in tools.iter().enumerate() {
            if i <= last_multi {
                if j == 0
                    && let Some(shield) = shields.get(i)
                {
                    for poly in &shield.polygons {
                        out.push(ToolPath {
                            kind: PathKind::OozeShield,
                            points: poly.outer.clone(),
                            closed: true,
                            tool: t,
                        });
                    }
                }
                if s.prime_tower_enabled {
                    out.extend(tower_part(j, tools.len(), i, t)?);
                }
            }
            out.extend(paths.iter().filter(|p| p.tool == t).cloned());
        }
        current = tools.last().copied().unwrap_or(current);
        layer.paths = out;
    }
    Ok(())
}

/// One piece of slicing work: a layer, or the part of a layer that a model
/// zone covers, with the settings that apply there.
struct Job {
    layer: usize,
    region: Region,
    settings: slicecraft_doc::PrintSettings,
}

/// Split each layer into jobs: the area of each model zone's models (their
/// own cross-sections) gets the zone's settings, the rest the layer's.
fn layer_jobs(
    doc: &Document,
    slots: &[LayerSlot],
    regions: &[Region],
    layer_settings: &[slicecraft_doc::PrintSettings],
    observer: &mut dyn SliceObserver,
) -> Result<Vec<Job>, EngineError> {
    let model_zones: Vec<&slicecraft_doc::SettingZone> =
        doc.zones.iter().filter(|z| !z.objects.is_empty()).collect();
    // each model zone's footprint on every layer
    let mut footprints: Vec<Vec<Region>> = Vec::with_capacity(model_zones.len());
    for zone in &model_zones {
        let mut meshes = Vec::new();
        for &i in &zone.objects {
            if let Some(o) = doc.objects.get(i).filter(|o| o.visible) {
                let world = o.world_mesh()?;
                let drop = world.bounds().min().z;
                meshes.push(if drop == 0.0 {
                    world
                } else {
                    world.map_vertices(|v| v - Vec3::new(0.0, 0.0, drop))?
                });
            }
        }
        footprints.push(par_map(slots, Stage::Sections, observer, |_, slot| {
            if !zone.covers_z(slot.cut_z()) {
                return Ok(Region::new());
            }
            let mut loops = Vec::new();
            for m in &meshes {
                loops.extend(cross_section(m, slot.cut_z()).loops.into_iter().map(|l| l.points));
            }
            Ok(Region::from_loops(&loops)?.offset(0.05)?)
        })?);
    }
    let mut jobs = Vec::with_capacity(regions.len());
    for (i, region) in regions.iter().enumerate() {
        let base = layer_settings.get(i).cloned().unwrap_or_else(|| doc.settings.clone());
        let mut rest = region.clone();
        for (zone, fp) in model_zones.iter().zip(&footprints) {
            let Some(fp) = fp.get(i).filter(|f| !f.is_empty()) else { continue };
            let part = rest.intersection(fp)?;
            if part.is_empty() {
                continue;
            }
            rest = rest.difference(&part)?;
            jobs.push(Job { layer: i, region: part, settings: zone.apply(&base) });
        }
        jobs.push(Job { layer: i, region: rest, settings: base });
    }
    Ok(jobs)
}

fn point_segment_distance(p: Point2, a: Point2, b: Point2) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(Point2::new(a.x + t * dx, a.y + t * dy))
}

/// Distance from `p` to a path's line, open or closed.
fn path_distance(p: Point2, path: &ToolPath) -> f64 {
    let n = path.points.len();
    let segs = if path.closed { n } else { n.saturating_sub(1) };
    let mut best = path.points.first().map_or(f64::INFINITY, |q| q.distance(p));
    for i in 0..segs {
        if let (Some(a), Some(b)) = (path.points.get(i), path.points.get((i + 1) % n)) {
            best = best.min(point_segment_distance(p, *a, *b));
        }
    }
    best
}

/// Walls in sets: every inner wall joins the outer wall of the outline it
/// follows, and each set is printed whole (inside first or outside first)
/// before travelling to the nearest next set. Fewer hops between letters or
/// holes mean fewer retractions.
fn order_walls(
    paths: Vec<ToolPath>,
    from: Point2,
    inside_first: bool,
    crosses: &dyn Fn(Point2, Point2) -> bool,
) -> (Vec<ToolPath>, Point2) {
    if paths.len() > 20_000 {
        return order_paths(paths, from);
    }
    let (gaps, paths): (Vec<ToolPath>, Vec<ToolPath>) =
        paths.into_iter().partition(|p| p.kind == PathKind::GapFill);
    let (outers, inners): (Vec<ToolPath>, Vec<ToolPath>) =
        paths.into_iter().partition(|p| p.kind == PathKind::OuterPerimeter);
    // outline boxes to find candidates quickly
    let boxes: Vec<(f64, f64, f64, f64)> = outers
        .iter()
        .map(|o| {
            o.points.iter().fold(
                (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
                |b, q| (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y)),
            )
        })
        .collect();
    let mut sets: Vec<(Vec<ToolPath>, Vec<ToolPath>, Vec<ToolPath>)> =
        outers.into_iter().map(|o| (Vec::new(), vec![o], Vec::new())).collect();
    let mut loose: Vec<(Vec<ToolPath>, Vec<ToolPath>, Vec<ToolPath>)> = Vec::new();
    let tagged = inners.into_iter().map(|p| (p, false)).chain(gaps.into_iter().map(|p| (p, true)));
    for (w, is_gap) in tagged {
        let Some(at) = w.start() else { continue };
        let mut best: Option<(f64, usize)> = None;
        for (k, b) in boxes.iter().enumerate() {
            // an inner wall lies inside its outline's box, or close to it
            let reach = 5.0;
            if at.x < b.0 - reach || at.x > b.2 + reach || at.y < b.1 - reach || at.y > b.3 + reach
            {
                continue;
            }
            let Some((_, outer, _)) = sets.get(k) else { continue };
            let d = outer.first().map_or(f64::INFINITY, |o| path_distance(at, o));
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, k));
            }
        }
        match (best.and_then(|(_, k)| sets.get_mut(k)), is_gap) {
            (Some(set), false) => set.0.push(w),
            (Some(set), true) => set.2.push(w),
            (None, false) => loose.push((vec![w], Vec::new(), Vec::new())),
            (None, true) => loose.push((Vec::new(), Vec::new(), vec![w])),
        }
    }
    sets.extend(loose);
    // the nearest point of a set, and its distance
    let near = |set: &(Vec<ToolPath>, Vec<ToolPath>, Vec<ToolPath>), at: Point2| {
        set.0
            .iter()
            .chain(set.1.iter())
            .chain(set.2.iter())
            .flat_map(|p| p.points.iter())
            .map(|q| (q.distance_sq(at), *q))
            .fold((f64::INFINITY, at), |a, b| if b.0 < a.0 { b } else { a })
    };
    let mut out = Vec::new();
    let mut at = from;
    while !sets.is_empty() {
        // among the nearest few sets, one reachable without leaving the part
        let mut ranked: Vec<(f64, usize, Point2)> = sets
            .iter()
            .enumerate()
            .map(|(i, set)| {
                let (d, q) = near(set, at);
                (d, i, q)
            })
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut k = ranked.first().map_or(0, |r| r.1);
        let mut best = f64::INFINITY;
        for (d, i, q) in ranked.into_iter().take(6) {
            let cost = d.sqrt() + if crosses(at, q) { 10.0 } else { 0.0 };
            if cost < best {
                best = cost;
                k = i;
            }
        }
        let (ins, outs, fill) = sets.swap_remove(k);
        let (first, second) = if inside_first { (ins, outs) } else { (outs, ins) };
        let (a, end) = order_paths(first, at);
        let (b, end) = order_paths(second, end);
        let (c, end) = order_paths(fill, end);
        out.extend(a);
        out.extend(b);
        out.extend(c);
        at = end;
    }
    (out, at)
}

/// Direction of a polygon's long axis, radians, from its outline's
/// edge-weighted second moments.
fn long_axis(ring: &[Point2]) -> f64 {
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
        return 0.0;
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
    0.5 * (2.0 * sxy).atan2(sxx - syy)
}

/// A layer's model paths as feature groups in print order: walls (in the
/// profile's perimeter order), solid infill, sparse infill, or infill first.
fn feature_groups(paths: Vec<ToolPath>, s: &slicecraft_doc::PrintSettings) -> Vec<Vec<ToolPath>> {
    let (mut outer, mut inner, mut solid, mut sparse) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut gaps, mut bridges, mut tops, mut irons) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for p in paths {
        match p.kind {
            PathKind::OuterPerimeter => outer.push(p),
            PathKind::InnerPerimeter => inner.push(p),
            PathKind::GapFill => gaps.push(p),
            PathKind::Bridge => bridges.push(p),
            PathKind::TopSolid => tops.push(p),
            PathKind::Ironing => irons.push(p),
            PathKind::SolidInfill => solid.push(p),
            _ => sparse.push(p),
        }
    }
    // inner and outer walls go together: each outline's walls are printed as
    // a set before the next outline's (see `order_walls`)
    let mut together = inner;
    together.extend(outer);
    let walls = [together, Vec::new()];
    let mut groups = Vec::with_capacity(5);
    if s.infill_before_perimeters {
        groups.push(solid);
        groups.push(sparse);
        groups.extend(walls);
        groups.push(gaps);
    } else {
        groups.extend(walls);
        groups.push(gaps);
        groups.push(solid);
        groups.push(sparse);
    }
    // bridges after the walls they anchor on; top surfaces and ironing last
    groups.push(bridges);
    groups.push(tops);
    groups.push(irons);
    groups.retain(|g| !g.is_empty());
    groups
}

/// Split paths by the island (polygon of the layer's region) they start in.
/// A path that starts in no island joins the nearest one by bounding box.
fn split_islands(paths: Vec<ToolPath>, region: &Region) -> Vec<Vec<ToolPath>> {
    let islands: Vec<(Region, slicecraft_geom::Bounds2)> = region
        .polygons
        .iter()
        .filter_map(|p| {
            let one = Region { polygons: vec![p.clone()] };
            let b = one.bounds()?;
            Some((one, b))
        })
        .collect();
    if islands.len() <= 1 {
        return vec![paths];
    }
    let mut out: Vec<Vec<ToolPath>> = vec![Vec::new(); islands.len()];
    for p in paths {
        let Some(at) = p.start() else { continue };
        let inside_box = |b: &slicecraft_geom::Bounds2| {
            at.x >= b.min_x && at.x <= b.max_x && at.y >= b.min_y && at.y <= b.max_y
        };
        let candidates: Vec<usize> = islands
            .iter()
            .enumerate()
            .filter(|(_, (_, b))| inside_box(b))
            .map(|(k, _)| k)
            .collect();
        let chosen = match candidates.as_slice() {
            [only] => *only,
            many if !many.is_empty() => many
                .iter()
                .copied()
                .find(|k| islands.get(*k).is_some_and(|(r, _)| r.contains(at)))
                .unwrap_or(many.first().copied().unwrap_or(0)),
            _ => {
                // outside every box: the nearest box
                let dist = |b: &slicecraft_geom::Bounds2| {
                    let dx = (b.min_x - at.x).max(at.x - b.max_x).max(0.0);
                    let dy = (b.min_y - at.y).max(at.y - b.max_y).max(0.0);
                    dx * dx + dy * dy
                };
                islands
                    .iter()
                    .enumerate()
                    .min_by(|a, b| dist(&a.1.1).total_cmp(&dist(&b.1.1)))
                    .map_or(0, |(k, _)| k)
            }
        };
        if let Some(bucket) = out.get_mut(chosen) {
            bucket.push(p);
        }
    }
    out.retain(|g| !g.is_empty());
    out
}

/// The island with a path starting nearest `at`.
fn nearest_island(islands: &[Vec<Vec<ToolPath>>], at: Point2) -> usize {
    let mut best = (f64::INFINITY, 0);
    for (k, groups) in islands.iter().enumerate() {
        for p in groups.iter().flatten() {
            for q in [p.start(), p.end()].into_iter().flatten() {
                let d = q.distance_sq(at);
                if d < best.0 {
                    best = (d, k);
                }
            }
        }
    }
    best.1
}

/// Sort key for "same order every layer": left to right, then front to back.
fn island_key(groups: &[Vec<ToolPath>]) -> f64 {
    let mut min = (f64::INFINITY, f64::INFINITY);
    for p in groups.iter().flatten() {
        for q in &p.points {
            if (q.x, q.y) < min {
                min = (q.x, q.y);
            }
        }
    }
    (min.0 / 5.0).floor() * 1e6 + min.1
}

/// Slice a document into toolpaths and G-code.
/// Toolpaths for every layer, before G-code: the layers, each layer's
/// settings, the slice notes and how many open outlines were dropped.
struct Layered {
    layers: Vec<LayerPaths>,
    settings: Vec<slicecraft_doc::PrintSettings>,
    warnings: Vec<String>,
    dropped_chains: usize,
}

pub fn slice_document(
    doc: &Document,
    observer: &mut dyn SliceObserver,
) -> Result<SliceOutput, EngineError> {
    let s = &doc.settings;
    let parts = doc
        .objects
        .iter()
        .filter(|o| o.visible && o.role == slicecraft_doc::ModelRole::Print)
        .count();
    let l = if s.sequential_printing && parts > 1 {
        sequential(doc, observer)?
    } else {
        slice_layers(doc, observer)?
    };
    let per_layer: Vec<&slicecraft_doc::PrintSettings> = l.settings.iter().collect();
    let out = slicecraft_io::gcode::write_gcode_layers(&l.layers, s, &per_layer, &doc.bed);
    Ok(SliceOutput {
        layers: l.layers,
        gcode: out.text,
        stats: SliceStats {
            gcode: out.stats,
            dropped_chains: l.dropped_chains,
            warnings: l.warnings,
        },
    })
}

/// Sequential printing: each part sliced alone, printed to its top before
/// the next. Refused when the parts stand too close for the head or a part
/// other than the last is taller than the gantry clearance.
fn sequential(doc: &Document, observer: &mut dyn SliceObserver) -> Result<Layered, EngineError> {
    use slicecraft_doc::ModelRole;
    let s = &doc.settings;
    let parts: Vec<usize> = doc
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.visible && o.role == ModelRole::Print)
        .map(|(i, _)| i)
        .collect();
    // footprints and heights, as placed
    let mut boxes = Vec::with_capacity(parts.len());
    for &i in &parts {
        let Some(o) = doc.objects.get(i) else { continue };
        boxes.push((i, o.world_mesh()?.bounds()));
    }
    // shortest first, so the tallest goes last and may rise above the gantry
    boxes.sort_by(|a, b| a.1.size().z.total_cmp(&b.1.size().z));
    for (k, (i, b)) in boxes.iter().enumerate() {
        if k + 1 < boxes.len() && b.size().z > s.gantry_height {
            return Err(EngineError::File(format!(
                "one model at a time: model {i} is {:.1} mm tall, above the {:.1} mm gantry clearance; only the last model may be taller",
                b.size().z,
                s.gantry_height
            )));
        }
        for (j, c) in boxes.iter().skip(k + 1) {
            let dx = (c.min().x - b.max().x).max(b.min().x - c.max().x).max(0.0);
            let dy = (c.min().y - b.max().y).max(b.min().y - c.max().y).max(0.0);
            if dx.hypot(dy) < s.head_clearance {
                return Err(EngineError::File(format!(
                    "one model at a time: models {i} and {j} stand {:.1} mm apart; the head needs {:.1} mm",
                    dx.hypot(dy),
                    s.head_clearance
                )));
            }
        }
    }
    let mut out = Layered {
        layers: Vec::new(),
        settings: Vec::new(),
        warnings: Vec::new(),
        dropped_chains: 0,
    };
    for (k, (i, _)) in boxes.iter().enumerate() {
        // this part alone, with the models that cut it; zones that name it follow it
        let mut sub = doc.clone();
        let mut keep = Vec::new();
        for (j, o) in doc.objects.iter().enumerate() {
            let take = j == *i
                || (o.visible && o.role == ModelRole::Cutter)
                || (k == 0 && o.visible && o.role == ModelRole::Support);
            if take {
                keep.push(j);
            }
        }
        sub.objects = keep.iter().filter_map(|&j| doc.objects.get(j).cloned()).collect();
        sub.zones = doc
            .zones
            .iter()
            .filter_map(|z| {
                let mut z = z.clone();
                if z.objects.is_empty() {
                    return Some(z);
                }
                z.objects =
                    z.objects.iter().filter_map(|o| keep.iter().position(|j| j == o)).collect();
                (!z.objects.is_empty()).then_some(z)
            })
            .collect();
        // only the first part gets a skirt
        if k > 0 {
            sub.settings.skirt_enabled = false;
        }
        let mut l = slice_layers(&sub, observer)?;
        out.layers.append(&mut l.layers);
        out.settings.append(&mut l.settings);
        out.dropped_chains += l.dropped_chains;
        for w in l.warnings {
            if !out.warnings.contains(&w) {
                out.warnings.push(w);
            }
        }
    }
    Ok(out)
}

fn slice_layers(doc: &Document, observer: &mut dyn SliceObserver) -> Result<Layered, EngineError> {
    let s = &doc.settings;
    let w = s.line_width();
    // 1. regions
    let Sections { slots, regions, dropped_chains, role_support, open: open_lines } =
        layer_regions(doc, observer)?;

    // 2. support, from the model's layers
    let support_areas =
        crate::support::support_layers(&regions, &slots, s, w, &doc.support_edits, observer)?;
    // models used as support print as support, where they stand
    let mut support_areas = support_areas;
    for (layer, extra) in support_areas.iter_mut().zip(&role_support) {
        if !extra.is_empty() {
            layer.body = layer.body.union(extra)?;
        }
    }
    let support_first = match support_areas.first() {
        Some(l) if !l.is_empty() => l.area()?,
        _ => Region::new(),
    };
    let supported_layers = support_areas.iter().filter(|l| !l.is_empty()).count();
    let mut support_toolpaths = par_map(&support_areas, Stage::Support, observer, |i, layer| {
        crate::support::support_paths(layer, i, s, w)
    })?;
    drop(support_areas);
    let base_tool = tool_of(s);
    for p in support_toolpaths.iter_mut().flatten() {
        p.tool = base_tool;
    }

    // 3. per-layer settings (height zones) and the parts of each layer that
    // model zones give their own settings
    let layer_settings: Vec<slicecraft_doc::PrintSettings> =
        slots.iter().map(|sl| doc.settings_at(sl.cut_z())).collect();
    let jobs = layer_jobs(doc, &slots, &regions, &layer_settings, observer)?;

    // shells, per job
    let shelled = par_map(&jobs, Stage::Shells, observer, |_, job| {
        let (i, region, s) = (job.layer, &job.region, &job.settings);
        let w = slicecraft_io::gcode::layer_line_width(s, i);
        let (mut shells, inner) = perimeters(region, s.perimeter_count, w)?;
        shells.extend(gap_fill(region, &inner, s, w)?);
        Ok(Shelled { shells, inner })
    })?;

    // 4. solid vs sparse, infill
    let empty = Region::new();
    let n = regions.len();
    let infilled = par_map(&shelled, Stage::Infill, observer, |k, sh| {
        let Some(job) = jobs.get(k) else { return Ok(Vec::new()) };
        let (i, s) = (job.layer, &job.settings);
        let (below, above) = (s.solid_layers_bottom as usize, s.solid_layers_top as usize);
        let base_angle = s.infill_angle.to_radians();
        let w = slicecraft_io::gcode::layer_line_width(s, i);
        let overlap = s.infill_overlap / 100.0 * w;
        let infill_w = w * s.infill_width_percent / 100.0;
        let sparse_spacing = if s.infill_percentage > 0.0 {
            Some(infill_w * 100.0 / s.infill_percentage)
        } else {
            None
        };
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
        // reinforcement: whole solid layers at a fixed interval
        let every = s.solid_every_layers as usize;
        let solid = if every > 0 && i > 0 && i % every < s.solid_every_count.max(1) as usize {
            sh.inner.clone()
        } else {
            solid
        };
        let mut solid = solid;
        let mut sparse = sh.inner.difference(&solid)?;
        // small sparse islands are stronger solid
        if s.solid_min_area > 0.0 && !sparse.is_empty() {
            let small = Region {
                polygons: sparse
                    .polygons
                    .iter()
                    .filter(|p| p.area() < s.solid_min_area)
                    .cloned()
                    .collect(),
            };
            if !small.is_empty() {
                solid = solid.union(&small)?;
                sparse = sparse.difference(&small)?;
            }
        }
        if s.solid_expansion > 0.0 && !solid.is_empty() {
            solid = solid.offset(s.solid_expansion)?.intersection(&sh.inner)?;
            sparse = sh.inner.difference(&solid)?;
        }
        let turn = s.infill_alternate && i % 2 == 1;
        let angle = if turn { base_angle + std::f64::consts::FRAC_PI_2 } else { base_angle };
        let solid_angle = angle + s.solid_angle_offset.to_radians();
        let solid_base = base_angle + s.solid_angle_offset.to_radians();
        // solid splits into bridges (air below), top surfaces (air above) and the rest
        let mut bridge = Region::new();
        if s.bridging_enabled && i > 0 && !solid.is_empty() {
            let below = regions.get(i - 1).unwrap_or(&empty);
            bridge = solid.difference(below)?.without_slivers(4.0 * w * w);
        }
        let top = if solid.is_empty() {
            Region::new()
        } else {
            match regions.get(i + 1) {
                Some(up) => solid.difference(up)?.difference(&bridge)?.without_slivers(w * w),
                None => solid.difference(&bridge)?,
            }
        };
        let rest = solid.difference(&bridge)?.difference(&top)?;
        let solid_fill = |area: &Region, kind: PathKind| -> Result<Vec<ToolPath>, EngineError> {
            let grown = area.offset(overlap)?;
            Ok(match s.solid_pattern {
                SolidPattern::Rectilinear => linked_lines(&grown, w, solid_angle, kind, w)?,
                SolidPattern::Lines => linked_lines(&grown, w, solid_base, kind, w)?,
                SolidPattern::Concentric => concentric_infill(area, w, kind)?,
            })
        };
        let mut paths = solid_fill(&rest, PathKind::SolidInfill)?;
        paths.extend(solid_fill(&top, PathKind::TopSolid)?);
        for poly in &bridge.polygons {
            // across the gap: perpendicular to the bridge's long axis
            let one = Region { polygons: vec![poly.clone()] };
            let across = long_axis(&poly.outer) + std::f64::consts::FRAC_PI_2;
            paths.extend(line_infill(
                &one.offset(overlap + w)?.intersection(&sh.inner.offset(overlap)?)?,
                w,
                across,
                PathKind::Bridge,
            )?);
        }
        if s.ironing_enabled && !top.is_empty() {
            let area = top.offset(-0.5 * w)?;
            paths.extend(line_infill(
                &area,
                s.ironing_spacing,
                angle + std::f64::consts::FRAC_PI_4,
                PathKind::Ironing,
            )?);
        }
        // denser infill just under top surfaces
        if s.dense_layers > 0 && !sparse.is_empty() {
            let first_above = i + above + 1;
            let mut covered: Option<Region> = None;
            for j in first_above..first_above + s.dense_layers as usize {
                let rj = regions.get(j).unwrap_or(&empty);
                covered = Some(match covered {
                    None => rj.clone(),
                    Some(c) => c.intersection(rj)?,
                });
            }
            let dense = match covered {
                Some(c) => sparse.difference(&c)?.without_slivers(w * w),
                None => Region::new(),
            };
            if !dense.is_empty() && s.dense_percentage > 0.0 {
                sparse = sparse.difference(&dense)?;
                paths.extend(line_infill(
                    &dense.offset(overlap)?,
                    w * 100.0 / s.dense_percentage,
                    angle,
                    PathKind::SparseInfill,
                )?);
            }
        }
        // combined infill: printed once per N layers, N times thicker
        let every = s.infill_every_layers.max(1) as usize;
        let sparse_here = every == 1 || (i + 1) % every == 0;
        if let (Some(spacing), true) = (sparse_spacing, sparse_here) {
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
                (slots.get(i).map_or(0.0, LayerSlot::cut_z), i),
                infill_w,
            )?);
        }
        Ok(paths)
    })?;

    // back to one list per layer
    let mut per_layer_shells: Vec<Vec<ToolPath>> = vec![Vec::new(); n];
    let mut per_layer_infill: Vec<Vec<ToolPath>> = vec![Vec::new(); n];
    for ((job, sh), fill) in jobs.iter().zip(shelled).zip(infilled) {
        let tool = tool_of(&job.settings);
        if let Some(v) = per_layer_shells.get_mut(job.layer) {
            v.extend(sh.shells.into_iter().map(|p| ToolPath { tool, ..p }));
        }
        if let Some(v) = per_layer_infill.get_mut(job.layer) {
            v.extend(fill.into_iter().map(|p| ToolPath { tool, ..p }));
        }
    }
    for (shells, open) in per_layer_shells.iter_mut().zip(&open_lines) {
        for chain in open {
            shells.push(ToolPath {
                kind: PathKind::OuterPerimeter,
                points: chain.clone(),
                closed: false,
                tool: base_tool,
            });
        }
    }
    let shelled: Vec<Shelled> = per_layer_shells
        .into_iter()
        .map(|shells| Shelled { shells, inner: Region::new() })
        .collect();
    let infilled = per_layer_infill;

    // 5. skirt and brim, around the first layer and its support (or the raft)
    let first = regions.first().cloned().unwrap_or_default();
    let first = if support_first.is_empty() { first } else { first.union(&support_first)? };
    let raft = if s.raft_enabled { Some(raft_layers(&first, s, w)?) } else { None };
    let first = match &raft {
        Some(r) => r.footprint.clone(),
        None => first,
    };
    let w0 = slicecraft_io::gcode::layer_line_width(s, 0);
    let brim = if s.brim_enabled && raft.is_none() {
        rings_around(&first, 0.0, s.brim_loops, w0, PathKind::Brim)?
    } else {
        Vec::new()
    };
    let brim: Vec<ToolPath> = brim.into_iter().map(|p| ToolPath { tool: base_tool, ..p }).collect();
    // a skirt goes around the brim, not through it
    let brim_reach = if s.brim_enabled { f64::from(s.brim_loops) * w0 } else { 0.0 };
    let skirt = if s.skirt_enabled {
        rings_around(&first, s.skirt_distance + brim_reach, s.skirt_loops, w0, PathKind::Skirt)?
    } else {
        Vec::new()
    };
    let skirt: Vec<ToolPath> =
        skirt.into_iter().map(|p| ToolPath { tool: base_tool, ..p }).collect();

    // 6. order everything
    let (hx, hy) = s.homing_corner.point(doc.bed.width, doc.bed.depth);
    let mut at = Point2::new(hx, hy);
    let mut layers = Vec::with_capacity(n);
    for (i, (slot, (sh, infill))) in slots.iter().zip(shelled.into_iter().zip(infilled)).enumerate()
    {
        observer.progress(Stage::Gcode, i, n);
        if observer.cancelled() {
            return Err(EngineError::Cancelled);
        }
        // first everything outside the model: skirt, brim, then support, so
        // walls never wait on a column that is not there yet
        let mut pre: Vec<Vec<ToolPath>> = Vec::with_capacity(3);
        // with a raft the skirt goes around the raft instead
        if i < s.skirt_layers as usize && raft.is_none() {
            pre.push(skirt.clone());
        }
        if i == 0 {
            pre.push(brim.clone());
        }
        if let Some(sup) = support_toolpaths.get_mut(i) {
            let mut sup = std::mem::take(sup);
            // support printed once per N layers, N times thicker
            let every = s.support_every_layers.max(1) as usize;
            if every > 1 && i > 0 && (i + 1) % every != 0 {
                sup.retain(|p| p.kind != PathKind::Support);
            }
            pre.push(sup);
        }
        let mut model: Vec<ToolPath> = sh.shells;
        if s.infill_min_length > 0.0 {
            model.extend(infill.into_iter().filter(|p| {
                p.kind != PathKind::SparseInfill || p.closed || p.length() >= s.infill_min_length
            }));
        } else {
            model.extend(infill);
        }
        if s.vase_mode && i >= s.solid_layers_bottom as usize {
            let wall = model
                .into_iter()
                .filter(|p| p.kind == PathKind::OuterPerimeter)
                .max_by(|a, b| a.points.len().cmp(&b.points.len()));
            model = wall.into_iter().collect();
            pre.retain(|g| g.iter().all(|p| p.kind != PathKind::Support));
        }
        let islands: Vec<Vec<Vec<ToolPath>>> = match s.island_order {
            IslandOrder::ByFeature => vec![feature_groups(model, s)],
            IslandOrder::Nearest | IslandOrder::Consistent => {
                let empty = Region::new();
                split_islands(model, regions.get(i).unwrap_or(&empty))
                    .into_iter()
                    .map(|paths| feature_groups(paths, s))
                    .collect()
            }
        };
        let seam_target = Point2::new(s.seam_x, s.seam_y);
        let mut paths = Vec::new();
        let place = |g: Vec<ToolPath>, at: &mut Point2, paths: &mut Vec<ToolPath>| {
            let monotonic =
                s.monotonic_solid && g.first().is_some_and(|p| p.kind == PathKind::TopSolid);
            let (mut ordered, end) = if monotonic {
                // keep the scan order, one line after the next, starting at
                // whichever end of the surface is nearer
                let mut g = g;
                let first = g
                    .first()
                    .and_then(ToolPath::start)
                    .map_or(f64::INFINITY, |p| p.distance_sq(*at));
                let last = g
                    .last()
                    .and_then(ToolPath::start)
                    .map_or(f64::INFINITY, |p| p.distance_sq(*at));
                if last < first {
                    g.reverse();
                }
                let end = g.last().and_then(ToolPath::end).unwrap_or(*at);
                (g, end)
            } else if g.first().is_some_and(|p| {
                matches!(p.kind, PathKind::OuterPerimeter | PathKind::InnerPerimeter)
            }) {
                let area = regions.get(i).filter(|_| s.retract_only_outside);
                let crosses = |a: Point2, b: Point2| area.is_some_and(|r| r.crosses_boundary(a, b));
                order_walls(g, *at, s.perimeter_order == PerimeterOrder::InsideOut, &crosses)
            } else if s.retract_only_outside
                && let Some(area) = regions.get(i).filter(|r| !r.is_empty())
            {
                // prefer the next path reachable without leaving the part
                let crosses = |a: Point2, b: Point2| area.crosses_boundary(a, b);
                slicecraft_render::paths::order_paths_avoiding(g, *at, &crosses, 10.0)
            } else {
                order_paths(g, *at)
            };
            *at = end;
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
                *at = e;
            }
            paths.extend(ordered);
        };
        for g in pre {
            place(g, &mut at, &mut paths);
        }
        let mut remaining = islands;
        if s.island_order == IslandOrder::Consistent {
            remaining.sort_by(|a, b| island_key(a).total_cmp(&island_key(b)));
            remaining.reverse();
        }
        while !remaining.is_empty() {
            let next = match s.island_order {
                IslandOrder::Nearest => nearest_island(&remaining, at),
                _ => remaining.len() - 1,
            };
            if next >= remaining.len() {
                break;
            }
            // the consistent order is sorted descending, so taking the last
            // element walks it in ascending order
            for g in remaining.swap_remove(next) {
                place(g, &mut at, &mut paths);
            }
        }
        layers.push(LayerPaths {
            index: slot.index,
            z: slot.top,
            height: slot.height,
            paths,
            inside_travel: Vec::new(),
            travel_via: Vec::new(),
        });
    }

    // several tools: one run per tool on each layer, with the prime tower
    // and the ooze shield
    arrange_tools(&mut layers, &regions, &slots, s, w, base_tool)?;

    // Travel that does not cross the model's outline (inside the part, or
    // outside it, between support pillars) needs no retraction: a string
    // there is hidden in infill or in support. With combing, travel that
    // would cross a hole walks around it inside the part instead.
    if s.retract_only_outside || s.avoid_crossing_walls {
        let routes = par_map(&layers, Stage::Gcode, observer, |i, layer| {
            let Some(area) = regions.get(i) else { return Ok((Vec::new(), Vec::new())) };
            let comb_area =
                if s.avoid_crossing_walls { Some(area.offset(-0.25 * w)?) } else { None };
            // the surfaces this layer leaves exposed: travel over them retracts
            let tops = if s.retract_over_tops {
                match regions.get(i + 1) {
                    Some(up) => area.difference(up)?.without_slivers(w * w),
                    None => area.clone(),
                }
            } else {
                Region::new()
            };
            let over_tops = |legs: &[Point2]| {
                !tops.is_empty()
                    && legs.windows(2).any(|l| match l {
                        [a, b] => {
                            tops.crosses_boundary(*a, *b)
                                || tops.contains(Point2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0))
                        }
                        _ => false,
                    })
            };
            let mut flags = Vec::with_capacity(layer.paths.len());
            let mut vias = Vec::with_capacity(layer.paths.len());
            let mut prev: Option<Point2> = None;
            for p in &layer.paths {
                let (mut inside, mut via) = (false, Vec::new());
                if let (Some(a), Some(b)) = (prev, p.start()) {
                    let crosses = area.crosses_boundary(a, b);
                    if !crosses {
                        inside = s.retract_only_outside;
                    } else if let Some(c) = &comb_area {
                        let limit = a.distance(b) * s.travel_detour_limit + 2.0 * w;
                        if let Some(route) = slicecraft_render::paths::comb(c, a, b, limit) {
                            inside = true;
                            via = route;
                        }
                    }
                    if inside {
                        let mut legs = vec![a];
                        legs.extend(via.iter().copied());
                        legs.push(b);
                        if over_tops(&legs) {
                            inside = false;
                            via.clear();
                        }
                    }
                }
                flags.push(inside);
                vias.push(via);
                prev = p.end();
            }
            Ok((flags, vias))
        })?;
        for (layer, (f, v)) in layers.iter_mut().zip(routes) {
            layer.inside_travel = f;
            layer.travel_via = v;
        }
    }
    if let Some(r) = raft {
        layers = with_raft(r, layers, if s.skirt_enabled { skirt.clone() } else { Vec::new() });
        for p in layers.iter_mut().flat_map(|l| l.paths.iter_mut()) {
            if p.kind == PathKind::Raft {
                p.tool = base_tool;
            }
        }
    }
    // each printed layer's settings: raft layers use the profile
    let raft_count = layers.len().saturating_sub(n);
    let per_layer: Vec<slicecraft_doc::PrintSettings> = layers
        .iter()
        .map(|l| {
            l.index.checked_sub(raft_count).and_then(|i| layer_settings.get(i)).unwrap_or(s).clone()
        })
        .collect();
    let mut warnings = Vec::new();
    if !s.infill_pattern.is_implemented() && s.infill_percentage > 0.0 {
        warnings.push(
            "the chosen infill pattern is not drawn yet; rectilinear was used instead".to_string(),
        );
    }
    if s.support_enabled && supported_layers == 0 && doc.support_edits.is_empty() {
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
    Ok(Layered { layers, settings: per_layer, warnings, dropped_chains })
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
