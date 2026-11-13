//! The printer setup assistant: four short pages, then a profile.

use crate::app::SliceCraftApp;
use eframe::egui::{self, RichText};
use slicecraft_doc::settings::{BedShape, Flavor};
use slicecraft_doc::setup::{Drive, Material, PrinterSetup};
use slicecraft_engine::Command;

const PAGES: [&str; 4] = ["Printer", "Extruder", "Material", "Finish"];

/// The assistant while it is open: the page and the answers so far.
#[derive(Debug, Clone, Default)]
pub struct Wizard {
    pub page: usize,
    pub setup: PrinterSetup,
}

fn mm(ui: &mut egui::Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>, speed: f64) {
    ui.add(egui::DragValue::new(value).range(range).speed(speed).suffix(" mm"));
}

pub fn show(app: &mut SliceCraftApp, ctx: &egui::Context) {
    let Some(w) = &mut app.wizard else { return };
    let mut open = true;
    let mut finish = false;
    egui::Window::new(crate::i18n::tr("Printer setup assistant"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (i, p) in PAGES.iter().enumerate() {
                    let text = RichText::new(format!("{}. {p}", i + 1));
                    ui.label(if i == w.page { text.strong() } else { text.weak() });
                }
            });
            ui.separator();
            let s = &mut w.setup;
            match w.page {
                0 => printer_page(ui, s),
                1 => extruder_page(ui, s),
                2 => material_page(ui, s),
                _ => finish_page(ui, s),
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(w.page > 0, egui::Button::new(crate::i18n::tr("Back"))).clicked()
                {
                    w.page -= 1;
                }
                let valid = w.setup.validate();
                if w.page + 1 < PAGES.len() {
                    if ui.button(crate::i18n::tr("Next")).clicked() {
                        w.page += 1;
                    }
                } else if ui
                    .add_enabled(valid.is_ok(), egui::Button::new(crate::i18n::tr("Make profile")))
                    .clicked()
                {
                    finish = true;
                }
                if let Err(e) = valid {
                    ui.colored_label(egui::Color32::from_rgb(0xff, 0x7a, 0x70), e.to_string());
                }
            });
        });
    if finish {
        let setup = app.wizard.take().map(|w| w.setup).unwrap_or_default();
        if app.run(Command::SetupPrinter(Box::new(setup))).is_some() {
            app.persist();
        }
    } else if !open {
        app.wizard = None;
    }
}

fn printer_page(ui: &mut egui::Ui, s: &mut PrinterSetup) {
    ui.label(crate::i18n::tr("What the printer is called and how large it prints."));
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Start from"));
        egui::ComboBox::from_id_salt("wiz-model").selected_text("a known printer…").show_ui(
            ui,
            |ui| {
                for m in slicecraft_doc::setup::PRINTERS {
                    if ui.selectable_label(false, m.name).clicked() {
                        *s = PrinterSetup::from_model(m, s.material);
                    }
                }
            },
        );
    });
    ui.weak(crate::i18n::tr(
        "Sizes are the makers' published build volumes; check them against your printer.",
    ));
    egui::Grid::new("wiz-printer").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label(crate::i18n::tr("Name"));
        ui.text_edit_singleline(&mut s.name);
        ui.end_row();
        ui.label(crate::i18n::tr("Firmware"));
        egui::ComboBox::from_id_salt("wiz-flavor").selected_text(flavor_label(s.flavor)).show_ui(
            ui,
            |ui| {
                for f in Flavor::ALL {
                    ui.selectable_value(&mut s.flavor, f, flavor_label(f));
                }
            },
        );
        ui.end_row();
        ui.label(crate::i18n::tr("Bed"));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.bed_shape, BedShape::Rectangle, "Rectangular");
            ui.selectable_value(&mut s.bed_shape, BedShape::Circle, "Round");
        });
        ui.end_row();
        if s.bed_shape == BedShape::Circle {
            ui.label(crate::i18n::tr("Diameter"));
            mm(ui, &mut s.bed_width, 20.0..=2000.0, 1.0);
            ui.end_row();
        } else {
            ui.label(crate::i18n::tr("Width (X)"));
            mm(ui, &mut s.bed_width, 20.0..=2000.0, 1.0);
            ui.end_row();
            ui.label(crate::i18n::tr("Depth (Y)"));
            mm(ui, &mut s.bed_depth, 20.0..=2000.0, 1.0);
            ui.end_row();
        }
        ui.label(crate::i18n::tr("Height (Z)"));
        mm(ui, &mut s.bed_height, 20.0..=2000.0, 1.0);
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut s.heated_bed, crate::i18n::tr("The bed heats"));
        ui.end_row();
        if s.bed_shape == BedShape::Rectangle {
            ui.label("");
            ui.checkbox(
                &mut s.origin_center,
                crate::i18n::tr("Coordinates start at the bed's centre"),
            );
            ui.end_row();
        }
    });
    if s.bed_shape == BedShape::Circle {
        ui.weak(crate::i18n::tr("Round beds are measured from their centre."));
    }
}

fn flavor_label(f: Flavor) -> &'static str {
    match f {
        Flavor::Marlin => "Marlin (most printers)",
        Flavor::RepRap => "RepRapFirmware (Duet)",
        Flavor::Klipper => "Klipper",
    }
}

fn extruder_page(ui: &mut egui::Ui, s: &mut PrinterSetup) {
    ui.label(crate::i18n::tr("The nozzle and how filament reaches it."));
    egui::Grid::new("wiz-extruder").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label(crate::i18n::tr("Nozzle"));
        ui.horizontal(|ui| {
            for d in [0.25, 0.4, 0.6, 0.8] {
                ui.selectable_value(&mut s.nozzle_diameter, d, format!("{d}"));
            }
            mm(ui, &mut s.nozzle_diameter, 0.1..=2.0, 0.01);
        });
        ui.end_row();
        ui.label(crate::i18n::tr("Filament"));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.filament_diameter, 1.75, "1.75");
            ui.selectable_value(&mut s.filament_diameter, 2.85, "2.85");
            mm(ui, &mut s.filament_diameter, 1.0..=3.5, 0.01);
        });
        ui.end_row();
    });
    ui.add_space(4.0);
    for d in Drive::ALL {
        ui.radio_value(&mut s.drive, d, d.label());
    }
    ui.weak(crate::i18n::tr("Bowden printers retract further; direct drive retracts less."));
}

fn material_page(ui: &mut egui::Ui, s: &mut PrinterSetup) {
    ui.label(crate::i18n::tr("The filament you print most. You can change it later."));
    egui::Grid::new("wiz-material").num_columns(4).striped(true).spacing([14.0, 4.0]).show(
        ui,
        |ui| {
            ui.label("");
            ui.weak(crate::i18n::tr("Nozzle"));
            ui.weak(crate::i18n::tr("Bed"));
            ui.weak(crate::i18n::tr("Fan"));
            ui.end_row();
            for m in Material::ALL {
                let v = m.values();
                ui.radio_value(&mut s.material, m, m.label());
                ui.label(format!("{:.0} °C", v.nozzle));
                ui.label(if s.heated_bed { format!("{:.0} °C", v.bed) } else { "off".into() });
                ui.label(format!("{:.0} %", v.fan));
                ui.end_row();
            }
        },
    );
}

fn finish_page(ui: &mut egui::Ui, s: &mut PrinterSetup) {
    let bed = if s.bed_shape == BedShape::Circle {
        format!("round, {:.0} mm across, {:.0} mm tall", s.bed_width, s.bed_height)
    } else {
        format!("{:.0} × {:.0} × {:.0} mm", s.bed_width, s.bed_depth, s.bed_height)
    };
    ui.label(RichText::new(s.name.trim()).strong());
    ui.label(format!("{} firmware, bed {bed}", flavor_label(s.flavor)));
    ui.label(format!(
        "{} mm nozzle, {} mm filament, {}",
        s.nozzle_diameter,
        s.filament_diameter,
        match s.drive {
            Drive::Direct => "direct drive",
            Drive::Bowden => "Bowden",
        }
    ));
    ui.label(format!("{} at {:.0} °C", s.material.label(), s.material.values().nozzle));
    ui.add_space(6.0);
    ui.weak(
        "The profile homes the printer and lays a priming line along the front edge before \
         printing. Fine-tune anything in All settings afterwards.",
    );
}
