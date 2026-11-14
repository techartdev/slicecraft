//! The printer setup assistant's answers, and the profile they make.
//!
//! A few questions a person can answer from their printer's box and their
//! filament's label; everything else comes from SliceCraft's own defaults.
//! The material table holds ordinary middle-of-the-range values from filament
//! makers' public data sheets, rounded; the start and end scripts are our own.

use crate::DocError;
use crate::profile::Profile;
use crate::settings::{BedShape, Flavor, PrintSettings};

/// A filament family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Material {
    #[default]
    Pla,
    Petg,
    Abs,
    Asa,
    Tpu,
    Nylon,
}

/// Starting values for one material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialValues {
    pub nozzle: f64,
    pub bed: f64,
    pub fan: f64,
    /// g/cm³
    pub density: f64,
    /// Print speed scale: flexible filament goes slowly.
    pub speed_scale: f64,
    /// Retraction scale: flexible filament strings less when it retracts less.
    pub retract_scale: f64,
}

impl Material {
    pub const ALL: [Material; 6] = [
        Material::Pla,
        Material::Petg,
        Material::Abs,
        Material::Asa,
        Material::Tpu,
        Material::Nylon,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Material::Pla => "PLA",
            Material::Petg => "PETG",
            Material::Abs => "ABS",
            Material::Asa => "ASA",
            Material::Tpu => "TPU (flexible)",
            Material::Nylon => "Nylon",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Material::Pla => "pla",
            Material::Petg => "petg",
            Material::Abs => "abs",
            Material::Asa => "asa",
            Material::Tpu => "tpu",
            Material::Nylon => "nylon",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(s.trim()))
    }

    pub fn values(self) -> MaterialValues {
        let v = |nozzle, bed, fan, density, speed_scale, retract_scale| MaterialValues {
            nozzle,
            bed,
            fan,
            density,
            speed_scale,
            retract_scale,
        };
        match self {
            Material::Pla => v(210.0, 60.0, 100.0, 1.24, 1.0, 1.0),
            Material::Petg => v(240.0, 80.0, 40.0, 1.27, 0.9, 1.0),
            Material::Abs => v(245.0, 100.0, 0.0, 1.04, 1.0, 1.0),
            Material::Asa => v(250.0, 100.0, 15.0, 1.07, 1.0, 1.0),
            Material::Tpu => v(225.0, 50.0, 60.0, 1.21, 0.5, 0.3),
            Material::Nylon => v(255.0, 80.0, 0.0, 1.14, 0.9, 1.0),
        }
    }
}

/// How the filament reaches the hot end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Drive {
    /// The motor sits on the print head.
    #[default]
    Direct,
    /// The motor sits on the frame and pushes through a tube.
    Bowden,
}

impl Drive {
    pub const ALL: [Drive; 2] = [Drive::Direct, Drive::Bowden];

    pub fn label(self) -> &'static str {
        match self {
            Drive::Direct => "Direct drive (motor on the print head)",
            Drive::Bowden => "Bowden (motor on the frame, filament in a tube)",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Drive::Direct => "direct",
            Drive::Bowden => "bowden",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name().eq_ignore_ascii_case(s.trim()))
    }

    fn retraction(self) -> (f64, f64) {
        match self {
            Drive::Direct => (0.8, 35.0),
            Drive::Bowden => (5.0, 45.0),
        }
    }
}

/// A known printer: a starting point for the assistant, from the build size
/// its maker publishes. Every answer can still be changed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrinterModel {
    pub name: &'static str,
    pub flavor: Flavor,
    pub round: bool,
    /// Width × depth × height, mm (a round bed: diameter × diameter × height).
    pub size: (f64, f64, f64),
    pub drive: Drive,
    pub origin_center: bool,
}

const fn model(
    name: &'static str,
    flavor: Flavor,
    size: (f64, f64, f64),
    drive: Drive,
) -> PrinterModel {
    PrinterModel { name, flavor, round: false, size, drive, origin_center: false }
}

/// Starting points, by published build volume. Our own table: names are the
/// makers' product names, numbers their specification sheets.
pub const PRINTERS: &[PrinterModel] = &[
    model(
        "Creality Ender-3 / Ender-3 Pro / Ender-3 V2",
        Flavor::Marlin,
        (220.0, 220.0, 250.0),
        Drive::Bowden,
    ),
    model("Creality Ender-3 S1", Flavor::Marlin, (220.0, 220.0, 270.0), Drive::Direct),
    model("Creality Ender-3 V3 SE", Flavor::Marlin, (220.0, 220.0, 250.0), Drive::Direct),
    model("Creality Ender-5", Flavor::Marlin, (220.0, 220.0, 300.0), Drive::Bowden),
    model("Creality CR-10", Flavor::Marlin, (300.0, 300.0, 400.0), Drive::Bowden),
    model("Prusa i3 MK3S+", Flavor::Marlin, (250.0, 210.0, 210.0), Drive::Direct),
    model("Prusa MINI+", Flavor::Marlin, (180.0, 180.0, 180.0), Drive::Bowden),
    model("Artillery Sidewinder X1", Flavor::Marlin, (300.0, 300.0, 400.0), Drive::Direct),
    model("Elegoo Neptune 3 Pro", Flavor::Marlin, (225.0, 225.0, 280.0), Drive::Direct),
    model("Voron 2.4 (350)", Flavor::Klipper, (350.0, 350.0, 340.0), Drive::Direct),
    PrinterModel {
        name: "Generic delta (round bed)",
        flavor: Flavor::Marlin,
        round: true,
        size: (200.0, 200.0, 300.0),
        drive: Drive::Bowden,
        origin_center: true,
    },
];

/// What the assistant asks.
#[derive(Debug, Clone, PartialEq)]
pub struct PrinterSetup {
    pub name: String,
    pub flavor: Flavor,
    pub bed_shape: BedShape,
    /// Rectangle: width × depth; circle: `bed_width` is the diameter.
    pub bed_width: f64,
    pub bed_depth: f64,
    pub bed_height: f64,
    /// Coordinates are measured from the bed's centre (most round printers).
    pub origin_center: bool,
    pub heated_bed: bool,
    pub nozzle_diameter: f64,
    pub filament_diameter: f64,
    pub drive: Drive,
    pub material: Material,
}

impl Default for PrinterSetup {
    fn default() -> Self {
        Self {
            name: "My printer".into(),
            flavor: Flavor::Marlin,
            bed_shape: BedShape::Rectangle,
            bed_width: 220.0,
            bed_depth: 220.0,
            bed_height: 250.0,
            origin_center: false,
            heated_bed: true,
            nozzle_diameter: 0.4,
            filament_diameter: 1.75,
            drive: Drive::Direct,
            material: Material::Pla,
        }
    }
}

fn within(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && v >= lo && v <= hi
}

impl PrinterSetup {
    /// Start from a known printer, keeping the material.
    pub fn from_model(m: &PrinterModel, material: Material) -> Self {
        Self {
            name: m.name.to_string(),
            flavor: m.flavor,
            bed_shape: if m.round { BedShape::Circle } else { BedShape::Rectangle },
            bed_width: m.size.0,
            bed_depth: m.size.1,
            bed_height: m.size.2,
            origin_center: m.origin_center,
            drive: m.drive,
            material,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), DocError> {
        if self.name.trim().is_empty() || self.name.len() > 120 {
            return Err(DocError::Setting("the printer needs a name of up to 120 characters"));
        }
        if !within(self.bed_width, 20.0, 2000.0) || !within(self.bed_height, 20.0, 2000.0) {
            return Err(DocError::Setting("bed sizes are between 20 and 2000 mm"));
        }
        if self.bed_shape == BedShape::Rectangle && !within(self.bed_depth, 20.0, 2000.0) {
            return Err(DocError::Setting("bed sizes are between 20 and 2000 mm"));
        }
        if !within(self.nozzle_diameter, 0.1, 2.0) {
            return Err(DocError::Setting("a nozzle is between 0.1 and 2 mm"));
        }
        if !within(self.filament_diameter, 1.0, 3.5) {
            return Err(DocError::Setting("filament is between 1 and 3.5 mm"));
        }
        Ok(())
    }

    /// The profile these answers make.
    pub fn profile(&self) -> Result<Profile, DocError> {
        self.validate()?;
        let m = self.material.values();
        let d = self.nozzle_diameter;
        let (retract, retract_speed) = self.drive.retraction();
        let round = self.bed_shape == BedShape::Circle;
        let (w, depth) =
            if round { (self.bed_width, self.bed_width) } else { (self.bed_width, self.bed_depth) };
        // layers about half the nozzle, rounded to 0.02 mm
        let layer = ((d * 0.5) / 0.02).round() * 0.02;
        let bed = if self.heated_bed { m.bed } else { 0.0 };
        let base = PrintSettings::default();
        let speed = |v: f64| (v * m.speed_scale).max(5.0);
        let settings = PrintSettings {
            flavor: self.flavor,
            nozzle_diameter: d,
            extrusion_width_auto: true,
            layer_height: layer.clamp(0.04, d * 0.8),
            first_layer_height: (d * 0.6).clamp(0.1, d * 0.8),
            retract_length: retract * m.retract_scale,
            retract_speed: if self.material == Material::Tpu { 20.0 } else { retract_speed },
            nozzle_temperature: m.nozzle,
            first_layer_nozzle_temperature: m.nozzle + 5.0,
            bed_temperature: bed,
            first_layer_bed_temperature: bed,
            fan_percent: m.fan,
            filament_diameter: self.filament_diameter,
            filament_density: m.density,
            print_speed: speed(base.print_speed),
            perimeter_speed: speed(base.perimeter_speed),
            first_layer_speed: speed(base.first_layer_speed),
            bed_shape: self.bed_shape,
            bed_width: w,
            bed_depth: depth,
            bed_diameter: if round { self.bed_width } else { base.bed_diameter },
            bed_height: self.bed_height,
            origin_center: self.origin_center || round,
            start_script: self.start_script(w, depth, round),
            end_script: self.end_script(),
            ..base
        };
        settings.validate()?;
        Ok(Profile { name: format!("{} · {}", self.name.trim(), self.material.label()), settings })
    }

    /// Home, then lay a priming line along the front edge.
    fn start_script(&self, width: f64, depth: f64, round: bool) -> String {
        let centred = self.origin_center || round;
        let (x0, y0) = if centred { (-width / 2.0, -depth / 2.0) } else { (0.0, 0.0) };
        // a line 60 % across the front, 15 mm in from the edge
        let inset = if round { width * 0.18 } else { 15.0 };
        let len = (width * 0.6).min(150.0);
        let start_x = x0 + (width - len) / 2.0;
        let y = y0 + inset;
        let h = 0.3;
        let lane = self.nozzle_diameter * 1.5;
        let area = std::f64::consts::PI * (self.filament_diameter / 2.0).powi(2);
        let e = lane * h * len / area;
        let mut s = String::new();
        s.push_str("G21 ; millimetres\nG90 ; absolute coordinates\nM82 ; absolute extrusion\n");
        s.push_str("G28 ; home all axes\n");
        s.push_str("G92 E0\nG1 Z2 F600\n");
        s.push_str(&format!("G1 X{start_x:.1} Y{y:.1} F6000 ; to the priming line\n"));
        s.push_str(&format!("G1 Z{h:.2} F600\n"));
        s.push_str(&format!("G1 X{:.1} E{e:.3} F1200 ; priming line\n", start_x + len));
        s.push_str(&format!("G1 Y{:.1} F1200\n", y + lane));
        s.push_str(&format!("G1 X{start_x:.1} E{:.3} F1200\n", e * 2.0));
        s.push_str("G92 E0\nG1 Z2 F600");
        s
    }

    fn end_script(&self) -> String {
        let mut s = String::from("M104 S0 ; nozzle off\n");
        if self.heated_bed {
            s.push_str("M140 S0 ; bed off\n");
        }
        s.push_str("M107 ; fan off\nG91\nG1 E-2 F1800\nG1 Z10 F600 ; lift\nG90\nM84 ; motors off");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_setup_makes_a_valid_profile() {
        let p = PrinterSetup::default().profile();
        assert!(p.is_ok(), "{p:?}");
        let Ok(p) = p else { return };
        assert_eq!(p.settings.layer_height, 0.2);
        assert_eq!(p.settings.nozzle_temperature, 210.0);
        assert!(p.settings.start_script.contains("G28"));
        assert!(p.name.contains("PLA"));
    }

    #[test]
    fn answers_shape_the_profile() {
        let s = PrinterSetup {
            bed_shape: BedShape::Circle,
            bed_width: 180.0,
            nozzle_diameter: 0.6,
            drive: Drive::Bowden,
            material: Material::Petg,
            heated_bed: false,
            flavor: Flavor::Klipper,
            ..PrinterSetup::default()
        };
        let Ok(p) = s.profile() else { return };
        let st = &p.settings;
        assert!(st.origin_center);
        assert_eq!(st.bed_shape, BedShape::Circle);
        assert_eq!(st.bed_diameter, 180.0);
        assert_eq!(st.bed_temperature, 0.0);
        assert_eq!(st.nozzle_temperature, 240.0);
        assert!((st.layer_height - 0.3).abs() < 1e-9);
        assert!(st.retract_length > 4.0);
        assert!(!st.end_script.contains("M140"));
        // the priming line stays on a round bed: within the radius
        assert!(st.start_script.contains("X-54.0"), "{}", st.start_script);
    }

    #[test]
    fn every_known_printer_makes_a_valid_profile() {
        assert!(PRINTERS.len() >= 10);
        for m in PRINTERS {
            let p = PrinterSetup::from_model(m, Material::Pla).profile();
            assert!(p.is_ok(), "{}: {p:?}", m.name);
        }
    }

    #[test]
    fn bad_answers_are_refused() {
        for s in [
            PrinterSetup { bed_width: f64::NAN, ..PrinterSetup::default() },
            PrinterSetup { nozzle_diameter: 0.0, ..PrinterSetup::default() },
            PrinterSetup { filament_diameter: 9.0, ..PrinterSetup::default() },
            PrinterSetup { name: " ".into(), ..PrinterSetup::default() },
        ] {
            assert!(s.profile().is_err());
        }
        let tpu = PrinterSetup { material: Material::Tpu, ..PrinterSetup::default() };
        assert!(tpu.profile().is_ok_and(|p| p.settings.print_speed < 40.0));
    }
}
