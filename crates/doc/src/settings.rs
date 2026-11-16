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
        // every pattern is drawn; kept so callers can still ask
        let _ = self;
        true
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

/// The printable area's outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BedShape {
    Rectangle,
    Circle,
}

impl BedShape {
    pub const ALL: [BedShape; 2] = [BedShape::Rectangle, BedShape::Circle];
    const NAMES: &'static [&'static str] = &["rectangle", "circle"];
}

/// The cross-section the extrusion volume is computed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeadShape {
    /// Width × height: the same volume as the reference, so tuned flow
    /// multipliers carry over.
    Rectangle,
    /// A rectangle with rounded sides, closer to a squashed bead.
    Rounded,
}

impl BeadShape {
    pub const ALL: [BeadShape; 2] = [BeadShape::Rectangle, BeadShape::Rounded];
    const NAMES: &'static [&'static str] = &["rectangle", "rounded sides"];
}

/// What to print where two walls do not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinWalls {
    /// Only full loops.
    Off,
    /// One line along parts of the model too thin for a loop.
    SingleLine,
    /// That, and single lines in gaps between the walls and the infill.
    GapFill,
}

impl ThinWalls {
    pub const ALL: [ThinWalls; 3] = [ThinWalls::Off, ThinWalls::SingleLine, ThinWalls::GapFill];
}

/// Where the nozzle rests after homing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeCorner {
    FrontLeft,
    FrontRight,
    BackLeft,
    BackRight,
    Centre,
}

impl HomeCorner {
    pub const ALL: [HomeCorner; 5] = [
        HomeCorner::FrontLeft,
        HomeCorner::FrontRight,
        HomeCorner::BackLeft,
        HomeCorner::BackRight,
        HomeCorner::Centre,
    ];

    /// The point on a `width` × `depth` bed, in bed coordinates.
    pub fn point(self, width: f64, depth: f64) -> (f64, f64) {
        match self {
            HomeCorner::FrontLeft => (0.0, 0.0),
            HomeCorner::FrontRight => (width, 0.0),
            HomeCorner::BackLeft => (0.0, depth),
            HomeCorner::BackRight => (width, depth),
            HomeCorner::Centre => (width / 2.0, depth / 2.0),
        }
    }
}

/// What to do with outlines a damaged mesh leaves open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenOutlines {
    /// Close those with a small gap; drop the rest.
    CloseGaps,
    /// Keep only outlines the mesh closes itself.
    Drop,
    /// Close small gaps, and print what stays open as single lines.
    Lines,
}

impl OpenOutlines {
    pub const ALL: [OpenOutlines; 3] =
        [OpenOutlines::CloseGaps, OpenOutlines::Drop, OpenOutlines::Lines];
}

/// How nested and overlapping outlines become solid areas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    Merge,
    Alternate,
    Outlines,
}

impl FillRule {
    pub const ALL: [FillRule; 3] = [FillRule::Merge, FillRule::Alternate, FillRule::Outlines];
}

/// The file the G-code is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Plain,
    Binary,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 2] = [OutputFormat::Plain, OutputFormat::Binary];

    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Plain => "gcode",
            OutputFormat::Binary => "bgcode",
        }
    }
}

/// How each layer is marked in the G-code, for post-processing scripts and
/// G-code viewers that look for the marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerMarks {
    /// `;LAYER:<n> Z:<z> HEIGHT:<h>`, counting from 0 (with move comments on).
    SliceCraft,
    /// `; layer <n>, Z = <z>`, counting from 1, and `; layer end` after the
    /// last layer: the marks Simplify3D post-processing scripts look for.
    Simplify3D,
}

impl LayerMarks {
    pub const ALL: [LayerMarks; 2] = [LayerMarks::SliceCraft, LayerMarks::Simplify3D];
}

/// Which paths end with a wipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WipeFeatures {
    OuterWalls,
    AllWalls,
    Everything,
}

impl WipeFeatures {
    pub const ALL: [WipeFeatures; 3] =
        [WipeFeatures::OuterWalls, WipeFeatures::AllWalls, WipeFeatures::Everything];
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
    pub bead_shape: BeadShape,
    /// First-layer line width, % of the normal width.
    pub first_layer_width_percent: f64,
    pub retract_enabled: bool,
    pub retract_length: f64,
    pub retract_speed: f64,
    pub retract_lift: f64,
    /// Travel moves shorter than this do not retract, mm.
    pub retract_min_travel: f64,
    pub retract_on_layer_change: bool,
    /// Extra filament pushed (or held back) after a retraction, mm.
    pub retract_extra_restart: f64,
    /// Stop extruding a little before a path ends that is followed by a retraction.
    pub coast_enabled: bool,
    pub coast_distance: f64,
    /// Move back along a finished path before retracting.
    pub wipe_enabled: bool,
    pub wipe_distance: f64,
    /// Write E as increments (M83).
    pub relative_extrusion: bool,
    /// Retract with G10/G11, leaving length and speed to the firmware.
    pub firmware_retraction: bool,
    /// Slow a layer down so it takes at least this long, s (0 = off).
    pub min_layer_time: f64,
    /// The slowest that slowdown may go, mm/s.
    pub min_layer_speed: f64,
    /// Run the fan at full speed on layers quicker than this, s (0 = off).
    pub fan_boost_below_seconds: f64,
    /// Start the fan at full power for a moment before a lower speed.
    pub fan_kickstart: bool,
    /// Skip a retraction when less than this was extruded since the last, mm.
    pub retract_min_extrusion: f64,
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
    pub thin_walls: ThinWalls,
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
    /// Grow (or shrink) outer contours and holes, mm.
    pub outer_size_offset: f64,
    pub hole_size_offset: f64,
    /// Print sparse infill once per this many layers, that much thicker.
    pub infill_every_layers: u32,
    /// One continuous rising outer wall above the bottom layers.
    pub vase_mode: bool,
    /// Layer heights that follow the surface slope.
    pub adaptive_layers: bool,
    pub adaptive_min_height: f64,
    pub adaptive_max_height: f64,
    pub adaptive_quality: u32,
    /// Walls shorter than this print slower, mm (0 = off).
    pub short_wall_length: f64,
    /// Cap on melt rate, mm³/s (0 = off).
    pub max_volumetric_flow: f64,
    /// Print solid areas over air as bridges: straight across, own flow, speed and fan.
    pub bridging_enabled: bool,
    pub bridge_flow_percent: f64,
    pub bridge_speed_percent: f64,
    pub bridge_fan_percent: f64,
    /// Top surfaces: flow, speed (of solid speed) and fan, %.
    pub top_flow_percent: f64,
    pub top_speed_percent: f64,
    pub top_fan_percent: f64,
    /// A light extra pass over top surfaces.
    pub ironing_enabled: bool,
    pub ironing_spacing: f64,
    pub ironing_flow: f64,
    /// Lay top-surface lines one after another in one direction.
    pub monotonic_solid: bool,
    /// Denser infill layers under top surfaces.
    pub dense_layers: u32,
    pub dense_percentage: f64,
    /// Sparse islands smaller than this are printed solid, mm².
    pub solid_min_area: f64,
    /// Grow solid areas into the sparse infill, mm.
    pub solid_expansion: f64,
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
    /// Comment each feature and layer in the G-code.
    pub verbose_comments: bool,
    /// The extruder (tool) that prints; a model zone may pick another.
    pub tool_number: u32,
    pub prime_tower_enabled: bool,
    /// Side of the square prime tower, mm.
    pub prime_tower_size: f64,
    /// Centre of the prime tower on the bed, mm.
    pub prime_tower_x: f64,
    pub prime_tower_y: f64,
    pub ooze_shield_enabled: bool,
    /// Gap between the part and the ooze shield, mm.
    pub ooze_shield_distance: f64,
    /// How far from vertical the shield may lean in to follow the part, degrees.
    pub ooze_shield_angle: f64,
    /// Temperature a waiting tool drops to, °C (0 leaves it hot).
    pub standby_temperature: f64,
    /// Inserted at every tool change ({old} and {new} name the tools).
    pub tool_change_script: String,
    /// Filament pulled back before a tool change, mm.
    pub tool_change_retract: f64,
    /// Speed of that retraction, mm/s.
    pub tool_change_speed: f64,
    /// Sparse infill line width, % of the line width.
    pub infill_width_percent: f64,
    /// Sparse infill lines shorter than this are left out, mm.
    pub infill_min_length: f64,
    /// Solid layers turn this far from the infill angle, degrees.
    pub solid_angle_offset: f64,
    /// Heated chamber target, °C (0 = none).
    pub chamber_temperature: f64,
    /// The part fan rises to full over this many layers.
    pub fan_ramp_layers: u32,
    /// A solid reinforcement every this many layers (0 = none).
    pub solid_every_layers: u32,
    /// Layers in each reinforcement.
    pub solid_every_count: u32,
    /// Support body printed once per this many layers, that much thicker.
    pub support_every_layers: u32,
    /// Lines of "layer temperature": the nozzle changes as that layer starts.
    pub temperature_schedule: String,
    /// Speed of the last solid layers of the print, %.
    pub top_layer_speed_percent: f64,
    /// Inserted after every retraction.
    pub retract_script: String,
    /// Inserted where a height zone's settings take over.
    pub profile_change_script: String,
    /// Where the nozzle rests after homing (the first travel starts there).
    pub homing_corner: HomeCorner,
    /// Which paths end with a wipe.
    pub wipe_features: WipeFeatures,
    /// Narrowest gap filled with a single line, % of the line width.
    pub single_line_min_width: f64,
    /// Widest gap filled with one line rather than two, % of the line width.
    pub single_line_max_width: f64,
    pub open_outline_mode: OpenOutlines,
    /// Open outlines with a gap up to this are closed, mm.
    pub slice_close_gap: f64,
    pub fill_rule: FillRule,
    /// Travel over a finished top surface always retracts.
    pub retract_over_tops: bool,
    /// A tool left idle this long drops to its standby temperature, s.
    pub idle_after_seconds: f64,
    /// Print each model to the top before starting the next.
    pub sequential_printing: bool,
    /// Plain or binary G-code.
    pub output_format: OutputFormat,
    /// How layers are marked in the G-code.
    pub layer_marks: LayerMarks,
    /// How far the print head reaches around the nozzle, mm.
    pub head_clearance: f64,
    /// Height under the gantry: models printed earlier must be lower, mm.
    pub gantry_height: f64,
    /// Embed a preview image this many pixels square (0 = none).
    pub thumbnail_size: u32,
    /// Replace runs of short segments on a circle with G2/G3 arcs.
    pub arc_fitting: bool,
    /// A program run on the finished file (only from the window).
    pub post_process: String,
    /// The printer's 0,0 is the middle of the bed.
    pub origin_center: bool,
    /// Rectangle or circle.
    pub bed_shape: BedShape,
    pub bed_diameter: f64,
    /// Acceleration for the time estimate, mm/s² (0 = ignore).
    pub max_acceleration: f64,
    // raft
    pub raft_enabled: bool,
    pub raft_base_layers: u32,
    pub raft_top_layers: u32,
    /// How far the raft reaches past the part, mm.
    pub raft_margin: f64,
    /// Air gap between the raft and the part, mm.
    pub raft_gap: f64,
    /// Density of the raft's base layers, %.
    pub raft_infill: f64,
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
            bead_shape: BeadShape::Rectangle,
            first_layer_width_percent: 100.0,
            retract_enabled: true,
            retract_length: 1.0,
            retract_speed: 35.0,
            retract_lift: 0.0,
            retract_min_travel: 2.0,
            retract_on_layer_change: true,
            retract_extra_restart: 0.0,
            coast_enabled: false,
            coast_distance: 0.2,
            wipe_enabled: false,
            wipe_distance: 4.0,
            relative_extrusion: false,
            firmware_retraction: false,
            min_layer_time: 0.0,
            min_layer_speed: 10.0,
            fan_boost_below_seconds: 0.0,
            fan_kickstart: false,
            retract_min_extrusion: 0.0,
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
            thin_walls: ThinWalls::GapFill,
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
            outer_size_offset: 0.0,
            hole_size_offset: 0.0,
            infill_every_layers: 1,
            vase_mode: false,
            adaptive_layers: false,
            adaptive_min_height: 0.08,
            adaptive_max_height: 0.28,
            adaptive_quality: 5,
            short_wall_length: 0.0,
            max_volumetric_flow: 0.0,
            bridging_enabled: true,
            bridge_flow_percent: 100.0,
            bridge_speed_percent: 70.0,
            bridge_fan_percent: 100.0,
            top_flow_percent: 100.0,
            top_speed_percent: 100.0,
            top_fan_percent: 100.0,
            ironing_enabled: false,
            ironing_spacing: 0.1,
            ironing_flow: 10.0,
            monotonic_solid: false,
            dense_layers: 0,
            dense_percentage: 50.0,
            solid_min_area: 0.0,
            solid_expansion: 0.0,
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
            verbose_comments: true,
            tool_number: 0,
            prime_tower_enabled: false,
            prime_tower_size: 12.0,
            prime_tower_x: 20.0,
            prime_tower_y: 20.0,
            ooze_shield_enabled: false,
            ooze_shield_distance: 2.0,
            ooze_shield_angle: 30.0,
            standby_temperature: 175.0,
            tool_change_script: String::new(),
            tool_change_retract: 10.0,
            tool_change_speed: 15.0,
            infill_width_percent: 100.0,
            infill_min_length: 0.0,
            solid_angle_offset: 0.0,
            chamber_temperature: 0.0,
            fan_ramp_layers: 0,
            solid_every_layers: 0,
            solid_every_count: 2,
            support_every_layers: 1,
            temperature_schedule: String::new(),
            top_layer_speed_percent: 100.0,
            retract_script: String::new(),
            profile_change_script: String::new(),
            homing_corner: HomeCorner::FrontLeft,
            wipe_features: WipeFeatures::OuterWalls,
            single_line_min_width: 30.0,
            single_line_max_width: 150.0,
            open_outline_mode: OpenOutlines::CloseGaps,
            slice_close_gap: 1.0,
            fill_rule: FillRule::Merge,
            retract_over_tops: false,
            idle_after_seconds: 120.0,
            sequential_printing: false,
            output_format: OutputFormat::Plain,
            layer_marks: LayerMarks::SliceCraft,
            head_clearance: 35.0,
            gantry_height: 25.0,
            thumbnail_size: 0,
            arc_fitting: false,
            post_process: String::new(),
            origin_center: false,
            bed_shape: BedShape::Rectangle,
            bed_diameter: 200.0,
            max_acceleration: 0.0,
            raft_enabled: false,
            raft_base_layers: 2,
            raft_top_layers: 2,
            raft_margin: 3.0,
            raft_gap: 0.15,
            raft_infill: 85.0,
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

/// Every setting whose value differs between `a` and `b`: (setting, a's value, b's value).
pub fn differences(
    a: &PrintSettings,
    b: &PrintSettings,
) -> Vec<(&'static SettingSpec, SettingValue, SettingValue)> {
    SETTINGS
        .iter()
        .filter_map(|s| {
            let (x, y) = (a.get(s.key)?, b.get(s.key)?);
            (x != y).then_some((s, x, y))
        })
        .collect()
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

    /// How far skirt, brim or raft reach outside the models (mm), for keeping
    /// them on the bed when models are laid out.
    pub fn adhesion_margin(&self) -> f64 {
        let w = self.line_width();
        let mut m: f64 = 0.0;
        if self.skirt_enabled {
            m = m.max(self.skirt_distance + f64::from(self.skirt_loops.min(100)) * w);
        }
        if self.brim_enabled {
            m = m.max(f64::from(self.brim_loops.min(200)) * w);
        }
        if self.raft_enabled {
            m = m.max(self.raft_margin);
        }
        if m.is_finite() { m.clamp(0.0, 50.0) } else { 0.0 }
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
        "bead_shape",
        "Bead shape",
        Extruder,
        "Nozzle",
        SettingKind::Choice(BeadShape::NAMES),
        Dc(0),
        "How extrusion volume is computed. Rectangle matches Simplify3D, so flow multipliers tuned there carry over.",
        bind_e!(bead_shape, BeadShape),
    ),
    row(
        "tool_number",
        "Tool number",
        Extruder,
        "Nozzle",
        i(0, 15, ""),
        Di(0),
        "Which tool this extruder drives on a multi-tool printer.",
        bind_u!(tool_number),
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
        bind_f!(retract_extra_restart),
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
        bind_b!(coast_enabled),
    ),
    row(
        "coast_distance",
        "Coasting distance",
        Extruder,
        "Ooze control",
        f(0.0, 5.0, 0.05, MM),
        Df(0.2),
        "Length of path printed without extrusion.",
        bind_f!(coast_distance),
    ),
    row(
        "wipe_enabled",
        "Wipe",
        Extruder,
        "Ooze control",
        B,
        Db(false),
        "Move along the finished path after retracting.",
        bind_b!(wipe_enabled),
    ),
    row(
        "wipe_distance",
        "Wipe distance",
        Extruder,
        "Ooze control",
        f(0.0, 20.0, 0.1, MM),
        Df(4.0),
        "Length of the wipe move.",
        bind_f!(wipe_distance),
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
        bind_f!(first_layer_width_percent),
    ),
    row(
        "adaptive_layers",
        "Adaptive layer height",
        Layer,
        "Layers",
        B,
        Db(false),
        "Thinner layers where the surface slopes gently.",
        bind_b!(adaptive_layers),
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
        bind_b!(vase_mode),
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
        bind_b!(raft_enabled),
    ),
    row(
        "raft_base_layers",
        "Raft base layers",
        Adhesion,
        "Raft",
        i(1, 10, ""),
        Di(2),
        "Thick, wide layers at the bottom of the raft.",
        bind_u!(raft_base_layers),
    ),
    row(
        "raft_top_layers",
        "Raft top layers",
        Adhesion,
        "Raft",
        i(1, 10, ""),
        Di(2),
        "Dense layers the part sits on.",
        bind_u!(raft_top_layers),
    ),
    row(
        "raft_margin",
        "Raft margin",
        Adhesion,
        "Raft",
        f(0.0, 30.0, 0.5, MM),
        Df(3.0),
        "How far the raft extends past the part.",
        bind_f!(raft_margin),
    ),
    row(
        "raft_gap",
        "Raft separation",
        Adhesion,
        "Raft",
        f(0.0, 2.0, 0.01, MM),
        Df(0.15),
        "Air gap between raft and part.",
        bind_f!(raft_gap),
    ),
    row(
        "raft_infill",
        "Raft infill",
        Adhesion,
        "Raft",
        f(1.0, 100.0, 1.0, PCT),
        Df(85.0),
        "Density of the raft layers.",
        bind_f!(raft_infill),
    ),
    row(
        "prime_tower_enabled",
        "Prime tower",
        Adhesion,
        "Prime tower",
        B,
        Db(false),
        "A tower that primes each tool after a change.",
        bind_b!(prime_tower_enabled),
    ),
    row(
        "prime_tower_size",
        "Prime tower size",
        Adhesion,
        "Prime tower",
        f(2.0, 100.0, 1.0, MM),
        Df(12.0),
        "Side of the tower.",
        bind_f!(prime_tower_size),
    ),
    row(
        "prime_tower_x",
        "Prime tower X",
        Adhesion,
        "Prime tower",
        f(-10_000.0, 10_000.0, 1.0, MM),
        Df(20.0),
        "Where the tower stands.",
        bind_f!(prime_tower_x),
    ),
    row(
        "prime_tower_y",
        "Prime tower Y",
        Adhesion,
        "Prime tower",
        f(-10_000.0, 10_000.0, 1.0, MM),
        Df(20.0),
        "Where the tower stands.",
        bind_f!(prime_tower_y),
    ),
    row(
        "ooze_shield_enabled",
        "Ooze shield",
        Adhesion,
        "Ooze shield",
        B,
        Db(false),
        "A thin wall around the part that catches ooze from idle tools.",
        bind_b!(ooze_shield_enabled),
    ),
    row(
        "ooze_shield_distance",
        "Ooze shield distance",
        Adhesion,
        "Ooze shield",
        f(0.5, 50.0, 0.5, MM),
        Df(2.0),
        "Gap between the part and the shield.",
        bind_f!(ooze_shield_distance),
    ),
    row(
        "ooze_shield_angle",
        "Ooze shield slope",
        Adhesion,
        "Ooze shield",
        f(0.0, 89.0, 1.0, DEG),
        Df(30.0),
        "How steeply the shield may follow the part.",
        bind_f!(ooze_shield_angle),
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
        "Shape of the sparse infill. Gyroid, cubic and octahedral are strong in every direction; honeycomb and wiggle are classic looks.",
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
        bind_f!(infill_width_percent),
    ),
    row(
        "infill_every_layers",
        "Combine infill every",
        Infill,
        "Sparse infill",
        i(1, 10, "layers"),
        Di(1),
        "Print sparse infill once per this many layers, thicker.",
        bind_u!(infill_every_layers),
    ),
    row(
        "infill_min_length",
        "Shortest infill line",
        Infill,
        "Sparse infill",
        f(0.0, 20.0, 0.1, MM),
        Df(0.0),
        "Skip infill lines shorter than this.",
        bind_f!(infill_min_length),
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
        bind_f!(solid_angle_offset),
    ),
    row(
        "dense_layers",
        "Dense layers under tops",
        Infill,
        "Dense infill",
        i(0, 20, ""),
        Di(0),
        "Layers of denser infill below the top skin.",
        bind_u!(dense_layers),
    ),
    row(
        "dense_percentage",
        "Dense infill density",
        Infill,
        "Dense infill",
        f(0.0, 100.0, 1.0, PCT),
        Df(50.0),
        "Density of those layers.",
        bind_f!(dense_percentage),
    ),
    row(
        "ironing_enabled",
        "Ironing",
        Infill,
        "Ironing",
        B,
        Db(false),
        "Pass the hot nozzle over top surfaces to smooth them.",
        bind_b!(ironing_enabled),
    ),
    row(
        "ironing_spacing",
        "Ironing spacing",
        Infill,
        "Ironing",
        f(0.01, 1.0, 0.01, MM),
        Df(0.1),
        "Distance between ironing passes.",
        bind_f!(ironing_spacing),
    ),
    row(
        "ironing_flow",
        "Ironing flow",
        Infill,
        "Ironing",
        f(0.0, 50.0, 1.0, PCT),
        Df(10.0),
        "Material pushed while ironing.",
        bind_f!(ironing_flow),
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
        bind_f!(chamber_temperature),
    ),
    row(
        "standby_temperature",
        "Idle tool temperature",
        Temperature,
        "Multi-tool",
        f(0.0, 400.0, 1.0, CEL),
        Df(175.0),
        "Temperature a waiting tool drops to.",
        bind_f!(standby_temperature),
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
        bind_u!(fan_ramp_layers),
    ),
    row(
        "bridge_fan_percent",
        "Bridge fan",
        Cooling,
        "Part fan",
        f(0.0, 100.0, 1.0, PCT),
        Df(100.0),
        "Fan duty while bridging.",
        bind_f!(bridge_fan_percent),
    ),
    row(
        "min_layer_time",
        "Minimum layer time",
        Cooling,
        "Slow small layers",
        f(0.0, 120.0, 1.0, "s"),
        Df(0.0),
        "Slow down so each layer takes at least this long.",
        bind_f!(min_layer_time),
    ),
    row(
        "min_layer_speed",
        "Slowest speed",
        Cooling,
        "Slow small layers",
        f(1.0, 100.0, 1.0, MMS),
        Df(10.0),
        "Never slow below this.",
        bind_f!(min_layer_speed),
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
        bind_f!(bridge_speed_percent),
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
        bind_b!(relative_extrusion),
    ),
    row(
        "firmware_retraction",
        "Firmware retraction",
        Gcode,
        "Output",
        B,
        Db(false),
        "Let the firmware retract (G10/G11).",
        bind_b!(firmware_retraction),
    ),
    row(
        "arc_fitting",
        "Arc moves",
        Gcode,
        "Output",
        B,
        Db(false),
        "Replace runs of short segments with G2/G3 arcs.",
        bind_b!(arc_fitting),
    ),
    row(
        "verbose_comments",
        "Explain moves in comments",
        Gcode,
        "Output",
        B,
        Db(true),
        "Comment each feature and layer.",
        bind_b!(verbose_comments),
    ),
    row(
        "thumbnail_size",
        "Thumbnail size",
        Gcode,
        "Output",
        i(0, 512, "px"),
        Di(0),
        "Embed a preview image of this size (0 = none).",
        bind_u!(thumbnail_size),
    ),
    row(
        "post_process",
        "Post-processing command",
        Gcode,
        "Output",
        SettingKind::Text,
        Dt(""),
        "One step per line. Text edits: {REPLACE \"find\" \"with\"}, {DELETE \"text\"}, {STRIP \"line start\"}, {PREPEND \"text\"}, {APPEND \"text\"}, {TOOL1REPLACE \"find\" \"with\"}; \n is a line break. Any other line is a program run on the written file ([output_filepath] stands for it); programs run only from the window, never for agents or the command line.",
        bind_t!(post_process),
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
        bind_t!(tool_change_script),
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
        SettingKind::Choice(BedShape::NAMES),
        Dc(0),
        "Shape of the printable area.",
        bind_e!(bed_shape, BedShape),
    ),
    row(
        "origin_center",
        "Origin at bed centre",
        Machine,
        "Build volume",
        B,
        Db(false),
        "The printer's 0,0 is the middle of the bed (deltas).",
        bind_b!(origin_center),
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
        bind_f!(max_acceleration),
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
        Dc(2),
        "Single line: one line down parts too thin for a loop. Gap fill: also the gaps the walls leave.",
        bind_e!(thin_walls, ThinWalls),
    ),
    row(
        "bridging_enabled",
        "Detect bridges",
        Advanced,
        "Bridging",
        B,
        Db(true),
        "Print unsupported spans as straight bridges.",
        bind_b!(bridging_enabled),
    ),
    row(
        "bridge_flow_percent",
        "Bridge flow",
        Advanced,
        "Bridging",
        f(10.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Flow while bridging.",
        bind_f!(bridge_flow_percent),
    ),
    row(
        "slice_close_gap",
        "Close outline gaps up to",
        Advanced,
        "Slicing",
        f(0.0, 5.0, 0.05, MM),
        Df(1.0),
        "Repair open outlines with gaps this small.",
        bind_f!(slice_close_gap),
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
        bind_f!(adaptive_min_height),
    ),
    row(
        "adaptive_max_height",
        "Thickest adaptive layer",
        Layer,
        "Adaptive layers",
        f(0.01, 2.0, 0.01, MM),
        Df(0.28),
        "Upper bound for adaptive layer heights.",
        bind_f!(adaptive_max_height),
    ),
    row(
        "adaptive_quality",
        "Adaptive detail",
        Layer,
        "Adaptive layers",
        i(1, 10, ""),
        Di(5),
        "Higher follows curved surfaces more closely.",
        bind_u!(adaptive_quality),
    ),
    row(
        "outer_size_offset",
        "Outer size correction",
        Layer,
        "Dimensional accuracy",
        f(-2.0, 2.0, 0.01, MM),
        Df(0.0),
        "Grow (or shrink) outer walls to correct the printed size.",
        bind_f!(outer_size_offset),
    ),
    row(
        "hole_size_offset",
        "Hole size correction",
        Layer,
        "Dimensional accuracy",
        f(-2.0, 2.0, 0.01, MM),
        Df(0.0),
        "Grow (or shrink) holes separately from outer walls.",
        bind_f!(hole_size_offset),
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
        bind_f!(solid_min_area),
    ),
    row(
        "solid_expansion",
        "Solid area expansion",
        Infill,
        "Solid infill",
        f(0.0, 10.0, 0.1, MM),
        Df(0.0),
        "Grow solid areas into the sparse infill for anchoring.",
        bind_f!(solid_expansion),
    ),
    row(
        "monotonic_solid",
        "Ordered solid lines",
        Infill,
        "Solid infill",
        B,
        Db(false),
        "Lay solid lines strictly side by side for an even top surface.",
        bind_b!(monotonic_solid),
    ),
    row(
        "top_flow_percent",
        "Top surface flow",
        Infill,
        "Top surfaces",
        f(10.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Flow on the exposed top layer.",
        bind_f!(top_flow_percent),
    ),
    row(
        "top_speed_percent",
        "Top surface speed",
        Infill,
        "Top surfaces",
        f(5.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Speed on the exposed top layer.",
        bind_f!(top_speed_percent),
    ),
    row(
        "top_fan_percent",
        "Top surface fan",
        Infill,
        "Top surfaces",
        f(0.0, 100.0, 1.0, PCT),
        Df(100.0),
        "Fan on the exposed top layer.",
        bind_f!(top_fan_percent),
    ),
    row(
        "solid_every_layers",
        "Solid layer every",
        Infill,
        "Reinforcement",
        i(0, 1000, "layers"),
        Di(0),
        "Insert solid layers at this interval for stiffness (0 = off).",
        bind_u!(solid_every_layers),
    ),
    row(
        "solid_every_count",
        "Reinforcement thickness",
        Infill,
        "Reinforcement",
        i(1, 20, "layers"),
        Di(2),
        "How many solid layers each reinforcement has.",
        bind_u!(solid_every_count),
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
        bind_u!(support_every_layers),
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
        bind_t!(temperature_schedule),
    ),
    row(
        "idle_after_seconds",
        "Cool an idle tool after",
        Temperature,
        "Multi-tool",
        f(1.0, 3600.0, 1.0, "s"),
        Df(120.0),
        "How long a tool waits before cooling down.",
        bind_f!(idle_after_seconds),
    ),
    row(
        "fan_boost_below_seconds",
        "Boost fan below layer time",
        Cooling,
        "Slow small layers",
        f(0.0, 300.0, 1.0, "s"),
        Df(0.0),
        "Run the fan harder on layers faster than this (0 = off).",
        bind_f!(fan_boost_below_seconds),
    ),
    row(
        "fan_kickstart",
        "Kick-start the fan",
        Cooling,
        "Part fan",
        B,
        Db(false),
        "Spin the fan at full power briefly before a low speed.",
        bind_b!(fan_kickstart),
    ),
    row(
        "top_layer_speed_percent",
        "Top layer speed",
        Speeds,
        "Printing",
        f(5.0, 200.0, 1.0, PCT),
        Df(100.0),
        "Speed of the last solid layers, relative to the default speed.",
        bind_f!(top_layer_speed_percent),
    ),
    row(
        "short_wall_length",
        "Slow walls shorter than",
        Speeds,
        "Printing",
        f(0.0, 500.0, 1.0, MM),
        Df(0.0),
        "Short perimeters print slower so they can cool (0 = off).",
        bind_f!(short_wall_length),
    ),
    row(
        "max_volumetric_flow",
        "Flow limit",
        Speeds,
        "Printing",
        f(0.0, 100.0, 0.5, "mm³/s"),
        Df(0.0),
        "Slow down where the hot end cannot melt fast enough (0 = off).",
        bind_f!(max_volumetric_flow),
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
        SettingKind::Choice(&["plain G-code", "binary G-code (.bgcode)"]),
        Dc(0),
        "Which file is written. Binary G-code is smaller and carries the preview and print facts in blocks.",
        bind_e!(output_format, OutputFormat),
    ),
    row(
        "layer_marks",
        "Layer marks",
        Gcode,
        "Output",
        SettingKind::Choice(&["SliceCraft", "Simplify3D-compatible"]),
        Dc(0),
        "How each layer is marked in the G-code. Simplify3D-compatible writes \"; layer 1, Z = 0.2800\" (counting from 1) and \"; layer end\", so post-processing scripts written for Simplify3D find their layers. Profiles imported from Simplify3D use it.",
        bind_e!(layer_marks, LayerMarks),
    ),
    row(
        "retract_script",
        "Retraction script",
        Scripts,
        "Scripts",
        SettingKind::Text,
        Dt(""),
        "Inserted at every retraction.",
        bind_t!(retract_script),
    ),
    row(
        "profile_change_script",
        "Profile change script",
        Scripts,
        "Scripts",
        SettingKind::Text,
        Dt(""),
        "Inserted where one profile hands over to the next.",
        bind_t!(profile_change_script),
    ),
    row(
        "homing_corner",
        "Home position",
        Machine,
        "Build volume",
        SettingKind::Choice(&["front left", "front right", "back left", "back right", "centre"]),
        Dc(0),
        "Where the nozzle rests after homing.",
        bind_e!(homing_corner, HomeCorner),
    ),
    row(
        "bed_diameter",
        "Bed diameter",
        Machine,
        "Build volume",
        f(10.0, 2000.0, 1.0, MM),
        Df(200.0),
        "Diameter of a round bed.",
        bind_f!(bed_diameter),
    ),
    row(
        "tool_change_retract",
        "Tool change retraction",
        Extruder,
        "Tool change",
        f(0.0, 100.0, 0.5, MM),
        Df(10.0),
        "Filament pulled back before a tool change.",
        bind_f!(tool_change_retract),
    ),
    row(
        "tool_change_speed",
        "Tool change speed",
        Extruder,
        "Tool change",
        f(1.0, 200.0, 1.0, MMS),
        Df(15.0),
        "Speed of the tool change retraction.",
        bind_f!(tool_change_speed),
    ),
    row(
        "wipe_features",
        "Wipe after",
        Advanced,
        "Travel",
        SettingKind::Choice(&["outer walls", "all walls", "every feature"]),
        Dc(0),
        "Which paths end with a wipe.",
        bind_e!(wipe_features, WipeFeatures),
    ),
    row(
        "retract_over_tops",
        "Always retract over top surfaces",
        Advanced,
        "Travel",
        B,
        Db(false),
        "Avoid scars on finished top surfaces.",
        bind_b!(retract_over_tops),
    ),
    row(
        "retract_min_extrusion",
        "Retract only after extruding",
        Advanced,
        "Travel",
        f(0.0, 50.0, 0.1, MM),
        Df(0.0),
        "Skip a retraction when little was printed since the last one.",
        bind_f!(retract_min_extrusion),
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
        Df(30.0),
        "Narrowest single-line wall, relative to the line width.",
        bind_f!(single_line_min_width),
    ),
    row(
        "single_line_max_width",
        "Single line widest",
        Advanced,
        "Walls",
        f(100.0, 300.0, 1.0, PCT),
        Df(150.0),
        "Widest single-line wall, relative to the line width.",
        bind_f!(single_line_max_width),
    ),
    row(
        "open_outline_mode",
        "Open outlines",
        Advanced,
        "Slicing",
        SettingKind::Choice(&["close small gaps", "drop them", "print as lines"]),
        Dc(0),
        "What to do with outlines a damaged mesh leaves open.",
        bind_e!(open_outline_mode, OpenOutlines),
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
        bind_e!(fill_rule, FillRule),
    ),
    row(
        "sequential_printing",
        "One model at a time",
        Advanced,
        "Sequential printing",
        B,
        Db(false),
        "Print each model to the top before starting the next. Models must stand apart by the head clearance, and all but the last must fit under the gantry.",
        bind_b!(sequential_printing),
    ),
    row(
        "head_clearance",
        "Head clearance",
        Advanced,
        "Sequential printing",
        f(1.0, 300.0, 1.0, MM),
        Df(35.0),
        "How far the print head reaches around the nozzle; models stand at least this far apart.",
        bind_f!(head_clearance),
    ),
    row(
        "gantry_height",
        "Gantry height",
        Advanced,
        "Sequential printing",
        f(1.0, 2000.0, 1.0, MM),
        Df(25.0),
        "Clearance under the gantry; every model but the last must be lower.",
        bind_f!(gantry_height),
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
        // any boolean setting the slicer does not use yet
        let Some(spec) =
            SETTINGS.iter().find(|s| !s.implemented() && matches!(s.kind, SettingKind::Bool))
        else {
            return;
        };
        let mut p = PrintSettings::default();
        assert!(p.unused_changes().is_empty());
        let on = SettingValue::Bool(spec.default.value() != SettingValue::Bool(true));
        assert!(p.set(spec.key, on.clone()).is_ok());
        assert_eq!(p.get(spec.key), Some(on));
        assert_eq!(p.unused_changes(), vec![spec.label]);
        // setting it back to the default forgets it
        assert!(p.set(spec.key, spec.default.value()).is_ok());
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
