//! Windows that open over the main view: the profile editor (rendered from the
//! settings schema), preferences, mesh analysis, the machine panel
//! (placeholder), shortcuts, feature status and about.

use crate::app::SliceCraftApp;
use crate::theme;
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
            || self.compare.is_some()
    }

    /// Close one window, the kind most likely on top: small notes first, the
    /// profile editor last. Closing the editor this way discards its draft,
    /// as Cancel does.
    pub fn close_top(&mut self) {
        if self.planned.take().is_some()
            || std::mem::take(&mut self.about)
            || std::mem::take(&mut self.features)
            || std::mem::take(&mut self.shortcuts)
            || self.compare.take().is_some()
            || self.import_report.take().is_some()
            || std::mem::take(&mut self.analysis)
            || std::mem::take(&mut self.preferences)
            || self.zones.take().is_some()
            || std::mem::take(&mut self.machine)
        {
            return;
        }
        self.settings = None;
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
    /// True until the window has been drawn once; the search box then takes
    /// the keyboard, so typing finds a setting straight away.
    pub fresh: bool,
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
            fresh: true,
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
    let mut profile_error: Option<String> = None;
    crate::theme::window(ctx, crate::i18n::tr("Setting zones: several profiles per job")).open(&mut open).default_width(640.0).show(ctx, |ui| {
        ui.label(crate::i18n::tr("Print part of the job with other settings: above or below a height, for some models only, or both. A zone can take a whole profile. Zones apply in order; later ones win."));
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
                    ui.label(crate::i18n::tr("Use a whole profile"));
                    let mut chosen = None;
                    egui::ComboBox::from_id_salt("zone-preset")
                        .selected_text(crate::i18n::tr("preset…"))
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for p in presets() {
                                if ui.selectable_label(false, &p.name).clicked() {
                                    chosen = Some(p);
                                }
                            }
                        });
                    if let Some(p) = chosen {
                        z.take_profile(&base, &p.settings);
                    }
                    if ui.button(crate::i18n::tr("From a file…")).clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Profiles (SliceCraft, Simplify3D)", &["scprofile", "fff", "txt", "gcode"])
                            .pick_file()
                    {
                        match slicecraft_engine::read_profile_file(&path) {
                            Ok(p) => z.take_profile(&base, &p.settings),
                            Err(e) => profile_error = Some(e.to_string()),
                        }
                    }
                })
                .response
                .on_hover_text(crate::i18n::tr(
                    "The zone takes every job setting where that profile differs from the plate's",
                ));
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
    if let Some(e) = profile_error {
        app.error(e);
    }
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
    crate::theme::window(ctx, crate::i18n::tr("Profile import"))
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
    crate::theme::window(ctx, crate::i18n::tr("Unsaved changes"))
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

/// Width of the value column in the profile editor.
const FIELD_W: f32 = 180.0;

/// One setting's editor widget, `width` wide (text gets the whole row).
/// Returns a new value when it was changed.
fn setting_widget(ui: &mut Ui, spec: &SettingSpec, value: &SettingValue) -> Option<SettingValue> {
    setting_widget_w(ui, spec, value, FIELD_W)
}

fn setting_widget_w(
    ui: &mut Ui,
    spec: &SettingSpec,
    value: &SettingValue,
    width: f32,
) -> Option<SettingValue> {
    let size = [width, 24.0];
    match (spec.kind, value) {
        (SettingKind::Float { min, max, step, unit }, SettingValue::Float(v)) => {
            let mut x = *v;
            let suffix = if unit.is_empty() { String::new() } else { format!(" {unit}") };
            let r = ui.add_sized(
                size,
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
            let r =
                ui.add_sized(size, egui::DragValue::new(&mut x).range(min..=max).suffix(suffix));
            r.changed().then_some(SettingValue::Int(x))
        }
        (SettingKind::Bool, SettingValue::Bool(v)) => {
            let mut x = *v;
            let text = if x { crate::i18n::tr("on") } else { crate::i18n::tr("off") };
            ui.checkbox(&mut x, RichText::new(text).color(theme::of(ui).dim))
                .changed()
                .then_some(SettingValue::Bool(x))
        }
        (SettingKind::Choice(opts), SettingValue::Choice(i)) => {
            let mut x = *i;
            let shown = crate::i18n::tr(opts.get(x).copied().unwrap_or("?"));
            egui::ComboBox::from_id_salt(spec.key).selected_text(shown).width(width).show_ui(
                ui,
                |ui| {
                    for (k, o) in opts.iter().enumerate() {
                        ui.selectable_value(&mut x, k, crate::i18n::tr(o));
                    }
                },
            );
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

/// The settings of one tab (or of a search), section by section, each
/// section a card of label / value rows.
fn setting_rows(ui: &mut Ui, ed: &mut SettingsEditor, specs: &[&'static SettingSpec]) {
    // keep each section together, in the order sections first appear
    let mut order: Vec<&str> = Vec::new();
    for s in specs {
        if !order.contains(&s.section) {
            order.push(s.section);
        }
    }
    let p = theme::of(ui);
    for section in order {
        let rows: Vec<&'static SettingSpec> =
            specs.iter().copied().filter(|s| s.section == section).collect();
        theme::card(ui, section, |ui| {
            let label_w = (ui.available_width() - FIELD_W - 70.0).clamp(140.0, 320.0);
            for spec in rows {
                let Some(value) = ed.draft.get(spec.key) else { continue };
                let changed = ed.original.get(spec.key).as_ref() != Some(&value);
                let is_default = value == spec.default.value();
                let text_kind = matches!(spec.kind, SettingKind::Text);
                let help = format!("{}\n\nkey: {}", spec.help, spec.key);
                ui.horizontal(|ui| {
                    let colour = if changed { CHANGED } else { p.text };
                    ui.allocate_ui_with_layout(
                        egui::vec2(label_w, 24.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(label_w);
                            if changed {
                                let (dot, _) = ui.allocate_exact_size(
                                    egui::vec2(6.0, 6.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(dot.center(), 3.0, CHANGED);
                            }
                            ui.add(
                                egui::Label::new(
                                    RichText::new(crate::i18n::tr(spec.label)).color(colour),
                                )
                                .truncate(),
                            )
                            .on_hover_text(help);
                        },
                    );
                    if !text_kind && let Some(v) = setting_widget(ui, spec, &value) {
                        match ed.draft.set(spec.key, v) {
                            Ok(()) => ed.error = None,
                            Err(e) => ed.error = Some(e.to_string()),
                        }
                    }
                    if !spec.implemented() {
                        theme::tag(ui, crate::i18n::tr("planned"), PLANNED).on_hover_text(
                            crate::i18n::tr("Saved with the profile, not used by the slicer yet"),
                        );
                    }
                    if !is_default
                        && ui
                            .small_button("↺")
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
        });
    }
}

/// How many settings of `tab` differ from the profile as it was opened.
fn changed_in(ed: &SettingsEditor, tab: Tab) -> usize {
    tab_settings(tab).filter(|s| ed.draft.get(s.key) != ed.original.get(s.key)).count()
}

fn settings_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let Some(mut ed) = app.dialogs.settings.take() else { return };
    let mut open = true;
    let mut apply = false;
    let mut close = false;
    let mut load = false;
    let mut save = false;
    let screen = ctx.content_rect();
    let size =
        egui::vec2((screen.width() - 80.0).min(1000.0), (screen.height() - 120.0).min(720.0));
    crate::theme::window(ctx, crate::i18n::tr("Print settings"))
        .open(&mut open)
        .default_size(size)
        .min_size([720.0, 440.0])
        .resizable(true)
        .show(ctx, |ui| {
            let p = theme::of(ui);
            egui::Panel::top("settings-head")
                .frame(
                    egui::Frame::new()
                        .inner_margin(egui::Margin { bottom: 8, ..Default::default() }),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        theme::field(ui, "Profile");
                        ui.add(egui::TextEdit::singleline(&mut ed.name).desired_width(240.0));
                        egui::ComboBox::from_id_salt("start-from")
                            .selected_text(crate::i18n::tr("Start from preset…"))
                            .width(170.0)
                            .show_ui(ui, |ui| {
                                for p in presets() {
                                    if ui.selectable_label(false, &p.name).clicked() {
                                        ed.name = p.name.clone();
                                        ed.draft = p.settings;
                                    }
                                }
                            });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.checkbox(&mut ed.only_changed, crate::i18n::tr("changed only"));
                            let find = ui.add(
                                egui::TextEdit::singleline(&mut ed.search)
                                    .hint_text(crate::i18n::tr("Find a setting…"))
                                    .desired_width(220.0),
                            );
                            if ed.fresh {
                                find.request_focus();
                                ed.fresh = false;
                            }
                        });
                    });
                });
            egui::Panel::bottom("settings-foot")
                .frame(
                    egui::Frame::new().inner_margin(egui::Margin { top: 8, ..Default::default() }),
                )
                .show(ui, |ui| {
                    if let Some(e) = &ed.error {
                        ui.colored_label(WARN, e);
                    } else {
                        ui.horizontal(|ui| {
                            let (dot, _) =
                                ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                            ui.painter().circle_filled(dot.center(), 3.0, CHANGED);
                            ui.label(
                                RichText::new(crate::i18n::tr("changed since opening"))
                                    .size(12.0)
                                    .color(p.dim),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(crate::i18n::tr("Hover a name for its help."))
                                    .size(12.0)
                                    .color(p.dim),
                            );
                        });
                    }
                    ui.add_space(4.0);
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
                            if theme::primary(
                                ui,
                                crate::i18n::tr("OK"),
                                true,
                                egui::vec2(96.0, 30.0),
                            )
                            .clicked()
                            {
                                apply = true;
                                close = true;
                            }
                            if ui
                                .add(
                                    egui::Button::new(crate::i18n::tr("Apply"))
                                        .min_size(egui::vec2(80.0, 30.0)),
                                )
                                .clicked()
                            {
                                apply = true;
                            }
                            if ui
                                .add(
                                    egui::Button::new(crate::i18n::tr("Cancel"))
                                        .min_size(egui::vec2(80.0, 30.0)),
                                )
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    });
                });
            egui::Panel::left("settings-nav")
                .resizable(false)
                .exact_size(186.0)
                .frame(
                    egui::Frame::new()
                        .inner_margin(egui::Margin { right: 12, ..Default::default() }),
                )
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt("settings-tabs").show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for t in Tab::ALL {
                            let selected = ed.tab == t && ed.search.is_empty();
                            let n = changed_in(&ed, t);
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 32.0),
                                egui::Sense::click(),
                            );
                            let bg = if selected {
                                p.accent.gamma_multiply(0.28)
                            } else if resp.hovered() {
                                p.hover
                            } else {
                                Color32::TRANSPARENT
                            };
                            ui.painter().rect_filled(rect, 6, bg);
                            if selected {
                                let bar = egui::Rect::from_min_size(
                                    rect.min,
                                    egui::vec2(3.0, rect.height()),
                                );
                                ui.painter().rect_filled(bar, 2, p.accent);
                            }
                            let planned = tab_settings(t).all(|s| !s.implemented());
                            let colour = if planned { PLANNED } else { p.text };
                            let font = if selected {
                                theme::strong(14.0)
                            } else {
                                egui::FontId::proportional(14.0)
                            };
                            ui.painter().text(
                                rect.left_center() + egui::vec2(12.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                crate::i18n::tr(t.label()),
                                font,
                                colour,
                            );
                            if n > 0 {
                                let c = rect.right_center() - egui::vec2(16.0, 0.0);
                                ui.painter().circle_filled(c, 9.0, CHANGED.gamma_multiply(0.85));
                                ui.painter().text(
                                    c,
                                    egui::Align2::CENTER_CENTER,
                                    n.to_string(),
                                    egui::FontId::proportional(11.0),
                                    Color32::WHITE,
                                );
                            }
                            let label = crate::i18n::tr(t.label()).to_string();
                            resp.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::SelectableLabel,
                                    true,
                                    selected,
                                    &label,
                                )
                            });
                            if resp.clicked() {
                                ed.tab = t;
                                ed.search.clear();
                            }
                        }
                    });
                });
            let query = ed.search.trim().to_lowercase();
            let specs: Vec<&'static SettingSpec> = if query.is_empty() {
                tab_settings(ed.tab).collect()
            } else {
                SETTINGS
                    .iter()
                    .filter(|s| {
                        s.label.to_lowercase().contains(&query)
                            || crate::i18n::tr(s.label).to_lowercase().contains(&query)
                            || s.key.contains(&query)
                    })
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
            egui::ScrollArea::vertical().id_salt("settings-body").auto_shrink([false, false]).show(
                ui,
                |ui| {
                    ui.add_space(2.0);
                    if specs.is_empty() {
                        theme::dim(ui, crate::i18n::tr("No settings match."));
                    }
                    if !query.is_empty() {
                        // group search results by tab
                        for t in Tab::ALL {
                            let in_tab: Vec<&'static SettingSpec> =
                                specs.iter().copied().filter(|s| s.tab == t).collect();
                            if !in_tab.is_empty() {
                                ui.label(
                                    RichText::new(crate::i18n::tr(t.label()))
                                        .font(theme::strong(17.0)),
                                );
                                ui.add_space(4.0);
                                setting_rows(ui, &mut ed, &in_tab);
                                ui.add_space(6.0);
                            }
                        }
                    } else {
                        setting_rows(ui, &mut ed, &specs);
                    }
                },
            );
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
    use crate::i18n::tr;
    let mut open = app.dialogs.preferences;
    let mut shortcuts = false;
    crate::theme::window(ctx, tr("Preferences"))
        .open(&mut open)
        .default_width(720.0)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.columns(2, |cols| {
                let [left, right] = cols else { return };
                let p = &mut app.prefs;
                theme::card(left, "Appearance", |ui| {
                    ui.horizontal(|ui| {
                        theme::field(ui, "Theme");
                        let mut dark = p.dark;
                        if theme::segmented(
                            ui,
                            &mut dark,
                            &[(true, tr("Dark"), true), (false, tr("Light"), true)],
                        ) {
                            p.dark = dark;
                        }
                    });
                    ui.horizontal(|ui| {
                        theme::field(ui, "Language");
                        let current = p.language;
                        egui::ComboBox::from_id_salt("language")
                            .selected_text(current.label())
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for l in crate::i18n::Language::ALL {
                                    ui.selectable_value(&mut p.language, l, l.label());
                                }
                            });
                        crate::i18n::set_language(p.language);
                    });
                    ui.label(
                        RichText::new(tr(
                            "Menus, panels and setting names follow the language; help text is in English.",
                        ))
                        .size(12.0)
                        .color(theme::of(ui).dim),
                    );
                });
                let gpu_line = match (&app.gpu, &app.gpu_note) {
                    (Some(g), _) => format!("{} {}", tr("Graphics card:"), g.describe()),
                    (None, Some(n)) => format!("{} {n}", tr("Not on the graphics card:")),
                    (None, None) => tr("No graphics card in use.").to_string(),
                };
                let p = &mut app.prefs;
                theme::card(left, "3D view", |ui| {
                    ui.checkbox(&mut p.gpu_view, tr("Draw the 3D view on the graphics card"));
                    ui.label(RichText::new(gpu_line).size(12.0).color(theme::of(ui).dim));
                    ui.checkbox(&mut p.bed.grid, tr("Bed grid"));
                    ui.checkbox(&mut p.bed.volume, tr("Build volume outline"));
                    ui.checkbox(&mut p.bed.axes, tr("Axes at the origin"));
                    ui.checkbox(&mut p.ortho, tr("Orthographic camera"));
                    ui.checkbox(&mut p.wireframe, tr("Draw models as wireframes"));
                    ui.checkbox(&mut p.invert_zoom, tr("Invert wheel zoom"));
                    ui.horizontal(|ui| {
                        theme::field(ui, "Orbit speed");
                        ui.add(egui::Slider::new(&mut p.orbit_sensitivity, 0.2..=3.0));
                    });
                });
                theme::card(right, "Workflow", |ui| {
                    ui.checkbox(&mut p.preview_after_slice, tr("Show the toolpaths after slicing"));
                    ui.checkbox(&mut p.fit_on_import, tr("Fit the view to models after import"));
                    ui.add_space(2.0);
                    if ui.button(tr("Keyboard shortcuts…")).clicked() {
                        shortcuts = true;
                    }
                });
                theme::card(right, "Import", |ui| {
                    ui.checkbox(
                        &mut p.inch_check,
                        tr("Scale models under 10 mm across by 25.4 (inches)"),
                    );
                    theme::field(ui, "Turn every import by");
                    ui.horizontal(|ui| {
                        for (k, v) in p.import_rotate.iter_mut().enumerate() {
                            theme::axis_chip(ui, k);
                            ui.add_sized(
                                [56.0, 24.0],
                                egui::DragValue::new(v).range(-360.0..=360.0).speed(1.0).suffix("°"),
                            );
                        }
                    });
                    ui.horizontal(|ui| {
                        theme::field(ui, "and scale it by");
                        ui.add_sized(
                            [70.0, 24.0],
                            egui::DragValue::new(&mut p.import_scale).range(0.001..=1000.0).speed(0.01),
                        );
                    });
                });
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let where_ = match crate::config::config_dir() {
                    Some(dir) => format!("{} {}", tr("Remembered in"), dir.display()),
                    None => tr("This system gives no settings folder; preferences last until the window closes.")
                        .to_string(),
                };
                ui.label(RichText::new(where_).size(12.0).color(theme::of(ui).dim));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("Restore default preferences")).clicked() {
                        app.prefs = crate::app::Preferences::default();
                    }
                });
            });
        });
    if shortcuts {
        app.dialogs.shortcuts = true;
    }
    if app.dialogs.preferences && !open {
        app.persist();
    }
    app.dialogs.preferences = open;
}

fn machine_window(app: &mut SliceCraftApp, ctx: &egui::Context) {
    use crate::i18n::tr;
    use slicecraft_engine::Command;
    let mut open = app.dialogs.machine;
    let connected = app.session.printer_port().map(str::to_string);
    let printers = app.session.printers();
    let mut run: Vec<Command> = Vec::new();
    theme::window(ctx, tr("Machine control"))
        .open(&mut open)
        .default_width(820.0)
        .resizable(false)
        .show(ctx, |ui| {
            let p = theme::of(ui);
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
            // connection across the top
            theme::card_with(
                ui,
                "Connection",
                |ui| match &connected {
                    Some(port) => {
                        theme::tag(ui, &format!("{} · {port}", tr("connected")), p.ok);
                    }
                    None => {
                        theme::tag(ui, tr("not connected"), p.dim);
                    }
                },
                |ui| {
                    ui.horizontal(|ui| {
                        theme::field(ui, "Port");
                        egui::ComboBox::from_id_salt("port")
                            .selected_text(if m.port.is_empty() {
                                tr("choose…")
                            } else {
                                m.port.as_str()
                            })
                            .width(180.0)
                            .show_ui(ui, |ui| {
                                for port in &m.ports {
                                    ui.selectable_value(&mut m.port, port.clone(), port);
                                }
                            });
                        if ui.button("↻").on_hover_text(tr("Look for serial ports again")).clicked()
                        {
                            m.ports = slicecraft_io::printer::list_ports();
                        }
                        theme::field(ui, "Speed");
                        egui::ComboBox::from_id_salt("baud")
                            .selected_text(m.baud.to_string())
                            .width(100.0)
                            .show_ui(ui, |ui| {
                                for b in [250_000u32, 115_200, 57_600, 38_400, 19_200] {
                                    ui.selectable_value(&mut m.baud, b, b.to_string());
                                }
                            });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            match &connected {
                                Some(_) => {
                                    if ui.button(tr("Disconnect")).clicked() {
                                        run.push(Command::DisconnectPrinter);
                                    }
                                    let other = !m.port.is_empty()
                                        && printers.iter().all(|x| x.port != m.port);
                                    if ui
                                        .add_enabled(
                                            other,
                                            egui::Button::new(tr("Connect another")),
                                        )
                                        .on_hover_text(tr(
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
                                    if theme::primary(
                                        ui,
                                        tr("Connect"),
                                        !m.port.is_empty(),
                                        egui::vec2(110.0, 28.0),
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
                    });
                    if printers.len() > 1 {
                        ui.add_space(4.0);
                        egui::Grid::new("printers").striped(true).num_columns(3).show(ui, |ui| {
                            for pr in &printers {
                                ui.label(&pr.port);
                                match &pr.status {
                                    None => {
                                        ui.label(tr("controlled here"));
                                    }
                                    Some(st) => {
                                        let what = match (st.progress, st.finished, &st.error) {
                                            (_, _, Some(e)) => format!("error: {e}"),
                                            (Some((d, n)), _, _) => format!(
                                                "printing {:.0} %",
                                                100.0 * d as f64 / n.max(1) as f64
                                            ),
                                            (None, true, _) => "finished".to_string(),
                                            _ => "idle".to_string(),
                                        };
                                        ui.label(format!(
                                            "{what} · {:.0} °C / {:.0} °C",
                                            st.nozzle, st.bed
                                        ));
                                    }
                                }
                                if !pr.active && ui.small_button(tr("Control")).clicked() {
                                    run.push(Command::SwitchPrinter(pr.port.clone()));
                                }
                                ui.end_row();
                            }
                        });
                    }
                },
            );
            let on = connected.is_some();
            ui.add_enabled_ui(on, |ui| {
                ui.columns(2, |cols| {
                    let [left, right] = cols else { return };
                    theme::card(left, "Temperatures", |ui| {
                        egui::Grid::new("temps").num_columns(4).spacing([10.0, 6.0]).show(
                            ui,
                            |ui| {
                                for (label, now, set, max, code) in [
                                    ("Nozzle", m.nozzle, &mut m.set_nozzle, 320.0, "M104"),
                                    ("Bed", m.bed, &mut m.set_bed, 130.0, "M140"),
                                ] {
                                    theme::field(ui, label);
                                    ui.label(
                                        RichText::new(format!("{:.1} °C", now.0))
                                            .font(theme::strong(16.0)),
                                    )
                                    .on_hover_text(format!(
                                        "{} {:.0} °C",
                                        tr("target"),
                                        now.1
                                    ));
                                    ui.add_sized(
                                        [80.0, 24.0],
                                        egui::DragValue::new(set).range(0.0..=max).suffix(" °C"),
                                    );
                                    if ui.button(tr("Set")).clicked() {
                                        run.push(Command::SendToPrinter(format!(
                                            "{code} S{:.0}",
                                            *set
                                        )));
                                    }
                                    ui.end_row();
                                }
                            },
                        );
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            if ui.button(tr("Heaters off")).clicked() {
                                run.push(Command::SendToPrinter("M104 S0".into()));
                                run.push(Command::SendToPrinter("M140 S0".into()));
                            }
                            if ui.button(tr("Fan on")).clicked() {
                                run.push(Command::SendToPrinter("M106 S255".into()));
                            }
                            if ui.button(tr("Fan off")).clicked() {
                                run.push(Command::SendToPrinter("M107".into()));
                            }
                        });
                    });
                    theme::card(left, "Print", |ui| {
                        let w = ui.available_width();
                        if theme::primary(
                            ui,
                            tr("Print the current slice"),
                            true,
                            egui::vec2(w, 32.0),
                        )
                        .clicked()
                        {
                            run.push(Command::PrintOverUsb);
                        }
                        ui.horizontal(|ui| {
                            let third =
                                (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
                            let b = |t: &str| {
                                egui::Button::new(tr(t)).min_size(egui::vec2(third, 28.0))
                            };
                            if ui.add(b("Pause")).clicked() {
                                run.push(Command::PausePrint);
                            }
                            if ui.add(b("Resume")).clicked() {
                                run.push(Command::ResumePrint);
                            }
                            if ui.add(b("Cancel")).clicked() {
                                run.push(Command::CancelPrint);
                            }
                        });
                        if let Some((done, total)) = m.progress {
                            let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                            ui.add(
                                egui::ProgressBar::new(frac)
                                    .fill(p.accent)
                                    .text(format!("{done} / {total} {}", tr("lines"))),
                            );
                        }
                    });
                    theme::card(right, "Move", |ui| {
                        ui.horizontal(|ui| {
                            theme::field(ui, "Step");
                            let mut step = m.step;
                            if theme::segmented(
                                ui,
                                &mut step,
                                &[
                                    (0.1, "0.1", true),
                                    (1.0, "1", true),
                                    (10.0, "10", true),
                                    (50.0, "50", true),
                                ],
                            ) {
                                m.step = step;
                            }
                            theme::dim(ui, "mm");
                        });
                        ui.add_space(4.0);
                        let step = m.step;
                        let jog = |run: &mut Vec<Command>, axis: &str, sign: f64| {
                            for l in [
                                "G91".to_string(),
                                format!("G1 {axis}{:.2} F3000", sign * step),
                                "G90".to_string(),
                            ] {
                                run.push(Command::SendToPrinter(l));
                            }
                        };
                        let key = egui::vec2(48.0, 36.0);
                        egui::Grid::new("jog").num_columns(5).spacing([6.0, 6.0]).show(ui, |ui| {
                            ui.label("");
                            if ui.add(egui::Button::new("Y+").min_size(key)).clicked() {
                                jog(&mut run, "Y", 1.0);
                            }
                            ui.label("");
                            ui.label("");
                            if ui.add(egui::Button::new("Z+").min_size(key)).clicked() {
                                jog(&mut run, "Z", 1.0);
                            }
                            ui.end_row();
                            if ui.add(egui::Button::new("X−").min_size(key)).clicked() {
                                jog(&mut run, "X", -1.0);
                            }
                            if ui
                                .add(egui::Button::new(tr("Home")).min_size(key))
                                .on_hover_text(tr("Home XY"))
                                .clicked()
                            {
                                run.push(Command::SendToPrinter("G28 X Y".into()));
                            }
                            if ui.add(egui::Button::new("X+").min_size(key)).clicked() {
                                jog(&mut run, "X", 1.0);
                            }
                            ui.label("");
                            if ui
                                .add(egui::Button::new(tr("All")).min_size(key))
                                .on_hover_text(tr("Home all"))
                                .clicked()
                            {
                                run.push(Command::SendToPrinter("G28".into()));
                            }
                            ui.end_row();
                            ui.label("");
                            if ui.add(egui::Button::new("Y−").min_size(key)).clicked() {
                                jog(&mut run, "Y", -1.0);
                            }
                            ui.label("");
                            ui.label("");
                            if ui.add(egui::Button::new("Z−").min_size(key)).clicked() {
                                jog(&mut run, "Z", -1.0);
                            }
                            ui.end_row();
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.button(tr("Extrude 5 mm")).clicked() {
                                for l in ["M83", "G1 E5 F300", "M82"] {
                                    run.push(Command::SendToPrinter(l.into()));
                                }
                            }
                            if ui.button(tr("Retract 5 mm")).clicked() {
                                for l in ["M83", "G1 E-5 F1800", "M82"] {
                                    run.push(Command::SendToPrinter(l.into()));
                                }
                            }
                            if ui.button(tr("Motors off")).clicked() {
                                run.push(Command::SendToPrinter("M84".into()));
                            }
                        });
                    });
                });
                theme::card(ui, "Terminal", |ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().extreme_bg_color)
                        .corner_radius(6)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical()
                                .max_height(140.0)
                                .min_scrolled_height(140.0)
                                .stick_to_bottom(true)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if m.log.is_empty() {
                                        theme::dim(ui, tr("The printer's replies appear here."));
                                    }
                                    for l in &m.log {
                                        ui.monospace(l);
                                    }
                                });
                        });
                    ui.horizontal(|ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut m.input)
                                .hint_text(tr("G-code to send, e.g. M115"))
                                .desired_width(ui.available_width() - 80.0),
                        );
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (ui
                            .add(egui::Button::new(tr("Send")).min_size(egui::vec2(70.0, 26.0)))
                            .clicked()
                            || enter)
                            && !m.input.trim().is_empty()
                        {
                            run.push(Command::SendToPrinter(std::mem::take(&mut m.input)));
                        }
                    });
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
    crate::theme::window(ctx, crate::i18n::tr("Mesh analysis")).open(&mut open).collapsible(false).default_width(560.0).show(ctx, |ui| {
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
    crate::theme::window(ctx, format!("Compare with {name}"))
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
    crate::theme::window(ctx, crate::i18n::tr("Keyboard shortcuts"))
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
    crate::theme::window(ctx, crate::i18n::tr("Feature status"))
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
    crate::theme::window(ctx, crate::i18n::tr("About SliceCraft")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
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
    crate::theme::window(ctx, spec.label).open(&mut open).collapsible(false).resizable(false).show(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_closes_small_windows_before_the_profile_editor() {
        let mut d = Dialogs {
            settings: Some(SettingsEditor::new("draft", PrintSettings::default())),
            about: true,
            machine: true,
            ..Default::default()
        };
        d.close_top();
        assert!(!d.about && d.machine && d.settings.is_some());
        d.close_top();
        assert!(!d.machine && d.settings.is_some());
        d.close_top();
        assert!(d.settings.is_none() && !d.any_open());
        // nothing open: closing again changes nothing
        d.close_top();
        assert!(!d.any_open());
    }
}
