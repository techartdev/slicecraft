#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! `slicecraft-cli` — headless SliceCraft.
//!
//! ```text
//! slicecraft-cli commands
//! slicecraft-cli info  <model.stl|obj>
//! slicecraft-cli slice [project.scproj] [model]... -o <out.gcode> [options]
//!   --flavor marlin|reprap|klipper   --layer-height MM   --first-layer MM
//!   --width MM   --perimeters N   --infill PCT   --top N   --bottom N
//!   --nozzle C   --bed C   --skirt N   --profile FILE   --json
//! ```
//!
//! A project file brings its models, placement and profile; `--profile` and
//! the options above then override its settings, and further models are added.
//!
//! Exit status is 1 on a refused input, 2 on a usage error. Every action goes
//! through the engine's commands, the same ones the window and MCP use.

use slicecraft_doc::Flavor;
use slicecraft_engine::{COMMANDS, Command, Outcome, Session};
use slicecraft_io::format_duration;
use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0u8,
        Err(e) => {
            eprintln!("slicecraft-cli: {e}");
            if matches!(e, CliError::Usage(_)) { 2 } else { 1 }
        }
    };
    std::process::ExitCode::from(code)
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Failed(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Usage(m) => write!(f, "{m}\n{USAGE}"),
            CliError::Failed(m) => write!(f, "{m}"),
        }
    }
}

const USAGE: &str = "usage: slicecraft-cli commands | info <model> | slice [project.scproj] [model]... -o <out.gcode> [--flavor NAME] [--layer-height MM] [--first-layer MM] [--width MM] [--perimeters N] [--infill PCT] [--top N] [--bottom N] [--nozzle C] [--bed C] [--skirt N] [--profile FILE] [--json]";

fn failed(e: impl std::fmt::Display) -> CliError {
    CliError::Failed(e.to_string())
}

fn run(args: &[String]) -> Result<(), CliError> {
    match args.first().map(String::as_str) {
        Some("commands") => {
            for c in COMMANDS {
                println!("{:<14} {:<18} {:?}", c.id, c.label, c.state);
            }
            Ok(())
        }
        Some("info") => {
            let path = args.get(1).ok_or_else(|| CliError::Usage("info needs a model".into()))?;
            let mut session = Session::new();
            let outcome =
                session.execute(Command::ImportFile(PathBuf::from(path))).map_err(failed)?;
            let b = session.document().bounds();
            println!("file: {path}");
            if let Outcome::Imported { triangles, skipped_facets, format, .. } = outcome {
                println!("format: {}", format.name());
                println!("triangles: {triangles}");
                println!("skipped facets: {skipped_facets}");
            }
            let size = b.size();
            println!("size: {:.3} x {:.3} x {:.3} mm", size.x, size.y, size.z);
            println!("placed at: {} .. {}", b.min(), b.max());
            Ok(())
        }
        Some("slice") => slice(args.get(1..).unwrap_or(&[])),
        _ => Err(CliError::Usage("unknown command".into())),
    }
}

fn number<T: std::str::FromStr>(flag: &str, value: Option<&String>) -> Result<T, CliError> {
    value
        .and_then(|v| v.parse::<T>().ok())
        .ok_or_else(|| CliError::Usage(format!("{flag} needs a number")))
}

fn slice(args: &[String]) -> Result<(), CliError> {
    let mut models = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut json = false;
    let mut session = Session::new();
    // a project first, then a profile file, so the options below override both
    let is_project = |a: &String| a.to_ascii_lowercase().ends_with(".scproj");
    let mut projects = args.iter().filter(|a| is_project(a));
    let project = projects.next();
    if projects.next().is_some() {
        return Err(CliError::Usage("slice takes at most one project".into()));
    }
    if let Some(path) = project
        && let Outcome::ProjectOpened { warnings, .. } =
            session.execute(Command::OpenProject(PathBuf::from(path))).map_err(failed)?
    {
        for w in warnings {
            eprintln!("slicecraft-cli: {path}: {w}");
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--profile") {
        let path =
            args.get(i + 1).ok_or_else(|| CliError::Usage("--profile needs a file".into()))?;
        if let Outcome::ProfileLoaded { warnings, .. } =
            session.execute(Command::LoadProfile(PathBuf::from(path))).map_err(failed)?
        {
            for w in warnings {
                eprintln!("slicecraft-cli: {path}: {w}");
            }
        }
    }
    let mut p = session.document().settings.clone();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--output" => {
                out = Some(PathBuf::from(
                    it.next().ok_or_else(|| CliError::Usage("-o needs a path".into()))?,
                ));
            }
            "--json" => json = true,
            "--profile" => {
                it.next();
            }
            "--reprap" => p.flavor = Flavor::RepRap,
            "--flavor" => {
                let name = it.next().map(String::as_str).unwrap_or_default();
                p.flavor = Flavor::from_name(name)
                    .ok_or_else(|| CliError::Usage(format!("unknown flavor {name:?}")))?;
            }
            "--layer-height" => p.layer_height = number(a, it.next())?,
            "--first-layer" => p.first_layer_height = number(a, it.next())?,
            "--width" => p.extrusion_width = number(a, it.next())?,
            "--perimeters" => p.perimeter_count = number(a, it.next())?,
            "--infill" => p.infill_percentage = number(a, it.next())?,
            "--top" => p.solid_layers_top = number(a, it.next())?,
            "--bottom" => p.solid_layers_bottom = number(a, it.next())?,
            "--nozzle" => p.nozzle_temperature = number(a, it.next())?,
            "--bed" => p.bed_temperature = number(a, it.next())?,
            "--skirt" => {
                let n: u32 = number(a, it.next())?;
                p.skirt_enabled = n > 0;
                p.skirt_loops = n.max(1);
            }
            flag if flag.starts_with("--") => {
                return Err(CliError::Usage(format!("unknown option {flag}")));
            }
            _ if is_project(a) => {}
            model => models.push(PathBuf::from(model)),
        }
    }
    // the old form `slice <in> <out.gcode>` still works
    if out.is_none()
        && models.len() == 2
        && models.get(1).is_some_and(|m| m.extension().is_some_and(|e| e == "gcode"))
    {
        out = models.pop();
    }
    let out = out.ok_or_else(|| CliError::Usage("slice needs -o <out.gcode>".into()))?;
    if models.is_empty() && project.is_none() {
        return Err(CliError::Usage("slice needs a project or at least one model".into()));
    }
    session.execute(Command::Profile(Box::new(p))).map_err(failed)?;
    for m in models {
        session.execute(Command::ImportFile(m)).map_err(failed)?;
    }
    let Outcome::Sliced { layers, stats } = session.execute(Command::Slice).map_err(failed)? else {
        return Err(CliError::Failed("slicing did not report a result".into()));
    };
    let Outcome::Exported { bytes, .. } =
        session.execute(Command::ExportGcode(out.clone())).map_err(failed)?
    else {
        return Err(CliError::Failed("export did not report a result".into()));
    };
    let g = stats.gcode;
    if json {
        let warnings: Vec<String> =
            stats.warnings.iter().map(|w| format!("\"{}\"", w.replace('"', "'"))).collect();
        println!(
            "{{\"output\":\"{}\",\"bytes\":{bytes},\"layers\":{layers},\"filament_mm\":{:.1},\"filament_g\":{:.2},\"seconds\":{:.0},\"retractions\":{},\"warnings\":[{}]}}",
            out.display().to_string().replace('\\', "\\\\").replace('"', "\\\""),
            g.filament_mm,
            g.filament_grams,
            g.estimated_seconds,
            g.retractions,
            warnings.join(",")
        );
    } else {
        println!("wrote {} ({bytes} bytes)", out.display());
        println!("layers: {layers}");
        println!("filament: {:.0} mm ({:.1} g)", g.filament_mm, g.filament_grams);
        println!("estimated time: {}", format_duration(g.estimated_seconds));
        for w in &stats.warnings {
            println!("warning: {w}");
        }
    }
    Ok(())
}
