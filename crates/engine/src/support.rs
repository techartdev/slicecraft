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
//! 3. **Clearance.** Each layer's support keeps `support_xy_gap` from the
//!    model beside it and `support_bottom_gap_layers` from the model below;
//!    whatever is then narrower than a line is dropped.
//! 4. **Density.** The `support_interface_layers` under the model, the
//!    `support_floor_layers` on top of the model and the `support_base_layers`
//!    on the bed are dense; the rest is the body at `support_infill`.

use crate::EngineError;
use crate::pipeline::{LayerSlot, SliceObserver, Stage, par_map};
use slicecraft_doc::{PrintSettings, SupportPattern, SupportPlacement};
use slicecraft_geom::Region;
use slicecraft_render::paths::{line_infill, perimeters};
use slicecraft_render::{PathKind, ToolPath};

/// Steepest overhang angle used; at 90° nothing would ever need support.
const MAX_ANGLE_DEG: f64 = 89.0;

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
    observer: &mut dyn SliceObserver,
) -> Result<Vec<SupportLayer>, EngineError> {
    let n = regions.len();
    if !s.support_enabled || n == 0 {
        return Ok(vec![SupportLayer::default(); n]);
    }
    let over = overhangs(regions, slots, s, w, observer)?;
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
        let beside = regions.get(j).map_or(Ok(Region::new()), |r| r.offset(s.support_xy_gap))?;
        let mut area = col.difference(&beside)?;
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
                out.extend(line_infill(&layer.body, spacing, 0.0, PathKind::Support)?);
            }
            SupportPattern::Grid => {
                for k in 0..2 {
                    out.extend(line_infill(
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
        let density = if index < s.support_base_layers as usize {
            100.0
        } else {
            s.support_interface_infill.max(1.0)
        };
        // crossing the body lines, turning each layer
        let angle = if index.is_multiple_of(2) { quarter } else { 0.0 };
        out.extend(line_infill(
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
        PrintSettings { support_enabled: true, ..PrintSettings::default() }
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
        let sup = support_layers(&regions, &slots(n, h), &s, w, &mut crate::pipeline::Quiet);
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
            support_layers(&regions, &slots(n, 0.2), &on(), 0.45, &mut crate::pipeline::Quiet);
        assert!(sup.is_ok_and(|l| l.iter().all(SupportLayer::is_empty)));
        // one that grows 1 mm per side per layer needs support at 45°, not at 85°
        let grows: Vec<Region> = (0..n)
            .map(|i| square(40.0 - i as f64, 40.0 - i as f64, 2.0 * i as f64 + 1.0))
            .collect();
        let sup = support_layers(&grows, &slots(n, 0.2), &on(), 0.45, &mut crate::pipeline::Quiet);
        assert!(sup.is_ok_and(|l| l.iter().any(|x| !x.is_empty())));
        let steep = PrintSettings { support_angle: 85.0, ..on() };
        let sup = support_layers(&grows, &slots(n, 0.2), &steep, 0.45, &mut crate::pipeline::Quiet);
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
            support_layers(&regions, &slots(n, 0.2), &on(), 0.45, &mut crate::pipeline::Quiet);
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
            support_layers(&regions, &slots(n, 0.2), &on(), 0.45, &mut crate::pipeline::Quiet);
        assert!(
            everywhere.as_ref().is_ok_and(|l| l.iter().skip(11).take(5).all(|x| !x.is_empty()))
        );
        // the bottom gap keeps support off the base's top layer
        assert!(everywhere.is_ok_and(|l| l.get(10).is_some_and(SupportLayer::is_empty)));
        let bed_only = PrintSettings { support_placement: SupportPlacement::BedOnly, ..on() };
        let sup =
            support_layers(&regions, &slots(n, 0.2), &bed_only, 0.45, &mut crate::pipeline::Quiet);
        assert!(sup.is_ok_and(|l| l.iter().all(SupportLayer::is_empty)));
    }

    #[test]
    fn base_layers_are_dense_and_paths_are_support() {
        let (n, w) = (20, 0.45);
        let regions = table(n, 10);
        let s = PrintSettings { support_base_layers: 2, support_outlines: 1, ..on() };
        let sup = support_layers(&regions, &slots(n, 0.2), &s, w, &mut crate::pipeline::Quiet);
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
        let sup = support_layers(&regions, &slots(n, 0.2), &off, w, &mut crate::pipeline::Quiet);
        assert!(sup.is_ok_and(|l| l.len() == n && l.iter().all(SupportLayer::is_empty)));
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
        let r = support_layers(&regions, &slots(20, 0.2), &on(), 0.45, &mut Stop);
        assert!(matches!(r, Err(EngineError::Cancelled)));
    }
}
