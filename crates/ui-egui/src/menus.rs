//! The menu bar and the toolbar.
//!
//! Every item is either wired to an engine command or listed in
//! `slicecraft_engine::COMMANDS` as planned; planned items are shown disabled
//! with a "planned" note, and clicking their row opens an explanation.

use crate::app::{Mode, Pending, SliceCraftApp, Tool};
use crate::camera::ViewPreset;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::keys::Action;
use eframe::egui::{self, Ui};
use slicecraft_doc::{Axis, ModelRole};
use slicecraft_engine::{COMMANDS, Command, CommandState};

/// A menu row for a command: enabled when ready (and `ok`), else marked planned.
fn item(app: &mut SliceCraftApp, ui: &mut Ui, id: &'static str, shortcut: &str, ok: bool) -> bool {
    let Some(spec) = COMMANDS.iter().find(|c| c.id == id) else { return false };
    let ready = spec.state == CommandState::Ready;
    if ready {
        let b = egui::Button::new(tr(spec.label)).shortcut_text(shortcut);
        ui.add_enabled(ok, b).clicked()
    } else {
        let b = egui::Button::new(format!("{}  (planned)", tr(spec.label))).shortcut_text(shortcut);
        if ui.add(b).on_hover_text(tr("Not implemented yet")).clicked() {
            app.planned(id);
        }
        false
    }
}

fn plain(ui: &mut Ui, label: &str, shortcut: &str, ok: bool) -> bool {
    ui.add_enabled(ok, egui::Button::new(tr(label)).shortcut_text(shortcut)).clicked()
}

pub fn menu_bar(app: &mut SliceCraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let has_models = !app.session.document().objects.is_empty();
    let has_sel = !app.selected.is_empty();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(tr("File"), |ui| {
            if item(app, ui, "new-project", &app.prefs.keys.text(Action::NewProject), true) {
                app.request(Pending::NewProject);
            }
            if item(app, ui, "open-project", &app.prefs.keys.text(Action::OpenProject), true) {
                app.request(Pending::OpenProject(None));
            }
            let recent = app.recent.clone();
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                ui.menu_button(tr("Open Recent"), |ui| {
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
            if item(app, ui, "save-project", &app.prefs.keys.text(Action::SaveProject), true) {
                app.save_project(false);
            }
            if plain(ui, "Save Project As…", &app.prefs.keys.text(Action::SaveProjectAs), true) {
                app.save_project(true);
            }
            ui.separator();
            if item(app, ui, "import", &app.prefs.keys.text(Action::Import), true) {
                app.open_import_dialog();
            }
            if item(app, ui, "open-gcode", "", true) {
                app.open_gcode_dialog();
            }
            if item(app, ui, "export-models", "", has_models) {
                app.export_models_dialog();
            }
            ui.separator();
            if item(app, ui, "load-profile", "", true) {
                app.load_profile_dialog();
            }
            if item(app, ui, "save-profile", "", true) {
                app.save_profile_dialog();
            }
            if item(app, ui, "compare-profile", "", true) {
                app.compare_profile_dialog();
            }
            ui.separator();
            if item(
                app,
                ui,
                "export-gcode",
                &app.prefs.keys.text(Action::ExportGcode),
                has_models && app.job.is_none(),
            ) {
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
        ui.menu_button(tr("Edit"), |ui| {
            if item(app, ui, "undo", &app.prefs.keys.text(Action::Undo), app.session.can_undo()) {
                app.run(Command::Undo);
            }
            if item(app, ui, "redo", &app.prefs.keys.text(Action::Redo), app.session.can_redo()) {
                app.run(Command::Redo);
            }
            ui.separator();
            if plain(ui, "Select all", &app.prefs.keys.text(Action::SelectAll), has_models) {
                app.select_all();
            }
            if plain(ui, "Select none", "Esc", has_sel) {
                app.selected.clear();
            }
            ui.separator();
            if item(app, ui, "duplicate", &app.prefs.keys.text(Action::Duplicate), has_sel) {
                app.duplicate_selected();
            }
            if item(app, ui, "remove", &app.prefs.keys.text(Action::Remove), has_sel) {
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
            if item(app, ui, "group-models", "", app.selected.len() >= 2) {
                let indices: Vec<usize> = app.selected.iter().copied().collect();
                app.run(Command::GroupModels { indices });
            }
            let grouped = app
                .selected
                .iter()
                .any(|&i| app.session.document().objects.get(i).is_some_and(|o| o.group.is_some()));
            if plain(ui, "Ungroup", "", grouped) {
                let indices: Vec<usize> = app.selected.iter().copied().collect();
                app.run(Command::UngroupModels { indices });
            }
            if item(app, ui, "merge-models", "", app.selected.len() >= 2) {
                let indices: Vec<usize> = app.selected.iter().copied().collect();
                app.run(Command::MergeModels { indices });
                app.selected.clear();
            }
        });
        ui.menu_button(tr("View"), |ui| {
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
            if plain(ui, "Fit models", &app.prefs.keys.text(Action::FitView), true) {
                app.fit_view();
            }
            if plain(ui, "Fit selection", "", has_sel) {
                app.fit_selection();
            }
            ui.checkbox(&mut app.prefs.ortho, tr("Orthographic camera"));
            ui.checkbox(&mut app.prefs.wireframe, tr("Wireframe"));
            ui.separator();
            ui.checkbox(&mut app.prefs.bed.grid, tr("Bed grid"));
            ui.checkbox(&mut app.prefs.bed.volume, tr("Build volume"));
            ui.checkbox(&mut app.prefs.bed.axes, tr("Axes"));
            let mut section = app.section.is_some();
            if ui.checkbox(&mut section, tr("Section view")).changed() {
                app.section = if section { Some(10.0) } else { None };
            }
            ui.separator();
            ui.add_enabled_ui(app.preview_cache.is_some(), |ui| {
                if ui.selectable_label(app.mode == Mode::Preview, tr("Toolpath preview")).clicked()
                {
                    app.mode = Mode::Preview;
                }
            });
            if ui.selectable_label(app.mode == Mode::Prepare, tr("Prepare models")).clicked() {
                app.mode = Mode::Prepare;
            }
        });
        ui.menu_button(tr("Model"), |ui| {
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
            ui.menu_button(tr("Correct units"), |ui| {
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
            ui.add_enabled_ui(has_sel, |ui| {
                ui.menu_button(tr("Use Model As"), |ui| {
                    for (role, label) in [
                        (ModelRole::Print, "Part (print it)"),
                        (ModelRole::Support, "Support (print as support material)"),
                        (ModelRole::Cutter, "Cutter (remove it from the parts)"),
                    ] {
                        if ui.button(label).clicked() {
                            app.for_selected(|index| Command::SetRole { index, role });
                            ui.close();
                        }
                    }
                });
            });
            if item(app, ui, "lay-flat", "", has_sel) {
                app.for_selected(|index| Command::LayFlat { index });
            }
            if item(app, ui, "split-parts", "", has_sel)
                && let Some(index) = app.primary()
            {
                app.run(Command::SplitParts { index });
            }
            if item(app, ui, "cut-plane", "", has_sel)
                && let Some(index) = app.primary()
            {
                // at the section height when the section view is on, else half way
                let half =
                    app.session.document().objects.get(index).map_or(0.0, |o| o.size().z / 2.0);
                let z = app.section.unwrap_or(half);
                app.run(Command::CutPlane { index, z });
            }
            ui.add_enabled_ui(has_sel, |ui| {
                ui.menu_button(tr("Hollow Model"), |ui| {
                    for wall in [1.0, 2.0, 3.0, 5.0] {
                        if ui.button(format!("Walls {wall} mm")).clicked() {
                            app.for_selected(|index| Command::Hollow { index, wall });
                            ui.close();
                        }
                    }
                });
            });
        });
        ui.menu_button(tr("Repair"), |ui| {
            if item(app, ui, "analyze-mesh", "", has_models) {
                app.dialogs.analysis = true;
            }
            ui.separator();
            if item(app, ui, "find-intersections", "", has_sel)
                && let Some(index) = app.primary()
            {
                app.run(Command::FindIntersections { index });
            }
            if item(app, ui, "repair-mesh", "", has_sel) {
                app.for_selected(|index| Command::RepairMesh { index, fill: true });
            }
            ui.add_enabled_ui(has_sel, |ui| {
                ui.menu_button(tr("Reduce Triangle Count"), |ui| {
                    for cell in [0.1, 0.25, 0.5, 1.0] {
                        if ui.button(format!("Merge points within {cell} mm")).clicked() {
                            app.for_selected(|index| Command::ReduceMesh { index, cell });
                            ui.close();
                        }
                    }
                });
            });
            if item(app, ui, "fix-normals", "", has_sel) {
                app.for_selected(|index| Command::RepairMesh { index, fill: false });
            }
            if item(app, ui, "fill-holes", "", has_sel) {
                app.for_selected(|index| Command::RepairMesh { index, fill: true });
            }
        });
        ui.menu_button(tr("Supports"), |ui| {
            let mut show = app.show_support;
            if ui.checkbox(&mut show, tr("Show Support Before Slicing")).changed() {
                app.set_show_support(show);
            }
            ui.separator();
            let mut on = app.session.document().settings.support_enabled;
            if ui
                .checkbox(&mut on, "Generate Supports")
                .on_hover_text(tr("Build support under overhangs when slicing"))
                .changed()
            {
                app.run(Command::Supports(on));
            }
            if item(app, ui, "support-edit", "", true) {
                app.tool = Tool::SupportAdd;
                app.set_show_support(true);
            }
            if item(app, ui, "support-blockers", "", true) {
                app.tool = Tool::SupportBlock;
                app.set_show_support(true);
            }
            let edits = app.session.document().support_edits.len();
            if item(app, ui, "support-clear", "", edits > 0) {
                app.run(Command::ClearSupportEdits);
            }
        });
        ui.menu_button(tr("Slice"), |ui| {
            if app.job.is_some() {
                if plain(ui, "Cancel slicing", "", true) {
                    app.cancel_slice();
                }
            } else if item(app, ui, "slice", &app.prefs.keys.text(Action::Slice), has_models) {
                app.start_slice(&ctx);
            }
            if plain(ui, "Print settings…", &app.prefs.keys.text(Action::PrintSettings), true) {
                app.open_settings();
            }
            ui.separator();
            let several = item(app, ui, "multi-profile", "", true);
            let zones = item(app, ui, "setting-zones", "", true);
            if several || zones {
                app.dialogs.zones = Some(Default::default());
            }
            if item(app, ui, "variable-layers", "", true) {
                app.dialogs.zones = Some(Default::default());
                app.info("Add a height zone and set its layer height: layers follow it.");
            }
        });
        ui.menu_button(tr("Machine"), |ui| {
            if item(app, ui, "machine-control", "", true) {
                app.dialogs.machine = true;
            }
            if item(app, ui, "print-usb", "", true) {
                app.dialogs.machine = true;
            }
            if item(app, ui, "print-network", "", true) {
                app.net.open = true;
            }
            if item(app, ui, "firmware-terminal", "", true) {
                app.dialogs.machine = true;
            }
            if item(app, ui, "camera-monitor", "", true) {
                app.net.camera_open = true;
            }
            ui.separator();
            if item(app, ui, "profile-wizard", "", true) {
                app.wizard = Some(Default::default());
            }
        });
        ui.menu_button(tr("Help"), |ui| {
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
        if icons::button(ui, Icon::Import, tr("Import models (Ctrl+O)"), true, false).clicked() {
            app.open_import_dialog();
        }
        if icons::button(ui, Icon::Arrange, tr("Arrange models"), has_models, false).clicked() {
            app.run(Command::Arrange);
        }
        if icons::button(ui, Icon::Undo, tr("Undo (Ctrl+Z)"), app.session.can_undo(), false)
            .clicked()
        {
            app.run(Command::Undo);
        }
        if icons::button(ui, Icon::Redo, tr("Redo (Ctrl+Y)"), app.session.can_redo(), false)
            .clicked()
        {
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
        if icons::button(ui, Icon::Fit, tr("Fit models (F)"), true, false).clicked() {
            app.fit_view();
        }
        if icons::button(ui, Icon::Grid, tr("Bed grid"), true, app.prefs.bed.grid).clicked() {
            app.prefs.bed.grid = !app.prefs.bed.grid;
        }
        if icons::button(ui, Icon::Section, tr("Section view"), true, app.section.is_some())
            .clicked()
        {
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
            if icons::button(ui, Icon::Duplicate, tr("Duplicate (Ctrl+D)"), has_sel, false)
                .clicked()
            {
                app.duplicate_selected();
            }
            if icons::button(ui, Icon::MirrorX, tr("Mirror X"), has_sel, false).clicked() {
                app.mirror_selected(Axis::X);
            }
            if icons::button(ui, Icon::MirrorY, tr("Mirror Y"), has_sel, false).clicked() {
                app.mirror_selected(Axis::Y);
            }
            if icons::button(ui, Icon::Center, tr("Centre on the bed"), has_sel, false).clicked() {
                app.center_selected();
            }
            if icons::button(
                ui,
                Icon::LayFlat,
                "Lay the largest flat face on the bed",
                has_sel,
                false,
            )
            .clicked()
            {
                app.for_selected(|index| Command::LayFlat { index });
            }
            if icons::button(ui, Icon::Delete, tr("Remove (Del)"), has_sel, false).clicked() {
                app.remove_selected();
            }
            ui.separator();
            if icons::button(ui, Icon::Analyze, tr("Analyse mesh"), has_models, false).clicked() {
                app.dialogs.analysis = true;
            }
            if icons::button(ui, Icon::Repair, tr("Repair mesh"), has_sel, false).clicked() {
                app.for_selected(|index| Command::RepairMesh { index, fill: true });
            }
            let on = app.session.document().settings.support_enabled;
            let tip = if on {
                "Support is on (click to turn off)"
            } else {
                "Generate support when slicing"
            };
            if icons::button(ui, Icon::Supports, tip, ready("supports"), on).clicked() {
                app.run(Command::Supports(!on));
            }
            let adding = app.tool == Tool::SupportAdd;
            if icons::button(
                ui,
                Icon::SupportAdd,
                "Add support: drag a box on the bed",
                true,
                adding,
            )
            .clicked()
            {
                app.tool = if adding { Tool::Select } else { Tool::SupportAdd };
                app.set_show_support(true);
            }
            if icons::button(
                ui,
                Icon::SupportRemove,
                "Block support: drag a box on the bed",
                true,
                app.tool == Tool::SupportBlock,
            )
            .clicked()
            {
                let blocking = app.tool == Tool::SupportBlock;
                app.tool = if blocking { Tool::Select } else { Tool::SupportBlock };
                app.set_show_support(true);
            }
        }
        ui.separator();
        if icons::button(ui, Icon::Settings, tr("Print settings (Ctrl+P)"), true, false).clicked() {
            app.open_settings();
        }
        if icons::button(ui, Icon::Machine, tr("Machine control"), true, false).clicked() {
            app.dialogs.machine = true;
        }
        ui.separator();
        if app.job.is_some() {
            if ui.button("Cancel slicing").clicked() {
                app.cancel_slice();
            }
        } else {
            let slice = egui::Button::new(egui::RichText::new(tr("  Slice  ")).strong());
            if ui.add_enabled(has_models, slice).on_hover_text(tr("Ctrl+R")).clicked() {
                app.start_slice(&ctx);
            }
        }
        let can_export = has_models && app.job.is_none();
        if icons::button(ui, Icon::Export, tr("Export G-code (Ctrl+E)"), can_export, false)
            .clicked()
        {
            app.export(&ctx);
        }
        ui.separator();
        ui.selectable_value(&mut app.mode, Mode::Prepare, "Prepare");
        ui.add_enabled_ui(app.preview_cache.is_some(), |ui| {
            ui.selectable_value(&mut app.mode, Mode::Preview, "Preview");
        });
    });
}
