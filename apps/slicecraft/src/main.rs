#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
// no console window behind the GUI on Windows release builds
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! SliceCraft desktop entry point.
//!
//! `slicecraft [model files…]` opens the window with those models on the bed.
//! `--version` and `--commands` answer from the terminal without a window.

use slicecraft_engine::{COMMANDS, Command, Session};
use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => {
            println!("slicecraft {}", env!("CARGO_PKG_VERSION"));
            std::process::ExitCode::SUCCESS
        }
        Some("--commands") => {
            for c in COMMANDS {
                println!("{:<14} {:<18} {:?}", c.id, c.label, c.state);
            }
            std::process::ExitCode::SUCCESS
        }
        _ => {
            let mut session = Session::new();
            for a in args.iter().filter(|a| !a.starts_with("--")) {
                if let Err(e) = session.execute(Command::ImportFile(PathBuf::from(a))) {
                    eprintln!("slicecraft: {a}: {e}");
                }
            }
            match slicecraft_ui_egui::run(session) {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("slicecraft: cannot open the window: {e}");
                    std::process::ExitCode::FAILURE
                }
            }
        }
    }
}
