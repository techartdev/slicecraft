//! The 3D view: interaction (orbit, pan, zoom, pick, move/rotate/scale tools)
//! and the overlays drawn on top of it.

use crate::app::{Mode, SliceCraftApp, Tool};
use crate::camera::{Frame, ViewPreset, Viewport};
use crate::gizmo::{self, Gizmo, GizmoDrag, Handle};
use crate::gpu;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::scene::{self, Builder, Coloring, ModelStyle, PreviewStyle};
use crate::theme;
use eframe::egui::{self, Align2, Color32, FontId, PointerButton, Pos2, Rect, Sense, Ui};
use slicecraft_doc::{SupportEdit, SupportEditKind};
use slicecraft_engine::Command;
use slicecraft_geom::Vec3;

/// A model drag in progress.
#[derive(Debug, Clone, Copy)]
pub enum Drag {
    Move {
        index: usize,
        grab: Vec3,
    },
    /// A handle of the move, rotate or scale tool.
    Gizmo(GizmoDrag),
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

/// Draw the bed, models and toolpaths on the graphics card, under
/// everything else. False when the processor has to draw them instead: no
/// graphics card, turned off in Preferences, or the card refused the work
/// (which turns it off for the session, with a note).
fn draw_on_gpu(app: &mut SliceCraftApp, ui: &Ui, frame: &Frame, rect: Rect) -> bool {
    if !app.prefs.gpu_view || rect.width() < 2.0 || rect.height() < 2.0 {
        return false;
    }
    let Some(gpu) = app.gpu.as_mut() else { return false };
    let doc = app.session.document();
    let none = std::collections::BTreeSet::new();
    let no_support: Vec<scene::Prism> = Vec::new();
    let support = match (&app.support_view, app.show_support) {
        (Some((_, prisms)), true) => prisms,
        _ => &no_support,
    };
    let models = app.geometry.as_ref().and_then(|(revision, geometry)| match app.mode {
        Mode::Prepare if !app.prefs.wireframe => Some(gpu::Models {
            geometry,
            revision: *revision,
            selected: &app.selected,
            support,
            cut_z: app.section,
            ghost: false,
        }),
        Mode::Preview if app.preview.ghost_model => Some(gpu::Models {
            geometry,
            revision: *revision,
            selected: &none,
            support: &[],
            cut_z: None,
            ghost: true,
        }),
        _ => None,
    });
    let p = &app.preview;
    let paths = match (app.mode, &app.preview_cache) {
        (Mode::Preview, Some((key, layers))) => Some(gpu::Paths {
            layers,
            key: *key,
            settings: &doc.settings,
            first_layer: match p.only_n {
                Some(n) => p.last_layer.saturating_sub(n.saturating_sub(1)),
                None => p.first_layer,
            },
            last_layer: p.last_layer,
            moves_on_last: if p.by_line { Some(p.line_cursor) } else { None },
            coloring: p.coloring,
            filter: p.filter,
            show_travel: p.show_travel,
        }),
        _ => None,
    };
    let scene = gpu::Scene {
        frame,
        size: rect.size(),
        pixels_per_point: ui.ctx().pixels_per_point(),
        dark: app.prefs.dark,
        bed: (doc.bed.width, doc.bed.depth, doc.bed.height),
        round_bed: doc.settings.bed_shape == slicecraft_doc::BedShape::Circle,
        bed_style: app.prefs.bed,
        models,
        paths,
    };
    match gpu.render(&scene) {
        Ok(id) => {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            ui.painter_at(rect).image(id, rect, uv, Color32::WHITE);
            true
        }
        Err(e) => {
            app.gpu = None;
            app.error(format!("{} {e}", tr("The 3D view moved to the processor:")));
            app.gpu_note = Some(e);
            false
        }
    }
}

/// The handles of the selected model, when the active tool has handles.
fn selection_gizmo(app: &SliceCraftApp, f: &Frame) -> Option<Gizmo> {
    if app.mode != Mode::Prepare || !matches!(app.tool, Tool::Move | Tool::Rotate | Tool::Scale) {
        return None;
    }
    let index = app.primary()?;
    Gizmo::new(f, g_bounds(app, index))
}

/// A model's placed bounds, as last drawn.
fn g_bounds(app: &SliceCraftApp, index: usize) -> slicecraft_geom::Bounds3 {
    app.geometry
        .as_ref()
        .and_then(|(_, g)| g.objects.get(index))
        .map(|o| o.bounds)
        .unwrap_or_default()
}

fn ring_axis(h: Handle) -> usize {
    match h {
        Handle::Ring(k) => k,
        _ => 2,
    }
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
    app.camera.ortho = app.prefs.ortho;
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

    // a click with a support tool edits one pillar: block removes the pillar
    // under the pointer, add puts a pillar where the pointer meets the bed
    if let (Mode::Prepare, Some(kind)) = (app.mode, support_tool)
        && resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (x, y) = (f64::from(p.x), f64::from(p.y));
        match kind {
            SupportEditKind::Block => {
                let (origin, dir) = frame0.ray(x, y);
                let hit = app.support_view.as_ref().and_then(|(_, prisms)| {
                    prisms
                        .iter()
                        .filter_map(|pr| pr.hit(origin, dir).map(|t| (t, pr)))
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .and_then(|(_, pr)| pr.footprint())
                });
                match hit {
                    Some((x0, y0, x1, y1)) => {
                        let e = SupportEdit::columns(
                            kind,
                            (x0 - 0.05, y0 - 0.05),
                            (x1 + 0.05, y1 + 0.05),
                        );
                        if e.is_valid() {
                            app.run(Command::AddSupportEdit(e));
                        }
                    }
                    None => app
                        .info("No support pillar there. Turn on Show before slicing to see them."),
                }
            }
            SupportEditKind::Add => {
                if let Some(at) = frame0.bed_point(x, y) {
                    let half = app.session.document().settings.support_pillar_size.max(1.0) / 2.0;
                    let e = SupportEdit::columns(
                        kind,
                        (at.x - half, at.y - half),
                        (at.x + half, at.y + half),
                    );
                    if e.is_valid() {
                        app.run(Command::AddSupportEdit(e));
                    }
                }
            }
        }
    }

    // start of a left drag: a model tool, or the camera
    if app.mode == Mode::Prepare
        && support_tool.is_none()
        && resp.drag_started_by(PointerButton::Primary)
    {
        app.drag = None;
        let p = resp.interact_pointer_pos();
        // a handle of the active tool comes first
        if let Some(p) = p
            && let Some(index) = app.primary()
            && let Some(g) = selection_gizmo(app, &frame0)
            && let Some(handle) = g.hit(&frame0, app.tool, p)
            && let Some(start) = app.session.document().objects.get(index).map(|o| o.transform)
        {
            let grab = match handle {
                Handle::MovePlane => frame0
                    .bed_point(f64::from(p.x), f64::from(p.y))
                    .map_or(Vec3::ZERO, |at| start.translate - at),
                Handle::Ring(_) => {
                    let c =
                        frame0.project(g.center).map_or(p, |c| Pos2::new(c.x as f32, c.y as f32));
                    Vec3::new(
                        gizmo::screen_angle(c, p),
                        g.ring_angle_at(&frame0, ring_axis(handle), p),
                        0.0,
                    )
                }
                Handle::ScaleAxis(k) => {
                    let size = g.bounds.size();
                    let half = match k {
                        0 => size.x,
                        1 => size.y,
                        _ => size.z,
                    } / 2.0;
                    Vec3::new(half, 0.0, 0.0)
                }
                _ => Vec3::ZERO,
            };
            app.drag = Some(Drag::Gizmo(GizmoDrag {
                index,
                handle,
                start,
                from: p,
                center: g.center,
                grab,
            }));
            if matches!(handle, Handle::MoveAxis(_) | Handle::MovePlane) {
                app.selected = app.with_group(index);
            }
        } else {
            let hit = p.and_then(|p| pick(app, &frame0, p));
            let tool = if shift && app.tool == Tool::Select { Tool::Move } else { app.tool };
            // dragging a model with the move tool (or shift) slides it on the bed
            if tool == Tool::Move
                && let (Some(index), Some(p)) = (hit, p)
                && let Some(t) = app.session.document().objects.get(index).map(|o| o.transform)
                && let Some(at) = frame0.bed_point(f64::from(p.x), f64::from(p.y))
            {
                app.drag = Some(Drag::Move { index, grab: t.translate - at });
                app.selected = app.with_group(index);
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
                    if let Some(at) = frame0.bed_point(f64::from(p.x), f64::from(p.y)) {
                        app.move_with_group(index, Vec3::new(at.x + grab.x, at.y + grab.y, 0.0));
                    }
                }
                Drag::SupportBox { .. } => {}
                Drag::Gizmo(g) => {
                    let gz = Gizmo::new(&frame0, g_bounds(app, g.index));
                    if let Some(gz) = gz
                        && let Some(t) = gizmo::apply(&frame0, &g, &gz, p, app.uniform_scale, shift)
                    {
                        match g.handle {
                            Handle::MoveAxis(_) | Handle::MovePlane => {
                                app.move_with_group(g.index, t.translate)
                            }
                            _ => app.place(g.index, t),
                        }
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
            (Some(i), add) => app.click_select(i, add),
            (None, false) => app.selected.clear(),
            (None, true) => {}
        }
    }
    if resp.double_clicked() && app.mode == Mode::Prepare {
        app.fit_view();
    }

    // draw: on the graphics card when it can, else on the processor
    let painter = ui.painter_at(rect);
    let on_gpu = draw_on_gpu(app, ui, &frame, rect);
    let bed = app.session.document().bed.clone();
    let mut b = Builder::default();
    if !on_gpu {
        crate::theme::paint_backdrop(&painter, rect, app.prefs.dark);
        scene::bed(
            &mut b,
            &frame,
            (bed.width, bed.depth, bed.height),
            app.prefs.bed,
            app.prefs.dark,
        );
    }
    if !on_gpu && app.session.document().settings.bed_shape == slicecraft_doc::BedShape::Circle {
        let r = bed.width / 2.0;
        let n = 96;
        for k in 0..n {
            let a0 = k as f64 / n as f64 * std::f64::consts::TAU;
            let a1 = (k + 1) as f64 / n as f64 * std::f64::consts::TAU;
            b.world_line(
                &frame,
                Vec3::new(r + r * a0.cos(), r + r * a0.sin(), 0.0),
                Vec3::new(r + r * a1.cos(), r + r * a1.sin(), 0.0),
                2.0,
                Color32::from_rgb(0x6a, 0xb0, 0xe0),
            );
        }
    }
    let mut head = None;
    let mut dial_label: Option<(Pos2, f64)> = None;
    match app.mode {
        Mode::Preview => {
            if let Some((_, layers)) = &app.preview_cache
                && on_gpu
            {
                let p = &app.preview;
                let cursor = if p.by_line { Some(p.line_cursor) } else { None };
                if let Some((h, retractions)) = scene::nozzle_at(layers, p.last_layer, cursor) {
                    head = Some(h);
                    if p.show_retractions {
                        for r in retractions {
                            if let Some(q) = frame.project(*r) {
                                b.dot(Pos2::new(q.x as f32, q.y as f32), 3.5, scene::RETRACTION);
                            }
                        }
                    }
                    if p.show_toolhead {
                        scene::toolhead(&mut b, &frame, h);
                    }
                }
            } else if let Some((_, layers)) = &app.preview_cache {
                if app.preview.ghost_model
                    && let Some((_, geo)) = &app.geometry
                {
                    let none = Default::default();
                    scene::models(
                        &mut b,
                        &frame,
                        geo,
                        ModelStyle {
                            selected: &none,
                            support: &[],
                            alpha: 0.15,
                            cut_z: None,
                            wireframe: false,
                        },
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
                    flow_range: scene::flow_range(layers),
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
                if on_gpu {
                    if app.prefs.wireframe {
                        let style = ModelStyle {
                            selected: &app.selected,
                            support: &[],
                            alpha: 1.0,
                            cut_z: app.section,
                            wireframe: true,
                        };
                        scene::models(&mut b, &frame, geo, style);
                    } else if let Some(cut) = app.section {
                        scene::section_outline(&mut b, &frame, geo, cut);
                    }
                } else {
                    let style = ModelStyle {
                        selected: &app.selected,
                        support,
                        alpha: 1.0,
                        cut_z: app.section,
                        wireframe: app.prefs.wireframe,
                    };
                    scene::models(&mut b, &frame, geo, style);
                }
            }
            let doc = app.session.document();
            let top = doc.bounds();
            let height = if top.is_empty() { 20.0 } else { top.max().z.max(1.0) };
            scene::support_edits(&mut b, &frame, &doc.support_edits, height);
            if let Some(Drag::SupportBox { kind, from, to }) = app.drag {
                let e = SupportEdit::columns(kind, from, to);
                scene::support_edits(&mut b, &frame, &[e], height);
            }
            if let Some(g) = selection_gizmo(app, &frame) {
                let dragging = match app.drag {
                    Some(Drag::Gizmo(d)) => Some(d),
                    _ => None,
                };
                let hot = dragging
                    .map(|d| d.handle)
                    .or_else(|| resp.hover_pos().and_then(|p| g.hit(&frame, app.tool, p)));
                if hot.is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                g.draw(&mut b, &frame, app.tool, hot);
                if let Some(d) = dragging
                    && let Handle::Ring(k) = d.handle
                    && let Some(p) = resp.interact_pointer_pos()
                {
                    let turned = gizmo::ring_turned(&frame, &d, p, shift);
                    g.draw_dial(&mut b, &frame, k, d.grab.y, turned);
                    dial_label = Some((p, turned.to_degrees()));
                }
            }
        }
    }
    if !b.is_empty() {
        painter.add(b.into_shape());
    }
    if let Some((p, deg)) = dial_label {
        let text = format!("{deg:+.0}°");
        painter.text(
            p + egui::vec2(16.0, -16.0),
            Align2::LEFT_BOTTOM,
            text,
            theme::strong(16.0),
            Color32::WHITE,
        );
    }
    overlays(app, ui, rect, head);
}

/// A badge of text in a corner of the view.
fn badge(ui: &mut Ui, at: Pos2, align: Align2, text: &str) {
    let p = theme::of(ui);
    let font = FontId::proportional(13.0);
    let galley = ui.ctx().fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, p.text));
    let size = galley.size() + egui::vec2(16.0, 8.0);
    let r = align.anchor_size(at, size);
    let painter = ui.painter();
    painter.rect_filled(r, 8, p.card.gamma_multiply(0.92));
    painter.rect_stroke(r, 8, egui::Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    painter.galley(r.min + egui::vec2(8.0, 4.0), galley, p.text);
}

/// The floating bar at the top of the 3D view: framing and display toggles.
/// The camera directions are on the view cube.
fn view_bar(app: &mut SliceCraftApp, ui: &mut Ui, rect: Rect) {
    let prepare = app.mode == Mode::Prepare;
    let buttons = if prepare { 5.0 } else { 4.0 };
    let width = buttons * 30.0 + 9.0 + 8.0;
    let at = egui::pos2(rect.center().x - width / 2.0, rect.top() + 10.0);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(at, egui::vec2(width + 20.0, 44.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            theme::overlay_frame(ui).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if icons::sized(ui, Icon::ViewIso, tr("Perspective view (0)"), true, false, 28.0)
                    .clicked()
                {
                    app.view(ViewPreset::Iso);
                }
                if icons::sized(ui, Icon::Fit, tr("Fit models (F)"), true, false, 28.0).clicked() {
                    app.fit_view();
                }
                theme::divider(ui);
                if icons::sized(ui, Icon::Grid, tr("Bed grid"), true, app.prefs.bed.grid, 28.0)
                    .clicked()
                {
                    app.prefs.bed.grid = !app.prefs.bed.grid;
                }
                if prepare {
                    let cut = app.section.is_some();
                    let tip = tr("Section view: cut the models at a height to see inside");
                    if icons::sized(ui, Icon::Section, tip, true, cut, 28.0).clicked() {
                        app.set_section(!cut);
                    }
                }
                let wire = app.prefs.wireframe;
                if icons::sized(
                    ui,
                    Icon::Wireframe,
                    tr("Draw models as wireframes"),
                    true,
                    wire,
                    28.0,
                )
                .clicked()
                {
                    app.prefs.wireframe = !wire;
                }
            });
        },
    );
}

/// The view cube in the bottom-right corner.
fn cube_corner(app: &mut SliceCraftApp, ui: &mut Ui, rect: Rect) {
    let vp = crate::camera::Viewport {
        left: f64::from(rect.left()),
        top: f64::from(rect.top()),
        width: f64::from(rect.width()),
        height: f64::from(rect.height()),
    };
    let f = app.camera.frame(vp);
    let area = Rect::from_min_size(
        rect.right_bottom() - egui::vec2(CUBE_SIDE + 8.0, CUBE_SIDE + 30.0),
        egui::vec2(CUBE_SIDE, CUBE_SIDE),
    );
    if let Some(preset) = gizmo::view_cube(ui, &f, area) {
        app.view(preset);
    }
}

/// Side of the view cube's square (px).
const CUBE_SIDE: f32 = 104.0;

/// The floating bar of placement tools at the left of the 3D view.
fn tool_bar(app: &mut SliceCraftApp, ui: &mut Ui, rect: Rect) {
    let height = 4.0 * 34.0 + 10.0;
    let at = egui::pos2(rect.left() + 10.0, rect.center().y - height / 2.0);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(at, egui::vec2(48.0, height + 10.0)))
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            theme::overlay_frame(ui).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (tool, icon, tip) in [
                    (Tool::Select, Icon::Select, "Select (Q)"),
                    (Tool::Move, Icon::Move, "Move (W)"),
                    (Tool::Rotate, Icon::Rotate, "Rotate (E)"),
                    (Tool::Scale, Icon::Scale, "Scale (R)"),
                ] {
                    if icons::button(ui, icon, tr(tip), true, app.tool == tool).clicked() {
                        app.tool = tool;
                    }
                }
            });
        },
    );
}

/// What an empty plate shows: where models come from.
fn empty_plate(app: &mut SliceCraftApp, ui: &mut Ui, rect: Rect) {
    let size = egui::vec2(360.0, 150.0);
    let r = Rect::from_center_size(rect.center(), size);
    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
        theme::overlay_frame(ui).inner_margin(egui::Margin::same(18)).show(ui, |ui| {
            ui.set_width(size.x - 36.0);
            ui.vertical_centered(|ui| {
                ui.label(theme::title_text(ui, "Nothing on the plate yet"));
                ui.add_space(2.0);
                theme::dim(ui, tr("Drop STL, OBJ or 3MF files here, or import them."));
                ui.add_space(10.0);
                if theme::primary(ui, tr("Import models…"), true, egui::vec2(200.0, 34.0)).clicked()
                {
                    app.open_import_dialog();
                }
            });
        });
    });
}

fn overlays(app: &mut SliceCraftApp, ui: &mut Ui, rect: Rect, head: Option<Vec3>) {
    let p = theme::of(ui);
    let painter = ui.painter_at(rect);
    let faint = p.dim;
    view_bar(app, ui, rect);
    cube_corner(app, ui, rect);
    if app.mode == Mode::Prepare {
        tool_bar(app, ui, rect);
        let top = rect.center().y - (4.0 * 34.0 + 10.0) / 2.0;
        gizmo::tool_panel(app, ui, egui::pos2(rect.left() + 66.0, top));
        if app.session.document().objects.is_empty() {
            empty_plate(app, ui, rect);
        }
    }

    let hint = match (app.mode, app.tool) {
        (Mode::Prepare, Tool::Select) => {
            "drag: orbit · right-drag: pan · wheel: zoom · click: select · shift-drag: move"
        }
        (Mode::Prepare, Tool::Move) => {
            "drag an arrow to move along it, the square or the model to move freely · drag empty space to orbit"
        }
        (Mode::Prepare, Tool::Rotate) => {
            "drag a ring to turn the model about its axis · Shift: 15° steps · drag empty space to orbit"
        }
        (Mode::Prepare, Tool::Scale) => {
            "drag a coloured square to stretch one way, the white corner to scale evenly · drag empty space to orbit"
        }
        (Mode::Prepare, Tool::SupportAdd) => {
            "click: one pillar here · drag a box: every surface facing down inside it gets support · Q: back to select"
        }
        (Mode::Prepare, Tool::SupportBlock) => {
            "click a pillar to remove it · drag a box: no support inside it · Q: back to select"
        }
        (Mode::Preview, _) => {
            "drag: orbit · right-drag: pan · wheel: zoom · Up/Down: layer · Space: play"
        }
    };
    painter.text(
        rect.left_bottom() + egui::vec2(10.0, -8.0),
        Align2::LEFT_BOTTOM,
        tr(hint),
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
            Pos2::new(rect.right() - 54.0, rect.top() + 44.0),
            Pos2::new(rect.right() - 14.0, rect.bottom() - CUBE_SIDE - 48.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
            ui.spacing_mut().slider_width = r.height();
            ui.add(egui::Slider::new(z, 0.0..=top.max(0.1)).vertical().show_value(false))
                .on_hover_text(tr("Drag to move the cut up or down"));
        });
        let label = format!("{} {:.2} mm", tr("Section at Z"), *z);
        badge(ui, Pos2::new(rect.right() - 12.0, rect.top() + 12.0), Align2::RIGHT_TOP, &label);
    }

    if app.mode != Mode::Preview {
        return;
    }
    let Some((_, layers)) = &app.preview_cache else { return };
    let max = layers.len().saturating_sub(1);
    let z = layers.get(app.preview.last_layer).map_or(0.0, |l| l.z);
    let label =
        format!("{} {} / {}   ·   Z {:.2} mm", tr("Layer"), app.preview.last_layer + 1, max + 1, z);
    badge(ui, Pos2::new(rect.right() - 12.0, rect.top() + 12.0), Align2::RIGHT_TOP, &label);
    let r = Rect::from_min_max(
        Pos2::new(rect.right() - 54.0, rect.top() + 44.0),
        Pos2::new(rect.right() - 14.0, rect.bottom() - CUBE_SIDE - 48.0),
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
            format!("{}  X {:.3}  Y {:.3}  Z {:.3}", tr("nozzle"), h.x, h.y, h.z),
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
            .max_rect(Rect::from_min_size(at, egui::vec2(210.0, 420.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            theme::overlay_frame(ui).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.label(theme::title_text(ui, app.preview.coloring.label()));
                for (color, label, switch) in &legend {
                    ui.horizontal(|ui| {
                        let (sw, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                        ui.painter().rect_filled(sw, 2.0, *color);
                        match (by_feature, switch.and_then(|k| app.preview.filter.0.get_mut(k))) {
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
