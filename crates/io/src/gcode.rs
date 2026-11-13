//! G-code output.
//!
//! `GcodeWriter` is the only place in SliceCraft that formats a move. It owns
//! the extruder state (axis position, retraction), the feed rate, the fan and
//! the running estimates, so the pipeline above it never thinks about
//! dialects. The start and end sequences are SliceCraft's own, written from the
//! public RepRap/Marlin/Klipper command documentation.

use slicecraft_doc::{Bed, Flavor, PrintSettings};
use slicecraft_render::{LayerPaths, PathKind, Point2, ToolPath};
use std::fmt::Write as _;

/// Clearance lift at the end of the print, mm.
const END_LIFT: f64 = 10.0;

/// Material cross-section of one extruded line, mm²: a rectangle with
/// semicircular sides (`(w − h)·h + π·h²/4`), the usual model for a squashed
/// bead; a plain rectangle when the line is narrower than it is tall.
pub fn bead_area(width: f64, height: f64) -> f64 {
    if width >= height {
        (width - height) * height + std::f64::consts::PI * height * height / 4.0
    } else {
        width * height
    }
}

/// Millimetres of filament per millimetre of path.
pub fn filament_per_mm(width: f64, height: f64, settings: &PrintSettings) -> f64 {
    let filament_area = std::f64::consts::PI * settings.filament_diameter.powi(2) / 4.0;
    if filament_area <= 0.0 {
        return 0.0;
    }
    bead_area(width, height) / filament_area * settings.extrusion_multiplier
}

/// Feed speed for a feature on a layer, mm/s: the one rule the writer and the
/// preview share.
/// True when a start script heats the nozzle or bed itself (an `M104`,
/// `M109`, `M140` or `M190` command, or a temperature placeholder).
pub fn script_sets_temperatures(script: &str) -> bool {
    script.contains("{nozzle}")
        || script.contains("{bed}")
        || script.contains("{first_nozzle}")
        || script.contains("{first_bed}")
        || script.lines().any(|l| {
            let code = l.split(';').next().unwrap_or("").trim().to_ascii_uppercase();
            let word = code.split_whitespace().next().unwrap_or("");
            matches!(word, "M104" | "M109" | "M140" | "M190")
        })
}

pub fn path_speed(s: &PrintSettings, kind: PathKind, layer_index: usize) -> f64 {
    if layer_index == 0 {
        return s.first_layer_speed;
    }
    match kind {
        PathKind::Skirt | PathKind::Brim => s.first_layer_speed,
        PathKind::OuterPerimeter => s.perimeter_speed * s.outer_perimeter_speed_percent / 100.0,
        PathKind::InnerPerimeter => s.perimeter_speed,
        PathKind::SolidInfill => s.print_speed * s.solid_infill_speed_percent / 100.0,
        PathKind::SparseInfill => s.print_speed,
    }
}

/// The written file and what it will cost.
#[derive(Debug, Clone, PartialEq)]
pub struct GcodeOutput {
    pub text: String,
    pub stats: GcodeStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GcodeStats {
    pub layers: usize,
    /// Filament pushed into the nozzle, mm (retractions excluded).
    pub filament_mm: f64,
    /// Weight at the profile's filament density, grams.
    pub filament_grams: f64,
    /// Price at the profile's cost per kilogram.
    pub filament_cost: f64,
    /// Path length at constant feed rate, no acceleration: an optimistic estimate.
    pub estimated_seconds: f64,
    pub extruded_mm: f64,
    pub travel_mm: f64,
    pub retractions: usize,
}

/// Writes G-code for one tool, in absolute millimetres and absolute E.
pub struct GcodeWriter<'a> {
    out: String,
    settings: &'a PrintSettings,
    pos: Option<Point2>,
    z: Option<f64>,
    /// Commanded E axis position.
    e: f64,
    /// Filament currently pulled back by a retraction.
    retracted: f64,
    feed: Option<f64>,
    kind: Option<PathKind>,
    stats: GcodeStats,
}

impl<'a> GcodeWriter<'a> {
    pub fn new(settings: &'a PrintSettings) -> Self {
        Self {
            out: String::new(),
            settings,
            pos: None,
            z: None,
            e: 0.0,
            retracted: 0.0,
            feed: None,
            kind: None,
            stats: GcodeStats::default(),
        }
    }

    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    pub fn comment(&mut self, text: &str) {
        for l in text.lines() {
            let _ = writeln!(self.out, "; {l}");
        }
    }

    /// `F` only when the feed changes, as firmware keeps the last one.
    fn feed_word(&mut self, mm_per_s: f64) -> String {
        let f = (mm_per_s * 60.0).round();
        if self.feed == Some(f) {
            String::new()
        } else {
            self.feed = Some(f);
            format!(" F{f:.0}")
        }
    }

    /// Insert a user script with its placeholders filled in.
    fn script(&mut self, text: &str, layer: Option<&LayerPaths>) {
        let s = self.settings;
        let mut t = text
            .replace("{nozzle}", &format!("{:.0}", s.nozzle_temperature))
            .replace("{bed}", &format!("{:.0}", s.bed_temperature))
            .replace("{first_nozzle}", &format!("{:.0}", s.first_layer_nozzle_temperature))
            .replace("{first_bed}", &format!("{:.0}", s.first_layer_bed_temperature));
        if let Some(l) = layer {
            t = t
                .replace("{layer}", &(l.index + 1).to_string())
                .replace("{z}", &format!("{:.3}", l.z));
        }
        for l in t.lines() {
            self.line(l.trim_end());
        }
    }

    fn start(&mut self) {
        let s = self.settings;
        let (nozzle, bed) = (s.first_layer_nozzle_temperature, s.first_layer_bed_temperature);
        let script = s.start_script.trim();
        // heat first, unless the script sets temperatures itself; a script
        // without heating would otherwise prime a cold nozzle
        if script.is_empty() || !script_sets_temperatures(script) {
            if bed > 0.0 {
                let _ = writeln!(self.out, "M140 S{bed:.0} ; bed heating");
            }
            let _ = writeln!(self.out, "M104 S{nozzle:.0} ; nozzle heating");
            if s.wait_for_temperatures {
                if bed > 0.0 {
                    let _ = writeln!(self.out, "M190 S{bed:.0} ; wait for bed");
                }
                let _ = writeln!(self.out, "M109 S{nozzle:.0} ; wait for nozzle");
            }
        }
        if script.is_empty() {
            self.line("G21 ; millimetres");
            self.line("G28 ; home all axes");
        } else {
            self.line("; start script");
            let text = s.start_script.clone();
            self.script(&text, None);
        }
        // the writer's own assumptions hold whatever the script did
        self.line("G90 ; absolute coordinates");
        self.line("M82 ; absolute extrusion");
        self.line("G92 E0");
        self.line("M107 ; fan off");
        self.e = 0.0;
        self.retracted = 0.0;
    }

    fn retract(&mut self) {
        let r = self.settings.retraction();
        // nothing extruded yet: there is no pressure to relieve
        if r <= 0.0 || self.retracted > 0.0 || self.stats.filament_mm <= 0.0 {
            return;
        }
        self.e -= r;
        self.retracted = r;
        let f = self.feed_word(self.settings.retract_speed);
        let _ = writeln!(self.out, "G1 E{:.5}{f}", self.e);
        self.stats.retractions += 1;
        self.stats.estimated_seconds += r / self.settings.retract_speed;
    }

    fn unretract(&mut self) {
        if self.retracted <= 0.0 {
            return;
        }
        self.e += self.retracted;
        let r = self.retracted;
        self.retracted = 0.0;
        let f = self.feed_word(self.settings.retract_speed);
        let _ = writeln!(self.out, "G1 E{:.5}{f}", self.e);
        self.stats.estimated_seconds += r / self.settings.retract_speed;
    }

    /// Move to `z` for a new layer: retract, lift, reset E.
    fn layer_change(&mut self, layer: &LayerPaths) {
        let _ = writeln!(
            self.out,
            ";LAYER:{} Z:{:.3} HEIGHT:{:.3}",
            layer.index, layer.z, layer.height
        );
        if self.settings.retract_on_layer_change {
            self.retract();
        }
        let z_speed = self.settings.z_speed.min(self.settings.travel_speed);
        let f = self.feed_word(z_speed);
        let _ = writeln!(self.out, "G0 Z{:.3}{f}", layer.z + self.settings.gcode_offset_z);
        let dz = self.z.map_or(layer.z, |z| (layer.z - z).abs());
        self.stats.estimated_seconds += dz / z_speed;
        self.z = Some(layer.z);
        let s = self.settings;
        if layer.index == 1 {
            if s.nozzle_temperature != s.first_layer_nozzle_temperature {
                let _ = writeln!(
                    self.out,
                    "M104 S{:.0} ; nozzle to printing temperature",
                    s.nozzle_temperature
                );
            }
            if s.bed_temperature != s.first_layer_bed_temperature {
                let _ = writeln!(
                    self.out,
                    "M140 S{:.0} ; bed to printing temperature",
                    s.bed_temperature
                );
            }
        }
        if !s.layer_script.trim().is_empty() {
            let text = s.layer_script.clone();
            self.script(&text, Some(layer));
        }
        // Keep E small: firmware stores it in single precision. The reset names
        // the current (possibly retracted) filament position zero, so a pending
        // unretraction then moves to +retracted.
        self.line("G92 E0");
        self.e = 0.0;
        let fan_layer = layer.index + 1;
        let from = self.settings.fan_from_layer.max(1) as usize;
        if fan_layer == from && self.settings.fan_percent > 0.0 {
            let duty = (self.settings.fan_percent / 100.0 * 255.0).round();
            let _ = writeln!(self.out, "M106 S{duty:.0} ; fan on");
        }
        self.kind = None;
    }

    fn travel_to(&mut self, p: Point2) {
        let d = self.pos.map(|q| q.distance(p));
        if d.is_some_and(|d| d < 1e-6) {
            return;
        }
        let mut lifted = None;
        if d.is_none_or(|d| d >= self.settings.retract_min_travel) {
            let was = self.retracted;
            self.retract();
            // Z hop only for a travel that retracted here, on a known layer
            let hop = self.settings.retract_lift;
            if hop > 0.0
                && was <= 0.0
                && self.retracted > 0.0
                && let Some(z) = self.z
            {
                let f = self.feed_word(self.settings.z_speed);
                let _ = writeln!(self.out, "G0 Z{:.3}{f}", z + hop + self.settings.gcode_offset_z);
                self.stats.estimated_seconds += 2.0 * hop / self.settings.z_speed;
                lifted = Some(z);
            }
        }
        let f = self.feed_word(self.settings.travel_speed);
        let (ox, oy) = (self.settings.gcode_offset_x, self.settings.gcode_offset_y);
        let _ = writeln!(self.out, "G0 X{:.3} Y{:.3}{f}", p.x + ox, p.y + oy);
        if let Some(z) = lifted {
            let f = self.feed_word(self.settings.z_speed);
            let _ = writeln!(self.out, "G0 Z{:.3}{f}", z + self.settings.gcode_offset_z);
        }
        if let Some(d) = d {
            self.stats.travel_mm += d;
            self.stats.estimated_seconds += d / self.settings.travel_speed;
        }
        self.pos = Some(p);
    }

    fn extrude_to(&mut self, p: Point2, speed: f64, e_per_mm: f64) {
        let Some(from) = self.pos else {
            self.travel_to(p);
            return;
        };
        let d = from.distance(p);
        if d < 1e-6 {
            return;
        }
        self.unretract();
        let de = d * e_per_mm;
        self.e += de;
        let f = self.feed_word(speed);
        let (ox, oy) = (self.settings.gcode_offset_x, self.settings.gcode_offset_y);
        let _ = writeln!(self.out, "G1 X{:.3} Y{:.3} E{:.5}{f}", p.x + ox, p.y + oy, self.e);
        self.stats.extruded_mm += d;
        self.stats.filament_mm += de;
        self.stats.estimated_seconds += d / speed;
        self.pos = Some(p);
    }

    /// One path: travel to its start, then extrude along it.
    pub fn path(&mut self, path: &ToolPath, layer: &LayerPaths) {
        let Some(start) = path.start() else { return };
        if self.kind != Some(path.kind) {
            let _ = writeln!(self.out, ";TYPE:{}", path.kind.label());
            self.kind = Some(path.kind);
        }
        let speed = path_speed(self.settings, path.kind, layer.index);
        let e_per_mm = filament_per_mm(self.settings.line_width(), layer.height, self.settings);
        self.travel_to(start);
        for p in path.points.iter().skip(1) {
            self.extrude_to(*p, speed, e_per_mm);
        }
        if path.closed && path.points.len() > 2 {
            self.extrude_to(start, speed, e_per_mm);
        }
    }

    pub fn layer(&mut self, layer: &LayerPaths) {
        self.layer_change(layer);
        for p in &layer.paths {
            self.path(p, layer);
        }
        self.stats.layers += 1;
    }

    fn end(&mut self, bed: &Bed) {
        self.retract();
        if !self.settings.end_script.trim().is_empty() {
            self.line("; end script");
            let text = self.settings.end_script.clone();
            self.script(&text, None);
            return;
        }
        self.line("M107 ; fan off");
        self.line("M104 S0 ; nozzle off");
        self.line("M140 S0 ; bed off");
        let lift = self.z.map_or(END_LIFT, |z| (z + END_LIFT).min(bed.height));
        let f = self.feed_word(self.settings.z_speed);
        let _ = writeln!(self.out, "G0 Z{lift:.3}{f} ; clear the part");
        self.line("G28 X Y ; park");
        self.line("M84 ; motors off");
    }
}

/// Write a complete job: header, start sequence, every layer, end sequence.
pub fn write_gcode(layers: &[LayerPaths], settings: &PrintSettings, bed: &Bed) -> GcodeOutput {
    let mut w = GcodeWriter::new(settings);
    w.start();
    for layer in layers {
        w.layer(layer);
    }
    w.end(bed);
    let mut stats = w.stats;
    let filament_area = std::f64::consts::PI * settings.filament_diameter.powi(2) / 4.0;
    stats.filament_grams = stats.filament_mm * filament_area / 1000.0 * settings.filament_density;
    stats.filament_cost = stats.filament_grams / 1000.0 * settings.filament_cost;

    let mut text = String::with_capacity(w.out.len() + 1024);
    let _ = writeln!(text, "; generated by SliceCraft {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(text, "; flavor: {}", flavor_label(settings.flavor));
    let _ = writeln!(text, "; layers: {}", stats.layers);
    let _ = writeln!(
        text,
        "; layer height: {:.3} mm, first layer {:.3} mm, width {:.3} mm",
        settings.layer_height,
        settings.first_layer_height,
        settings.line_width()
    );
    let _ = writeln!(
        text,
        "; perimeters: {}, solid layers top/bottom: {}/{}, infill: {:.0}%",
        settings.perimeter_count,
        settings.solid_layers_top,
        settings.solid_layers_bottom,
        settings.infill_percentage
    );
    let _ = writeln!(
        text,
        "; filament: {:.1} mm ({:.1} g at {} g/cm3)",
        stats.filament_mm, stats.filament_grams, settings.filament_density
    );
    let _ = writeln!(
        text,
        "; estimated time (no acceleration): {}",
        format_duration(stats.estimated_seconds)
    );
    text.push_str(&w.out);
    GcodeOutput { text, stats }
}

fn flavor_label(f: Flavor) -> &'static str {
    match f {
        Flavor::Marlin => "Marlin",
        Flavor::RepRap => "RepRap",
        Flavor::Klipper => "Klipper",
    }
}

/// `1h 02m 03s`-style duration.
pub fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unknown".into();
    }
    let s = seconds.round() as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 { format!("{h}h {m:02}m {s:02}s") } else { format!("{m}m {s:02}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: (f64, f64), b: (f64, f64), kind: PathKind) -> ToolPath {
        ToolPath { kind, points: vec![Point2::new(a.0, a.1), Point2::new(b.0, b.1)], closed: false }
    }

    fn one_layer(paths: Vec<ToolPath>) -> Vec<LayerPaths> {
        vec![LayerPaths { index: 0, z: 0.3, height: 0.3, paths }]
    }

    #[test]
    fn bead_area_matches_the_model() {
        let a = bead_area(0.45, 0.2);
        assert!((a - (0.25 * 0.2 + std::f64::consts::PI * 0.01)).abs() < 1e-12);
        assert!((bead_area(0.1, 0.2) - 0.02).abs() < 1e-12);
    }

    #[test]
    fn moving_between_paths_is_a_travel_not_an_extrusion() {
        // regression: the old writer extruded from one segment's end to the next start
        let s = PrintSettings::default();
        let layers = one_layer(vec![
            line((0.0, 0.0), (10.0, 0.0), PathKind::SolidInfill),
            line((50.0, 50.0), (60.0, 50.0), PathKind::SolidInfill),
        ]);
        let out = write_gcode(&layers, &s, &Bed::default());
        assert!(out.text.contains("G0 X50.000 Y50.000"), "{}", out.text);
        assert!(!out.text.contains("G1 X50.000 Y50.000"));
        // the long travel retracted (no retraction before the first extrusion)
        assert_eq!(out.stats.retractions, 2); // travel, end
        assert!((out.stats.extruded_mm - 20.0).abs() < 1e-9);
    }

    #[test]
    fn extrusion_follows_width_and_height() {
        // regression: the old writer used a fixed 0.45 width and a 0.05 factor
        let mut s = PrintSettings::default();
        let layers = one_layer(vec![line((0.0, 0.0), (100.0, 0.0), PathKind::SolidInfill)]);
        let a = write_gcode(&layers, &s, &Bed::default()).stats.filament_mm;
        s.extrusion_width = 0.9;
        let b = write_gcode(&layers, &s, &Bed::default()).stats.filament_mm;
        let expect_a = 100.0 * filament_per_mm(0.45, 0.3, &PrintSettings::default());
        assert!((a - expect_a).abs() < 1e-9);
        assert!(b > a * 2.0, "wider line uses more than twice the filament");
    }

    #[test]
    fn closed_path_returns_to_start() {
        let s = PrintSettings::default();
        let ring = ToolPath {
            kind: PathKind::OuterPerimeter,
            points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), Point2::new(10.0, 10.0)],
            closed: true,
        };
        let out = write_gcode(&one_layer(vec![ring]), &s, &Bed::default());
        assert!((out.stats.extruded_mm - (20.0 + 200f64.sqrt())).abs() < 1e-9);
    }

    #[test]
    fn header_start_layers_fan_and_end() {
        let s = PrintSettings::default();
        let mut layers = one_layer(vec![line((0.0, 0.0), (10.0, 0.0), PathKind::Skirt)]);
        layers.push(LayerPaths {
            index: 1,
            z: 0.5,
            height: 0.2,
            paths: vec![line((0.0, 0.0), (10.0, 0.0), PathKind::SparseInfill)],
        });
        let out = write_gcode(&layers, &s, &Bed::default());
        let t = &out.text;
        assert!(t.starts_with("; generated by SliceCraft"));
        for needle in [
            "M190 S60",
            "M109 S210",
            "M82",
            "G28 ; home",
            ";LAYER:0 Z:0.300",
            ";LAYER:1 Z:0.500",
            "M106 S255",
            ";TYPE:skirt",
            "M104 S0",
            "M84",
        ] {
            assert!(t.contains(needle), "missing {needle}");
        }
        assert_eq!(out.stats.layers, 2);
        // fan switches on at layer 2, after layer 1 started
        let fan = t.find("M106").unwrap_or(0);
        assert!(fan > t.find(";LAYER:1").unwrap_or(usize::MAX));
    }

    #[test]
    fn unretract_after_layer_reset_restores_the_filament() {
        // regression: after `G92 E0` while retracted, the writer unretracted to E0,
        // a no-op that starved the start of every layer
        let s = PrintSettings::default();
        let mut layers = one_layer(vec![line((0.0, 0.0), (10.0, 0.0), PathKind::Skirt)]);
        layers.push(LayerPaths {
            index: 1,
            z: 0.5,
            height: 0.2,
            paths: vec![line((0.0, 0.0), (10.0, 0.0), PathKind::SparseInfill)],
        });
        let text = write_gcode(&layers, &s, &Bed::default()).text;
        let after = text.split(";LAYER:1").nth(1).unwrap_or_default();
        let mut lines = after.lines().skip_while(|l| !l.starts_with("G92 E0"));
        assert_eq!(lines.next(), Some("G92 E0"));
        let unretract = lines.find(|l| l.starts_with("G1 E")).unwrap_or_default();
        assert!(unretract.starts_with("G1 E1.00000"), "{unretract}");
        // and nothing retracts before the first extrusion
        assert!(!text.split(";TYPE:skirt").next().unwrap_or_default().contains("G1 E-"));
    }

    #[test]
    fn z_hop_lifts_and_lowers_around_retracted_travel() {
        let s = PrintSettings { retract_lift: 0.4, ..PrintSettings::default() };
        let layers = one_layer(vec![
            line((0.0, 0.0), (10.0, 0.0), PathKind::SolidInfill),
            line((50.0, 50.0), (60.0, 50.0), PathKind::SolidInfill),
        ]);
        let t = write_gcode(&layers, &s, &Bed::default()).text;
        let hop = t.find("G0 Z0.700").unwrap_or(usize::MAX);
        let travel = t.find("G0 X50.000 Y50.000").unwrap_or(0);
        let back = t.rfind("G0 Z0.300").unwrap_or(0);
        assert!(hop < travel && travel < back, "{t}");
    }

    #[test]
    fn scripts_replace_the_built_in_sequences() {
        let s = PrintSettings {
            start_script: "G28 ; mine\nM109 S{first_nozzle}".into(),
            end_script: "M84 ; bye".into(),
            layer_script: ";at layer {layer} z {z}".into(),
            first_layer_nozzle_temperature: 215.0,
            ..PrintSettings::default()
        };
        let layers = one_layer(vec![line((0.0, 0.0), (5.0, 0.0), PathKind::Skirt)]);
        let t = write_gcode(&layers, &s, &Bed::default()).text;
        assert!(t.contains("G28 ; mine\nM109 S215"));
        assert!(t.contains(";at layer 1 z 0.300"));
        assert!(t.trim_end().ends_with("M84 ; bye"));
        assert!(!t.contains("M190"), "the built-in heat-up is replaced");
        assert!(t.contains("M82"), "the writer still states its own modes");
    }

    #[test]
    fn first_layer_temperatures_switch_on_layer_two() {
        let s = PrintSettings { first_layer_nozzle_temperature: 220.0, ..PrintSettings::default() };
        let mut layers = one_layer(vec![line((0.0, 0.0), (5.0, 0.0), PathKind::Skirt)]);
        layers.push(LayerPaths { index: 1, z: 0.5, height: 0.2, paths: vec![] });
        let t = write_gcode(&layers, &s, &Bed::default()).text;
        assert!(t.contains("M109 S220"));
        let l1 = t.find(";LAYER:1").unwrap_or(usize::MAX);
        assert!(t.find("M104 S210").is_some_and(|i| i > l1));
    }

    #[test]
    fn cost_and_weight_follow_the_material() {
        let s = PrintSettings {
            filament_density: 2.0,
            filament_cost: 1000.0,
            ..PrintSettings::default()
        };
        let layers = one_layer(vec![line((0.0, 0.0), (100.0, 0.0), PathKind::Skirt)]);
        let out = write_gcode(&layers, &s, &Bed::default());
        let area = std::f64::consts::PI * 1.75f64.powi(2) / 4.0;
        let grams = out.stats.filament_mm * area / 1000.0 * 2.0;
        assert!((out.stats.filament_grams - grams).abs() < 1e-9);
        assert!((out.stats.filament_cost - grams).abs() < 1e-9);
    }

    #[test]
    fn coordinate_offsets_shift_every_move() {
        let s = PrintSettings {
            gcode_offset_x: 1.0,
            gcode_offset_y: -2.0,
            gcode_offset_z: 0.05,
            ..PrintSettings::default()
        };
        let layers = one_layer(vec![line((10.0, 10.0), (20.0, 10.0), PathKind::Skirt)]);
        let t = write_gcode(&layers, &s, &Bed::default()).text;
        assert!(t.contains("G0 Z0.350"), "{t}");
        assert!(t.contains("G0 X11.000 Y8.000"));
        assert!(t.contains("G1 X21.000 Y8.000"));
    }

    #[test]
    fn durations_format() {
        assert_eq!(format_duration(3723.0), "1h 02m 03s");
        assert_eq!(format_duration(59.4), "0m 59s");
        assert_eq!(format_duration(f64::NAN), "unknown");
    }

    #[test]
    fn heating_comes_before_a_start_script_without_temperatures() {
        let mut s = PrintSettings {
            start_script: "G28 ; home\nG1 Y80 E10 F600 ; prime".into(),
            ..PrintSettings::default()
        };
        let mut w = GcodeWriter::new(&s);
        w.start();
        let text = w.out.clone();
        let heat = text.find("M109").unwrap_or(usize::MAX);
        let prime = text.find("G1 Y80 E10").unwrap_or(0);
        assert!(heat < prime, "the nozzle is hot before the script primes:\n{text}");
        assert!(!text.contains("G21 ; millimetres"), "the script does its own homing");

        s.start_script = "M190 S{first_bed}\nM109 S{first_nozzle}\nG28".into();
        let mut w = GcodeWriter::new(&s);
        w.start();
        assert_eq!(
            w.out.matches("M109").count(),
            1,
            "a script that heats is not doubled:\n{}",
            w.out
        );
        assert!(script_sets_temperatures("m104 s200"));
        assert!(!script_sets_temperatures("G28 ; M104 later"));
    }
}
