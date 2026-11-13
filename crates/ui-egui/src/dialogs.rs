//! Windows that open over the main view: the profile editor (rendered from the
//! settings schema), preferences, mesh analysis, the machine panel
//! (placeholder), shortcuts, feature status and about.

use crate::app::SliceCraftApp;
use eframe::egui::{self, Color32, RichText, Ui};
use slicecraft_doc::PrintSettings;
use slicecraft_doc::profile::presets;
use slicecraft_doc::settings::{
    SETTINGS, SettingKind, SettingSpec, SettingValue, Tab, tab_settings,
};
use slicecraft_engine::{COMMANDS, CommandSpec, CommandState};

const WARN: Color32 = Color32::from_rgb(0xff, 0x7a, 0x70);
const CHANGED: Color32 = Color32::from_rgb(0x6c, 0xb4, 0xff);
const PLANNED: Color32 = Color32::from_rgb(0xc9, 0xa2, 0x4c);

/// Which windows are open.
#[derive(Default)]
pub struct Dialogs {
    pub settings: Option<SettingsEditor>,
    pub preferences: bool,
    pub machine: bool,
    pub analysis: bool,
    pub shortcuts: bool,
    pub features: bool,
    pub about: bool,
    pub planned: Option<&'static CommandSpec>,
    /// The last profile imported from another slicer, with its report.
    pub import_report: Option<(String, slicecraft_io::ImportReport)>,
    /// The setting-zones window: open, with the selected zone and the search.
    pub zones: Option<ZonesEditor>,
}

#[derive(Debug, Clone, Default)]
pub struct ZonesEditor {
    pub selected: usize,
    pub search: String,
}

impl Dialogs {
    pub fn any_open(&self) -> bool {
        self.settings.is_some()
            || self.preferences
            || self.machine
            || self.analysis
            || self.shortcuts
            || self.features
            || self.about
            || self.planned.is_some()
            || self.import_report.is_some()
            || self.zones.is_some()
    }
}

/// The profile editor: a draft copy, applied on OK or Apply.
pub struct SettingsEditor {
    pub name: String,
    pub draft: PrintSettings,
    /// The profile as it was when the editor opened, for change marks.
    pub original: PrintSettings,
    pub tab: Tab,
    pub search: String,
    pub error: Option<String>,
    pub only_changed: bool,
}

impl SettingsEditor {
    pub fn new(name: &str, settings: PrintSettings) -> Self {
        Self {
            name: name.to_string(),
            original: settings.clone(),
            draft: settings,
            tab: Tab::Layer,
            search: String::new(),
            error: None,
            only_changed: false,
        }
    }
}

pub fn show(app: &mut SliceCraftApp, ctx: &egui::Context) {
    settings_window(app, ctx);
    preferences_window(app, ctx);
    machine_window(app, ctx);
    analysis_window(app, ctx);
    shortcuts_window(app, ctx);
    features_window(app, ctx);
    about_window(app, ctx);
    planned_window(app, ctx);
    confirm_window(app, ctx);
    import_report_window(app, ctx);
    zones_window(app, ctx);
}

/// Setting zones: settings that change above a height, or for some models.
fn zones_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    use slicecraft_doc::SettingZone;
    use slicecraft_engine::Command;
    let Some(mut editor) = app.dialogs.zones.clone() else { return };
    let doc = app.session.document();
    let zones = doc.zones.clone();
    let names: Vec<String> = doc.objects.iter().map(|o| o.name.clone()).collect();
    let base = doc.settings.clone();
    let top = slicecraft_geom::MAX_COORD;
    let mut open = true;
    let mut command: Option<Command> = None;
    egui::Window::new("Setting zones").open(&mut open).default_width(640.0).show(ctx, |ui| {
        ui.label("Change settings above a height, or for some models only. Zones apply in order; later ones win.");
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(180.0);
                for (i, z) in zones.iter().enumerate() {
                    let range = if z.z_to >= top {
                        format!("from {:.1} mm", z.z_from)
                    } else {
                        format!("{:.1}–{:.1} mm", z.z_from, z.z_to)
                    };
                    let who = if z.objects.is_empty() { "all models".to_string() } else { format!("{} model(s)", z.objects.len()) };
                    if ui.selectable_label(editor.selected == i, format!("{}\n{range} · {who}", z.name)).clicked() {
                        editor.selected = i;
                    }
                }
                ui.add_space(6.0);
                if ui.button("+ Add zone").clicked() {
                    let mut z = SettingZone::new(format!("Zone {}", zones.len() + 1));
                    z.z_from = 0.0;
                    command = Some(Command::AddZone(Box::new(z)));
                    editor.selected = zones.len();
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let Some(zone) = zones.get(editor.selected) else {
                    ui.weak("Add a zone to change settings for part of the job.");
                    return;
                };
                let mut z = zone.clone();
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut z.name);
                });
                ui.horizontal(|ui| {
                    ui.label("From");
                    ui.add(egui::DragValue::new(&mut z.z_from).range(0.0..=10_000.0).speed(0.1).suffix(" mm"));
                    let mut to_top = z.z_to >= top;
                    ui.checkbox(&mut to_top, "to the top");
                    if to_top {
                        z.z_to = top;
                    } else {
                        if z.z_to >= top {
                            z.z_to = z.z_from + 10.0;
                        }
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut z.z_to).range(0.0..=10_000.0).speed(0.1).suffix(" mm"));
                    }
                });
                ui.label(RichText::new("Models").strong());
                if names.is_empty() {
                    ui.weak("No models yet.");
                }
                ui.horizontal_wrapped(|ui| {
                    for (i, n) in names.iter().enumerate() {
                        let mut on = z.objects.contains(&i);
                        if ui.checkbox(&mut on, n).changed() {
                            if on {
                                z.objects.push(i);
                                z.objects.sort_unstable();
                            } else {
                                z.objects.retain(|&k| k != i);
                            }
                        }
                    }
                });
                ui.weak(if z.objects.is_empty() {
                    "No model ticked: the zone applies to every model."
                } else {
                    "Only the ticked models; speeds and temperatures follow the layer."
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Settings").strong());
                let mut remove = None;
                egui::Grid::new("zone-settings").num_columns(3).show(ui, |ui| {
                    for (key, value) in z.overrides.clone() {
                        let Some(spec) = slicecraft_doc::settings::spec(&key) else { continue };
                        ui.label(spec.label);
                        if let Some(v) = setting_widget(ui, spec, &value) {
                            z.overrides.insert(key.clone(), v);
                        }
                        if ui.small_button("×").on_hover_text("Stop changing this setting").clicked() {
                            remove = Some(key.clone());
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = remove {
                    z.overrides.remove(&k);
                }
                ui.horizontal(|ui| {
                    ui.label("Add a setting:");
                    ui.text_edit_singleline(&mut editor.search);
                });
                let q = editor.search.to_lowercase();
                if !q.is_empty() {
                    egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                        for spec in SETTINGS.iter().filter(|s| s.label.to_lowercase().contains(&q) || s.key.contains(&q)).take(30) {
                            if ui.button(spec.label).clicked()
                                && let Some(v) = base.get(spec.key)
                            {
                                z.overrides.insert(spec.key.to_string(), v);
                                editor.search.clear();
                            }
                        }
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Remove zone").clicked() {
                        command = Some(Command::RemoveZone { index: editor.selected });
                        editor.selected = editor.selected.saturating_sub(1);
                    }
                });
                if command.is_none() && z != *zone && z.validate().is_ok() {
                    command = Some(Command::UpdateZone { index: editor.selected, zone: Box::new(z) });
                }
            });
        });
    });
    if let Some(c) = command {
        app.run(c);
    }
    app.dialogs.zones = if open { Some(editor) } else { None };
}

/// What a profile import carried over, converted or left out.
fn import_report_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let Some((name, report)) = app.dialogs.import_report.clone() else { return };
    let mut open = true;
    let mut edit = false;
    let mut close = false;
    egui::Window::new("Profile import")
        .open(&mut open)
        .collapsible(false)
        .default_width(460.0)
        .show(ctx, |ui| {
            ui.label(RichText::new(format!("\"{name}\"")).strong());
            ui.weak(format!("written by {}", report.written_by));
            ui.add_space(4.0);
            egui::Grid::new("import-counts").num_columns(2).show(ui, |ui| {
                ui.label("Used by the slicer");
                ui.label(report.applied.len().to_string());
                ui.end_row();
                ui.label("Kept, not used yet (planned)");
                ui.label(report.placeholders.len().to_string());
                ui.end_row();
                ui.label("No SliceCraft counterpart");
                ui.label(report.ignored.len().to_string());
                ui.end_row();
            });
            if !report.notes.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new("Notes").strong());
                for n in &report.notes {
                    ui.label(format!("• {n}"));
                }
            }
            ui.add_space(6.0);
            let label = |key: &str| {
                slicecraft_doc::settings::spec(key)
                    .map_or_else(|| key.to_string(), |s| s.label.to_string())
            };
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                ui.collapsing(format!("Used by the slicer ({})", report.applied.len()), |ui| {
                    ui.label(
                        report.applied.iter().map(|k| label(k)).collect::<Vec<_>>().join(", "),
                    );
                });
                ui.collapsing(format!("Kept as planned ({})", report.placeholders.len()), |ui| {
                    ui.label(
                        report.placeholders.iter().map(|k| label(k)).collect::<Vec<_>>().join(", "),
                    );
                });
                ui.collapsing(format!("Not carried over ({})", report.ignored.len()), |ui| {
                    ui.weak(report.ignored.join(", "));
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Review settings…").clicked() {
                    edit = true;
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });
    if !open || close {
        app.dialogs.import_report = None;
    }
    if edit {
        app.dialogs.import_report = None;
        app.open_settings();
    }
}

/// "Save your changes?" before an action that replaces the document.
fn confirm_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let Some(action) = app.confirm.clone() else { return };
    let mut choice = None;
    egui::Window::new("Unsaved changes")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(format!(
                "Save the changes to \"{}\" before {}?",
                app.project_name(),
                action.verb()
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(true);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(false);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    app.confirm = None;
                }
            });
        });
    if let Some(save) = choice {
        app.confirm = None;
        if !save || app.save_project(false) {
            app.perform(action);
        }
    }
}

/// One setting's editor widget. Returns a new value when it was changed.
fn setting_widget(ui: &mut Ui, spec: &SettingSpec, value: &SettingValue) -> Option<SettingValue> {
    match (spec.kind, value) {
        (SettingKind::Float { min, max, step, unit }, SettingValue::Float(v)) => {
            let mut x = *v;
            let suffix = if unit.is_empty() { String::new() } else { format!(" {unit}") };
            let r = ui.add(
                egui::DragValue::new(&mut x)
                    .range(min..=max)
                    .speed(step)
                    .max_decimals(4)
                    .suffix(suffix),
            );
            r.changed().then_some(SettingValue::Float(x))
        }
        (SettingKind::Int { min, max, unit }, SettingValue::Int(v)) => {
            let mut x = *v;
            let suffix = if unit.is_empty() { String::new() } else { format!(" {unit}") };
            let r = ui.add(egui::DragValue::new(&mut x).range(min..=max).suffix(suffix));
            r.changed().then_some(SettingValue::Int(x))
        }
        (SettingKind::Bool, SettingValue::Bool(v)) => {
            let mut x = *v;
            ui.checkbox(&mut x, "").changed().then_some(SettingValue::Bool(x))
        }
        (SettingKind::Choice(opts), SettingValue::Choice(i)) => {
            let mut x = *i;
            let shown = opts.get(x).copied().unwrap_or("?");
            egui::ComboBox::from_id_salt(spec.key).selected_text(shown).show_ui(ui, |ui| {
                for (k, o) in opts.iter().enumerate() {
                    ui.selectable_value(&mut x, k, *o);
                }
            });
            (x != *i).then_some(SettingValue::Choice(x))
        }
        (SettingKind::Text, SettingValue::Text(t)) => {
            let mut x = t.clone();
            let r = ui.add(
                egui::TextEdit::multiline(&mut x)
                    .code_editor()
                    .desired_rows(6)
                    .desired_width(f32::INFINITY),
            );
            r.changed().then_some(SettingValue::Text(x))
        }
        _ => {
            ui.colored_label(WARN, "unreadable value");
            None
        }
    }
}

fn setting_rows(ui: &mut Ui, ed: &mut SettingsEditor, specs: &[&'static SettingSpec]) {
    // keep each section together, in the order sections first appear
    let mut order: Vec<&str> = Vec::new();
    for s in specs {
        if !order.contains(&s.section) {
            order.push(s.section);
        }
    }
    let mut specs: Vec<&'static SettingSpec> = specs.to_vec();
    specs.sort_by_key(|s| order.iter().position(|o| *o == s.section).unwrap_or(usize::MAX));
    let mut section = "";
    for spec in specs {
        if spec.section != section {
            section = spec.section;
            ui.add_space(6.0);
            ui.label(RichText::new(section).strong());
            ui.separator();
        }
        let Some(value) = ed.draft.get(spec.key) else { continue };
        let changed = ed.original.get(spec.key).as_ref() != Some(&value);
        let is_default = value == spec.default.value();
        let text_kind = matches!(spec.kind, SettingKind::Text);
        ui.horizontal(|ui| {
            let mut label = RichText::new(spec.label);
            if changed {
                label = label.color(CHANGED);
            }
            let help = format!("{}\n\nkey: {}", spec.help, spec.key);
            ui.allocate_ui_with_layout(
                egui::vec2(210.0, 18.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(210.0);
                    ui.add(egui::Label::new(label).truncate()).on_hover_text(help);
                },
            );
            if !text_kind && let Some(v) = setting_widget(ui, spec, &value) {
                match ed.draft.set(spec.key, v) {
                    Ok(()) => ed.error = None,
                    Err(e) => ed.error = Some(e.to_string()),
                }
            }
            if !spec.implemented() {
                ui.label(RichText::new("planned").small().color(PLANNED))
                    .on_hover_text("Saved with the profile, not used by the slicer yet");
            }
            if !is_default
                && ui.small_button("↺").on_hover_text("Back to the default").clicked()
                && let Err(e) = ed.draft.set(spec.key, spec.default.value())
            {
                ed.error = Some(e.to_string());
            }
        });
        if text_kind
            && let Some(v) = setting_widget(ui, spec, &value)
            && let Err(e) = ed.draft.set(spec.key, v)
        {
            ed.error = Some(e.to_string());
        }
    }
}

fn settings_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let Some(mut ed) = app.dialogs.settings.take() else { return };
    let mut open = true;
    let mut apply = false;
    let mut close = false;
    let mut load = false;
    let mut save = false;
    egui::Window::new("Print settings")
        .open(&mut open)
        .default_size([820.0, 620.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Profile");
                ui.add(egui::TextEdit::singleline(&mut ed.name).desired_width(220.0));
                egui::ComboBox::from_id_salt("start-from")
                    .selected_text("Start from preset…")
                    .show_ui(ui, |ui| {
                        for p in presets() {
                            if ui.selectable_label(false, &p.name).clicked() {
                                ed.name = p.name.clone();
                                ed.draft = p.settings;
                            }
                        }
                    });
                ui.separator();
                ui.label("Find");
                ui.add(
                    egui::TextEdit::singleline(&mut ed.search)
                        .hint_text("setting name")
                        .desired_width(170.0),
                );
                ui.checkbox(&mut ed.only_changed, "changed only");
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for t in Tab::ALL {
                    let planned = tab_settings(t).all(|s| !s.implemented());
                    let mut text = RichText::new(t.label());
                    if planned {
                        text = text.color(PLANNED);
                    }
                    if ui.selectable_label(ed.tab == t && ed.search.is_empty(), text).clicked() {
                        ed.tab = t;
                        ed.search.clear();
                    }
                }
            });
            ui.separator();
            let query = ed.search.trim().to_lowercase();
            let specs: Vec<&'static SettingSpec> = if query.is_empty() {
                tab_settings(ed.tab).collect()
            } else {
                SETTINGS
                    .iter()
                    .filter(|s| s.label.to_lowercase().contains(&query) || s.key.contains(&query))
                    .collect()
            };
            let specs: Vec<&'static SettingSpec> = if ed.only_changed {
                specs
                    .into_iter()
                    .filter(|s| ed.draft.get(s.key) != Some(s.default.value()))
                    .collect()
            } else {
                specs
            };
            let footer = 70.0;
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - footer)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if specs.is_empty() {
                        ui.weak("No settings match.");
                    }
                    if !query.is_empty() {
                        // group search results by tab
                        for t in Tab::ALL {
                            let in_tab: Vec<&'static SettingSpec> =
                                specs.iter().copied().filter(|s| s.tab == t).collect();
                            if !in_tab.is_empty() {
                                ui.label(RichText::new(t.label()).heading().size(15.0));
                                setting_rows(ui, &mut ed, &in_tab);
                                ui.add_space(8.0);
                            }
                        }
                    } else {
                        setting_rows(ui, &mut ed, &specs);
                    }
                });
            ui.separator();
            if let Some(e) = &ed.error {
                ui.colored_label(WARN, e);
            } else {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("blue").color(CHANGED));
                    ui.weak("= changed since opening ·");
                    ui.label(RichText::new("planned").color(PLANNED));
                    ui.weak("= kept in the profile, not used by the slicer yet");
                });
            }
            ui.horizontal(|ui| {
                if ui.button("Restore defaults").clicked() {
                    ed.draft = PrintSettings::default();
                }
                if ui.button("Load…").clicked() {
                    load = true;
                }
                if ui.button("Save as…").clicked() {
                    save = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("OK").clicked() {
                        apply = true;
                        close = true;
                    }
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        });
    if apply {
        match ed.draft.validate() {
            Ok(()) => {
                if app.apply_profile(ed.draft.clone(), Some(ed.name.clone())) {
                    ed.original = ed.draft.clone();
                } else {
                    close = false;
                }
            }
            Err(e) => {
                ed.error = Some(e.to_string());
                close = false;
            }
        }
    }
    if save {
        // save what is in the editor: apply first so the saved file matches
        if app.apply_profile(ed.draft.clone(), Some(ed.name.clone())) {
            ed.original = ed.draft.clone();
            app.save_profile_dialog();
        }
    }
    if load {
        app.load_profile_dialog();
        let s = app.session.document().settings.clone();
        ed = SettingsEditor::new(app.session.profile_name(), s);
    }
    if open && !close {
        app.dialogs.settings = Some(ed);
    }
}

fn preferences_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let mut open = app.dialogs.preferences;
    egui::Window::new("Preferences").open(&mut open).resizable(false).collapsible(false).show(
        ctx,
        |ui| {
            let p = &mut app.prefs;
            ui.label(RichText::new("Display").strong());
            ui.checkbox(&mut p.dark, "Dark theme");
            ui.checkbox(&mut p.bed.grid, "Bed grid");
            ui.checkbox(&mut p.bed.volume, "Build volume outline");
            ui.checkbox(&mut p.bed.axes, "Axes at the origin");
            ui.add_space(6.0);
            ui.label(RichText::new("Camera").strong());
            ui.checkbox(&mut p.invert_zoom, "Invert wheel zoom");
            ui.add(egui::Slider::new(&mut p.orbit_sensitivity, 0.2..=3.0).text("orbit speed"));
            ui.add_space(6.0);
            ui.label(RichText::new("Workflow").strong());
            ui.checkbox(&mut p.preview_after_slice, "Show the toolpaths after slicing");
            ui.checkbox(&mut p.fit_on_import, "Fit the view to models after import");
            ui.add_space(6.0);
            match crate::config::config_dir() {
                Some(dir) => ui.weak(format!("Remembered in {}", dir.display())),
                None => ui.weak("This system gives no settings folder; preferences last until the window closes."),
            };
            if ui.button("Restore default preferences").clicked() {
                app.prefs = crate::app::Preferences::default();
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Planned").strong().color(PLANNED));
            ui.add_enabled_ui(false, |ui| {
                let mut f = false;
                ui.checkbox(&mut f, "Ask to scale models that look like inches");
                ui.checkbox(&mut f, "Rotate or scale every imported model by a fixed amount");
                ui.checkbox(&mut f, "Orthographic camera");
                ui.checkbox(&mut f, "Interface language");
                ui.checkbox(&mut f, "Keyboard shortcut editor");
            });
        },
    );
    if app.dialogs.preferences && !open {
        app.persist();
    }
    app.dialogs.preferences = open;
}

fn machine_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let mut open = app.dialogs.machine;
    egui::Window::new("Machine control")
        .open(&mut open)
        .default_width(460.0)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.colored_label(
                PLANNED,
                "Printer connection is planned. Every control here is a placeholder.",
            );
            ui.add_space(4.0);
            ui.add_enabled_ui(false, |ui| {
                ui.label(RichText::new("Connection").strong());
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("port").selected_text("port").show_ui(ui, |_| {});
                    egui::ComboBox::from_id_salt("baud")
                        .selected_text("115200")
                        .show_ui(ui, |_| {});
                    let _ = ui.button("Connect");
                    let _ = ui.button("Find network printers");
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Move").strong());
                egui::Grid::new("jog").show(ui, |ui| {
                    for row in [["", "Y+", ""], ["X−", "Home", "X+"], ["", "Y−", ""]] {
                        for c in row {
                            if c.is_empty() {
                                ui.label("");
                            } else {
                                let _ = ui.button(c);
                            }
                        }
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    let _ = ui.button("Z+");
                    let _ = ui.button("Z−");
                    let _ = ui.button("Extrude");
                    let _ = ui.button("Retract");
                    let _ = ui.button("Motors off");
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Temperatures").strong());
                let mut t = 0.0f64;
                ui.horizontal(|ui| {
                    ui.label("Nozzle");
                    ui.add(egui::DragValue::new(&mut t).suffix(" °C"));
                    ui.label("Bed");
                    ui.add(egui::DragValue::new(&mut t).suffix(" °C"));
                    let _ = ui.button("Set");
                    let _ = ui.button("Cool down");
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Terminal").strong());
                let mut s = String::new();
                ui.add(egui::TextEdit::multiline(&mut s).desired_rows(4).hint_text("firmware log"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut s).hint_text("G-code to send"));
                    let _ = ui.button("Send");
                });
                ui.horizontal(|ui| {
                    let _ = ui.button("Print");
                    let _ = ui.button("Pause");
                    let _ = ui.button("Stop");
                });
            });
        });
    app.dialogs.machine = open;
}

fn analysis_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let mut open = app.dialogs.analysis;
    let names: Vec<String> =
        app.session.document().objects.iter().map(|o| o.name.clone()).collect();
    egui::Window::new("Mesh analysis").open(&mut open).collapsible(false).default_width(560.0).show(ctx, |ui| {
        let Some((_, geo)) = &app.geometry else { return };
        if geo.objects.is_empty() {
            ui.weak("No models.");
            return;
        }
        egui::Grid::new("analysis").striped(true).num_columns(8).show(ui, |ui| {
            for h in ["Model", "Triangles", "Volume", "Area", "Open", "Non-manifold", "Degenerate", "Status"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (name, g) in names.iter().zip(&geo.objects) {
                let r = g.report;
                ui.label(name);
                ui.label(r.triangles.to_string());
                ui.label(format!("{:.2} cm³", r.volume / 1000.0));
                ui.label(format!("{:.1} cm²", r.area / 100.0));
                ui.label(r.open_edges.to_string());
                ui.label(r.non_manifold_edges.to_string());
                ui.label(r.degenerate_triangles.to_string());
                if r.is_watertight() {
                    ui.label("ok");
                } else if r.volume < 0.0 {
                    ui.colored_label(WARN, "inside out");
                } else {
                    ui.colored_label(WARN, "needs repair");
                }
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        ui.weak("Open edges are holes; non-manifold edges join more than two faces. Repair tools are planned.");
    });
    app.dialogs.analysis = open;
}

fn shortcuts_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let mut open = app.dialogs.shortcuts;
    egui::Window::new("Keyboard shortcuts").open(&mut open).collapsible(false).show(ctx, |ui| {
        egui::Grid::new("keys").striped(true).show(ui, |ui| {
            for (k, v) in [
                ("Ctrl+O", "Import models"),
                ("Ctrl+R", "Slice"),
                ("Ctrl+E", "Export G-code"),
                ("Ctrl+P", "Print settings"),
                ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
                ("Ctrl+A", "Select all"),
                ("Ctrl+D", "Duplicate"),
                ("Del", "Remove selection"),
                ("Q W E R", "Select, move, rotate, scale tools"),
                ("0 – 6", "Perspective, front, back, left, right, top, bottom view"),
                ("F / double-click", "Fit the models in view"),
                ("Esc", "Clear selection / leave the preview"),
                ("Up / Down, Home / End", "Layer in the preview"),
                ("Space", "Play / pause the preview"),
                ("Shift-drag", "Move a model (select tool); snap rotation (rotate tool)"),
                ("Right or middle drag", "Pan"),
                ("Wheel", "Zoom"),
            ] {
                ui.label(RichText::new(k).monospace());
                ui.label(v);
                ui.end_row();
            }
        });
    });
    app.dialogs.shortcuts = open;
}

fn features_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let mut open = app.dialogs.features;
    egui::Window::new("Feature status")
        .open(&mut open)
        .collapsible(false)
        .default_size([520.0, 520.0])
        .show(ctx, |ui| {
            let ready = COMMANDS.iter().filter(|c| c.state == CommandState::Ready).count();
            let bound = SETTINGS.iter().filter(|s| s.implemented()).count();
            ui.label(format!(
                "{ready} of {} commands work today; {bound} of {} settings drive the slicer.",
                COMMANDS.len(),
                SETTINGS.len()
            ));
            ui.weak("The full inventory and plan are in docs/research/02-feature-inventory.md.");
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.columns(2, |cols| {
                    if let Some(ui) = cols.get_mut(0) {
                        ui.label(RichText::new("Commands").strong());
                        for c in COMMANDS {
                            let (mark, color) = match c.state {
                                CommandState::Ready => ("✔", Color32::from_rgb(0x6a, 0xc4, 0x6a)),
                                CommandState::Planned => ("…", PLANNED),
                            };
                            ui.horizontal(|ui| {
                                ui.colored_label(color, mark);
                                ui.label(c.label);
                            });
                        }
                    }
                    if let Some(ui) = cols.get_mut(1) {
                        ui.label(RichText::new("Settings by tab").strong());
                        for t in Tab::ALL {
                            let all = tab_settings(t).count();
                            let on = tab_settings(t).filter(|s| s.implemented()).count();
                            ui.label(format!("{}: {on} of {all} in use", t.label()));
                        }
                    }
                });
            });
        });
    app.dialogs.features = open;
}

fn about_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let mut open = app.dialogs.about;
    egui::Window::new("About SliceCraft").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.heading("SliceCraft");
        ui.label(format!("version {} · pre-alpha", env!("CARGO_PKG_VERSION")));
        ui.label("An open-source, clean-room 3D print slicer in pure Rust.");
        ui.label("Licence: MIT OR Apache-2.0.");
        ui.add_space(4.0);
        ui.weak("Written from public specifications and observed behaviour; no code, profiles, strings or artwork from other slicers.");
    });
    app.dialogs.about = open;
}

fn planned_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let Some(spec) = app.dialogs.planned else { return };
    let mut open = true;
    egui::Window::new(spec.label).open(&mut open).collapsible(false).resizable(false).show(
        ctx,
        |ui| {
            ui.colored_label(PLANNED, "Not implemented yet.");
            ui.label("This command is on the plan; the menu shows it so nothing is hidden.");
            ui.weak(format!("command id: {}", spec.id));
            ui.weak("See docs/research/02-feature-inventory.md for the plan.");
        },
    );
    if !open {
        app.dialogs.planned = None;
    }
}
