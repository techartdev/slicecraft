//! The menu bar and the toolbar.
//!
//! Every item is either wired to an engine command or listed in
//! `slicecraft_engine::COMMANDS` as planned; planned items are shown disabled
//! with a "planned" note, and clicking their row opens an explanation.

use crate::app::{Mode, Pending, SliceCraftApp, Tool};
use crate::camera::ViewPreset;
use crate::icons::{self, Icon};
use eframe::egui::{self, Ui};
use slicecraft_doc::Axis;
use slicecraft_engine::{COMMANDS, Command, CommandState};

/// A menu row for a command: enabled when ready (and `ok`), else marked planned.
fn item(app: &mut SliceCraftApp, ui: &mut Ui, id: &'static str, shortcut: &str, ok: bool) -> bool {
    let Some(spec) = COMMANDS.iter().find(|c| c.id == id) else { return false };
    let ready = spec.state == CommandState::Ready;
    if ready {
        let b = egui::Button::new(spec.label).shortcut_text(shortcut);
        ui.add_enabled(ok, b).clicked()
    } else {
        let b = egui::Button::new(format!("{}  (planned)", spec.label)).shortcut_text(shortcut);
        if ui.add(b).on_hover_text("Not implemented yet").clicked() {
            app.planned(id);
        }
        false
    }
}

fn plain(ui: &mut Ui, label: &str, shortcut: &str, ok: bool) -> bool {
    ui.add_enabled(ok, egui::Button::new(label).shortcut_text(shortcut)).clicked()
}

pub fn menu_bar(app: &mut SliceCraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let has_models = !app.session.document().objects.is_empty();
    let has_sel = !app.selected.is_empty();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if item(app, ui, "new-project", "Ctrl+N", true) {
                app.request(Pending::NewProject);
            }
            if item(app, ui, "open-project", "Ctrl+Shift+O", true) {
                app.request(Pending::OpenProject(None));
            }
            let recent = app.recent.clone();
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                ui.menu_button("Open Recent", |ui| {
                    for path in recent {
                        let label = path.file_name().map_or_else(
                            || path.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        if ui.button(label).on_hover_text(path.display().to_string()).clicked() {
                            app.request(Pending::OpenProject(Some(path)));
                        }
                    }
                    ui.separator();
                    if ui.button("Clear the list").clicked() {
                        app.recent.clear();
                        app.persist();
                    }
                });
            });
            if item(app, ui, "save-project", "Ctrl+S", true) {
                app.save_project(false);
            }
            if plain(ui, "Save Project As…", "Ctrl+Shift+S", true) {
                app.save_project(true);
            }
            ui.separator();
            if item(app, ui, "import", "Ctrl+O", true) {
                app.open_import_dialog();
            }
            item(app, ui, "open-gcode", "", true);
            item(app, ui, "export-models", "", true);
            ui.separator();
            if item(app, ui, "load-profile", "", true) {
                app.load_profile_dialog();
            }
            if item(app, ui, "save-profile", "", true) {
                app.save_profile_dialog();
            }
            ui.separator();
            if item(app, ui, "export-gcode", "Ctrl+E", has_models && app.job.is_none()) {
                app.export(&ctx);
            }
            ui.separator();
            if plain(ui, "Preferences…", "", true) {
                app.dialogs.preferences = true;
            }
            ui.separator();
            if plain(ui, "Quit", "", true) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button("Edit", |ui| {
            if item(app, ui, "undo", "Ctrl+Z", app.session.can_undo()) {
                app.run(Command::Undo);
            }
            if item(app, ui, "redo", "Ctrl+Y", app.session.can_redo()) {
                app.run(Command::Redo);
            }
            ui.separator();
            if plain(ui, "Select all", "Ctrl+A", has_models) {
                app.select_all();
            }
            if plain(ui, "Select none", "Esc", has_sel) {
                app.selected.clear();
            }
            ui.separator();
            if item(app, ui, "duplicate", "Ctrl+D", has_sel) {
                app.duplicate_selected();
            }
            if item(app, ui, "remove", "Del", has_sel) {
                app.remove_selected();
            }
            ui.separator();
            if item(app, ui, "arrange", "", has_models) {
                app.run(Command::Arrange);
            }
            if plain(ui, "Centre selection", "", has_sel) {
                app.center_selected();
            }
            if plain(ui, "Reset rotation and scale", "", has_sel) {
                app.reset_selected();
            }
            item(app, ui, "group-models", "", true);
            item(app, ui, "merge-models", "", true);
        });
        ui.menu_button("View", |ui| {
            for v in ViewPreset::ALL {
                let key = match v {
                    ViewPreset::Iso => "0",
                    ViewPreset::Front => "1",
                    ViewPreset::Back => "2",
                    ViewPreset::Left => "3",
                    ViewPreset::Right => "4",
                    ViewPreset::Top => "5",
                    ViewPreset::Bottom => "6",
                };
                if plain(ui, v.label(), key, true) {
                    app.view(v);
                }
            }
            if plain(ui, "Fit models", "F", true) {
                app.fit_view();
            }
            if plain(ui, "Fit selection", "", has_sel) {
                app.fit_selection();
            }
            ui.add_enabled_ui(false, |ui| {
                let _ = ui.button("Orthographic camera  (planned)");
                let _ = ui.button("Wireframe and normals  (planned)");
            });
            ui.separator();
            ui.checkbox(&mut app.prefs.bed.grid, "Bed grid");
            ui.checkbox(&mut app.prefs.bed.volume, "Build volume");
            ui.checkbox(&mut app.prefs.bed.axes, "Axes");
            let mut section = app.section.is_some();
            if ui.checkbox(&mut section, "Section view").changed() {
                app.section = if section { Some(10.0) } else { None };
            }
            ui.separator();
            ui.add_enabled_ui(app.preview_cache.is_some(), |ui| {
                if ui.selectable_label(app.mode == Mode::Preview, "Toolpath preview").clicked() {
                    app.mode = Mode::Preview;
                }
            });
            if ui.selectable_label(app.mode == Mode::Prepare, "Prepare models").clicked() {
                app.mode = Mode::Prepare;
            }
        });
        ui.menu_button("Model", |ui| {
            for (axis, label) in
                [(Axis::X, "Mirror X"), (Axis::Y, "Mirror Y"), (Axis::Z, "Mirror Z")]
            {
                if plain(ui, label, "", has_sel) {
                    app.mirror_selected(axis);
                }
            }
            ui.separator();
            for (axis, label) in [
                (Axis::X, "Rotate 90° about X"),
                (Axis::Y, "Rotate 90° about Y"),
                (Axis::Z, "Rotate 90° about Z"),
            ] {
                if plain(ui, label, "", has_sel) {
                    app.rotate_selected(axis, 90.0);
                }
            }
            ui.separator();
            if plain(ui, "Scale to fit the bed", "", has_sel) {
                app.scale_to_fit_bed();
            }
            ui.menu_button("Correct units", |ui| {
                if plain(ui, "Inches → millimetres (×25.4)", "", has_sel) {
                    app.scale_selected(25.4);
                }
                if plain(ui, "Centimetres → millimetres (×10)", "", has_sel) {
                    app.scale_selected(10.0);
                }
                if plain(ui, "Millimetres → inches (÷25.4)", "", has_sel) {
                    app.scale_selected(1.0 / 25.4);
                }
            });
            ui.separator();
            item(app, ui, "model-role", "", true);
            item(app, ui, "lay-flat", "", true);
            item(app, ui, "split-parts", "", true);
            item(app, ui, "cut-plane", "", true);
            item(app, ui, "hollow", "", true);
        });
        ui.menu_button("Repair", |ui| {
            if item(app, ui, "analyze-mesh", "", has_models) {
                app.dialogs.analysis = true;
            }
            ui.separator();
            item(app, ui, "find-intersections", "", true);
            item(app, ui, "repair-mesh", "", true);
            item(app, ui, "reduce-mesh", "", true);
            item(app, ui, "fix-normals", "", true);
            item(app, ui, "fill-holes", "", true);
        });
        ui.menu_button("Supports", |ui| {
            item(app, ui, "supports", "", true);
            item(app, ui, "support-edit", "", true);
            item(app, ui, "support-clear", "", true);
            item(app, ui, "support-blockers", "", true);
        });
        ui.menu_button("Slice", |ui| {
            if app.job.is_some() {
                if plain(ui, "Cancel slicing", "", true) {
                    app.cancel_slice();
                }
            } else if item(app, ui, "slice", "Ctrl+R", has_models) {
                app.start_slice(&ctx);
            }
            if plain(ui, "Print settings…", "Ctrl+P", true) {
                app.open_settings();
            }
            ui.separator();
            item(app, ui, "multi-profile", "", true);
            item(app, ui, "setting-zones", "", true);
            item(app, ui, "variable-layers", "", true);
        });
        ui.menu_button("Machine", |ui| {
            if item(app, ui, "machine-control", "", true) {
                app.dialogs.machine = true;
            }
            item(app, ui, "print-usb", "", true);
            item(app, ui, "print-network", "", true);
            item(app, ui, "firmware-terminal", "", true);
            item(app, ui, "camera-monitor", "", true);
            ui.separator();
            item(app, ui, "profile-wizard", "", true);
        });
        ui.menu_button("Help", |ui| {
            if plain(ui, "Keyboard shortcuts", "", true) {
                app.dialogs.shortcuts = true;
            }
            if plain(ui, "Feature status", "", true) {
                app.dialogs.features = true;
            }
            if plain(ui, "About SliceCraft", "", true) {
                app.dialogs.about = true;
            }
        });
    });
}

pub fn toolbar(app: &mut SliceCraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let has_models = !app.session.document().objects.is_empty();
    let has_sel = !app.selected.is_empty();
    let ready = SliceCraftApp::command_ready;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if icons::button(ui, Icon::Import, "Import models (Ctrl+O)", true, false).clicked() {
            app.open_import_dialog();
        }
        if icons::button(ui, Icon::Arrange, "Arrange models", has_models, false).clicked() {
            app.run(Command::Arrange);
        }
        if icons::button(ui, Icon::Undo, "Undo (Ctrl+Z)", app.session.can_undo(), false).clicked() {
            app.run(Command::Undo);
        }
        if icons::button(ui, Icon::Redo, "Redo (Ctrl+Y)", app.session.can_redo(), false).clicked() {
            app.run(Command::Redo);
        }
        ui.separator();
        for (v, icon) in [
            (ViewPreset::Iso, Icon::ViewIso),
            (ViewPreset::Front, Icon::ViewFront),
            (ViewPreset::Back, Icon::ViewBack),
            (ViewPreset::Left, Icon::ViewLeft),
            (ViewPreset::Right, Icon::ViewRight),
            (ViewPreset::Top, Icon::ViewTop),
            (ViewPreset::Bottom, Icon::ViewBottom),
        ] {
            if icons::button(ui, icon, &format!("{} view", v.label()), true, false).clicked() {
                app.view(v);
            }
        }
        if icons::button(ui, Icon::Fit, "Fit models (F)", true, false).clicked() {
            app.fit_view();
        }
        if icons::button(ui, Icon::Grid, "Bed grid", true, app.prefs.bed.grid).clicked() {
            app.prefs.bed.grid = !app.prefs.bed.grid;
        }
        if icons::button(ui, Icon::Section, "Section view", true, app.section.is_some()).clicked() {
            app.section = if app.section.is_some() { None } else { Some(10.0) };
        }
        ui.separator();
        if app.mode == Mode::Prepare {
            for (tool, icon) in [
                (Tool::Select, Icon::Select),
                (Tool::Move, Icon::Move),
                (Tool::Rotate, Icon::Rotate),
                (Tool::Scale, Icon::Scale),
            ] {
                if icons::button(ui, icon, tool.label(), true, app.tool == tool).clicked() {
                    app.tool = tool;
                }
            }
            ui.separator();
            if icons::button(ui, Icon::Duplicate, "Duplicate (Ctrl+D)", has_sel, false).clicked() {
                app.duplicate_selected();
            }
            if icons::button(ui, Icon::MirrorX, "Mirror X", has_sel, false).clicked() {
                app.mirror_selected(Axis::X);
            }
            if icons::button(ui, Icon::MirrorY, "Mirror Y", has_sel, false).clicked() {
                app.mirror_selected(Axis::Y);
            }
            if icons::button(ui, Icon::Center, "Centre on the bed", has_sel, false).clicked() {
                app.center_selected();
            }
            if icons::button(ui, Icon::LayFlat, "Lay a face flat", ready("lay-flat"), false)
                .clicked()
            {
                app.planned("lay-flat");
            }
            if icons::button(ui, Icon::Delete, "Remove (Del)", has_sel, false).clicked() {
                app.remove_selected();
            }
            ui.separator();
            if icons::button(ui, Icon::Analyze, "Analyse mesh", has_models, false).clicked() {
                app.dialogs.analysis = true;
            }
            if icons::button(ui, Icon::Repair, "Repair mesh", ready("repair-mesh"), false).clicked()
            {
                app.planned("repair-mesh");
            }
            if icons::button(ui, Icon::Supports, "Generate supports", ready("supports"), false)
                .clicked()
            {
                app.planned("supports");
            }
            if icons::button(ui, Icon::SupportAdd, "Add support", ready("support-edit"), false)
                .clicked()
            {
                app.planned("support-edit");
            }
            if icons::button(
                ui,
                Icon::SupportRemove,
                "Remove support",
                ready("support-edit"),
                false,
            )
            .clicked()
            {
                app.planned("support-edit");
            }
        }
        ui.separator();
        if icons::button(ui, Icon::Settings, "Print settings (Ctrl+P)", true, false).clicked() {
            app.open_settings();
        }
        if icons::button(ui, Icon::Machine, "Machine control", true, false).clicked() {
            app.dialogs.machine = true;
        }
        ui.separator();
        if app.job.is_some() {
            if ui.button("Cancel slicing").clicked() {
                app.cancel_slice();
            }
        } else {
            let slice = egui::Button::new(egui::RichText::new("  Slice  ").strong());
            if ui.add_enabled(has_models, slice).on_hover_text("Ctrl+R").clicked() {
                app.start_slice(&ctx);
            }
        }
        let can_export = has_models && app.job.is_none();
        if icons::button(ui, Icon::Export, "Export G-code (Ctrl+E)", can_export, false).clicked() {
            app.export(&ctx);
        }
        ui.separator();
        ui.selectable_value(&mut app.mode, Mode::Prepare, "Prepare");
        ui.add_enabled_ui(app.preview_cache.is_some(), |ui| {
            ui.selectable_value(&mut app.mode, Mode::Preview, "Preview");
        });
    });
}
