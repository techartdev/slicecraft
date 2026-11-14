#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! The agent surface: one tool per engine command, described once.
//!
//! [`server`] speaks MCP (JSON-RPC 2.0 over standard input and output); the
//! tool table below is the contract, so a tool cannot be added to the agent
//! surface without naming the engine command it drives. Programmatic
//! calls never open dialogs: every path is an argument.

pub mod json;
pub mod server;

pub use slicecraft_engine::Session;
use slicecraft_engine::{Command, Outcome};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

/// One callable tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    /// The engine command this tool runs, so the surfaces cannot drift.
    pub command: &'static str,
    pub description: &'static str,
    /// Arguments as `name:type`, with a trailing `!` when required; the
    /// type is string (when left out), integer, number or boolean.
    pub args: &'static [&'static str],
}

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "slicecraft.import",
        command: "import",
        description: "Import an STL, OBJ or 3MF model from a file into the document",
        args: &["path!"],
    },
    ToolSpec {
        name: "slicecraft.open_project",
        command: "open-project",
        description: "Replace the document with a SliceCraft project file",
        args: &["path!"],
    },
    ToolSpec {
        name: "slicecraft.save_project",
        command: "save-project",
        description: "Save models, placement, zones and profile as a project file",
        args: &["path!"],
    },
    ToolSpec {
        name: "slicecraft.new_project",
        command: "new-project",
        description: "Start an empty project, keeping the current profile",
        args: &[],
    },
    ToolSpec {
        name: "slicecraft.describe",
        command: "analyze-mesh",
        description: "Describe the document: each model's index, name, role, group, size and mesh state, the profile, and whether a slice is ready",
        args: &[],
    },
    ToolSpec {
        name: "slicecraft.arrange",
        command: "arrange",
        description: "Lay all models out on the bed without overlap",
        args: &[],
    },
    ToolSpec {
        name: "slicecraft.remove",
        command: "remove",
        description: "Remove a model",
        args: &["index:integer!"],
    },
    ToolSpec {
        name: "slicecraft.duplicate",
        command: "duplicate",
        description: "Copy a model; the copy is placed beside the others",
        args: &["index:integer!"],
    },
    ToolSpec {
        name: "slicecraft.place",
        command: "place",
        description: "Move, turn or scale a model; values left out are kept. x and y are the model centre on the bed in mm, rotate_x/rotate_y/rotate_z in degrees, scale a uniform factor",
        args: &[
            "index:integer!",
            "x:number",
            "y:number",
            "rotate_x:number",
            "rotate_y:number",
            "rotate_z:number",
            "scale:number",
        ],
    },
    ToolSpec {
        name: "slicecraft.lay_flat",
        command: "lay-flat",
        description: "Turn a model so its largest flat face lies on the bed",
        args: &["index:integer!"],
    },
    ToolSpec {
        name: "slicecraft.repair",
        command: "repair-mesh",
        description: "Repair a model's mesh: weld, drop degenerate faces, fix normals, and close holes when fill_holes is true",
        args: &["index:integer!", "fill_holes:boolean"],
    },
    ToolSpec {
        name: "slicecraft.visibility",
        command: "visibility",
        description: "Include a model when slicing (visible = true) or leave it out",
        args: &["index:integer!", "visible:boolean!"],
    },
    ToolSpec {
        name: "slicecraft.model_role",
        command: "model-role",
        description: "Use a model as a part, as support, or as a cutter removed from the parts (role = print, support or cutter)",
        args: &["index:integer!", "role:string!"],
    },
    ToolSpec {
        name: "slicecraft.group",
        command: "group-models",
        description: "Group models so they move together (indices such as 0,1,2); ungroup = true takes them out of groups",
        args: &["indices:string!", "ungroup:boolean"],
    },
    ToolSpec {
        name: "slicecraft.undo",
        command: "undo",
        description: "Undo the last change",
        args: &[],
    },
    ToolSpec {
        name: "slicecraft.redo",
        command: "redo",
        description: "Redo the last undone change",
        args: &[],
    },
    ToolSpec {
        name: "slicecraft.load_profile",
        command: "load-profile",
        description: "Load a SliceCraft .scprofile or a Simplify3D .fff profile",
        args: &["path!"],
    },
    ToolSpec {
        name: "slicecraft.save_profile",
        command: "save-profile",
        description: "Save the current profile to a file under a name",
        args: &["path!", "name:string"],
    },
    ToolSpec {
        name: "slicecraft.list_settings",
        command: "profile",
        description: "List profile settings with their keys, values and whether the slicer uses them; search filters by key or label",
        args: &["search:string"],
    },
    ToolSpec {
        name: "slicecraft.compare_profile",
        command: "compare-profile",
        description: "List how a profile file (SliceCraft or Simplify3D) differs from the current profile, without loading it",
        args: &["path!"],
    },
    ToolSpec {
        name: "slicecraft.get_setting",
        command: "profile",
        description: "Read one profile setting by key",
        args: &["key:string!"],
    },
    ToolSpec {
        name: "slicecraft.set_setting",
        command: "profile",
        description: "Change one profile setting by key (see list_settings); the value is checked against the setting's kind and range",
        args: &["key:string!", "value:string!"],
    },
    ToolSpec {
        name: "slicecraft.setup_printer",
        command: "profile-wizard",
        description: "Make and load a profile from printer facts (printer: a known model such as \"ender-3 s1\" to start from), flavor (marlin, reprap, klipper), bed (WxDxH, or diameter x height with round = true), nozzle, filament, drive (direct, bowden), material (pla, petg, abs, asa, tpu, nylon), heated_bed",
        args: &[
            "printer:string",
            "name:string",
            "flavor:string",
            "bed:string",
            "round:boolean",
            "nozzle:number",
            "filament:number",
            "drive:string",
            "material:string",
            "heated_bed:boolean",
        ],
    },
    ToolSpec {
        name: "slicecraft.add_zone",
        command: "setting-zones",
        description: "Change settings over a height range: settings such as \"layer_height=0.1; infill_percentage=40\" from z_from to z_to mm (models: optional indices such as 0,2)",
        args: &["settings:string!", "name:string", "z_from:number", "z_to:number", "models:string"],
    },
    ToolSpec {
        name: "slicecraft.zone_from_profile",
        command: "zone-from-profile",
        description: "Print part of the job with another profile file: from z_from to z_to mm, and only the listed models (optional indices such as 0,2); the file's job settings that differ from the current profile become the zone",
        args: &["profile:string!", "name:string", "z_from:number", "z_to:number", "models:string"],
    },
    ToolSpec {
        name: "slicecraft.preview",
        command: "preview",
        description: "Describe the layer preview of the current document",
        args: &[],
    },
    ToolSpec {
        name: "slicecraft.supports",
        command: "supports",
        description: "Turn automatic support on (enabled = true) or off",
        args: &["enabled:boolean!"],
    },
    ToolSpec {
        name: "slicecraft.support_preview",
        command: "support-preview",
        description: "Report the support a slice would print (layers, volume) without slicing",
        args: &[],
    },
    ToolSpec {
        name: "slicecraft.slice",
        command: "slice",
        description: "Slice the document and report layers, filament and time",
        args: &[],
    },
    ToolSpec {
        name: "slicecraft.export_gcode",
        command: "export-gcode",
        description: "Write G-code for the current document to a file",
        args: &["path!"],
    },
    ToolSpec {
        name: "slicecraft.export_models",
        command: "export-models",
        description: "Write the visible models as placed to an STL, OBJ or 3MF file",
        args: &["path!"],
    },
    ToolSpec {
        name: "slicecraft.open_gcode",
        command: "open-gcode",
        description: "Read a G-code file for preview and statistics (layers, filament, time)",
        args: &["path!"],
    },
    ToolSpec {
        name: "slicecraft.network_check",
        command: "print-network",
        description: "Check a network print host: kind = octoprint or moonraker, url, optional api_key",
        args: &["url:string!", "kind:string", "api_key:string"],
    },
    ToolSpec {
        name: "slicecraft.network_upload",
        command: "print-network",
        description: "Upload the slice (slicing first if needed) to a network print host; start = true prints it at once",
        args: &["url:string!", "kind:string", "api_key:string", "name:string", "start:boolean"],
    },
];

/// A request as it arrives, before transport decoding.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Call {
    pub tool: String,
    pub args: BTreeMap<String, String>,
}

impl Call {
    pub fn new(tool: &str) -> Self {
        Self { tool: tool.to_string(), args: BTreeMap::new() }
    }

    pub fn arg(mut self, key: &str, value: &str) -> Self {
        self.args.insert(key.to_string(), value.to_string());
        self
    }
}

/// What a tool returns to the agent.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Text(String),
    Error(String),
}

impl Reply {
    pub fn is_error(&self) -> bool {
        matches!(self, Reply::Error(_))
    }
}

fn run(session: &mut Session, command: Command) -> Reply {
    match session.execute(command) {
        Ok(Outcome::Imported { index, triangles, skipped_facets, format }) => Reply::Text(format!(
            "imported model {index}: {triangles} triangles ({}), {skipped_facets} skipped",
            format.name()
        )),
        Ok(Outcome::Changed) => Reply::Text("ok".into()),
        Ok(Outcome::Network { message }) => Reply::Text(message),
        Ok(Outcome::ProfileCompared { name, diffs }) => {
            let mut t = format!("{} setting(s) differ from {name}:\n", diffs.len());
            for d in diffs.iter().take(400) {
                let kind = slicecraft_doc::settings::spec(d.key).map(|s| s.kind);
                let f = |v: &slicecraft_doc::SettingValue| {
                    kind.map_or_else(
                        || format!("{v:?}"),
                        |k| slicecraft_doc::profile::format_value(k, v),
                    )
                };
                t.push_str(&format!(
                    "{}: current {} · theirs {}\n",
                    d.key,
                    f(&d.ours),
                    f(&d.theirs)
                ));
            }
            Reply::Text(t)
        }
        Ok(Outcome::Unchanged) => Reply::Text("nothing to change".into()),
        Ok(Outcome::ProfileLoaded { name, warnings }) => {
            Reply::Text(format!("loaded profile {name} ({} warnings)", warnings.len()))
        }
        Ok(Outcome::ProfileImported { name, report }) => Reply::Text(format!(
            "imported profile {name}: {} settings applied, {} kept as planned, {} not used; notes: {}",
            report.applied.len(),
            report.placeholders.len(),
            report.ignored.len(),
            report.notes.join("; ")
        )),
        Ok(Outcome::PrinterConnected { port }) => Reply::Text(format!("connected to {port}")),
        Ok(Outcome::PrintStarted { lines }) => Reply::Text(format!("printing {lines} lines")),
        Ok(Outcome::GcodeOpened { path, layers }) => {
            Reply::Text(format!("opened {} ({layers} layers)", path.display()))
        }
        Ok(Outcome::SupportPreviewed { layers, volume }) => {
            Reply::Text(format!("support on {layers} layers, {volume:.0} mm³"))
        }
        Ok(Outcome::Intersections { index, pairs }) => Reply::Text(match pairs {
            Some(n) => format!("model {index}: {n} self-intersecting triangle pairs"),
            None => format!("model {index} is too large to check"),
        }),
        Ok(Outcome::Repaired { index, report }) => Reply::Text(format!(
            "model {index}: {} degenerate removed, {} flipped, {} holes filled",
            report.degenerate_removed, report.flipped, report.holes_filled
        )),
        Ok(Outcome::Saved { path }) => Reply::Text(format!("saved {}", path.display())),
        Ok(Outcome::ProjectOpened { path, objects, warnings }) => Reply::Text(format!(
            "opened {} with {objects} model(s) ({} warnings)",
            path.display(),
            warnings.len()
        )),
        Ok(Outcome::Sliced { layers, stats }) => Reply::Text(format!(
            "{layers} layers, {:.0} mm filament, about {:.0} s",
            stats.gcode.filament_mm, stats.gcode.estimated_seconds
        )),
        Ok(Outcome::Exported { path, bytes }) => {
            Reply::Text(format!("wrote {bytes} bytes to {}", path.display()))
        }
        Err(e) => Reply::Error(e.to_string()),
    }
}

fn index_arg(call: &Call, key: &str) -> Result<usize, String> {
    call.args
        .get(key)
        .and_then(|i| i.trim().parse::<usize>().ok())
        .ok_or_else(|| format!("{} needs `{key}`, a model number from describe", short(&call.tool)))
}

fn number_arg(call: &Call, key: &str) -> Result<Option<f64>, String> {
    match call.args.get(key) {
        None => Ok(None),
        Some(v) => match v.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(format!("`{key}` is not a number")),
        },
    }
}

fn flag_arg(call: &Call, key: &str) -> Option<bool> {
    call.args.get(key).map(|v| v.trim() == "true")
}

fn short(tool: &str) -> &str {
    tool.strip_prefix("slicecraft.").unwrap_or(tool)
}

fn indices_arg(s: &str) -> Option<Vec<usize>> {
    s.split(',').map(|i| i.trim().parse::<usize>().ok()).collect()
}

/// Dispatch a call against a session.
pub fn dispatch(session: &mut Session, call: &Call) -> Reply {
    let Some(spec) = TOOLS.iter().find(|t| t.name == call.tool) else {
        return Reply::Error(format!("unknown tool: {}", call.tool));
    };
    if session.command(spec.command).is_none() {
        return Reply::Error(format!("tool {} names no engine command", spec.name));
    }
    for a in spec.args {
        if let Some(name) = a.strip_suffix('!') {
            let name = name.split(':').next().unwrap_or(name);
            if !call.args.contains_key(name) {
                return Reply::Error(format!("{} needs `{name}`", short(spec.name)));
            }
        }
    }
    match call_tool(session, spec, call) {
        Ok(r) => r,
        Err(e) => Reply::Error(e),
    }
}

fn call_tool(session: &mut Session, spec: &ToolSpec, call: &Call) -> Result<Reply, String> {
    let path =
        || call.args.get("path").map(PathBuf::from).ok_or("this tool needs `path`".to_string());
    Ok(match short(spec.name) {
        "import" => run(session, Command::ImportFile(path()?)),
        "open_project" => run(session, Command::OpenProject(path()?)),
        "save_project" => run(session, Command::SaveProject(path()?)),
        "new_project" => run(session, Command::NewProject),
        "describe" => Reply::Text(describe(session)),
        "arrange" => run(session, Command::Arrange),
        "remove" => run(session, Command::Remove { index: index_arg(call, "index")? }),
        "duplicate" => run(session, Command::Duplicate { index: index_arg(call, "index")? }),
        "place" => {
            let index = index_arg(call, "index")?;
            let Some(mut t) = session.document().objects.get(index).map(|o| o.transform) else {
                return Err(format!("there is no model {index}"));
            };
            if let Some(x) = number_arg(call, "x")? {
                t.translate.x = x;
            }
            if let Some(y) = number_arg(call, "y")? {
                t.translate.y = y;
            }
            if let Some(r) = number_arg(call, "rotate_x")? {
                t.rotate.x = r;
            }
            if let Some(r) = number_arg(call, "rotate_y")? {
                t.rotate.y = r;
            }
            if let Some(r) = number_arg(call, "rotate_z")? {
                t.rotate.z = r;
            }
            if let Some(k) = number_arg(call, "scale")? {
                t.scale = slicecraft_geom::Vec3::new(k, k, k);
            }
            run(session, Command::Place { index, transform: t })
        }
        "lay_flat" => run(session, Command::LayFlat { index: index_arg(call, "index")? }),
        "repair" => run(
            session,
            Command::RepairMesh {
                index: index_arg(call, "index")?,
                fill: flag_arg(call, "fill_holes").unwrap_or(false),
            },
        ),
        "visibility" => run(
            session,
            Command::SetVisible {
                index: index_arg(call, "index")?,
                visible: flag_arg(call, "visible").unwrap_or(true),
            },
        ),
        "model_role" => {
            let index = index_arg(call, "index")?;
            let role = call
                .args
                .get("role")
                .and_then(|r| slicecraft_doc::ModelRole::from_name(r.trim()))
                .ok_or("model_role needs `role` = print, support or cutter")?;
            run(session, Command::SetRole { index, role })
        }
        "group" => {
            let indices = call
                .args
                .get("indices")
                .and_then(|s| indices_arg(s))
                .ok_or("group needs `indices`, such as 0,1,2")?;
            if flag_arg(call, "ungroup") == Some(true) {
                run(session, Command::UngroupModels { indices })
            } else {
                run(session, Command::GroupModels { indices })
            }
        }
        "undo" => run(session, Command::Undo),
        "redo" => run(session, Command::Redo),
        "load_profile" => run(session, Command::LoadProfile(path()?)),
        "compare_profile" => run(session, Command::CompareProfile(path()?)),
        "save_profile" => {
            let name = call
                .args
                .get("name")
                .cloned()
                .unwrap_or_else(|| session.profile_name().to_string());
            run(session, Command::SaveProfile { path: path()?, name })
        }
        "list_settings" => {
            Reply::Text(list_settings(session, call.args.get("search").map(String::as_str)))
        }
        "get_setting" => {
            let key = call.args.get("key").map(|k| k.trim()).unwrap_or("");
            let spec =
                slicecraft_doc::settings::spec(key).ok_or(format!("unknown setting {key}"))?;
            let v = session.document().settings.get(key).ok_or(format!("unknown setting {key}"))?;
            Reply::Text(format!(
                "{key} = {} ({}{})",
                slicecraft_doc::profile::format_value(spec.kind, &v),
                spec.label,
                if spec.implemented() { "" } else { "; stored, not used by the slicer yet" }
            ))
        }
        "set_setting" => {
            let key = call.args.get("key").map(|k| k.trim()).unwrap_or("");
            let raw = call.args.get("value").map(|v| v.trim()).unwrap_or("");
            let spec =
                slicecraft_doc::settings::spec(key).ok_or(format!("unknown setting {key}"))?;
            let value = slicecraft_doc::profile::parse_value(spec.kind, raw)
                .ok_or(format!("{key}: {raw} is not a value of this setting's kind"))?;
            let mut settings = session.document().settings.clone();
            settings.set(key, value).map_err(|e| e.to_string())?;
            run(session, Command::Profile(Box::new(settings)))
        }
        "setup_printer" => {
            run(session, Command::SetupPrinter(Box::new(setup_from_args(&call.args)?)))
        }
        "add_zone" => {
            let mut zone = slicecraft_doc::SettingZone::new(
                call.args.get("name").cloned().unwrap_or_else(|| "zone".into()),
            );
            if let Some(z) = number_arg(call, "z_from")? {
                zone.z_from = z;
            }
            if let Some(z) = number_arg(call, "z_to")? {
                zone.z_to = z;
            }
            if let Some(m) = call.args.get("models") {
                zone.objects = indices_arg(m).ok_or("`models` is a list such as 0,2")?;
            }
            let text = call.args.get("settings").map(String::as_str).unwrap_or("");
            for pair in text.split([';', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
                let (k, v) = pair.split_once('=').ok_or(format!("`{pair}` is not key=value"))?;
                let (k, v) = (k.trim(), v.trim());
                let spec =
                    slicecraft_doc::settings::spec(k).ok_or(format!("unknown setting {k}"))?;
                let value = slicecraft_doc::profile::parse_value(spec.kind, v)
                    .ok_or(format!("{k}: {v} is not a value of this setting's kind"))?;
                zone.overrides.insert(k.to_string(), value);
            }
            run(session, Command::AddZone(Box::new(zone)))
        }
        "zone_from_profile" => {
            let mut zone = slicecraft_doc::SettingZone::new(
                call.args.get("name").cloned().unwrap_or_default(),
            );
            if let Some(z) = number_arg(call, "z_from")? {
                zone.z_from = z;
            }
            if let Some(z) = number_arg(call, "z_to")? {
                zone.z_to = z;
            }
            if let Some(m) = call.args.get("models") {
                zone.objects = indices_arg(m).ok_or("`models` is a list such as 0,2")?;
            }
            let path = call.args.get("profile").ok_or("`profile` is the path of a profile file")?;
            run(session, Command::ZoneFromProfile { path: path.into(), zone: Box::new(zone) })
        }
        "preview" => {
            let layers = session.preview_layers();
            let z = layers.last().map(|l| l.z);
            Reply::Text(format!("{} layers, top z {}", layers.len(), z.unwrap_or(0.0)))
        }
        "supports" => run(session, Command::Supports(flag_arg(call, "enabled").unwrap_or(false))),
        "support_preview" => run(session, Command::PreviewSupport),
        "slice" => run(session, Command::Slice),
        "export_gcode" => run(session, Command::ExportGcode(path()?)),
        "export_models" => run(session, Command::ExportModels(path()?)),
        "open_gcode" => run(session, Command::OpenGcode(path()?)),
        "network_check" | "network_upload" => {
            let kind = match call.args.get("kind") {
                None => slicecraft_io::network::HostKind::OctoPrint,
                Some(k) => slicecraft_io::network::HostKind::from_name(k)
                    .ok_or("`kind` is octoprint or moonraker")?,
            };
            let printer = slicecraft_io::network::NetworkPrinter {
                kind,
                url: call.args.get("url").cloned().unwrap_or_default(),
                api_key: call.args.get("api_key").cloned().unwrap_or_default(),
            };
            if short(spec.name) == "network_check" {
                run(session, Command::CheckNetworkPrinter(printer))
            } else {
                let name = call.args.get("name").cloned().unwrap_or_else(|| session.job_name());
                let start = flag_arg(call, "start").unwrap_or(false);
                run(session, Command::UploadToNetwork { printer, name, start })
            }
        }
        other => Reply::Error(format!("{other} is declared but not implemented yet")),
    })
}

/// The document in a few lines, for an agent to find its way.
pub fn describe(session: &Session) -> String {
    let doc = session.document();
    let mut out = format!("profile: {}\n", session.profile_name());
    let s = &doc.settings;
    out.push_str(&format!(
        "bed: {} x {} x {} mm; layer {} mm; infill {} %; support {}\n",
        s.bed_width,
        s.bed_depth,
        s.bed_height,
        s.layer_height,
        s.infill_percentage,
        if s.support_enabled { "on" } else { "off" }
    ));
    if doc.objects.is_empty() {
        out.push_str("no models\n");
    }
    let outside = doc.outside_bed();
    for (i, o) in doc.objects.iter().enumerate() {
        let size = o.size();
        let report = o.mesh.report();
        out.push_str(&format!(
            "model {i}: {} · {} · at {:.1}, {:.1} · {:.1} x {:.1} x {:.1} mm · {} triangles · {}{}{}{}\n",
            o.name,
            o.role.name(),
            o.transform.translate.x,
            o.transform.translate.y,
            size.x,
            size.y,
            size.z,
            o.mesh.len(),
            if report.is_watertight() { "closed" } else { "open (repair suggested)" },
            o.group.map(|g| format!(" · group {g}")).unwrap_or_default(),
            if o.visible { "" } else { " · hidden" },
            if outside.contains(&o.name) { " · OUTSIDE THE BED" } else { "" },
        ));
    }
    for (i, z) in doc.zones.iter().enumerate() {
        out.push_str(&format!(
            "zone {i}: {} from {} to {} mm, {} setting(s)\n",
            z.name,
            z.z_from,
            z.z_to,
            z.overrides.len()
        ));
    }
    match session.slice_result() {
        Some(r) => out.push_str(&format!(
            "sliced: {} layers, {:.0} mm filament, about {:.0} s\n",
            r.layers.len(),
            r.stats.gcode.filament_mm,
            r.stats.gcode.estimated_seconds
        )),
        None => out.push_str("not sliced since the last change\n"),
    }
    out
}

fn list_settings(session: &Session, search: Option<&str>) -> String {
    let needle = search.map(|s| s.trim().to_ascii_lowercase()).unwrap_or_default();
    let mut out = String::new();
    let mut n = 0usize;
    for s in slicecraft_doc::settings::SETTINGS {
        if !needle.is_empty()
            && !s.key.contains(&needle)
            && !s.label.to_ascii_lowercase().contains(&needle)
        {
            continue;
        }
        let Some(v) = session.document().settings.get(s.key) else { continue };
        n += 1;
        if n > 400 {
            out.push_str("… more; narrow the search\n");
            break;
        }
        out.push_str(&format!(
            "{} = {} · {}{}\n",
            s.key,
            slicecraft_doc::profile::format_value(s.kind, &v),
            s.label,
            if s.implemented() { "" } else { " · planned" }
        ));
    }
    if n == 0 {
        out.push_str("no setting matches\n");
    }
    out
}

/// The setup assistant's answers from tool arguments; missing ones keep their defaults.
fn setup_from_args(
    args: &BTreeMap<String, String>,
) -> Result<slicecraft_doc::setup::PrinterSetup, String> {
    use slicecraft_doc::settings::{BedShape, Flavor};
    use slicecraft_doc::setup::{Drive, Material, PrinterSetup};
    let mut s = PrinterSetup::default();
    if let Some(name) = args.get("printer") {
        let needle = name.trim().to_ascii_lowercase();
        let m = slicecraft_doc::setup::PRINTERS
            .iter()
            .find(|m| m.name.to_ascii_lowercase().contains(&needle))
            .ok_or_else(|| {
                let names: Vec<&str> =
                    slicecraft_doc::setup::PRINTERS.iter().map(|m| m.name).collect();
                format!("unknown printer {name}; known: {}", names.join("; "))
            })?;
        s = PrinterSetup::from_model(m, s.material);
    }
    let num = |k: &str| -> Result<Option<f64>, String> {
        match args.get(k) {
            None => Ok(None),
            Some(v) => {
                v.trim().parse::<f64>().map(Some).map_err(|_| format!("`{k}` is not a number"))
            }
        }
    };
    if let Some(n) = args.get("name") {
        s.name = n.clone();
    }
    if let Some(f) = args.get("flavor") {
        s.flavor = Flavor::ALL
            .into_iter()
            .find(|x| x.name().eq_ignore_ascii_case(f.trim()))
            .ok_or("`flavor` is marlin, reprap or klipper")?;
    }
    let flag = |k: &str| args.get(k).is_some_and(|v| v == "true");
    if flag("round") {
        s.bed_shape = BedShape::Circle;
        s.origin_center = true;
    }
    if let Some(bed) = args.get("bed") {
        let parts: Vec<f64> = bed
            .split(['x', 'X', '*'])
            .map(|p| p.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| "`bed` is WxDxH in mm, such as 220x220x250".to_string())?;
        match (s.bed_shape, parts.as_slice()) {
            (BedShape::Circle, [d, h]) => (s.bed_width, s.bed_height) = (*d, *h),
            (BedShape::Rectangle, [w, d, h]) => {
                (s.bed_width, s.bed_depth, s.bed_height) = (*w, *d, *h)
            }
            _ => return Err("`bed` is WxDxH, or diameter x height with `round` = true".into()),
        }
    }
    if let Some(v) = num("nozzle")? {
        s.nozzle_diameter = v;
    }
    if let Some(v) = num("filament")? {
        s.filament_diameter = v;
    }
    if let Some(d) = args.get("drive") {
        s.drive = Drive::from_name(d).ok_or("`drive` is direct or bowden")?;
    }
    if let Some(m) = args.get("material") {
        s.material =
            Material::from_name(m).ok_or("`material` is pla, petg, abs, asa, tpu or nylon")?;
    }
    if let Some(h) = args.get("heated_bed") {
        s.heated_bed = h != "false";
    }
    if flag("origin_center") {
        s.origin_center = true;
    }
    Ok(s)
}

/// Every tool must name a real engine command; checked by a test and by `xtask`.
pub fn unbound_tools(session_command_ids: &[&str]) -> Vec<&'static str> {
    TOOLS.iter().filter(|t| !session_command_ids.contains(&t.command)).map(|t| t.name).collect()
}

impl fmt::Display for ToolSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.name, self.command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_engine::COMMANDS;

    #[test]
    fn every_tool_names_an_engine_command() {
        let ids: Vec<&str> = COMMANDS.iter().map(|c| c.id).collect();
        assert!(unbound_tools(&ids).is_empty());
    }

    #[test]
    fn settings_zones_and_placement_through_tools() {
        let mut s = Session::new();
        let set = |s: &mut Session, k: &str, v: &str| {
            dispatch(s, &Call::new("slicecraft.set_setting").arg("key", k).arg("value", v))
        };
        assert!(!set(&mut s, "infill_percentage", "35").is_error());
        assert_eq!(s.document().settings.infill_percentage, 35.0);
        assert!(set(&mut s, "infill_percentage", "lots").is_error());
        assert!(set(&mut s, "infill_percentage", "500").is_error());
        assert!(set(&mut s, "no_such_key", "1").is_error());
        let got =
            dispatch(&mut s, &Call::new("slicecraft.get_setting").arg("key", "infill_percentage"));
        assert!(
            matches!(&got, Reply::Text(t) if t.starts_with("infill_percentage = 35")),
            "{got:?}"
        );
        let list = dispatch(&mut s, &Call::new("slicecraft.list_settings").arg("search", "infill"));
        assert!(matches!(&list, Reply::Text(t) if t.contains("infill_pattern")));
        let zone = Call::new("slicecraft.add_zone")
            .arg("settings", "layer_height=0.1; infill_percentage=50")
            .arg("z_from", "5");
        assert!(!dispatch(&mut s, &zone).is_error());
        assert_eq!(s.document().zones.len(), 1);
        assert!(
            dispatch(&mut s, &Call::new("slicecraft.add_zone").arg("settings", "layer_height"))
                .is_error()
        );
        let Some(mesh) = slicecraft_render::fixtures::cube(10.0) else { return };
        let mut stl = Vec::new();
        stl.extend_from_slice(&[0u8; 80]);
        stl.extend_from_slice(&(mesh.triangles().len() as u32).to_le_bytes());
        for t in mesh.triangles() {
            stl.extend_from_slice(&[0u8; 12]);
            for v in [t.a, t.b, t.c] {
                for c in [v.x, v.y, v.z] {
                    stl.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            stl.extend_from_slice(&[0u8; 2]);
        }
        assert!(
            s.execute(Command::Import {
                name: "c".into(),
                bytes: stl,
                extension: Some("stl".into())
            })
            .is_ok()
        );
        let place =
            Call::new("slicecraft.place").arg("index", "0").arg("x", "50").arg("rotate_z", "45");
        assert!(!dispatch(&mut s, &place).is_error());
        assert_eq!(s.document().objects.first().map(|o| o.transform.translate.x), Some(50.0));
        let d = dispatch(&mut s, &Call::new("slicecraft.describe"));
        assert!(
            matches!(&d, Reply::Text(t) if t.contains("model 0: c") && t.contains("zone 0")),
            "{d:?}"
        );
        assert!(dispatch(&mut s, &Call::new("slicecraft.place").arg("index", "9")).is_error());
    }

    #[test]
    fn setup_printer_reads_its_arguments() {
        let mut s = Session::new();
        let call = Call::new("slicecraft.setup_printer")
            .arg("name", "Delta")
            .arg("round", "true")
            .arg("bed", "200x300")
            .arg("material", "abs")
            .arg("drive", "bowden");
        assert!(matches!(dispatch(&mut s, &call), Reply::Text(t) if t.contains("Delta")));
        assert!(s.document().settings.origin_center);
        assert_eq!(s.document().settings.bed_temperature, 100.0);
        assert!(
            dispatch(&mut s, &Call::new("slicecraft.setup_printer").arg("bed", "1x2")).is_error()
        );
        assert!(
            dispatch(&mut s, &Call::new("slicecraft.setup_printer").arg("nozzle", "big"))
                .is_error()
        );
    }

    #[test]
    fn network_tools_check_their_arguments() {
        let mut s = Session::new();
        assert!(dispatch(&mut s, &Call::new("slicecraft.network_check")).is_error());
        assert!(
            dispatch(
                &mut s,
                &Call::new("slicecraft.network_check").arg("url", "x").arg("kind", "nope")
            )
            .is_error()
        );
        let r =
            dispatch(&mut s, &Call::new("slicecraft.network_check").arg("url", "https://printer"));
        assert!(matches!(r, Reply::Error(e) if e.contains("HTTPS")));
        // nothing to slice: an error before any network traffic
        assert!(
            dispatch(&mut s, &Call::new("slicecraft.network_upload").arg("url", "127.0.0.1:9"))
                .is_error()
        );
    }

    #[test]
    fn roles_and_groups_need_their_arguments() {
        let mut s = Session::new();
        assert!(dispatch(&mut s, &Call::new("slicecraft.model_role").arg("index", "0")).is_error());
        assert!(
            dispatch(
                &mut s,
                &Call::new("slicecraft.model_role").arg("index", "0").arg("role", "glue")
            )
            .is_error()
        );
        assert!(dispatch(&mut s, &Call::new("slicecraft.group").arg("indices", "0,x")).is_error());
        // no model 0 yet: an engine error, not a crash
        assert!(
            dispatch(
                &mut s,
                &Call::new("slicecraft.model_role").arg("index", "0").arg("role", "cutter")
            )
            .is_error()
        );
    }

    #[test]
    fn unknown_tool_is_an_error() {
        let mut s = Session::new();
        assert!(dispatch(&mut s, &Call::new("slicecraft.nope")).is_error());
    }

    #[test]
    fn preview_reports_layers() {
        let mut s = Session::new();
        let r = dispatch(&mut s, &Call::new("slicecraft.preview"));
        assert!(matches!(r, Reply::Text(t) if t.contains("0 layers")));
    }

    #[test]
    fn missing_arguments_are_errors_not_dialogs() {
        let mut s = Session::new();
        assert!(dispatch(&mut s, &Call::new("slicecraft.import")).is_error());
        assert!(dispatch(&mut s, &Call::new("slicecraft.export_gcode")).is_error());
        assert!(dispatch(&mut s, &Call::new("slicecraft.remove").arg("index", "x")).is_error());
    }

    #[test]
    fn import_slice_export_round_trip() {
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let model = dir.join(format!("slicecraft-mcp-{pid}.stl"));
        let out = dir.join(format!("slicecraft-mcp-{pid}.gcode"));
        let Some(mesh) = slicecraft_render::fixtures::cube(10.0) else { return };
        let mut stl = String::from("solid c\n");
        for t in mesh.triangles() {
            stl.push_str("facet normal 0 0 0\nouter loop\n");
            for v in t.vertices() {
                stl.push_str(&format!("vertex {} {} {}\n", v.x, v.y, v.z));
            }
            stl.push_str("endloop\nendfacet\n");
        }
        if std::fs::write(&model, stl).is_err() {
            return;
        }
        let mut s = Session::new();
        let path = model.display().to_string();
        assert!(!dispatch(&mut s, &Call::new("slicecraft.import").arg("path", &path)).is_error());
        assert!(
            matches!(dispatch(&mut s, &Call::new("slicecraft.slice")), Reply::Text(t) if t.contains("layers"))
        );
        let out_s = out.display().to_string();
        let r = dispatch(&mut s, &Call::new("slicecraft.export_gcode").arg("path", &out_s));
        assert!(!r.is_error(), "{r:?}");
        assert!(out.exists());
        let _ = std::fs::remove_file(&model);
        let _ = std::fs::remove_file(&out);
    }
}
