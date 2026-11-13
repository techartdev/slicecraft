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
//! The transport (JSON-RPC over stdio) arrives with the MCP phase. What exists
//! here is the contract and the dispatch table, so a tool cannot be added to the
//! agent surface without naming the engine command it drives. Programmatic
//! calls never open dialogs: every path is an argument.

use slicecraft_engine::{Command, Outcome, Session};
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
}

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "slicecraft.import",
        command: "import",
        description: "Import an STL or OBJ model from `path` into the document",
    },
    ToolSpec {
        name: "slicecraft.open_project",
        command: "open-project",
        description: "Replace the document with the SliceCraft project at `path`",
    },
    ToolSpec {
        name: "slicecraft.save_project",
        command: "save-project",
        description: "Save models, placement and profile as a project at `path`",
    },
    ToolSpec {
        name: "slicecraft.arrange",
        command: "arrange",
        description: "Lay all models out on the bed without overlap",
    },
    ToolSpec {
        name: "slicecraft.remove",
        command: "remove",
        description: "Remove the model at `index`",
    },
    ToolSpec {
        name: "slicecraft.preview",
        command: "preview",
        description: "Describe the layer preview of the current document",
    },
    ToolSpec {
        name: "slicecraft.supports",
        command: "supports",
        description: "Turn automatic support on (`enabled` = true) or off",
    },
    ToolSpec {
        name: "slicecraft.support_preview",
        command: "support-preview",
        description: "Report the support a slice would print (layers, volume) without slicing",
    },
    ToolSpec {
        name: "slicecraft.slice",
        command: "slice",
        description: "Slice the document and report layers, filament and time",
    },
    ToolSpec {
        name: "slicecraft.export_gcode",
        command: "export-gcode",
        description: "Write G-code for the current document to `path`",
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

/// Dispatch a call against a session.
pub fn dispatch(session: &mut Session, call: &Call) -> Reply {
    let Some(spec) = TOOLS.iter().find(|t| t.name == call.tool) else {
        return Reply::Error(format!("unknown tool: {}", call.tool));
    };
    if session.command(spec.command).is_none() {
        return Reply::Error(format!("tool {} names no engine command", spec.name));
    }
    let path = call.args.get("path").map(PathBuf::from);
    match spec.command {
        "import" => match path {
            Some(p) => run(session, Command::ImportFile(p)),
            None => Reply::Error("import needs a `path` argument".into()),
        },
        "open-project" => match path {
            Some(p) => run(session, Command::OpenProject(p)),
            None => Reply::Error("open_project needs a `path` argument".into()),
        },
        "save-project" => match path {
            Some(p) => run(session, Command::SaveProject(p)),
            None => Reply::Error("save_project needs a `path` argument".into()),
        },
        "arrange" => run(session, Command::Arrange),
        "remove" => match call.args.get("index").and_then(|i| i.parse::<usize>().ok()) {
            Some(index) => run(session, Command::Remove { index }),
            None => Reply::Error("remove needs an `index` argument".into()),
        },
        "preview" => {
            let layers = session.preview_layers();
            let z = layers.last().map(|l| l.z);
            Reply::Text(format!("{} layers, top z {}", layers.len(), z.unwrap_or(0.0)))
        }
        "supports" => match call.args.get("enabled").map(String::as_str) {
            Some("true") => run(session, Command::Supports(true)),
            Some("false") => run(session, Command::Supports(false)),
            _ => Reply::Error("supports needs `enabled` = true or false".into()),
        },
        "support-preview" => run(session, Command::PreviewSupport),
        "slice" => run(session, Command::Slice),
        "export-gcode" => match path {
            Some(p) => run(session, Command::ExportGcode(p)),
            None => Reply::Error("export_gcode needs a `path` argument".into()),
        },
        other => Reply::Error(format!("{other} is declared but not implemented yet")),
    }
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
