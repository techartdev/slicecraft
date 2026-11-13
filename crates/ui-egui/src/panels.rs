//! The side panels, the preview controls and the status bar.

use crate::app::{Mode, STAGES, SliceCraftApp};
use crate::scene::{self, Coloring, SegKind};
use eframe::egui::{self, Color32, RichText, Sense, Ui};
use slicecraft_doc::profile::presets;
use slicecraft_doc::{Axis, Transform};
use slicecraft_engine::{Command, Stage};
use slicecraft_geom::Vec3;
use slicecraft_io::format_duration;
use slicecraft_render::PathKind;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

const WARN: Color32 = Color32::from_rgb(0xff, 0x7a, 0x70);

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).heading().size(17.0));
    ui.add_space(2.0);
}

/// Left panel in prepare mode: models, the profile and the slice button.
pub fn models_panel(app: &mut SliceCraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    heading(ui, "Models");
    let doc = app.session.document();
    let outside = doc.outside_bed();
    let rows: Vec<(String, bool, bool)> = doc
        .objects
        .iter()
        .map(|o| (o.name.clone(), o.visible, outside.contains(&o.name)))
        .collect();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_height(140.0);
        ui.set_width(ui.available_width());
        if rows.is_empty() {
            ui.weak("No models. Import or drop STL/OBJ files.");
        }
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            for (i, (name, visible, off)) in rows.iter().enumerate() {
                ui.horizontal(|ui| {
                    let mut v = *visible;
                    if ui.checkbox(&mut v, "").on_hover_text("Include when slicing").changed() {
                        app.run(Command::SetVisible { index: i, visible: v });
                    }
                    let mut text = RichText::new(name);
                    if *off {
                        text = text.color(WARN);
                    }
                    if !visible {
                        text = text.weak();
                    }
                    let resp = ui.selectable_label(app.selected.contains(&i), text);
                    if resp.clicked() {
                        let multi = ui.input(|inp| inp.modifiers.command || inp.modifiers.shift);
                        if multi {
                            if !app.selected.remove(&i) {
                                app.selected.insert(i);
                            }
                        } else {
                            app.selected = std::iter::once(i).collect();
                        }
                    }
                    resp.context_menu(|ui| {
                        app.selected = std::iter::once(i).collect();
                        if ui.button("Duplicate").clicked() {
                            app.duplicate_selected();
                        }
                        for (axis, label) in
                            [(Axis::X, "Mirror X"), (Axis::Y, "Mirror Y"), (Axis::Z, "Mirror Z")]
                        {
                            if ui.button(label).clicked() {
                                app.mirror_selected(axis);
                            }
                        }
                        if ui.button("Centre on the bed").clicked() {
                            app.center_selected();
                        }
                        if ui.button("Reset rotation and scale").clicked() {
                            app.reset_selected();
                        }
                        ui.separator();
                        if ui.button("Remove").clicked() {
                            app.remove_selected();
                        }
                    });
                });
            }
        });
    });
    ui.horizontal(|ui| {
        if ui.button("Import…").clicked() {
            app.open_import_dialog();
        }
        if ui.add_enabled(!app.selected.is_empty(), egui::Button::new("Remove")).clicked() {
            app.remove_selected();
        }
        if ui.add_enabled(!rows.is_empty(), egui::Button::new("Arrange")).clicked() {
            app.run(Command::Arrange);
        }
    });

    ui.add_space(8.0);
    heading(ui, "Print profile");
    let current = app.session.profile_name().to_string();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut chosen = None;
        egui::ComboBox::from_id_salt("preset")
            .selected_text(&current)
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for p in presets() {
                    if ui.selectable_label(p.name == current, &p.name).clicked() {
                        chosen = Some(p);
                    }
                }
            });
        if let Some(p) = chosen {
            app.apply_profile(p.settings, Some(p.name));
        }
        quick_settings(app, ui);
        ui.horizontal(|ui| {
            if ui.button("All settings…").on_hover_text("Ctrl+P").clicked() {
                app.open_settings();
            }
            if ui.button("Load…").clicked() {
                app.load_profile_dialog();
            }
            if ui.button("Save…").clicked() {
                app.save_profile_dialog();
            }
        });
        ui.add_enabled_ui(false, |ui| {
            ui.horizontal(|ui| {
                let _ = ui.button("Add profile").on_disabled_hover_text(
                    "planned: several profiles in one job, each for some models or heights",
                );
                let _ = ui
                    .button("Setting zones…")
                    .on_disabled_hover_text("planned: change settings above a height");
            });
        });
    });

    support_section(app, ui);

    ui.add_space(8.0);
    let has_models = !rows.is_empty();
    ui.vertical_centered_justified(|ui| {
        if app.job.is_some() {
            if ui.add(egui::Button::new("Cancel slicing").min_size(egui::vec2(0.0, 34.0))).clicked()
            {
                app.cancel_slice();
            }
        } else {
            let b = egui::Button::new(RichText::new("Slice and preview").strong().size(16.0))
                .min_size(egui::vec2(0.0, 36.0));
            if ui.add_enabled(has_models, b).on_hover_text("Ctrl+R").clicked() {
                app.start_slice(&ctx);
            }
        }
    });
}

/// Support preview and the boxes that add or block support.
fn support_section(app: &mut SliceCraftApp, ui: &mut Ui) {
    use crate::app::Tool;
    use slicecraft_doc::SupportEditKind;
    ui.add_space(6.0);
    ui.heading("Support");
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut show = app.show_support;
        if ui.checkbox(&mut show, "Show before slicing").changed() {
            app.set_show_support(show);
        }
        if app.show_support {
            let busy = app.support_job.is_some();
            match (&app.support_note, busy) {
                (_, true) => ui.weak("Working out support…"),
                (Some(n), false) => ui.weak(n.as_str()),
                (None, false) => ui.weak(""),
            };
        }
        ui.horizontal(|ui| {
            let add = app.tool == Tool::SupportAdd;
            if ui
                .selectable_label(add, "+ Add area")
                .on_hover_text(
                    "Drag a box on the bed: every surface facing down inside it gets support",
                )
                .clicked()
            {
                app.tool = if add { Tool::Select } else { Tool::SupportAdd };
                app.set_show_support(true);
            }
            let block = app.tool == Tool::SupportBlock;
            if ui
                .selectable_label(block, "− Block area")
                .on_hover_text("Drag a box on the bed: no support inside it")
                .clicked()
            {
                app.tool = if block { Tool::Select } else { Tool::SupportBlock };
                app.set_show_support(true);
            }
        });
        let edits = app.session.document().support_edits.clone();
        let mut remove = None;
        for (i, e) in edits.iter().enumerate() {
            ui.horizontal(|ui| {
                let (label, colour) = match e.kind {
                    SupportEditKind::Add => ("add", egui::Color32::from_rgb(0x4c, 0xc0, 0x6a)),
                    SupportEditKind::Block => ("block", egui::Color32::from_rgb(0xe0, 0x50, 0x50)),
                };
                ui.colored_label(colour, label);
                ui.weak(format!(
                    "{:.0}×{:.0} mm at {:.0}, {:.0}",
                    e.x1 - e.x0,
                    e.y1 - e.y0,
                    e.x0,
                    e.y0
                ));
                if ui.small_button("×").on_hover_text("Remove this box").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(index) = remove {
            app.run(Command::RemoveSupportEdit { index });
        }
        if !edits.is_empty() && ui.small_button("Clear all boxes").clicked() {
            app.run(Command::ClearSupportEdits);
        }
    });
}

/// The few settings most people change on every job.
fn quick_settings(app: &mut SliceCraftApp, ui: &mut Ui) {
    let mut s = app.session.document().settings.clone();
    let before = s.clone();
    egui::Grid::new("quick").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Layer height");
        ui.add(
            egui::DragValue::new(&mut s.layer_height).speed(0.01).range(0.05..=1.0).suffix(" mm"),
        );
        ui.end_row();
        ui.label("Infill");
        ui.add(egui::Slider::new(&mut s.infill_percentage, 0.0..=100.0).suffix(" %"));
        ui.end_row();
        ui.label("Perimeters");
        ui.add(egui::DragValue::new(&mut s.perimeter_count).range(0..=20));
        ui.end_row();
        ui.label("Nozzle / bed");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut s.nozzle_temperature).range(0.0..=350.0).suffix("°"));
            ui.add(egui::DragValue::new(&mut s.bed_temperature).range(0.0..=130.0).suffix("°"));
        });
        ui.end_row();
        ui.label("Adhesion");
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.skirt_enabled, "skirt");
            ui.checkbox(&mut s.brim_enabled, "brim");
        });
        ui.end_row();
        ui.label("Support");
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.support_enabled, "generate");
            ui.add_enabled_ui(s.support_enabled, |ui| {
                let mut bed_only = s.support_placement == slicecraft_doc::SupportPlacement::BedOnly;
                if ui.checkbox(&mut bed_only, "from bed only").changed() {
                    s.support_placement = if bed_only {
                        slicecraft_doc::SupportPlacement::BedOnly
                    } else {
                        slicecraft_doc::SupportPlacement::Everywhere
                    };
                }
            });
        });
        ui.end_row();
    });
    // keep first-layer temperatures in step with the quick fields
    if s.nozzle_temperature != before.nozzle_temperature
        && before.first_layer_nozzle_temperature == before.nozzle_temperature
    {
        s.first_layer_nozzle_temperature = s.nozzle_temperature;
    }
    if s.bed_temperature != before.bed_temperature
        && before.first_layer_bed_temperature == before.bed_temperature
    {
        s.first_layer_bed_temperature = s.bed_temperature;
    }
    if s != before {
        app.apply_profile(s, None);
    }
}

/// Right panel in prepare mode: exact placement of the selection.
pub fn transform_panel(app: &mut SliceCraftApp, ui: &mut Ui) {
    heading(ui, "Model placement");
    let count = app.selected.len();
    if count == 0 {
        ui.weak("Select a model in the list or the 3D view.");
        ui.add_space(6.0);
        ui.weak("Q select · W move · E rotate · R scale");
        return;
    }
    if count > 1 {
        ui.label(format!("{count} models selected"));
        multi_actions(app, ui);
        return;
    }
    let Some(i) = app.primary() else { return };
    let Some(o) = app.session.document().objects.get(i) else { return };
    let name = o.name.clone();
    let tris = o.mesh.len();
    let original = o.original_size();
    let placed = o.size();
    let mut t = o.transform;
    let before = t;
    let bed = app.session.document().bed.clone();
    let off_bed = app.session.document().outside_bed().contains(&name);

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(&name).strong());
        ui.weak(format!(
            "{tris} triangles · {:.1} × {:.1} × {:.1} mm on the bed",
            placed.x, placed.y, placed.z
        ));
        if off_bed {
            ui.colored_label(WARN, "Outside the build volume");
        }
    });

    ui.add_space(4.0);
    ui.label(RichText::new("Position").strong());
    egui::Grid::new("pos").num_columns(3).show(ui, |ui| {
        ui.label("X");
        ui.add(
            egui::DragValue::new(&mut t.translate.x)
                .speed(0.5)
                .range(-bed.width..=2.0 * bed.width)
                .suffix(" mm"),
        );
        ui.end_row();
        ui.label("Y");
        ui.add(
            egui::DragValue::new(&mut t.translate.y)
                .speed(0.5)
                .range(-bed.depth..=2.0 * bed.depth)
                .suffix(" mm"),
        );
        ui.end_row();
        ui.label("Reference");
        ui.weak("model centre, on the bed");
        ui.end_row();
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Size").strong());
        ui.checkbox(&mut app.uniform_scale, "uniform");
    });
    let mut size =
        Vec3::new(original.x * t.scale.x, original.y * t.scale.y, original.z * t.scale.z);
    let mut pct = t.scale.scale(100.0);
    let mut changed_axis: Option<(usize, f64)> = None;
    egui::Grid::new("scale").num_columns(3).show(ui, |ui| {
        ui.label("");
        ui.weak("mm");
        ui.weak("%");
        ui.end_row();
        for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
            ui.label(*axis);
            let (sz, p, orig) = match k {
                0 => (&mut size.x, &mut pct.x, original.x),
                1 => (&mut size.y, &mut pct.y, original.y),
                _ => (&mut size.z, &mut pct.z, original.z),
            };
            if ui.add(egui::DragValue::new(sz).speed(0.5).range(0.01..=10_000.0)).changed()
                && orig > 0.0
            {
                changed_axis = Some((k, *sz / orig));
            }
            if ui.add(egui::DragValue::new(p).speed(1.0).range(0.1..=100_000.0)).changed() {
                changed_axis = Some((k, *p / 100.0));
            }
            ui.end_row();
        }
    });
    if let Some((k, factor)) = changed_axis
        && factor.is_finite()
        && factor > 0.0
    {
        if app.uniform_scale {
            let old = match k {
                0 => t.scale.x,
                1 => t.scale.y,
                _ => t.scale.z,
            };
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

    ui.add_space(4.0);
    ui.label(RichText::new("Rotation").strong());
    egui::Grid::new("rot").num_columns(3).show(ui, |ui| {
        for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
            ui.label(*axis);
            let r = match k {
                0 => &mut t.rotate.x,
                1 => &mut t.rotate.y,
                _ => &mut t.rotate.z,
            };
            ui.add(egui::DragValue::new(r).speed(1.0).range(-360.0..=360.0).suffix("°"));
            ui.horizontal(|ui| {
                if ui.small_button("−90").clicked() {
                    *r = (*r - 90.0) % 360.0;
                }
                if ui.small_button("+90").clicked() {
                    *r = (*r + 90.0) % 360.0;
                }
            });
            ui.end_row();
        }
    });
    if t != before && t.is_valid() {
        app.place(i, t);
    }

    ui.add_space(6.0);
    multi_actions(app, ui);

    if let Some((_, geo)) = &app.geometry
        && let Some(g) = geo.objects.get(i)
    {
        ui.add_space(6.0);
        ui.label(RichText::new("Mesh check").strong());
        let r = g.report;
        if r.is_watertight() {
            ui.label(format!("Closed and manifold · {:.1} cm³", r.volume / 1000.0));
        } else {
            ui.colored_label(
                WARN,
                format!("{} open edges, {} non-manifold edges", r.open_edges, r.non_manifold_edges),
            );
            ui.weak("Slicing closes small gaps; repair tools are planned.");
        }
    }
}

fn multi_actions(app: &mut SliceCraftApp, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Centre").clicked() {
            app.center_selected();
        }
        if ui.button("Duplicate").clicked() {
            app.duplicate_selected();
        }
        if ui.button("Reset").on_hover_text("Reset rotation and scale").clicked() {
            app.reset_selected();
        }
        if ui.button("Remove").clicked() {
            app.remove_selected();
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Mirror");
        for (axis, l) in [(Axis::X, "X"), (Axis::Y, "Y"), (Axis::Z, "Z")] {
            if ui.small_button(l).clicked() {
                app.mirror_selected(axis);
            }
        }
        ui.add_enabled_ui(false, |ui| {
            let _ = ui
                .small_button("Lay flat")
                .on_disabled_hover_text("planned: click a face to put it on the bed");
        });
    });
    let _ = Transform::default();
}

/// Left panel in preview mode: what the job costs, and what to show.
pub fn summary_panel(app: &mut SliceCraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    heading(ui, "Build summary");
    let Some(out) = app.session.slice_result() else {
        ui.weak("Slice to see the summary.");
        return;
    };
    let s = &app.session.document().settings;
    let g = out.stats.gcode;
    let area = std::f64::consts::PI * s.filament_diameter.powi(2) / 4.0;
    // extent of the toolpaths
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut by_kind: BTreeMap<&'static str, f64> = BTreeMap::new();
    if let Some((_, layers)) = &app.preview_cache {
        for seg in layers.iter().flat_map(|l| l.segs.iter()) {
            if let SegKind::Path(k) = seg.kind {
                for p in [seg.a, seg.b] {
                    lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                    hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
                }
                *by_kind.entry(k.label()).or_insert(0.0) += (seg.b - seg.a).length();
            }
        }
    }
    let size = if lo.x.is_finite() { hi - lo } else { Vec3::ZERO };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("summary").num_columns(2).striped(true).show(ui, |ui| {
            let mut row = |k: &str, v: String| {
                ui.label(k);
                ui.label(v);
                ui.end_row();
            };
            row("Print time", format_duration(g.estimated_seconds));
            row("Filament", format!("{:.2} m", g.filament_mm / 1000.0));
            row("Volume", format!("{:.2} cm³", g.filament_mm * area / 1000.0));
            row("Weight", format!("{:.1} g", g.filament_grams));
            row("Cost", format!("{:.2}", g.filament_cost));
            row("Size X", format!("{:.2} mm", size.x));
            row("Size Y", format!("{:.2} mm", size.y));
            row("Size Z", format!("{:.2} mm", hi.z.max(0.0)));
            row("Layers", out.layers.len().to_string());
            row(
                "Models",
                app.session.document().objects.iter().filter(|o| o.visible).count().to_string(),
            );
            row("Retractions", g.retractions.to_string());
            row("Travel", format!("{:.1} m", g.travel_mm / 1000.0));
        });
        ui.weak("Time ignores acceleration; weight uses the profile's density.");
    });
    egui::CollapsingHeader::new("Extruded length by feature").default_open(false).show(ui, |ui| {
        egui::Grid::new("byfeature").num_columns(2).show(ui, |ui| {
            for (k, v) in &by_kind {
                ui.label(*k);
                ui.label(format!("{:.2} m", v / 1000.0));
                ui.end_row();
            }
        });
    });
    if !out.stats.warnings.is_empty() {
        egui::CollapsingHeader::new(RichText::new("Notes").color(WARN)).default_open(true).show(
            ui,
            |ui| {
                for w in &out.stats.warnings {
                    ui.label(w);
                }
            },
        );
    }

    ui.add_space(6.0);
    heading(ui, "Show");
    let p = &mut app.preview;
    ui.checkbox(&mut p.show_travel, "Travel moves");
    ui.checkbox(&mut p.show_retractions, "Retractions");
    ui.checkbox(&mut p.show_toolhead, "Nozzle");
    ui.checkbox(&mut p.ghost_model, "Model outline");
    ui.horizontal(|ui| {
        ui.label("Colour by");
        egui::ComboBox::from_id_salt("coloring").selected_text(p.coloring.label()).show_ui(
            ui,
            |ui| {
                for c in Coloring::ALL {
                    ui.selectable_value(&mut p.coloring, c, c.label());
                }
                ui.add_enabled_ui(false, |ui| {
                    let _ = ui.selectable_label(false, "Tool (planned)");
                    let _ = ui.selectable_label(false, "Flow rate (planned)");
                    let _ = ui.selectable_label(false, "Profile (planned)");
                });
            },
        );
    });

    ui.add_space(10.0);
    ui.vertical_centered_justified(|ui| {
        if ui
            .add(
                egui::Button::new(RichText::new("Export G-code…").strong())
                    .min_size(egui::vec2(0.0, 32.0)),
            )
            .clicked()
        {
            app.export(&ctx);
        }
        ui.add_enabled_ui(false, |ui| {
            let _ = ui
                .add(egui::Button::new("Print on a printer…").min_size(egui::vec2(0.0, 28.0)))
                .on_disabled_hover_text("planned: USB and network printing");
        });
        if ui.button("Back to models").on_hover_text("Esc").clicked() {
            app.mode = Mode::Prepare;
        }
    });
}

/// Bottom bar in preview mode: playback and the layer range.
pub fn preview_controls(app: &mut SliceCraftApp, ui: &mut Ui) {
    let Some((_, layers)) = &app.preview_cache else { return };
    let max = layers.len().saturating_sub(1);
    let moves = layers.get(app.preview.last_layer).map_or(0, |l| l.segs.len());
    let p = &mut app.preview;
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        if ui.button(if p.playing { "⏸ Pause" } else { "▶ Play" }).on_hover_text("Space").clicked()
        {
            p.playing = !p.playing;
            if p.playing && p.last_layer >= max && !p.by_line {
                p.last_layer = p.first_layer;
            }
        }
        if ui.button("⏮").on_hover_text("Previous layer").clicked() {
            p.last_layer = p.last_layer.saturating_sub(1);
            p.line_cursor = 0;
        }
        if ui.button("⏭").on_hover_text("Next layer").clicked() {
            p.last_layer = (p.last_layer + 1).min(max);
            p.line_cursor = 0;
        }
        ui.add(egui::Slider::new(&mut p.speed, 1.0..=200.0).logarithmic(true).text("speed"));
        ui.separator();
        ui.label("Step by");
        ui.selectable_value(&mut p.by_line, false, "layer");
        if ui.selectable_value(&mut p.by_line, true, "move").clicked() {
            p.line_cursor = 0;
        }
        ui.separator();
        let mut only = p.only_n.is_some();
        if ui.checkbox(&mut only, "Only the last").changed() {
            p.only_n = if only { Some(1) } else { None };
        }
        if let Some(n) = &mut p.only_n {
            ui.add(egui::DragValue::new(n).range(1..=10_000));
            ui.label("layers");
        }
    });
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 160.0).max(120.0);
        ui.spacing_mut().slider_width = w;
        ui.label("From ");
        ui.add(
            egui::Slider::new(&mut p.first_layer, 0..=max)
                .custom_formatter(|v, _| format!("{}", v as usize + 1)),
        );
    });
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 160.0).max(120.0);
        ui.spacing_mut().slider_width = w;
        ui.label("To     ");
        if ui
            .add(
                egui::Slider::new(&mut p.last_layer, 0..=max)
                    .custom_formatter(|v, _| format!("{}", v as usize + 1)),
            )
            .changed()
        {
            p.line_cursor = 0;
        }
    });
    if p.by_line {
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 160.0).max(120.0);
            ui.spacing_mut().slider_width = w;
            ui.label("Move  ");
            ui.add(egui::Slider::new(&mut p.line_cursor, 0..=moves));
        });
    }
    if p.first_layer > p.last_layer {
        p.first_layer = p.last_layer;
    }
    ui.add_space(2.0);
}

pub fn status_bar(app: &mut SliceCraftApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        if let Some(job) = &app.job {
            let stage = STAGES
                .get(job.progress.stage.load(Ordering::Relaxed))
                .copied()
                .unwrap_or(Stage::Sections);
            let done = job.progress.done.load(Ordering::Relaxed);
            let total = job.progress.total.load(Ordering::Relaxed).max(1);
            let stage_i = STAGES.iter().position(|s| *s == stage).unwrap_or(0);
            let frac = (stage_i as f32 + done as f32 / total as f32) / STAGES.len() as f32;
            ui.add(
                egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                    .desired_width(220.0)
                    .text(stage.label()),
            );
        }
        let color = if app.status_is_error { WARN } else { ui.visuals().text_color() };
        let status = app.status.clone();
        ui.add(
            egui::Label::new(RichText::new(status).color(color)).truncate().sense(Sense::hover()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let doc = app.session.document();
            ui.weak(format!(
                "{} · bed {:.0}×{:.0}×{:.0} mm · {} model(s)",
                app.session.profile_name(),
                doc.bed.width,
                doc.bed.depth,
                doc.bed.height,
                doc.objects.len()
            ));
        });
    });
}

/// Legend colours and labels for the current colouring, for the viewport overlay.
pub fn legend(app: &SliceCraftApp) -> Vec<(Color32, String)> {
    match app.preview.coloring {
        Coloring::Feature => {
            let mut v: Vec<(Color32, String)> = PathKind::ALL
                .iter()
                .map(|k| (scene::kind_color(*k), k.label().to_string()))
                .collect();
            if app.preview.show_travel {
                v.push((scene::TRAVEL, "travel".into()));
            }
            v
        }
        Coloring::Speed => {
            let (lo, hi) =
                app.preview_cache.as_ref().map_or((0.0, 1.0), |(_, l)| scene::speed_range(l));
            (0..5)
                .map(|k| {
                    let t = f64::from(k) / 4.0;
                    (scene::ramp(t), format!("{:.0} mm/s", f64::from(lo) + t * f64::from(hi - lo)))
                })
                .collect()
        }
        Coloring::Height => {
            let top = app.preview_cache.as_ref().and_then(|(_, l)| l.last()).map_or(0.0, |l| l.z);
            (0..5)
                .map(|k| {
                    let t = f64::from(k) / 4.0;
                    (scene::ramp(t), format!("{:.1} mm", t * top))
                })
                .collect()
        }
    }
}
