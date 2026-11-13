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
    /// The shortcut being changed: the next key pressed becomes its binding.
    pub capture: Option<crate::keys::Action>,
    /// A profile compared with the current one: its name and the differences.
    pub compare: Option<(String, Vec<slicecraft_engine::ProfileDiff>)>,
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
    crate::network::network_window(app, ctx);
    crate::network::camera_window(app, ctx);
    crate::wizard::show(app, ctx);
    compare_window(app, ctx);
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
    egui::Window::new(crate::i18n::tr("Setting zones")).open(&mut open).default_width(640.0).show(ctx, |ui| {
        ui.label(crate::i18n::tr("Change settings above a height, or for some models only. Zones apply in order; later ones win."));
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
                if ui.button(crate::i18n::tr("+ Add zone")).clicked() {
                    let mut z = SettingZone::new(format!("Zone {}", zones.len() + 1));
                    z.z_from = 0.0;
                    command = Some(Command::AddZone(Box::new(z)));
                    editor.selected = zones.len();
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let Some(zone) = zones.get(editor.selected) else {
                    ui.weak(crate::i18n::tr("Add a zone to change settings for part of the job."));
                    return;
                };
                let mut z = zone.clone();
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Name"));
                    ui.text_edit_singleline(&mut z.name);
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("From"));
                    ui.add(egui::DragValue::new(&mut z.z_from).range(0.0..=10_000.0).speed(0.1).suffix(" mm"));
                    let mut to_top = z.z_to >= top;
                    ui.checkbox(&mut to_top, crate::i18n::tr("to the top"));
                    if to_top {
                        z.z_to = top;
                    } else {
                        if z.z_to >= top {
                            z.z_to = z.z_from + 10.0;
                        }
                        ui.label(crate::i18n::tr("to"));
                        ui.add(egui::DragValue::new(&mut z.z_to).range(0.0..=10_000.0).speed(0.1).suffix(" mm"));
                    }
                });
                ui.label(RichText::new(crate::i18n::tr("Models")).strong());
                if names.is_empty() {
                    ui.weak(crate::i18n::tr("No models yet."));
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
                ui.label(RichText::new(crate::i18n::tr("Settings")).strong());
                let mut remove = None;
                egui::Grid::new("zone-settings").num_columns(3).show(ui, |ui| {
                    for (key, value) in z.overrides.clone() {
                        let Some(spec) = slicecraft_doc::settings::spec(&key) else { continue };
                        ui.label(spec.label);
                        if let Some(v) = setting_widget(ui, spec, &value) {
                            z.overrides.insert(key.clone(), v);
                        }
                        if ui.small_button(crate::i18n::tr("×")).on_hover_text(crate::i18n::tr("Stop changing this setting")).clicked() {
                            remove = Some(key.clone());
                        }
                        ui.end_row();
                    }
                });
                if let Some(k) = remove {
                    z.overrides.remove(&k);
                }
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Add a setting:"));
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
                    if ui.button(crate::i18n::tr("Remove zone")).clicked() {
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
    egui::Window::new(crate::i18n::tr("Profile import"))
        .open(&mut open)
        .collapsible(false)
        .default_width(460.0)
        .show(ctx, |ui| {
            ui.label(RichText::new(format!("\"{name}\"")).strong());
            ui.weak(format!("written by {}", report.written_by));
            ui.add_space(4.0);
            egui::Grid::new("import-counts").num_columns(2).show(ui, |ui| {
                ui.label(crate::i18n::tr("Used by the slicer"));
                ui.label(report.applied.len().to_string());
                ui.end_row();
                ui.label(crate::i18n::tr("Kept, not used yet (planned)"));
                ui.label(report.placeholders.len().to_string());
                ui.end_row();
                ui.label(crate::i18n::tr("No SliceCraft counterpart"));
                ui.label(report.ignored.len().to_string());
                ui.end_row();
            });
            if !report.notes.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(crate::i18n::tr("Notes")).strong());
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
                if ui.button(crate::i18n::tr("Review settings…")).clicked() {
                    edit = true;
                }
                if ui.button(crate::i18n::tr("Close")).clicked() {
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
    egui::Window::new(crate::i18n::tr("Unsaved changes"))
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
                if ui.button(crate::i18n::tr("Save")).clicked() {
                    choice = Some(true);
                }
                if ui.button(crate::i18n::tr("Don't save")).clicked() {
                    choice = Some(false);
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
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
            let shown = crate::i18n::tr(opts.get(x).copied().unwrap_or("?"));
            egui::ComboBox::from_id_salt(spec.key).selected_text(shown).show_ui(ui, |ui| {
                for (k, o) in opts.iter().enumerate() {
                    ui.selectable_value(&mut x, k, crate::i18n::tr(o));
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
            ui.label(RichText::new(crate::i18n::tr(section)).strong());
            ui.separator();
        }
        let Some(value) = ed.draft.get(spec.key) else { continue };
        let changed = ed.original.get(spec.key).as_ref() != Some(&value);
        let is_default = value == spec.default.value();
        let text_kind = matches!(spec.kind, SettingKind::Text);
        ui.horizontal(|ui| {
            let mut label = RichText::new(crate::i18n::tr(spec.label));
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
                ui.label(RichText::new(crate::i18n::tr("planned")).small().color(PLANNED))
                    .on_hover_text(crate::i18n::tr(
                        "Saved with the profile, not used by the slicer yet",
                    ));
            }
            if !is_default
                && ui
                    .small_button(crate::i18n::tr("↺"))
                    .on_hover_text(crate::i18n::tr("Back to the default"))
                    .clicked()
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
    egui::Window::new(crate::i18n::tr("Print settings"))
        .open(&mut open)
        .default_size([820.0, 620.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Profile"));
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
                ui.label(crate::i18n::tr("Find"));
                ui.add(
                    egui::TextEdit::singleline(&mut ed.search)
                        .hint_text("setting name")
                        .desired_width(170.0),
                );
                ui.checkbox(&mut ed.only_changed, crate::i18n::tr("changed only"));
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for t in Tab::ALL {
                    let planned = tab_settings(t).all(|s| !s.implemented());
                    let mut text = RichText::new(crate::i18n::tr(t.label()));
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
                        ui.weak(crate::i18n::tr("No settings match."));
                    }
                    if !query.is_empty() {
                        // group search results by tab
                        for t in Tab::ALL {
                            let in_tab: Vec<&'static SettingSpec> =
                                specs.iter().copied().filter(|s| s.tab == t).collect();
                            if !in_tab.is_empty() {
                                ui.label(
                                    RichText::new(crate::i18n::tr(t.label())).heading().size(15.0),
                                );
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
                    ui.label(RichText::new(crate::i18n::tr("blue")).color(CHANGED));
                    ui.weak(crate::i18n::tr("= changed since opening ·"));
                    ui.label(RichText::new(crate::i18n::tr("planned")).color(PLANNED));
                    ui.weak(crate::i18n::tr("= kept in the profile, not used by the slicer yet"));
                });
            }
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Restore defaults")).clicked() {
                    ed.draft = PrintSettings::default();
                }
                if ui.button(crate::i18n::tr("Load…")).clicked() {
                    load = true;
                }
                if ui.button(crate::i18n::tr("Save as…")).clicked() {
                    save = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("OK")).clicked() {
                        apply = true;
                        close = true;
                    }
                    if ui.button(crate::i18n::tr("Apply")).clicked() {
                        apply = true;
                    }
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
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
    egui::Window::new(crate::i18n::tr("Preferences")).open(&mut open).resizable(false).collapsible(false).show(
        ctx,
        |ui| {
            let p = &mut app.prefs;
            ui.label(RichText::new(crate::i18n::tr("Display")).strong());
            ui.checkbox(&mut p.dark, crate::i18n::tr("Dark theme"));
            ui.checkbox(&mut p.bed.grid, crate::i18n::tr("Bed grid"));
            ui.checkbox(&mut p.bed.volume, crate::i18n::tr("Build volume outline"));
            ui.checkbox(&mut p.bed.axes, crate::i18n::tr("Axes at the origin"));
            ui.add_space(6.0);
            ui.label(RichText::new(crate::i18n::tr("Camera")).strong());
            ui.checkbox(&mut p.invert_zoom, crate::i18n::tr("Invert wheel zoom"));
            ui.add(egui::Slider::new(&mut p.orbit_sensitivity, 0.2..=3.0).text("orbit speed"));
            ui.add_space(6.0);
            ui.label(RichText::new(crate::i18n::tr("Workflow")).strong());
            ui.checkbox(&mut p.preview_after_slice, crate::i18n::tr("Show the toolpaths after slicing"));
            ui.checkbox(&mut p.fit_on_import, crate::i18n::tr("Fit the view to models after import"));
            ui.add_space(6.0);
            match crate::config::config_dir() {
                Some(dir) => ui.weak(format!("Remembered in {}", dir.display())),
                None => ui.weak(crate::i18n::tr("This system gives no settings folder; preferences last until the window closes.")),
            };
            if ui.button(crate::i18n::tr("Restore default preferences")).clicked() {
                app.prefs = crate::app::Preferences::default();
            }
            ui.add_space(6.0);
            ui.label(RichText::new(crate::i18n::tr("Import")).strong());
            ui.checkbox(&mut app.prefs.inch_check, crate::i18n::tr("Scale models under 10 mm across by 25.4 (inches)"));
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Turn every import by"));
                for (axis, v) in ["X", "Y", "Z"].iter().zip(app.prefs.import_rotate.iter_mut()) {
                    ui.add(egui::DragValue::new(v).range(-360.0..=360.0).speed(1.0).prefix(format!("{axis} ")).suffix("°"));
                }
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("and scale it by"));
                ui.add(egui::DragValue::new(&mut app.prefs.import_scale).range(0.001..=1000.0).speed(0.01));
            });
            ui.add_space(6.0);
            ui.label(RichText::new(crate::i18n::tr("View")).strong());
            ui.checkbox(&mut app.prefs.ortho, crate::i18n::tr("Orthographic camera"));
            ui.checkbox(&mut app.prefs.wireframe, crate::i18n::tr("Draw models as wireframes"));
            if ui.button(crate::i18n::tr("Keyboard shortcuts…")).clicked() {
                app.dialogs.shortcuts = true;
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Language"));
                let current = app.prefs.language;
                egui::ComboBox::from_id_salt("language").selected_text(current.label()).show_ui(ui, |ui| {
                    for l in crate::i18n::Language::ALL {
                        ui.selectable_value(&mut app.prefs.language, l, l.label());
                    }
                });
                crate::i18n::set_language(app.prefs.language);
            });
            ui.weak(crate::i18n::tr("Menus, panels and setting names follow the language; help text is in English."));
        },
    );
    if app.dialogs.preferences && !open {
        app.persist();
    }
    app.dialogs.preferences = open;
}

fn machine_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    use slicecraft_engine::Command;
    let mut open = app.dialogs.machine;
    let connected = app.session.printer_port().map(str::to_string);
    let printers = app.session.printers();
    let mut run: Vec<Command> = Vec::new();
    egui::Window::new(crate::i18n::tr("Machine control"))
        .open(&mut open)
        .default_width(520.0)
        .show(ctx, |ui| {
            let m = &mut app.machine;
            if !m.scanned {
                m.scanned = true;
                m.ports = slicecraft_io::printer::list_ports();
                if m.port.is_empty()
                    && let [only] = m.ports.as_slice()
                {
                    m.port = only.clone();
                }
            }
            ui.label(RichText::new(crate::i18n::tr("Connection")).strong());
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("port")
                    .selected_text(if m.port.is_empty() { "port" } else { m.port.as_str() })
                    .show_ui(ui, |ui| {
                        for p in &m.ports {
                            ui.selectable_value(&mut m.port, p.clone(), p);
                        }
                    });
                if ui
                    .small_button(crate::i18n::tr("↻"))
                    .on_hover_text(crate::i18n::tr("Look for serial ports again"))
                    .clicked()
                {
                    m.ports = slicecraft_io::printer::list_ports();
                }
                egui::ComboBox::from_id_salt("baud").selected_text(m.baud.to_string()).show_ui(
                    ui,
                    |ui| {
                        for b in [250_000u32, 115_200, 57_600, 38_400, 19_200] {
                            ui.selectable_value(&mut m.baud, b, b.to_string());
                        }
                    },
                );
                match &connected {
                    Some(p) => {
                        ui.colored_label(
                            Color32::from_rgb(0x4c, 0xc0, 0x6a),
                            format!("connected · {p}"),
                        );
                        if ui.button(crate::i18n::tr("Disconnect")).clicked() {
                            run.push(Command::DisconnectPrinter);
                        }
                        let other = !m.port.is_empty() && printers.iter().all(|x| x.port != m.port);
                        if ui
                            .add_enabled(
                                other,
                                egui::Button::new(crate::i18n::tr("Connect another")),
                            )
                            .on_hover_text(crate::i18n::tr(
                                "This printer keeps printing in the background",
                            ))
                            .clicked()
                        {
                            run.push(Command::ConnectPrinter {
                                port: m.port.clone(),
                                baud: m.baud,
                            });
                        }
                    }
                    None => {
                        if ui
                            .add_enabled(
                                !m.port.is_empty(),
                                egui::Button::new(crate::i18n::tr("Connect")),
                            )
                            .clicked()
                        {
                            run.push(Command::ConnectPrinter {
                                port: m.port.clone(),
                                baud: m.baud,
                            });
                        }
                    }
                }
            });
            if printers.len() > 1 {
                ui.add_space(4.0);
                ui.label(RichText::new(crate::i18n::tr("Printers")).strong());
                egui::Grid::new("printers").striped(true).num_columns(3).show(ui, |ui| {
                    for pr in &printers {
                        ui.label(&pr.port);
                        match &pr.status {
                            None => {
                                ui.label(crate::i18n::tr("controlled here"));
                            }
                            Some(st) => {
                                let what = match (st.progress, st.finished, &st.error) {
                                    (_, _, Some(e)) => format!("error: {e}"),
                                    (Some((d, n)), _, _) => {
                                        format!(
                                            "printing {:.0} %",
                                            100.0 * d as f64 / n.max(1) as f64
                                        )
                                    }
                                    (None, true, _) => "finished".to_string(),
                                    _ => "idle".to_string(),
                                };
                                ui.label(format!(
                                    "{what} · {:.0} °C / {:.0} °C",
                                    st.nozzle, st.bed
                                ));
                            }
                        }
                        if !pr.active && ui.small_button(crate::i18n::tr("Control")).clicked() {
                            run.push(Command::SwitchPrinter(pr.port.clone()));
                        }
                        ui.end_row();
                    }
                });
            }
            let on = connected.is_some();
            ui.add_enabled_ui(on, |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(crate::i18n::tr("Temperatures")).strong());
                egui::Grid::new("temps").num_columns(4).show(ui, |ui| {
                    ui.label(crate::i18n::tr("Nozzle"));
                    ui.label(format!("{:.1} / {:.0} °C", m.nozzle.0, m.nozzle.1));
                    ui.add(
                        egui::DragValue::new(&mut m.set_nozzle).range(0.0..=320.0).suffix(" °C"),
                    );
                    if ui.button(crate::i18n::tr("Set")).clicked() {
                        run.push(Command::SendToPrinter(format!("M104 S{:.0}", m.set_nozzle)));
                    }
                    ui.end_row();
                    ui.label(crate::i18n::tr("Bed"));
                    ui.label(format!("{:.1} / {:.0} °C", m.bed.0, m.bed.1));
                    ui.add(egui::DragValue::new(&mut m.set_bed).range(0.0..=130.0).suffix(" °C"));
                    if ui.button(crate::i18n::tr("Set")).clicked() {
                        run.push(Command::SendToPrinter(format!("M140 S{:.0}", m.set_bed)));
                    }
                    ui.end_row();
                });
                if ui.button(crate::i18n::tr("Heaters off")).clicked() {
                    run.push(Command::SendToPrinter("M104 S0".into()));
                    run.push(Command::SendToPrinter("M140 S0".into()));
                }
                ui.add_space(6.0);
                ui.label(RichText::new(crate::i18n::tr("Move")).strong());
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Step"));
                    for d in [0.1, 1.0, 10.0, 50.0] {
                        ui.selectable_value(&mut m.step, d, format!("{d} mm"));
                    }
                });
                let step = m.step;
                ui.horizontal(|ui| {
                    for (label, axis, sign) in [
                        ("X−", "X", -1.0),
                        ("X+", "X", 1.0),
                        ("Y−", "Y", -1.0),
                        ("Y+", "Y", 1.0),
                        ("Z−", "Z", -1.0),
                        ("Z+", "Z", 1.0),
                    ] {
                        if ui.button(label).clicked() {
                            for l in [
                                "G91".to_string(),
                                format!("G1 {axis}{:.2} F3000", sign * step),
                                "G90".to_string(),
                            ] {
                                run.push(Command::SendToPrinter(l));
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Home all")).clicked() {
                        run.push(Command::SendToPrinter("G28".into()));
                    }
                    if ui.button(crate::i18n::tr("Home XY")).clicked() {
                        run.push(Command::SendToPrinter("G28 X Y".into()));
                    }
                    if ui.button(crate::i18n::tr("Extrude 5 mm")).clicked() {
                        for l in ["M83", "G1 E5 F300", "M82"] {
                            run.push(Command::SendToPrinter(l.into()));
                        }
                    }
                    if ui.button(crate::i18n::tr("Retract 5 mm")).clicked() {
                        for l in ["M83", "G1 E-5 F1800", "M82"] {
                            run.push(Command::SendToPrinter(l.into()));
                        }
                    }
                    if ui.button(crate::i18n::tr("Fan on")).clicked() {
                        run.push(Command::SendToPrinter("M106 S255".into()));
                    }
                    if ui.button(crate::i18n::tr("Fan off")).clicked() {
                        run.push(Command::SendToPrinter("M107".into()));
                    }
                    if ui.button(crate::i18n::tr("Motors off")).clicked() {
                        run.push(Command::SendToPrinter("M84".into()));
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new(crate::i18n::tr("Print")).strong());
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Print the current slice")).clicked() {
                        run.push(Command::PrintOverUsb);
                    }
                    if ui.button(crate::i18n::tr("Pause")).clicked() {
                        run.push(Command::PausePrint);
                    }
                    if ui.button(crate::i18n::tr("Resume")).clicked() {
                        run.push(Command::ResumePrint);
                    }
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        run.push(Command::CancelPrint);
                    }
                });
                if let Some((done, total)) = m.progress {
                    let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                    ui.add(egui::ProgressBar::new(frac).text(format!("{done} / {total} lines")));
                }
                ui.add_space(6.0);
                ui.label(RichText::new(crate::i18n::tr("Terminal")).strong());
                egui::ScrollArea::vertical().max_height(160.0).stick_to_bottom(true).show(
                    ui,
                    |ui| {
                        for l in &m.log {
                            ui.monospace(l);
                        }
                    },
                );
                ui.horizontal(|ui| {
                    let r = ui.text_edit_singleline(&mut m.input);
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui.button(crate::i18n::tr("Send")).clicked() || enter)
                        && !m.input.trim().is_empty()
                    {
                        run.push(Command::SendToPrinter(std::mem::take(&mut m.input)));
                    }
                });
            });
        });
    for c in run {
        let switch = match &c {
            Command::SwitchPrinter(p) | Command::ConnectPrinter { port: p, .. } => Some(p.clone()),
            _ => None,
        };
        if app.run(c).is_some()
            && let Some(p) = switch
        {
            // the controls now follow another printer
            app.machine.progress = None;
            app.machine.log.push_back(format!("— controlling {p} —"));
        }
    }
    app.dialogs.machine = open;
}

fn analysis_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let mut open = app.dialogs.analysis;
    let names: Vec<String> =
        app.session.document().objects.iter().map(|o| o.name.clone()).collect();
    egui::Window::new(crate::i18n::tr("Mesh analysis")).open(&mut open).collapsible(false).default_width(560.0).show(ctx, |ui| {
        let Some((_, geo)) = &app.geometry else { return };
        if geo.objects.is_empty() {
            ui.weak(crate::i18n::tr("No models."));
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
                    ui.label(crate::i18n::tr("ok"));
                } else if r.volume < 0.0 {
                    ui.colored_label(WARN, "inside out");
                } else {
                    ui.colored_label(WARN, "needs repair");
                }
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        ui.weak(crate::i18n::tr("Open edges are holes; non-manifold edges join more than two faces. Model ▸ Repair fixes most of them."));
    });
    app.dialogs.analysis = open;
}

fn compare_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let Some((name, diffs)) = app.dialogs.compare.clone() else { return };
    let mut open = true;
    let mut take: Vec<(&'static str, slicecraft_doc::SettingValue)> = Vec::new();
    egui::Window::new(format!("Compare with {name}"))
        .open(&mut open)
        .default_size([620.0, 480.0])
        .show(ctx, |ui| {
            if diffs.is_empty() {
                ui.label(crate::i18n::tr("The profiles are the same."));
                return;
            }
            ui.label(format!(
                "{} setting(s) differ. Take a value to copy it into the current profile.",
                diffs.len()
            ));
            if ui.button(crate::i18n::tr("Take all of theirs")).clicked() {
                take.extend(diffs.iter().map(|d| (d.key, d.theirs.clone())));
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("compare").striped(true).num_columns(4).show(ui, |ui| {
                    ui.strong(crate::i18n::tr("Setting"));
                    ui.strong(crate::i18n::tr("Current"));
                    ui.strong(&name);
                    ui.label("");
                    ui.end_row();
                    for d in &diffs {
                        let Some(spec) = slicecraft_doc::settings::spec(d.key) else { continue };
                        let f = |v: &slicecraft_doc::SettingValue| {
                            let t = slicecraft_doc::profile::format_value(spec.kind, v);
                            if t.chars().count() > 40 {
                                format!("{}…", t.chars().take(40).collect::<String>())
                            } else {
                                t
                            }
                        };
                        ui.label(d.label).on_hover_text(d.key);
                        ui.monospace(f(&d.ours));
                        ui.monospace(f(&d.theirs));
                        if ui.small_button(crate::i18n::tr("Take")).clicked() {
                            take.push((d.key, d.theirs.clone()));
                        }
                        ui.end_row();
                    }
                });
            });
        });
    if !take.is_empty() {
        let mut settings = app.session.document().settings.clone();
        let mut failed = 0usize;
        for (k, v) in &take {
            if settings.set(k, v.clone()).is_err() {
                failed += 1;
            }
        }
        if app.run(slicecraft_engine::Command::Profile(Box::new(settings))).is_some() {
            let taken: Vec<&str> = take.iter().map(|(k, _)| *k).collect();
            app.dialogs.compare = Some((
                name.clone(),
                diffs.into_iter().filter(|d| !taken.contains(&d.key)).collect(),
            ));
            app.info(format!("Took {} value(s) from {name}.", take.len() - failed));
        }
    }
    if !open {
        app.dialogs.compare = None;
    }
}

fn shortcuts_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    use crate::keys::{Action, Binding};
    let mut open = app.dialogs.shortcuts;
    // a key pressed while waiting becomes the binding (Esc cancels)
    if let Some(action) = app.dialogs.capture {
        let hit = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    Some(Binding { key: *key, cmd: modifiers.command, shift: modifiers.shift })
                }
                _ => None,
            })
        });
        if let Some(b) = hit {
            if b.key != egui::Key::Escape {
                let clashes = app.prefs.keys.clashes(action, b);
                app.prefs.keys.set(action, b);
                if !clashes.is_empty() {
                    let names: Vec<&str> = clashes.iter().map(|a| a.label()).collect();
                    app.info(format!("{} is also on: {}", b.text(), names.join(", ")));
                }
            }
            app.dialogs.capture = None;
        }
    }
    egui::Window::new(crate::i18n::tr("Keyboard shortcuts"))
        .open(&mut open)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.weak(crate::i18n::tr("Click a shortcut, then press the new keys. Esc cancels."));
            egui::Grid::new("keys").striped(true).show(ui, |ui| {
                for action in Action::ALL {
                    ui.label(action.label());
                    let waiting = app.dialogs.capture == Some(action);
                    let text = if waiting {
                        "press keys…".to_string()
                    } else {
                        app.prefs.keys.text(action)
                    };
                    if ui.button(RichText::new(text).monospace()).clicked() {
                        app.dialogs.capture = Some(action);
                    }
                    let changed = app.prefs.keys.get(action) != action.default_binding();
                    if ui
                        .add_enabled(changed, egui::Button::new(crate::i18n::tr("Reset")))
                        .clicked()
                    {
                        app.prefs.keys.set(action, action.default_binding());
                    }
                    ui.end_row();
                }
            });
            ui.separator();
            egui::Grid::new("fixed-keys").striped(true).show(ui, |ui| {
                for (k, v) in [
                    ("0 – 6", "Perspective, front, back, left, right, top, bottom view"),
                    ("Ctrl+Shift+Z", "Redo"),
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
            if ui.button(crate::i18n::tr("Reset all")).clicked() {
                app.prefs.keys = crate::keys::Shortcuts::default();
            }
        });
    if app.dialogs.shortcuts && !open {
        app.dialogs.capture = None;
        app.persist();
    }
    app.dialogs.shortcuts = open;
}

fn features_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let mut open = app.dialogs.features;
    egui::Window::new(crate::i18n::tr("Feature status"))
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
            ui.weak(crate::i18n::tr(
                "The full inventory and plan are in docs/research/02-feature-inventory.md.",
            ));
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.columns(2, |cols| {
                    if let Some(ui) = cols.get_mut(0) {
                        ui.label(RichText::new(crate::i18n::tr("Commands")).strong());
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
                        ui.label(RichText::new(crate::i18n::tr("Settings by tab")).strong());
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
    egui::Window::new(crate::i18n::tr("About SliceCraft")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.heading("SliceCraft");
        ui.label(format!("version {} · pre-alpha", env!("CARGO_PKG_VERSION")));
        ui.label(crate::i18n::tr("An open-source, clean-room 3D print slicer in pure Rust."));
        ui.label(crate::i18n::tr("Licence: MIT OR Apache-2.0."));
        ui.add_space(4.0);
        ui.weak(crate::i18n::tr("Written from public specifications and observed behaviour; no code, profiles, strings or artwork from other slicers."));
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
            ui.label(crate::i18n::tr(
                "This command is on the plan; the menu shows it so nothing is hidden.",
            ));
            ui.weak(format!("command id: {}", spec.id));
            ui.weak(crate::i18n::tr("See docs/research/02-feature-inventory.md for the plan."));
        },
    );
    if !open {
        app.dialogs.planned = None;
    }
}
