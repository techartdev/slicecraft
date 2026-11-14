//! The side panels, the preview controls and the status bar.
//!
//! Each panel is a column of cards (see `theme`), with its main action pinned
//! to the bottom so it never scrolls away.

use crate::app::{Mode, STAGES, SliceCraftApp, Tool};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::scene::{self, Coloring, SegKind};
use crate::theme;
use eframe::egui::{self, Color32, RichText, Sense, Ui, vec2};
use slicecraft_doc::profile::presets;
use slicecraft_doc::{Axis, ModelRole, SupportPlacement};
use slicecraft_engine::{Command, Stage};
use slicecraft_geom::Vec3;
use slicecraft_io::format_duration;
use slicecraft_render::PathKind;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

/// Width of the value column in panel grids.
const VALUE_W: f32 = 120.0;

/// Pin `bottom` to the foot of the panel and scroll `body` above it.
fn with_footer(
    ui: &mut Ui,
    id: &str,
    app: &mut SliceCraftApp,
    body: impl FnOnce(&mut SliceCraftApp, &mut Ui),
    bottom: impl FnOnce(&mut SliceCraftApp, &mut Ui),
) {
    egui::Panel::bottom(egui::Id::new(id))
        .frame(egui::Frame::new().inner_margin(egui::Margin {
            top: 10,
            bottom: 4,
            left: 0,
            right: 0,
        }))
        .show_separator_line(false)
        .resizable(false)
        .show(ui, |ui| bottom(app, ui));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(2.0);
        body(app, ui);
    });
}

/// Left panel in prepare mode: models, the profile, support and the slice button.
pub fn models_panel(app: &mut SliceCraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let has_models = !app.session.document().objects.is_empty();
    with_footer(
        ui,
        "prepare-footer",
        app,
        |app, ui| {
            models_card(app, ui);
            profile_card(app, ui);
            support_card(app, ui);
        },
        |app, ui| {
            if app.job.is_some() {
                if theme::secondary(ui, tr("Cancel slicing"), true, 40.0).clicked() {
                    app.cancel_slice();
                }
            } else if theme::primary(ui, tr("Slice and preview"), has_models, vec2(0.0, 42.0))
                .on_hover_text(tr("Ctrl+R"))
                .clicked()
            {
                app.start_slice(&ctx);
            }
        },
    );
}

/// Short coloured notes after a model's name.
type Tags = Vec<(String, Color32)>;

/// One model in the list: visibility, name, tags. Returns the row's response
/// and whether the eye was clicked.
fn model_row(
    ui: &mut Ui,
    name: &str,
    tags: &[(String, Color32)],
    visible: bool,
    selected: bool,
) -> (egui::Response, bool) {
    let p = theme::of(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    let bg = if selected {
        p.accent.gamma_multiply(0.28)
    } else if resp.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6, bg);
    if selected {
        let bar = egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height()));
        ui.painter().rect_filled(bar, 2, p.accent);
    }
    let mut eye = false;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(6.0, 2.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let icon = if visible { Icon::Eye } else { Icon::EyeOff };
            if icons::sized(ui, icon, tr("Include when slicing"), true, false, 22.0).clicked() {
                eye = true;
            }
            let colour = if visible { p.text } else { p.dim };
            ui.add(
                egui::Label::new(RichText::new(name).color(colour)).truncate().selectable(false),
            );
            for (t, c) in tags {
                theme::tag(ui, t, *c);
            }
        },
    );
    (resp, eye)
}

fn models_card(app: &mut SliceCraftApp, ui: &mut Ui) {
    let doc = app.session.document();
    let outside = doc.outside_bed();
    let p = theme::of(ui);
    let rows: Vec<(String, Tags, bool)> = doc
        .objects
        .iter()
        .map(|o| {
            let mut tags = Vec::new();
            match o.role {
                ModelRole::Print => {}
                ModelRole::Support => tags.push((tr("support").to_string(), p.dim)),
                ModelRole::Cutter => tags.push((tr("cutter").to_string(), scene::MODEL_CUTTER)),
            }
            if let Some(g) = o.group {
                tags.push((format!("{} {g}", tr("group")), p.accent_hover));
            }
            if outside.contains(&o.name) {
                tags.push((tr("off the bed").to_string(), p.warn));
            }
            (o.name.clone(), tags, o.visible)
        })
        .collect();
    let count = rows.len();
    let title =
        if count == 0 { tr("Models").to_string() } else { format!("{} ({count})", tr("Models")) };
    let (mut arrange, mut import) = (false, false);
    theme::card_with(
        ui,
        &title,
        |ui| {
            arrange = icons::sized(ui, Icon::Arrange, tr("Arrange models"), count > 0, false, 26.0)
                .clicked();
            import = icons::sized(ui, Icon::Plus, tr("Import models (Ctrl+O)"), true, false, 26.0)
                .clicked();
        },
        |ui| {
            if rows.is_empty() {
                ui.add_space(4.0);
                theme::dim(ui, tr("No models. Import or drop STL/OBJ files."));
                ui.add_space(4.0);
                return;
            }
            ui.spacing_mut().item_spacing.y = 2.0;
            egui::ScrollArea::vertical().id_salt("model-list").max_height(240.0).show(ui, |ui| {
                for (i, (name, tags, visible)) in rows.iter().enumerate() {
                    let (resp, eye) =
                        model_row(ui, name, tags, *visible, app.selected.contains(&i));
                    if eye {
                        app.run(Command::SetVisible { index: i, visible: !visible });
                    } else if resp.clicked() {
                        let multi = ui.input(|inp| inp.modifiers.command || inp.modifiers.shift);
                        app.click_select(i, multi);
                    }
                    resp.context_menu(|ui| {
                        app.selected = std::iter::once(i).collect();
                        if ui.button(tr("Duplicate")).clicked() {
                            app.duplicate_selected();
                        }
                        for (axis, label) in
                            [(Axis::X, "Mirror X"), (Axis::Y, "Mirror Y"), (Axis::Z, "Mirror Z")]
                        {
                            if ui.button(tr(label)).clicked() {
                                crate::gizmo::mirror(app, axis);
                            }
                        }
                        if ui.button(tr("Centre on the bed")).clicked() {
                            app.center_selected();
                        }
                        if ui.button(tr("Reset rotation and scale")).clicked() {
                            app.reset_selected();
                        }
                        ui.separator();
                        if ui.button(tr("Remove")).clicked() {
                            app.remove_selected();
                        }
                    });
                }
            });
        },
    );
    if arrange {
        app.run(Command::Arrange);
    }
    if import {
        app.open_import_dialog();
    }
}

fn profile_card(app: &mut SliceCraftApp, ui: &mut Ui) {
    let current = app.session.profile_name().to_string();
    theme::card(ui, "Print profile", |ui| {
        let mut chosen = None;
        egui::ComboBox::from_id_salt("preset")
            .selected_text(&current)
            .width(ui.available_width())
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
        ui.add_space(4.0);
        quick_settings(app, ui);
        ui.add_space(6.0);
        let w = ui.available_width();
        if ui
            .add(egui::Button::new(tr("All settings…")).min_size(vec2(w, 28.0)))
            .on_hover_text(tr("Ctrl+P"))
            .clicked()
        {
            app.open_settings();
        }
        ui.horizontal(|ui| {
            let third = (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
            if ui.add(egui::Button::new(tr("Load…")).min_size(vec2(third, 26.0))).clicked() {
                app.load_profile_dialog();
            }
            if ui.add(egui::Button::new(tr("Save…")).min_size(vec2(third, 26.0))).clicked() {
                app.save_profile_dialog();
            }
            let zones = app.session.document().zones.len();
            let label = if zones == 0 {
                tr("Zones…").to_string()
            } else {
                format!("{} ({zones})", tr("Zones"))
            };
            if ui
                .add(egui::Button::new(label).min_size(vec2(third, 26.0)))
                .on_hover_text(tr(
                    "Change settings above a height, or give some models their own settings",
                ))
                .clicked()
            {
                app.dialogs.zones = Some(Default::default());
            }
        });
    });
}

/// The few settings most people change on every job.
fn quick_settings(app: &mut SliceCraftApp, ui: &mut Ui) {
    let mut s = app.session.document().settings.clone();
    let before = s.clone();
    egui::Grid::new("quick").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        theme::field(ui, "Layer height");
        ui.add_sized(
            [VALUE_W, 24.0],
            egui::DragValue::new(&mut s.layer_height).speed(0.01).range(0.05..=1.0).suffix(" mm"),
        );
        ui.end_row();
        theme::field(ui, "Infill");
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = VALUE_W - 10.0;
            ui.add(
                egui::Slider::new(&mut s.infill_percentage, 0.0..=100.0)
                    .suffix(" %")
                    .max_decimals(0),
            );
        });
        ui.end_row();
        theme::field(ui, "Perimeters");
        ui.add_sized([VALUE_W, 24.0], egui::DragValue::new(&mut s.perimeter_count).range(0..=20));
        ui.end_row();
        theme::field(ui, "Nozzle");
        ui.add_sized(
            [VALUE_W, 24.0],
            egui::DragValue::new(&mut s.nozzle_temperature).range(0.0..=350.0).suffix(" °C"),
        );
        ui.end_row();
        theme::field(ui, "Bed");
        ui.add_sized(
            [VALUE_W, 24.0],
            egui::DragValue::new(&mut s.bed_temperature).range(0.0..=130.0).suffix(" °C"),
        );
        ui.end_row();
        theme::field(ui, "Adhesion");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.checkbox(&mut s.skirt_enabled, tr("skirt"));
            ui.checkbox(&mut s.brim_enabled, tr("brim"));
            ui.checkbox(&mut s.raft_enabled, tr("raft"));
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

/// Support: whether to generate it, the preview, and the boxes that add or
/// block it.
fn support_card(app: &mut SliceCraftApp, ui: &mut Ui) {
    use slicecraft_doc::SupportEditKind;
    let mut s = app.session.document().settings.clone();
    let before = s.clone();
    theme::card(ui, "Support", |ui| {
        ui.checkbox(&mut s.support_enabled, tr("Generate support"));
        ui.add_enabled_ui(s.support_enabled, |ui| {
            let mut bed_only = s.support_placement == SupportPlacement::BedOnly;
            if ui.checkbox(&mut bed_only, tr("Only from the bed")).changed() {
                s.support_placement =
                    if bed_only { SupportPlacement::BedOnly } else { SupportPlacement::Everywhere };
            }
        });
        let mut show = app.show_support;
        if ui.checkbox(&mut show, tr("Show before slicing")).changed() {
            app.set_show_support(show);
        }
        if app.show_support {
            let busy = app.support_job.is_some();
            match (&app.support_note, busy) {
                (_, true) => theme::dim(ui, tr("Working out support…")),
                (Some(n), false) => theme::dim(ui, n.as_str()),
                (None, false) => theme::dim(ui, ""),
            };
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            let add = app.tool == Tool::SupportAdd;
            if ui
                .add(egui::Button::new(tr("+ Add area")).selected(add).min_size(vec2(half, 28.0)))
                .on_hover_text(tr(
                    "Drag a box on the bed: every surface facing down inside it gets support",
                ))
                .clicked()
            {
                app.tool = if add { Tool::Select } else { Tool::SupportAdd };
                app.set_show_support(true);
            }
            let block = app.tool == Tool::SupportBlock;
            if ui
                .add(
                    egui::Button::new(tr("− Block area"))
                        .selected(block)
                        .min_size(vec2(half, 28.0)),
                )
                .on_hover_text(tr("Drag a box on the bed: no support inside it"))
                .clicked()
            {
                app.tool = if block { Tool::Select } else { Tool::SupportBlock };
                app.set_show_support(true);
            }
        });
        let edits = app.session.document().support_edits.clone();
        let mut remove = None;
        let p = theme::of(ui);
        for (i, e) in edits.iter().enumerate() {
            ui.horizontal(|ui| {
                let (label, colour) = match e.kind {
                    SupportEditKind::Add => (tr("add"), p.ok),
                    SupportEditKind::Block => (tr("block"), p.warn),
                };
                theme::tag(ui, label, colour);
                theme::dim(
                    ui,
                    format!("{:.0}×{:.0} mm at {:.0}, {:.0}", e.x1 - e.x0, e.y1 - e.y0, e.x0, e.y0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text(tr("Remove this box")).clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(index) = remove {
            app.run(Command::RemoveSupportEdit { index });
        }
        if !edits.is_empty() && ui.small_button(tr("Clear all boxes")).clicked() {
            app.run(Command::ClearSupportEdits);
        }
    });
    if s != before {
        app.apply_profile(s, None);
    }
}

/// Right panel in prepare mode: what the selected model is and how it
/// prints. Exact position, angle and size are in the move, rotate and scale
/// tools' own panels.
pub fn transform_panel(app: &mut SliceCraftApp, ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(2.0);
        object_cards(app, ui);
    });
}

/// The zone that gives model `i` its own settings over its whole height.
fn own_zone(app: &SliceCraftApp, i: usize) -> Option<usize> {
    app.session
        .document()
        .zones
        .iter()
        .position(|z| z.objects == [i] && z.z_from <= 0.0 && z.z_to >= slicecraft_geom::MAX_COORD)
}

/// Give model `i` its own settings from `profile`, replacing any it had.
fn give_profile(
    app: &mut SliceCraftApp,
    i: usize,
    label: &str,
    profile: &slicecraft_doc::PrintSettings,
) {
    let name = app.session.document().objects.get(i).map(|o| o.name.clone()).unwrap_or_default();
    let mut zone = slicecraft_doc::SettingZone::new(format!("{name}: {label}"));
    zone.objects = vec![i];
    zone.take_profile(&app.session.document().settings, profile);
    match own_zone(app, i) {
        Some(index) => app.run(Command::UpdateZone { index, zone: Box::new(zone) }),
        None => app.run(Command::AddZone(Box::new(zone))),
    };
}

fn object_cards(app: &mut SliceCraftApp, ui: &mut Ui) {
    let count = app.selected.len();
    if count == 0 {
        theme::card(ui, "Model", |ui| {
            theme::dim(ui, tr("Select a model in the list or the 3D view."));
            ui.add_space(6.0);
            for (k, what) in [("Q", "select"), ("W", "move"), ("E", "rotate"), ("R", "scale")] {
                ui.horizontal(|ui| {
                    theme::key(ui, k);
                    theme::dim(ui, tr(what));
                });
            }
        });
        return;
    }
    let p = theme::of(ui);
    if count > 1 {
        theme::card(ui, "Models", |ui| {
            ui.label(format!("{count} {}", tr("models selected")));
        });
        mirror_card(app, ui);
        actions_card(app, ui);
        return;
    }
    let Some(i) = app.primary() else { return };
    let Some(o) = app.session.document().objects.get(i) else { return };
    let name = o.name.clone();
    let tris = o.mesh.len();
    let placed = o.size();
    let role = o.role;
    let off_bed = app.session.document().outside_bed().contains(&name);

    theme::card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(&name).font(theme::strong(16.0))).truncate());
        theme::dim(ui, format!("{:.1} × {:.1} × {:.1} mm", placed.x, placed.y, placed.z));
        theme::dim(ui, format!("{tris} {}", tr("triangles")));
        if off_bed {
            ui.colored_label(p.warn, tr("Outside the build volume"));
        }
    });
    ui.add_space(8.0);

    // exact placement lives with the tools
    theme::card(ui, "Place exactly", |ui| {
        theme::dim(ui, tr("Pick a tool for its handles and exact values:"));
        ui.horizontal(|ui| {
            for (tool, label) in
                [(Tool::Move, "Move (W)"), (Tool::Rotate, "Rotate (E)"), (Tool::Scale, "Scale (R)")]
            {
                if ui.add(egui::Button::new(tr(label)).selected(app.tool == tool)).clicked() {
                    app.tool = tool;
                }
            }
        });
    });

    theme::card(ui, "Use as", |ui| {
        let mut r = role;
        if theme::segmented(
            ui,
            &mut r,
            &[
                (ModelRole::Print, tr("Part"), true),
                (ModelRole::Support, tr("Support"), true),
                (ModelRole::Cutter, tr("Cutter"), true),
            ],
        ) {
            app.run(Command::SetRole { index: i, role: r });
        }
        theme::dim(
            ui,
            match r {
                ModelRole::Print => tr("Printed as it is."),
                ModelRole::Support => tr("Printed with the support settings, easy to break away."),
                ModelRole::Cutter => {
                    tr("Not printed: removes its shape from the parts it overlaps.")
                }
            },
        );
    });

    // several profiles per job: this model's own settings
    let own = own_zone(app, i);
    let own_name = own
        .and_then(|z| app.session.document().zones.get(z))
        .map(|z| (z.name.clone(), z.overrides.len()));
    theme::card(ui, "Print settings", |ui| {
        match &own_name {
            Some((zname, n)) => {
                ui.label(RichText::new(zname).strong());
                theme::dim(ui, format!("{n} {}", tr("settings differ from the plate's profile")));
            }
            None => {
                theme::dim(ui, tr("Prints with the plate's profile."));
            }
        }
        let mut chosen = None;
        egui::ComboBox::from_id_salt("model-profile")
            .selected_text(tr("Use a profile for this model…"))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for preset in presets() {
                    if ui.selectable_label(false, &preset.name).clicked() {
                        chosen = Some(preset);
                    }
                }
            });
        if let Some(preset) = chosen {
            give_profile(app, i, &preset.name, &preset.settings);
        }
        ui.horizontal(|ui| {
            if ui.button(tr("From a file…")).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_title(tr("Profile for this model"))
                    .add_filter(
                        "Profiles (SliceCraft, Simplify3D)",
                        &["scprofile", "fff", "txt", "gcode"],
                    )
                    .pick_file()
            {
                match slicecraft_engine::read_profile_file(&path) {
                    Ok(profile) => give_profile(app, i, &profile.name, &profile.settings),
                    Err(e) => app.error(e.to_string()),
                }
            }
            if let Some(z) = own {
                if ui.button(tr("Edit…")).clicked() {
                    app.dialogs.zones =
                        Some(crate::dialogs::ZonesEditor { selected: z, search: String::new() });
                }
                if ui.button(tr("Back to the plate's")).clicked() {
                    app.run(Command::RemoveZone { index: z });
                }
            }
        });
    });

    mirror_card(app, ui);
    actions_card(app, ui);

    if let Some((_, geo)) = &app.geometry
        && let Some(g) = geo.objects.get(i)
    {
        let r = g.report;
        let mut repair = false;
        theme::card(ui, "Mesh check", |ui| {
            if r.is_watertight() {
                ui.horizontal(|ui| {
                    theme::tag(ui, tr("OK"), p.ok);
                    ui.label(format!(
                        "{} · {:.1} cm³",
                        tr("Closed and manifold"),
                        r.volume / 1000.0
                    ));
                });
            } else {
                ui.colored_label(
                    p.warn,
                    format!(
                        "{} {}, {} {}",
                        r.open_edges,
                        tr("open edges"),
                        r.non_manifold_edges,
                        tr("non-manifold edges")
                    ),
                );
                repair = ui
                    .button(tr("Repair"))
                    .on_hover_text(tr("Weld, fix normals and close holes"))
                    .clicked();
            }
        });
        if repair {
            app.run(Command::RepairMesh { index: i, fill: true });
        }
    }
}

/// Mirror buttons that say what they do.
fn mirror_card(app: &mut SliceCraftApp, ui: &mut Ui) {
    theme::card(ui, "Mirror", |ui| {
        ui.horizontal(|ui| {
            for (k, axis, tip) in [
                (0, Axis::X, "Flip left to right"),
                (1, Axis::Y, "Flip front to back"),
                (2, Axis::Z, "Turn upside down"),
            ] {
                let letter = ["X", "Y", "Z"].get(k).copied().unwrap_or("?");
                let colour = theme::AXES.get(k).copied().unwrap_or(Color32::GRAY);
                if ui
                    .add(
                        egui::Button::new(RichText::new(letter).color(colour).strong())
                            .min_size(vec2(44.0, 28.0)),
                    )
                    .on_hover_text(tr(tip))
                    .clicked()
                {
                    crate::gizmo::mirror(app, axis);
                }
            }
        });
        theme::dim(
            ui,
            tr("A mirror image: lettering reads backwards. A symmetric model looks the same."),
        );
    });
}

/// Actions on the selection, as a grid of equal buttons.
fn actions_card(app: &mut SliceCraftApp, ui: &mut Ui) {
    theme::card(ui, "Arrange", |ui| {
        let gap = ui.spacing().item_spacing.x;
        let half = (ui.available_width() - gap) / 2.0;
        let b = |text: &str| egui::Button::new(tr(text)).min_size(vec2(half, 28.0));
        ui.horizontal(|ui| {
            if ui.add(b("Centre")).clicked() {
                app.center_selected();
            }
            if ui.add(b("Duplicate")).clicked() {
                app.duplicate_selected();
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add(b("Lay flat"))
                .on_hover_text(tr("Put the largest flat face on the bed"))
                .clicked()
            {
                app.for_selected(|index| Command::LayFlat { index });
            }
            if ui.add(b("Reset")).on_hover_text(tr("Reset rotation and scale")).clicked() {
                app.reset_selected();
            }
        });
        ui.add_space(2.0);
        let p = theme::of(ui);
        let w = ui.available_width();
        if ui
            .add(
                egui::Button::new(RichText::new(tr("Remove")).color(p.warn))
                    .min_size(vec2(w, 28.0)),
            )
            .clicked()
        {
            app.remove_selected();
        }
    });
}

/// Left panel in preview mode: what the job costs, and what to show.
pub fn summary_panel(app: &mut SliceCraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    with_footer(ui, "preview-footer", app, summary_cards, |app, ui| {
        let ok = app.session.slice_result().is_some();
        if theme::primary(ui, tr("Export G-code…"), ok, vec2(0.0, 42.0))
            .on_hover_text(tr("Ctrl+E"))
            .clicked()
        {
            app.export(&ctx);
        }
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            let print = egui::Button::new(tr("Print…")).min_size(vec2(half, 28.0));
            egui::containers::menu::MenuButton::from_button(print).ui(ui, |ui| {
                if ui.button(tr("Over USB…")).clicked() {
                    app.dialogs.machine = true;
                    ui.close();
                }
                if ui.button(tr("To a network printer…")).clicked() {
                    app.net.open = true;
                    ui.close();
                }
            });
            if ui
                .add(egui::Button::new(tr("Back to models")).min_size(vec2(half, 28.0)))
                .on_hover_text(tr("Esc"))
                .clicked()
            {
                app.mode = Mode::Prepare;
            }
        });
    });
}

fn summary_cards(app: &mut SliceCraftApp, ui: &mut Ui) {
    let Some(out) = app.session.slice_result() else {
        theme::card(ui, "Build summary", |ui| {
            theme::dim(ui, tr("Slice to see the summary."));
        });
        return;
    };
    let s = &app.session.document().settings;
    let g = out.stats.gcode;
    let area = std::f64::consts::PI * s.filament_diameter.powi(2) / 4.0;
    // extent of the toolpaths, and extruded length by feature: measured once
    // per slice, not every frame
    let key = app.preview_cache.as_ref().map(|(k, _)| *k);
    let fresh = match (&app.summary_cache, key) {
        (Some(c), Some(k)) => c.key != k,
        (None, Some(_)) => true,
        _ => false,
    };
    if fresh && let Some((k, layers)) = &app.preview_cache {
        app.summary_cache = Some(measure(*k, layers));
    }
    let (size, hi, by_kind) = match &app.summary_cache {
        Some(c) if Some(c.key) == key => (c.size, c.top, c.by_kind.clone()),
        _ => (Vec3::ZERO, Vec3::ZERO, BTreeMap::new()),
    };
    let layers = out.layers.len();
    let warnings = out.stats.warnings.clone();
    let models = app.session.document().objects.iter().filter(|o| o.visible).count();

    theme::card(ui, "Build summary", |ui| {
        let gap = ui.spacing().item_spacing.x;
        let half = (ui.available_width() - gap) / 2.0;
        ui.horizontal(|ui| {
            theme::figure(ui, "Print time", &format_duration(g.estimated_seconds), half);
            theme::figure(ui, "Filament", &format!("{:.2} m", g.filament_mm / 1000.0), half);
        });
        ui.horizontal(|ui| {
            theme::figure(ui, "Weight", &format!("{:.1} g", g.filament_grams), half);
            theme::figure(ui, "Cost", &format!("{:.2}", g.filament_cost), half);
        });
        ui.add_space(6.0);
        egui::Grid::new("summary").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            let mut row = |k: &str, v: String| {
                theme::field(ui, k);
                ui.label(v);
                ui.end_row();
            };
            row("Size", format!("{:.1} × {:.1} × {:.1} mm", size.x, size.y, hi.z.max(0.0)));
            row("Volume", format!("{:.2} cm³", g.filament_mm * area / 1000.0));
            row("Layers", layers.to_string());
            row("Models", models.to_string());
            row("Retractions", g.retractions.to_string());
            row("Travel", format!("{:.1} m", g.travel_mm / 1000.0));
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new(tr("Weight uses the profile's filament density."))
                .size(12.0)
                .color(theme::of(ui).dim),
        );
    });

    if !by_kind.is_empty() {
        let total: f64 = by_kind.values().map(|(v, _)| *v).sum::<f64>().max(1e-9);
        let mut rows: Vec<(&str, f64, Color32)> =
            by_kind.iter().map(|(k, (v, c))| (*k, *v, *c)).collect();
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        theme::card(ui, "By feature", |ui| {
            let p = theme::of(ui);
            for (k, v, c) in rows {
                ui.horizontal(|ui| {
                    let (sw, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                    ui.painter().rect_filled(sw, 3, c);
                    ui.label(tr(k));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{:.2} m", v / 1000.0)).color(p.dim).size(12.0),
                        );
                    });
                });
                let (bar, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
                ui.painter().rect_filled(bar, 2, p.input);
                let mut fill = bar;
                fill.set_width((bar.width() * (v / total) as f32).max(2.0));
                ui.painter().rect_filled(fill, 2, c);
            }
        });
    }

    if !warnings.is_empty() {
        let warn = theme::of(ui).warn;
        theme::card(ui, "Notes", |ui| {
            for w in &warnings {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(warn, "•");
                    ui.label(w);
                });
            }
        });
    }

    theme::card(ui, "Display", |ui| {
        let p = &mut app.preview;
        egui::Grid::new("show").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            ui.checkbox(&mut p.show_travel, tr("Travel moves"));
            ui.checkbox(&mut p.show_retractions, tr("Retractions"));
            ui.end_row();
            ui.checkbox(&mut p.show_toolhead, tr("Nozzle"));
            ui.checkbox(&mut p.ghost_model, tr("Model outline"));
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            theme::field(ui, "Colour by");
            egui::ComboBox::from_id_salt("coloring")
                .selected_text(tr(p.coloring.label()))
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for c in Coloring::ALL {
                        ui.selectable_value(&mut p.coloring, c, tr(c.label()));
                    }
                });
        });
    });
}

/// Facts about a slice's toolpaths for the summary, measured once.
pub struct Measured {
    pub key: usize,
    pub size: Vec3,
    pub top: Vec3,
    pub by_kind: BTreeMap<&'static str, (f64, Color32)>,
}

fn measure(key: usize, layers: &[scene::PreviewLayer]) -> Measured {
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut by_kind: BTreeMap<&'static str, (f64, Color32)> = BTreeMap::new();
    for seg in layers.iter().flat_map(|l| l.segs.iter()) {
        if let SegKind::Path(k) = seg.kind {
            for p in [seg.a, seg.b] {
                lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
            }
            let e = by_kind.entry(k.label()).or_insert((0.0, scene::kind_color(k)));
            e.0 += (seg.b - seg.a).length();
        }
    }
    let size = if lo.x.is_finite() { hi - lo } else { Vec3::ZERO };
    Measured { key, size, top: if hi.z.is_finite() { hi } else { Vec3::ZERO }, by_kind }
}

/// Bottom bar in preview mode: playback and the layer range.
pub fn preview_controls(app: &mut SliceCraftApp, ui: &mut Ui) {
    let Some((_, layers)) = &app.preview_cache else { return };
    let max = layers.len().saturating_sub(1);
    let moves = layers.get(app.preview.last_layer).map_or(0, |l| l.segs.len());
    let p = &mut app.preview;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (icon, tip) = if p.playing {
            (Icon::Pause, tr("Pause (Space)"))
        } else {
            (Icon::Play, tr("Play (Space)"))
        };
        if icons::button(ui, icon, tip, true, p.playing).clicked() {
            p.playing = !p.playing;
            if p.playing && p.last_layer >= max && !p.by_line {
                p.last_layer = p.first_layer;
            }
        }
        if icons::button(ui, Icon::StepBack, tr("Previous layer"), true, false).clicked() {
            p.last_layer = p.last_layer.saturating_sub(1);
            p.line_cursor = 0;
        }
        if icons::button(ui, Icon::StepForward, tr("Next layer"), true, false).clicked() {
            p.last_layer = (p.last_layer + 1).min(max);
            p.line_cursor = 0;
        }
        ui.add_space(6.0);
        theme::field(ui, "Speed");
        ui.add(egui::Slider::new(&mut p.speed, 1.0..=200.0).logarithmic(true).max_decimals(0));
        theme::divider(ui);
        theme::field(ui, "Step by");
        let mut by_line = p.by_line;
        if theme::segmented(
            ui,
            &mut by_line,
            &[(false, tr("layer"), true), (true, tr("move"), true)],
        ) {
            p.by_line = by_line;
            p.line_cursor = 0;
        }
        theme::divider(ui);
        let mut only = p.only_n.is_some();
        if ui.checkbox(&mut only, tr("Only the last")).changed() {
            p.only_n = if only { Some(1) } else { None };
        }
        if let Some(n) = &mut p.only_n {
            ui.add(egui::DragValue::new(n).range(1..=10_000));
            ui.label(tr("layers"));
        }
    });
    ui.add_space(2.0);
    let w = (ui.available_width() - 190.0).max(120.0);
    egui::Grid::new("layer-range").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        theme::field(ui, "From layer");
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = w;
            ui.add(
                egui::Slider::new(&mut p.first_layer, 0..=max)
                    .custom_formatter(|v, _| format!("{}", v as usize + 1)),
            );
        });
        ui.end_row();
        theme::field(ui, "To layer");
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = w;
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
        ui.end_row();
        if p.by_line {
            theme::field(ui, "Move");
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = w;
                ui.add(egui::Slider::new(&mut p.line_cursor, 0..=moves));
            });
            ui.end_row();
        }
    });
    if p.first_layer > p.last_layer {
        p.first_layer = p.last_layer;
    }
    ui.add_space(2.0);
}

pub fn status_bar(app: &mut SliceCraftApp, ui: &mut Ui) {
    let p = theme::of(ui);
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
                    .desired_height(16.0)
                    .fill(p.accent)
                    .corner_radius(8)
                    .text(tr(stage.label())),
            );
        }
        let color = if app.status_is_error { p.warn } else { p.text };
        let status = app.status.clone();
        // leave room for the profile, bed and model count on the right; the
        // whole message shows on hover
        let room = (ui.available_width() - 440.0).max(120.0);
        ui.scope(|ui| {
            ui.set_max_width(room);
            ui.add(
                egui::Label::new(RichText::new(&status).size(13.0).color(color))
                    .truncate()
                    .sense(Sense::hover()),
            )
            .on_hover_text(&status);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let doc = app.session.document();
            let info = |ui: &mut Ui, text: String| {
                ui.label(RichText::new(text).size(12.0).color(p.dim));
            };
            info(ui, format!("{} {}", tr("Models:"), doc.objects.len()));
            theme::divider(ui);
            info(
                ui,
                format!(
                    "{} {:.0}×{:.0}×{:.0} mm",
                    tr("bed"),
                    doc.bed.width,
                    doc.bed.depth,
                    doc.bed.height
                ),
            );
            theme::divider(ui);
            info(ui, app.session.profile_name().to_string());
        });
    });
}

/// Legend rows for the current colouring, for the viewport overlay: colour,
/// label, and for feature colouring the index of the kind's show/hide switch.
/// Kinds the job does not contain are left out.
pub fn legend(app: &SliceCraftApp) -> Vec<(Color32, String, Option<usize>)> {
    let plain = |v: Vec<(Color32, String)>| v.into_iter().map(|(c, l)| (c, l, None)).collect();
    match app.preview.coloring {
        Coloring::Feature => {
            let mut v: Vec<(Color32, String, Option<usize>)> = PathKind::ALL
                .iter()
                .enumerate()
                .filter(|(i, _)| app.kinds_present.get(*i).copied().unwrap_or(true))
                .map(|(i, k)| (scene::kind_color(*k), tr(k.label()).to_string(), Some(i)))
                .collect();
            if app.preview.show_travel {
                v.push((scene::TRAVEL, tr("travel").into(), None));
            }
            v
        }
        Coloring::Speed => {
            let (lo, hi) =
                app.preview_cache.as_ref().map_or((0.0, 1.0), |(_, l)| scene::speed_range(l));
            plain(
                (0..5)
                    .map(|k| {
                        let t = f64::from(k) / 4.0;
                        let v = f64::from(lo) + t * f64::from(hi - lo);
                        (scene::ramp(t), format!("{v:.0} mm/s"))
                    })
                    .collect(),
            )
        }
        Coloring::Height => {
            let top = app.preview_cache.as_ref().and_then(|(_, l)| l.last()).map_or(0.0, |l| l.z);
            plain(
                (0..5)
                    .map(|k| {
                        let t = f64::from(k) / 4.0;
                        (scene::ramp(t), format!("{:.1} mm", t * top))
                    })
                    .collect(),
            )
        }
        Coloring::Tool => plain(
            app.preview_cache
                .as_ref()
                .map(|(_, l)| scene::tools_used(l))
                .unwrap_or_default()
                .into_iter()
                .map(|t| (scene::tool_color(t), format!("{} {t}", tr("tool"))))
                .collect(),
        ),
        Coloring::Flow => {
            let (lo, hi) =
                app.preview_cache.as_ref().map_or((0.0, 1.0), |(_, l)| scene::flow_range(l));
            plain(
                (0..5)
                    .map(|k| {
                        let t = f64::from(k) / 4.0;
                        let v = f64::from(lo) + t * f64::from(hi - lo);
                        (scene::ramp(t), format!("{v:.1} mm³/s"))
                    })
                    .collect(),
            )
        }
    }
}
