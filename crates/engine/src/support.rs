//! Support: which areas of each layer hold up the model, and their toolpaths.
//!
//! SliceCraft's own region-based design (the reference builds support from a
//! grid of pillars; see `docs/research/05-support.md` for what was observed):
//!
//! 1. **Overhang.** Layer *i* overhangs where it reaches further than
//!    `h · tan(angle)` past layer *i − 1*, grown by `support_expansion`. Thin
//!    strips are kept: on a slope they add up layer by layer.
//! 2. **Columns.** From the top down, each overhang starts a column (strips
//!    from a slope are closed into one area as they stack)
//!    `support_top_gap_layers` below it; a column continues straight down and is
//!    cut wherever the model occupies the layer, so it ends on the model or the
//!    bed. With "from the bed only", whatever does not reach the bed is removed
//!    again, from the bottom up.
//! 3. **Pillars** (the default style): each layer's column area is snapped to
//!    square cells of `support_pillar_size` on a grid fixed to the bed, a line
//!    width apart, so pillars stack exactly and snap off one by one. The
//!    "areas" style keeps the exact column shape.
//! 4. **Clearance.** Each layer's support keeps `support_xy_gap` from the
//!    model beside it and `support_bottom_gap_layers` from the model below;
//!    whatever is then narrower than a line is dropped.
//! 5. **Density.** The `support_interface_layers` under the model, the
//!    `support_floor_layers` on top of the model and the `support_base_layers`
//!    on the bed are dense; the rest is the body at `support_infill`. Each
//!    pillar or area is filled with one zigzag, not separate lines.

use crate::EngineError;
use crate::pipeline::{LayerSlot, SliceObserver, Stage, par_map};
use slicecraft_doc::{
    PrintSettings, SupportEdit, SupportEditKind, SupportPattern, SupportPlacement, SupportStyle,
};
use slicecraft_geom::{Point2, Region};
use slicecraft_render::paths::{line_infill, perimeters, zigzag};
use slicecraft_render::{PathKind, ToolPath};

/// Steepest overhang angle used; at 90° nothing would ever need support.
const MAX_ANGLE_DEG: f64 = 89.0;
/// Most pillar cells considered on one layer; past it the exact area is used.
const MAX_PILLAR_CELLS: usize = 250_000;
/// Sample positions inside a cell, as fractions of its side.
const SAMPLES: [f64; 3] = [0.25, 0.5, 0.75];
/// Of the nine samples, how many must be covered for a cell to become a pillar:
/// about a third of the cell, so edge cells that are barely touched stay empty.
const MIN_SAMPLES: u8 = 3;

/// Snap `area` to pillars: the cells of a `cell`-sized grid, aligned to the bed
/// origin so pillars stack exactly from layer to layer, that `area` covers at
/// `MIN_SAMPLES` of nine sample points (or that hold a whole small island), each shrunk
/// by `gap / 2` so neighbouring pillars stand apart.
fn pillars(area: &Region, cell: f64, gap: f64) -> Result<Region, EngineError> {
    let Some(b) = area.bounds() else { return Ok(Region::new()) };
    if !(cell > gap && cell.is_finite()) {
        return Ok(area.clone());
    }
    let (c0, c1) = ((b.min_x / cell).floor(), (b.max_x / cell).floor());
    let (r0, r1) = ((b.min_y / cell).floor(), (b.max_y / cell).floor());
    let (cols, rows) = ((c1 - c0 + 1.0).max(1.0), (r1 - r0 + 1.0).max(1.0));
    if cols * rows > MAX_PILLAR_CELLS as f64 || (cols * rows).is_nan() {
        return Ok(area.clone());
    }
    let (cols, rows) = (cols as usize, rows as usize);
    let mut hit = vec![0u8; cols * rows];
    let mut mark = |c: f64, r: f64, weight: u8| {
        let (ci, ri) = (c - c0, r - r0);
        if ci >= 0.0
            && ri >= 0.0
            && (ci as usize) < cols
            && (ri as usize) < rows
            && let Some(h) = hit.get_mut(ri as usize * cols + ci as usize)
        {
            *h = h.saturating_add(weight);
        }
    };
    let rings: Vec<&Vec<Point2>> = area.contours().collect();
    for r in 0..rows {
        let row = r0 + r as f64;
        for fy in SAMPLES {
            let y = (row + fy) * cell;
            let mut xs = Vec::new();
            for ring in &rings {
                let n = ring.len();
                for i in 0..n {
                    let (Some(p), Some(q)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
                    if (p.y > y) != (q.y > y) {
                        xs.push(p.x + (y - p.y) * (q.x - p.x) / (q.y - p.y));
                    }
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks_exact(2) {
                let &[x0, x1] = pair else { continue };
                for fx in SAMPLES {
                    let (lo, hi) = ((x0 / cell - fx).ceil(), (x1 / cell - fx).floor());
                    let mut c = lo;
                    while c <= hi {
                        mark(c, row, 1);
                        c += 1.0;
                    }
                }
            }
        }
    }
    // an island smaller than the samples still gets the cells it sits in
    for poly in &area.polygons {
        if poly.area() < cell * cell {
            for v in &poly.outer {
                mark((v.x / cell).floor(), (v.y / cell).floor(), MIN_SAMPLES);
            }
        }
    }
    let g = gap / 2.0;
    let mut squares = Vec::new();
    for (k, count) in hit.iter().enumerate() {
        if *count < MIN_SAMPLES {
            continue;
        }
        let x = (c0 + (k % cols) as f64) * cell;
        let y = (r0 + (k / cols) as f64) * cell;
        squares.push(vec![
            Point2::new(x + g, y + g),
            Point2::new(x + cell - g, y + g),
            Point2::new(x + cell - g, y + cell - g),
            Point2::new(x + g, y + cell - g),
        ]);
    }
    Ok(Region::from_loops(&squares)?)
}

/// The footprint of a support edit.
fn edit_box(e: &SupportEdit) -> Result<Region, EngineError> {
    Ok(Region::from_loops(&[vec![
        Point2::new(e.x0, e.y0),
        Point2::new(e.x1, e.y0),
        Point2::new(e.x1, e.y1),
        Point2::new(e.x0, e.y1),
    ]])?)
}

/// Fill each island of `region` with parallel lines joined into zigzags.
fn zigzag_fill(
    region: &Region,
    spacing: f64,
    angle: f64,
    kind: PathKind,
) -> Result<Vec<ToolPath>, EngineError> {
    let mut out = Vec::new();
    for poly in &region.polygons {
        let one = Region { polygons: vec![poly.clone()] };
        let lines = line_infill(&one, spacing, angle, kind)?;
        out.extend(zigzag(lines, &one, spacing * 2.0));
    }
    Ok(out)
}

/// The support of one layer.
#[derive(Debug, Clone, Default)]
pub struct SupportLayer {
    /// Printed at the body density.
    pub body: Region,
    /// Printed dense: under or on the model, or at the foot on the bed.
    pub dense: Region,
}

impl SupportLayer {
    pub fn is_empty(&self) -> bool {
        self.body.is_empty() && self.dense.is_empty()
    }

    /// Everything the layer's support covers.
    pub fn area(&self) -> Result<Region, EngineError> {
        Ok(self.body.union(&self.dense)?)
    }
}

/// Union of `regions[from..to]` (clamped to the stack).
fn union_range(regions: &[Region], from: usize, to: usize) -> Result<Region, EngineError> {
    let mut out = Region::new();
    for r in regions.iter().take(to.min(regions.len())).skip(from) {
        if !r.is_empty() {
            out = out.union(r)?;
        }
    }
    Ok(out)
}

/// Areas of every layer that overhang the layer below.
pub fn overhangs(
    regions: &[Region],
    slots: &[LayerSlot],
    s: &PrintSettings,
    w: f64,
    observer: &mut dyn SliceObserver,
) -> Result<Vec<Region>, EngineError> {
    let tan = s.support_angle.clamp(0.0, MAX_ANGLE_DEG).to_radians().tan();
    par_map(regions, Stage::Support, observer, |i, region| {
        let Some(below) = i.checked_sub(1).and_then(|j| regions.get(j)) else {
            // the first layer stands on the bed
            return Ok(Region::new());
        };
        let h = slots.get(i).map_or(s.layer_height, |sl| sl.height);
        let reach = below.offset(h * tan)?;
        // On a slope each layer adds only a thin strip; the strips add up to
        // the area under the slope in the columns, so none is dropped here.
        let mut o = region.difference(&reach)?.without_slivers(w * w * 0.01);
        if !o.is_empty() && s.support_expansion != 0.0 {
            o = o.offset(s.support_expansion)?;
        }
        Ok(o)
    })
}

/// Support areas for every layer. `regions` are the model's layers.
pub fn support_layers(
    regions: &[Region],
    slots: &[LayerSlot],
    s: &PrintSettings,
    w: f64,
    edits: &[SupportEdit],
    observer: &mut dyn SliceObserver,
) -> Result<Vec<SupportLayer>, EngineError> {
    let n = regions.len();
    let boxes = |kind: SupportEditKind| -> Result<Vec<(Region, SupportEdit)>, EngineError> {
        edits.iter().filter(|e| e.kind == kind).map(|e| Ok((edit_box(e)?, *e))).collect()
    };
    let (adds, blocks) = (boxes(SupportEditKind::Add)?, boxes(SupportEditKind::Block)?);
    if (!s.support_enabled && adds.is_empty()) || n == 0 {
        return Ok(vec![SupportLayer::default(); n]);
    }
    let cut_z = |j: usize| slots.get(j).map_or(0.0, LayerSlot::cut_z);
    let mut over = if s.support_enabled {
        overhangs(regions, slots, s, w, observer)?
    } else {
        vec![Region::new(); n]
    };
    // "add" boxes: every downward-facing surface inside, whatever its angle
    if !adds.is_empty() {
        for k in 1..n {
            let (Some(here), Some(below)) = (regions.get(k), regions.get(k - 1)) else { continue };
            for (b, e) in &adds {
                if !e.covers_z(cut_z(k)) {
                    continue;
                }
                let exposed = here.intersection(b)?.difference(below)?;
                if !exposed.is_empty()
                    && let Some(o) = over.get_mut(k)
                {
                    *o = o.union(&exposed)?;
                }
            }
        }
    }
    let gap_top = s.support_top_gap_layers as usize;
    // Strips from a slope are separated by the band each layer may reach out
    // unsupported; closing by half that band (at least half a line) joins
    // them into one area.
    let tan = s.support_angle.clamp(0.0, MAX_ANGLE_DEG).to_radians().tan();
    let join = (s.layer_height * tan).max(w) * 0.5 + 0.01;
    // the gaps are only ever between neighbouring layers' strips, so closing
    // each pair is enough, and those regions are small
    let joined = par_map(&over, Stage::Support, observer, |k, here| {
        if here.is_empty() {
            return Ok(Region::new());
        }
        Ok(match over.get(k + 1) {
            Some(next) if !next.is_empty() => here.union(next)?.offset(join)?.offset(-join)?,
            _ => here.offset(join)?.offset(-join)?,
        })
    })?;
    drop(over);

    // columns, from the top down
    let mut columns = vec![Region::new(); n];
    let mut c = Region::new();
    for j in (0..n).rev() {
        if observer.cancelled() {
            return Err(EngineError::Cancelled);
        }
        if let Some(o) = joined.get(j + 1 + gap_top)
            && !o.is_empty()
        {
            c = c.union(o)?;
        }
        if let Some(r) = regions.get(j)
            && !c.is_empty()
        {
            c = c.difference(r)?;
        }
        // "block" boxes cut the column like the model does
        for (b, e) in &blocks {
            if !c.is_empty() && e.covers_z(cut_z(j)) {
                c = c.difference(b)?;
            }
        }
        if let Some(slot) = columns.get_mut(j) {
            *slot = c.clone();
        }
    }

    // only what reaches the bed, from the bottom up
    if s.support_placement == SupportPlacement::BedOnly {
        let mut reach: Option<Region> = None;
        for col in columns.iter_mut() {
            let kept = match &reach {
                None => col.clone(),
                Some(r) => col.intersection(r)?,
            };
            *col = kept.clone();
            reach = Some(kept);
        }
    }

    // clearance and density
    let gap_bottom = s.support_bottom_gap_layers as usize;
    let interface = s.support_interface_layers as usize;
    let floor = s.support_floor_layers as usize;
    let base = s.support_base_layers as usize;
    par_map(&columns, Stage::Support, observer, |j, col| {
        if col.is_empty() {
            return Ok(SupportLayer::default());
        }
        let source = match s.support_style {
            SupportStyle::Pillars => pillars(col, s.support_pillar_size, w)?,
            SupportStyle::Areas => col.clone(),
        };
        let beside = regions.get(j).map_or(Ok(Region::new()), |r| r.offset(s.support_xy_gap))?;
        let mut area = source.difference(&beside)?;
        // snapping to pillars must not reach back into a blocked box
        for (b, e) in &blocks {
            if e.covers_z(cut_z(j)) {
                area = area.difference(b)?;
            }
        }
        if gap_bottom > 0 {
            let below = union_range(regions, j.saturating_sub(gap_bottom), j)?;
            if !below.is_empty() {
                area = area.difference(&below.offset(s.support_xy_gap)?)?;
            }
        }
        // what is narrower than a line cannot be printed: open by half a line
        let area = area.offset(-w / 2.0)?.offset(w / 2.0)?.without_slivers(w * w);
        if area.is_empty() {
            return Ok(SupportLayer::default());
        }
        let mut dense_zone = Region::new();
        if j < base {
            dense_zone = area.clone();
        } else {
            if interface > 0 {
                let from = j + 1 + gap_top;
                dense_zone = dense_zone.union(&union_range(regions, from, from + interface)?)?;
            }
            if floor > 0 {
                let to = j.saturating_sub(gap_bottom);
                let from = to.saturating_sub(floor);
                dense_zone = dense_zone.union(&union_range(regions, from, to)?)?;
            }
        }
        let dense = area.intersection(&dense_zone)?.without_slivers(w * w);
        let body = area.difference(&dense)?;
        Ok(SupportLayer { body, dense })
    })
}

/// Toolpaths for one layer's support.
pub fn support_paths(
    layer: &SupportLayer,
    index: usize,
    s: &PrintSettings,
    w: f64,
) -> Result<Vec<ToolPath>, EngineError> {
    let mut out = Vec::new();
    if layer.is_empty() {
        return Ok(out);
    }
    if s.support_outlines > 0 {
        let (walls, _) = perimeters(&layer.area()?, s.support_outlines, w)?;
        out.extend(walls.into_iter().map(|p| ToolPath { kind: PathKind::Support, ..p }));
    }
    let quarter = std::f64::consts::FRAC_PI_2;
    if !layer.body.is_empty() && s.support_infill > 0.0 {
        let spacing = w * 100.0 / s.support_infill;
        match s.support_pattern {
            // lines stack on lines, so the body is a set of thin walls
            SupportPattern::Lines => {
                out.extend(zigzag_fill(&layer.body, spacing, 0.0, PathKind::Support)?);
            }
            SupportPattern::Grid => {
                for k in 0..2 {
                    out.extend(zigzag_fill(
                        &layer.body,
                        spacing * 2.0,
                        f64::from(k) * quarter,
                        PathKind::Support,
                    )?);
                }
            }
        }
    }
    if !layer.dense.is_empty() {
        // the base on the bed joins the pillars into one footprint at the
        // body's density (as the reference does: a wider, not a denser, base);
        // interfaces on the model are dense
        let density = if index < s.support_base_layers as usize {
            s.support_infill.max(1.0)
        } else {
            s.support_interface_infill.max(1.0)
        };
        // crossing the body lines, turning each layer
        let angle = if index.is_multiple_of(2) { quarter } else { 0.0 };
        out.extend(zigzag_fill(
            &layer.dense,
            w * 100.0 / density,
            angle,
            PathKind::SupportInterface,
        )?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_geom::Point2;

    fn square(x0: f64, y0: f64, size: f64) -> Region {
        let pts = vec![
            Point2::new(x0, y0),
            Point2::new(x0 + size, y0),
            Point2::new(x0 + size, y0 + size),
            Point2::new(x0, y0 + size),
        ];
        Region::from_loops(&[pts]).unwrap_or_default()
    }

    fn slots(n: usize, h: f64) -> Vec<LayerSlot> {
        (0..n).map(|index| LayerSlot { index, top: (index + 1) as f64 * h, height: h }).collect()
    }

    fn on() -> PrintSettings {
        PrintSettings {
            support_enabled: true,
            support_style: SupportStyle::Areas,
            support_interface_layers: 2,
            ..PrintSettings::default()
        }
    }

    /// A 10 mm post with a 30 mm table top from layer `top_from` up.
    fn table(n: usize, top_from: usize) -> Vec<Region> {
        (0..n)
            .map(|i| if i < top_from { square(10.0, 10.0, 10.0) } else { square(0.0, 0.0, 30.0) })
            .collect()
    }

    #[test]
    fn a_table_top_is_held_up_from_the_bed_and_the_post_is_not() {
        let (n, h, w) = (20, 0.2, 0.45);
        let regions = table(n, 10);
        let s = on();
        let sup = support_layers(&regions, &slots(n, h), &s, w, &[], &mut crate::pipeline::Quiet);
        assert!(sup.is_ok(), "{sup:?}");
        let Ok(sup) = sup else { return };
        // under the table: support on every layer up to the gap
        let gap = s.support_top_gap_layers as usize;
        for (j, layer) in sup.iter().enumerate().take(10 - gap) {
            let a = layer.area().map(|a| a.area()).unwrap_or(0.0);
            // 900 − post (100) − clearance ring ≈ 780, never into the post
            assert!(a > 700.0 && a < 800.0, "layer {j}: {a}");
            let into_post =
                layer.area().and_then(|a| Ok(a.intersection(&square(10.0, 10.0, 10.0))?));
            assert!(into_post.is_ok_and(|r| r.area() < 1e-6), "support stays off the post");
        }
        // the top gap layer and everything from the table up: nothing
        for layer in sup.iter().skip(10 - gap) {
            assert!(layer.is_empty());
        }
        // the layers just under the table are dense
        assert!(sup.get(10 - gap - 1).is_some_and(|l| !l.dense.is_empty()));
        assert!(sup.get(2).is_some_and(|l| l.dense.is_empty() && !l.body.is_empty()));
    }

    #[test]
    fn gentle_slopes_need_no_support() {
        // a pyramid stepping in by 0.1 mm per 0.2 mm layer (about 27° from vertical)
        let n = 30;
        let regions: Vec<Region> =
            (0..n).map(|i| square(i as f64 * 0.1, i as f64 * 0.1, 30.0 - i as f64 * 0.2)).collect();
        let sup =
            support_layers(&regions, &slots(n, 0.2), &on(), 0.45, &[], &mut crate::pipeline::Quiet);
        assert!(sup.is_ok_and(|l| l.iter().all(SupportLayer::is_empty)));
        // one that grows 1 mm per side per layer needs support at 45°, not at 85°
        let grows: Vec<Region> = (0..n)
            .map(|i| square(40.0 - i as f64, 40.0 - i as f64, 2.0 * i as f64 + 1.0))
            .collect();
        let sup =
            support_layers(&grows, &slots(n, 0.2), &on(), 0.45, &[], &mut crate::pipeline::Quiet);
        assert!(sup.is_ok_and(|l| l.iter().any(|x| !x.is_empty())));
        let steep = PrintSettings { support_angle: 85.0, ..on() };
        let sup =
            support_layers(&grows, &slots(n, 0.2), &steep, 0.45, &[], &mut crate::pipeline::Quiet);
        assert!(sup.is_ok_and(|l| l.iter().all(SupportLayer::is_empty)));
    }

    #[test]
    fn a_moderate_slope_is_supported_although_each_step_is_thin() {
        // a block whose left side leans out 0.35 mm per 0.2 mm layer (about 60°
        // from vertical): each layer's new overhang is a 0.15 mm strip,
        // thinner than a line, but together they need support
        let n = 60;
        let regions: Vec<Region> = (0..n)
            .map(|i| {
                let x0 = 30.0 - i as f64 * 0.35;
                let pts = vec![
                    Point2::new(x0, 0.0),
                    Point2::new(40.0, 0.0),
                    Point2::new(40.0, 10.0),
                    Point2::new(x0, 10.0),
                ];
                Region::from_loops(&[pts]).unwrap_or_default()
            })
            .collect();
        let sup =
            support_layers(&regions, &slots(n, 0.2), &on(), 0.45, &[], &mut crate::pipeline::Quiet);
        assert!(sup.as_ref().is_ok_and(|l| l.get(20).is_some_and(|x| !x.is_empty())), "{sup:?}");
        // the support under layer 20 is about as wide as the slope has leaned out above it
        let width = sup
            .ok()
            .and_then(|l| l.get(10).and_then(|x| x.area().ok()))
            .map_or(0.0, |a| a.area() / 10.0);
        assert!(width > 10.0, "support under the slope is {width} mm wide");
    }

    #[test]
    fn bed_only_drops_support_that_would_stand_on_the_model() {
        // a wide base, a gap, and a ledge above it: support for the ledge stands on the base
        let n = 30;
        let regions: Vec<Region> = (0..n)
            .map(|i| match i {
                0..10 => square(0.0, 0.0, 30.0),
                10..20 => square(0.0, 0.0, 5.0),
                _ => square(0.0, 0.0, 30.0),
            })
            .collect();
        let everywhere =
            support_layers(&regions, &slots(n, 0.2), &on(), 0.45, &[], &mut crate::pipeline::Quiet);
        assert!(
            everywhere.as_ref().is_ok_and(|l| l.iter().skip(11).take(5).all(|x| !x.is_empty()))
        );
        // the bottom gap keeps support off the base's top layer
        assert!(everywhere.is_ok_and(|l| l.get(10).is_some_and(SupportLayer::is_empty)));
        let bed_only = PrintSettings { support_placement: SupportPlacement::BedOnly, ..on() };
        let sup = support_layers(
            &regions,
            &slots(n, 0.2),
            &bed_only,
            0.45,
            &[],
            &mut crate::pipeline::Quiet,
        );
        assert!(sup.is_ok_and(|l| l.iter().all(SupportLayer::is_empty)));
    }

    #[test]
    fn base_layers_are_dense_and_paths_are_support() {
        let (n, w) = (20, 0.45);
        let regions = table(n, 10);
        let s = PrintSettings { support_base_layers: 2, support_outlines: 1, ..on() };
        let sup = support_layers(&regions, &slots(n, 0.2), &s, w, &[], &mut crate::pipeline::Quiet);
        assert!(sup.is_ok());
        let Ok(sup) = sup else { return };
        assert!(sup.first().is_some_and(|l| l.body.is_empty() && !l.dense.is_empty()));
        let paths = sup.first().map(|l| support_paths(l, 0, &s, w));
        assert!(paths.is_some_and(|p| p.is_ok_and(|p| {
            !p.is_empty()
                && p.iter()
                    .all(|t| matches!(t.kind, PathKind::Support | PathKind::SupportInterface))
        })));
        // off: nothing at all
        let off = PrintSettings::default();
        let sup =
            support_layers(&regions, &slots(n, 0.2), &off, w, &[], &mut crate::pipeline::Quiet);
        assert!(sup.is_ok_and(|l| l.len() == n && l.iter().all(SupportLayer::is_empty)));
    }

    #[test]
    fn pillars_are_separate_grid_squares_that_stack() {
        let (n, w) = (20, 0.45);
        let regions = table(n, 10);
        let s = PrintSettings { support_style: SupportStyle::Pillars, ..on() };
        let sup = support_layers(&regions, &slots(n, 0.2), &s, w, &[], &mut crate::pipeline::Quiet);
        assert!(sup.is_ok());
        let Ok(sup) = sup else { return };
        let (Some(a), Some(b)) = (sup.get(3), sup.get(5)) else { return };
        let (Ok(a), Ok(b)) = (a.area(), b.area()) else { return };
        // separate pillars, each at most one cell
        assert!(a.polygons.len() > 10, "{} pillars", a.polygons.len());
        assert!(a.polygons.iter().all(|p| p.area() <= 16.0 + 1e-6));
        // the same grid on every layer: they stack exactly
        assert!((a.area() - b.area()).abs() < 1e-6);
        // never into the post
        let into = a.intersection(&square(10.0, 10.0, 10.0));
        assert!(into.is_ok_and(|r| r.area() < 1e-6));
        // each pillar is one zigzag
        let paths = sup.get(3).map(|l| support_paths(l, 3, &s, w));
        let Some(Ok(paths)) = paths else { return };
        let pillars = a.polygons.len();
        assert!(
            paths.len() <= pillars + pillars / 4,
            "{} paths for {pillars} pillars",
            paths.len()
        );
    }

    #[test]
    fn a_tiny_island_still_gets_a_pillar() {
        let mut cell = Region::new();
        let tiny = Region::from_loops(&[vec![
            Point2::new(1.1, 1.1),
            Point2::new(1.4, 1.1),
            Point2::new(1.4, 1.4),
            Point2::new(1.1, 1.4),
        ]]);
        if let Ok(t) = tiny {
            cell = pillars(&t, 4.0, 0.45).unwrap_or_default();
        }
        assert_eq!(cell.polygons.len(), 1);
    }

    #[test]
    fn a_block_box_on_one_pillar_removes_only_that_pillar() {
        let (n, w) = (20, 0.45);
        let regions = table(n, 10);
        let s = PrintSettings { support_style: slicecraft_doc::SupportStyle::Pillars, ..on() };
        let layer3 = |edits: &[SupportEdit]| {
            support_layers(&regions, &slots(n, 0.2), &s, w, edits, &mut crate::pipeline::Quiet)
                .ok()
                .and_then(|l| l.get(3).and_then(|x| x.area().ok()))
                .unwrap_or_default()
        };
        let before = layer3(&[]);
        assert!(before.polygons.len() > 4, "{} pillars", before.polygons.len());
        // the footprint of one pillar, as the window takes it from the preview
        let Some(one) = before.polygons.get(before.polygons.len() / 2) else { return };
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &one.outer {
            (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
        }
        let block = SupportEdit::columns(
            SupportEditKind::Block,
            (x0 - 0.05, y0 - 0.05),
            (x1 + 0.05, y1 + 0.05),
        );
        let after = layer3(&[block]);
        assert_eq!(after.polygons.len() + 1, before.polygons.len());
        assert!((before.area() - after.area() - one.area()).abs() < 0.05 * one.area().max(1.0));
    }

    #[test]
    fn add_boxes_support_gentle_surfaces_and_block_boxes_remove_support() {
        let (n, w) = (20, 0.45);
        let regions = table(n, 10);
        let off = PrintSettings { support_enabled: false, ..on() };
        // automatic support off: an add box under one corner of the table
        let add = SupportEdit::columns(SupportEditKind::Add, (0.0, 0.0), (8.0, 8.0));
        let sup =
            support_layers(&regions, &slots(n, 0.2), &off, w, &[add], &mut crate::pipeline::Quiet);
        let a =
            sup.ok().and_then(|l| l.get(3).and_then(|x| x.area().ok())).map_or(0.0, |r| r.area());
        assert!(a > 40.0 && a <= 64.0, "{a}");
        // automatic on, with a block over half the table
        let block = SupportEdit::columns(SupportEditKind::Block, (-1.0, -1.0), (15.0, 31.0));
        let all =
            support_layers(&regions, &slots(n, 0.2), &on(), w, &[], &mut crate::pipeline::Quiet);
        let some = support_layers(
            &regions,
            &slots(n, 0.2),
            &on(),
            w,
            &[block],
            &mut crate::pipeline::Quiet,
        );
        let area = |r: &Result<Vec<SupportLayer>, EngineError>| {
            r.as_ref()
                .ok()
                .and_then(|l| l.get(3).and_then(|x| x.area().ok()))
                .map_or(0.0, |r| r.area())
        };
        let (a_all, a_some) = (area(&all), area(&some));
        assert!(a_some > 0.0 && a_some < a_all * 0.7, "{a_some} vs {a_all}");
        let inside = some.ok().and_then(|l| l.get(3).and_then(|x| x.area().ok()));
        let into_block = inside.map(|r| r.intersection(&edit_box(&block).unwrap_or_default()));
        assert!(into_block.is_some_and(|r| r.is_ok_and(|r| r.area() < 1e-6)));
    }

    #[test]
    fn cancelling_stops_the_column_pass() {
        let regions = table(20, 10);
        struct Stop;
        impl SliceObserver for Stop {
            fn cancelled(&self) -> bool {
                true
            }
        }
        let r = support_layers(&regions, &slots(20, 0.2), &on(), 0.45, &[], &mut Stop);
        assert!(matches!(r, Err(EngineError::Cancelled)));
    }
}
