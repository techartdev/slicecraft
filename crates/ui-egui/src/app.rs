//! The desktop window's state and the actions every panel shares.
//!
//! Panels read engine state and act through `slicecraft_engine::Command`.
//! Slicing runs on a worker thread through the engine's `SliceJob`; its result
//! is accepted only if the document has not changed meanwhile.

use crate::camera::{OrbitCamera, ViewPreset};
use crate::dialogs::{Dialogs, SettingsEditor};
use crate::scene::{self, BedStyle, Coloring, KindFilter, ModelGeometry, PreviewLayer};
use eframe::egui::{self, Key};
use slicecraft_doc::{Axis, PrintSettings, Transform};
use slicecraft_engine::{
    COMMANDS, Command, CommandState, EngineError, Outcome, Session, SliceObserver, SliceOutput,
    Stage,
};
use slicecraft_io::format_duration;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

/// Which half of the workflow the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Place models and edit the profile.
    Prepare,
    /// Inspect the sliced toolpaths.
    Preview,
}

/// What a left-drag in the 3D view does in prepare mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
    Rotate,
    Scale,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Select, Tool::Move, Tool::Rotate, Tool::Scale];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select and orbit (Q)",
            Tool::Move => "Move on the bed (W)",
            Tool::Rotate => "Rotate about Z (E)",
            Tool::Scale => "Scale (R)",
        }
    }
}

/// Progress shared between the slicing thread and the window.
#[derive(Default)]
pub struct Progress {
    pub stage: AtomicUsize,
    pub done: AtomicUsize,
    pub total: AtomicUsize,
}

pub const STAGES: [Stage; 4] = [Stage::Sections, Stage::Shells, Stage::Infill, Stage::Gcode];

struct Observer {
    progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
}

impl SliceObserver for Observer {
    fn progress(&mut self, stage: Stage, done: usize, total: usize) {
        let i = STAGES.iter().position(|s| *s == stage).unwrap_or(0);
        self.progress.stage.store(i, Ordering::Relaxed);
        self.progress.done.store(done, Ordering::Relaxed);
        self.progress.total.store(total, Ordering::Relaxed);
        self.ctx.request_repaint();
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub struct Job {
    pub revision: u64,
    rx: mpsc::Receiver<Result<SliceOutput, EngineError>>,
    pub progress: Arc<Progress>,
    pub cancel: Arc<AtomicBool>,
}

/// Toolpath view state.
#[derive(Debug, Clone)]
pub struct PreviewState {
    pub first_layer: usize,
    pub last_layer: usize,
    /// Show only this many layers ending at `last_layer`.
    pub only_n: Option<usize>,
    pub by_line: bool,
    /// Moves drawn on the last layer while stepping line by line.
    pub line_cursor: usize,
    pub playing: bool,
    /// Layers (or moves, by line) per second.
    pub speed: f32,
    pub coloring: Coloring,
    pub filter: KindFilter,
    pub show_travel: bool,
    pub show_retractions: bool,
    pub show_toolhead: bool,
    pub ghost_model: bool,
    play_accum: f32,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            first_layer: 0,
            last_layer: 0,
            only_n: None,
            by_line: false,
            line_cursor: 0,
            playing: false,
            speed: 10.0,
            coloring: Coloring::Feature,
            filter: KindFilter::default(),
            show_travel: false,
            show_retractions: false,
            show_toolhead: true,
            ghost_model: false,
            play_accum: 0.0,
        }
    }
}

/// Something that would drop unsaved changes, waiting for the user's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    NewProject,
    /// Open a project; `None` asks for the file.
    OpenProject(Option<PathBuf>),
    Quit,
}

impl Pending {
    pub fn verb(&self) -> &'static str {
        match self {
            Pending::NewProject => "starting a new project",
            Pending::OpenProject(_) => "opening another project",
            Pending::Quit => "quitting",
        }
    }
}

/// Window preferences, remembered between sessions (see `config`).
#[derive(Debug, Clone)]
pub struct Preferences {
    pub bed: BedStyle,
    pub dark: bool,
    pub invert_zoom: bool,
    pub orbit_sensitivity: f32,
    pub preview_after_slice: bool,
    pub fit_on_import: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            bed: BedStyle::default(),
            dark: true,
            invert_zoom: false,
            orbit_sensitivity: 1.0,
            preview_after_slice: true,
            fit_on_import: false,
        }
    }
}

/// The SliceCraft window.
pub struct SliceCraftApp {
    pub(crate) session: Session,
    pub(crate) camera: OrbitCamera,
    pub(crate) mode: Mode,
    pub(crate) tool: Tool,
    pub(crate) selected: BTreeSet<usize>,
    pub(crate) status: String,
    pub(crate) status_is_error: bool,
    pub(crate) job: Option<Job>,
    pending_export: Option<PathBuf>,
    pub(crate) geometry: Option<(u64, ModelGeometry)>,
    pub(crate) preview_cache: Option<(usize, Vec<PreviewLayer>)>,
    pub(crate) preview: PreviewState,
    pub(crate) prefs: Preferences,
    pub(crate) dialogs: Dialogs,
    /// Section view: hide model parts above this height.
    pub(crate) section: Option<f64>,
    pub(crate) uniform_scale: bool,
    pub(crate) drag: Option<crate::viewport::Drag>,
    applied_dark: Option<bool>,
    /// Recent project files, newest first.
    pub(crate) recent: Vec<PathBuf>,
    /// Where preferences are remembered; `None` keeps them for this session only.
    config_dir: Option<PathBuf>,
    /// An action waiting for "save your changes?".
    pub(crate) confirm: Option<Pending>,
    /// Set once the user agreed to close, so the next close request goes through.
    allow_close: bool,
    /// Close the window on the next frame (after "save changes?" was answered).
    quit_requested: bool,
    shown_title: String,
}

impl Default for SliceCraftApp {
    fn default() -> Self {
        Self::new(Session::new())
    }
}

impl SliceCraftApp {
    pub fn new(session: Session) -> Self {
        let bed = session.document().bed.clone();
        Self {
            session,
            camera: OrbitCamera::for_bed(bed.width, bed.depth, bed.height),
            mode: Mode::Prepare,
            tool: Tool::Select,
            selected: BTreeSet::new(),
            status: "Drop STL or OBJ files on the window, or use Import.".into(),
            status_is_error: false,
            job: None,
            pending_export: None,
            geometry: None,
            preview_cache: None,
            preview: PreviewState::default(),
            prefs: Preferences::default(),
            dialogs: Dialogs::default(),
            section: None,
            uniform_scale: true,
            drag: None,
            applied_dark: None,
            recent: Vec::new(),
            config_dir: None,
            confirm: None,
            allow_close: false,
            quit_requested: false,
            shown_title: String::new(),
        }
    }

    /// The window with remembered preferences, recent projects and the last
    /// profile from `dir`, saving them back there on exit.
    pub fn with_config(session: Session, dir: Option<PathBuf>) -> Self {
        let mut app = Self::new(session);
        if let Some(dir) = &dir {
            let (config, notes) = crate::config::load(dir);
            app.prefs = config.prefs;
            app.recent = config.recent;
            if let Some(profile) = config.profile
                && app.session.project_path().is_none()
                && let Err(e) = app.session.restore_profile(profile)
            {
                app.error(format!("The last profile was not restored: {e}"));
            }
            if !notes.is_empty() {
                app.info(format!("Preferences: {}", notes.join("; ")));
            }
            let bed = app.session.document().bed.clone();
            app.camera = OrbitCamera::for_bed(bed.width, bed.depth, bed.height);
        }
        app.config_dir = dir;
        app
    }

    /// Write preferences, recent projects and the current profile.
    pub(crate) fn persist(&mut self) {
        let Some(dir) = self.config_dir.clone() else { return };
        let config = crate::config::Config {
            prefs: self.prefs.clone(),
            recent: self.recent.clone(),
            profile: Some(self.session.profile()),
        };
        if let Err(e) = crate::config::save(&dir, &config) {
            self.error(format!("Preferences were not saved: {e}"));
        }
    }

    /// Open files given on the command line or dropped: projects, profiles or models.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>) {
        let mut models = Vec::new();
        for path in paths {
            match extension(&path).as_deref() {
                Some("scproj") => self.request(Pending::OpenProject(Some(path))),
                Some("scprofile" | "fff") => {
                    self.run(Command::LoadProfile(path));
                }
                _ => models.push(path),
            }
        }
        if !models.is_empty() {
            self.import_paths(models);
        }
    }

    /// The project's display name: its file name, or "Untitled".
    pub(crate) fn project_name(&self) -> String {
        self.session
            .project_path()
            .and_then(|p| p.file_stem())
            .map_or_else(|| "Untitled".into(), |s| s.to_string_lossy().into_owned())
    }

    /// Unsaved work that an action would drop.
    pub(crate) fn has_unsaved_work(&self) -> bool {
        self.session.is_modified() && !self.session.document().objects.is_empty()
    }

    /// Run an action that would replace the document, asking first when
    /// there are unsaved changes.
    pub(crate) fn request(&mut self, action: Pending) {
        if self.has_unsaved_work() {
            self.confirm = Some(action);
        } else {
            self.perform(action);
        }
    }

    /// Run an action whose unsaved changes were dealt with.
    pub(crate) fn perform(&mut self, action: Pending) {
        match action {
            Pending::NewProject => {
                if self.run(Command::NewProject).is_some() {
                    self.after_document_replaced();
                    self.info("New project.");
                }
            }
            Pending::OpenProject(Some(path)) => self.open_project(path),
            Pending::OpenProject(None) => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Open project")
                    .add_filter("SliceCraft project", &["scproj"])
                    .pick_file()
                {
                    self.open_project(path);
                }
            }
            Pending::Quit => {
                self.allow_close = true;
                self.quit_requested = true;
            }
        }
    }

    fn open_project(&mut self, path: PathBuf) {
        if let Some(Outcome::ProjectOpened { objects, warnings, .. }) =
            self.run(Command::OpenProject(path.clone()))
        {
            crate::config::push_recent(&mut self.recent, &path);
            self.after_document_replaced();
            let mut msg = format!("Opened {} ({objects} model(s)).", path.display());
            if !warnings.is_empty() {
                msg.push_str(&format!(
                    " {} line(s) skipped: {}",
                    warnings.len(),
                    warnings.join("; ")
                ));
            }
            self.info(msg);
            self.persist();
        } else if !path.exists() {
            // a project that is gone leaves the recent list
            self.recent.retain(|p| p != &path);
        }
    }

    fn after_document_replaced(&mut self) {
        self.selected.clear();
        self.mode = Mode::Prepare;
        self.dialogs.settings = None;
        self.refresh_caches();
        self.fit_view();
    }

    /// Save to the project's file, or ask for one. False when nothing was saved.
    pub(crate) fn save_project(&mut self, ask: bool) -> bool {
        let path = match self.session.project_path() {
            Some(p) if !ask => Some(p.to_path_buf()),
            current => {
                let name = current
                    .and_then(|p| p.file_name())
                    .map_or_else(|| "project.scproj".into(), |n| n.to_string_lossy().into_owned());
                rfd::FileDialog::new()
                    .set_title("Save project")
                    .add_filter("SliceCraft project", &["scproj"])
                    .set_file_name(name)
                    .save_file()
            }
        };
        let Some(mut path) = path else { return false };
        if path.extension().is_none() {
            path.set_extension("scproj");
        }
        if self.run(Command::SaveProject(path.clone())).is_some() {
            crate::config::push_recent(&mut self.recent, &path);
            self.persist();
            true
        } else {
            false
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let dirty = if self.session.is_modified() { " *" } else { "" };
        let title = format!("{}{dirty} - SliceCraft", self.project_name());
        if title != self.shown_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.shown_title = title;
        }
    }

    /// Catch the window's close button when there is unsaved work.
    fn guard_close(&mut self, ctx: &egui::Context) {
        if self.quit_requested {
            self.quit_requested = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.has_unsaved_work() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm = Some(Pending::Quit);
            } else {
                self.allow_close = true;
            }
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub(crate) fn info(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_is_error = false;
    }

    pub(crate) fn error(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_is_error = true;
    }

    /// The first selected model.
    pub(crate) fn primary(&self) -> Option<usize> {
        self.selected.iter().next().copied()
    }

    pub(crate) fn command_ready(id: &str) -> bool {
        COMMANDS.iter().any(|c| c.id == id && c.state == CommandState::Ready)
    }

    /// Run a command and report its outcome in the status bar.
    pub(crate) fn run(&mut self, command: Command) -> Option<Outcome> {
        let id = command.id();
        match self.session.execute(command) {
            Ok(o) => {
                match &o {
                    Outcome::Imported { index, triangles, skipped_facets, format } => {
                        self.selected = BTreeSet::from([*index]);
                        self.mode = Mode::Prepare;
                        let skipped = if *skipped_facets > 0 {
                            format!(", {skipped_facets} damaged facets skipped")
                        } else {
                            String::new()
                        };
                        self.info(format!(
                            "Added a model: {triangles} triangles ({}){skipped}.",
                            format.name()
                        ));
                    }
                    Outcome::Exported { path, bytes } => {
                        self.info(format!("Wrote {} ({} KB).", path.display(), bytes / 1024));
                    }
                    Outcome::ProfileLoaded { name, warnings } => {
                        let w = if warnings.is_empty() {
                            String::new()
                        } else {
                            format!(" {} line(s) skipped: {}", warnings.len(), warnings.join("; "))
                        };
                        self.info(format!("Loaded profile \"{name}\".{w}"));
                    }
                    Outcome::ProfileImported { name, report } => {
                        self.info(format!(
                            "Imported \"{name}\": {} settings applied, {} kept as planned, {} not used.",
                            report.applied.len(),
                            report.placeholders.len(),
                            report.ignored.len()
                        ));
                        self.dialogs.import_report = Some((name.clone(), report.clone()));
                    }
                    Outcome::Saved { path } => self.info(format!("Saved {}.", path.display())),
                    Outcome::Sliced { .. }
                    | Outcome::ProjectOpened { .. }
                    | Outcome::Changed
                    | Outcome::Unchanged => {}
                }
                Some(o)
            }
            Err(e) => {
                self.error(format!("{id}: {e}"));
                None
            }
        }
    }

    pub(crate) fn import_paths(&mut self, paths: Vec<PathBuf>) {
        for p in paths {
            self.run(Command::ImportFile(p));
        }
        if self.prefs.fit_on_import {
            self.refresh_caches();
            self.fit_view();
        }
    }

    pub(crate) fn open_import_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Import models")
            .add_filter("3D models", &["stl", "obj"])
            .add_filter("All files", &["*"])
            .pick_files()
        {
            self.import_paths(paths);
        }
    }

    pub(crate) fn load_profile_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Load profile")
            .add_filter("Profiles (SliceCraft, Simplify3D)", &["scprofile", "fff", "txt"])
            .add_filter("SliceCraft profile", &["scprofile", "txt"])
            .add_filter("Simplify3D profile", &["fff"])
            .pick_file()
            && self.run(Command::LoadProfile(path)).is_some()
        {
            self.dialogs.settings = None;
        }
    }

    pub(crate) fn save_profile_dialog(&mut self) {
        let name = self.session.profile_name().to_string();
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Save profile")
            .add_filter("SliceCraft profile", &["scprofile"])
            .set_file_name(format!("{name}.scprofile"))
            .save_file()
        {
            self.run(Command::SaveProfile { path, name });
        }
    }

    pub(crate) fn open_settings(&mut self) {
        let s = self.session.document().settings.clone();
        self.dialogs.settings = Some(SettingsEditor::new(self.session.profile_name(), s));
    }

    pub(crate) fn apply_profile(&mut self, settings: PrintSettings, name: Option<String>) -> bool {
        let ok = self.run(Command::Profile(Box::new(settings))).is_some();
        if ok {
            if let Some(n) = name {
                self.session.set_profile_name(n);
            }
            self.info("Profile updated; slice again to see the changes.");
        }
        ok
    }

    pub(crate) fn start_slice(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        if let Err(e) = self.session.document().sliceable() {
            self.error(format!("Cannot slice: {e}"));
            return;
        }
        let job = self.session.slice_job();
        let revision = job.revision();
        let progress = Arc::new(Progress::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let mut observer = Observer {
            progress: Arc::clone(&progress),
            cancel: Arc::clone(&cancel),
            ctx: ctx.clone(),
        };
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("slice".into()).spawn(move || {
            let result = job.run(&mut observer);
            let _ = tx.send(result);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => {
                self.job = Some(Job { revision, rx, progress, cancel });
                self.info("Slicing…");
            }
            Err(e) => self.error(format!("Cannot start slicing: {e}")),
        }
    }

    pub(crate) fn cancel_slice(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn poll_job(&mut self) {
        let Some(job) = &self.job else { return };
        let received = match job.rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(EngineError::Internal("the slicing thread stopped without a result"))
            }
        };
        let revision = job.revision;
        self.job = None;
        match received {
            Ok(out) => {
                let layers = out.layers.len();
                let g = out.stats.gcode;
                let warn = out.stats.warnings.clone();
                if self.session.accept_slice(revision, out) {
                    if self.prefs.preview_after_slice {
                        self.mode = Mode::Preview;
                    }
                    self.preview.first_layer = 0;
                    self.preview.last_layer = layers.saturating_sub(1);
                    self.preview.playing = false;
                    let mut msg = format!(
                        "Sliced: {layers} layers, {:.2} m of filament ({:.1} g), about {}.",
                        g.filament_mm / 1000.0,
                        g.filament_grams,
                        format_duration(g.estimated_seconds)
                    );
                    for w in warn {
                        msg.push_str(" Note: ");
                        msg.push_str(&w);
                    }
                    self.info(msg);
                    if let Some(path) = self.pending_export.take() {
                        self.run(Command::ExportGcode(path));
                    }
                } else {
                    self.pending_export = None;
                    self.error("The models changed while slicing; slice again.");
                }
            }
            Err(EngineError::Cancelled) => {
                self.pending_export = None;
                self.info("Slicing cancelled.");
            }
            Err(e) => {
                self.pending_export = None;
                self.error(format!("Slicing failed: {e}"));
            }
        }
    }

    pub(crate) fn export(&mut self, ctx: &egui::Context) {
        if self.session.document().objects.is_empty() {
            self.error("Nothing to export: import a model first.");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export G-code")
            .add_filter("G-code", &["gcode", "gco", "g"])
            .set_file_name("slicecraft.gcode")
            .save_file()
        else {
            return;
        };
        if self.session.slice_result().is_some() {
            self.run(Command::ExportGcode(path));
        } else {
            self.pending_export = Some(path);
            self.start_slice(ctx);
        }
    }

    /// Show a planned command as planned, instead of doing nothing.
    pub(crate) fn planned(&mut self, id: &'static str) {
        if let Some(spec) = COMMANDS.iter().find(|c| c.id == id) {
            self.dialogs.planned = Some(spec);
        }
    }

    pub(crate) fn place(&mut self, index: usize, transform: Transform) {
        self.run(Command::Place { index, transform });
    }

    pub(crate) fn remove_selected(&mut self) {
        // remove from the back so indices stay valid
        let indices: Vec<usize> = self.selected.iter().rev().copied().collect();
        for i in indices {
            self.run(Command::Remove { index: i });
        }
        self.selected.clear();
    }

    pub(crate) fn duplicate_selected(&mut self) {
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for i in indices {
            self.run(Command::Duplicate { index: i });
        }
    }

    pub(crate) fn mirror_selected(&mut self, axis: Axis) {
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for i in indices {
            self.run(Command::Mirror { index: i, axis });
        }
    }

    /// Rotate every selected model by `deg` about one axis.
    pub(crate) fn rotate_selected(&mut self, axis: Axis, deg: f64) {
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for i in indices {
            let Some(mut t) = self.session.document().objects.get(i).map(|o| o.transform) else {
                continue;
            };
            match axis {
                Axis::X => t.rotate.x = (t.rotate.x + deg) % 360.0,
                Axis::Y => t.rotate.y = (t.rotate.y + deg) % 360.0,
                Axis::Z => t.rotate.z = (t.rotate.z + deg) % 360.0,
            }
            self.run(Command::Place { index: i, transform: t });
        }
    }

    /// Multiply the scale of every selected model by `factor` (unit fixes).
    pub(crate) fn scale_selected(&mut self, factor: f64) {
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for i in indices {
            let Some(mut t) = self.session.document().objects.get(i).map(|o| o.transform) else {
                continue;
            };
            t.scale = t.scale.scale(factor);
            if t.is_valid() {
                self.run(Command::Place { index: i, transform: t });
            } else {
                self.error("That scale is outside the allowed range.");
            }
        }
    }

    /// Scale each selected model up (or down) to the largest size that fits the bed.
    pub(crate) fn scale_to_fit_bed(&mut self) {
        let bed = self.session.document().bed.clone();
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for i in indices {
            let Some(o) = self.session.document().objects.get(i) else { continue };
            let size = o.size();
            let mut t = o.transform;
            let fits = [bed.width / size.x, bed.depth / size.y, bed.height / size.z];
            let k = fits
                .iter()
                .copied()
                .filter(|v| v.is_finite() && *v > 0.0)
                .fold(f64::INFINITY, f64::min);
            if k.is_finite() {
                t.scale = t.scale.scale(k * 0.98);
                t.translate = slicecraft_geom::Vec3::new(bed.width / 2.0, bed.depth / 2.0, 0.0);
                if t.is_valid() {
                    self.run(Command::Place { index: i, transform: t });
                }
            }
        }
    }

    /// Point the camera at the selected models.
    pub(crate) fn fit_selection(&mut self) {
        let Some((_, geo)) = &self.geometry else { return };
        let mut b = slicecraft_geom::Bounds3::empty();
        for i in &self.selected {
            if let Some(o) = geo.objects.get(*i)
                && !o.bounds.is_empty()
            {
                b.add(o.bounds.min());
                b.add(o.bounds.max());
            }
        }
        if b.is_empty() {
            self.fit_view();
        } else {
            self.camera.fit(b.min(), b.max());
        }
    }

    pub(crate) fn reset_selected(&mut self) {
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for i in indices {
            let Some(t) = self.session.document().objects.get(i).map(|o| o.transform) else {
                continue;
            };
            let reset = Transform { translate: t.translate, ..Transform::default() };
            self.run(Command::Place { index: i, transform: reset });
        }
    }

    pub(crate) fn center_selected(&mut self) {
        let bed = self.session.document().bed.clone();
        let indices: Vec<usize> = self.selected.iter().copied().collect();
        for i in indices {
            let Some(mut t) = self.session.document().objects.get(i).map(|o| o.transform) else {
                continue;
            };
            t.translate = slicecraft_geom::Vec3::new(bed.width / 2.0, bed.depth / 2.0, 0.0);
            self.run(Command::Place { index: i, transform: t });
        }
    }

    pub(crate) fn select_all(&mut self) {
        self.selected = (0..self.session.document().objects.len()).collect();
    }

    pub(crate) fn fit_view(&mut self) {
        if let Some((_, geo)) = &self.geometry {
            let b = geo.bounds();
            if !b.is_empty() {
                self.camera.fit(b.min(), b.max());
                return;
            }
        }
        let bed = &self.session.document().bed;
        self.camera = OrbitCamera::for_bed(bed.width, bed.depth, bed.height);
    }

    pub(crate) fn view(&mut self, preset: ViewPreset) {
        if preset == ViewPreset::Iso {
            let bed = &self.session.document().bed;
            self.camera = OrbitCamera::for_bed(bed.width, bed.depth, bed.height);
        } else {
            self.camera.look(preset);
        }
    }

    pub(crate) fn refresh_caches(&mut self) {
        let rev = self.session.revision();
        if self.geometry.as_ref().is_none_or(|(r, _)| *r != rev) {
            self.geometry = Some((rev, ModelGeometry::build(self.session.document())));
        }
        match self.session.slice_result() {
            Some(out) => {
                let key = Arc::as_ptr(&out) as usize;
                if self.preview_cache.as_ref().is_none_or(|(k, _)| *k != key) {
                    let layers =
                        scene::build_preview(&out.layers, &self.session.document().settings);
                    self.preview_cache = Some((key, layers));
                }
            }
            None => {
                self.preview_cache = None;
                if self.mode == Mode::Preview {
                    self.mode = Mode::Prepare;
                }
            }
        }
        let count = self.session.document().objects.len();
        self.selected.retain(|i| *i < count);
        let n = self.preview_cache.as_ref().map_or(0, |(_, l)| l.len());
        let max = n.saturating_sub(1);
        self.preview.last_layer = self.preview.last_layer.min(max);
        self.preview.first_layer = self.preview.first_layer.min(self.preview.last_layer);
    }

    fn advance_playback(&mut self, ctx: &egui::Context) {
        if !self.preview.playing || self.mode != Mode::Preview {
            return;
        }
        let Some((_, layers)) = &self.preview_cache else { return };
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        self.preview.play_accum += dt * self.preview.speed;
        let steps = self.preview.play_accum.floor() as usize;
        self.preview.play_accum -= steps as f32;
        let max = layers.len().saturating_sub(1);
        for _ in 0..steps {
            if self.preview.by_line {
                let n = layers.get(self.preview.last_layer).map_or(0, |l| l.segs.len());
                if self.preview.line_cursor < n {
                    self.preview.line_cursor += 1;
                } else if self.preview.last_layer < max {
                    self.preview.last_layer += 1;
                    self.preview.line_cursor = 0;
                } else {
                    self.preview.playing = false;
                }
            } else if self.preview.last_layer < max {
                self.preview.last_layer += 1;
            } else {
                self.preview.playing = false;
            }
        }
        ctx.request_repaint();
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.confirm.is_some() {
            return;
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if cmd && !shift && pressed(Key::O) {
            self.open_import_dialog();
        }
        if cmd && shift && pressed(Key::O) {
            self.request(Pending::OpenProject(None));
        }
        if cmd && pressed(Key::S) {
            self.save_project(shift);
        }
        if cmd && pressed(Key::N) {
            self.request(Pending::NewProject);
        }
        if cmd && pressed(Key::E) {
            self.export(ctx);
        }
        if cmd && pressed(Key::R) {
            self.start_slice(ctx);
        }
        if cmd && pressed(Key::P) {
            self.open_settings();
        }
        if typing || self.dialogs.any_open() {
            return;
        }
        if cmd && pressed(Key::Z) {
            self.run(if shift { Command::Redo } else { Command::Undo });
        }
        if cmd && pressed(Key::Y) {
            self.run(Command::Redo);
        }
        if cmd && pressed(Key::A) {
            self.select_all();
        }
        if cmd && pressed(Key::D) {
            self.duplicate_selected();
        }
        if self.mode == Mode::Prepare && (pressed(Key::Delete) || pressed(Key::Backspace)) {
            self.remove_selected();
        }
        if pressed(Key::Escape) {
            if self.mode == Mode::Preview {
                self.mode = Mode::Prepare;
            } else {
                self.selected.clear();
            }
        }
        if !cmd {
            for (k, tool) in [
                (Key::Q, Tool::Select),
                (Key::W, Tool::Move),
                (Key::E, Tool::Rotate),
                (Key::R, Tool::Scale),
            ] {
                if pressed(k) {
                    self.tool = tool;
                }
            }
            for (k, v) in [
                (Key::Num0, ViewPreset::Iso),
                (Key::Num1, ViewPreset::Front),
                (Key::Num2, ViewPreset::Back),
                (Key::Num3, ViewPreset::Left),
                (Key::Num4, ViewPreset::Right),
                (Key::Num5, ViewPreset::Top),
                (Key::Num6, ViewPreset::Bottom),
            ] {
                if pressed(k) {
                    self.view(v);
                }
            }
            if pressed(Key::F) {
                self.fit_view();
            }
        }
        if self.mode == Mode::Preview {
            let max = self.preview_cache.as_ref().map_or(0, |(_, l)| l.len().saturating_sub(1));
            let p = &mut self.preview;
            if pressed(Key::ArrowUp) || pressed(Key::PageUp) {
                p.last_layer = (p.last_layer + 1).min(max);
            }
            if pressed(Key::ArrowDown) || pressed(Key::PageDown) {
                p.last_layer = p.last_layer.saturating_sub(1);
            }
            if pressed(Key::Home) {
                p.last_layer = 0;
            }
            if pressed(Key::End) {
                p.last_layer = max;
            }
            if pressed(Key::Space) {
                p.playing = !p.playing;
            }
            p.first_layer = p.first_layer.min(p.last_layer);
        }
    }

    fn dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let mut paths = Vec::new();
        for f in dropped {
            if let Some(path) = f.path {
                paths.push(path);
            } else if let Some(bytes) = f.bytes {
                let ext = std::path::Path::new(&f.name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_string);
                self.run(Command::Import {
                    name: f.name.clone(),
                    bytes: bytes.to_vec(),
                    extension: ext,
                });
            }
        }
        if !paths.is_empty() {
            self.open_paths(paths);
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.applied_dark != Some(self.prefs.dark) {
            ctx.set_visuals(if self.prefs.dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            self.applied_dark = Some(self.prefs.dark);
        }
    }
}

impl eframe::App for SliceCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        self.guard_close(&ctx);
        self.update_title(&ctx);
        self.poll_job();
        self.dropped_files(&ctx);
        self.shortcuts(&ctx);
        self.refresh_caches();
        self.advance_playback(&ctx);

        egui::Panel::top("menu").show(ui, |ui| crate::menus::menu_bar(self, ui));
        egui::Panel::top("toolbar").show(ui, |ui| crate::menus::toolbar(self, ui));
        egui::Panel::bottom("status").show(ui, |ui| crate::panels::status_bar(self, ui));
        if self.mode == Mode::Preview {
            egui::Panel::bottom("preview-controls")
                .show(ui, |ui| crate::panels::preview_controls(self, ui));
        }
        egui::Panel::left("left").resizable(true).default_size(270.0).show(ui, |ui| {
            match self.mode {
                Mode::Prepare => crate::panels::models_panel(self, ui),
                Mode::Preview => crate::panels::summary_panel(self, ui),
            }
        });
        if self.mode == Mode::Prepare {
            egui::Panel::right("right").resizable(true).default_size(290.0).show(ui, |ui| {
                crate::panels::transform_panel(self, ui);
            });
        }
        egui::CentralPanel::no_frame().show(ui, |ui| crate::viewport::viewport(self, ui));
        crate::dialogs::show(self, &ctx);
        self.refresh_caches();
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.cancel_slice();
        self.persist();
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase)
}
