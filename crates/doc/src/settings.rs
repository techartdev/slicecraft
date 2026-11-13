//! The print profile and its schema.
//!
//! Every setting is described once in `SETTINGS`: key, label, tab, section,
//! kind, range, default and help. Front ends render the profile editor from
//! this table, profile files are read and written through it, and validation
//! is driven by it, so a setting cannot exist in one place only.
//!
//! A setting the slicer already uses is *bound* to a field of `PrintSettings`.
//! Every other setting is a *placeholder*: it is stored, validated, saved and
//! shown, and marked as not used yet, until the stage that uses it lands.
//!
//! Keys, labels, defaults, ranges and help text are SliceCraft's own. The
//! feature inventory they cover is in `docs/research/02-feature-inventory.md`.

use crate::DocError;
use std::collections::BTreeMap;

/// The firmware family the G-code is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Marlin,
    RepRap,
    Klipper,
}

impl Flavor {
    pub const ALL: [Flavor; 3] = [Flavor::Marlin, Flavor::RepRap, Flavor::Klipper];
    const NAMES: &'static [&'static str] = &["marlin", "reprap", "klipper"];

    pub fn name(self) -> &'static str {
        match self {
            Flavor::Marlin => "marlin",
            Flavor::RepRap => "reprap",
            Flavor::Klipper => "klipper",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name().eq_ignore_ascii_case(name))
    }
}

/// Sparse infill pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfillPattern {
    Rectilinear,
    /// Parallel lines in one direction on every layer.
    Lines,
    Grid,
    Triangles,
    Concentric,
    Honeycomb,
    Gyroid,
    Wiggle,
    Cubic,
    Octahedral,
}

impl InfillPattern {
    pub const ALL: [InfillPattern; 10] = [
        InfillPattern::Rectilinear,
        InfillPattern::Lines,
        InfillPattern::Grid,
        InfillPattern::Triangles,
        InfillPattern::Concentric,
        InfillPattern::Honeycomb,
        InfillPattern::Gyroid,
        InfillPattern::Wiggle,
        InfillPattern::Cubic,
        InfillPattern::Octahedral,
    ];
    const NAMES: &'static [&'static str] = &[
        "rectilinear",
        "lines (one direction)",
        "grid",
        "triangles",
        "concentric",
        "honeycomb",
        "gyroid",
        "wiggle",
        "cubic",
        "octahedral",
    ];

    /// Patterns the slicer draws today; the others fall back to rectilinear.
    pub fn is_implemented(self) -> bool {
        matches!(
            self,
            InfillPattern::Rectilinear
                | InfillPattern::Lines
                | InfillPattern::Grid
                | InfillPattern::Triangles
                | InfillPattern::Concentric
        )
    }
}

/// Pattern for solid (top, bottom) infill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolidPattern {
    Rectilinear,
    /// One direction on every layer.
    Lines,
    Concentric,
}

impl SolidPattern {
    pub const ALL: [SolidPattern; 3] =
        [SolidPattern::Rectilinear, SolidPattern::Lines, SolidPattern::Concentric];
    const NAMES: &'static [&'static str] = &["rectilinear", "lines (one direction)", "concentric"];
}

/// Which perimeter is printed first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerimeterOrder {
    InsideOut,
    OutsideIn,
}

impl PerimeterOrder {
    pub const ALL: [PerimeterOrder; 2] = [PerimeterOrder::InsideOut, PerimeterOrder::OutsideIn];
    const NAMES: &'static [&'static str] = &["inside first", "outside first"];
}

/// Where closed perimeters start (the seam).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeamMode {
    /// Wherever the nozzle is closest: least travel, scattered seams.
    Nearest,
    /// The point nearest `seam_x`, `seam_y`: seams line up.
    Aligned,
    /// A different place on every layer: seams are spread out.
    Random,
}

impl SeamMode {
    pub const ALL: [SeamMode; 3] = [SeamMode::Nearest, SeamMode::Aligned, SeamMode::Random];
    const NAMES: &'static [&'static str] = &["nearest", "aligned to a point", "scattered"];
}

/// Where support may stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportPlacement {
    /// On the bed and on the model.
    Everywhere,
    /// Only columns that reach the bed.
    BedOnly,
}

impl SupportPlacement {
    pub const ALL: [SupportPlacement; 2] =
        [SupportPlacement::Everywhere, SupportPlacement::BedOnly];
    const NAMES: &'static [&'static str] = &["everywhere", "from the bed only"];
}

/// The order islands (separate parts of a layer) are printed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandOrder {
    /// Finish one island, then go to the nearest next one: fewest travels.
    Nearest,
    /// The same order on every layer (left to right, front to back).
    Consistent,
    /// Each feature on every island before the next feature.
    ByFeature,
}

impl IslandOrder {
    pub const ALL: [IslandOrder; 3] =
        [IslandOrder::Nearest, IslandOrder::Consistent, IslandOrder::ByFeature];
}

/// How support areas are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportStyle {
    /// Separate square pillars on a grid: less contact, each snaps off alone.
    Pillars,
    /// The exact area under the overhang: holds every point of the surface.
    Areas,
}

impl SupportStyle {
    pub const ALL: [SupportStyle; 2] = [SupportStyle::Pillars, SupportStyle::Areas];
    const NAMES: &'static [&'static str] = &["pillars", "areas"];
}

/// Shape of the support body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportPattern {
    /// Parallel lines in one direction, stacked on every layer.
    Lines,
    /// Lines in two directions on every layer.
    Grid,
}

impl SupportPattern {
    pub const ALL: [SupportPattern; 2] = [SupportPattern::Lines, SupportPattern::Grid];
    const NAMES: &'static [&'static str] = &["lines", "grid"];
}

/// The print profile: the settings the slicer uses as typed fields, and every
/// other setting in `extra`.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintSettings {
    pub flavor: Flavor,
    // extruder
    pub nozzle_diameter: f64,
    pub extrusion_width_auto: bool,
    pub extrusion_width: f64,
    pub extrusion_multiplier: f64,
    pub retract_enabled: bool,
    pub retract_length: f64,
    pub retract_speed: f64,
    pub retract_lift: f64,
    /// Travel moves shorter than this do not retract, mm.
    pub retract_min_travel: f64,
    pub retract_on_layer_change: bool,
    /// Retract only on travel that crosses the model's outline.
    pub retract_only_outside: bool,
    /// Route travel around holes inside the part instead of across them.
    pub avoid_crossing_walls: bool,
    /// Longest combed route, relative to the straight one.
    pub travel_detour_limit: f64,
    // layer
    pub layer_height: f64,
    pub first_layer_height: f64,
    pub perimeter_count: u32,
    pub solid_layers_top: u32,
    pub solid_layers_bottom: u32,
    pub perimeter_order: PerimeterOrder,
    pub island_order: IslandOrder,
    pub seam_mode: SeamMode,
    pub seam_x: f64,
    pub seam_y: f64,
    // adhesion
    pub skirt_enabled: bool,
    pub skirt_loops: u32,
    /// Gap between the part and the skirt, mm.
    pub skirt_distance: f64,
    pub skirt_layers: u32,
    pub brim_enabled: bool,
    pub brim_loops: u32,
    // infill
    pub infill_percentage: f64,
    pub infill_pattern: InfillPattern,
    pub solid_pattern: SolidPattern,
    /// Infill line direction, degrees.
    pub infill_angle: f64,
    /// Turn the infill 90° on every other layer.
    pub infill_alternate: bool,
    /// How far infill reaches into the innermost perimeter, % of the width.
    pub infill_overlap: f64,
    pub infill_before_perimeters: bool,
    // temperature
    pub nozzle_temperature: f64,
    pub bed_temperature: f64,
    pub first_layer_nozzle_temperature: f64,
    pub first_layer_bed_temperature: f64,
    pub wait_for_temperatures: bool,
    // cooling
    /// Part cooling fan duty, %.
    pub fan_percent: f64,
    /// First layer number (1-based) on which the fan runs.
    pub fan_from_layer: u32,
    // speeds
    /// Infill and general extrusion speed, mm/s.
    pub print_speed: f64,
    pub perimeter_speed: f64,
    pub outer_perimeter_speed_percent: f64,
    pub solid_infill_speed_percent: f64,
    pub first_layer_speed: f64,
    pub travel_speed: f64,
    pub z_speed: f64,
    // scripts
    pub start_script: String,
    pub end_script: String,
    pub layer_script: String,
    // machine
    pub bed_width: f64,
    pub bed_depth: f64,
    pub bed_height: f64,
    // material
    pub filament_diameter: f64,
    /// g/cm³
    pub filament_density: f64,
    /// Price per kilogram, in the user's currency.
    pub filament_cost: f64,
    // output
    /// Added to every X, Y and Z written, mm (fine-tunes the first layer).
    pub gcode_offset_x: f64,
    pub gcode_offset_y: f64,
    pub gcode_offset_z: f64,
    // support
    pub support_enabled: bool,
    pub support_placement: SupportPlacement,
    pub support_style: SupportStyle,
    /// Side of one pillar's grid cell, mm.
    pub support_pillar_size: f64,
    /// Surfaces flatter than this, measured from vertical, get support. Degrees.
    pub support_angle: f64,
    /// Density of the support body, %.
    pub support_infill: f64,
    pub support_pattern: SupportPattern,
    /// Horizontal gap between support and model, mm.
    pub support_xy_gap: f64,
    /// Empty layers between the top of support and the model above it.
    pub support_top_gap_layers: u32,
    /// Empty layers between the model and support standing on it.
    pub support_bottom_gap_layers: u32,
    /// Dense layers directly under supported surfaces.
    pub support_interface_layers: u32,
    /// Density of those layers, %.
    pub support_interface_infill: f64,
    /// Dense layers where support stands on the model.
    pub support_floor_layers: u32,
    /// Solid layers at the foot of support on the bed.
    pub support_base_layers: u32,
    /// Grows (or shrinks) every supported area, mm.
    pub support_expansion: f64,
    /// Perimeters around support areas.
    pub support_outlines: u32,
    /// Support speed, % of the default speed.
    pub support_speed_percent: f64,
    /// Placeholder settings, by key. Missing keys read as their default.
    pub extra: BTreeMap<String, SettingValue>,
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self {
            flavor: Flavor::Marlin,
            nozzle_diameter: 0.4,
            extrusion_width_auto: false,
            extrusion_width: 0.45,
            extrusion_multiplier: 1.0,
            retract_enabled: true,
            retract_length: 1.0,
            retract_speed: 35.0,
            retract_lift: 0.0,
            retract_min_travel: 2.0,
            retract_on_layer_change: true,
            retract_only_outside: true,
            avoid_crossing_walls: true,
            travel_detour_limit: 3.0,
            layer_height: 0.2,
            first_layer_height: 0.3,
            perimeter_count: 2,
            solid_layers_top: 4,
            solid_layers_bottom: 3,
            perimeter_order: PerimeterOrder::InsideOut,
            island_order: IslandOrder::Nearest,
            seam_mode: SeamMode::Nearest,
            seam_x: 0.0,
            seam_y: 300.0,
            skirt_enabled: true,
            skirt_loops: 1,
            skirt_distance: 4.0,
            skirt_layers: 1,
            brim_enabled: false,
            brim_loops: 5,
            infill_percentage: 20.0,
            infill_pattern: InfillPattern::Rectilinear,
            solid_pattern: SolidPattern::Rectilinear,
            infill_angle: 45.0,
            infill_alternate: true,
            infill_overlap: 15.0,
            infill_before_perimeters: false,
            nozzle_temperature: 210.0,
            bed_temperature: 60.0,
            first_layer_nozzle_temperature: 210.0,
            first_layer_bed_temperature: 60.0,
            wait_for_temperatures: true,
            fan_percent: 100.0,
            fan_from_layer: 2,
            print_speed: 60.0,
            perimeter_speed: 40.0,
            outer_perimeter_speed_percent: 100.0,
            solid_infill_speed_percent: 100.0,
            first_layer_speed: 20.0,
            travel_speed: 150.0,
            z_speed: 10.0,
            start_script: String::new(),
            end_script: String::new(),
            layer_script: String::new(),
            bed_width: 220.0,
            bed_depth: 220.0,
            bed_height: 250.0,
            filament_diameter: 1.75,
            filament_density: 1.24,
            filament_cost: 20.0,
            gcode_offset_x: 0.0,
            gcode_offset_y: 0.0,
            gcode_offset_z: 0.0,
            support_enabled: false,
            support_placement: SupportPlacement::Everywhere,
            support_style: SupportStyle::Pillars,
            support_pillar_size: 2.0,
            support_angle: 45.0,
            support_infill: 25.0,
            support_pattern: SupportPattern::Lines,
            support_xy_gap: 0.4,
            support_top_gap_layers: 1,
            support_bottom_gap_layers: 1,
            support_interface_layers: 0,
            support_interface_infill: 70.0,
            support_floor_layers: 0,
            support_base_layers: 0,
            support_expansion: 0.0,
            support_outlines: 0,
            support_speed_percent: 100.0,
            extra: BTreeMap::new(),
        }
    }
}

/// The tabs of the profile editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tab {
    Extruder,
    Layer,
    Adhesion,
    Infill,
    Support,
    Temperature,
    Cooling,
    Speeds,
    Gcode,
    Scripts,
    Machine,
    Material,
    Advanced,
}

impl Tab {
    pub const ALL: [Tab; 13] = [
        Tab::Extruder,
        Tab::Layer,
        Tab::Adhesion,
        Tab::Infill,
        Tab::Support,
        Tab::Temperature,
        Tab::Cooling,
        Tab::Speeds,
        Tab::Gcode,
        Tab::Scripts,
        Tab::Machine,
        Tab::Material,
        Tab::Advanced,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Extruder => "Extruder",
            Tab::Layer => "Layers",
            Tab::Adhesion => "Adhesion",
            Tab::Infill => "Infill",
            Tab::Support => "Support",
            Tab::Temperature => "Temperature",
            Tab::Cooling => "Cooling",
            Tab::Speeds => "Speeds",
            Tab::Gcode => "G-code",
            Tab::Scripts => "Scripts",
            Tab::Machine => "Machine",
            Tab::Material => "Material",
            Tab::Advanced => "Advanced",
        }
    }
}

/// What kind of value a setting holds, with its bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingKind {
    Float {
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
    },
    Int {
        min: i64,
        max: i64,
        unit: &'static str,
    },
    Bool,
    Choice(&'static [&'static str]),
    /// Free text, such as a G-code script.
    Text,
}

/// A setting's value.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    /// Index into the setting's option list.
    Choice(usize),
    Text(String),
}

/// A default, in a form that can live in a `static`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Def {
    F(f64),
    I(i64),
    B(bool),
    C(usize),
    T(&'static str),
}

impl Def {
    pub fn value(self) -> SettingValue {
        match self {
            Def::F(v) => SettingValue::Float(v),
            Def::I(v) => SettingValue::Int(v),
            Def::B(v) => SettingValue::Bool(v),
            Def::C(v) => SettingValue::Choice(v),
            Def::T(v) => SettingValue::Text(v.to_string()),
        }
    }
}

/// How a bound setting reads and writes its field.
#[derive(Clone, Copy)]
pub struct Binding {
    pub get: fn(&PrintSettings) -> SettingValue,
    /// Returns false when the value has the wrong kind.
    pub set: fn(&mut PrintSettings, &SettingValue) -> bool,
}

impl std::fmt::Debug for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Binding")
    }
}

/// One row of the schema.
#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub tab: Tab,
    pub section: &'static str,
    pub kind: SettingKind,
    pub default: Def,
    pub help: &'static str,
    /// `Some` when the slicer uses the setting today.
    pub binding: Option<Binding>,
}

impl SettingSpec {
    pub fn implemented(&self) -> bool {
        self.binding.is_some()
    }

    /// Check that `v` has this setting's kind and is inside its range.
    pub fn check(&self, v: &SettingValue) -> Result<(), DocError> {
        let bad = |why: String| Err(DocError::InvalidSetting(format!("{}: {why}", self.label)));
        match (self.kind, v) {
            (SettingKind::Float { min, max, unit, .. }, SettingValue::Float(x)) => {
                if !x.is_finite() {
                    return bad("must be a number".into());
                }
                if *x < min || *x > max {
                    return bad(format!("must be between {min} and {max} {unit}"));
                }
                Ok(())
            }
            (SettingKind::Int { min, max, unit }, SettingValue::Int(x)) => {
                if *x < min || *x > max {
                    return bad(format!("must be between {min} and {max} {unit}"));
                }
                Ok(())
            }
            (SettingKind::Bool, SettingValue::Bool(_)) => Ok(()),
            (SettingKind::Choice(opts), SettingValue::Choice(i)) => {
                if *i < opts.len() {
                    Ok(())
                } else {
                    bad("is not one of its options".into())
                }
            }
            (SettingKind::Text, SettingValue::Text(t)) => {
                if t.len() > MAX_TEXT_BYTES {
                    bad(format!("is longer than {MAX_TEXT_BYTES} bytes"))
                } else {
                    Ok(())
                }
            }
            _ => bad("has the wrong kind of value".into()),
        }
    }
}

/// Longest text (script) setting, in bytes.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;

/// Look up a setting by key.
pub fn spec(key: &str) -> Option<&'static SettingSpec> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// Settings of one tab, in table order.
pub fn tab_settings(tab: Tab) -> impl Iterator<Item = &'static SettingSpec> {
    SETTINGS.iter().filter(move |s| s.tab == tab)
}

impl PrintSettings {
    /// The current value of `key` (its default when never set).
    pub fn get(&self, key: &str) -> Option<SettingValue> {
        let s = spec(key)?;
        Some(match s.binding {
            Some(b) => (b.get)(self),
            None => self.extra.get(key).cloned().unwrap_or_else(|| s.default.value()),
        })
    }

    /// Set `key` after checking kind and range.
    pub fn set(&mut self, key: &str, value: SettingValue) -> Result<(), DocError> {
        let s =
            spec(key).ok_or_else(|| DocError::InvalidSetting(format!("unknown setting {key}")))?;
        s.check(&value)?;
        match s.binding {
            Some(b) => {
                if !(b.set)(self, &value) {
                    return Err(DocError::InvalidSetting(format!("{}: wrong kind", s.label)));
                }
            }
            None => {
                if value == s.default.value() {
                    self.extra.remove(key);
                } else {
                    self.extra.insert(key.to_string(), value);
                }
            }
        }
        Ok(())
    }

    /// Every value must be printable: each setting inside its range, and the
    /// extra map holding only known placeholder keys.
    pub fn validate(&self) -> Result<(), DocError> {
        for s in SETTINGS {
            if let Some(v) = self.get(s.key) {
                s.check(&v)?;
            }
        }
        for key in self.extra.keys() {
            match spec(key) {
                Some(s) if !s.implemented() => {}
                _ => {
                    return Err(DocError::InvalidSetting(format!(
                        "{key} is not a placeholder setting"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Placeholder settings that differ from their defaults: they are kept,
    /// but the slicer ignores them for now.
    pub fn unused_changes(&self) -> Vec<&'static str> {
        SETTINGS
            .iter()
            .filter(|s| !s.implemented() && self.extra.contains_key(s.key))
            .map(|s| s.label)
            .collect()
    }

    /// The line width actually used: automatic follows the nozzle.
    pub fn line_width(&self) -> f64 {
        if self.extrusion_width_auto { self.nozzle_diameter * 1.125 } else { self.extrusion_width }
    }

    /// Effective retraction length (zero when retraction is off).
    pub fn retraction(&self) -> f64 {
        if self.retract_enabled { self.retract_length } else { 0.0 }
    }
}

// ---------------------------------------------------------------- bindings

macro_rules! bind_f {
    ($field:ident) => {
        Some(Binding {
            get: |s| SettingValue::Float(s.$field),
            set: |s, v| match v {
                SettingValue::Float(x) => {
                    s.$field = *x;
                    true
                }
                _ => false,
            },
        })
    };
}

macro_rules! bind_u {
    ($field:ident) => {
        Some(Binding {
            get: |s| SettingValue::Int(i64::from(s.$field)),
            set: |s, v| match v {
                SettingValue::Int(x) => match u32::try_from(*x) {
                    Ok(n) => {
                        s.$field = n;
                        true
                    }
                    Err(_) => false,
                },
                _ => false,
            },
        })
    };
}

macro_rules! bind_b {
    ($field:ident) => {
        Some(Binding {
            get: |s| SettingValue::Bool(s.$field),
            set: |s, v| match v {
                SettingValue::Bool(x) => {
                    s.$field = *x;
                    true
                }
                _ => false,
            },
        })
    };
}

macro_rules! bind_t {
    ($field:ident) => {
        Some(Binding {
            get: |s| SettingValue::Text(s.$field.clone()),
            set: |s, v| match v {
                SettingValue::Text(x) => {
                    s.$field = x.clone();
                    true
                }
                _ => false,
            },
        })
    };
}

macro_rules! bind_e {
    ($field:ident, $ty:ident) => {
        Some(Binding {
            get: |s| {
                SettingValue::Choice($ty::ALL.iter().position(|x| *x == s.$field).unwrap_or(0))
            },
            set: |s, v| match v {
                SettingValue::Choice(i) => match $ty::ALL.get(*i) {
                    Some(x) => {
                        s.$field = *x;
                        true
                    }
                    None => false,
                },
                _ => false,
            },
        })
    };
}

// ---------------------------------------------------------------- the table

const fn f(min: f64, max: f64, step: f64, unit: &'static str) -> SettingKind {
    SettingKind::Float { min, max, step, unit }
}

const fn i(min: i64, max: i64, unit: &'static str) -> SettingKind {
    SettingKind::Int { min, max, unit }
}

const B: SettingKind = SettingKind::Bool;

#[allow(clippy::too_many_arguments)]
const fn row(
    key: &'static str,
    label: &'static str,
    tab: Tab,
    section: &'static str,
    kind: SettingKind,
    default: Def,
    help: &'static str,
    binding: Option<Binding>,
) -> SettingSpec {
    SettingSpec { key, label, tab, section, kind, default, help, binding }
}

use Def::{B as Db, C as Dc, F as Df, I as Di, T as Dt};
use Tab::*;

const MM: &str = "mm";
const MMS: &str = "mm/s";
const PCT: &str = "%";
const DEG: &str = "°";
const CEL: &str = "°C";

/// The whole schema, grouped by tab and section in display order.
pub static SETTINGS: &[SettingSpec] = &[
    // ----------------------------------------------------------- Extruder
    row(
        "nozzle_diameter",
        "Nozzle diameter",
        Extruder,
        "Nozzle",
        f(0.1, 2.0, 0.05, MM),
        Df(0.4),
        "Bore of the nozzle; sets the automatic line width.",
        bind_f!(nozzle_diameter),
    ),
    row(
        "extrusion_width_auto",
        "Automatic line width",
        Extruder,
        "Nozzle",
        B,
        Db(false),
        "Derive the line width from the nozzle diameter.",
        bind_b!(extrusion_width_auto),
    ),
    row(
        "extrusion_width",
        "Line width",
        Extruder,
        "Nozzle",
        f(0.1, 3.0, 0.01, MM),
        Df(0.45),
        "Width of one extruded line when automatic width is off.",
        bind_f!(extrusion_width),
    ),
    row(
        "extrusion_multiplier",
        "Flow multiplier",
        Extruder,
        "Nozzle",
        f(0.1, 5.0, 0.01, ""),
        Df(1.0),
        "Scales all extrusion; tune it per filament.",
        bind_f!(extrusion_multiplier),
    ),
    row(
        "tool_number",
        "Tool number",
        Extruder,
        "Nozzle",
        i(0, 15, ""),
        Di(0),
        "Which tool this extruder drives on a multi-tool printer.",
        None,
    ),
    row(
        "retract_enabled",
        "Retract",
        Extruder,
        "Retraction",
        B,
        Db(true),
        "Pull filament back before travel moves.",
        bind_b!(retract_enabled),
    ),
    row(
        "retract_length",
        "Retraction length",
        Extruder,
        "Retraction",
        f(0.0, 20.0, 0.1, MM),
        Df(1.0),
        "How much filament to pull back.",
        bind_f!(retract_length),
    ),
    row(
        "retract_speed",
        "Retraction speed",
        Extruder,
        "Retraction",
        f(1.0, 200.0, 1.0, MMS),
        Df(35.0),
        "Speed of the retract and of the restore.",
        bind_f!(retract_speed),
    ),
    row(
        "retract_extra_restart",
        "Extra restart length",
        Extruder,
        "Retraction",
        f(-5.0, 5.0, 0.05, MM),
        Df(0.0),
        "Push more (or less) than was retracted when resuming.",
        None,
    ),
    row(
        "retract_lift",
        "Z lift on retraction",
        Extruder,
        "Retraction",
        f(0.0, 10.0, 0.05, MM),
        Df(0.0),
        "Raise the nozzle during retracted travel (Z hop).",
        bind_f!(retract_lift),
    ),
    row(
        "retract_min_travel",
        "Retract only after",
        Extruder,
        "Retraction",
        f(0.0, 100.0, 0.1, MM),
        Df(2.0),
        "Shorter travel moves do not retract.",
        bind_f!(retract_min_travel),
    ),
    row(
        "retract_on_layer_change",
        "Retract on layer change",
        Extruder,
        "Retraction",
        B,
        Db(true),
        "Retract before every move to the next layer.",
        bind_b!(retract_on_layer_change),
    ),
    row(
        "coast_enabled",
        "Coast at path end",
        Extruder,
        "Ooze control",
        B,
        Db(false),
        "Stop extruding a little before the end of each path.",
        None,
    ),
    row(
        "coast_distance",
        "Coasting distance",
        Extruder,
        "Ooze control",
        f(0.0, 5.0, 0.05, MM),
        Df(0.2),
        "Length of path printed without extrusion.",
        None,
    ),
    row(
        "wipe_enabled",
        "Wipe",
        Extruder,
        "Ooze control",
        B,
        Db(false),
        "Move along the finished path after retracting.",
        None,
    ),
    row(
        "wipe_distance",
        "Wipe distance",
        Extruder,
        "Ooze control",
        f(0.0, 20.0, 0.1, MM),
        Df(4.0),
        "Length of the wipe move.",
        None,
    ),
    // ----------------------------------------------------------- Layer
    row(
        "layer_height",
        "Layer height",
        Layer,
        "Layers",
        f(0.01, 2.0, 0.01, MM),
        Df(0.2),
        "Thickness of every layer after the first.",
        bind_f!(layer_height),
    ),
    row(
        "first_layer_height",
        "First layer height",
        Layer,
        "Layers",
        f(0.01, 2.0, 0.01, MM),
        Df(0.3),
        "A thicker first layer forgives an uneven bed.",
        bind_f!(first_layer_height),
    ),
    row(
        "first_layer_width_percent",
        "First layer width",
        Layer,
        "Layers",
        f(50.0, 300.0, 1.0, PCT),
        Df(100.0),
        "Line width on the first layer, relative to the normal width.",
        None,
    ),
    row(
        "adaptive_layers",
        "Adaptive layer height",
        Layer,
        "Layers",
        B,
        Db(false),
        "Thinner layers where the surface slopes gently.",
        None,
    ),
    row(
        "perimeter_count",
        "Perimeters",
        Layer,
        "Walls",
        i(0, 50, ""),
        Di(2),
        "Number of outline shells on every layer.",
        bind_u!(perimeter_count),
    ),
    row(
        "perimeter_order",
        "Perimeter order",
        Layer,
        "Walls",
        SettingKind::Choice(PerimeterOrder::NAMES),
        Dc(0),
        "Print the outer wall last (accurate) or first (clean surface).",
        bind_e!(perimeter_order, PerimeterOrder),
    ),
    row(
        "solid_layers_top",
        "Top solid layers",
        Layer,
        "Solid layers",
        i(0, 100, ""),
        Di(4),
        "Solid layers under every upward-facing surface.",
        bind_u!(solid_layers_top),
    ),
    row(
        "solid_layers_bottom",
        "Bottom solid layers",
        Layer,
        "Solid layers",
        i(0, 100, ""),
        Di(3),
        "Solid layers over every downward-facing surface.",
        bind_u!(solid_layers_bottom),
    ),
    row(
        "seam_mode",
        "Seam placement",
        Layer,
        "Seam",
        SettingKind::Choice(SeamMode::NAMES),
        Dc(0),
        "Where each closed perimeter starts and ends.",
        bind_e!(seam_mode, SeamMode),
    ),
    row(
        "seam_x",
        "Seam point X",
        Layer,
        "Seam",
        f(-10_000.0, 10_000.0, 1.0, MM),
        Df(0.0),
        "Seams gather near this point when aligned.",
        bind_f!(seam_x),
    ),
    row(
        "seam_y",
        "Seam point Y",
        Layer,
        "Seam",
        f(-10_000.0, 10_000.0, 1.0, MM),
        Df(300.0),
        "Seams gather near this point when aligned.",
        bind_f!(seam_y),
    ),
    row(
        "vase_mode",
        "Spiral outer wall",
        Layer,
        "Special modes",
        B,
        Db(false),
        "One continuous rising wall, no top, no infill (vase mode).",
        None,
    ),
    // ----------------------------------------------------------- Adhesion
    row(
        "skirt_enabled",
        "Skirt",
        Adhesion,
        "Skirt",
        B,
        Db(true),
        "Loops around the part to prime the nozzle.",
        bind_b!(skirt_enabled),
    ),
    row(
        "skirt_loops",
        "Skirt loops",
        Adhesion,
        "Skirt",
        i(1, 20, ""),
        Di(1),
        "How many loops.",
        bind_u!(skirt_loops),
    ),
    row(
        "skirt_distance",
        "Skirt distance",
        Adhesion,
        "Skirt",
        f(0.0, 100.0, 0.5, MM),
        Df(4.0),
        "Gap between the part and the skirt.",
        bind_f!(skirt_distance),
    ),
    row(
        "skirt_layers",
        "Skirt layers",
        Adhesion,
        "Skirt",
        i(1, 100, ""),
        Di(1),
        "Layers the skirt rises; a tall skirt shields drafts.",
        bind_u!(skirt_layers),
    ),
    row(
        "brim_enabled",
        "Brim",
        Adhesion,
        "Brim",
        B,
        Db(false),
        "Loops attached to the part's first layer for grip.",
        bind_b!(brim_enabled),
    ),
    row(
        "brim_loops",
        "Brim loops",
        Adhesion,
        "Brim",
        i(1, 100, ""),
        Di(5),
        "How many loops.",
        bind_u!(brim_loops),
    ),
    row(
        "raft_enabled",
        "Raft",
        Adhesion,
        "Raft",
        B,
        Db(false),
        "Print the part on a disposable lattice.",
        None,
    ),
    row(
        "raft_base_layers",
        "Raft base layers",
        Adhesion,
        "Raft",
        i(1, 10, ""),
        Di(2),
        "Thick, wide layers at the bottom of the raft.",
        None,
    ),
    row(
        "raft_top_layers",
        "Raft top layers",
        Adhesion,
        "Raft",
        i(1, 10, ""),
        Di(2),
        "Dense layers the part sits on.",
        None,
    ),
    row(
        "raft_margin",
        "Raft margin",
        Adhesion,
        "Raft",
        f(0.0, 30.0, 0.5, MM),
        Df(3.0),
        "How far the raft extends past the part.",
        None,
    ),
    row(
        "raft_gap",
        "Raft separation",
        Adhesion,
        "Raft",
        f(0.0, 2.0, 0.01, MM),
        Df(0.15),
        "Air gap between raft and part.",
        None,
    ),
    row(
        "raft_infill",
        "Raft infill",
        Adhesion,
        "Raft",
        f(1.0, 100.0, 1.0, PCT),
        Df(85.0),
        "Density of the raft layers.",
        None,
    ),
    row(
        "prime_tower_enabled",
        "Prime tower",
        Adhesion,
        "Prime tower",
        B,
        Db(false),
        "A tower that primes each tool after a change.",
        None,
    ),
    row(
        "prime_tower_size",
        "Prime tower size",
        Adhesion,
        "Prime tower",
        f(2.0, 100.0, 1.0, MM),
        Df(12.0),
        "Side of the tower.",
        None,
    ),
    row(
        "prime_tower_x",
        "Prime tower X",
        Adhesion,
        "Prime tower",
        f(-10_000.0, 10_000.0, 1.0, MM),
        Df(20.0),
        "Where the tower stands.",
        None,
    ),
    row(
        "prime_tower_y",
        "Prime tower Y",
        Adhesion,
        "Prime tower",
        f(-10_000.0, 10_000.0, 1.0, MM),
        Df(20.0),
        "Where the tower stands.",
        None,
    ),
    row(
        "ooze_shield_enabled",
        "Ooze shield",
        Adhesion,
        "Ooze shield",
        B,
        Db(false),
        "A thin wall around the part that catches ooze from idle tools.",
        None,
    ),
    row(
        "ooze_shield_distance",
        "Ooze shield distance",
        Adhesion,
        "Ooze shield",
        f(0.5, 50.0, 0.5, MM),
        Df(2.0),
        "Gap between the part and the shield.",
        None,
    ),
    row(
        "ooze_shield_angle",
        "Ooze shield slope",
        Adhesion,
        "Ooze shield",
        f(0.0, 89.0, 1.0, DEG),
        Df(30.0),
        "How steeply the shield may follow the part.",
        None,
    ),
    // ----------------------------------------------------------- Infill
    row(
        "infill_percentage",
        "Infill density",
        Infill,
        "Sparse infill",
        f(0.0, 100.0, 1.0, PCT),
        Df(20.0),
        "How much of the inside is filled.",
        bind_f!(infill_percentage),
    ),
    row(
        "infill_pattern",
        "Infill pattern",
        Infill,
        "Sparse infill",
        SettingKind::Choice(InfillPattern::NAMES),
        Dc(0),
        "Shape of the sparse infill. Honeycomb and gyroid fall back to rectilinear for now.",
        bind_e!(infill_pattern, InfillPattern),
    ),
    row(
        "infill_angle",
        "Infill angle",
        Infill,
        "Sparse infill",
        f(-360.0, 360.0, 1.0, DEG),
        Df(45.0),
        "Direction of the infill lines.",
        bind_f!(infill_angle),
    ),
    row(
        "infill_alternate",
        "Turn every layer",
        Infill,
        "Sparse infill",
        B,
        Db(true),
        "Turn the lines 90° on every other layer.",
        bind_b!(infill_alternate),
    ),
    row(
        "infill_overlap",
        "Infill overlap",
        Infill,
        "Sparse infill",
        f(0.0, 100.0, 1.0, PCT),
        Df(15.0),
        "How far infill reaches into the walls, % of the line width.",
        bind_f!(infill_overlap),
    ),
    row(
        "infill_before_perimeters",
        "Infill before walls",
        Infill,
        "Sparse infill",
        B,
        Db(false),
        "Print infill first; walls then cover its ends.",
        bind_b!(infill_before_perimeters),
    ),
    row(
        "infill_width_percent",
        "Infill line width",
        Infill,
        "Sparse infill",
        f(50.0, 300.0, 1.0, PCT),
        Df(100.0),
        "Infill line width, relative to the normal width.",
        None,
    ),
    row(
        "infill_every_layers",
        "Combine infill every",
        Infill,
        "Sparse infill",
        i(1, 10, "layers"),
        Di(1),
        "Print sparse infill once per this many layers, thicker.",
        None,
    ),
    row(
        "infill_min_length",
        "Shortest infill line",
        Infill,
        "Sparse infill",
        f(0.0, 20.0, 0.1, MM),
        Df(0.0),
        "Skip infill lines shorter than this.",
        None,
    ),
    row(
        "solid_pattern",
        "Solid pattern",
        Infill,
        "Solid infill",
        SettingKind::Choice(SolidPattern::NAMES),
        Dc(0),
        "Pattern for top and bottom layers.",
        bind_e!(solid_pattern, SolidPattern),
    ),
    row(
        "solid_angle_offset",
        "Solid angle offset",
        Infill,
        "Solid infill",
        f(-180.0, 180.0, 1.0, DEG),
        Df(0.0),
        "Turn solid layers relative to the infill angle.",
        None,
    ),
    row(
        "dense_layers",
        "Dense layers under tops",
        Infill,
        "Dense infill",
        i(0, 20, ""),
        Di(0),
        "Layers of denser infill below the top skin.",
        None,
    ),
    row(
        "dense_percentage",
        "Dense infill density",
        Infill,
        "Dense infill",
        f(0.0, 100.0, 1.0, PCT),
        Df(50.0),
        "Density of those layers.",
        None,
    ),
    row(
        "ironing_enabled",
        "Ironing",
        Infill,
        "Ironing",
        B,
        Db(false),
        "Pass the hot nozzle over top surfaces to smooth them.",
        None,
    ),
    row(
        "ironing_spacing",
        "Ironing spacing",
        Infill,
        "Ironing",
        f(0.01, 1.0, 0.01, MM),
        Df(0.1),
        "Distance between ironing passes.",
        None,
    ),
    row(
        "ironing_flow",
        "Ironing flow",
        Infill,
        "Ironing",
        f(0.0, 50.0, 1.0, PCT),
        Df(10.0),
        "Material pushed while ironing.",
        None,
    ),
    // ----------------------------------------------------------- Support
    row(
        "support_enabled",
        "Generate support",
        Support,
        "Support",
        B,
        Db(false),
        "Hold up overhangs with removable structures.",
        bind_b!(support_enabled),
    ),
    row(
        "support_type",
        "Support placement",
        Support,
        "Support",
        SettingKind::Choice(SupportPlacement::NAMES),
        Dc(0),
        "Whether support may stand on the model.",
        bind_e!(support_placement, SupportPlacement),
    ),
    row(
        "support_style",
        "Support style",
        Support,
        "Support",
        SettingKind::Choice(SupportStyle::NAMES),
        Dc(0),
        "Pillars: separate blocks on a grid that break away one by one and use less material. Areas: the exact shape under the overhang.",
        bind_e!(support_style, SupportStyle),
    ),
    row(
        "support_angle",
        "Overhang angle",
        Support,
        "Support",
        f(0.0, 90.0, 1.0, DEG),
        Df(45.0),
        "Surfaces flatter than this (from vertical) get support.",
        bind_f!(support_angle),
    ),
    row(
        "support_infill",
        "Support density",
        Support,
        "Support",
        f(1.0, 100.0, 1.0, PCT),
        Df(25.0),
        "Density of the support body.",
        bind_f!(support_infill),
    ),
    row(
        "support_pattern",
        "Support pattern",
        Support,
        "Support",
        SettingKind::Choice(SupportPattern::NAMES),
        Dc(0),
        "Shape of the support body.",
        bind_e!(support_pattern, SupportPattern),
    ),
    row(
        "support_xy_gap",
        "Horizontal gap",
        Support,
        "Clearance",
        f(0.0, 5.0, 0.05, MM),
        Df(0.4),
        "Space between support and the walls of the part.",
        bind_f!(support_xy_gap),
    ),
    row(
        "support_top_gap_layers",
        "Top gap layers",
        Support,
        "Clearance",
        i(0, 10, ""),
        Di(1),
        "Empty layers between support and the part above it.",
        bind_u!(support_top_gap_layers),
    ),
    row(
        "support_bottom_gap_layers",
        "Bottom gap layers",
        Support,
        "Clearance",
        i(0, 10, ""),
        Di(1),
        "Empty layers between the part and support standing on it.",
        bind_u!(support_bottom_gap_layers),
    ),
    row(
        "support_interface_layers",
        "Interface layers",
        Support,
        "Interface",
        i(0, 10, ""),
        Di(0),
        "Dense layers directly under supported surfaces.",
        bind_u!(support_interface_layers),
    ),
    row(
        "support_interface_infill",
        "Interface density",
        Support,
        "Interface",
        f(1.0, 100.0, 1.0, PCT),
        Df(70.0),
        "Density of the interface layers.",
        bind_f!(support_interface_infill),
    ),
    row(
        "support_base_layers",
        "Base layers",
        Support,
        "Base",
        i(0, 10, ""),
        Di(0),
        "Wider, denser layers where support meets the bed.",
        bind_u!(support_base_layers),
    ),
    row(
        "support_expansion",
        "Horizontal expansion",
        Support,
        "Base",
        f(-5.0, 10.0, 0.1, MM),
        Df(0.0),
        "Grow support areas by this much.",
        bind_f!(support_expansion),
    ),
    row(
        "support_pillar_size",
        "Pillar size",
        Support,
        "Support",
        f(0.5, 20.0, 0.5, MM),
        Df(2.0),
        "Grid cell of one pillar. Smaller pillars follow small details; larger ones print faster.",
        bind_f!(support_pillar_size),
    ),
    // ----------------------------------------------------------- Temperature
    row(
        "nozzle_temperature",
        "Nozzle",
        Temperature,
        "Printing",
        f(0.0, 400.0, 1.0, CEL),
        Df(210.0),
        "Hot end temperature from the second layer on.",
        bind_f!(nozzle_temperature),
    ),
    row(
        "bed_temperature",
        "Bed",
        Temperature,
        "Printing",
        f(0.0, 150.0, 1.0, CEL),
        Df(60.0),
        "Bed temperature from the second layer on.",
        bind_f!(bed_temperature),
    ),
    row(
        "first_layer_nozzle_temperature",
        "First layer nozzle",
        Temperature,
        "First layer",
        f(0.0, 400.0, 1.0, CEL),
        Df(210.0),
        "Hot end temperature for the first layer.",
        bind_f!(first_layer_nozzle_temperature),
    ),
    row(
        "first_layer_bed_temperature",
        "First layer bed",
        Temperature,
        "First layer",
        f(0.0, 150.0, 1.0, CEL),
        Df(60.0),
        "Bed temperature for the first layer.",
        bind_f!(first_layer_bed_temperature),
    ),
    row(
        "wait_for_temperatures",
        "Wait for temperatures",
        Temperature,
        "Start",
        B,
        Db(true),
        "Wait until the heaters reach temperature before printing.",
        bind_b!(wait_for_temperatures),
    ),
    row(
        "chamber_temperature",
        "Chamber",
        Temperature,
        "Start",
        f(0.0, 100.0, 1.0, CEL),
        Df(0.0),
        "Heated chamber target (0 = none).",
        None,
    ),
    row(
        "standby_temperature",
        "Idle tool temperature",
        Temperature,
        "Multi-tool",
        f(0.0, 400.0, 1.0, CEL),
        Df(175.0),
        "Temperature a waiting tool drops to.",
        None,
    ),
    // ----------------------------------------------------------- Cooling
    row(
        "fan_percent",
        "Fan speed",
        Cooling,
        "Part fan",
        f(0.0, 100.0, 1.0, PCT),
        Df(100.0),
        "Part cooling fan duty.",
        bind_f!(fan_percent),
    ),
    row(
        "fan_from_layer",
        "Fan from layer",
        Cooling,
        "Part fan",
        i(1, 1000, ""),
        Di(2),
        "Layer number on which the fan starts.",
        bind_u!(fan_from_layer),
    ),
    row(
        "fan_ramp_layers",
        "Fan ramp",
        Cooling,
        "Part fan",
        i(0, 50, "layers"),
        Di(0),
        "Raise the fan gradually over this many layers.",
        None,
    ),
    row(
        "bridge_fan_percent",
        "Bridge fan",
        Cooling,
        "Part fan",
        f(0.0, 100.0, 1.0, PCT),
        Df(100.0),
        "Fan duty while bridging.",
        None,
    ),
    row(
        "min_layer_time",
        "Minimum layer time",
        Cooling,
        "Slow small layers",
        f(0.0, 120.0, 1.0, "s"),
        Df(0.0),
        "Slow down so each layer takes at least this long.",
        None,
    ),
    row(
        "min_layer_speed",
        "Slowest speed",
        Cooling,
        "Slow small layers",
        f(1.0, 100.0, 1.0, MMS),
        Df(10.0),
        "Never slow below this.",
        None,
    ),
    // ----------------------------------------------------------- Speeds
    row(
        "print_speed",
        "Default speed",
        Speeds,
        "Printing",
        f(1.0, 1000.0, 1.0, MMS),
        Df(60.0),
        "Infill and anything without its own speed.",
        bind_f!(print_speed),
    ),
    row(
        "perimeter_speed",
        "Wall speed",
        Speeds,
        "Printing",
        f(1.0, 1000.0, 1.0, MMS),
        Df(40.0),
        "Speed for perimeters.",
        bind_f!(perimeter_speed),
    ),
    row(
        "outer_perimeter_speed_percent",
        "Outer wall speed",
        Speeds,
        "Printing",
        f(5.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Outer perimeter speed, relative to the wall speed.",
        bind_f!(outer_perimeter_speed_percent),
    ),
    row(
        "solid_infill_speed_percent",
        "Solid infill speed",
        Speeds,
        "Printing",
        f(5.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Solid infill speed, relative to the default speed.",
        bind_f!(solid_infill_speed_percent),
    ),
    row(
        "support_speed_percent",
        "Support speed",
        Speeds,
        "Printing",
        f(5.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Support speed, relative to the default speed.",
        bind_f!(support_speed_percent),
    ),
    row(
        "bridge_speed_percent",
        "Bridge speed",
        Speeds,
        "Printing",
        f(5.0, 200.0, 1.0, PCT),
        Df(70.0),
        "Bridge speed, relative to the default speed.",
        None,
    ),
    row(
        "first_layer_speed",
        "First layer speed",
        Speeds,
        "First layer",
        f(1.0, 1000.0, 1.0, MMS),
        Df(20.0),
        "Everything on the first layer prints at this speed.",
        bind_f!(first_layer_speed),
    ),
    row(
        "travel_speed",
        "Travel speed",
        Speeds,
        "Moves",
        f(1.0, 2000.0, 1.0, MMS),
        Df(150.0),
        "Speed of moves without extrusion.",
        bind_f!(travel_speed),
    ),
    row(
        "z_speed",
        "Z speed",
        Speeds,
        "Moves",
        f(0.5, 100.0, 0.5, MMS),
        Df(10.0),
        "Speed of vertical moves.",
        bind_f!(z_speed),
    ),
    // ----------------------------------------------------------- G-code
    row(
        "flavor",
        "Firmware",
        Gcode,
        "Output",
        SettingKind::Choice(Flavor::NAMES),
        Dc(0),
        "Which firmware the G-code is written for.",
        bind_e!(flavor, Flavor),
    ),
    row(
        "relative_extrusion",
        "Relative extrusion",
        Gcode,
        "Output",
        B,
        Db(false),
        "Write E as increments (M83) instead of positions.",
        None,
    ),
    row(
        "firmware_retraction",
        "Firmware retraction",
        Gcode,
        "Output",
        B,
        Db(false),
        "Let the firmware retract (G10/G11).",
        None,
    ),
    row(
        "arc_fitting",
        "Arc moves",
        Gcode,
        "Output",
        B,
        Db(false),
        "Replace runs of short segments with G2/G3 arcs.",
        None,
    ),
    row(
        "verbose_comments",
        "Explain moves in comments",
        Gcode,
        "Output",
        B,
        Db(true),
        "Comment each feature and layer.",
        None,
    ),
    row(
        "thumbnail_size",
        "Thumbnail size",
        Gcode,
        "Output",
        i(0, 512, "px"),
        Di(0),
        "Embed a preview image of this size (0 = none).",
        None,
    ),
    row(
        "post_process",
        "Post-processing command",
        Gcode,
        "Output",
        SettingKind::Text,
        Dt(""),
        "A program to run on the finished file (never run by agents).",
        None,
    ),
    // ----------------------------------------------------------- Scripts
    row(
        "start_script",
        "Start script",
        Scripts,
        "Scripts",
        SettingKind::Text,
        Dt(""),
        "Runs after heat-up and replaces the built-in homing when not empty; a script that sets temperatures itself skips the heat-up. Placeholders: {nozzle} {bed} {first_nozzle} {first_bed}.",
        bind_t!(start_script),
    ),
    row(
        "end_script",
        "End script",
        Scripts,
        "Scripts",
        SettingKind::Text,
        Dt(""),
        "Replaces the built-in end sequence when not empty.",
        bind_t!(end_script),
    ),
    row(
        "layer_script",
        "Layer change script",
        Scripts,
        "Scripts",
        SettingKind::Text,
        Dt(""),
        "Inserted at every layer change. Placeholders: {layer} {z}.",
        bind_t!(layer_script),
    ),
    row(
        "tool_change_script",
        "Tool change script",
        Scripts,
        "Scripts",
        SettingKind::Text,
        Dt(""),
        "Inserted at every tool change.",
        None,
    ),
    // ----------------------------------------------------------- Machine
    row(
        "bed_width",
        "Bed width (X)",
        Machine,
        "Build volume",
        f(10.0, 2000.0, 1.0, MM),
        Df(220.0),
        "Printable width.",
        bind_f!(bed_width),
    ),
    row(
        "bed_depth",
        "Bed depth (Y)",
        Machine,
        "Build volume",
        f(10.0, 2000.0, 1.0, MM),
        Df(220.0),
        "Printable depth.",
        bind_f!(bed_depth),
    ),
    row(
        "bed_height",
        "Build height (Z)",
        Machine,
        "Build volume",
        f(10.0, 2000.0, 1.0, MM),
        Df(250.0),
        "Printable height.",
        bind_f!(bed_height),
    ),
    row(
        "bed_shape",
        "Bed shape",
        Machine,
        "Build volume",
        SettingKind::Choice(&["rectangle", "circle"]),
        Dc(0),
        "Shape of the printable area.",
        None,
    ),
    row(
        "origin_center",
        "Origin at bed centre",
        Machine,
        "Build volume",
        B,
        Db(false),
        "The printer's 0,0 is the middle of the bed (deltas).",
        None,
    ),
    row("tool_count", "Tools", Machine, "Tools", i(1, 8, ""), Di(1), "Number of extruders.", None),
    row(
        "max_acceleration",
        "Acceleration",
        Machine,
        "Kinematics",
        f(0.0, 50_000.0, 100.0, "mm/s²"),
        Df(0.0),
        "Used for the time estimate (0 = ignore).",
        None,
    ),
    // ----------------------------------------------------------- Material
    row(
        "filament_diameter",
        "Filament diameter",
        Material,
        "Filament",
        f(0.5, 5.0, 0.01, MM),
        Df(1.75),
        "Measured diameter of the filament.",
        bind_f!(filament_diameter),
    ),
    row(
        "filament_density",
        "Density",
        Material,
        "Filament",
        f(0.1, 10.0, 0.01, "g/cm³"),
        Df(1.24),
        "Used for the weight estimate.",
        bind_f!(filament_density),
    ),
    row(
        "filament_cost",
        "Price",
        Material,
        "Filament",
        f(0.0, 10_000.0, 0.5, "per kg"),
        Df(20.0),
        "Used for the cost estimate.",
        bind_f!(filament_cost),
    ),
    // ----------------------------------------------------------- Advanced
    row(
        "avoid_crossing_walls",
        "Avoid crossing walls",
        Advanced,
        "Travel",
        B,
        Db(true),
        "Route travel inside the part instead of across its outline.",
        bind_b!(avoid_crossing_walls),
    ),
    row(
        "retract_only_outside",
        "Retract only when crossing outlines",
        Advanced,
        "Travel",
        B,
        Db(true),
        "Retract only when a travel crosses the part's outline. Travel inside the part or between support pillars leaves any string where it cannot be seen.",
        bind_b!(retract_only_outside),
    ),
    row(
        "thin_walls",
        "Thin wall handling",
        Advanced,
        "Walls",
        SettingKind::Choice(&["off", "single line", "gap fill"]),
        Dc(0),
        "What to print where two walls do not fit.",
        None,
    ),
    row(
        "gap_fill",
        "Fill gaps between walls",
        Advanced,
        "Walls",
        B,
        Db(false),
        "Fill slivers left between perimeters and infill.",
        None,
    ),
    row(
        "bridging_enabled",
        "Detect bridges",
        Advanced,
        "Bridging",
        B,
        Db(false),
        "Print unsupported spans as straight bridges.",
        None,
    ),
    row(
        "bridge_flow_percent",
        "Bridge flow",
        Advanced,
        "Bridging",
        f(10.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Flow while bridging.",
        None,
    ),
    row(
        "slice_close_gap",
        "Close outline gaps up to",
        Advanced,
        "Slicing",
        f(0.0, 5.0, 0.05, MM),
        Df(1.0),
        "Repair open outlines with gaps this small.",
        None,
    ),
    row(
        "merge_overlapping",
        "Merge overlapping bodies",
        Advanced,
        "Slicing",
        B,
        Db(true),
        "Treat overlapping parts of one model as one solid.",
        None,
    ),
    // ---- additions from the feature inventory (docs/research/02) ----
    row(
        "adaptive_min_height",
        "Thinnest adaptive layer",
        Layer,
        "Adaptive layers",
        f(0.01, 1.0, 0.01, MM),
        Df(0.08),
        "Lower bound for adaptive layer heights.",
        None,
    ),
    row(
        "adaptive_max_height",
        "Thickest adaptive layer",
        Layer,
        "Adaptive layers",
        f(0.01, 2.0, 0.01, MM),
        Df(0.28),
        "Upper bound for adaptive layer heights.",
        None,
    ),
    row(
        "adaptive_quality",
        "Adaptive detail",
        Layer,
        "Adaptive layers",
        i(1, 10, ""),
        Di(5),
        "Higher follows curved surfaces more closely.",
        None,
    ),
    row(
        "outer_size_offset",
        "Outer size correction",
        Layer,
        "Dimensional accuracy",
        f(-2.0, 2.0, 0.01, MM),
        Df(0.0),
        "Grow (or shrink) outer walls to correct the printed size.",
        None,
    ),
    row(
        "hole_size_offset",
        "Hole size correction",
        Layer,
        "Dimensional accuracy",
        f(-2.0, 2.0, 0.01, MM),
        Df(0.0),
        "Grow (or shrink) holes separately from outer walls.",
        None,
    ),
    row(
        "island_order",
        "Island order",
        Layer,
        "Walls",
        SettingKind::Choice(&[
            "nearest first",
            "same order every layer",
            "each feature across all islands",
        ]),
        Dc(0),
        "How separate parts of a layer are visited.",
        bind_e!(island_order, IslandOrder),
    ),
    row(
        "solid_min_area",
        "Make small areas solid below",
        Infill,
        "Solid infill",
        f(0.0, 1000.0, 1.0, "mm²"),
        Df(0.0),
        "Infill regions smaller than this print solid.",
        None,
    ),
    row(
        "solid_expansion",
        "Solid area expansion",
        Infill,
        "Solid infill",
        f(0.0, 10.0, 0.1, MM),
        Df(0.0),
        "Grow solid areas into the sparse infill for anchoring.",
        None,
    ),
    row(
        "monotonic_solid",
        "Ordered solid lines",
        Infill,
        "Solid infill",
        B,
        Db(false),
        "Lay solid lines strictly side by side for an even top surface.",
        None,
    ),
    row(
        "top_flow_percent",
        "Top surface flow",
        Infill,
        "Top surfaces",
        f(10.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Flow on the exposed top layer.",
        None,
    ),
    row(
        "top_speed_percent",
        "Top surface speed",
        Infill,
        "Top surfaces",
        f(5.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Speed on the exposed top layer.",
        None,
    ),
    row(
        "top_fan_percent",
        "Top surface fan",
        Infill,
        "Top surfaces",
        f(0.0, 100.0, 1.0, PCT),
        Df(100.0),
        "Fan on the exposed top layer.",
        None,
    ),
    row(
        "solid_every_layers",
        "Solid layer every",
        Infill,
        "Reinforcement",
        i(0, 1000, "layers"),
        Di(0),
        "Insert solid layers at this interval for stiffness (0 = off).",
        None,
    ),
    row(
        "solid_every_count",
        "Reinforcement thickness",
        Infill,
        "Reinforcement",
        i(1, 20, "layers"),
        Di(2),
        "How many solid layers each reinforcement has.",
        None,
    ),
    row(
        "support_outlines",
        "Support walls",
        Support,
        "Support",
        i(0, 5, ""),
        Di(0),
        "Perimeters around support areas.",
        bind_u!(support_outlines),
    ),
    row(
        "support_every_layers",
        "Support every",
        Support,
        "Support",
        i(1, 5, "layers"),
        Di(1),
        "Print support once per this many layers, thicker.",
        None,
    ),
    row(
        "support_floor_layers",
        "Floor interface layers",
        Support,
        "Interface",
        i(0, 10, ""),
        Di(0),
        "Dense layers where support stands on the part.",
        bind_u!(support_floor_layers),
    ),
    row(
        "temperature_schedule",
        "Temperature by layer",
        Temperature,
        "Schedule",
        SettingKind::Text,
        Dt(""),
        "One 'layer temperature' pair per line, e.g. '30 205'.",
        None,
    ),
    row(
        "idle_after_seconds",
        "Cool an idle tool after",
        Temperature,
        "Multi-tool",
        f(1.0, 3600.0, 1.0, "s"),
        Df(120.0),
        "How long a tool waits before cooling down.",
        None,
    ),
    row(
        "fan_boost_below_seconds",
        "Boost fan below layer time",
        Cooling,
        "Slow small layers",
        f(0.0, 300.0, 1.0, "s"),
        Df(0.0),
        "Run the fan harder on layers faster than this (0 = off).",
        None,
    ),
    row(
        "fan_kickstart",
        "Kick-start the fan",
        Cooling,
        "Part fan",
        B,
        Db(false),
        "Spin the fan at full power briefly before a low speed.",
        None,
    ),
    row(
        "top_layer_speed_percent",
        "Top layer speed",
        Speeds,
        "Printing",
        f(5.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Speed of the last solid layers, relative to the default speed.",
        None,
    ),
    row(
        "short_wall_length",
        "Slow walls shorter than",
        Speeds,
        "Printing",
        f(0.0, 500.0, 1.0, MM),
        Df(0.0),
        "Short perimeters print slower so they can cool (0 = off).",
        None,
    ),
    row(
        "max_volumetric_flow",
        "Flow limit",
        Speeds,
        "Printing",
        f(0.0, 100.0, 0.5, "mm³/s"),
        Df(0.0),
        "Slow down where the hot end cannot melt fast enough (0 = off).",
        None,
    ),
    row(
        "gcode_offset_x",
        "Offset X",
        Gcode,
        "Coordinate offsets",
        f(-100.0, 100.0, 0.01, MM),
        Df(0.0),
        "Added to every X written.",
        bind_f!(gcode_offset_x),
    ),
    row(
        "gcode_offset_y",
        "Offset Y",
        Gcode,
        "Coordinate offsets",
        f(-100.0, 100.0, 0.01, MM),
        Df(0.0),
        "Added to every Y written.",
        bind_f!(gcode_offset_y),
    ),
    row(
        "gcode_offset_z",
        "Offset Z",
        Gcode,
        "Coordinate offsets",
        f(-5.0, 5.0, 0.01, MM),
        Df(0.0),
        "Added to every Z written; tunes first-layer squish.",
        bind_f!(gcode_offset_z),
    ),
    row(
        "output_format",
        "File format",
        Gcode,
        "Output",
        SettingKind::Choice(&[
            "plain G-code",
            "binary G-code (planned)",
            "printer-specific (planned)",
        ]),
        Dc(0),
        "Which file is written.",
        None,
    ),
    row(
        "retract_script",
        "Retraction script",
        Scripts,
        "Scripts",
        SettingKind::Text,
        Dt(""),
        "Inserted at every retraction.",
        None,
    ),
    row(
        "profile_change_script",
        "Profile change script",
        Scripts,
        "Scripts",
        SettingKind::Text,
        Dt(""),
        "Inserted where one profile hands over to the next.",
        None,
    ),
    row(
        "homing_corner",
        "Home position",
        Machine,
        "Build volume",
        SettingKind::Choice(&["front left", "front right", "back left", "back right", "centre"]),
        Dc(0),
        "Where the nozzle rests after homing.",
        None,
    ),
    row(
        "bed_diameter",
        "Bed diameter",
        Machine,
        "Build volume",
        f(10.0, 2000.0, 1.0, MM),
        Df(200.0),
        "Diameter of a round bed.",
        None,
    ),
    row(
        "tool_change_retract",
        "Tool change retraction",
        Extruder,
        "Tool change",
        f(0.0, 100.0, 0.5, MM),
        Df(10.0),
        "Filament pulled back before a tool change.",
        None,
    ),
    row(
        "tool_change_speed",
        "Tool change speed",
        Extruder,
        "Tool change",
        f(1.0, 200.0, 1.0, MMS),
        Df(15.0),
        "Speed of the tool change retraction.",
        None,
    ),
    row(
        "wipe_features",
        "Wipe after",
        Advanced,
        "Travel",
        SettingKind::Choice(&["outer walls", "all walls", "every feature"]),
        Dc(0),
        "Which paths end with a wipe.",
        None,
    ),
    row(
        "retract_over_tops",
        "Always retract over top surfaces",
        Advanced,
        "Travel",
        B,
        Db(false),
        "Avoid scars on finished top surfaces.",
        None,
    ),
    row(
        "retract_min_extrusion",
        "Retract only after extruding",
        Advanced,
        "Travel",
        f(0.0, 50.0, 0.1, MM),
        Df(0.0),
        "Skip a retraction when little was printed since the last one.",
        None,
    ),
    row(
        "travel_detour_limit",
        "Detour limit",
        Advanced,
        "Travel",
        f(1.0, 10.0, 0.1, "×"),
        Df(3.0),
        "Longest detour, relative to the direct route, when avoiding walls.",
        bind_f!(travel_detour_limit),
    ),
    row(
        "single_line_min_width",
        "Single line narrowest",
        Advanced,
        "Walls",
        f(10.0, 100.0, 1.0, PCT),
        Df(50.0),
        "Narrowest single-line wall, relative to the line width.",
        None,
    ),
    row(
        "single_line_max_width",
        "Single line widest",
        Advanced,
        "Walls",
        f(100.0, 300.0, 1.0, PCT),
        Df(200.0),
        "Widest single-line wall, relative to the line width.",
        None,
    ),
    row(
        "open_outline_mode",
        "Open outlines",
        Advanced,
        "Slicing",
        SettingKind::Choice(&["close small gaps", "drop them", "print as lines"]),
        Dc(0),
        "What to do with outlines a damaged mesh leaves open.",
        None,
    ),
    row(
        "fill_rule",
        "Solid regions",
        Advanced,
        "Slicing",
        SettingKind::Choice(&[
            "merge overlaps",
            "alternate inside and outside",
            "everything inside any outline",
        ]),
        Dc(0),
        "How nested and overlapping outlines become solid areas.",
        None,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_defaults_valid() {
        let mut keys: Vec<&str> = SETTINGS.iter().map(|s| s.key).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "duplicate setting key");
        for s in SETTINGS {
            assert!(s.check(&s.default.value()).is_ok(), "default of {} is invalid", s.key);
        }
    }

    #[test]
    fn bound_settings_default_to_the_struct_defaults() {
        let d = PrintSettings::default();
        for s in SETTINGS.iter().filter(|s| s.implemented()) {
            assert_eq!(d.get(s.key), Some(s.default.value()), "{} default differs", s.key);
        }
    }

    #[test]
    fn every_tab_has_settings_and_keys_are_snake_case() {
        for t in Tab::ALL {
            assert!(tab_settings(t).next().is_some(), "{t:?} is empty");
        }
        for s in SETTINGS {
            assert!(
                s.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            );
        }
    }

    #[test]
    fn set_checks_kind_and_range() {
        let mut p = PrintSettings::default();
        assert!(p.set("layer_height", SettingValue::Float(0.12)).is_ok());
        assert_eq!(p.layer_height, 0.12);
        assert!(p.set("layer_height", SettingValue::Float(9.0)).is_err());
        assert!(p.set("layer_height", SettingValue::Bool(true)).is_err());
        assert!(p.set("layer_height", SettingValue::Float(f64::NAN)).is_err());
        assert!(p.set("infill_pattern", SettingValue::Choice(3)).is_ok());
        assert_eq!(p.infill_pattern, InfillPattern::Triangles);
        assert!(p.set("infill_pattern", SettingValue::Choice(99)).is_err());
        assert!(p.set("nope", SettingValue::Bool(true)).is_err());
    }

    #[test]
    fn placeholders_are_stored_and_reported() {
        let mut p = PrintSettings::default();
        assert!(p.unused_changes().is_empty());
        assert!(p.set("raft_enabled", SettingValue::Bool(true)).is_ok());
        assert_eq!(p.get("raft_enabled"), Some(SettingValue::Bool(true)));
        assert_eq!(p.unused_changes(), vec!["Raft"]);
        // setting it back to the default forgets it
        assert!(p.set("raft_enabled", SettingValue::Bool(false)).is_ok());
        assert!(p.extra.is_empty());
    }

    #[test]
    fn validate_catches_bad_struct_values() {
        let p = PrintSettings { perimeter_count: 10_000, ..PrintSettings::default() };
        assert!(p.validate().is_err());
        let mut p = PrintSettings::default();
        p.extra.insert("layer_height".into(), SettingValue::Float(0.2));
        assert!(p.validate().is_err(), "bound keys may not hide in extra");
    }

    #[test]
    fn automatic_width_and_retraction_switch() {
        let p = PrintSettings { extrusion_width_auto: true, ..PrintSettings::default() };
        assert!((p.line_width() - 0.45).abs() < 1e-12);
        let p = PrintSettings { retract_enabled: false, ..PrintSettings::default() };
        assert_eq!(p.retraction(), 0.0);
    }
}
