//! Toolbar icons, drawn in code (no image assets): simple line glyphs on a
//! 20-unit grid, scaled into the button.

use eframe::egui::{
    self, Color32, CornerRadius, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    ViewIso,
    ViewFront,
    ViewBack,
    ViewLeft,
    ViewRight,
    ViewTop,
    ViewBottom,
    Fit,
    Grid,
    Section,
    Select,
    Move,
    Rotate,
    Scale,
    Import,
    Arrange,
    Duplicate,
    MirrorX,
    MirrorY,
    Drop,
    LayFlat,
    Center,
    Delete,
    Undo,
    Redo,
    Slice,
    Export,
    Settings,
    Supports,
    SupportAdd,
    SupportRemove,
    Machine,
    Analyze,
    Repair,
    Eye,
    EyeOff,
    Play,
    Pause,
    StepBack,
    StepForward,
    Plus,
    Wireframe,
}

/// Map a point on the 20×20 design grid into `r`.
fn at(r: Rect, x: f32, y: f32) -> Pos2 {
    pos2(r.left() + r.width() * x / 20.0, r.top() + r.height() * y / 20.0)
}

fn line(p: &Painter, r: Rect, pts: &[(f32, f32)], s: Stroke) {
    let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| at(r, x, y)).collect();
    p.add(Shape::line(pts, s));
}

fn closed(p: &Painter, r: Rect, pts: &[(f32, f32)], s: Stroke, fill: Color32) {
    let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| at(r, x, y)).collect();
    p.add(Shape::convex_polygon(pts, fill, s));
}

/// A small cube in oblique view; `face` highlights one side.
fn cube(p: &Painter, r: Rect, s: Stroke, face: Option<&[(f32, f32)]>, hi: Color32) {
    if let Some(f) = face {
        closed(p, r, f, Stroke::NONE, hi);
    }
    line(p, r, &[(4.0, 7.0), (12.0, 7.0), (12.0, 17.0), (4.0, 17.0), (4.0, 7.0)], s);
    line(p, r, &[(4.0, 7.0), (8.0, 3.0), (16.0, 3.0), (12.0, 7.0)], s);
    line(p, r, &[(16.0, 3.0), (16.0, 13.0), (12.0, 17.0)], s);
}

const FRONT: &[(f32, f32)] = &[(4.0, 7.0), (12.0, 7.0), (12.0, 17.0), (4.0, 17.0)];
const TOP: &[(f32, f32)] = &[(4.0, 7.0), (8.0, 3.0), (16.0, 3.0), (12.0, 7.0)];
const RIGHT: &[(f32, f32)] = &[(12.0, 7.0), (16.0, 3.0), (16.0, 13.0), (12.0, 17.0)];

fn arrow(p: &Painter, r: Rect, from: (f32, f32), to: (f32, f32), s: Stroke) {
    line(p, r, &[from, to], s);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (ux, uy) = (dx / len, dy / len);
    let head = 3.0;
    let left = (to.0 - head * (ux - uy * 0.7), to.1 - head * (uy + ux * 0.7));
    let right = (to.0 - head * (ux + uy * 0.7), to.1 - head * (uy - ux * 0.7));
    line(p, r, &[left, to, right], s);
}

/// Paint `icon` into `r` with colour `c`.
pub fn paint(p: &Painter, r: Rect, icon: Icon, c: Color32) {
    let s = Stroke::new((r.width() / 14.0).max(1.2), c);
    let hi = c.gamma_multiply(0.45);
    match icon {
        Icon::ViewIso => cube(p, r, s, None, hi),
        Icon::ViewFront => cube(p, r, s, Some(FRONT), hi),
        Icon::ViewTop => cube(p, r, s, Some(TOP), hi),
        Icon::ViewRight => cube(p, r, s, Some(RIGHT), hi),
        Icon::ViewBack | Icon::ViewLeft | Icon::ViewBottom => {
            cube(p, r, s, None, hi);
            // a dashed hint that the hidden face is meant
            let dash = Stroke::new(s.width * 0.7, c.gamma_multiply(0.7));
            match icon {
                Icon::ViewBack => line(p, r, &[(8.0, 3.0), (8.0, 13.0), (16.0, 13.0)], dash),
                Icon::ViewLeft => line(p, r, &[(8.0, 3.0), (8.0, 13.0), (4.0, 17.0)], dash),
                _ => line(p, r, &[(4.0, 17.0), (8.0, 13.0), (16.0, 13.0)], dash),
            }
        }
        Icon::Fit => {
            for (a, b, cc) in [
                ((3.0, 7.0), (3.0, 3.0), (7.0, 3.0)),
                ((13.0, 3.0), (17.0, 3.0), (17.0, 7.0)),
                ((17.0, 13.0), (17.0, 17.0), (13.0, 17.0)),
                ((7.0, 17.0), (3.0, 17.0), (3.0, 13.0)),
            ] {
                line(p, r, &[a, b, cc], s);
            }
            p.rect_stroke(
                Rect::from_min_max(at(r, 7.0, 7.0), at(r, 13.0, 13.0)),
                CornerRadius::ZERO,
                s,
                StrokeKind::Middle,
            );
        }
        Icon::Grid => {
            for k in [4.0, 8.0, 12.0, 16.0] {
                line(p, r, &[(k, 3.0), (k, 17.0)], s);
                line(p, r, &[(3.0, k), (17.0, k)], s);
            }
        }
        Icon::Section => {
            cube(p, r, s, None, hi);
            closed(p, r, &[(1.0, 12.0), (14.0, 12.0), (19.0, 8.0), (6.0, 8.0)], Stroke::NONE, hi);
            line(p, r, &[(1.0, 12.0), (14.0, 12.0), (19.0, 8.0), (6.0, 8.0), (1.0, 12.0)], s);
        }
        Icon::Select => {
            closed(
                p,
                r,
                &[
                    (5.0, 3.0),
                    (15.0, 12.0),
                    (10.5, 12.5),
                    (13.0, 17.5),
                    (11.0, 18.0),
                    (8.5, 13.5),
                    (5.0, 16.0),
                ],
                s,
                Color32::TRANSPARENT,
            );
        }
        Icon::Move => {
            arrow(p, r, (10.0, 10.0), (10.0, 2.5), s);
            arrow(p, r, (10.0, 10.0), (10.0, 17.5), s);
            arrow(p, r, (10.0, 10.0), (2.5, 10.0), s);
            arrow(p, r, (10.0, 10.0), (17.5, 10.0), s);
        }
        Icon::Rotate | Icon::Undo | Icon::Redo => {
            let center = at(r, 10.0, 11.0);
            let rad = r.width() * 6.0 / 20.0;
            let (from, to) = match icon {
                Icon::Undo => (0.6f32, 4.2f32),
                Icon::Redo => (-1.0, 2.6),
                _ => (-0.4, 4.5),
            };
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = from + (to - from) * i as f32 / 16.0;
                    center + vec2(a.cos(), -a.sin()) * rad
                })
                .collect();
            let end = pts.last().copied().unwrap_or(center);
            p.add(Shape::line(pts, s));
            let tip = to;
            let tan = vec2(-tip.sin(), -tip.cos());
            let normal = vec2(tan.y, -tan.x);
            let h = r.width() * 3.0 / 20.0;
            p.add(Shape::line(
                vec![end - tan * h + normal * h, end, end - tan * h - normal * h],
                s,
            ));
        }
        Icon::Scale => {
            p.rect_stroke(
                Rect::from_min_max(at(r, 3.0, 9.0), at(r, 11.0, 17.0)),
                CornerRadius::ZERO,
                s,
                StrokeKind::Middle,
            );
            line(p, r, &[(7.0, 5.0), (15.0, 5.0), (15.0, 13.0)], Stroke::new(s.width * 0.7, c));
            arrow(p, r, (9.0, 11.0), (17.0, 3.0), s);
        }
        Icon::Import => {
            line(p, r, &[(3.0, 12.0), (3.0, 17.0), (17.0, 17.0), (17.0, 12.0)], s);
            arrow(p, r, (10.0, 2.0), (10.0, 13.0), s);
        }
        Icon::Export => {
            line(p, r, &[(3.0, 12.0), (3.0, 17.0), (17.0, 17.0), (17.0, 12.0)], s);
            arrow(p, r, (10.0, 13.0), (10.0, 2.0), s);
        }
        Icon::Arrange => {
            for (x, y) in [(3.0, 3.0), (11.0, 3.0), (3.0, 11.0), (11.0, 11.0)] {
                p.rect_stroke(
                    Rect::from_min_max(at(r, x, y), at(r, x + 6.0, y + 6.0)),
                    CornerRadius::ZERO,
                    s,
                    StrokeKind::Middle,
                );
            }
        }
        Icon::Duplicate => {
            p.rect_stroke(
                Rect::from_min_max(at(r, 3.0, 7.0), at(r, 13.0, 17.0)),
                CornerRadius::ZERO,
                s,
                StrokeKind::Middle,
            );
            line(p, r, &[(7.0, 7.0), (7.0, 3.0), (17.0, 3.0), (17.0, 13.0), (13.0, 13.0)], s);
        }
        Icon::MirrorX | Icon::MirrorY => {
            let dash = Stroke::new(s.width * 0.7, c.gamma_multiply(0.7));
            line(p, r, &[(10.0, 2.0), (10.0, 18.0)], dash);
            closed(p, r, &[(8.0, 5.0), (8.0, 15.0), (2.0, 15.0)], s, hi);
            closed(p, r, &[(12.0, 5.0), (18.0, 15.0), (12.0, 15.0)], s, Color32::TRANSPARENT);
            if icon == Icon::MirrorY {
                // a small Y marker
                line(p, r, &[(15.0, 2.0), (16.5, 4.0), (18.0, 2.0)], dash);
                line(p, r, &[(16.5, 4.0), (16.5, 6.0)], dash);
            }
        }
        Icon::Drop => {
            arrow(p, r, (10.0, 2.0), (10.0, 13.0), s);
            line(p, r, &[(2.0, 17.0), (18.0, 17.0)], s);
        }
        Icon::LayFlat => {
            closed(p, r, &[(4.0, 13.0), (16.0, 13.0), (13.0, 5.0)], s, hi);
            line(p, r, &[(2.0, 17.0), (18.0, 17.0)], s);
        }
        Icon::Center => {
            p.circle_stroke(at(r, 10.0, 10.0), r.width() * 5.0 / 20.0, s);
            line(p, r, &[(10.0, 2.0), (10.0, 18.0)], s);
            line(p, r, &[(2.0, 10.0), (18.0, 10.0)], s);
        }
        Icon::Delete => {
            line(p, r, &[(3.0, 5.0), (17.0, 5.0)], s);
            line(p, r, &[(8.0, 5.0), (8.0, 3.0), (12.0, 3.0), (12.0, 5.0)], s);
            line(p, r, &[(5.0, 5.0), (6.0, 17.0), (14.0, 17.0), (15.0, 5.0)], s);
            line(p, r, &[(10.0, 8.0), (10.0, 14.0)], s);
        }
        Icon::Slice => {
            for (k, y) in [5.0, 9.0, 13.0].iter().enumerate() {
                let inset = k as f32;
                line(p, r, &[(3.0 + inset, *y), (17.0 - inset, *y)], s);
            }
            line(p, r, &[(2.0, 17.0), (18.0, 17.0)], s);
        }
        Icon::Settings => {
            let center = at(r, 10.0, 10.0);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([center + d * r.width() * 0.22, center + d * r.width() * 0.42], s);
            }
            p.circle_stroke(center, r.width() * 0.25, s);
            p.circle_stroke(center, r.width() * 0.09, s);
        }
        Icon::Supports | Icon::SupportAdd | Icon::SupportRemove => {
            line(p, r, &[(2.0, 4.0), (18.0, 4.0)], s);
            for x in [5.0, 10.0, 15.0] {
                line(p, r, &[(x, 4.0), (x, 17.0)], Stroke::new(s.width * 0.8, c));
            }
            line(p, r, &[(2.0, 17.0), (18.0, 17.0)], s);
            match icon {
                Icon::SupportAdd => {
                    p.circle_filled(at(r, 15.5, 13.0), r.width() * 0.2, c);
                    let w = Stroke::new(s.width, Color32::BLACK);
                    line(p, r, &[(15.5, 10.8), (15.5, 15.2)], w);
                    line(p, r, &[(13.3, 13.0), (17.7, 13.0)], w);
                }
                Icon::SupportRemove => {
                    p.circle_filled(at(r, 15.5, 13.0), r.width() * 0.2, c);
                    line(p, r, &[(13.3, 13.0), (17.7, 13.0)], Stroke::new(s.width, Color32::BLACK));
                }
                _ => {}
            }
        }
        Icon::Machine => {
            p.rect_stroke(
                Rect::from_min_max(at(r, 3.0, 3.0), at(r, 17.0, 17.0)),
                CornerRadius::ZERO,
                s,
                StrokeKind::Middle,
            );
            line(p, r, &[(3.0, 6.0), (17.0, 6.0)], s);
            closed(p, r, &[(8.0, 6.0), (12.0, 6.0), (10.0, 10.0)], s, hi);
            line(p, r, &[(5.0, 14.0), (15.0, 14.0)], s);
        }
        Icon::Analyze => {
            p.circle_stroke(at(r, 8.0, 8.0), r.width() * 5.0 / 20.0, s);
            line(p, r, &[(11.5, 11.5), (17.0, 17.0)], s);
        }
        Icon::Repair => {
            line(p, r, &[(4.0, 16.0), (12.0, 8.0)], s);
            p.circle_stroke(at(r, 14.0, 6.0), r.width() * 3.5 / 20.0, s);
        }
        Icon::Eye | Icon::EyeOff => {
            let upper: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    (2.0 + 16.0 * t, 10.0 - 6.0 * (std::f32::consts::PI * t).sin())
                })
                .collect();
            let lower: Vec<(f32, f32)> = upper.iter().map(|&(x, y)| (x, 20.0 - y)).collect();
            line(p, r, &upper, s);
            line(p, r, &lower, s);
            p.circle_filled(at(r, 10.0, 10.0), r.width() * 2.6 / 20.0, c);
            if icon == Icon::EyeOff {
                line(p, r, &[(3.0, 17.0), (17.0, 3.0)], s);
            }
        }
        Icon::Play => {
            closed(p, r, &[(6.0, 4.0), (16.0, 10.0), (6.0, 16.0)], Stroke::NONE, c);
        }
        Icon::Pause => {
            closed(p, r, &[(5.0, 4.0), (8.5, 4.0), (8.5, 16.0), (5.0, 16.0)], Stroke::NONE, c);
            closed(p, r, &[(11.5, 4.0), (15.0, 4.0), (15.0, 16.0), (11.5, 16.0)], Stroke::NONE, c);
        }
        Icon::StepBack => {
            closed(p, r, &[(16.0, 4.0), (8.0, 10.0), (16.0, 16.0)], Stroke::NONE, c);
            line(p, r, &[(5.5, 4.0), (5.5, 16.0)], s);
        }
        Icon::StepForward => {
            closed(p, r, &[(4.0, 4.0), (12.0, 10.0), (4.0, 16.0)], Stroke::NONE, c);
            line(p, r, &[(14.5, 4.0), (14.5, 16.0)], s);
        }
        Icon::Wireframe => {
            cube(p, r, s, None, hi);
            let thin = Stroke::new(s.width * 0.6, c.gamma_multiply(0.75));
            line(p, r, &[(4.0, 7.0), (12.0, 17.0)], thin);
            line(p, r, &[(12.0, 7.0), (16.0, 13.0)], thin);
            line(p, r, &[(4.0, 7.0), (16.0, 3.0)], thin);
        }
        Icon::Plus => {
            line(p, r, &[(10.0, 4.0), (10.0, 16.0)], s);
            line(p, r, &[(4.0, 10.0), (16.0, 10.0)], s);
        }
    }
}

/// A square toolbar button with an icon. Disabled buttons still show their
/// tooltip, so a greyed-out button explains itself.
pub fn button(ui: &mut Ui, icon: Icon, tooltip: &str, enabled: bool, active: bool) -> Response {
    sized(ui, icon, tooltip, enabled, active, 32.0)
}

/// [`button`] at a chosen size.
pub fn sized(
    ui: &mut Ui,
    icon: Icon,
    tooltip: &str,
    enabled: bool,
    active: bool,
    side: f32,
) -> Response {
    let size = Vec2::splat(side);
    // a disabled button senses hover only, so it can never report a click
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let p = crate::theme::of(ui);
    let hovered = resp.hovered() && enabled;
    let bg = if active {
        p.accent
    } else if resp.is_pointer_button_down_on() && enabled {
        p.input
    } else if hovered {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), bg);
    let color = if !enabled {
        p.dim.gamma_multiply(0.5)
    } else if active {
        p.on_accent
    } else {
        p.text
    };
    paint(ui.painter(), rect.shrink(side * 0.22), icon, color);
    let label = tooltip.to_string();
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, &label)
    });
    let resp = resp.on_hover_text(tooltip);
    if enabled { resp } else { resp.on_hover_cursor(egui::CursorIcon::NotAllowed) }
}
