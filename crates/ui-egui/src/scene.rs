//! The 3D bed view, painted with egui triangle meshes.
//!
//! No GPU code of our own: geometry is projected on the CPU with
//! `camera::Frame` and handed to egui as coloured triangles, sorted back to
//! front. That keeps the view backend-agnostic (glow, wgpu, web) at the cost of
//! a triangle budget, which the caps below enforce.

use crate::camera::{Frame, Projected};
use eframe::egui::{Color32, Mesh, Pos2, Shape};
use slicecraft_doc::{Document, PrintSettings};
use slicecraft_geom::{Bounds3, MeshReport, Vec3};
use slicecraft_io::path_speed;
use slicecraft_render::{LayerPaths, PathKind};
use std::collections::BTreeSet;

/// Most model triangles drawn per frame; larger models are thinned evenly.
pub const MAX_DRAWN_TRIANGLES: usize = 400_000;
/// Most toolpath segments drawn per frame, newest layers first.
pub const MAX_DRAWN_SEGMENTS: usize = 200_000;

pub const MODEL: Color32 = Color32::from_rgb(0x6f, 0xa8, 0xdc);
pub const MODEL_SELECTED: Color32 = Color32::from_rgb(0xf2, 0xa6, 0x4b);
pub const MODEL_OUTSIDE: Color32 = Color32::from_rgb(0xd9, 0x55, 0x55);
pub const TRAVEL: Color32 = Color32::from_rgb(0x9a, 0xa4, 0xb4);
pub const RETRACTION: Color32 = Color32::from_rgb(0xff, 0x4f, 0x8b);
const BED: Color32 = Color32::from_rgb(0x2b, 0x2e, 0x33);
const GRID_MINOR: Color32 = Color32::from_rgb(0x3a, 0x3e, 0x45);
const GRID_MAJOR: Color32 = Color32::from_rgb(0x50, 0x55, 0x5e);
const BED_EDGE: Color32 = Color32::from_rgb(0x8a, 0x90, 0x9a);
const VOLUME: Color32 = Color32::from_rgba_premultiplied(0x40, 0x44, 0x4a, 0x60);

/// Feature colours, shared by the view and its legend.
pub fn kind_color(kind: PathKind) -> Color32 {
    match kind {
        PathKind::Skirt => Color32::from_rgb(0x4d, 0xb6, 0xac),
        PathKind::Brim => Color32::from_rgb(0x2f, 0x8f, 0xd8),
        PathKind::OuterPerimeter => Color32::from_rgb(0xff, 0x98, 0x30),
        PathKind::InnerPerimeter => Color32::from_rgb(0xf0, 0xd0, 0x50),
        PathKind::SolidInfill => Color32::from_rgb(0xb8, 0x6b, 0xe8),
        PathKind::SparseInfill => Color32::from_rgb(0xe0, 0x56, 0x56),
        PathKind::Support => Color32::from_rgb(0x9a, 0xa4, 0xae),
        PathKind::SupportInterface => Color32::from_rgb(0x5c, 0x8a, 0x6e),
    }
}

/// A cold-to-hot ramp for gradients (0 = blue, 1 = red).
pub fn ramp(t: f64) -> Color32 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let stops = [
        (0.0, [0x2c, 0x7b, 0xe5]),
        (0.33, [0x2c, 0xc8, 0xb4]),
        (0.66, [0xf2, 0xc9, 0x4c]),
        (1.0, [0xe5, 0x48, 0x3b]),
    ];
    for w in stops.windows(2) {
        let (Some(&(t0, c0)), Some(&(t1, c1))) = (w.first(), w.get(1)) else { continue };
        if t <= t1 {
            let k = (t - t0) / (t1 - t0);
            let mix =
                |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * k).round() as u8;
            return Color32::from_rgb(mix(c0[0], c1[0]), mix(c0[1], c1[1]), mix(c0[2], c1[2]));
        }
    }
    Color32::from_rgb(0xe5, 0x48, 0x3b)
}

fn shade(c: Color32, k: f64) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    let f = |v: u8| (f64::from(v) * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(f(c.r()), f(c.g()), f(c.b()), c.a())
}

fn pos(p: Projected) -> Pos2 {
    Pos2::new(p.x as f32, p.y as f32)
}

/// Collects triangles for one egui mesh.
#[derive(Default)]
pub struct Builder {
    mesh: Mesh,
}

impl Builder {
    pub fn tri(&mut self, a: Pos2, b: Pos2, c: Pos2, color: Color32) {
        let i = self.mesh.vertices.len() as u32;
        self.mesh.colored_vertex(a, color);
        self.mesh.colored_vertex(b, color);
        self.mesh.colored_vertex(c, color);
        self.mesh.add_triangle(i, i + 1, i + 2);
    }

    /// A screen-space line as a thin quad.
    pub fn line(&mut self, a: Pos2, b: Pos2, width: f32, color: Color32) {
        let d = b - a;
        let len = d.length();
        if !(len.is_finite() && len > 1e-3) {
            return;
        }
        let n = eframe::egui::vec2(-d.y, d.x) * (width * 0.5 / len);
        let i = self.mesh.vertices.len() as u32;
        self.mesh.colored_vertex(a + n, color);
        self.mesh.colored_vertex(b + n, color);
        self.mesh.colored_vertex(b - n, color);
        self.mesh.colored_vertex(a - n, color);
        self.mesh.add_triangle(i, i + 1, i + 2);
        self.mesh.add_triangle(i, i + 2, i + 3);
    }

    /// A small screen-space diamond, for markers.
    pub fn dot(&mut self, at: Pos2, r: f32, color: Color32) {
        let (a, b, c, d) = (
            at + eframe::egui::vec2(0.0, -r),
            at + eframe::egui::vec2(r, 0.0),
            at + eframe::egui::vec2(0.0, r),
            at + eframe::egui::vec2(-r, 0.0),
        );
        self.tri(a, b, c, color);
        self.tri(a, c, d, color);
    }

    pub fn world_line(&mut self, f: &Frame, a: Vec3, b: Vec3, width: f32, color: Color32) {
        if let Some((pa, pb)) = f.project_segment(a, b) {
            self.line(pos(pa), pos(pb), width, color);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.mesh.vertices.is_empty()
    }

    pub fn into_shape(self) -> Shape {
        Shape::mesh(self.mesh)
    }
}

/// What to draw around the bed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BedStyle {
    pub grid: bool,
    pub volume: bool,
    pub axes: bool,
}

impl Default for BedStyle {
    fn default() -> Self {
        Self { grid: true, volume: true, axes: true }
    }
}

/// The bed plate, its grid and the build volume outline.
pub fn bed(out: &mut Builder, f: &Frame, width: f64, depth: f64, height: f64, style: BedStyle) {
    let c = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(width, 0.0, 0.0),
        Vec3::new(width, depth, 0.0),
        Vec3::new(0.0, depth, 0.0),
    ];
    // the plate as small cells so near-plane clipping never eats it whole
    let cells = 8;
    for i in 0..cells {
        for j in 0..cells {
            let x0 = width * f64::from(i) / f64::from(cells);
            let x1 = width * f64::from(i + 1) / f64::from(cells);
            let y0 = depth * f64::from(j) / f64::from(cells);
            let y1 = depth * f64::from(j + 1) / f64::from(cells);
            let q = [
                f.project(Vec3::new(x0, y0, 0.0)),
                f.project(Vec3::new(x1, y0, 0.0)),
                f.project(Vec3::new(x1, y1, 0.0)),
                f.project(Vec3::new(x0, y1, 0.0)),
            ];
            if let [Some(a), Some(b), Some(cc), Some(d)] = q {
                out.tri(pos(a), pos(b), pos(cc), BED);
                out.tri(pos(a), pos(cc), pos(d), BED);
            }
        }
    }
    if style.grid {
        let step = 10.0;
        let lines_x = (width / step).floor() as usize;
        let lines_y = (depth / step).floor() as usize;
        for k in 1..=lines_x.min(2000) {
            let x = k as f64 * step;
            let col = if k % 5 == 0 { GRID_MAJOR } else { GRID_MINOR };
            out.world_line(f, Vec3::new(x, 0.0, 0.0), Vec3::new(x, depth, 0.0), 1.0, col);
        }
        for k in 1..=lines_y.min(2000) {
            let y = k as f64 * step;
            let col = if k % 5 == 0 { GRID_MAJOR } else { GRID_MINOR };
            out.world_line(f, Vec3::new(0.0, y, 0.0), Vec3::new(width, y, 0.0), 1.0, col);
        }
    }
    for k in 0..4 {
        if let (Some(&a), Some(&b)) = (c.get(k), c.get((k + 1) % 4)) {
            out.world_line(f, a, b, 1.5, BED_EDGE);
            if style.volume {
                let up = Vec3::new(0.0, 0.0, height);
                out.world_line(f, a + up, b + up, 1.0, VOLUME);
                out.world_line(f, a, a + up, 1.0, VOLUME);
            }
        }
    }
    if style.axes {
        let o = Vec3::ZERO;
        let (x, y, z) = (
            Color32::from_rgb(0xe0, 0x50, 0x50),
            Color32::from_rgb(0x50, 0xc0, 0x60),
            Color32::from_rgb(0x50, 0x8c, 0xe8),
        );
        out.world_line(f, o, Vec3::new(20.0, 0.0, 0.0), 2.5, x);
        out.world_line(f, o, Vec3::new(0.0, 20.0, 0.0), 2.5, y);
        out.world_line(f, o, Vec3::new(0.0, 0.0, 20.0), 2.5, z);
    }
}

/// Model geometry in bed coordinates, rebuilt when the document changes.
#[derive(Debug, Clone, Default)]
pub struct ModelGeometry {
    pub objects: Vec<ObjectGeometry>,
}

#[derive(Debug, Clone, Default)]
pub struct ObjectGeometry {
    /// Triangles with their unit normals (`None` for degenerate ones).
    pub triangles: Vec<([Vec3; 3], Option<Vec3>)>,
    pub bounds: Bounds3,
    pub outside_bed: bool,
    pub visible: bool,
    /// Facts about the placed mesh, for the analysis window.
    pub report: MeshReport,
}

impl ModelGeometry {
    pub fn build(doc: &Document) -> Self {
        let outside = doc.outside_bed();
        let objects = doc
            .objects
            .iter()
            .map(|o| {
                let Ok(world) = o.world_mesh() else { return ObjectGeometry::default() };
                let triangles: Vec<([Vec3; 3], Option<Vec3>)> =
                    world.triangles().iter().map(|t| ([t.a, t.b, t.c], t.normal())).collect();
                ObjectGeometry {
                    triangles,
                    bounds: world.bounds(),
                    outside_bed: o.visible && outside.contains(&o.name),
                    visible: o.visible,
                    report: world.report(),
                }
            })
            .collect();
        Self { objects }
    }

    pub fn triangle_count(&self) -> usize {
        self.objects.iter().map(|o| o.triangles.len()).sum()
    }

    /// Bounds of every model, for "fit to view".
    pub fn bounds(&self) -> Bounds3 {
        let mut b = Bounds3::empty();
        for o in self.objects.iter().filter(|o| !o.bounds.is_empty()) {
            b.add(o.bounds.min());
            b.add(o.bounds.max());
        }
        b
    }
}

/// How model triangles are drawn.
#[derive(Debug, Clone, Copy)]
pub struct ModelStyle<'a> {
    pub selected: &'a BTreeSet<usize>,
    /// 1 = opaque, lower ghosts the models.
    pub alpha: f32,
    /// Hide everything above this height (a section view).
    pub cut_z: Option<f64>,
}

/// Shaded, depth-sorted model triangles.
pub fn models(out: &mut Builder, f: &Frame, geo: &ModelGeometry, style: ModelStyle<'_>) {
    let total = geo.triangle_count().max(1);
    let stride = total.div_ceil(MAX_DRAWN_TRIANGLES).max(1);
    let light = f.light();
    let eye = f.eye();
    let mut drawn: Vec<(f64, [Pos2; 3], Color32)> =
        Vec::with_capacity(total.min(MAX_DRAWN_TRIANGLES));
    for (oi, o) in geo.objects.iter().enumerate() {
        let base = if o.outside_bed {
            MODEL_OUTSIDE
        } else if style.selected.contains(&oi) {
            MODEL_SELECTED
        } else {
            MODEL
        };
        let alpha = if o.visible { style.alpha } else { style.alpha * 0.25 };
        for (ti, (v, n)) in o.triangles.iter().enumerate() {
            if ti % stride != 0 {
                continue;
            }
            if let Some(cut) = style.cut_z
                && v.iter().all(|p| p.z > cut)
            {
                continue;
            }
            let centroid = (v[0] + v[1] + v[2]).scale(1.0 / 3.0);
            let k = match n {
                Some(n) => {
                    if n.dot(eye - centroid) < 0.0 {
                        // inside faces show only through a section cut, darker
                        if style.cut_z.is_none() {
                            continue;
                        }
                        0.25
                    } else {
                        0.32 + 0.68 * n.dot(light).max(0.0)
                    }
                }
                None => 0.6,
            };
            let (Some(a), Some(b), Some(c)) = (f.project(v[0]), f.project(v[1]), f.project(v[2]))
            else {
                continue;
            };
            let depth = (a.depth + b.depth + c.depth) / 3.0;
            let col = shade(base, k).gamma_multiply(alpha);
            drawn.push((depth, [pos(a), pos(b), pos(c)], col));
        }
    }
    drawn.sort_by(|x, y| y.0.total_cmp(&x.0));
    for (_, [a, b, c], col) in drawn {
        out.tri(a, b, c, col);
    }
}

/// What a preview segment is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegKind {
    Path(PathKind),
    Travel,
}

/// One drawable move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewSeg {
    pub a: Vec3,
    pub b: Vec3,
    pub kind: SegKind,
    /// Feed speed, mm/s.
    pub speed: f32,
}

/// A layer's moves in print order.
#[derive(Debug, Clone, Default)]
pub struct PreviewLayer {
    pub z: f64,
    pub height: f64,
    pub segs: Vec<PreviewSeg>,
    /// Where the filament is pulled back.
    pub retractions: Vec<Vec3>,
}

/// Flatten toolpaths into drawable moves once per slice, with travels between
/// paths and retraction marks where the writer retracts.
pub fn build_preview(layers: &[LayerPaths], s: &PrintSettings) -> Vec<PreviewLayer> {
    let retracts = s.retraction() > 0.0;
    let mut last: Option<Vec3> = None;
    let mut out = Vec::with_capacity(layers.len());
    for l in layers {
        let at = |q: slicecraft_geom::Point2| Vec3::new(q.x, q.y, l.z);
        let mut pl = PreviewLayer { z: l.z, height: l.height, ..PreviewLayer::default() };
        let mut first_in_layer = true;
        for p in &l.paths {
            let Some(start) = p.start() else { continue };
            let s0 = at(start);
            if let Some(prev) = last {
                let dist = prev.xy().distance(s0.xy());
                let layer_change = first_in_layer && s.retract_on_layer_change;
                if retracts && (dist >= s.retract_min_travel || layer_change) {
                    pl.retractions.push(prev);
                }
                pl.segs.push(PreviewSeg {
                    a: prev,
                    b: s0,
                    kind: SegKind::Travel,
                    speed: s.travel_speed as f32,
                });
            }
            first_in_layer = false;
            let speed = path_speed(s, p.kind, l.index) as f32;
            let kind = SegKind::Path(p.kind);
            for w in p.points.windows(2) {
                if let (Some(&a), Some(&b)) = (w.first(), w.get(1)) {
                    pl.segs.push(PreviewSeg { a: at(a), b: at(b), kind, speed });
                }
            }
            if p.closed
                && p.points.len() > 2
                && let (Some(&a), Some(&b)) = (p.points.last(), p.points.first())
            {
                pl.segs.push(PreviewSeg { a: at(a), b: at(b), kind, speed });
            }
            last = p.end().map(at);
        }
        out.push(pl);
    }
    out
}

/// Which toolpath features are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindFilter(pub [bool; 8]);

impl Default for KindFilter {
    fn default() -> Self {
        Self([true; 8])
    }
}

impl KindFilter {
    pub fn shows(&self, kind: PathKind) -> bool {
        let i = PathKind::ALL.iter().position(|k| *k == kind).unwrap_or(0);
        self.0.get(i).copied().unwrap_or(true)
    }
}

/// How toolpaths are coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coloring {
    Feature,
    Speed,
    Height,
}

impl Coloring {
    pub const ALL: [Coloring; 3] = [Coloring::Feature, Coloring::Speed, Coloring::Height];

    pub fn label(self) -> &'static str {
        match self {
            Coloring::Feature => "Feature type",
            Coloring::Speed => "Speed",
            Coloring::Height => "Height (Z)",
        }
    }
}

/// Everything that decides what the toolpath view shows.
#[derive(Debug, Clone, Copy)]
pub struct PreviewStyle {
    pub first_layer: usize,
    pub last_layer: usize,
    /// On the last layer, draw only this many moves (line-by-line playback).
    pub moves_on_last: Option<usize>,
    pub coloring: Coloring,
    pub filter: KindFilter,
    pub show_travel: bool,
    pub show_retractions: bool,
    pub width_mm: f64,
    pub speed_range: (f32, f32),
    pub z_range: (f64, f64),
}

/// Toolpaths in the chosen layer range; the last layer bright, the others
/// dimmed. Returns where the nozzle is at the end of what was drawn.
pub fn toolpaths(
    out: &mut Builder,
    f: &Frame,
    layers: &[PreviewLayer],
    style: &PreviewStyle,
) -> Option<Vec3> {
    let last = style.last_layer.min(layers.len().checked_sub(1)?);
    let first = style.first_layer.min(last);
    // newest first until the budget is spent, then paint bottom-up
    let mut budget = MAX_DRAWN_SEGMENTS;
    let mut start = last + 1;
    for i in (first..=last).rev() {
        let n = layers.get(i).map_or(0, |l| l.segs.len());
        if n > budget {
            break;
        }
        budget -= n;
        start = i;
    }
    let (s_lo, s_hi) = style.speed_range;
    let (z_lo, z_hi) = style.z_range;
    let mut head = None;
    for i in start..=last {
        let Some(layer) = layers.get(i) else { continue };
        let is_last = i == last;
        let dim = if is_last { 1.0 } else { 0.6 };
        let limit = if is_last { style.moves_on_last.unwrap_or(usize::MAX) } else { usize::MAX };
        for (n, seg) in layer.segs.iter().enumerate() {
            if n >= limit {
                break;
            }
            head = Some(seg.b);
            let color = match seg.kind {
                SegKind::Travel => {
                    if !style.show_travel {
                        continue;
                    }
                    TRAVEL
                }
                SegKind::Path(kind) => {
                    if !style.filter.shows(kind) {
                        continue;
                    }
                    match style.coloring {
                        Coloring::Feature => kind_color(kind),
                        Coloring::Speed => {
                            ramp(f64::from((seg.speed - s_lo) / (s_hi - s_lo).max(1e-3)))
                        }
                        Coloring::Height => ramp((layer.z - z_lo) / (z_hi - z_lo).max(1e-6)),
                    }
                }
            };
            let Some((pa, pb)) = f.project_segment(seg.a, seg.b) else { continue };
            let w = if seg.kind == SegKind::Travel {
                1.0
            } else {
                let px = style.width_mm * f.scale_at((pa.depth + pb.depth) / 2.0);
                (if is_last { px.clamp(1.5, 12.0) } else { px.clamp(1.0, 10.0) }) as f32
            };
            out.line(pos(pa), pos(pb), w, shade(color, dim));
        }
        if style.show_retractions && is_last {
            for r in &layer.retractions {
                if let Some(p) = f.project(*r) {
                    out.dot(pos(p), 3.5, RETRACTION);
                }
            }
        }
    }
    head
}

/// The nozzle marker: a small cone over `at`.
pub fn toolhead(out: &mut Builder, f: &Frame, at: Vec3) {
    let tip = f.project(at);
    let top = f.project(at + Vec3::new(0.0, 0.0, 8.0));
    if let (Some(t), Some(u)) = (tip, top) {
        let (t, u) = (pos(t), pos(u));
        let half = ((t - u).length() * 0.45).max(4.0);
        let body = Color32::from_rgb(0xd0, 0xd4, 0xda);
        out.tri(t, u + eframe::egui::vec2(-half, 0.0), u + eframe::egui::vec2(half, 0.0), body);
        out.dot(t, 2.5, Color32::WHITE);
    }
}

/// Speed range over a whole slice, for the speed colouring.
pub fn speed_range(layers: &[PreviewLayer]) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for s in layers.iter().flat_map(|l| l.segs.iter()) {
        if matches!(s.kind, SegKind::Path(_)) {
            lo = lo.min(s.speed);
            hi = hi.max(s.speed);
        }
    }
    if lo.is_finite() && hi.is_finite() { (lo, hi) } else { (0.0, 1.0) }
}

/// Screen rectangle of an object's bounds, for picking.
pub fn screen_box(f: &Frame, b: &Bounds3) -> Option<(Pos2, Pos2, f64)> {
    if b.is_empty() {
        return None;
    }
    let (lo, hi) = (b.min(), b.max());
    let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut depth = f64::INFINITY;
    for i in 0..8 {
        let p = Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let q = f.project(p)?;
        min = min.min(pos(q));
        max = max.max(pos(q));
        depth = depth.min(q.depth);
    }
    Some((min, max, depth))
}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_render::{Point2, ToolPath};

    fn two_paths() -> Vec<LayerPaths> {
        vec![LayerPaths {
            index: 0,
            z: 0.2,
            height: 0.2,
            paths: vec![
                ToolPath {
                    kind: PathKind::OuterPerimeter,
                    points: vec![
                        Point2::new(0.0, 0.0),
                        Point2::new(1.0, 0.0),
                        Point2::new(1.0, 1.0),
                    ],
                    closed: true,
                },
                ToolPath {
                    kind: PathKind::SparseInfill,
                    points: vec![Point2::new(30.0, 0.0), Point2::new(40.0, 0.0)],
                    closed: false,
                },
            ],
            inside_travel: Vec::new(),
        }]
    }

    #[test]
    fn preview_has_paths_travel_and_retractions() {
        let layers = build_preview(&two_paths(), &PrintSettings::default());
        let l = &layers[0];
        let paths = l.segs.iter().filter(|s| matches!(s.kind, SegKind::Path(_))).count();
        let travels = l.segs.iter().filter(|s| s.kind == SegKind::Travel).count();
        assert_eq!(paths, 4, "three ring edges and one line");
        assert_eq!(travels, 1);
        assert_eq!(l.retractions.len(), 1, "the 29 mm hop retracts");
        assert!(l.segs.iter().all(|s| (s.a.z - 0.2).abs() < 1e-12));
    }

    #[test]
    fn speeds_follow_the_profile() {
        let layers = build_preview(&two_paths(), &PrintSettings::default());
        assert_eq!(speed_range(&layers), (20.0, 20.0), "the first layer runs at one speed");
    }

    #[test]
    fn filter_hides_kinds() {
        let mut f = KindFilter::default();
        assert!(f.shows(PathKind::SparseInfill));
        f.0[5] = false;
        assert!(!f.shows(PathKind::SparseInfill));
        assert!(f.shows(PathKind::Skirt));
    }

    #[test]
    fn ramp_and_shade() {
        assert_eq!(ramp(0.0), Color32::from_rgb(0x2c, 0x7b, 0xe5));
        assert_eq!(ramp(1.0), Color32::from_rgb(0xe5, 0x48, 0x3b));
        assert_eq!(ramp(f64::NAN), ramp(0.0));
        let c = shade(Color32::from_rgb(200, 100, 50), 0.5);
        assert_eq!((c.r(), c.g(), c.b()), (100, 50, 25));
    }
}
