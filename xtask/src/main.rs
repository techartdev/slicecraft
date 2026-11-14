#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! Repository tasks. `cargo xtask <task>`.
//!
//! - `ci`      run what CI runs, in order, stopping at the first failure
//! - `layers`  check the crate layering rules
//! - `stats`   count lines per crate
//! - `dist`    build the release and pack it (archive, installer, checksums)

mod dist;

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("ci") => ci(),
        Some("layers") => layers(),
        Some("stats") => stats(),
        Some("dist") => dist::dist(),
        _ => {
            eprintln!("usage: cargo xtask <ci|layers|stats|dist>");
            ExitCode::from(2)
        }
    }
}

fn ci() -> ExitCode {
    let steps: [(&str, &[&str]); 4] = [
        ("fmt", &["fmt", "--all", "--check"]),
        ("clippy", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
        ("test", &["test", "--workspace"]),
        ("layers", &["run", "-q", "-p", "xtask", "--", "layers"]),
    ];
    for (name, args) in steps {
        println!("== {name}: cargo {}", args.join(" "));
        match Command::new("cargo").args(args).status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("xtask ci: {name} failed ({status})");
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("xtask ci: cannot run cargo: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    println!("xtask ci: ok");
    ExitCode::SUCCESS
}

/// Nothing below L5 may depend on a windowing or GPU crate, and only apps may
/// depend on the UI crate. This reads the manifests; it does not build them.
/// Comments and the crate's own name are ignored, so a description that mentions
/// egui cannot trip the check.
fn layers() -> ExitCode {
    const FORBIDDEN_BELOW_UI: [&str; 6] = ["egui", "eframe", "winit", "wgpu", "rfd", "egui-wgpu"];
    const UI_CRATE: &str = "slicecraft-ui-egui";
    let entries = match std::fs::read_dir("crates") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("xtask layers: cannot read crates/: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut failures = Vec::new();
    for entry in entries.flatten() {
        let manifest = entry.path().join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else { continue };
        // judge by the package name, not the directory name (`crates/ui-egui`
        // holds `slicecraft-ui-egui`)
        let crate_name =
            package_name(&text).unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        let is_ui = crate_name == UI_CRATE;
        for dep in dependency_keys(&text) {
            if FORBIDDEN_BELOW_UI.iter().any(|f| dep == *f) && !is_ui {
                failures.push(format!(
                    "{crate_name} depends on {dep}, which only the UI layer may use"
                ));
            }
            if dep == UI_CRATE && !is_ui {
                failures.push(format!("{crate_name} depends on {UI_CRATE}; only apps may"));
            }
        }
    }
    for f in &failures {
        eprintln!("xtask layers: {f}");
    }
    if failures.is_empty() {
        println!("xtask layers: ok");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The `name` in a manifest's `[package]` section.
fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "name"
        {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// Dependency names from `[dependencies]`, `[dev-dependencies]` and
/// `[build-dependencies]` sections of a manifest.
fn dependency_keys(manifest: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut in_deps = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_deps = line.contains("dependencies");
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            // `name.workspace = true` names the dependency `name`
            let key = key.trim().trim_matches('"');
            let key = key.split('.').next().unwrap_or(key).trim_matches('"');
            if !key.is_empty() {
                keys.push(key.to_string());
            }
        }
    }
    keys
}

fn stats() -> ExitCode {
    let mut total = 0usize;
    for group in ["crates", "apps", "xtask"] {
        let Ok(entries) = std::fs::read_dir(group) else { continue };
        for entry in entries.flatten() {
            let src = entry.path().join("src");
            let Ok(files) = std::fs::read_dir(&src) else { continue };
            let mut lines = 0usize;
            for file in files.flatten() {
                if file.path().extension().is_some_and(|e| e == "rs")
                    && let Ok(text) = std::fs::read_to_string(file.path())
                {
                    lines += text.lines().count();
                }
            }
            if lines > 0 {
                println!("{:>6}  {}", lines, entry.file_name().to_string_lossy());
                total += lines;
            }
        }
    }
    println!("{:>6}  total", total);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "[package]
name = \"slicecraft-ui-egui\"

[dependencies]
# egui is only a comment here
eframe = { version = \"0.35\" }
slicecraft-engine.workspace = true

[lints]
workspace = true
";

    #[test]
    fn package_name_comes_from_the_package_section() {
        assert_eq!(package_name(MANIFEST).as_deref(), Some("slicecraft-ui-egui"));
        assert_eq!(
            package_name(
                "[dependencies]
name = \"x\"
"
            ),
            None
        );
    }

    #[test]
    fn dependency_keys_ignore_comments_and_other_sections() {
        let keys = dependency_keys(MANIFEST);
        assert_eq!(keys, vec!["eframe".to_string(), "slicecraft-engine".to_string()]);
    }
}
