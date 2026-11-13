//! Import of Simplify3D `.fff` profiles.
//!
//! A `.fff` file is XML: a `<profile name=…>` root with one element per
//! setting, `<extruder>` blocks, `<temperatureController>` blocks and fan
//! schedules. Two generations exist and both are read:
//!
//! - **classic** (written by 4.x): plain values (`<layerHeight>0.3</layerHeight>`),
//!   numeric choices, script lines joined with `,`, schedules as `<setpoint …/>`;
//! - **option lists** (written by 5.x): choices as `<option value=… selected="1"/>`
//!   lists, schedules as `<option value="layer|value"/>`, script lines joined with `|`.
//!
//! Speeds in the file are mm/min; SliceCraft's are mm/s. Simplify3D's
//! automatic line width is 1.2 × the nozzle and its speeds per feature are
//! fractions of one default speed; both are converted to SliceCraft's model so
//! the imported profile prints the same way. The observations behind every
//! conversion are in `docs/research/09-fff-profiles.md`.
//!
//! The file is hostile input: size-capped, no DTD, a node limit, every value
//! parsed and range-checked through `PrintSettings::set`. Whatever is not
//! understood is listed in the report, never dropped silently.

use crate::IoError;
use roxmltree::{Document, Node, ParsingOptions};
use slicecraft_doc::profile::Profile;
use slicecraft_doc::settings::{SettingKind, SettingValue, spec};
use slicecraft_doc::{
    Flavor, InfillPattern, PerimeterOrder, PrintSettings, SeamMode, SolidPattern,
};
use std::collections::BTreeSet;

/// Largest `.fff` file read, in bytes (real files are 10–40 KB).
pub const MAX_FFF_BYTES: usize = 4 << 20;
/// Largest number of XML nodes parsed.
const MAX_NODES: u32 = 200_000;
/// Simplify3D's automatic line width, as a multiple of the nozzle (measured).
pub const AUTO_WIDTH_FACTOR: f64 = 1.2;

/// What an import did, for the person who asked for it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportReport {
    /// The program and version that wrote the file (`app` attribute).
    pub written_by: String,
    /// SliceCraft settings set from the file that the slicer uses today.
    pub applied: Vec<&'static str>,
    /// SliceCraft settings set from the file that are still placeholders.
    pub placeholders: Vec<&'static str>,
    /// Conversions and anything that could not be carried over exactly.
    pub notes: Vec<String>,
    /// Elements of the file SliceCraft has no counterpart for.
    pub ignored: Vec<String>,
}

/// True when `text` looks like a Simplify3D profile (an XML `<profile>` root).
pub fn looks_like_fff(text: &str) -> bool {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    (t.starts_with("<?xml") || t.starts_with("<profile")) && t.contains("<profile")
}

/// Read a `.fff` profile on top of SliceCraft's defaults.
pub fn import_fff(text: &str) -> Result<(Profile, ImportReport), IoError> {
    if text.len() > MAX_FFF_BYTES {
        return Err(IoError::BadFff("the file is larger than a profile can be"));
    }
    let options =
        ParsingOptions { allow_dtd: false, nodes_limit: MAX_NODES, ..ParsingOptions::default() };
    let doc = Document::parse_with_options(text, options)
        .map_err(|_| IoError::BadFff("the file is not well-formed XML"))?;
    let root = doc.root_element();
    if !root.has_tag_name("profile") {
        return Err(IoError::BadFff("the XML root is not a profile"));
    }
    let name = root.attribute("name").map_or_else(|| "Imported".to_string(), str::to_string);
    let mut im = Importer::new(root);
    im.report.written_by = root.attribute("app").unwrap_or("unknown").to_string();
    im.run();
    let Importer { settings, mut report, used, ext_used, extruder, root, .. } = im;
    report.ignored = unused(root, &used, "");
    if let Some(ext) = extruder {
        report.ignored.extend(unused(ext, &ext_used, "extruder/"));
    }
    settings.validate().map_err(|_| IoError::BadFff("the imported settings do not validate"))?;
    Ok((Profile { name, settings }, report))
}

/// Element children of `n` not in `used`, once each, in file order.
fn unused(n: Node, used: &BTreeSet<String>, prefix: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    n.children()
        .filter(|c| c.is_element())
        .map(|c| c.tag_name().name())
        .filter(|name| !used.contains(*name) && seen.insert(*name))
        .map(|name| format!("{prefix}{name}"))
        .collect()
}

/// The value of a setting element: the selected option of an option list,
/// or the element's text.
fn value_of(n: Node) -> String {
    let mut options = n.children().filter(|c| c.has_tag_name("option")).peekable();
    if options.peek().is_some() {
        return options
            .find(|o| o.attribute("selected") == Some("1"))
            .and_then(|o| o.attribute("value"))
            .unwrap_or("")
            .to_string();
    }
    n.text().map(str::trim).unwrap_or("").to_string()
}

/// `layer|value` pairs from either schedule form, sorted by layer.
fn schedule(n: Node, value_attr: &str) -> Vec<(u32, f64)> {
    let mut out: Vec<(u32, f64)> = Vec::new();
    for c in n.descendants().filter(|c| c.is_element()) {
        let pair = if c.has_tag_name("setpoint") {
            c.attribute("layer").zip(c.attribute(value_attr))
        } else if c.has_tag_name("option") {
            c.attribute("value").and_then(|v| v.split_once('|'))
        } else {
            None
        };
        if let Some((l, v)) = pair
            && let (Ok(l), Ok(v)) = (l.trim().parse::<u32>(), v.trim().parse::<f64>())
            && v.is_finite()
        {
            out.push((l, v));
        }
        if out.len() >= 1000 {
            break;
        }
    }
    out.sort_by_key(|(l, _)| *l);
    out
}

fn parse_f(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A speed factor stored as a fraction (0.8). Values above 2 are taken as a
/// percentage, which hand-edited files sometimes contain.
fn fraction(v: f64) -> f64 {
    if v > 2.0 { v / 100.0 } else { v }
}

fn flag(raw: &str) -> bool {
    matches!(raw.trim(), "1" | "true")
}

struct Importer<'a, 'i> {
    root: Node<'a, 'i>,
    extruder: Option<Node<'a, 'i>>,
    used: BTreeSet<String>,
    ext_used: BTreeSet<String>,
    settings: PrintSettings,
    report: ImportReport,
    /// Option lists (5.x) rather than plain values (4.x).
    option_lists: bool,
}

impl<'a, 'i> Importer<'a, 'i> {
    fn new(root: Node<'a, 'i>) -> Self {
        let option_lists = root.attribute("technology").is_some()
            || root.children().any(|c| c.children().any(|o| o.has_tag_name("option")));
        Self {
            root,
            extruder: None,
            used: BTreeSet::new(),
            ext_used: BTreeSet::new(),
            settings: PrintSettings::default(),
            report: ImportReport::default(),
            option_lists,
        }
    }

    fn note(&mut self, msg: impl Into<String>) {
        self.report.notes.push(msg.into());
    }

    /// First present element among `names` at the top level; marked used.
    fn top(&mut self, names: &[&str]) -> Option<String> {
        for name in names {
            if let Some(n) = self.root.children().find(|c| c.has_tag_name(*name)) {
                self.used.insert((*name).to_string());
                return Some(value_of(n));
            }
        }
        None
    }

    /// Like `top`, inside the first extruder.
    fn ext(&mut self, names: &[&str]) -> Option<String> {
        let ext = self.extruder?;
        for name in names {
            if let Some(n) = ext.children().find(|c| c.has_tag_name(*name)) {
                self.ext_used.insert((*name).to_string());
                return Some(value_of(n));
            }
        }
        None
    }

    fn top_f(&mut self, names: &[&str]) -> Option<f64> {
        self.top(names).as_deref().and_then(parse_f)
    }

    fn ext_f(&mut self, names: &[&str]) -> Option<f64> {
        self.ext(names).as_deref().and_then(parse_f)
    }

    fn mark(&mut self, name: &str) {
        self.used.insert(name.to_string());
    }

    /// Set one of our settings; a refused value keeps the default and says so.
    fn put(&mut self, key: &'static str, value: SettingValue, from: &str) {
        match self.settings.set(key, value) {
            Ok(()) => {
                let Some(s) = spec(key) else { return };
                let list = if s.implemented() {
                    &mut self.report.applied
                } else {
                    &mut self.report.placeholders
                };
                if !list.contains(&s.key) {
                    list.push(s.key);
                }
            }
            Err(e) => self.note(format!("{from}: {e}; kept SliceCraft's default")),
        }
    }

    /// Set a number, flag or text setting from a number, by the setting's kind.
    fn num(&mut self, key: &'static str, value: Option<f64>, from: &str) {
        let (Some(v), Some(s)) = (value, spec(key)) else { return };
        let value = match s.kind {
            SettingKind::Float { .. } => SettingValue::Float(v),
            SettingKind::Int { .. } => SettingValue::Int(v.round() as i64),
            SettingKind::Bool => SettingValue::Bool(v != 0.0),
            _ => return,
        };
        self.put(key, value, from);
    }

    fn boolean(&mut self, key: &'static str, value: Option<String>, from: &str) {
        if let Some(v) = value {
            self.put(key, SettingValue::Bool(flag(&v)), from);
        }
    }

    fn choice(&mut self, key: &'static str, index: Option<usize>, from: &str) {
        if let Some(i) = index {
            self.put(key, SettingValue::Choice(i), from);
        }
    }

    fn run(&mut self) {
        self.extruders();
        self.layers();
        self.speeds();
        self.adhesion();
        self.infill();
        self.support();
        self.temperatures();
        self.cooling();
        self.gcode();
        self.machine();
        self.scripts();
        self.advanced();
        let presets = self
            .root
            .children()
            .filter(|c| {
                c.has_tag_name("autoConfigureMaterial") || c.has_tag_name("autoConfigureQuality")
            })
            .count();
        if presets > 0 {
            self.mark("autoConfigureMaterial");
            self.mark("autoConfigureQuality");
            self.note(format!(
                "the file carries {presets} material/quality presets; only the main settings were imported"
            ));
        }
        for bookkeeping in ["baseProfile", "printMaterial", "printQuality", "printExtruders"] {
            self.mark(bookkeeping);
        }
    }

    fn extruders(&mut self) {
        let exts: Vec<Node<'a, 'i>> =
            self.root.children().filter(|c| c.has_tag_name("extruder")).collect();
        self.mark("extruder");
        self.mark("primaryExtruder");
        self.extruder = exts.first().copied();
        if exts.len() > 1 {
            let name = self.extruder.and_then(|e| e.attribute("name")).unwrap_or("the first");
            self.note(format!(
                "the profile has {} extruders; imported {name} (several tools are planned)",
                exts.len()
            ));
        }
        let nozzle = self.ext_f(&["diameter", "nozzleDiameter"]);
        self.num("nozzle_diameter", nozzle, "nozzle diameter");
        let auto = self.ext(&["autoWidth"]).map(|v| flag(&v)).or_else(|| {
            self.ext(&["extrusionWidthMode"]).map(|v| v.eq_ignore_ascii_case("automatic"))
        });
        let width = self.ext_f(&["width", "extrusionWidth"]);
        let nozzle = self.settings.nozzle_diameter;
        if auto == Some(true) {
            let w = (nozzle * AUTO_WIDTH_FACTOR * 1000.0).round() / 1000.0;
            self.put("extrusion_width_auto", SettingValue::Bool(false), "automatic width");
            self.num("extrusion_width", Some(w), "automatic width");
            self.note(format!(
                "automatic line width became a fixed {w} mm ({AUTO_WIDTH_FACTOR} × nozzle, as the source computes it)"
            ));
        } else if width.is_some() {
            self.put("extrusion_width_auto", SettingValue::Bool(false), "line width");
            self.num("extrusion_width", width, "line width");
        }
        let v = self.ext_f(&["extrusionMultiplier"]);
        self.num("extrusion_multiplier", v, "extrusion multiplier");
        let v = self.ext_f(&["toolheadNumber"]);
        self.num("tool_number", v, "toolhead number");
        let v = self.ext(&["useRetract", "useRetraction"]);
        self.boolean("retract_enabled", v, "retraction");
        let v = self.ext_f(&["retractionDistance", "retractDistance"]);
        self.num("retract_length", v, "retraction distance");
        let v = self.ext_f(&["extraRestartDistance"]);
        self.num("retract_extra_restart", v, "extra restart distance");
        let v = self.ext_f(&["retractionZLift", "retractVerticalLift"]);
        self.num("retract_lift", v, "retraction lift");
        let v = self.ext_f(&["retractionSpeed", "retractSpeed"]).map(|s| s / 60.0);
        self.num("retract_speed", v, "retraction speed");
        let v = self.ext(&["useCoasting"]);
        self.boolean("coast_enabled", v, "coasting");
        let v = self.ext_f(&["coastingDistance"]);
        self.num("coast_distance", v, "coasting distance");
        let v = self.ext(&["useWipe", "useWiping"]);
        self.boolean("wipe_enabled", v, "wipe");
        let v = self.ext_f(&["wipeDistance", "wipingDistance"]);
        self.num("wipe_distance", v, "wipe distance");
        // filament: per extruder in 5.x, `a|b|…` lists in 4.x
        let first = |s: Option<String>| s.and_then(|s| s.split('|').next().and_then(parse_f));
        let d =
            self.ext_f(&["filamentDiameter"]).or_else(|| first(self.top(&["filamentDiameters"])));
        self.num("filament_diameter", d, "filament diameter");
        let c = self
            .ext_f(&["filamentPricePerKilogram"])
            .or_else(|| first(self.top(&["filamentPricesPerKg"])));
        self.num("filament_cost", c, "filament price");
        let r =
            self.ext_f(&["filamentDensity"]).or_else(|| first(self.top(&["filamentDensities"])));
        self.num("filament_density", r, "filament density");
    }

    fn layers(&mut self) {
        let lh = self.top_f(&["layerHeight"]);
        self.num("layer_height", lh, "layer height");
        let v = self.top_f(&["topSolidLayers"]);
        self.num("solid_layers_top", v, "top solid layers");
        let v = self.top_f(&["bottomSolidLayers"]);
        self.num("solid_layers_bottom", v, "bottom solid layers");
        let v = self.top_f(&["perimeterOutlines", "outlinePerimeters"]);
        self.num("perimeter_count", v, "perimeter outlines");

        let absolute = self.top(&["firstLayerUnits"]).is_some_and(|u| u == "absolute");
        let lh = self.settings.layer_height;
        let width = self.settings.line_width();
        let pct_h = self.top_f(&["firstLayerHeightPercentage"]);
        let abs_h = self.top_f(&["firstLayerHeightAbsolute"]);
        let pct_w = self.top_f(&["firstLayerWidthPercentage"]);
        let abs_w = self.top_f(&["firstLayerWidthAbsolute"]);
        let (h, w) = if absolute {
            (abs_h, abs_w.map(|a| a / width * 100.0))
        } else {
            (pct_h.map(|p| lh * p / 100.0), pct_w)
        };
        self.num("first_layer_height", h, "first layer height");
        self.num("first_layer_width_percent", w, "first layer width");

        // perimeter order
        if let Some(v) = self.top(&["printPerimetersInsideOut"]) {
            let order =
                if flag(&v) { PerimeterOrder::InsideOut } else { PerimeterOrder::OutsideIn };
            let i = PerimeterOrder::ALL.iter().position(|o| *o == order);
            self.choice("perimeter_order", i, "perimeter order");
        } else if let Some(v) = self.top(&["perimeterPrintingOrder"]) {
            let order = if v == "outside_in" {
                PerimeterOrder::OutsideIn
            } else {
                PerimeterOrder::InsideOut
            };
            let i = PerimeterOrder::ALL.iter().position(|o| *o == order);
            self.choice("perimeter_order", i, "perimeter order");
        }

        // start points (seams)
        let seam = if let Some(v) = self.top(&["startPointStrategy"]) {
            match v.as_str() {
                "random" => Some(SeamMode::Random),
                "aligned" => Some(SeamMode::Aligned),
                "closest" => Some(SeamMode::Nearest),
                _ => None,
            }
        } else {
            // 4.x numbers the same three choices from 1
            match self.top_f(&["startPointOption"]).map(|v| v as i64) {
                Some(1) => Some(SeamMode::Random),
                Some(2) => Some(SeamMode::Nearest),
                Some(3) => Some(SeamMode::Aligned),
                _ => None,
            }
        };
        if let Some(mode) = seam {
            let i = SeamMode::ALL.iter().position(|m| *m == mode);
            self.choice("seam_mode", i, "start points");
        }
        let v = self.top_f(&["startPointOriginX", "alignStartPointsX"]);
        self.num("seam_x", v, "start point X");
        let v = self.top_f(&["startPointOriginY", "alignStartPointsY"]);
        self.num("seam_y", v, "start point Y");

        let v = self.top(&["spiralVaseMode", "useVaseMode"]);
        self.boolean("vase_mode", v, "vase mode");
        let v = self.top(&["useAdaptiveLayerHeights"]);
        self.boolean("adaptive_layers", v, "adaptive layers");
        let v = self.top_f(&["minAdaptiveLayerHeight"]);
        self.num("adaptive_min_height", v, "adaptive minimum");
        let v = self.top_f(&["maxAdaptiveLayerHeight"]);
        self.num("adaptive_max_height", v, "adaptive maximum");
    }

    fn speeds(&mut self) {
        let default = self.top_f(&["defaultSpeed", "defaultPrintSpeed"]).map(|v| v / 60.0);
        self.num("print_speed", default, "default speed");
        let d = self.settings.print_speed;
        // walls: 4.x has one outline factor — the outer wall runs at it and the
        // inner walls halfway to full speed (measured); 5.x has both
        if let Some(u) = self.top_f(&["outlineUnderspeed"]).map(fraction) {
            let inner = d * (1.0 + u) / 2.0;
            self.num("perimeter_speed", Some(inner), "outline underspeed");
            if inner > 0.0 {
                self.num(
                    "outer_perimeter_speed_percent",
                    Some(d * u / inner * 100.0),
                    "outline underspeed",
                );
            }
        } else {
            let inner = self.top_f(&["innerPerimeterSpeedPercentage"]);
            let outer = self.top_f(&["outerPerimeterSpeedPercentage"]);
            let inner_speed = d * inner.unwrap_or(100.0) / 100.0;
            if inner.is_some() || outer.is_some() {
                self.num("perimeter_speed", Some(inner_speed), "inner perimeter speed");
            }
            if let Some(o) = outer
                && inner_speed > 0.0
            {
                self.num(
                    "outer_perimeter_speed_percent",
                    Some(d * o / 100.0 / inner_speed * 100.0),
                    "outer perimeter speed",
                );
            }
        }
        let v = self
            .top_f(&["solidInfillUnderspeed"])
            .map(|u| fraction(u) * 100.0)
            .or_else(|| self.top_f(&["solidInfillSpeedPercentage"]));
        self.num("solid_infill_speed_percent", v, "solid infill speed");
        let v = self
            .top_f(&["supportUnderspeed"])
            .map(|u| fraction(u) * 100.0)
            .or_else(|| self.top_f(&["sparseSupportSpeedPercentage"]));
        self.num("support_speed_percent", v, "support speed");
        let v = self.top_f(&["topLayerSpeedPercentage"]);
        self.num("top_speed_percent", v, "top layer speed");
        let v = self.top_f(&["rapidXYspeed", "travelSpeedXY"]).map(|v| v / 60.0);
        self.num("travel_speed", v, "travel speed");
        let v = self.top_f(&["rapidZspeed", "travelSpeedZ"]).map(|v| v / 60.0);
        self.num("z_speed", v, "Z speed");
        // first layer speed
        let units_abs = self.top(&["firstLayerUnits"]).is_some_and(|u| u == "absolute");
        let under = self.top_f(&["firstLayerUnderspeed"]).map(fraction);
        let pct = self.top_f(&["firstLayerSpeedPercentage"]);
        let abs = self.top_f(&["firstLayerSpeedAbsolute"]);
        let first = if let Some(u) = under {
            Some(d * u)
        } else if units_abs {
            abs.map(|a| a / 60.0)
        } else {
            pct.map(|p| d * p / 100.0)
        };
        self.num("first_layer_speed", first, "first layer speed");
        let v = self.top_f(&["accelXY"]);
        self.num("max_acceleration", v, "acceleration");
    }

    fn adhesion(&mut self) {
        let use_skirt = self.top(&["useSkirt"]).map(|v| flag(&v));
        let layers = self.top_f(&["skirtLayers"]);
        let loops = self.top_f(&["skirtOutlines"]);
        let offset = self.top_f(&["skirtOffset"]);
        self.mark("skirtExtruder");
        if use_skirt == Some(true) && offset.is_some_and(|o| o <= 0.0) {
            // a skirt that touches the part is a brim
            self.put("skirt_enabled", SettingValue::Bool(false), "skirt");
            self.put("brim_enabled", SettingValue::Bool(true), "skirt");
            self.num("brim_loops", loops, "skirt outlines");
            self.note("a skirt touching the part (offset 0) was imported as a brim");
        } else {
            if let Some(on) = use_skirt {
                self.put("skirt_enabled", SettingValue::Bool(on), "skirt");
            }
            self.num("skirt_loops", loops, "skirt outlines");
            self.num("skirt_distance", offset, "skirt offset");
        }
        self.num("skirt_layers", layers, "skirt layers");
        if let Some(on) = self.top(&["enableBrim"]) {
            self.put("brim_enabled", SettingValue::Bool(flag(&on)), "brim");
        }
        if let Some(w) = self.top_f(&["brimWidth"]) {
            let line = self.settings.line_width();
            if line > 0.0 {
                self.num("brim_loops", Some((w / line).ceil().max(1.0)), "brim width");
            }
        }

        let v = self.top(&["useRaft"]);
        self.boolean("raft_enabled", v, "raft");
        self.mark("raftExtruder");
        let v = self.top_f(&["raftBaseLayers"]);
        self.num("raft_base_layers", v, "raft base layers");
        let v = self.top_f(&["raftTopLayers"]);
        self.num("raft_top_layers", v, "raft top layers");
        let v = self.top_f(&["raftOffset"]);
        self.num("raft_margin", v, "raft offset");
        let v = self.top_f(&["raftSeparationDistance"]);
        self.num("raft_gap", v, "raft separation");
        let v = self.top_f(&["raftTopInfill"]);
        self.num("raft_infill", v, "raft infill");

        let v = self.top(&["usePrimePillar"]);
        self.boolean("prime_tower_enabled", v, "prime pillar");
        let v = self.top_f(&["primePillarWidth"]);
        self.num("prime_tower_size", v, "prime pillar width");
        let v = self.top(&["useOozeShield"]);
        self.boolean("ooze_shield_enabled", v, "ooze shield");
        let v = self.top_f(&["oozeShieldOffset"]);
        self.num("ooze_shield_distance", v, "ooze shield offset");
        let v = self.top_f(&["oozeShieldSidewallAngle"]);
        self.num("ooze_shield_angle", v, "ooze shield angle");
    }

    fn infill(&mut self) {
        let v = self.top_f(&["infillPercentage", "sparseInfillPercentage"]);
        self.num("infill_percentage", v, "infill percentage");
        self.mark("infillExtruder");
        self.mark("sparseInfillExtruder");
        if let Some(name) = self.top(&["internalInfillPattern", "sparseInfillPattern"]) {
            let key = name.trim().to_ascii_lowercase().replace(' ', "_");
            let pattern = match key.as_str() {
                "rectilinear" => Some(InfillPattern::Rectilinear),
                "aligned" => Some(InfillPattern::Lines),
                "grid" => Some(InfillPattern::Grid),
                "triangular" => Some(InfillPattern::Triangles),
                "wiggle" => Some(InfillPattern::Wiggle),
                "fast_honeycomb" | "full_honeycomb" | "honeycomb" => Some(InfillPattern::Honeycomb),
                "gyroid" => Some(InfillPattern::Gyroid),
                "cubic" => Some(InfillPattern::Cubic),
                "octahedral" => Some(InfillPattern::Octahedral),
                "concentric" => Some(InfillPattern::Concentric),
                _ => None,
            };
            match pattern {
                Some(p) => {
                    let i = InfillPattern::ALL.iter().position(|x| *x == p);
                    self.choice("infill_pattern", i, "infill pattern");
                }
                None => self.note(format!(
                    "infill pattern \"{name}\" has no SliceCraft match; kept rectilinear"
                )),
            }
        }
        if let Some(name) = self.top(&["externalInfillPattern"]) {
            let p = match name.trim().to_ascii_lowercase().as_str() {
                "concentric" => SolidPattern::Concentric,
                "aligned" => SolidPattern::Lines,
                _ => SolidPattern::Rectilinear,
            };
            let i = SolidPattern::ALL.iter().position(|x| *x == p);
            self.choice("solid_pattern", i, "solid pattern");
        }
        // angles: 4.x lists them ("45,-45"); 5.x stores a rotation of 45/-45
        let angles = |s: Option<String>| -> Vec<f64> {
            s.map(|s| s.split(',').filter_map(parse_f).take(16).collect()).unwrap_or_default()
        };
        let internal = angles(self.top(&["internalInfillAngles"]));
        let external = angles(self.top(&["externalInfillAngles"]));
        let rot_i = self.top_f(&["sparseInfillRotation"]);
        let rot_e = self.top_f(&["externalInfillRotation"]);
        if let Some(a) = internal.first() {
            self.num("infill_angle", Some(*a), "infill angles");
            self.put("infill_alternate", SettingValue::Bool(internal.len() > 1), "infill angles");
            if internal.len() > 2 {
                self.note("more than two infill angles: imported the first, alternating 90°");
            }
            if let Some(e) = external.first() {
                self.num("solid_angle_offset", Some(e - a), "solid infill angles");
            }
        } else if let Some(r) = rot_i {
            self.num("infill_angle", Some(45.0 + r), "infill rotation");
            if let Some(e) = rot_e {
                self.num("solid_angle_offset", Some(e - r), "solid infill rotation");
            }
        }
        self.mark("overlapInternalInfillAngles");
        let v = self.top_f(&["outlineOverlapPercentage", "infillOutlineOverlapPercentage"]);
        self.num("infill_overlap", v, "outline overlap");
        let v =
            self.top_f(&["infillExtrusionWidthPercentage", "sparseInfillExtrusionWidthPercentage"]);
        self.num("infill_width_percent", v, "infill width");
        let v = self.top_f(&["infillLayerInterval", "sparseInfillCombinedLayers"]);
        self.num("infill_every_layers", v, "combine infill");
        let v = self.top_f(&["minInfillLength"]);
        self.num("infill_min_length", v, "minimum infill length");
        let v = self.top(&["printInfillBeforePerimeters"]);
        self.boolean("infill_before_perimeters", v, "infill before perimeters");
        let v = self.top_f(&["denseInfillLayers"]);
        self.num("dense_layers", v, "dense infill layers");
        let v = self.top_f(&["denseInfillPercentage"]);
        self.num("dense_percentage", v, "dense infill");
        let v = self.top(&["useIroning"]);
        self.boolean("ironing_enabled", v, "ironing");
        let v = self.top(&["useMonotonic"]);
        self.boolean("monotonic_solid", v, "monotonic");
        let v = self.top_f(&["solidInfillThresholdArea"]);
        self.num("solid_min_area", v, "solid infill threshold");
        let v = self.top_f(&["topLayerExtrusionModifier"]);
        self.num("top_flow_percent", v, "top layer extrusion");
        if self.top(&["overrideFanSpeedForTopLayer"]).is_some_and(|v| flag(&v)) {
            let v = self.top_f(&["topLayerFanSpeedPercentage"]);
            self.num("top_fan_percent", v, "top layer fan");
        }
        if self.top(&["useDiaphragm"]).is_some_and(|v| flag(&v)) {
            let v = self.top_f(&["diaphragmLayerInterval", "diaphragmSpacingLayers"]);
            self.num("solid_every_layers", v, "diaphragm interval");
            let v = self.top_f(&["diaphragmSolidLayers"]);
            self.num("solid_every_count", v, "diaphragm layers");
        }
    }

    fn support(&mut self) {
        let v = self.top(&["generateSupport"]);
        self.boolean("support_enabled", v, "support");
        for k in ["supportExtruder", "sparseSupportExtruder", "denseSupportExtruder"] {
            self.mark(k);
        }
        let v = self.top_f(&["supportInfillPercentage", "sparseSupportInfillPercentage"]);
        self.num("support_infill", v, "support infill");
        let v = self.top_f(&["supportExtraInflation", "supportInflationDistance"]);
        self.num("support_expansion", v, "support inflation");
        let v = self.top_f(&["supportBaseLayers", "baseSupportLayers"]);
        self.num("support_base_layers", v, "support base layers");
        let v = self.top_f(&["denseSupportLayers", "upperDenseSupportLayers"]);
        self.num("support_interface_layers", v, "dense support layers");
        let v = self.top_f(&["lowerDenseSupportLayers"]);
        self.num("support_floor_layers", v, "lower dense support layers");
        let v = self.top_f(&["denseSupportInfillPercentage"]);
        self.num("support_interface_infill", v, "dense support infill");
        let v = self.top_f(&["supportLayerInterval", "sparseSupportCombinedLayers"]);
        self.num("support_every_layers", v, "combine support");
        let v = self.top_f(&["supportHorizontalPartOffset"]);
        self.num("support_xy_gap", v, "support horizontal offset");
        let v = self.top_f(&["supportUpperSeparationLayers"]);
        self.num("support_top_gap_layers", v, "upper separation layers");
        let v = self.top_f(&["supportLowerSeparationLayers"]);
        self.num("support_bottom_gap_layers", v, "lower separation layers");
        let v = self.top_f(&["supportGridSpacing"]);
        if v.is_some() {
            // the source always builds pillars on this grid
            self.choice("support_style", Some(0), "support grid");
        }
        self.num("support_pillar_size", v, "support pillar size");
        let v = self.top_f(&["maxOverhangAngle"]);
        self.num("support_angle", v, "overhang angle");
        let v = self.top_f(&["sparseSupportOutlines"]);
        self.num("support_outlines", v, "support outlines");
        if let Some(t) = self.top_f(&["supportType"]) {
            self.choice("support_type", Some(usize::from(t >= 1.0)), "support type");
        }
        if let Some(p) = self.top(&["sparseSupportInfillPattern"]) {
            self.choice("support_pattern", Some(usize::from(p == "grid")), "support pattern");
        }
    }

    fn temperatures(&mut self) {
        self.mark("temperatureController");
        let controllers: Vec<Node<'a, 'i>> =
            self.root.children().filter(|c| c.has_tag_name("temperatureController")).collect();
        let mut nozzle_done = false;
        let mut bed_done = false;
        for c in controllers {
            let child = |name: &str| c.children().find(|n| n.has_tag_name(name)).map(value_of);
            if child("enableTemperatureController").is_some_and(|v| !flag(&v)) {
                continue;
            }
            let kind = match child("temperatureType") {
                Some(t) => t,
                None if child("isHeatedBed").is_some_and(|v| flag(&v)) => "platform".to_string(),
                None => "extruder".to_string(),
            };
            let points = schedule(c, "temperature");
            let Some(&(_, first)) = points.first() else { continue };
            let later = points.iter().find(|(l, _)| *l >= 2).map_or(first, |(_, t)| *t);
            if points.len() > 2 {
                self.note(format!(
                    "a temperature schedule with {} steps: imported the first two",
                    points.len()
                ));
            }
            let wait = child("stabilizeAtStartup").map(|v| flag(&v));
            match kind.as_str() {
                "extruder" if !nozzle_done => {
                    nozzle_done = true;
                    self.num("first_layer_nozzle_temperature", Some(first), "extruder temperature");
                    self.num("nozzle_temperature", Some(later), "extruder temperature");
                    if let Some(w) = wait {
                        self.put(
                            "wait_for_temperatures",
                            SettingValue::Bool(w),
                            "stabilize at startup",
                        );
                    }
                }
                "platform" if !bed_done => {
                    bed_done = true;
                    self.num("first_layer_bed_temperature", Some(first), "bed temperature");
                    self.num("bed_temperature", Some(later), "bed temperature");
                }
                "chamber" => self.num("chamber_temperature", Some(first), "chamber temperature"),
                _ => self.note("a second extruder or bed temperature was not imported"),
            }
        }
    }

    fn cooling(&mut self) {
        self.mark("fanSpeed");
        self.mark("fan");
        let fan = self
            .root
            .children()
            .find(|c| c.has_tag_name("fanSpeed"))
            .or_else(|| self.root.children().find(|c| c.has_tag_name("fan")));
        if let Some(f) = fan {
            let points = schedule(f, "speed");
            if let Some(&(layer, speed)) = points.iter().find(|(_, s)| *s > 0.0) {
                self.num("fan_from_layer", Some(f64::from(layer.max(1))), "fan schedule");
                self.num("fan_percent", Some(speed), "fan schedule");
                if points.iter().any(|(_, s)| *s > 0.0 && (*s - speed).abs() > 0.5) {
                    self.note(
                        "the fan schedule changes speed more than once; imported the first speed",
                    );
                }
            } else if !points.is_empty() {
                self.num("fan_percent", Some(0.0), "fan schedule");
            }
            if let Some(b) =
                f.children().find(|n| n.has_tag_name("blipFanToFullPower")).map(value_of)
            {
                self.put("fan_kickstart", SettingValue::Bool(flag(&b)), "blip fan");
            }
            // 5.x keeps the quick-layer fan boost inside <fan>
            let inner = |name: &str| f.children().find(|n| n.has_tag_name(name)).map(value_of);
            if inner("increaseFanForQuickLayers").is_some_and(|v| flag(&v)) {
                let v = inner("fanMaxQuickLayerTime").as_deref().and_then(parse_f);
                self.num("fan_boost_below_seconds", v, "fan for quick layers");
            }
        }
        let v = self.top(&["blipFanToFullPower"]);
        self.boolean("fan_kickstart", v, "blip fan");
        // slow down short layers
        let slow =
            self.top(&["adjustSpeedForCooling", "reduceSpeedForQuickLayers"]).map(|v| flag(&v));
        let time = self.top_f(&["minSpeedLayerTime", "speedMaxQuickLayerTime"]);
        let pct = self.top_f(&["minCoolingSpeedSlowdown", "minQuickLayerSpeedPercentage"]);
        match slow {
            Some(true) => {
                self.num("min_layer_time", time, "minimum layer time");
                let d = self.settings.print_speed;
                self.num("min_layer_speed", pct.map(|p| d * p / 100.0), "slowest cooling speed");
            }
            Some(false) => self.num("min_layer_time", Some(0.0), "minimum layer time"),
            None => {}
        }
        if self.top(&["increaseFanForCooling"]).is_some_and(|v| flag(&v)) {
            let v = self.top_f(&["minFanLayerTime"]);
            self.num("fan_boost_below_seconds", v, "fan for cooling");
        }
        for k in ["minFanLayerTime", "maxCoolingFanSpeed", "fanSpeedControl"] {
            self.mark(k);
        }
        let bridge_fan = self.top(&["increaseFanForBridging", "overrideFanSpeedForBridging"]);
        if bridge_fan.is_some_and(|v| flag(&v)) {
            let v = self.top_f(&["bridgingFanSpeed", "bridgingFanSpeedPercentage"]);
            self.num("bridge_fan_percent", v, "bridging fan");
        }
        self.mark("bridgingFanSpeed");
        self.mark("bridgingFanSpeedPercentage");
        let v = self.top(&["useBridging"]);
        self.boolean("bridging_enabled", v, "bridging");
        let v = self
            .top_f(&["bridgingExtrusionMultiplier"])
            .map(|m| m * 100.0)
            .or_else(|| self.top_f(&["bridgingExtrusionModifier"]));
        self.num("bridge_flow_percent", v, "bridging extrusion");
        let v = self
            .top_f(&["bridgingSpeedMultiplier"])
            .map(|m| m * 100.0)
            .or_else(|| self.top_f(&["bridgingSpeedModifier"]));
        self.num("bridge_speed_percent", v, "bridging speed");
    }

    fn gcode(&mut self) {
        let v = self.top(&["relativeEdistances", "relativeExtrusionDistances"]);
        self.boolean("relative_extrusion", v, "relative extrusion");
        for (key, names) in [
            ("gcode_offset_x", ["gcodeXoffset", "globalOffsetX"]),
            ("gcode_offset_y", ["gcodeYoffset", "globalOffsetY"]),
            ("gcode_offset_z", ["gcodeZoffset", "globalOffsetZ"]),
        ] {
            let v = self.top_f(&names);
            self.num(key, v, "G-code offset");
        }
        let overridden = self.top(&["overrideFirmwareConfiguration"]).is_none_or(|v| flag(&v));
        if let Some(fw) = self.top(&["firmwareTypeOverride"])
            && overridden
        {
            let lower = fw.to_ascii_lowercase();
            let flavor = if lower.contains("klipper") {
                Some(Flavor::Klipper)
            } else if lower.contains("marlin") || lower.contains("reprap") {
                Some(Flavor::Marlin)
            } else {
                None
            };
            match flavor {
                Some(f) => {
                    let i = Flavor::ALL.iter().position(|x| *x == f);
                    self.choice("flavor", i, "firmware type");
                }
                None => {
                    self.note(format!("firmware \"{fw}\" has no SliceCraft flavour; kept Marlin"))
                }
            }
        }
        if self.top(&["includeThumbnailImages"]).is_some_and(|v| flag(&v)) {
            let size =
                self.root.children().find(|c| c.has_tag_name("thumbnailImageSizes")).and_then(
                    |n| {
                        n.children()
                            .filter_map(|o| o.attribute("value"))
                            .filter_map(|v| v.split('|').next().and_then(parse_f))
                            .reduce(f64::max)
                    },
                );
            self.num("thumbnail_size", size.map(|s| s.min(512.0)), "thumbnails");
        }
        self.mark("thumbnailImageSizes");
        if self.top(&["exportFileFormat"]).is_some_and(|f| !f.eq_ignore_ascii_case("gcode")) {
            self.note("the profile exports a printer-specific file format; SliceCraft writes plain G-code");
        }
    }

    fn machine(&mut self) {
        let v4_override = self.top(&["overrideMachineDefinition"]).map(|v| flag(&v));
        let (x, y, z) = if v4_override.is_some() {
            let x = self.top_f(&["strokeXoverride"]);
            let y = self.top_f(&["strokeYoverride"]);
            let z = self.top_f(&["strokeZoverride"]);
            if v4_override == Some(true) { (x, y, z) } else { (None, None, None) }
        } else {
            (
                self.top_f(&["buildVolumeX"]),
                self.top_f(&["buildVolumeY"]),
                self.top_f(&["buildVolumeZ"]),
            )
        };
        self.num("bed_width", x, "build volume X");
        self.num("bed_depth", y, "build volume Y");
        self.num("bed_height", z, "build volume Z");
        if self.top(&["buildVolumeShape"]).is_some_and(|s| s == "cylindrical") {
            self.choice("bed_shape", Some(1), "build volume shape");
            let v = self.top_f(&["buildVolumeDiameter"]);
            self.num("bed_diameter", v, "build volume diameter");
        }
        self.mark("buildVolumeDiameter");
        // home position: 4.x writes -1/1 per axis, 5.x min/center/max
        let home = |v: Option<String>| -> Option<i8> {
            match v?.as_str() {
                "min" | "-1" => Some(-1),
                "max" | "1" => Some(1),
                "center" | "0" => Some(0),
                _ => None,
            }
        };
        let hx = home(self.top(&["homeXdirOverride", "homingDirectionX"]));
        let hy = home(self.top(&["homeYdirOverride", "homingDirectionY"]));
        self.mark("homeZdirOverride");
        self.mark("homingDirectionZ");
        let corner = match (hx, hy) {
            (Some(-1), Some(-1)) => Some(0),
            (Some(1), Some(-1)) => Some(1),
            (Some(-1), Some(1)) => Some(2),
            (Some(1), Some(1)) => Some(3),
            (Some(0), Some(0)) => Some(4),
            _ => None,
        };
        self.choice("homing_corner", corner, "homing direction");
        let offsets = [
            self.top_f(&["originOffsetXoverride", "originOffsetX"]),
            self.top_f(&["originOffsetYoverride", "originOffsetY"]),
            self.top_f(&["originOffsetZoverride", "originOffsetZ"]),
        ];
        if offsets.iter().flatten().any(|o| o.abs() > 1e-9) {
            self.note("the machine origin offset was not imported; check the bed position");
        }
    }

    /// Script text: lines joined with `,` (4.x) or `|` (5.x), with the
    /// source's variables translated where SliceCraft has one.
    fn script(&mut self, names: &[&str], start: bool) -> Option<String> {
        let parts: Vec<String> = names.iter().filter_map(|n| self.top(&[*n])).collect();
        if parts.iter().all(|p| p.trim().is_empty()) {
            return None;
        }
        let sep = if self.option_lists { '|' } else { ',' };
        let mut lines: Vec<String> = Vec::new();
        for p in parts {
            lines.extend(p.split(sep).map(|l| l.trim_end().to_string()));
        }
        let mut text = lines.join("\n");
        let (nozzle, bed) =
            if start { ("{first_nozzle}", "{first_bed}") } else { ("{nozzle}", "{bed}") };
        for (from, to) in [
            ("[extruder0_temperature]", nozzle),
            ("[bed0_temperature]", bed),
            ("[current_layer]", "{layer}"),
            ("[current_Z_position]", "{z}"),
        ] {
            text = text.replace(from, to);
        }
        if let Some(open) = text.find('[')
            && text.get(open..).is_some_and(|rest| rest.contains(']'))
        {
            self.note(format!(
                "a script uses variables SliceCraft does not know (\"{}…\"); left as written",
                text.get(open..open.saturating_add(24).min(text.len())).unwrap_or("[")
            ));
        }
        Some(text)
    }

    fn scripts(&mut self) {
        let jobs: [(&'static str, &[&str], bool); 7] = [
            ("start_script", &["startingGcode", "startingScript"], true),
            ("end_script", &["endingGcode", "endingScript"], false),
            (
                "layer_script",
                &["layerChangeGcode", "preLayerChangeScript", "postLayerChangeScript"],
                false,
            ),
            (
                "retract_script",
                &["retractionGcode", "preRetractionScript", "postRetractionScript"],
                false,
            ),
            (
                "tool_change_script",
                &["toolChangeGcode", "preToolChangeScript", "postToolChangeScript"],
                false,
            ),
            ("profile_change_script", &["processChangeScript"], false),
            ("post_process", &["postProcessing", "postProcessingScript"], false),
        ];
        for (key, names, start) in jobs {
            if let Some(text) = self.script(names, start) {
                self.put(key, SettingValue::Text(text), key);
            }
        }
    }

    fn advanced(&mut self) {
        let v = self.top(&["onlyRetractWhenCrossingOutline", "onlyRetractWhenCrossingOutlines"]);
        self.boolean("retract_only_outside", v, "retract only when crossing outlines");
        let v = self.top(&["retractBetweenLayers"]);
        self.boolean("retract_on_layer_change", v, "retract between layers");
        let min_travel = self.top(&["useRetractionMinTravel"]).map(|v| flag(&v));
        let distance = self.top_f(&["retractionMinTravel"]);
        match min_travel {
            Some(true) => self.num("retract_min_travel", distance, "minimum travel"),
            Some(false) => self.num("retract_min_travel", Some(0.0), "minimum travel"),
            None => {}
        }
        if self.top(&["useRetractionMinExtrusion"]).is_some_and(|v| flag(&v)) {
            let v = self.top_f(&["retractionMinExtrusion"]);
            self.num("retract_min_extrusion", v, "minimum extrusion");
        }
        self.mark("retractionMinExtrusion");
        let v = self.top(&["avoidCrossingOutline", "avoidCrossingOutlines"]);
        self.boolean("avoid_crossing_walls", v, "avoid crossing outlines");
        let v = self.top_f(&["maxMovementDetourFactor"]);
        self.num("travel_detour_limit", v, "detour factor");
        if let Some(m) = self.top(&["wipingMode"]) {
            let i = match m.as_str() {
                "any_perimeter" => 1,
                "any_feature" => 2,
                _ => 0,
            };
            self.choice("wipe_features", Some(i), "wiping mode");
        }
        let v = self.top_f(&["toolChangeRetractionDistance", "toolChangeRetractDistance"]);
        self.num("tool_change_retract", v, "tool change retraction");
        let v =
            self.top_f(&["toolChangeRetractionSpeed", "toolChangeRetractSpeed"]).map(|v| v / 60.0);
        self.num("tool_change_speed", v, "tool change speed");
        let v = self.top_f(&[
            "singleExtrusionMinPrintingWidthPercentage",
            "singleExtrusionMinWidthPercentage",
        ]);
        self.num("single_line_min_width", v, "single extrusion minimum width");
        let v = self.top_f(&[
            "singleExtrusionMaxPrintingWidthPercentage",
            "singleExtrusionMaxWidthPercentage",
        ]);
        self.num("single_line_max_width", v, "single extrusion maximum width");
        if self.top(&["reduceSpeedForShortPerimeters"]).is_some_and(|v| flag(&v)) {
            let v = self.top_f(&["speedMaxShortPerimeterLength"]);
            self.num("short_wall_length", v, "short perimeters");
        }
        self.mark("speedMaxShortPerimeterLength");
        if self.top(&["use5D"]).is_some_and(|v| !flag(&v)) {
            self.note("the profile turns 5D extrusion off; SliceCraft always writes E values");
        }
        if let Some(t) = self.top(&["internalThinWallType"]) {
            let i = match t.as_str() {
                "perimeters_only" | "0" => Some(0),
                "single_extrusions" | "1" => Some(1),
                "gap_fill" | "2" => Some(2),
                _ => None,
            };
            self.choice("thin_walls", i, "thin walls");
        }
        let v = self.top_f(&["horizontalSizeCompensation", "horizontalOuterCompensation"]);
        self.num("outer_size_offset", v, "horizontal size compensation");
        let v = self.top_f(&["horizontalInnerCompensation"]);
        self.num("hole_size_offset", v, "inner size compensation");
        if let Some(o) = self.top(&["islandPrintingOrder"]) {
            let i = match o.as_str() {
                "consistent" => 1,
                "combined" => 2,
                _ => 0,
            };
            self.choice("island_order", Some(i), "island order");
        }
        if let Some(m) = self.top(&["openLoopSlicingBehavior"]) {
            let i = match m.as_str() {
                "discard" => 1,
                "thicken" => 2,
                _ => 0,
            };
            self.choice("open_outline_mode", Some(i), "open loops");
        }
        if let Some(m) = self.top(&["slicingRegionRepairMode"]) {
            let i = match m.as_str() {
                "alternating" => 1,
                "merge_all" => 2,
                _ => 0,
            };
            self.choice("fill_rule", Some(i), "region repair");
        } else if self.top(&["mergeAllIntoSolid"]).is_some_and(|v| flag(&v)) {
            self.choice("fill_rule", Some(2), "merge all into solid");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A classic (4.x-style) profile written by hand for the tests.
    const CLASSIC: &str = r#"<?xml version="1.0"?>
<profile name="Test classic" version="1" app="S3D-Software 4.0.0">
  <baseProfile>Default</baseProfile>
  <extruder name="Extruder 1">
    <diameter>0.4</diameter>
    <autoWidth>1</autoWidth>
    <width>0.5</width>
    <extrusionMultiplier>0.95</extrusionMultiplier>
    <useRetract>1</useRetract>
    <retractionDistance>2.5</retractionDistance>
    <retractionZLift>0.2</retractionZLift>
    <retractionSpeed>3000</retractionSpeed>
    <somethingNew>7</somethingNew>
  </extruder>
  <layerHeight>0.2</layerHeight>
  <topSolidLayers>5</topSolidLayers>
  <bottomSolidLayers>3</bottomSolidLayers>
  <perimeterOutlines>3</perimeterOutlines>
  <printPerimetersInsideOut>0</printPerimetersInsideOut>
  <startPointOption>3</startPointOption>
  <startPointOriginX>150</startPointOriginX>
  <startPointOriginY>10</startPointOriginY>
  <firstLayerHeightPercentage>150</firstLayerHeightPercentage>
  <firstLayerWidthPercentage>120</firstLayerWidthPercentage>
  <firstLayerUnderspeed>0.5</firstLayerUnderspeed>
  <useSkirt>1</useSkirt>
  <skirtLayers>1</skirtLayers>
  <skirtOutlines>4</skirtOutlines>
  <skirtOffset>0</skirtOffset>
  <internalInfillPattern>Grid</internalInfillPattern>
  <externalInfillPattern>Concentric</externalInfillPattern>
  <infillPercentage>25</infillPercentage>
  <internalInfillAngles>30,-60</internalInfillAngles>
  <externalInfillAngles>45,-45</externalInfillAngles>
  <temperatureController name="Extruder 1 Temperature">
    <isHeatedBed>0</isHeatedBed>
    <stabilizeAtStartup>1</stabilizeAtStartup>
    <setpoint layer="1" temperature="215"/>
    <setpoint layer="3" temperature="205"/>
  </temperatureController>
  <temperatureController name="Heated Bed">
    <isHeatedBed>1</isHeatedBed>
    <setpoint layer="1" temperature="65"/>
  </temperatureController>
  <fanSpeed>
    <setpoint layer="1" speed="0"/>
    <setpoint layer="3" speed="80"/>
  </fanSpeed>
  <adjustSpeedForCooling>1</adjustSpeedForCooling>
  <minSpeedLayerTime>12</minSpeedLayerTime>
  <minCoolingSpeedSlowdown>50</minCoolingSpeedSlowdown>
  <overrideMachineDefinition>1</overrideMachineDefinition>
  <strokeXoverride>300</strokeXoverride>
  <strokeYoverride>310</strokeYoverride>
  <strokeZoverride>400</strokeZoverride>
  <overrideFirmwareConfiguration>1</overrideFirmwareConfiguration>
  <firmwareTypeOverride>RepRap (Marlin/Repetier/Sprinter)</firmwareTypeOverride>
  <startingGcode>G28 ; home,M117 heating to [extruder0_temperature] &amp; go</startingGcode>
  <endingGcode>M104 S0,M84</endingGcode>
  <defaultSpeed>3600</defaultSpeed>
  <outlineUnderspeed>0.8</outlineUnderspeed>
  <solidInfillUnderspeed>0.75</solidInfillUnderspeed>
  <rapidXYspeed>6000</rapidXYspeed>
  <rapidZspeed>600</rapidZspeed>
  <filamentDiameters>1.73|1.75</filamentDiameters>
  <filamentPricesPerKg>30|46</filamentPricesPerKg>
  <filamentDensities>1.24|1.25</filamentDensities>
  <useRetractionMinTravel>1</useRetractionMinTravel>
  <retractionMinTravel>4</retractionMinTravel>
  <celebration>1</celebration>
</profile>
"#;

    /// An option-list (5.x-style) profile written by hand for the tests.
    const OPTIONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<profile name="Test options" version="1" app="S3D-Software 5.1.2" technology="fff">
  <extruder name="Extruder 1">
    <nozzleDiameter>0.60000</nozzleDiameter>
    <extrusionWidthMode>
      <option value="automatic" selected="1"/>
      <option value="manual"/>
    </extrusionWidthMode>
    <extrusionWidth>0.40000</extrusionWidth>
    <useRetraction>1</useRetraction>
    <retractDistance>1.00000</retractDistance>
    <retractSpeed>2400.00000</retractSpeed>
    <filamentDiameter>1.75000</filamentDiameter>
  </extruder>
  <layerHeight>0.30000</layerHeight>
  <outlinePerimeters>2</outlinePerimeters>
  <firstLayerUnits>
    <option value="percentage" selected="1"/>
    <option value="absolute"/>
  </firstLayerUnits>
  <firstLayerHeightPercentage>100</firstLayerHeightPercentage>
  <firstLayerSpeedPercentage>40</firstLayerSpeedPercentage>
  <startPointStrategy>
    <option value="random" selected="1"/>
    <option value="closest"/>
    <option value="aligned"/>
  </startPointStrategy>
  <sparseInfillPattern>
    <option value="rectilinear"/>
    <option value="gyroid" selected="1"/>
  </sparseInfillPattern>
  <sparseInfillPercentage>15</sparseInfillPercentage>
  <temperatureController name="Bed">
    <temperatureType>
      <option value="extruder"/>
      <option value="platform" selected="1"/>
    </temperatureType>
    <temperatureSetpoints><option value="1|70"/></temperatureSetpoints>
  </temperatureController>
  <temperatureController name="Nozzle">
    <temperatureType>
      <option value="extruder" selected="1"/>
      <option value="platform"/>
    </temperatureType>
    <stabilizeAtStartup>0</stabilizeAtStartup>
    <temperatureSetpoints><option value="1|240"/><option value="2|235"/></temperatureSetpoints>
  </temperatureController>
  <fan name="Cooling Fan">
    <fanSetpoints><option value="1|0"/><option value="2|60"/></fanSetpoints>
  </fan>
  <defaultPrintSpeed>4800.00000</defaultPrintSpeed>
  <outerPerimeterSpeedPercentage>50</outerPerimeterSpeedPercentage>
  <innerPerimeterSpeedPercentage>80</innerPerimeterSpeedPercentage>
  <buildVolumeX>250.00000</buildVolumeX>
  <buildVolumeY>210.00000</buildVolumeY>
  <buildVolumeZ>220.00000</buildVolumeZ>
  <startingScript>G28|G1 Z5 ; lift</startingScript>
  <endingScript>M84</endingScript>
</profile>
"#;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn recognises_profiles_by_content() {
        assert!(looks_like_fff(CLASSIC));
        assert!(looks_like_fff(OPTIONS));
        assert!(looks_like_fff("\u{feff}<profile name=\"x\"></profile>"));
        assert!(!looks_like_fff("# SliceCraft profile 1\n"));
        assert!(!looks_like_fff("solid cube"));
    }

    #[test]
    fn classic_profile_maps_onto_slicecraft_settings() {
        let r = import_fff(CLASSIC);
        assert!(r.is_ok(), "{r:?}");
        let Ok((p, report)) = r else { return };
        let s = &p.settings;
        assert_eq!(p.name, "Test classic");
        assert_eq!(report.written_by, "S3D-Software 4.0.0");
        // automatic width → fixed 1.2 × nozzle
        assert!(!s.extrusion_width_auto && close(s.extrusion_width, 0.48));
        assert!(close(s.extrusion_multiplier, 0.95));
        assert!(close(s.retract_length, 2.5) && close(s.retract_lift, 0.2));
        assert!(close(s.retract_speed, 50.0), "mm/min → mm/s");
        assert!(close(s.layer_height, 0.2) && close(s.first_layer_height, 0.3));
        assert_eq!((s.solid_layers_top, s.solid_layers_bottom, s.perimeter_count), (5, 3, 3));
        assert_eq!(s.perimeter_order, PerimeterOrder::OutsideIn);
        assert_eq!(s.seam_mode, SeamMode::Aligned);
        assert!(close(s.seam_x, 150.0) && close(s.seam_y, 10.0));
        // speeds: default 60 mm/s, outer 48, inner 54, solid 45, first layer 30
        assert!(close(s.print_speed, 60.0));
        assert!(close(s.perimeter_speed, 54.0));
        assert!(close(s.perimeter_speed * s.outer_perimeter_speed_percent / 100.0, 48.0));
        assert!(close(s.solid_infill_speed_percent, 75.0));
        assert!(close(s.first_layer_speed, 30.0));
        assert!(close(s.travel_speed, 100.0) && close(s.z_speed, 10.0));
        // skirt at offset 0 is a brim
        assert!(!s.skirt_enabled && s.brim_enabled && s.brim_loops == 4);
        assert_eq!(s.infill_pattern, InfillPattern::Grid);
        assert_eq!(s.solid_pattern, SolidPattern::Concentric);
        assert!(close(s.infill_percentage, 25.0) && close(s.infill_angle, 30.0));
        // temperatures and fan
        assert!(close(s.first_layer_nozzle_temperature, 215.0));
        assert!(close(s.nozzle_temperature, 205.0));
        assert!(close(s.first_layer_bed_temperature, 65.0) && close(s.bed_temperature, 65.0));
        assert!(s.wait_for_temperatures);
        assert_eq!(s.fan_from_layer, 3);
        assert!(close(s.fan_percent, 80.0));
        // machine, firmware, filament
        assert!(
            close(s.bed_width, 300.0) && close(s.bed_depth, 310.0) && close(s.bed_height, 400.0)
        );
        assert_eq!(s.flavor, Flavor::Marlin);
        assert!(close(s.filament_diameter, 1.73) && close(s.filament_cost, 30.0));
        assert!(close(s.filament_density, 1.24));
        assert!(close(s.retract_min_travel, 4.0));
        // scripts: `,` splits lines, entities decode, variables translate
        assert_eq!(s.start_script, "G28 ; home\nM117 heating to {first_nozzle} & go");
        assert_eq!(s.end_script, "M104 S0\nM84");
        // placeholders and the report
        assert_eq!(s.get("min_layer_time"), Some(SettingValue::Float(12.0)));
        assert_eq!(s.get("min_layer_speed"), Some(SettingValue::Float(30.0)));
        assert!(report.applied.contains(&"layer_height"));
        assert!(report.placeholders.contains(&"min_layer_time"));
        assert!(report.ignored.contains(&"celebration".to_string()));
        assert!(report.ignored.contains(&"extruder/somethingNew".to_string()));
        assert!(report.notes.iter().any(|n| n.contains("brim")));
    }

    #[test]
    fn option_list_profile_maps_onto_slicecraft_settings() {
        let r = import_fff(OPTIONS);
        assert!(r.is_ok(), "{r:?}");
        let Ok((p, report)) = r else { return };
        let s = &p.settings;
        assert_eq!(report.written_by, "S3D-Software 5.1.2");
        assert!(!s.extrusion_width_auto && close(s.extrusion_width, 0.72), "1.2 × 0.6");
        assert!(close(s.retract_speed, 40.0));
        assert!(close(s.layer_height, 0.3) && close(s.first_layer_height, 0.3));
        assert_eq!(s.seam_mode, SeamMode::Random);
        assert_eq!(s.infill_pattern, InfillPattern::Gyroid);
        assert!(close(s.infill_percentage, 15.0));
        assert!(close(s.print_speed, 80.0));
        assert!(close(s.perimeter_speed, 64.0));
        assert!(close(s.perimeter_speed * s.outer_perimeter_speed_percent / 100.0, 40.0));
        assert!(close(s.first_layer_speed, 32.0));
        assert!(
            close(s.first_layer_nozzle_temperature, 240.0) && close(s.nozzle_temperature, 235.0)
        );
        assert!(close(s.bed_temperature, 70.0));
        assert!(!s.wait_for_temperatures);
        assert_eq!(s.fan_from_layer, 2);
        assert!(close(s.fan_percent, 60.0));
        assert!(close(s.bed_width, 250.0) && close(s.bed_depth, 210.0));
        assert_eq!(s.start_script, "G28\nG1 Z5 ; lift", "`|` splits lines in option-list files");
    }

    #[test]
    fn out_of_range_values_keep_defaults_with_a_note() {
        let text =
            CLASSIC.replace("<layerHeight>0.2</layerHeight>", "<layerHeight>55</layerHeight>");
        let r = import_fff(&text);
        assert!(r.as_ref().is_ok_and(|(p, rep)| {
            close(p.settings.layer_height, PrintSettings::default().layer_height)
                && rep.notes.iter().any(|n| n.starts_with("layer height"))
        }));
    }

    #[test]
    fn hostile_files_are_refused_not_crashed() {
        for bad in [
            "",
            "<profile",
            "<?xml version=\"1.0\"?><notprofile/>",
            "<!DOCTYPE x [<!ENTITY a \"aaaa\">]><profile>&a;</profile>",
            "<profile><layerHeight>NaN</layerHeight><extruder><diameter>-1</diameter></extruder></profile>",
        ] {
            let r = import_fff(bad);
            // either refused, or imported with defaults kept
            if let Ok((p, _)) = r {
                assert!(p.settings.validate().is_ok());
            }
        }
        let big = format!("<profile>{}</profile>", "<a/>".repeat(300_000));
        assert!(import_fff(&big).is_err(), "node limit");
        // every prefix of a real-shaped file reads or is refused
        let mut cut = 0;
        while cut < CLASSIC.len() {
            let _ = import_fff(CLASSIC.get(..cut).unwrap_or(""));
            cut += 37;
        }
    }

    /// Imports every `.fff` under `SLICECRAFT_FFF_CORPUS` (not in the repo).
    #[test]
    fn corpus_profiles_import() {
        let Some(dir) = std::env::var_os("SLICECRAFT_FFF_CORPUS") else { return };
        let mut stack = vec![std::path::PathBuf::from(dir)];
        let mut count = 0;
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else { continue };
            for e in entries.flatten() {
                let path = e.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("fff")) {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    let r = import_fff(&text);
                    assert!(r.is_ok(), "{}: {r:?}", path.display());
                    if let Ok((p, rep)) = r {
                        eprintln!(
                            "{}: {} applied, {} planned, {} ignored, notes: {:?}",
                            p.name,
                            rep.applied.len(),
                            rep.placeholders.len(),
                            rep.ignored.len(),
                            rep.notes
                        );
                        if std::env::var_os("SLICECRAFT_FFF_VERBOSE").is_some() {
                            eprintln!("  ignored: {}", rep.ignored.join(" "));
                        }
                    }
                    count += 1;
                }
            }
        }
        eprintln!("imported {count} profiles");
    }
}
