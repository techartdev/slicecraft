//! The 3D view: interaction (orbit, pan, zoom, pick, move/rotate/scale tools)
//! and the overlays drawn on top of it.

use crate::app::{Mode, SliceCraftApp, Tool};
use crate::camera::{Frame, Viewport};
use crate::scene::{self, Builder, Coloring, ModelStyle, PreviewStyle};
use eframe::egui::{self, Align2, Color32, FontId, PointerButton, Pos2, Rect, Sense, Ui};
use slicecraft_doc::{SupportEdit, SupportEditKind, Transform};
use slicecraft_engine::Command;
use slicecraft_geom::Vec3;

/// A model drag in progress.
#[derive(Debug, Clone, Copy)]
pub enum Drag {
    Move {
        index: usize,
        grab: Vec3,
    },
    Rotate {
        index: usize,
        start: Transform,
        x0: f32,
    },
    Scale {
        index: usize,
        start: Transform,
        y0: f32,
    },
    /// A support box being drawn on the bed, corner to corner.
    SupportBox {
        kind: SupportEditKind,
        from: (f64, f64),
        to: (f64, f64),
    },
}

fn pick(app: &SliceCraftApp, f: &Frame, p: Pos2) -> Option<usize> {
    let (_, geo) = app.geometry.as_ref()?;
    let mut best: Option<(f64, usize)> = None;
    for (i, o) in geo.objects.iter().enumerate() {
        if let Some((lo, hi, depth)) = scene::screen_box(f, &o.bounds)
            && Rect::from_min_max(lo, hi).contains(p)
            && best.is_none_or(|(d, _)| depth < d)
        {
            best = Some((depth, i));
        }
    }
    best.map(|(_, i)| i)
}

pub fn viewport(app: &mut SliceCraftApp, ui: &mut Ui) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let vp = Viewport {
        left: f64::from(rect.left()),
        top: f64::from(rect.top()),
        width: f64::from(rect.width()),
        height: f64::from(rect.height()),
    };
    let (shift, command) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
    let frame0 = app.camera.frame(vp);

    // support boxes: drag on the bed, the box is added on release
    let support_tool = match app.tool {
        Tool::SupportAdd => Some(SupportEditKind::Add),
        Tool::SupportBlock => Some(SupportEditKind::Block),
        _ => None,
    };
    if let (Mode::Prepare, Some(kind)) = (app.mode, support_tool)
        && resp.drag_started_by(PointerButton::Primary)
        && let Some(p) = resp.interact_pointer_pos()
        && let Some(at) = frame0.bed_point(f64::from(p.x), f64::from(p.y))
    {
        app.drag = Some(Drag::SupportBox { kind, from: (at.x, at.y), to: (at.x, at.y) });
    }
    if let Some(Drag::SupportBox { kind, from, .. }) = app.drag
        && resp.dragged_by(PointerButton::Primary)
        && let Some(p) = resp.interact_pointer_pos()
        && let Some(at) = frame0.bed_point(f64::from(p.x), f64::from(p.y))
    {
        app.drag = Some(Drag::SupportBox { kind, from, to: (at.x, at.y) });
    }
    if let Some(Drag::SupportBox { kind, from, to }) = app.drag
        && resp.drag_stopped()
    {
        app.drag = None;
        let e = SupportEdit::columns(kind, from, to);
        if e.is_valid() {
            app.run(Command::AddSupportEdit(e));
        }
    }

    // start of a left drag: a model tool, or the camera
    if app.mode == Mode::Prepare
        && support_tool.is_none()
        && resp.drag_started_by(PointerButton::Primary)
    {
        app.drag = None;
        let p = resp.interact_pointer_pos();
        let hit = p.and_then(|p| pick(app, &frame0, p));
        let tool = if shift && app.tool == Tool::Select { Tool::Move } else { app.tool };
        let target = match tool {
            Tool::Move => hit,
            _ => hit.or(app.primary()),
        };
        if let (Some(index), Some(p)) = (target, p) {
            let t = app.session.document().objects.get(index).map(|o| o.transform);
            if let Some(t) = t {
                match tool {
                    Tool::Move => {
                        if let Some(at) = frame0.bed_point(f64::from(p.x), f64::from(p.y)) {
                            app.drag = Some(Drag::Move { index, grab: t.translate - at });
                            app.selected = std::iter::once(index).collect();
                        }
                    }
                    Tool::Rotate => app.drag = Some(Drag::Rotate { index, start: t, x0: p.x }),
                    Tool::Scale => app.drag = Some(Drag::Scale { index, start: t, y0: p.y }),
                    Tool::Select | Tool::SupportAdd | Tool::SupportBlock => {}
                }
            }
        }
    }
    let drawing_box = matches!(app.drag, Some(Drag::SupportBox { .. }));
    let dragging_model =
        app.drag.is_some() && !drawing_box && resp.dragged_by(PointerButton::Primary);
    if let (Some(drag), true) = (app.drag, dragging_model) {
        if let Some(p) = resp.interact_pointer_pos() {
            match drag {
                Drag::Move { index, grab } => {
                    if let (Some(at), Some(mut t)) = (
                        frame0.bed_point(f64::from(p.x), f64::from(p.y)),
                        app.session.document().objects.get(index).map(|o| o.transform),
                    ) {
                        t.translate = Vec3::new(at.x + grab.x, at.y + grab.y, 0.0);
                        app.place(index, t);
                    }
                }
                Drag::Rotate { index, start, x0 } => {
                    let mut t = start;
                    let deg = f64::from(p.x - x0) * 0.5;
                    let snapped = if shift { (deg / 15.0).round() * 15.0 } else { deg };
                    t.rotate.z = (start.rotate.z + snapped) % 360.0;
                    app.place(index, t);
                }
                Drag::SupportBox { .. } => {}
                Drag::Scale { index, start, y0 } => {
                    let mut t = start;
                    let k = (-f64::from(p.y - y0) * 0.005).exp();
                    t.scale = start.scale.scale(k);
                    if t.is_valid() {
                        app.place(index, t);
                    }
                }
            }
        }
    } else if resp.dragged_by(PointerButton::Primary) && !drawing_box {
        let d = resp.drag_delta();
        let k = f64::from(app.prefs.orbit_sensitivity) * 0.008;
        app.camera.orbit(-f64::from(d.x) * k, f64::from(d.y) * k);
    }
    if !resp.dragged() {
        app.drag = None;
    }
    if resp.dragged_by(PointerButton::Secondary) || resp.dragged_by(PointerButton::Middle) {
        let d = resp.drag_delta();
        app.camera.pan(f64::from(d.x), f64::from(d.y), vp);
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta().y);
        if scroll != 0.0 {
            let dir = if app.prefs.invert_zoom { 1.0 } else { -1.0 };
            app.camera.zoom((dir * f64::from(scroll) * 0.0018).exp());
        }
    }
    let frame = app.camera.frame(vp);
    if resp.clicked() && app.mode == Mode::Prepare {
        let hit = resp.interact_pointer_pos().and_then(|p| pick(app, &frame, p));
        match (hit, command || shift) {
            (Some(i), true) => {
                if !app.selected.remove(&i) {
                    app.selected.insert(i);
                }
            }
            (Some(i), false) => app.selected = std::iter::once(i).collect(),
            (None, false) => app.selected.clear(),
            (None, true) => {}
        }
    }
    if resp.double_clicked() && app.mode == Mode::Prepare {
        app.fit_view();
    }

    // draw
    let painter = ui.painter_at(rect);
    let bg = if app.prefs.dark {
        Color32::from_rgb(0x1b, 0x1d, 0x21)
    } else {
        Color32::from_rgb(0xd8, 0xdc, 0xe2)
    };
    painter.rect_filled(rect, 0.0, bg);
    let bed = app.session.document().bed.clone();
    let mut b = Builder::default();
    scene::bed(&mut b, &frame, bed.width, bed.depth, bed.height, app.prefs.bed);
    let mut head = None;
    match app.mode {
        Mode::Preview => {
            if let Some((_, layers)) = &app.preview_cache {
                if app.preview.ghost_model
                    && let Some((_, geo)) = &app.geometry
                {
                    let none = Default::default();
                    scene::models(
                        &mut b,
                        &frame,
                        geo,
                        ModelStyle { selected: &none, support: &[], alpha: 0.15, cut_z: None },
                    );
                }
                let p = &app.preview;
                let first = match p.only_n {
                    Some(n) => p.last_layer.saturating_sub(n.saturating_sub(1)),
                    None => p.first_layer,
                };
                let style = PreviewStyle {
                    first_layer: first,
                    last_layer: p.last_layer,
                    moves_on_last: if p.by_line { Some(p.line_cursor) } else { None },
                    coloring: p.coloring,
                    filter: p.filter,
                    show_travel: p.show_travel,
                    show_retractions: p.show_retractions,
                    width_mm: app.session.document().settings.line_width(),
                    speed_range: scene::speed_range(layers),
                    z_range: (0.0, layers.last().map_or(1.0, |l| l.z)),
                };
                head = scene::toolpaths(&mut b, &frame, layers, &style);
                if p.show_toolhead
                    && let Some(h) = head
                {
                    scene::toolhead(&mut b, &frame, h);
                }
            }
        }
        Mode::Prepare => {
            let no_support = Vec::new();
            let support = match (&app.support_view, app.show_support) {
                (Some((_, prisms)), true) => prisms,
                _ => &no_support,
            };
            if let Some((_, geo)) = &app.geometry {
                let style =
                    ModelStyle { selected: &app.selected, support, alpha: 1.0, cut_z: app.section };
                scene::models(&mut b, &frame, geo, style);
            }
            let doc = app.session.document();
            let top = doc.bounds();
            let height = if top.is_empty() { 20.0 } else { top.max().z.max(1.0) };
            scene::support_edits(&mut b, &frame, &doc.support_edits, height);
            if let Some(Drag::SupportBox { kind, from, to }) = app.drag {
                let e = SupportEdit::columns(kind, from, to);
                scene::support_edits(&mut b, &frame, &[e], height);
            }
        }
    }
    if !b.is_empty() {
        painter.add(b.into_shape());
    }
    overlays(app, ui, rect, head);
}

fn overlays(app: &mut SliceCraftApp, ui: &mut Ui, rect: Rect, head: Option<Vec3>) {
    let painter = ui.painter_at(rect);
    let faint = Color32::from_rgb(0x9a, 0xa0, 0xa8);
    if app.session.document().objects.is_empty() && app.mode == Mode::Prepare {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Drop STL or OBJ files here, or use Import",
            FontId::proportional(18.0),
            faint,
        );
    }
    let hint = match (app.mode, app.tool) {
        (Mode::Prepare, Tool::Select) => {
            "drag: orbit · right-drag: pan · wheel: zoom · click: select · shift-drag: move"
        }
        (Mode::Prepare, Tool::Move) => {
            "drag a model to move it on the bed · drag empty space to orbit"
        }
        (Mode::Prepare, Tool::Rotate) => {
            "drag left/right to turn the selection about Z · shift snaps to 15°"
        }
        (Mode::Prepare, Tool::Scale) => "drag up/down to scale the selection",
        (Mode::Prepare, Tool::SupportAdd) => {
            "drag a box on the bed: every surface facing down inside it gets support · Q: back to select"
        }
        (Mode::Prepare, Tool::SupportBlock) => {
            "drag a box on the bed: no support inside it · Q: back to select"
        }
        (Mode::Preview, _) => {
            "drag: orbit · right-drag: pan · wheel: zoom · Up/Down: layer · Space: play"
        }
    };
    painter.text(
        rect.left_bottom() + egui::vec2(10.0, -8.0),
        Align2::LEFT_BOTTOM,
        hint,
        FontId::proportional(12.0),
        faint,
    );

    // section height slider in prepare mode
    if app.mode == Mode::Prepare
        && let Some(z) = &mut app.section
    {
        let top = app.geometry.as_ref().map_or(100.0, |(_, g)| {
            let b = g.bounds();
            if b.is_empty() { 100.0 } else { b.max().z }
        });
        let r = Rect::from_min_max(
            Pos2::new(rect.right() - 54.0, rect.top() + 40.0),
            Pos2::new(rect.right() - 14.0, rect.bottom() - 60.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
            ui.spacing_mut().slider_width = r.height();
            ui.add(egui::Slider::new(z, 0.0..=top.max(0.1)).vertical().show_value(false));
        });
        painter.text(
            Pos2::new(rect.right() - 12.0, rect.top() + 14.0),
            Align2::RIGHT_TOP,
            format!("section at z {:.2} mm", *z),
            FontId::proportional(14.0),
            Color32::WHITE,
        );
    }

    if app.mode != Mode::Preview {
        return;
    }
    let Some((_, layers)) = &app.preview_cache else { return };
    let max = layers.len().saturating_sub(1);
    let z = layers.get(app.preview.last_layer).map_or(0.0, |l| l.z);
    painter.text(
        Pos2::new(rect.right() - 12.0, rect.top() + 14.0),
        Align2::RIGHT_TOP,
        format!("layer {} / {}   z {:.2} mm", app.preview.last_layer + 1, max + 1, z),
        FontId::proportional(14.0),
        Color32::WHITE,
    );
    let r = Rect::from_min_max(
        Pos2::new(rect.right() - 54.0, rect.top() + 40.0),
        Pos2::new(rect.right() - 14.0, rect.bottom() - 60.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
        ui.spacing_mut().slider_width = r.height();
        if ui
            .add(
                egui::Slider::new(&mut app.preview.last_layer, 0..=max)
                    .vertical()
                    .show_value(false),
            )
            .changed()
        {
            app.preview.line_cursor = 0;
        }
    });
    if let Some(h) = head
        && app.preview.show_toolhead
    {
        painter.text(
            rect.left_bottom() + egui::vec2(10.0, -26.0),
            Align2::LEFT_BOTTOM,
            format!("nozzle  X {:.3}  Y {:.3}  Z {:.3}", h.x, h.y, h.z),
            FontId::monospace(12.0),
            faint,
        );
    }
    // legend, with feature toggles when colouring by feature
    let legend = crate::panels::legend(app);
    let by_feature = app.preview.coloring == Coloring::Feature;
    let at = rect.left_top() + egui::vec2(10.0, 10.0);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(at, egui::vec2(200.0, 260.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(app.preview.coloring.label()).strong());
                for (k, (color, label)) in legend.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (sw, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                        ui.painter().rect_filled(sw, 2.0, *color);
                        match (by_feature, app.preview.filter.0.get_mut(k)) {
                            (true, Some(on)) => {
                                ui.checkbox(on, label.as_str());
                            }
                            _ => {
                                ui.label(label.as_str());
                            }
                        }
                    });
                }
            });
        },
    );
}
