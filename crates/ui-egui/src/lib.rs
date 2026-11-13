#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! The swappable front end: an egui/eframe window over the engine.
//!
//! Nothing below this crate knows a UI exists. Panels read engine state and
//! act through `slicecraft_engine::Command`, the same commands the CLI and MCP
//! use; replacing this crate replaces the UI and nothing else.

pub mod app;
pub mod camera;
pub mod config;
pub mod dialogs;
pub mod icons;
pub mod menus;
pub mod panels;
pub mod scene;
pub mod viewport;

use slicecraft_engine::{COMMANDS, CommandState, Session};
use slicecraft_render::{Layer, Point2};

pub use app::SliceCraftApp;

/// Open the SliceCraft window and run until it is closed. `files` are opened
/// as if dropped on the window: projects, profiles or models.
pub fn run(session: Session, files: Vec<std::path::PathBuf>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("SliceCraft")
            .with_inner_size([1500.0, 920.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "SliceCraft",
        options,
        Box::new(move |_cc| {
            let mut app = SliceCraftApp::with_config(session, config::config_dir());
            app.open_paths(files);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}

/// Which panel has focus. Panels read engine state and act through commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Objects,
    Settings,
    Preview,
    Console,
}

/// A row in the command palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteRow {
    pub id: &'static str,
    pub label: &'static str,
    pub enabled: bool,
}

/// Headless view state: what a front end shows, without a window.
#[derive(Debug, Default)]
pub struct ViewState {
    pub panel: Option<Panel>,
    pub preview_layer_index: usize,
    pub show_supports_overlay: bool,
}

impl ViewState {
    pub fn new() -> Self {
        Self { panel: Some(Panel::Objects), preview_layer_index: 0, show_supports_overlay: false }
    }

    /// The command palette, with planned commands shown but disabled.
    pub fn palette() -> Vec<PaletteRow> {
        COMMANDS
            .iter()
            .map(|c| PaletteRow {
                id: c.id,
                label: c.label,
                enabled: c.state == CommandState::Ready,
            })
            .collect()
    }

    /// Pick which layer to show, clamped to the layers that exist.
    pub fn scrub(&mut self, delta: i32, layers: &[Layer]) {
        if layers.is_empty() {
            self.preview_layer_index = 0;
            return;
        }
        let last = layers.len().saturating_sub(1);
        let current = self.preview_layer_index.min(last) as i64;
        let next = (current + i64::from(delta)).clamp(0, last as i64);
        self.preview_layer_index = next as usize;
    }

    /// The first loop of the selected layer, for a minimal canvas.
    pub fn visible_loops<'a>(&self, layers: &'a [Layer]) -> &'a [Point2] {
        layers
            .get(self.preview_layer_index)
            .and_then(|l| l.loops.first())
            .map(|l| l.points.as_slice())
            .unwrap_or(&[])
    }

    /// Layers a session would preview, and the Z of the selected one.
    pub fn describe(&self, session: &Session) -> (usize, Option<f64>) {
        let layers = session.preview_layers();
        (layers.len(), layers.get(self.preview_layer_index).map(|l| l.z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_render::Loop2;

    fn layer(z: f64) -> Layer {
        Layer {
            z,
            loops: vec![Loop2 {
                points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), Point2::new(0.0, 1.0)],
            }],
            triangles_considered: 1,
            dropped_chains: 0,
        }
    }

    #[test]
    fn palette_marks_planned_commands_disabled() {
        let rows = ViewState::palette();
        assert!(rows.iter().any(|r| r.id == "supports" && !r.enabled));
        assert!(rows.iter().any(|r| r.id == "undo" && r.enabled));
        assert!(rows.iter().any(|r| r.id == "slice" && r.enabled));
    }

    #[test]
    fn scrubbing_clamps() {
        let mut v = ViewState::new();
        let layers = vec![layer(0.2), layer(0.4), layer(0.6)];
        v.scrub(5, &layers);
        assert_eq!(v.preview_layer_index, 2);
        v.scrub(-10, &layers);
        assert_eq!(v.preview_layer_index, 0);
        v.scrub(1, &[]);
        assert_eq!(v.preview_layer_index, 0);
    }

    #[test]
    fn visible_loops_is_bounded() {
        let v = ViewState::new();
        let layers = vec![layer(0.2)];
        assert_eq!(v.visible_loops(&layers).len(), 3);
        assert!(v.visible_loops(&[]).is_empty());
    }

    #[test]
    fn app_starts_headless_without_a_window() {
        let app = SliceCraftApp::default();
        assert!(app.session().document().objects.is_empty());
    }
}
