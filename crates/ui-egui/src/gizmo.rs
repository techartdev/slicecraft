//! Handles drawn on the selected model for the move, rotate and scale tools,
//! the view cube in the corner of the 3D view, and the floating panel that
//! gives each tool its exact values.
//!
//! The handles are screen-sized: arrows and rings keep the same size in
//! pixels however far the camera is, and are drawn over the models so they
//! are never hidden inside one.

use crate::app::{SliceCraftApp, Tool};
use crate::camera::{Frame, ViewPreset};
use crate::i18n::tr;
use crate::scene::Builder;
use crate::theme;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Ui, pos2, vec2};
use slicecraft_doc::{Axis, Transform};
use slicecraft_engine::Command;
use slicecraft_geom::{Bounds3, Vec3};

/// Length of a move arrow or scale stalk on screen (px).
const ARM_PX: f64 = 110.0;
/// How close the pointer must be to a handle to grab it (px).
const GRAB_PX: f32 = 9.0;
/// Segments drawn per rotation ring.
const RING_SEGMENTS: usize = 96;

/// A part of a tool's handles that the pointer can grab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// Move along X (0) or Y (1).
    MoveAxis(usize),
    /// Move freely on the bed.
    MovePlane,
    /// Turn about X (0), Y (1) or Z (2).
    Ring(usize),
    /// Stretch along X (0), Y (1) or Z (2).
    ScaleAxis(usize),
    /// Scale evenly.
    ScaleAll,
}

/// A handle drag in progress: where it started, and the model's placement then.
#[derive(Debug, Clone, Copy)]
pub struct GizmoDrag {
    pub index: usize,
    pub handle: Handle,
    pub start: Transform,
    pub from: Pos2,
    pub center: Vec3,
    /// Grab offset on the bed for a plane move; the start angle (radians on
    /// screen) for a ring; the start size along the axis for a scale.
    pub grab: Vec3,
}

fn axis_vec(k: usize) -> Vec3 {
    match k {
        0 => Vec3::new(1.0, 0.0, 0.0),
        1 => Vec3::new(0.0, 1.0, 0.0),
        _ => Vec3::new(0.0, 0.0, 1.0),
    }
}

fn colour(k: usize) -> Color32 {
    theme::AXES.get(k).copied().unwrap_or(Color32::GRAY)
}

fn pos(p: crate::camera::Projected) -> Pos2 {
    pos2(p.x as f32, p.y as f32)
}

/// Distance from `p` to the segment `a`–`b` on screen.
fn to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let d = b - a;
    let len2 = d.length_sq();
    let t = if len2 > 0.0 { ((p - a).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + d * t)).length()
}

/// Where the handles of one model are, for one frame.
pub struct Gizmo {
    pub center: Vec3,
    /// Millimetres that show as one arm length on screen.
    pub arm: f64,
    /// Ring radius in millimetres.
    pub ring: f64,
    pub bounds: Bounds3,
}

impl Gizmo {
    pub fn new(f: &Frame, bounds: Bounds3) -> Option<Self> {
        if bounds.is_empty() {
            return None;
        }
        let center = (bounds.min() + bounds.max()).scale(0.5);
        let p = f.project(center)?;
        let k = f.scale_at(p.depth);
        if !(k.is_finite() && k > 0.0) {
            return None;
        }
        let arm = ARM_PX / k;
        let radius_px = ((bounds.size().length() / 2.0) * k + 14.0).clamp(70.0, 260.0);
        Some(Self { center, arm, ring: radius_px / k, bounds })
    }

    /// The point of a ring at angle `a` (radians) about axis `k`.
    fn ring_point(&self, k: usize, a: f64) -> Vec3 {
        let (s, c) = a.sin_cos();
        let (u, w) = match k {
            0 => (axis_vec(1), axis_vec(2)),
            1 => (axis_vec(2), axis_vec(0)),
            _ => (axis_vec(0), axis_vec(1)),
        };
        self.center + (u.scale(c) + w.scale(s)).scale(self.ring)
    }

    fn ring_screen(&self, f: &Frame, k: usize) -> Vec<Pos2> {
        (0..=RING_SEGMENTS)
            .filter_map(|i| {
                let a = i as f64 / RING_SEGMENTS as f64 * std::f64::consts::TAU;
                f.project(self.ring_point(k, a)).map(pos)
            })
            .collect()
    }

    fn scale_tip(&self, k: usize) -> Vec3 {
        let half = match k {
            0 => self.bounds.size().x,
            1 => self.bounds.size().y,
            _ => self.bounds.size().z,
        } / 2.0;
        self.center + axis_vec(k).scale(half + self.arm * 0.6)
    }

    fn corner(&self) -> Vec3 {
        self.bounds.max()
    }

    /// The handle under `p` for `tool`, if any.
    pub fn hit(&self, f: &Frame, tool: Tool, p: Pos2) -> Option<Handle> {
        let c = pos(f.project(self.center)?);
        let mut best: Option<(f32, Handle)> = None;
        let mut consider = |d: f32, h: Handle| {
            if d < GRAB_PX && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, h));
            }
        };
        match tool {
            Tool::Move => {
                for k in 0..2 {
                    if let Some(tip) = f.project(self.center + axis_vec(k).scale(self.arm)) {
                        consider(to_segment(p, c, pos(tip)), Handle::MoveAxis(k));
                    }
                }
                if let Some(q) = self.plane_square(f)
                    && egui::Rect::from_two_pos(q[0], q[2]).expand(2.0).contains(p)
                {
                    consider(0.0, Handle::MovePlane);
                }
            }
            Tool::Rotate => {
                for k in 0..3 {
                    let pts = self.ring_screen(f, k);
                    let d = pts
                        .windows(2)
                        .filter_map(|w| match w {
                            [a, b] => Some(to_segment(p, *a, *b)),
                            _ => None,
                        })
                        .fold(f32::INFINITY, f32::min);
                    // where rings cross, the one facing the camera wins
                    let (_, _, forward) = f.axes();
                    let facing = axis_vec(k).dot(forward).abs() as f32;
                    consider(d + (1.0 - facing) * 3.0, Handle::Ring(k));
                }
            }
            Tool::Scale => {
                for k in 0..3 {
                    if let Some(t) = f.project(self.scale_tip(k)) {
                        consider((p - pos(t)).length() - 3.0, Handle::ScaleAxis(k));
                    }
                }
                if let Some(t) = f.project(self.corner()) {
                    consider((p - pos(t)).length() - 3.0, Handle::ScaleAll);
                }
            }
            _ => {}
        }
        best.map(|(_, h)| h)
    }

    /// The small square for moving on the bed, between the X and Y arrows.
    fn plane_square(&self, f: &Frame) -> Option<[Pos2; 4]> {
        let s = self.arm * 0.32;
        let o = self.center;
        let pts = [
            o + Vec3::new(s * 0.35, s * 0.35, 0.0),
            o + Vec3::new(s, s * 0.35, 0.0),
            o + Vec3::new(s, s, 0.0),
            o + Vec3::new(s * 0.35, s, 0.0),
        ];
        let [a, b, c, d] = pts.map(|v| f.project(v).map(pos));
        Some([a?, b?, c?, d?])
    }

    /// Draw the handles of `tool`; `hot` is hovered or being dragged.
    pub fn draw(&self, out: &mut Builder, f: &Frame, tool: Tool, hot: Option<Handle>) {
        let Some(c) = f.project(self.center).map(pos) else { return };
        let width = |h: Handle| if hot == Some(h) { 4.5 } else { 3.0 };
        let tint = |h: Handle, col: Color32| {
            if hot == Some(h) { col.gamma_multiply(1.3) } else { col }
        };
        match tool {
            Tool::Move => {
                for k in 0..2 {
                    let h = Handle::MoveAxis(k);
                    let Some(tip) = f.project(self.center + axis_vec(k).scale(self.arm)).map(pos)
                    else {
                        continue;
                    };
                    arrow(out, c, tip, width(h), tint(h, colour(k)));
                }
                if let Some(q) = self.plane_square(f) {
                    let col = tint(Handle::MovePlane, Color32::from_rgb(0xf2, 0xc1, 0x4b));
                    out.tri(q[0], q[1], q[2], col.gamma_multiply(0.55));
                    out.tri(q[0], q[2], q[3], col.gamma_multiply(0.55));
                    for i in 0..4 {
                        out.line(q[i], q[(i + 1) % 4], 1.5, col);
                    }
                }
                out.dot(c, 4.0, Color32::WHITE);
            }
            Tool::Rotate => {
                for k in 0..3 {
                    let h = Handle::Ring(k);
                    let pts = self.ring_screen(f, k);
                    for w in pts.windows(2) {
                        if let [a, b] = w {
                            out.line(*a, *b, width(h), tint(h, colour(k)));
                        }
                    }
                }
            }
            Tool::Scale => {
                self.box_lines(out, f);
                for k in 0..3 {
                    let h = Handle::ScaleAxis(k);
                    let (Some(a), Some(t)) =
                        (f.project(self.center).map(pos), f.project(self.scale_tip(k)).map(pos))
                    else {
                        continue;
                    };
                    out.line(a, t, 1.5, colour(k).gamma_multiply(0.8));
                    square(out, t, if hot == Some(h) { 7.5 } else { 6.0 }, tint(h, colour(k)));
                }
                if let Some(t) = f.project(self.corner()).map(pos) {
                    let h = Handle::ScaleAll;
                    square(out, t, if hot == Some(h) { 7.5 } else { 6.0 }, tint(h, Color32::WHITE));
                }
            }
            _ => {}
        }
    }

    fn box_lines(&self, out: &mut Builder, f: &Frame) {
        let (lo, hi) = (self.bounds.min(), self.bounds.max());
        let corner = |i: usize| {
            Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            )
        };
        for (a, b) in [
            (0, 1),
            (1, 3),
            (3, 2),
            (2, 0),
            (4, 5),
            (5, 7),
            (7, 6),
            (6, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            if let Some((pa, pb)) = f.project_segment(corner(a), corner(b)) {
                out.line(pos(pa), pos(pb), 1.0, Color32::from_white_alpha(110));
            }
        }
    }

    /// While a ring is dragged: the angle turned, as a filled sector, with
    /// marks every 15° (longer every 90°).
    pub fn draw_dial(&self, out: &mut Builder, f: &Frame, k: usize, start: f64, turned: f64) {
        let col = colour(k);
        for i in 0..24 {
            let a = i as f64 * std::f64::consts::TAU / 24.0;
            let long = i % 6 == 0;
            let r0 = if long { 0.82 } else { 0.9 };
            let p0 = self.center + (self.ring_point(k, a) - self.center).scale(r0);
            if let (Some(a), Some(b)) = (f.project(p0), f.project(self.ring_point(k, a))) {
                out.line(pos(a), pos(b), if long { 2.0 } else { 1.2 }, col.gamma_multiply(0.85));
            }
        }
        let Some(c) = f.project(self.center).map(pos) else { return };
        let steps = ((turned.abs() / std::f64::consts::TAU * 96.0).ceil() as usize).clamp(1, 96);
        let mut prev = None;
        for i in 0..=steps {
            let a = start + turned * i as f64 / steps as f64;
            let Some(p) = f.project(self.ring_point(k, a)).map(pos) else { continue };
            if let Some(q) = prev {
                out.tri(c, q, p, col.gamma_multiply(0.28));
            }
            prev = Some(p);
        }
    }

    /// The ring angle (radians, in the ring's own plane) nearest to `p`.
    pub fn ring_angle_at(&self, f: &Frame, k: usize, p: Pos2) -> f64 {
        let mut best = (f32::INFINITY, 0.0);
        for i in 0..RING_SEGMENTS {
            let a = i as f64 / RING_SEGMENTS as f64 * std::f64::consts::TAU;
            if let Some(q) = f.project(self.ring_point(k, a)).map(pos) {
                let d = (q - p).length();
                if d < best.0 {
                    best = (d, a);
                }
            }
        }
        best.1
    }
}

fn arrow(out: &mut Builder, a: Pos2, b: Pos2, w: f32, col: Color32) {
    let d = b - a;
    let len = d.length();
    if len < 4.0 {
        return;
    }
    let u = d / len;
    let n = vec2(-u.y, u.x);
    let head = 13.0_f32.min(len * 0.4);
    let base = b - u * head;
    out.line(a, base, w, col);
    out.tri(b, base + n * (head * 0.45), base - n * (head * 0.45), col);
}

fn square(out: &mut Builder, c: Pos2, half: f32, col: Color32) {
    let r = Rect::from_center_size(c, vec2(half * 2.0, half * 2.0));
    out.tri(r.left_top(), r.right_top(), r.right_bottom(), col);
    out.tri(r.left_top(), r.right_bottom(), r.left_bottom(), col);
}

/// Screen angle of `p` about `c` (radians, egui's Y-down screen).
pub fn screen_angle(c: Pos2, p: Pos2) -> f64 {
    f64::from((p.y - c.y).atan2(p.x - c.x))
}

/// The smallest signed difference between two angles (radians).
pub fn wrap(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    let x = (a + std::f64::consts::PI).rem_euclid(t) - std::f64::consts::PI;
    if x.is_finite() { x } else { 0.0 }
}

/// Millimetres moved along world axis `k` for a pointer move `delta` (px)
/// at `center`: the pointer's motion projected on the axis as it is drawn.
pub fn along_axis(f: &Frame, center: Vec3, k: usize, delta: egui::Vec2) -> f64 {
    let (Some(a), Some(b)) = (f.project(center), f.project(center + axis_vec(k))) else {
        return 0.0;
    };
    let v = vec2((b.x - a.x) as f32, (b.y - a.y) as f32);
    let len2 = v.length_sq();
    if len2 < 1e-6 {
        return 0.0;
    }
    f64::from(delta.dot(v) / len2)
}

/// The new placement while `drag` is at `p`. `uniform` keeps proportions
/// for an axis scale; `snap` rounds angles to 15°.
pub fn apply(
    f: &Frame,
    drag: &GizmoDrag,
    gizmo: &Gizmo,
    p: Pos2,
    uniform: bool,
    snap: bool,
) -> Option<Transform> {
    let mut t = drag.start;
    match drag.handle {
        Handle::MoveAxis(k) => {
            let mm = along_axis(f, drag.center, k, p - drag.from);
            let d = axis_vec(k).scale(mm);
            t.translate = Vec3::new(t.translate.x + d.x, t.translate.y + d.y, 0.0);
        }
        Handle::MovePlane => {
            let at = f.bed_point(f64::from(p.x), f64::from(p.y))?;
            t.translate = Vec3::new(at.x + drag.grab.x, at.y + drag.grab.y, 0.0);
        }
        Handle::Ring(k) => {
            let c = pos(f.project(drag.center)?);
            let turned = wrap(screen_angle(c, p) - drag.grab.x);
            // seen from the axis's own tip a turn on screen is mirrored
            let facing = f.toward_viewer(drag.center).dot(axis_vec(k)) > 0.0;
            let mut deg = (if facing { -turned } else { turned }).to_degrees();
            deg = if snap { (deg / 15.0).round() * 15.0 } else { deg.round() };
            let r = match k {
                0 => &mut t.rotate.x,
                1 => &mut t.rotate.y,
                _ => &mut t.rotate.z,
            };
            *r = (*r + deg) % 360.0;
        }
        Handle::ScaleAxis(k) => {
            let mm = along_axis(f, drag.center, k, p - drag.from);
            let half = drag.grab.x.max(0.5);
            let factor = ((half + mm) / half).clamp(0.02, 50.0);
            if uniform {
                t.scale = t.scale.scale(factor);
            } else {
                match k {
                    0 => t.scale.x *= factor,
                    1 => t.scale.y *= factor,
                    _ => t.scale.z *= factor,
                }
            }
        }
        Handle::ScaleAll => {
            let c = pos(f.project(drag.center)?);
            let d0 = (drag.from - c).length().max(4.0);
            let factor = f64::from(((p - c).length() / d0).clamp(0.02, 50.0));
            t.scale = t.scale.scale(factor);
        }
    }
    let _ = gizmo;
    t.is_valid().then_some(t)
}

/// The angle turned so far by a ring drag, in the ring's own plane, for the dial.
pub fn ring_turned(f: &Frame, drag: &GizmoDrag, p: Pos2, snap: bool) -> f64 {
    let Some(c) = f.project(drag.center).map(pos) else { return 0.0 };
    let turned = wrap(screen_angle(c, p) - drag.grab.x);
    let k = match drag.handle {
        Handle::Ring(k) => k,
        _ => return 0.0,
    };
    let facing = f.toward_viewer(drag.center).dot(axis_vec(k)) > 0.0;
    let mut deg = (if facing { -turned } else { turned }).to_degrees();
    deg = if snap { (deg / 15.0).round() * 15.0 } else { deg.round() };
    deg.to_radians()
}

// ---------------------------------------------------------------------------
// view cube

/// The faces of the view cube: outward normal, label, preset.
const FACES: [(Vec3, &str, ViewPreset); 6] = [
    (Vec3::new(0.0, 0.0, 1.0), "Top", ViewPreset::Top),
    (Vec3::new(0.0, 0.0, -1.0), "Bottom", ViewPreset::Bottom),
    (Vec3::new(0.0, -1.0, 0.0), "Front", ViewPreset::Front),
    (Vec3::new(0.0, 1.0, 0.0), "Back", ViewPreset::Back),
    (Vec3::new(-1.0, 0.0, 0.0), "Left", ViewPreset::Left),
    (Vec3::new(1.0, 0.0, 0.0), "Right", ViewPreset::Right),
];

/// A face's four corners on the unit cube, wound counter-clockwise from outside.
fn face_corners(n: Vec3) -> [Vec3; 4] {
    // two directions across the face
    let (u, v) = if n.z.abs() > 0.5 {
        (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, n.z.signum(), 0.0))
    } else if n.y.abs() > 0.5 {
        (Vec3::new(-n.y.signum(), 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0))
    } else {
        (Vec3::new(0.0, n.x.signum(), 0.0), Vec3::new(0.0, 0.0, 1.0))
    };
    [n - u - v, n + u - v, n + u + v, n - u + v]
}

/// The view cube: a small cube turned like the scene, whose faces are named
/// and clickable. Returns the preset of a clicked face. `area` is where it
/// is drawn.
pub fn view_cube(ui: &mut Ui, f: &Frame, area: Rect) -> Option<ViewPreset> {
    let p = theme::of(ui);
    let (right, up, forward) = f.axes();
    let c = area.center();
    let s = f64::from(area.width().min(area.height())) * 0.25;
    let to_screen = |v: Vec3| pos2(c.x + (v.dot(right) * s) as f32, c.y - (v.dot(up) * s) as f32);
    let resp = ui.interact(area, ui.id().with("view-cube"), egui::Sense::click());
    let hover = resp.hover_pos();
    let mut faces: Vec<(f64, [Pos2; 4], &str, ViewPreset)> = FACES
        .iter()
        .filter(|(n, _, _)| n.dot(forward) < -0.08)
        .map(|(n, label, preset)| {
            let q = face_corners(*n).map(to_screen);
            (n.dot(forward), q, *label, *preset)
        })
        .collect();
    // farthest first, so nearer faces paint over them
    faces.sort_by(|a, b| b.0.total_cmp(&a.0));
    let inside = |q: &[Pos2; 4], pt: Pos2| {
        let mut sign = 0.0f32;
        for i in 0..4 {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            let cross = (b - a).x * (pt - a).y - (b - a).y * (pt - a).x;
            if cross.abs() < 1e-6 {
                continue;
            }
            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                return false;
            }
        }
        true
    };
    let hovered = hover.and_then(|h| faces.iter().rev().find(|f| inside(&f.1, h)).map(|f| f.3));
    let painter = ui.painter_at(area);
    for (facing, q, label, preset) in &faces {
        let shade = (0.55 + 0.45 * (-facing).clamp(0.0, 1.0)) as f32;
        let base = if hovered == Some(*preset) { p.accent } else { p.card };
        let fill =
            if hovered == Some(*preset) { base } else { base.gamma_multiply(shade.max(0.7)) };
        painter.add(egui::Shape::convex_polygon(q.to_vec(), fill, egui::Stroke::new(1.0, p.edge)));
        if -facing > 0.35 {
            let centre = q.iter().fold(Pos2::ZERO, |acc, v| acc + v.to_vec2()) / 4.0;
            let colour = if hovered == Some(*preset) { p.on_accent } else { p.text };
            let size = (11.0 + 3.0 * (-facing) as f32).min(13.5);
            painter.text(centre, Align2::CENTER_CENTER, tr(label), theme::strong(size), colour);
        }
    }
    // axis hints under the cube
    for (k, letter) in ["X", "Y", "Z"].iter().enumerate() {
        let a = Vec3::new(-1.25, -1.25, -1.25);
        let b = a + axis_vec(k).scale(0.9);
        let (pa, pb) = (to_screen(a), to_screen(b));
        painter.line_segment([pa, pb], egui::Stroke::new(2.0, colour(k)));
        painter.text(pb, Align2::CENTER_CENTER, *letter, FontId::proportional(10.0), colour(k));
    }
    let picked = if resp.clicked() { hovered } else { None };
    if let Some(h) = hovered {
        resp.on_hover_text(format!("{} {}", tr("Look from"), tr(h.label())));
    }
    picked
}

// ---------------------------------------------------------------------------
// the tool panel

/// The floating panel of the active tool: exact values and quick actions
/// for the selection. Drawn at `at` (its top-left corner).
pub fn tool_panel(app: &mut SliceCraftApp, ui: &mut Ui, at: Pos2) {
    let tool = app.tool;
    if !matches!(tool, Tool::Move | Tool::Rotate | Tool::Scale) {
        return;
    }
    let title = match tool {
        Tool::Move => "Move",
        Tool::Rotate => "Rotate",
        _ => "Scale",
    };
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(at, vec2(262.0, 420.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            theme::overlay_frame(ui).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(236.0);
                ui.label(theme::title_text(ui, title));
                ui.add_space(2.0);
                let Some(i) = app.primary() else {
                    theme::dim(ui, tr("Select a model in the list or the 3D view."));
                    return;
                };
                let Some(o) = app.session.document().objects.get(i) else { return };
                let original = o.original_size();
                let mut t = o.transform;
                let before = t;
                let bed = app.session.document().bed.clone();
                let many = app.selected.len() > 1;
                match tool {
                    Tool::Move => {
                        move_fields(ui, &mut t, (bed.width, bed.depth));
                        theme::dim(ui, tr("Drag an arrow to move along it, the square to move freely."));
                        ui.horizontal(|ui| {
                            if ui.button(tr("Centre on the bed")).clicked() {
                                app.center_selected();
                            }
                            if ui.button(tr("Arrange all")).clicked() {
                                app.run(Command::Arrange);
                            }
                        });
                    }
                    Tool::Rotate => {
                        rotate_fields(ui, &mut t);
                        theme::dim(ui, tr("Drag a ring to turn about its axis; hold Shift for 15° steps."));
                        ui.horizontal(|ui| {
                            if ui.button(tr("Lay flat")).on_hover_text(tr("Put the largest flat face on the bed")).clicked() {
                                app.for_selected(|index| Command::LayFlat { index });
                            }
                            if ui.button(tr("Reset rotation")).clicked() {
                                t.rotate = Vec3::ZERO;
                            }
                        });
                    }
                    _ => {
                        let mut uniform = app.uniform_scale;
                        scale_fields(ui, &mut t, original, &mut uniform);
                        app.uniform_scale = uniform;
                        theme::dim(ui, tr("Drag a coloured square to stretch one way, the white corner to scale evenly."));
                        ui.horizontal(|ui| {
                            if ui.button(tr("Fit the bed")).clicked() {
                                app.scale_to_fit_bed();
                            }
                            if ui.button(tr("Reset size")).clicked() {
                                t.scale = Vec3::new(1.0, 1.0, 1.0);
                            }
                        });
                    }
                }
                if many {
                    theme::dim(ui, format!("{} {}", app.selected.len(), tr("models selected; the fields show the first")));
                }
                if t != before && t.is_valid() {
                    if tool == Tool::Move {
                        app.move_with_group(i, t.translate);
                    } else {
                        app.place(i, t);
                    }
                }
            });
        },
    );
}

fn field_row(ui: &mut Ui, k: usize, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        theme::axis_chip(ui, k);
        add(ui);
    });
}

fn move_fields(ui: &mut Ui, t: &mut Transform, (w, d): (f64, f64)) {
    for (k, v, lim) in [(0, &mut t.translate.x, w), (1, &mut t.translate.y, d)] {
        field_row(ui, k, |ui| {
            ui.add_sized(
                [120.0, 24.0],
                egui::DragValue::new(v).speed(0.5).range(-lim..=2.0 * lim).suffix(" mm"),
            );
        });
    }
}

fn rotate_fields(ui: &mut Ui, t: &mut Transform) {
    for (k, r) in [(0, &mut t.rotate.x), (1, &mut t.rotate.y), (2, &mut t.rotate.z)] {
        field_row(ui, k, |ui| {
            ui.add_sized(
                [84.0, 24.0],
                egui::DragValue::new(r).speed(1.0).range(-360.0..=360.0).suffix("°"),
            );
            if ui.button("−90°").clicked() {
                *r = (*r - 90.0) % 360.0;
            }
            if ui.button("+90°").clicked() {
                *r = (*r + 90.0) % 360.0;
            }
        });
    }
}

fn scale_fields(ui: &mut Ui, t: &mut Transform, original: Vec3, uniform: &mut bool) {
    ui.checkbox(uniform, tr("Keep proportions"));
    let mut size =
        Vec3::new(original.x * t.scale.x, original.y * t.scale.y, original.z * t.scale.z);
    let mut pct = t.scale.scale(100.0);
    let mut changed: Option<(usize, f64)> = None;
    for k in 0..3 {
        let (sz, pc, orig) = match k {
            0 => (&mut size.x, &mut pct.x, original.x),
            1 => (&mut size.y, &mut pct.y, original.y),
            _ => (&mut size.z, &mut pct.z, original.z),
        };
        field_row(ui, k, |ui| {
            if ui
                .add_sized(
                    [100.0, 24.0],
                    egui::DragValue::new(sz).speed(0.5).range(0.01..=10_000.0).suffix(" mm"),
                )
                .changed()
                && orig > 0.0
            {
                changed = Some((k, *sz / orig));
            }
            if ui
                .add_sized(
                    [84.0, 24.0],
                    egui::DragValue::new(pc).speed(1.0).range(0.1..=100_000.0).suffix(" %"),
                )
                .changed()
            {
                changed = Some((k, *pc / 100.0));
            }
        });
    }
    if let Some((k, factor)) = changed
        && factor.is_finite()
        && factor > 0.0
    {
        let old = match k {
            0 => t.scale.x,
            1 => t.scale.y,
            _ => t.scale.z,
        };
        if *uniform {
            let ratio = if old > 0.0 { factor / old } else { 1.0 };
            t.scale = t.scale.scale(ratio);
        } else {
            match k {
                0 => t.scale.x = factor,
                1 => t.scale.y = factor,
                _ => t.scale.z = factor,
            }
        }
    }
}

/// Mirror the selection, saying what happened: a symmetric model looks the
/// same afterwards, so the status line is the only sign.
pub fn mirror(app: &mut SliceCraftApp, axis: Axis) {
    let n = app.selected.len();
    if n == 0 {
        app.info(tr("Select a model to mirror."));
        return;
    }
    app.mirror_selected(axis);
    let how = match axis {
        Axis::X => tr("left to right (X)"),
        Axis::Y => tr("front to back (Y)"),
        Axis::Z => tr("upside down (Z)"),
    };
    app.info(format!("{}: {n} · {how}", tr("Mirrored")));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{OrbitCamera, Viewport};

    fn frame(preset: ViewPreset) -> Frame {
        let mut cam = OrbitCamera::for_bed(220.0, 220.0, 250.0);
        cam.target = Vec3::new(110.0, 110.0, 10.0);
        cam.look(preset);
        cam.frame(Viewport { left: 0.0, top: 0.0, width: 1000.0, height: 800.0 })
    }

    fn cube_bounds() -> Bounds3 {
        let mut b = Bounds3::default();
        b.add(Vec3::new(100.0, 100.0, 0.0));
        b.add(Vec3::new(120.0, 120.0, 20.0));
        b
    }

    /// The screen point of `v`, which the tests' cameras always see.
    fn at(f: &Frame, v: Vec3) -> Pos2 {
        let p = f.project(v).map(pos);
        assert!(p.is_some(), "{v:?} is behind the camera");
        p.unwrap_or(Pos2::ZERO)
    }

    fn gizmo(f: &Frame) -> Gizmo {
        let g = Gizmo::new(f, cube_bounds());
        assert!(g.is_some(), "no gizmo");
        g.unwrap_or(Gizmo { center: Vec3::ZERO, arm: 1.0, ring: 1.0, bounds: cube_bounds() })
    }

    fn placed(f: &Frame, d: &GizmoDrag, g: &Gizmo, p: Pos2, uniform: bool) -> Transform {
        let t = apply(f, d, g, p, uniform, false);
        assert!(t.is_some(), "no placement");
        t.unwrap_or_default()
    }

    fn drag(handle: Handle, from: Pos2, g: &Gizmo, grab: Vec3) -> GizmoDrag {
        GizmoDrag { index: 0, handle, start: Transform::default(), from, center: g.center, grab }
    }

    #[test]
    fn dragging_the_x_arrow_moves_along_x_only() {
        let f = frame(ViewPreset::Iso);
        let g = gizmo(&f);
        let c = at(&f, g.center);
        let tip = at(&f, g.center + axis_vec(0).scale(g.arm));
        // grab the middle of the X arrow
        let mid = c + (tip - c) * 0.6;
        assert_eq!(g.hit(&f, Tool::Move, mid), Some(Handle::MoveAxis(0)));
        let d = drag(Handle::MoveAxis(0), mid, &g, Vec3::ZERO);
        // pull the pointer to the tip and a bit sideways
        let t = placed(&f, &d, &g, tip + (tip - c).rot90() * 0.2, false);
        assert!(t.translate.x > 0.0, "{:?}", t.translate);
        assert!(t.translate.y.abs() < 1e-9 && t.translate.z == 0.0);
    }

    #[test]
    fn a_quarter_turn_of_the_z_ring_seen_from_above_is_90_degrees() {
        let f = frame(ViewPreset::Top);
        let g = gizmo(&f);
        let c = at(&f, g.center);
        let start = at(&f, g.ring_point(2, std::f64::consts::FRAC_PI_4));
        assert_eq!(g.hit(&f, Tool::Rotate, start), Some(Handle::Ring(2)));
        let end = at(&f, g.ring_point(2, 3.0 * std::f64::consts::FRAC_PI_4));
        let d = drag(Handle::Ring(2), start, &g, Vec3::new(screen_angle(c, start), 0.0, 0.0));
        let t = placed(&f, &d, &g, end, false);
        // counter-clockwise seen from above: +90 about Z
        assert!((t.rotate.z - 90.0).abs() < 1.5, "{}", t.rotate.z);
        assert_eq!(t.rotate.x, 0.0);
    }

    #[test]
    fn scaling_by_a_handle_keeps_proportions_when_asked() {
        let f = frame(ViewPreset::Front);
        let g = gizmo(&f);
        let tip = at(&f, g.scale_tip(0));
        assert_eq!(g.hit(&f, Tool::Scale, tip), Some(Handle::ScaleAxis(0)));
        // 10 mm half width, pulled out by 10 mm: twice the size
        let d = drag(Handle::ScaleAxis(0), tip, &g, Vec3::new(10.0, 0.0, 0.0));
        let mm = at(&f, g.center + axis_vec(0)) - at(&f, g.center);
        let to = tip + mm * 10.0;
        let free = placed(&f, &d, &g, to, false);
        assert!((free.scale.x - 2.0).abs() < 0.02 && (free.scale.y - 1.0).abs() < 1e-9);
        let even = placed(&f, &d, &g, to, true);
        assert!((even.scale.y - even.scale.x).abs() < 1e-9 && (even.scale.z - 2.0).abs() < 0.02);
    }

    #[test]
    fn the_view_cube_shows_the_faces_toward_the_camera() {
        let f = frame(ViewPreset::Front);
        let (_, _, forward) = f.axes();
        let shown: Vec<&str> =
            FACES.iter().filter(|(n, _, _)| n.dot(forward) < -0.08).map(|(_, l, _)| *l).collect();
        assert_eq!(shown, vec!["Front"]);
        let f = frame(ViewPreset::Iso);
        let (_, _, forward) = f.axes();
        let shown: Vec<&str> =
            FACES.iter().filter(|(n, _, _)| n.dot(forward) < -0.08).map(|(_, l, _)| *l).collect();
        assert!(shown.contains(&"Top") && shown.contains(&"Front"), "{shown:?}");
    }

    #[test]
    fn angles_wrap_to_the_short_way_round() {
        assert!((wrap(3.5 * std::f64::consts::PI) + 0.5 * std::f64::consts::PI).abs() < 1e-9);
        assert!((wrap(0.25) - 0.25).abs() < 1e-12);
        assert_eq!(wrap(f64::NAN), 0.0);
    }
}
