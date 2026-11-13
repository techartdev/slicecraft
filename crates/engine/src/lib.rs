#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! Sessions and commands.
//!
//! Every user-visible action is a `Command` with a stable id from `COMMANDS`.
//! The UI, the command line, the CLI and the MCP server all go through
//! `Session::execute`, so nothing is reachable from one front end only.

pub mod pipeline;
pub mod support;

use slicecraft_doc::profile::{MAX_PROFILE_BYTES, Profile};
use slicecraft_doc::{
    Axis, DocError, Document, PrintSettings, SceneObject, SupportEdit, SupportEditKind, Transform,
};
use slicecraft_geom::{GeomError, Vec3};
use slicecraft_io::fff::MAX_FFF_BYTES;
use slicecraft_io::{
    ImportReport, IoError, MeshFormat, import_fff, looks_like_fff, read_mesh, read_project,
    write_project,
};
use slicecraft_render::{Layer, RenderError, layers};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use pipeline::{
    MAX_LAYERS, Quiet, SliceObserver, SliceOutput, SliceStats, Stage, SupportPreviewLayer,
    slice_document, support_preview,
};

/// Largest model file SliceCraft reads, in bytes.
pub const MAX_FILE_BYTES: u64 = 1 << 30;
/// Gap left between models when arranging, mm.
pub const ARRANGE_GAP: f64 = 5.0;
/// Undo steps kept. Documents share meshes, so a step costs little.
pub const MAX_UNDO: usize = 100;

/// One command the engine understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Implemented, or declared and returning `NotImplemented` until its phase lands.
    pub state: CommandState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandState {
    Ready,
    Planned,
}

const fn ready(id: &'static str, label: &'static str) -> CommandSpec {
    CommandSpec { id, label, state: CommandState::Ready }
}

const fn planned(id: &'static str, label: &'static str) -> CommandSpec {
    CommandSpec { id, label, state: CommandState::Planned }
}

/// Every capability, implemented or planned. Front ends build their menus from
/// this list, so a planned feature shows up disabled instead of pretending.
/// The inventory behind the planned ones is `docs/research/02-feature-inventory.md`.
pub const COMMANDS: &[CommandSpec] = &[
    // models
    ready("import", "Import Models"),
    ready("place", "Place Model"),
    ready("remove", "Remove Model"),
    ready("duplicate", "Duplicate Model"),
    ready("mirror", "Mirror Model"),
    ready("visibility", "Show or Hide Model"),
    ready("arrange", "Arrange Models"),
    ready("undo", "Undo"),
    ready("redo", "Redo"),
    ready("lay-flat", "Lay Face Flat"),
    ready("split-parts", "Split into Parts"),
    ready("merge-models", "Merge Models"),
    planned("group-models", "Group Models"),
    planned("model-role", "Use Model as Support or Cutter"),
    ready("export-models", "Export Models (STL, OBJ, 3MF)"),
    // projects and profiles
    ready("profile", "Print Settings"),
    ready("load-profile", "Load Profile"),
    ready("save-profile", "Save Profile"),
    ready("new-project", "New Project"),
    ready("open-project", "Open Project"),
    ready("save-project", "Save Project"),
    ready("multi-profile", "Several Profiles per Job"),
    ready("setting-zones", "Height and Model Setting Zones"),
    planned("profile-wizard", "Printer Setup Assistant"),
    // mesh tools
    ready("repair-mesh", "Repair Mesh"),
    ready("analyze-mesh", "Analyse Mesh"),
    ready("fix-normals", "Fix Inverted Faces"),
    planned("find-intersections", "Find Self-Intersections"),
    planned("reduce-mesh", "Reduce Triangle Count"),
    ready("fill-holes", "Fill Holes"),
    planned("hollow", "Hollow Model"),
    planned("cut-plane", "Cut with a Plane"),
    // support
    ready("supports", "Generate Supports"),
    ready("support-edit", "Add or Remove Support by Hand"),
    ready("support-clear", "Clear Support Edits"),
    ready("support-blockers", "Support Blockers"),
    ready("support-preview", "Show Support Before Slicing"),
    // slicing and output
    ready("preview", "Layer Preview"),
    ready("slice", "Slice"),
    ready("export-gcode", "Export G-code"),
    ready("gcode-preview", "Toolpath Preview"),
    ready("open-gcode", "Open G-code for Preview"),
    planned("variable-layers", "Variable Layer Heights"),
    // machine
    ready("machine-control", "Machine Control Panel"),
    ready("print-usb", "Print over USB"),
    planned("print-network", "Send to a Network Printer"),
    ready("firmware-terminal", "Firmware Terminal"),
    planned("camera-monitor", "Print Camera"),
];

/// A request to the engine. `Command::id` names its `COMMANDS` entry.
#[derive(Debug, Clone)]
pub enum Command {
    /// Add a model from bytes; `extension` picks the format (`stl`, `obj`).
    Import {
        name: String,
        bytes: Vec<u8>,
        extension: Option<String>,
    },
    /// Add a model from a file on disk.
    ImportFile(PathBuf),
    /// Set a model's placement. Z is kept on the bed.
    Place {
        index: usize,
        transform: Transform,
    },
    Remove {
        index: usize,
    },
    /// Copy a model; the copy is placed beside the others.
    Duplicate {
        index: usize,
    },
    /// Mirror a model's mesh across one of its own axes.
    Mirror {
        index: usize,
        axis: Axis,
    },
    /// Weld, drop degenerate faces, fix the winding and (with `fill`) close holes.
    RepairMesh {
        index: usize,
        fill: bool,
    },
    /// Split a model into its connected parts, each its own model.
    SplitParts {
        index: usize,
    },
    /// Combine models into one, as they are placed.
    MergeModels {
        indices: Vec<usize>,
    },
    /// Turn a model so its largest flat face lies on the bed.
    LayFlat {
        index: usize,
    },
    /// Include or leave out a model when slicing.
    SetVisible {
        index: usize,
        visible: bool,
    },
    Arrange,
    Undo,
    Redo,
    /// Replace the print profile after validating it.
    Profile(Box<PrintSettings>),
    /// Read a profile file and make it the current profile: a SliceCraft
    /// `.scprofile` or a Simplify3D `.fff`, recognised by content.
    LoadProfile(PathBuf),
    /// Write the current profile to a file under `name`.
    SaveProfile {
        path: PathBuf,
        name: String,
    },
    /// Start an empty project, keeping the current profile. Clears undo.
    NewProject,
    /// Replace the document with a project file. Clears undo.
    OpenProject(PathBuf),
    /// Write models, placement and profile to a project file.
    SaveProject(PathBuf),
    /// Turn automatic support on or off in the current profile.
    Supports(bool),
    /// Add a setting zone (settings for a height range and/or some models).
    AddZone(Box<slicecraft_doc::SettingZone>),
    /// Replace a setting zone.
    UpdateZone {
        index: usize,
        zone: Box<slicecraft_doc::SettingZone>,
    },
    /// Remove a setting zone.
    RemoveZone {
        index: usize,
    },
    /// Add a box where support is added or blocked.
    AddSupportEdit(SupportEdit),
    /// Remove one support edit.
    RemoveSupportEdit {
        index: usize,
    },
    /// Remove every support edit.
    ClearSupportEdits,
    /// Work out the support a slice would print, without slicing.
    PreviewSupport,
    /// Slice now, on the calling thread.
    Slice,
    /// Write G-code to a file, slicing first if the slice is stale.
    ExportGcode(PathBuf),
    /// Write the visible models, as placed, to an STL, OBJ or 3MF file.
    ExportModels(PathBuf),
    /// Read a G-code file (from any slicer) and show it as the slice.
    OpenGcode(PathBuf),
    /// Open a serial connection to a printer.
    ConnectPrinter {
        port: String,
        baud: u32,
    },
    DisconnectPrinter,
    /// Send one line to the printer (terminal, jogging, temperatures).
    SendToPrinter(String),
    /// Print the current slice (slicing first if needed) over the connection.
    PrintOverUsb,
    PausePrint,
    ResumePrint,
    CancelPrint,
}

impl Command {
    pub fn id(&self) -> &'static str {
        match self {
            Command::Import { .. } | Command::ImportFile(_) => "import",
            Command::Place { .. } => "place",
            Command::Remove { .. } => "remove",
            Command::Duplicate { .. } => "duplicate",
            Command::Mirror { .. } => "mirror",
            Command::SetVisible { .. } => "visibility",
            Command::RepairMesh { fill: true, .. } => "repair-mesh",
            Command::RepairMesh { fill: false, .. } => "fix-normals",
            Command::SplitParts { .. } => "split-parts",
            Command::MergeModels { .. } => "merge-models",
            Command::LayFlat { .. } => "lay-flat",
            Command::Arrange => "arrange",
            Command::Undo => "undo",
            Command::Redo => "redo",
            Command::Profile(_) => "profile",
            Command::LoadProfile(_) => "load-profile",
            Command::SaveProfile { .. } => "save-profile",
            Command::NewProject => "new-project",
            Command::OpenProject(_) => "open-project",
            Command::SaveProject(_) => "save-project",
            Command::Supports(_) => "supports",
            Command::AddSupportEdit(e) => match e.kind {
                SupportEditKind::Add => "support-edit",
                SupportEditKind::Block => "support-blockers",
            },
            Command::RemoveSupportEdit { .. } => "support-edit",
            Command::AddZone(z) | Command::UpdateZone { zone: z, .. } => {
                if z.objects.is_empty() {
                    "setting-zones"
                } else {
                    "multi-profile"
                }
            }
            Command::RemoveZone { .. } => "setting-zones",
            Command::ClearSupportEdits => "support-clear",
            Command::PreviewSupport => "support-preview",
            Command::Slice => "slice",
            Command::ExportGcode(_) => "export-gcode",
            Command::ExportModels(_) => "export-models",
            Command::OpenGcode(_) => "open-gcode",
            Command::ConnectPrinter { .. } | Command::DisconnectPrinter => "machine-control",
            Command::SendToPrinter(_) => "firmware-terminal",
            Command::PrintOverUsb
            | Command::PausePrint
            | Command::ResumePrint
            | Command::CancelPrint => "print-usb",
        }
    }
}

/// What a command did.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Imported {
        index: usize,
        triangles: usize,
        skipped_facets: usize,
        format: MeshFormat,
    },
    Changed,
    /// Undo or redo had nothing to do.
    Unchanged,
    ProfileLoaded {
        name: String,
        warnings: Vec<String>,
    },
    /// A profile from another slicer, converted; the report says how.
    ProfileImported {
        name: String,
        report: ImportReport,
    },
    Saved {
        path: PathBuf,
    },
    ProjectOpened {
        path: PathBuf,
        objects: usize,
        warnings: Vec<String>,
    },
    /// What a repair changed.
    Repaired {
        index: usize,
        report: slicecraft_geom::repair::RepairReport,
    },
    PrinterConnected {
        port: String,
    },
    PrintStarted {
        lines: usize,
    },
    /// A G-code file read for preview.
    GcodeOpened {
        path: PathBuf,
        layers: usize,
    },
    /// The support a slice would print.
    SupportPreviewed {
        layers: usize,
        /// Support volume, mm³ (area × layer height).
        volume: f64,
    },
    Sliced {
        layers: usize,
        stats: SliceStats,
    },
    Exported {
        path: PathBuf,
        bytes: usize,
    },
}

/// A self-contained slicing job, so the work can leave the UI thread.
#[derive(Debug, Clone)]
pub struct SliceJob {
    doc: Document,
    revision: u64,
}

impl SliceJob {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn run(&self, observer: &mut dyn SliceObserver) -> Result<SliceOutput, EngineError> {
        slice_document(&self.doc, observer)
    }
}

/// A self-contained support preview, so it can run off the UI thread.
#[derive(Debug, Clone)]
pub struct SupportJob {
    doc: Document,
    revision: u64,
}

impl SupportJob {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn run(
        &self,
        observer: &mut dyn SliceObserver,
    ) -> Result<Vec<SupportPreviewLayer>, EngineError> {
        support_preview(&self.doc, observer)
    }
}

/// What a session holds: the document, its revision, undo history and its
/// latest slice.
#[derive(Debug, Default)]
pub struct Session {
    doc: Document,
    revision: u64,
    sliced: Option<(u64, Arc<SliceOutput>)>,
    undo: Vec<Document>,
    redo: Vec<Document>,
    /// The last undoable edit, so a run of edits to one thing (dragging a
    /// model, typing in a field) becomes one undo step.
    last_edit: Option<(&'static str, usize)>,
    profile_name: String,
    /// The project file this document was last opened from or saved to.
    project_path: Option<PathBuf>,
    /// Revision at the last project save or open; differs when there are
    /// unsaved changes.
    saved_revision: u64,
    /// The printer connection, when there is one.
    printer: Option<slicecraft_io::printer::Connection>,
    /// A person drives this session (the window): the profile's
    /// post-processing program may run. Never set for agents or scripts.
    interactive: bool,
}

impl Session {
    pub fn new() -> Self {
        Self { profile_name: "Standard 0.20 mm".into(), ..Self::default() }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Direct document access for tests and tools; bumps the revision, so any
    /// slice taken before is stale. Not recorded for undo.
    pub fn document_mut(&mut self) -> &mut Document {
        self.revision += 1;
        &mut self.doc
    }

    /// Increases on every change to the document.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn command(&self, id: &str) -> Option<&'static CommandSpec> {
        COMMANDS.iter().find(|c| c.id == id)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Name of the current profile (from a preset or a loaded file).
    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }

    pub fn set_profile_name(&mut self, name: impl Into<String>) {
        self.profile_name = name.into();
    }

    /// Install a profile without an undo step or an unsaved-changes mark, as
    /// when restoring the last session's profile at start-up.
    pub fn restore_profile(&mut self, profile: Profile) -> Result<(), EngineError> {
        profile.settings.validate()?;
        let clean = !self.is_modified();
        self.doc.set_settings(profile.settings);
        self.profile_name = profile.name;
        self.revision += 1;
        if clean {
            self.saved_revision = self.revision;
        }
        Ok(())
    }

    /// The current profile with its name.
    pub fn profile(&self) -> Profile {
        Profile { name: self.profile_name.clone(), settings: self.doc.settings.clone() }
    }

    /// Mark this session as driven by a person, so post-processing runs.
    pub fn set_interactive(&mut self, on: bool) {
        self.interactive = on;
    }

    /// The connected printer's port, if any.
    pub fn printer_port(&self) -> Option<&str> {
        self.printer.as_ref().map(|c| c.port.as_str())
    }

    /// Everything the printer said since the last call. A dropped link
    /// closes the connection.
    pub fn printer_events(&mut self) -> Vec<slicecraft_io::printer::HostEvent> {
        let Some(c) = &self.printer else { return Vec::new() };
        let events = c.events();
        if events.contains(&slicecraft_io::printer::HostEvent::Disconnected) {
            self.printer = None;
        }
        events
    }

    /// Use an already running connection (tests, other transports).
    pub fn attach_printer(&mut self, c: slicecraft_io::printer::Connection) {
        self.printer = Some(c);
    }

    fn printer_send(&self, cmd: slicecraft_io::printer::HostCommand) -> Result<(), EngineError> {
        match &self.printer {
            Some(c) if c.send(cmd) => Ok(()),
            _ => Err(EngineError::File("no printer is connected".into())),
        }
    }

    /// The project file of this document, once opened or saved.
    pub fn project_path(&self) -> Option<&Path> {
        self.project_path.as_deref()
    }

    /// True when the document changed since it was last opened or saved.
    pub fn is_modified(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// The slice of the current document, if one has been taken since the last change.
    pub fn slice_result(&self) -> Option<Arc<SliceOutput>> {
        match &self.sliced {
            Some((rev, out)) if *rev == self.revision => Some(Arc::clone(out)),
            _ => None,
        }
    }

    /// A job that previews the support of the document as it is now.
    pub fn support_job(&self) -> SupportJob {
        SupportJob { doc: self.doc.clone(), revision: self.revision }
    }

    /// A job that slices the document as it is now.
    pub fn slice_job(&self) -> SliceJob {
        SliceJob { doc: self.doc.clone(), revision: self.revision }
    }

    /// Install a finished job's output. Refused (false) when the document has
    /// changed since the job was taken.
    pub fn accept_slice(&mut self, job_revision: u64, output: SliceOutput) -> bool {
        if job_revision != self.revision {
            return false;
        }
        self.sliced = Some((job_revision, Arc::new(output)));
        true
    }

    /// Cross sections of every visible model as it sits on the bed.
    pub fn preview_layers(&self) -> Vec<Layer> {
        let Ok(meshes) = pipeline::bed_meshes(&self.doc) else { return Vec::new() };
        let mut out = Vec::new();
        for mesh in &meshes {
            out.extend(layers(mesh, self.doc.settings.layer_height, MAX_LAYERS));
        }
        out
    }

    /// Remember the document before an edit. Edits with the same `key` in a
    /// row share one undo step.
    fn checkpoint(&mut self, key: Option<(&'static str, usize)>) {
        if key.is_none() || key != self.last_edit {
            self.undo.push(self.doc.clone());
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_edit = key;
    }

    fn changed(&mut self) -> Outcome {
        self.revision += 1;
        Outcome::Changed
    }

    fn object_index(&self, index: usize) -> Result<usize, EngineError> {
        if index < self.doc.objects.len() {
            Ok(index)
        } else {
            Err(EngineError::Doc(DocError::NoSuchObject(index)))
        }
    }

    /// Run one command.
    pub fn execute(&mut self, command: Command) -> Result<Outcome, EngineError> {
        if !matches!(
            command,
            Command::Place { .. } | Command::Profile(_) | Command::UpdateZone { .. }
        ) {
            // anything else ends a run of merged edits
            self.last_edit = None;
        }
        match command {
            Command::Import { name, bytes, extension } => {
                self.import(name, &bytes, extension.as_deref())
            }
            Command::ImportFile(path) => {
                let bytes = read_bounded(&path, MAX_FILE_BYTES)?;
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model").to_string();
                let ext = path.extension().and_then(|e| e.to_str()).map(str::to_string);
                self.import(name, &bytes, ext.as_deref())
            }
            Command::Place { index, mut transform } => {
                transform.translate.z = 0.0;
                if !transform.is_valid() {
                    return Err(EngineError::Doc(DocError::BadTransform));
                }
                let index = self.object_index(index)?;
                if self.doc.objects.get(index).is_some_and(|o| o.transform == transform) {
                    return Ok(Outcome::Unchanged);
                }
                self.checkpoint(Some(("place", index)));
                if let Some(o) = self.doc.objects.get_mut(index) {
                    o.transform = transform;
                }
                Ok(self.changed())
            }
            Command::Remove { index } => {
                let index = self.object_index(index)?;
                self.checkpoint(None);
                self.doc.objects.remove(index);
                self.doc.forget_object(index);
                Ok(self.changed())
            }
            Command::Duplicate { index } => {
                let index = self.object_index(index)?;
                let Some(mut copy) = self.doc.objects.get(index).cloned() else {
                    return Err(EngineError::Doc(DocError::NoSuchObject(index)));
                };
                copy.name = format!("{} copy", copy.name);
                self.checkpoint(None);
                let new = self.doc.add(copy)?;
                self.doc.arrange(ARRANGE_GAP)?;
                self.changed();
                Ok(Outcome::Imported {
                    index: new,
                    triangles: self.doc.objects.get(new).map_or(0, |o| o.mesh.len()),
                    skipped_facets: 0,
                    format: MeshFormat::StlBinary,
                })
            }
            Command::Mirror { index, axis } => {
                let index = self.object_index(index)?;
                self.checkpoint(None);
                if let Some(o) = self.doc.objects.get_mut(index) {
                    o.mirror(axis)?;
                }
                Ok(self.changed())
            }
            Command::RepairMesh { index, fill } => {
                let index = self.object_index(index)?;
                let Some(o) = self.doc.objects.get(index) else {
                    return Err(EngineError::Doc(DocError::NoSuchObject(index)));
                };
                let (mesh, report) = slicecraft_geom::repair::repair(&o.mesh, fill)?;
                let replaced = with_mesh(o, o.name.clone(), mesh);
                self.checkpoint(None);
                if let Some(slot) = self.doc.objects.get_mut(index) {
                    *slot = replaced;
                }
                self.changed();
                Ok(Outcome::Repaired { index, report })
            }
            Command::SplitParts { index } => {
                let index = self.object_index(index)?;
                let Some(o) = self.doc.objects.get(index).cloned() else {
                    return Err(EngineError::Doc(DocError::NoSuchObject(index)));
                };
                let parts = slicecraft_geom::repair::parts(&o.mesh)?;
                if parts.len() < 2 {
                    return Ok(Outcome::Unchanged);
                }
                if self.doc.objects.len() + parts.len() > slicecraft_doc::MAX_OBJECTS {
                    return Err(EngineError::Doc(DocError::TooManyObjects));
                }
                self.checkpoint(None);
                self.doc.objects.remove(index);
                self.doc.forget_object(index);
                for (k, mesh) in parts.into_iter().enumerate() {
                    // each part keeps its place: the anchor moves with its own box
                    let mut part = with_mesh(&o, format!("{} part {}", o.name, k + 1), mesh);
                    let shift = o.transform.apply_linear(part.anchor() - o.anchor());
                    part.transform.translate = Vec3::new(
                        o.transform.translate.x + shift.x,
                        o.transform.translate.y + shift.y,
                        0.0,
                    );
                    self.doc.add(part)?;
                }
                Ok(self.changed())
            }
            Command::MergeModels { indices } => {
                let mut seen = std::collections::BTreeSet::new();
                let mut tris = Vec::new();
                let mut names = Vec::new();
                for &i in &indices {
                    let i = self.object_index(i)?;
                    if !seen.insert(i) {
                        continue;
                    }
                    if let Some(o) = self.doc.objects.get(i) {
                        tris.extend_from_slice(o.world_mesh()?.triangles());
                        names.push(o.name.clone());
                    }
                }
                if seen.len() < 2 {
                    return Ok(Outcome::Unchanged);
                }
                let mesh = slicecraft_geom::Mesh::new(tris)?;
                let centre = mesh.bounds().center();
                let mut merged = SceneObject::new(names.join(" + "), mesh);
                merged.transform.translate = Vec3::new(centre.x, centre.y, 0.0);
                self.checkpoint(None);
                for i in seen.iter().rev() {
                    self.doc.objects.remove(*i);
                    self.doc.forget_object(*i);
                }
                let index = self.doc.add(merged)?;
                self.changed();
                Ok(Outcome::Imported {
                    index,
                    triangles: self.doc.objects.get(index).map_or(0, |o| o.mesh.len()),
                    skipped_facets: 0,
                    format: MeshFormat::StlBinary,
                })
            }
            Command::LayFlat { index } => {
                let index = self.object_index(index)?;
                let Some(o) = self.doc.objects.get(index).cloned() else {
                    return Err(EngineError::Doc(DocError::NoSuchObject(index)));
                };
                // the face is found on the model as it is turned and scaled now
                let t = o.transform;
                let shaped = o.mesh.map_vertices(|v| t.apply_linear(v))?;
                let Some(n) = slicecraft_geom::repair::largest_flat_face(&shaped) else {
                    return Ok(Outcome::Unchanged);
                };
                let flat = slicecraft_geom::repair::rotate_to_down(&shaped, n)?;
                let mut laid = with_mesh(&o, o.name.clone(), flat);
                laid.transform =
                    slicecraft_doc::Transform { translate: t.translate, ..Default::default() };
                self.checkpoint(None);
                if let Some(slot) = self.doc.objects.get_mut(index) {
                    *slot = laid;
                }
                Ok(self.changed())
            }
            Command::SetVisible { index, visible } => {
                let index = self.object_index(index)?;
                if self.doc.objects.get(index).is_some_and(|o| o.visible == visible) {
                    return Ok(Outcome::Unchanged);
                }
                self.checkpoint(None);
                if let Some(o) = self.doc.objects.get_mut(index) {
                    o.visible = visible;
                }
                Ok(self.changed())
            }
            Command::Arrange => {
                self.checkpoint(None);
                self.doc.arrange(ARRANGE_GAP)?;
                Ok(self.changed())
            }
            Command::Undo => match self.undo.pop() {
                Some(prev) => {
                    self.redo.push(std::mem::replace(&mut self.doc, prev));
                    Ok(self.changed())
                }
                None => Ok(Outcome::Unchanged),
            },
            Command::Redo => match self.redo.pop() {
                Some(next) => {
                    self.undo.push(std::mem::replace(&mut self.doc, next));
                    Ok(self.changed())
                }
                None => Ok(Outcome::Unchanged),
            },
            Command::Profile(settings) => {
                settings.validate()?;
                if *settings == self.doc.settings {
                    return Ok(Outcome::Unchanged);
                }
                self.checkpoint(Some(("profile", 0)));
                self.doc.set_settings(*settings);
                Ok(self.changed())
            }
            Command::LoadProfile(path) => {
                let limit = MAX_PROFILE_BYTES.max(MAX_FFF_BYTES) as u64;
                let bytes = read_bounded(&path, limit)?;
                let text =
                    String::from_utf8(bytes).map_err(|_| EngineError::Io(IoError::NotUtf8))?;
                if looks_like_fff(&text) {
                    let (profile, report) = import_fff(&text)?;
                    profile.settings.validate()?;
                    self.checkpoint(None);
                    self.doc.set_settings(profile.settings);
                    self.profile_name = profile.name.clone();
                    self.changed();
                    return Ok(Outcome::ProfileImported { name: profile.name, report });
                }
                let (profile, warnings) = Profile::from_text(&text)?;
                profile.settings.validate()?;
                self.checkpoint(None);
                self.doc.set_settings(profile.settings);
                self.profile_name = profile.name.clone();
                self.changed();
                Ok(Outcome::ProfileLoaded { name: profile.name, warnings })
            }
            Command::SaveProfile { path, name } => {
                let text =
                    Profile { name: name.clone(), settings: self.doc.settings.clone() }.to_text();
                std::fs::write(&path, text).map_err(|e| {
                    EngineError::File(format!("cannot write {}: {e}", path.display()))
                })?;
                self.profile_name = name;
                Ok(Outcome::Saved { path })
            }
            Command::NewProject => {
                let settings = self.doc.settings.clone();
                self.doc = Document::default();
                self.doc.set_settings(settings);
                self.reset_history(None);
                Ok(Outcome::Changed)
            }
            Command::OpenProject(path) => {
                let bytes = read_bounded(&path, MAX_FILE_BYTES)?;
                let text =
                    String::from_utf8(bytes).map_err(|_| EngineError::Io(IoError::NotUtf8))?;
                let project = read_project(&text)?;
                project.profile.settings.validate()?;
                let mut doc = Document::default();
                doc.set_settings(project.profile.settings);
                let objects = project.objects.len();
                for o in project.objects {
                    doc.add(o)?;
                }
                for e in project.support_edits {
                    doc.add_support_edit(e)?;
                }
                doc.zones = project.zones;
                self.doc = doc;
                self.profile_name = project.profile.name;
                self.reset_history(Some(path.clone()));
                Ok(Outcome::ProjectOpened { path, objects, warnings: project.warnings })
            }
            Command::SaveProject(path) => {
                let text = write_project(
                    &self.profile(),
                    &self.doc.objects,
                    &self.doc.support_edits,
                    &self.doc.zones,
                );
                std::fs::write(&path, text).map_err(|e| {
                    EngineError::File(format!("cannot write {}: {e}", path.display()))
                })?;
                self.project_path = Some(path.clone());
                self.saved_revision = self.revision;
                Ok(Outcome::Saved { path })
            }
            Command::Supports(on) => {
                if self.doc.settings.support_enabled == on {
                    return Ok(Outcome::Unchanged);
                }
                self.checkpoint(None);
                let mut settings = self.doc.settings.clone();
                settings.support_enabled = on;
                self.doc.set_settings(settings);
                Ok(self.changed())
            }
            Command::AddZone(zone) => {
                self.check_zone(&zone)?;
                if self.doc.zones.len() >= slicecraft_doc::MAX_ZONES {
                    return Err(EngineError::Doc(DocError::Setting("too many setting zones")));
                }
                self.checkpoint(None);
                self.doc.zones.push(*zone);
                Ok(self.changed())
            }
            Command::UpdateZone { index, zone } => {
                self.check_zone(&zone)?;
                if index >= self.doc.zones.len() {
                    return Err(EngineError::Doc(DocError::Setting("there is no such zone")));
                }
                if self.doc.zones.get(index) == Some(&*zone) {
                    return Ok(Outcome::Unchanged);
                }
                self.checkpoint(Some(("zone", index)));
                if let Some(slot) = self.doc.zones.get_mut(index) {
                    *slot = *zone;
                }
                Ok(self.changed())
            }
            Command::RemoveZone { index } => {
                if index >= self.doc.zones.len() {
                    return Err(EngineError::Doc(DocError::Setting("there is no such zone")));
                }
                self.checkpoint(None);
                self.doc.zones.remove(index);
                Ok(self.changed())
            }
            Command::AddSupportEdit(e) => {
                if !e.is_valid() {
                    return Err(EngineError::Doc(DocError::Setting(
                        "a support box must be finite and at least 0.1 mm wide",
                    )));
                }
                self.checkpoint(None);
                self.doc.add_support_edit(e)?;
                Ok(self.changed())
            }
            Command::RemoveSupportEdit { index } => {
                if index >= self.doc.support_edits.len() {
                    return Err(EngineError::Doc(DocError::Setting(
                        "there is no such support edit",
                    )));
                }
                self.checkpoint(None);
                self.doc.support_edits.remove(index);
                Ok(self.changed())
            }
            Command::ClearSupportEdits => {
                if self.doc.support_edits.is_empty() {
                    return Ok(Outcome::Unchanged);
                }
                self.checkpoint(None);
                self.doc.support_edits.clear();
                Ok(self.changed())
            }
            Command::PreviewSupport => {
                let layers = support_preview(&self.doc, &mut Quiet)?;
                let volume = layers.iter().map(|l| l.area.area() * l.slot.height).sum();
                Ok(Outcome::SupportPreviewed { layers: layers.len(), volume })
            }
            Command::Slice => {
                let out = self.slice_now()?;
                Ok(Outcome::Sliced { layers: out.layers.len(), stats: out.stats.clone() })
            }
            Command::ConnectPrinter { port, baud } => {
                let c = slicecraft_io::printer::connect(&port, baud).map_err(EngineError::File)?;
                self.printer = Some(c);
                Ok(Outcome::PrinterConnected { port })
            }
            Command::DisconnectPrinter => {
                if self.printer.take().is_none() {
                    return Ok(Outcome::Unchanged);
                }
                Ok(Outcome::Changed)
            }
            Command::SendToPrinter(line) => {
                let line = line.trim().to_string();
                if line.is_empty() || line.len() > 256 {
                    return Err(EngineError::File(
                        "a printer command is one line, up to 256 characters".into(),
                    ));
                }
                self.printer_send(slicecraft_io::printer::HostCommand::Send(line))?;
                Ok(Outcome::Changed)
            }
            Command::PrintOverUsb => {
                if self.printer.is_none() {
                    return Err(EngineError::File("no printer is connected".into()));
                }
                let out = match self.slice_result() {
                    Some(out) => out,
                    None => self.slice_now()?,
                };
                let lines = slicecraft_io::printer::printable_lines(&out.gcode);
                let count = lines.len();
                self.printer_send(slicecraft_io::printer::HostCommand::Print(lines))?;
                Ok(Outcome::PrintStarted { lines: count })
            }
            Command::PausePrint => {
                self.printer_send(slicecraft_io::printer::HostCommand::Pause)?;
                Ok(Outcome::Changed)
            }
            Command::ResumePrint => {
                self.printer_send(slicecraft_io::printer::HostCommand::Resume)?;
                Ok(Outcome::Changed)
            }
            Command::CancelPrint => {
                self.printer_send(slicecraft_io::printer::HostCommand::Cancel)?;
                Ok(Outcome::Changed)
            }
            Command::OpenGcode(path) => {
                let bytes = read_bounded(&path, MAX_FILE_BYTES)?;
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let mut read = slicecraft_io::gcode_in::read_gcode(&text)?;
                let layers = read.layers.len();
                // weight and cost at the profile's filament
                let s = &self.doc.settings;
                let area = std::f64::consts::PI * s.filament_diameter.powi(2) / 4.0;
                read.stats.filament_grams =
                    read.stats.filament_mm * area / 1000.0 * s.filament_density;
                read.stats.filament_cost = read.stats.filament_grams / 1000.0 * s.filament_cost;
                let out = SliceOutput {
                    layers: read.layers,
                    gcode: text,
                    stats: SliceStats {
                        gcode: read.stats,
                        dropped_chains: 0,
                        warnings: Vec::new(),
                    },
                };
                // shown until the document changes
                self.sliced = Some((self.revision, Arc::new(out)));
                Ok(Outcome::GcodeOpened { path, layers })
            }
            Command::ExportModels(path) => {
                let mut models = Vec::new();
                for o in self.doc.objects.iter().filter(|o| o.visible) {
                    models.push((o.name.clone(), o.world_mesh()?));
                }
                if models.is_empty() {
                    return Err(EngineError::NothingToSlice);
                }
                let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
                let bytes = match ext.as_deref() {
                    Some("obj") => slicecraft_io::threemf::write_obj(&models),
                    Some("3mf") => slicecraft_io::threemf::write_3mf(&models),
                    _ => slicecraft_io::threemf::write_stl(&models),
                };
                std::fs::write(&path, &bytes).map_err(|e| {
                    EngineError::File(format!("cannot write {}: {e}", path.display()))
                })?;
                Ok(Outcome::Exported { path, bytes: bytes.len() })
            }
            Command::ExportGcode(path) => {
                let out = match self.slice_result() {
                    Some(out) => out,
                    None => self.slice_now()?,
                };
                std::fs::write(&path, out.gcode.as_bytes()).map_err(|e| {
                    EngineError::File(format!("cannot write {}: {e}", path.display()))
                })?;
                let program = self.doc.settings.post_process.trim().to_string();
                if self.interactive && !program.is_empty() {
                    post_process(&program, &path)?;
                }
                Ok(Outcome::Exported { path, bytes: out.gcode.len() })
            }
        }
    }

    fn check_zone(&self, zone: &slicecraft_doc::SettingZone) -> Result<(), EngineError> {
        zone.validate()?;
        if zone.objects.iter().any(|&i| i >= self.doc.objects.len()) {
            return Err(EngineError::Doc(DocError::NoSuchObject(
                zone.objects.iter().copied().max().unwrap_or(0),
            )));
        }
        Ok(())
    }

    /// A new or opened document: no undo history, nothing unsaved.
    fn reset_history(&mut self, path: Option<PathBuf>) {
        self.undo.clear();
        self.redo.clear();
        self.last_edit = None;
        self.revision += 1;
        self.saved_revision = self.revision;
        self.project_path = path;
    }

    fn slice_now(&mut self) -> Result<Arc<SliceOutput>, EngineError> {
        let out = Arc::new(slice_document(&self.doc, &mut Quiet)?);
        self.sliced = Some((self.revision, Arc::clone(&out)));
        Ok(out)
    }

    fn import(
        &mut self,
        name: String,
        bytes: &[u8],
        extension: Option<&str>,
    ) -> Result<Outcome, EngineError> {
        // a 3MF package can hold several models: each becomes its own
        if extension.is_some_and(|e| e.eq_ignore_ascii_case("3mf")) {
            let models = slicecraft_io::threemf::read_3mf(bytes)?;
            self.checkpoint(None);
            let mut last = 0;
            let mut triangles = 0;
            let several = models.len() > 1;
            for (model_name, mesh) in models {
                triangles += mesh.len();
                let n = if several { format!("{name} - {model_name}") } else { name.clone() };
                last = self.doc.add(SceneObject::new(n, mesh))?;
            }
            if self.doc.objects.len() == 1 {
                self.doc.center_object(last)?;
            } else {
                self.doc.arrange(ARRANGE_GAP)?;
            }
            self.revision += 1;
            return Ok(Outcome::Imported {
                index: last,
                triangles,
                skipped_facets: 0,
                format: MeshFormat::ThreeMf,
            });
        }
        let imported = read_mesh(bytes, extension)?;
        if imported.mesh.is_empty() {
            return Err(EngineError::Io(IoError::UnknownFormat));
        }
        let triangles = imported.mesh.len();
        self.checkpoint(None);
        let index = self.doc.add(SceneObject::new(name, imported.mesh))?;
        if self.doc.objects.len() == 1 {
            self.doc.center_object(index)?;
        } else {
            self.doc.arrange(ARRANGE_GAP)?;
        }
        self.revision += 1;
        Ok(Outcome::Imported {
            index,
            triangles,
            skipped_facets: imported.skipped_facets,
            format: imported.format,
        })
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, EngineError> {
    let meta = std::fs::metadata(path)
        .map_err(|e| EngineError::File(format!("cannot read {}: {e}", path.display())))?;
    if meta.len() > limit {
        return Err(EngineError::Io(IoError::TooLarge));
    }
    std::fs::read(path)
        .map_err(|e| EngineError::File(format!("cannot read {}: {e}", path.display())))
}

/// Run the profile's post-processing program on a written file: the first
/// word is the program, the rest its arguments, with `{file}` replaced by the
/// path (or the path added last when no argument names it).
fn post_process(program: &str, path: &Path) -> Result<(), EngineError> {
    let mut words = program.split_whitespace();
    let Some(exe) = words.next() else { return Ok(()) };
    let file = path.display().to_string();
    let mut args: Vec<String> = words.map(|w| w.replace("{file}", &file)).collect();
    if !program.contains("{file}") {
        args.push(file);
    }
    let status = std::process::Command::new(exe)
        .args(&args)
        .status()
        .map_err(|e| EngineError::File(format!("post-processing could not start {exe}: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(EngineError::File(format!("post-processing ({exe}) failed: {status}")))
    }
}

/// A copy of `o` with another mesh (and name): same placement and visibility.
fn with_mesh(o: &SceneObject, name: String, mesh: slicecraft_geom::Mesh) -> SceneObject {
    let mut out = SceneObject::new(name, mesh);
    out.transform = o.transform;
    out.visible = o.visible;
    out
}

/// A world-space vector helper for front ends that move models.
pub fn bed_point(x: f64, y: f64) -> Vec3 {
    Vec3::new(x, y, 0.0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    Doc(DocError),
    Io(IoError),
    Render(RenderError),
    Geometry(GeomError),
    File(String),
    NothingToSlice,
    TooManyLayers,
    Cancelled,
    Internal(&'static str),
    UnknownCommand(String),
    NotImplemented(&'static str),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Doc(e) => write!(f, "{e}"),
            EngineError::Io(e) => write!(f, "{e}"),
            EngineError::Render(e) => write!(f, "{e}"),
            EngineError::Geometry(e) => write!(f, "{e}"),
            EngineError::File(msg) => f.write_str(msg),
            EngineError::NothingToSlice => f.write_str("no part of the model reaches the bed"),
            EngineError::TooManyLayers => {
                write!(f, "the job would need more than {MAX_LAYERS} layers")
            }
            EngineError::Cancelled => f.write_str("slicing was cancelled"),
            EngineError::Internal(what) => write!(f, "internal error: {what}"),
            EngineError::UnknownCommand(id) => write!(f, "unknown command: {id}"),
            EngineError::NotImplemented(what) => write!(f, "{what} is not implemented yet"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<DocError> for EngineError {
    fn from(e: DocError) -> Self {
        EngineError::Doc(e)
    }
}

impl From<IoError> for EngineError {
    fn from(e: IoError) -> Self {
        EngineError::Io(e)
    }
}

impl From<RenderError> for EngineError {
    fn from(e: RenderError) -> Self {
        EngineError::Render(e)
    }
}

impl From<GeomError> for EngineError {
    fn from(e: GeomError) -> Self {
        EngineError::Geometry(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_render::PathKind;
    use slicecraft_render::fixtures::{cube, cuboid, hollow_box};

    fn stl_of(mesh: &slicecraft_geom::Mesh) -> Vec<u8> {
        let mut s = String::from("solid t\n");
        for t in mesh.triangles() {
            s.push_str("facet normal 0 0 0\nouter loop\n");
            for v in t.vertices() {
                s.push_str(&format!("vertex {} {} {}\n", v.x, v.y, v.z));
            }
            s.push_str("endloop\nendfacet\n");
        }
        s.push_str("endsolid t\n");
        s.into_bytes()
    }

    fn session_with(mesh: Option<slicecraft_geom::Mesh>) -> Option<Session> {
        let mut s = Session::new();
        let bytes = stl_of(&mesh?);
        s.execute(Command::Import { name: "m".into(), bytes, extension: Some("stl".into()) })
            .ok()?;
        Some(s)
    }

    #[test]
    fn every_command_variant_names_a_ready_command() {
        let s = Session::new();
        let samples = [
            Command::Import { name: String::new(), bytes: Vec::new(), extension: None },
            Command::Place { index: 0, transform: Transform::default() },
            Command::Remove { index: 0 },
            Command::Arrange,
            Command::Duplicate { index: 0 },
            Command::Mirror { index: 0, axis: Axis::X },
            Command::SetVisible { index: 0, visible: true },
            Command::Undo,
            Command::Redo,
            Command::Profile(Box::default()),
            Command::LoadProfile(PathBuf::new()),
            Command::SaveProfile { path: PathBuf::new(), name: String::new() },
            Command::Slice,
            Command::ExportGcode(PathBuf::new()),
            Command::Supports(true),
            Command::ExportModels(PathBuf::new()),
            Command::OpenGcode(PathBuf::new()),
            Command::ConnectPrinter { port: String::new(), baud: 115_200 },
            Command::DisconnectPrinter,
            Command::SendToPrinter(String::new()),
            Command::PrintOverUsb,
            Command::PausePrint,
            Command::ResumePrint,
            Command::CancelPrint,
            Command::RepairMesh { index: 0, fill: true },
            Command::RepairMesh { index: 0, fill: false },
            Command::SplitParts { index: 0 },
            Command::MergeModels { indices: vec![] },
            Command::LayFlat { index: 0 },
            Command::AddZone(Box::new(slicecraft_doc::SettingZone::new("z"))),
            Command::RemoveZone { index: 0 },
            Command::AddSupportEdit(SupportEdit::columns(
                SupportEditKind::Block,
                (0.0, 0.0),
                (1.0, 1.0),
            )),
            Command::AddSupportEdit(SupportEdit::columns(
                SupportEditKind::Add,
                (0.0, 0.0),
                (1.0, 1.0),
            )),
            Command::RemoveSupportEdit { index: 0 },
            Command::ClearSupportEdits,
            Command::PreviewSupport,
        ];
        for c in samples {
            assert_eq!(s.command(c.id()).map(|spec| spec.state), Some(CommandState::Ready));
        }
        assert_eq!(s.command("camera-monitor").map(|c| c.state), Some(CommandState::Planned));
        assert!(s.command("nope").is_none());
    }

    #[test]
    fn empty_session_refuses_to_slice() {
        let mut s = Session::new();
        assert_eq!(s.execute(Command::Slice).err(), Some(EngineError::Doc(DocError::Empty)));
    }

    #[test]
    fn import_centres_the_model_on_the_bed() {
        let Some(s) = session_with(cube(20.0)) else { return };
        let b = s.document().bounds();
        assert!((b.center().x - 110.0).abs() < 1e-9 && (b.center().y - 110.0).abs() < 1e-9);
        assert_eq!(b.min().z, 0.0);
    }

    #[test]
    fn cube_slices_into_real_toolpaths() {
        let Some(mut s) = session_with(cube(20.0)) else { return };
        let r = s.execute(Command::Slice);
        assert!(matches!(r, Ok(Outcome::Sliced { layers: 99, .. })), "{r:?}");
        let Some(out) = s.slice_result() else { return };
        // first layer: skirt, two shells, solid infill
        let first = &out.layers[0];
        for kind in [PathKind::Skirt, PathKind::OuterPerimeter, PathKind::SolidInfill] {
            assert!(first.paths.iter().any(|p| p.kind == kind), "layer 0 lacks {kind:?}");
        }
        // a middle layer is sparse inside, solid near the top
        assert!(out.layers[50].paths.iter().any(|p| p.kind == PathKind::SparseInfill));
        assert!(!out.layers[50].paths.iter().any(|p| p.kind == PathKind::SolidInfill));
        assert!(
            out.layers[98].paths.iter().any(|p| p.kind == PathKind::TopSolid),
            "the last layer is a top surface"
        );
        // the outer shell of a 20 mm cube is a 19.55 mm square at its centreline
        let outer: f64 = out.layers[50]
            .paths
            .iter()
            .filter(|p| p.kind == PathKind::OuterPerimeter)
            .map(|p| p.length())
            .sum();
        assert!((outer - 4.0 * 19.55).abs() < 0.01, "outer {outer}");
        assert!(out.gcode.contains(";LAYER:98 Z:19.900"));
        assert_eq!(out.stats.dropped_chains, 0);
    }

    #[test]
    fn cube_filament_matches_hand_estimate() {
        // 20 mm cube, 2 shells, 4 top / 3 bottom solid, 20% sparse: the filament
        // must land within ±10% of an independent area-based estimate.
        let Some(mut s) = session_with(cube(20.0)) else { return };
        let p = PrintSettings { skirt_enabled: false, ..PrintSettings::default() };
        let w = p.extrusion_width;
        assert!(s.execute(Command::Profile(Box::new(p.clone()))).is_ok());
        let Ok(Outcome::Sliced { stats, .. }) = s.execute(Command::Slice) else { return };
        let fil_area = std::f64::consts::PI * 1.75f64.powi(2) / 4.0;
        let mut expect = 0.0;
        for i in 0..99usize {
            let h = if i == 0 { 0.3 } else { 0.2 };
            // the default bead is the rectangle, as in the reference
            let bead = w * h;
            let shells = 4.0 * (20.0 - w) + 4.0 * (20.0 - 3.0 * w);
            let inner = (20.0 - 4.0 * w) * (20.0 - 4.0 * w);
            let solid = !(3..95).contains(&i);
            let infill_len = if solid { inner / w } else { inner / (w * 5.0) };
            expect += (shells + infill_len) * bead / fil_area;
        }
        let got = stats.gcode.filament_mm;
        assert!((got - expect).abs() / expect < 0.10, "got {got:.1} mm, expected {expect:.1} mm");
    }

    #[test]
    fn placement_moves_the_gcode() {
        // regression: placement used to be ignored when writing G-code
        let Some(mut s) = session_with(cube(10.0)) else { return };
        let t = Transform { translate: bed_point(30.0, 40.0), ..Transform::default() };
        assert!(s.execute(Command::Place { index: 0, transform: t }).is_ok());
        assert!(s.execute(Command::Slice).is_ok());
        let Some(out) = s.slice_result() else { return };
        let xs: Vec<f64> = out.layers[5]
            .paths
            .iter()
            .filter(|p| p.kind == PathKind::OuterPerimeter)
            .flat_map(|p| p.points.iter().map(|q| q.x))
            .collect();
        assert!(!xs.is_empty());
        assert!(xs.iter().all(|x| (24.0..36.0).contains(x)), "{xs:?}");
    }

    #[test]
    fn hole_gets_its_own_perimeter() {
        let Some(mut s) = session_with(hollow_box(20.0, 8.0)) else { return };
        assert!(s.execute(Command::Slice).is_ok());
        let Some(out) = s.slice_result() else { return };
        let outers =
            out.layers[40].paths.iter().filter(|p| p.kind == PathKind::OuterPerimeter).count();
        assert_eq!(outers, 2, "outside wall and hole wall");
    }

    #[test]
    fn stale_slice_is_not_served_and_late_job_is_refused() {
        let Some(mut s) = session_with(cube(10.0)) else { return };
        let job = s.slice_job();
        let Ok(out) = job.run(&mut Quiet) else { return };
        assert!(s.accept_slice(job.revision(), out.clone()));
        assert!(s.slice_result().is_some());
        assert!(s.execute(Command::Arrange).is_ok());
        assert!(s.slice_result().is_none());
        assert!(!s.accept_slice(job.revision(), out));
    }

    #[test]
    fn off_bed_and_bad_input_are_refused() {
        let Some(mut s) = session_with(cuboid(Vec3::ZERO, Vec3::new(300.0, 10.0, 10.0))) else {
            return;
        };
        assert!(matches!(
            s.execute(Command::Slice),
            Err(EngineError::Doc(DocError::OutsideBed(_)))
        ));
        let bad = Transform { scale: Vec3::new(f64::NAN, 1.0, 1.0), ..Transform::default() };
        assert!(s.execute(Command::Place { index: 0, transform: bad }).is_err());
        assert!(s.execute(Command::Remove { index: 9 }).is_err());
        let r = s.execute(Command::Import {
            name: "junk".into(),
            bytes: b"hello".to_vec(),
            extension: None,
        });
        assert!(r.is_err());
        let p = PrintSettings { layer_height: -1.0, ..PrintSettings::default() };
        assert!(s.execute(Command::Profile(Box::new(p))).is_err());
    }

    #[test]
    fn emitted_gcode_stays_on_the_bed_and_travels_dry() {
        let Some(mut s) = session_with(hollow_box(30.0, 10.0)) else { return };
        assert!(s.execute(Command::Slice).is_ok());
        let Some(out) = s.slice_result() else { return };
        let bed = s.document().bed.clone();
        let word = |line: &str, c: char| -> Option<f64> {
            line.split_whitespace().find(|w| w.starts_with(c)).and_then(|w| w[1..].parse().ok())
        };
        let mut moves = 0;
        for line in out.gcode.lines().map(|l| l.split(';').next().unwrap_or("").trim()) {
            let is_g0 = line.starts_with("G0 ");
            let is_g1 = line.starts_with("G1 ");
            if !(is_g0 || is_g1) {
                continue;
            }
            moves += 1;
            if is_g0 {
                assert!(word(line, 'E').is_none(), "travel extrudes: {line}");
            }
            if let (Some(x), Some(y)) = (word(line, 'X'), word(line, 'Y')) {
                assert!((0.0..=bed.width).contains(&x) && (0.0..=bed.depth).contains(&y), "{line}");
            }
            if let Some(z) = word(line, 'Z') {
                assert!((0.0..=bed.height).contains(&z), "{line}");
            }
        }
        assert!(moves > 1000);
    }

    #[test]
    fn undo_and_redo_walk_the_history_and_merge_drags() {
        let Some(mut s) = session_with(cube(10.0)) else { return };
        assert!(s.can_undo(), "import is undoable");
        let at = |s: &Session| s.document().objects.first().map(|o| o.transform.translate.x);
        let start = at(&s);
        for x in [50.0, 60.0, 70.0] {
            let t = Transform { translate: bed_point(x, 100.0), ..Transform::default() };
            assert!(s.execute(Command::Place { index: 0, transform: t }).is_ok());
        }
        assert_eq!(at(&s), Some(70.0));
        // three moves in a row are one step
        assert!(s.execute(Command::Undo).is_ok());
        assert_eq!(at(&s), start);
        assert!(s.execute(Command::Redo).is_ok());
        assert_eq!(at(&s), Some(70.0));
        assert!(s.execute(Command::Undo).is_ok());
        assert!(s.execute(Command::Undo).is_ok());
        assert!(s.document().objects.is_empty(), "the import is undone too");
        assert_eq!(s.execute(Command::Undo).ok(), Some(Outcome::Unchanged));
    }

    #[test]
    fn duplicate_mirror_and_hide() {
        let Some(mut s) = session_with(cuboid(Vec3::ZERO, Vec3::new(10.0, 20.0, 5.0))) else {
            return;
        };
        assert!(matches!(
            s.execute(Command::Duplicate { index: 0 }),
            Ok(Outcome::Imported { index: 1, .. })
        ));
        assert_eq!(s.document().objects.len(), 2);
        assert!(s.document().outside_bed().is_empty());
        assert!(s.execute(Command::Mirror { index: 1, axis: Axis::Y }).is_ok());
        assert!(s.execute(Command::SetVisible { index: 0, visible: false }).is_ok());
        let r = s.execute(Command::Slice);
        assert!(r.is_ok(), "{r:?}");
        assert!(s.execute(Command::Duplicate { index: 9 }).is_err());
    }

    #[test]
    fn profiles_save_and_load_through_files() {
        let Some(mut s) = session_with(cube(10.0)) else { return };
        let p = PrintSettings { layer_height: 0.16, bed_width: 300.0, ..PrintSettings::default() };
        assert!(s.execute(Command::Profile(Box::new(p))).is_ok());
        assert_eq!(s.document().bed.width, 300.0, "the bed follows the profile");
        let path =
            std::env::temp_dir().join(format!("slicecraft-profile-{}.txt", std::process::id()));
        let r = s.execute(Command::SaveProfile { path: path.clone(), name: "Mine".into() });
        assert!(r.is_ok(), "{r:?}");
        assert!(s.execute(Command::Profile(Box::default())).is_ok());
        let r = s.execute(Command::LoadProfile(path.clone()));
        assert!(
            matches!(&r, Ok(Outcome::ProfileLoaded { name, warnings }) if name == "Mine" && warnings.is_empty()),
            "{r:?}"
        );
        assert_eq!(s.document().settings.layer_height, 0.16);
        assert_eq!(s.profile_name(), "Mine");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn projects_save_open_and_track_unsaved_changes() {
        let Some(mut s) = session_with(cube(10.0)) else { return };
        assert!(
            s.execute(Command::Import {
                name: "box".into(),
                bytes: stl_of(&cuboid(Vec3::ZERO, Vec3::new(5.0, 8.0, 3.0)).unwrap_or_default()),
                extension: Some("stl".into())
            })
            .is_ok()
        );
        assert!(s.execute(Command::Mirror { index: 1, axis: Axis::Y }).is_ok());
        assert!(s.execute(Command::SetVisible { index: 0, visible: false }).is_ok());
        let p = PrintSettings { layer_height: 0.12, ..PrintSettings::default() };
        assert!(s.execute(Command::Profile(Box::new(p))).is_ok());
        assert!(s.is_modified());
        let path =
            std::env::temp_dir().join(format!("slicecraft-project-{}.scproj", std::process::id()));
        let r = s.execute(Command::SaveProject(path.clone()));
        assert!(r.is_ok(), "{r:?}");
        assert!(!s.is_modified());
        assert_eq!(s.project_path(), Some(path.as_path()));
        let saved = std::fs::read(&path).unwrap_or_default();

        // a fresh session opens it back to the same document
        let mut t = Session::new();
        let r = t.execute(Command::OpenProject(path.clone()));
        assert!(
            matches!(&r, Ok(Outcome::ProjectOpened { objects: 2, warnings, .. }) if warnings.is_empty()),
            "{r:?}"
        );
        assert!(!t.is_modified());
        assert!(!t.can_undo(), "opening a project starts a new history");
        assert_eq!(t.document().settings.layer_height, 0.12);
        assert!(!t.document().objects.first().is_some_and(|o| o.visible));
        assert_eq!(
            t.document().objects.get(1).map(|o| o.world_bounds()),
            s.document().objects.get(1).map(|o| o.world_bounds())
        );
        // and saving it again writes the same bytes
        let again = std::env::temp_dir()
            .join(format!("slicecraft-project-{}-again.scproj", std::process::id()));
        assert!(t.execute(Command::SaveProject(again.clone())).is_ok());
        assert_eq!(std::fs::read(&again).unwrap_or_default(), saved);

        // new project: no models, same profile, nothing to save
        assert!(t.execute(Command::NewProject).is_ok());
        assert!(t.document().objects.is_empty());
        assert_eq!(t.document().settings.layer_height, 0.12);
        assert!(!t.is_modified() && t.project_path().is_none());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&again);
    }

    #[test]
    fn a_damaged_project_is_refused_and_the_document_kept() {
        let Some(mut s) = session_with(cube(10.0)) else { return };
        let path =
            std::env::temp_dir().join(format!("slicecraft-bad-{}.scproj", std::process::id()));
        let _ = std::fs::write(
            &path,
            "# SliceCraft project 1
[profile]
# SliceCraft profile 1
layer_height = 0.2
[object]
triangles = 2
mesh = AAAA
",
        );
        assert!(s.execute(Command::OpenProject(path.clone())).is_err());
        assert_eq!(s.document().objects.len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn simplify3d_profiles_load_through_the_profile_command() {
        let Some(mut s) = session_with(cube(10.0)) else { return };
        let path = std::env::temp_dir().join(format!("slicecraft-{}.fff", std::process::id()));
        let fff = "<?xml version=\"1.0\"?>\n<profile name=\"Mine\" app=\"S3D-Software 4.0.0\">\n<extruder name=\"E\"><diameter>0.4</diameter><autoWidth>1</autoWidth></extruder>\n<layerHeight>0.25</layerHeight><defaultSpeed>3000</defaultSpeed><outlineUnderspeed>0.5</outlineUnderspeed>\n</profile>\n";
        let _ = std::fs::write(&path, fff);
        let r = s.execute(Command::LoadProfile(path.clone()));
        assert!(
            matches!(&r, Ok(Outcome::ProfileImported { name, report }) if name == "Mine" && report.applied.contains(&"layer_height")),
            "{r:?}"
        );
        assert_eq!(s.document().settings.layer_height, 0.25);
        assert_eq!(s.profile_name(), "Mine");
        assert!(s.can_undo(), "an import is one undo step");
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        let _ = std::fs::write(&path, "<profile><layerHeight>0.2");
        assert!(s.execute(Command::LoadProfile(path.clone())).is_err());
        assert_eq!(s.document().settings.layer_height, 0.25, "a broken file changes nothing");
        let _ = std::fs::remove_file(&path);
    }

    /// A 10 mm post under a 30 mm slab: the slab's underside needs support.
    fn table_mesh() -> Option<slicecraft_geom::Mesh> {
        let post = cuboid(Vec3::new(10.0, 10.0, 0.0), Vec3::new(10.0, 10.0, 10.0))?;
        let top = cuboid(Vec3::new(0.0, 0.0, 10.0), Vec3::new(30.0, 30.0, 3.0))?;
        let mut tris = post.triangles().to_vec();
        tris.extend_from_slice(top.triangles());
        slicecraft_geom::Mesh::new(tris).ok()
    }

    #[test]
    fn support_holds_up_a_table_top_and_toggles_by_command() {
        let Some(mut s) = session_with(table_mesh()) else { return };
        let r = s.execute(Command::Slice);
        assert!(matches!(&r, Ok(Outcome::Sliced { .. })), "{r:?}");
        let has_support = |s: &Session| {
            s.slice_result().is_some_and(|o| {
                o.layers.iter().any(|l| l.paths.iter().any(|p| p.kind == PathKind::Support))
            })
        };
        assert!(!has_support(&s), "off by default");

        assert!(matches!(s.execute(Command::Supports(true)), Ok(Outcome::Changed)));
        assert!(matches!(s.execute(Command::Supports(true)), Ok(Outcome::Unchanged)));
        let r = s.execute(Command::Slice);
        assert!(matches!(&r, Ok(Outcome::Sliced { .. })), "{r:?}");
        assert!(has_support(&s));
        let Some(out) = s.slice_result() else { return };
        // support only below the slab, dense right under it, and in the G-code
        let slab = 10.0;
        for l in &out.layers {
            let sup = l
                .paths
                .iter()
                .any(|p| matches!(p.kind, PathKind::Support | PathKind::SupportInterface));
            assert!(!sup || l.z < slab, "support at z {}", l.z);
        }
        // dense interface layers are off by default: support stays easy to remove
        assert!(
            !out.layers
                .iter()
                .any(|l| l.paths.iter().any(|p| p.kind == PathKind::SupportInterface))
        );
        assert!(out.gcode.contains(";TYPE:support"));
        // and it is undoable like any other edit
        assert!(s.execute(Command::Undo).is_ok());
        assert!(!s.document().settings.support_enabled);
    }

    #[test]
    fn travel_inside_a_part_skips_retraction_but_between_parts_does_not() {
        let Some(mut s) = session_with(cube(20.0)) else { return };
        let bytes = stl_of(&cube(20.0).unwrap_or_default());
        assert!(
            s.execute(Command::Import { name: "b".into(), bytes, extension: Some("stl".into()) })
                .is_ok()
        );
        let count = |s: &mut Session, inside: bool| {
            let p = PrintSettings { retract_only_outside: inside, ..s.document().settings.clone() };
            let _ = s.execute(Command::Profile(Box::new(p)));
            match s.execute(Command::Slice) {
                Ok(Outcome::Sliced { stats, .. }) => stats.gcode.retractions,
                _ => 0,
            }
        };
        let always = count(&mut s, false);
        let only_outside = count(&mut s, true);
        assert!(only_outside < always, "{only_outside} vs {always}");
        // the two cubes stand apart: every layer still retracts between them
        assert!(only_outside >= 99, "{only_outside}");
    }

    #[test]
    fn support_edits_are_commands_that_undo_save_and_shape_the_slice() {
        let Some(mut s) = session_with(table_mesh()) else { return };
        // nothing yet: no automatic support, no edits
        assert!(matches!(
            s.execute(Command::PreviewSupport),
            Ok(Outcome::SupportPreviewed { layers: 0, .. })
        ));
        // the table sits centred on the bed; add support under the whole slab
        let b = s.document().bounds();
        let add = SupportEdit::columns(
            SupportEditKind::Add,
            (b.min().x, b.min().y),
            (b.max().x, b.max().y),
        );
        assert!(s.execute(Command::AddSupportEdit(add)).is_ok());
        let with_add = match s.execute(Command::PreviewSupport) {
            Ok(Outcome::SupportPreviewed { layers, volume }) => (layers, volume),
            other => panic_free_fail(&format!("{other:?}")),
        };
        assert!(with_add.0 > 0 && with_add.1 > 0.0, "{with_add:?}");
        // blocking the same box removes it again
        let block = SupportEdit { kind: SupportEditKind::Block, ..add };
        assert!(s.execute(Command::AddSupportEdit(block)).is_ok());
        assert!(matches!(
            s.execute(Command::PreviewSupport),
            Ok(Outcome::SupportPreviewed { layers: 0, .. })
        ));
        // undo the block, then the slice prints support
        assert!(s.execute(Command::Undo).is_ok());
        assert_eq!(s.document().support_edits.len(), 1);
        let sliced = s.execute(Command::Slice);
        assert!(matches!(sliced, Ok(Outcome::Sliced { .. })));
        assert!(s.slice_result().is_some_and(|o| o.gcode.contains(";TYPE:support")));
        // edits survive a project round trip
        let path =
            std::env::temp_dir().join(format!("slicecraft-edits-{}.scproj", std::process::id()));
        assert!(s.execute(Command::SaveProject(path.clone())).is_ok());
        let mut t = Session::new();
        assert!(t.execute(Command::OpenProject(path.clone())).is_ok());
        assert_eq!(t.document().support_edits, s.document().support_edits);
        let _ = std::fs::remove_file(&path);
        // bad boxes and indices are refused
        let bad = SupportEdit::columns(SupportEditKind::Add, (0.0, 0.0), (0.0, 5.0));
        assert!(s.execute(Command::AddSupportEdit(bad)).is_err());
        assert!(s.execute(Command::RemoveSupportEdit { index: 9 }).is_err());
        assert!(s.execute(Command::ClearSupportEdits).is_ok());
        assert!(s.document().support_edits.is_empty());
    }

    /// A failed expectation without `panic!` (denied in this crate).
    fn panic_free_fail(what: &str) -> (usize, f64) {
        assert!(what.is_empty(), "unexpected: {what}");
        (0, 0.0)
    }

    #[test]
    fn combing_walks_around_holes_instead_of_retracting() {
        // a box with a square hole: infill on either side of the hole
        let Some(mut s) = session_with(hollow_box(30.0, 10.0)) else { return };
        let count = |s: &mut Session, comb: bool| {
            let p = PrintSettings { avoid_crossing_walls: comb, ..s.document().settings.clone() };
            let _ = s.execute(Command::Profile(Box::new(p)));
            match s.execute(Command::Slice) {
                Ok(Outcome::Sliced { stats, .. }) => stats.gcode.retractions,
                _ => usize::MAX,
            }
        };
        let straight = count(&mut s, false);
        let combed = count(&mut s, true);
        assert!(combed < straight, "{combed} vs {straight}");
        // every combed travel stays on the bed
        let out = s.slice_result();
        assert!(out.is_some_and(|o| o.layers.iter().all(|l| {
            l.travel_via
                .iter()
                .flatten()
                .all(|p| p.x > 0.0 && p.y > 0.0 && p.x < 220.0 && p.y < 220.0)
        })));
    }

    #[test]
    fn islands_are_finished_one_at_a_time() {
        // two cubes side by side: printing by island travels between them
        // once per layer, by feature three times
        let Some(mut s) = session_with(cube(15.0)) else { return };
        let bytes = stl_of(&cube(15.0).unwrap_or_default());
        assert!(
            s.execute(Command::Import { name: "b".into(), bytes, extension: Some("stl".into()) })
                .is_ok()
        );
        let run = |s: &mut Session, order: slicecraft_doc::IslandOrder| {
            let p = PrintSettings { island_order: order, ..s.document().settings.clone() };
            let _ = s.execute(Command::Profile(Box::new(p)));
            match s.execute(Command::Slice) {
                Ok(Outcome::Sliced { stats, .. }) => stats.gcode.retractions,
                _ => usize::MAX,
            }
        };
        let by_feature = run(&mut s, slicecraft_doc::IslandOrder::ByFeature);
        let nearest = run(&mut s, slicecraft_doc::IslandOrder::Nearest);
        let consistent = run(&mut s, slicecraft_doc::IslandOrder::Consistent);
        assert!(nearest < by_feature, "{nearest} vs {by_feature}");
        assert!(consistent < by_feature, "{consistent} vs {by_feature}");
        // within one island, walls still come before infill
        let Some(out) = s.slice_result() else { return };
        let Some(layer) = out.layers.get(20) else { return };
        let kinds: Vec<PathKind> = layer.paths.iter().map(|p| p.kind).collect();
        let first_infill =
            kinds.iter().position(|k| matches!(k, PathKind::SparseInfill | PathKind::SolidInfill));
        let first_wall = kinds
            .iter()
            .position(|k| matches!(k, PathKind::OuterPerimeter | PathKind::InnerPerimeter));
        assert!(first_wall < first_infill);
    }

    #[test]
    fn a_raft_goes_under_the_part_and_lifts_it() {
        let Some(mut s) = session_with(cube(20.0)) else { return };
        let plain = match s.execute(Command::Slice) {
            Ok(Outcome::Sliced { layers, .. }) => layers,
            _ => 0,
        };
        let p = PrintSettings { raft_enabled: true, ..s.document().settings.clone() };
        assert!(s.execute(Command::Profile(Box::new(p.clone()))).is_ok());
        let r = s.execute(Command::Slice);
        assert!(matches!(r, Ok(Outcome::Sliced { .. })), "{r:?}");
        let Some(out) = s.slice_result() else { return };
        let raft = (p.raft_base_layers + p.raft_top_layers) as usize;
        assert_eq!(out.layers.len(), plain + raft);
        // the raft layers come first, and are raft (plus the skirt)
        for l in out.layers.iter().take(raft) {
            assert!(l.paths.iter().all(|t| matches!(t.kind, PathKind::Raft | PathKind::Skirt)));
            assert!(l.paths.iter().any(|t| t.kind == PathKind::Raft));
        }
        // the part sits on the raft plus the air gap
        let raft_top = out.layers.get(raft - 1).map_or(0.0, |l| l.z);
        let part_first = out.layers.get(raft).map_or(0.0, |l| l.z);
        assert!((part_first - (raft_top + p.raft_gap + p.first_layer_height)).abs() < 1e-9);
        // the raft reaches past the part by about the margin
        let reach = |kind: PathKind| {
            out.layers
                .iter()
                .flat_map(|l| l.paths.iter())
                .filter(|t| t.kind == kind)
                .flat_map(|t| t.points.iter())
                .fold(f64::INFINITY, |m, q| m.min(q.x))
        };
        let gap = reach(PathKind::OuterPerimeter) - reach(PathKind::Raft);
        assert!(gap > p.raft_margin - 1.0 && gap < p.raft_margin + 1.0, "{gap}");
        assert!(out.gcode.contains(";TYPE:raft"));
    }

    #[test]
    fn bridges_top_surfaces_and_ironing_are_their_own_features() {
        let Some(mut s) = session_with(table_mesh()) else { return };
        let count = |s: &Session, kind: PathKind| {
            s.slice_result().map_or(0, |o| {
                o.layers.iter().flat_map(|l| l.paths.iter()).filter(|p| p.kind == kind).count()
            })
        };
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        assert!(count(&s, PathKind::Bridge) > 0, "the slab's underside bridges");
        assert!(count(&s, PathKind::TopSolid) > 0, "the slab has a top surface");
        assert_eq!(count(&s, PathKind::Ironing), 0);
        // bridges run across the shorter span: here every bridge layer is on the slab's first layer
        let first_bridge = s.slice_result().and_then(|o| {
            o.layers
                .iter()
                .find(|l| l.paths.iter().any(|p| p.kind == PathKind::Bridge))
                .map(|l| l.z)
        });
        assert!(first_bridge.is_some_and(|z| z > 10.0 && z < 10.6), "{first_bridge:?}");
        let p = PrintSettings {
            ironing_enabled: true,
            bridging_enabled: false,
            monotonic_solid: true,
            ..s.document().settings.clone()
        };
        assert!(s.execute(Command::Profile(Box::new(p))).is_ok());
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        assert!(count(&s, PathKind::Ironing) > 0);
        assert_eq!(count(&s, PathKind::Bridge), 0);
    }

    #[test]
    fn adaptive_layers_follow_the_slope() {
        use slicecraft_render::fixtures::octahedron;
        let Some(mut s) = session_with(octahedron(10.0)) else { return };
        let p = PrintSettings { adaptive_layers: true, ..s.document().settings.clone() };
        assert!(s.execute(Command::Profile(Box::new(p.clone()))).is_ok());
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        let Some(out) = s.slice_result() else { return };
        let heights: Vec<f64> = out.layers.iter().skip(1).map(|l| l.height).collect();
        assert!(
            heights
                .iter()
                .all(|h| *h >= p.adaptive_min_height - 1e-9 && *h <= p.adaptive_max_height + 1e-9)
        );
        // an octahedron's faces are 54.7° from vertical: neither the smallest nor the largest
        let (lo, hi) =
            heights.iter().fold((f64::INFINITY, 0.0f64), |(a, b), h| (a.min(*h), b.max(*h)));
        assert!(lo < p.adaptive_max_height, "{lo} {hi}");
        // finer quality, thinner layers
        let fine = PrintSettings { adaptive_quality: 10, ..p };
        assert!(s.execute(Command::Profile(Box::new(fine))).is_ok());
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        let more = s.slice_result().map_or(0, |o| o.layers.len());
        assert!(more > out.layers.len(), "{more} vs {}", out.layers.len());
    }

    #[test]
    fn vase_mode_spirals_one_wall() {
        let Some(mut s) = session_with(cube(20.0)) else { return };
        let p = PrintSettings { vase_mode: true, ..s.document().settings.clone() };
        assert!(s.execute(Command::Profile(Box::new(p.clone()))).is_ok());
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        let Some(out) = s.slice_result() else { return };
        let mid = out.layers.get(50).map(|l| l.paths.iter().map(|p| p.kind).collect::<Vec<_>>());
        assert_eq!(mid, Some(vec![PathKind::OuterPerimeter]));
        // the wall's moves carry a rising Z
        assert!(
            out.gcode.lines().filter(|l| l.starts_with("G1 X") && l.contains(" Z")).count() > 100
        );
    }

    #[test]
    fn combined_infill_and_size_compensation() {
        let Some(mut s) = session_with(hollow_box(30.0, 10.0)) else { return };
        let base = s.document().settings.clone();
        let sparse_layers = |s: &Session| {
            s.slice_result().map_or(0, |o| {
                o.layers
                    .iter()
                    .filter(|l| l.paths.iter().any(|p| p.kind == PathKind::SparseInfill))
                    .count()
            })
        };
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        let every = sparse_layers(&s);
        let p = PrintSettings { infill_every_layers: 2, ..base.clone() };
        assert!(s.execute(Command::Profile(Box::new(p))).is_ok());
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        let half = sparse_layers(&s);
        assert!(half * 2 <= every + 1 && half * 2 >= every - 1, "{half} vs {every}");
        // holes 0.2 mm bigger: the inner wall moves 0.2 mm out
        let outer_x = |s: &Session| {
            s.slice_result().and_then(|o| {
                o.layers.get(30).map(|l| {
                    l.paths
                        .iter()
                        .filter(|p| p.kind == PathKind::OuterPerimeter)
                        .flat_map(|p| p.points.iter())
                        .map(|q| q.x)
                        .fold(f64::INFINITY, f64::min)
                })
            })
        };
        let p = PrintSettings { outer_size_offset: 0.2, ..base };
        let before = outer_x(&s);
        assert!(s.execute(Command::Profile(Box::new(p))).is_ok());
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        let after = outer_x(&s);
        assert!(
            matches!((before, after), (Some(a), Some(b)) if (a - b - 0.2).abs() < 0.02),
            "{before:?} {after:?}"
        );
    }

    #[test]
    fn models_export_and_3mf_imports_as_separate_models() {
        let Some(mut s) = session_with(cube(10.0)) else { return };
        let bytes = stl_of(&cuboid(Vec3::ZERO, Vec3::new(5.0, 8.0, 3.0)).unwrap_or_default());
        assert!(
            s.execute(Command::Import { name: "b".into(), bytes, extension: Some("stl".into()) })
                .is_ok()
        );
        let dir = std::env::temp_dir();
        for ext in ["stl", "obj", "3mf"] {
            let path = dir.join(format!("slicecraft-export-{}.{ext}", std::process::id()));
            let r = s.execute(Command::ExportModels(path.clone()));
            assert!(matches!(r, Ok(Outcome::Exported { .. })), "{ext}: {r:?}");
            let mut t = Session::new();
            assert!(t.execute(Command::ImportFile(path.clone())).is_ok(), "{ext} reads back");
            let want = if ext == "3mf" { 2 } else { 1 };
            assert_eq!(t.document().objects.len(), want, "{ext}");
            let tris: usize = t.document().objects.iter().map(|o| o.mesh.len()).sum();
            assert_eq!(tris, 24, "{ext}");
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn mesh_tools_repair_split_merge_and_lay_flat() {
        use slicecraft_render::fixtures::octahedron;
        // two separate cubes in one mesh, one face flipped
        let a = cuboid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap_or_default();
        let b = cuboid(Vec3::new(20.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap_or_default();
        let mut tris = a.triangles().to_vec();
        tris.extend_from_slice(b.triangles());
        if let Some(t) = tris.get_mut(0) {
            std::mem::swap(&mut t.b, &mut t.c);
        }
        let Some(mut s) = session_with(slicecraft_geom::Mesh::new(tris).ok()) else { return };
        let r = s.execute(Command::RepairMesh { index: 0, fill: true });
        assert!(matches!(&r, Ok(Outcome::Repaired { report, .. }) if report.flipped == 1), "{r:?}");
        assert!(s.document().objects.first().is_some_and(|o| o.mesh.report().is_watertight()));
        let before = s.document().objects.first().map(|o| o.world_bounds());
        assert!(matches!(s.execute(Command::SplitParts { index: 0 }), Ok(Outcome::Changed)));
        assert_eq!(s.document().objects.len(), 2);
        // the parts stay where they were
        let mut after = slicecraft_geom::Bounds3::empty();
        for o in &s.document().objects {
            let wb = o.world_bounds();
            after.add(wb.min());
            after.add(wb.max());
        }
        let before = before.unwrap_or_else(slicecraft_geom::Bounds3::empty);
        assert!(
            (after.min().x - before.min().x).abs() < 1e-6
                && (after.max().x - before.max().x).abs() < 1e-6
        );
        assert!(matches!(
            s.execute(Command::MergeModels { indices: vec![0, 1] }),
            Ok(Outcome::Imported { .. })
        ));
        assert_eq!(s.document().objects.len(), 1);
        assert!(s.execute(Command::Undo).is_ok());
        assert_eq!(s.document().objects.len(), 2);
        // lay an octahedron on a face: its lowest layer becomes a full triangle
        let Some(mut t) = session_with(octahedron(10.0)) else { return };
        assert!(matches!(t.execute(Command::LayFlat { index: 0 }), Ok(Outcome::Changed)));
        let mesh = t.document().objects.first().map(|o| o.world_mesh());
        let Some(Ok(mesh)) = mesh else { return };
        let bottom = mesh.bounds().min().z;
        let on_bed = mesh
            .triangles()
            .iter()
            .filter(|t| t.vertices().iter().all(|v| (v.z - bottom).abs() < 1e-6))
            .count();
        assert_eq!(on_bed, 1, "one face lies on the bed");
    }

    #[test]
    fn setting_zones_change_heights_and_models() {
        use slicecraft_doc::SettingZone;
        use slicecraft_doc::settings::SettingValue;
        // two cubes; the second gets 4 walls through a model zone, and the
        // top half of both gets 60 % infill and 220 °C through a height zone
        let Some(mut s) = session_with(cube(20.0)) else { return };
        let bytes = stl_of(&cube(20.0).unwrap_or_default());
        assert!(
            s.execute(Command::Import { name: "b".into(), bytes, extension: Some("stl".into()) })
                .is_ok()
        );
        let mut walls = SettingZone::new("thick walls");
        walls.objects = vec![1];
        walls.overrides.insert("perimeter_count".into(), SettingValue::Int(4));
        assert!(s.execute(Command::AddZone(Box::new(walls))).is_ok());
        let mut top = SettingZone::new("top half");
        top.z_from = 10.0;
        top.overrides.insert("nozzle_temperature".into(), SettingValue::Float(220.0));
        top.overrides.insert("infill_percentage".into(), SettingValue::Float(60.0));
        assert!(s.execute(Command::AddZone(Box::new(top))).is_ok());
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        let Some(out) = s.slice_result() else { return };
        // layer 20: cube b has twice the shells of cube a
        let Some(l) = out.layers.get(20) else { return };
        let b_x = s.document().objects.get(1).map_or(0.0, |o| o.world_bounds().min().x);
        let shells = |left: bool| {
            l.paths
                .iter()
                .filter(|p| matches!(p.kind, PathKind::OuterPerimeter | PathKind::InnerPerimeter))
                .filter(|p| p.points.first().is_some_and(|q| (q.x >= b_x - 1.0) != left))
                .count()
        };
        assert_eq!(
            shells(false),
            2 * shells(true),
            "zone walls: {} vs {}",
            shells(false),
            shells(true)
        );
        // the height zone sets the temperature and denser infill above 10 mm
        assert!(out.gcode.contains("M104 S220 ; zone temperature"));
        let sparse_len = |l: &slicecraft_render::LayerPaths| {
            l.paths
                .iter()
                .filter(|p| p.kind == PathKind::SparseInfill)
                .map(|p| p.points.len())
                .sum::<usize>()
        };
        let (low, high) = (out.layers.get(20).map(sparse_len), out.layers.get(70).map(sparse_len));
        assert!(matches!((low, high), (Some(a), Some(b)) if b > a), "{low:?} {high:?}");
        // removing the first model forgets it in the zone
        assert!(s.execute(Command::Remove { index: 0 }).is_ok());
        assert_eq!(s.document().zones.first().map(|z| z.objects.clone()), Some(vec![0]));
        // a zone naming a missing model is refused
        let mut bad = SettingZone::new("bad");
        bad.objects = vec![9];
        assert!(s.execute(Command::AddZone(Box::new(bad))).is_err());
    }

    #[test]
    fn a_gcode_file_opens_for_preview() {
        let Some(mut s) = session_with(cube(20.0)) else { return };
        assert!(matches!(s.execute(Command::Slice), Ok(Outcome::Sliced { .. })));
        let path =
            std::env::temp_dir().join(format!("slicecraft-open-{}.gcode", std::process::id()));
        assert!(s.execute(Command::ExportGcode(path.clone())).is_ok());
        let made = s.slice_result().map_or(0, |o| o.layers.len());
        let mut t = Session::new();
        let r = t.execute(Command::OpenGcode(path.clone()));
        assert!(matches!(r, Ok(Outcome::GcodeOpened { layers, .. }) if layers == made), "{r:?}");
        assert!(t.slice_result().is_some_and(|o| {
            o.layers.iter().any(|l| l.paths.iter().any(|p| p.kind == PathKind::OuterPerimeter))
        }));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn printing_needs_a_connection_and_sends_the_slice() {
        let Some(mut s) = session_with(cube(10.0)) else { return };
        assert!(s.execute(Command::PrintOverUsb).is_err(), "no printer yet");
        assert!(s.execute(Command::SendToPrinter("G28".into())).is_err());
        assert!(matches!(s.execute(Command::DisconnectPrinter), Ok(Outcome::Unchanged)));
        assert!(
            s.execute(Command::ConnectPrinter { port: "/no/such/port".into(), baud: 115_200 })
                .is_err()
        );
    }

    /// A printer that says `ok` to everything.
    struct OkPrinter {
        pending: std::collections::VecDeque<u8>,
        lines: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl std::io::Write for OkPrinter {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.pending.extend(b"ok\n");
            if let Ok(mut n) = self.lines.lock() {
                *n += 1;
            }
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl std::io::Read for OkPrinter {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.pending.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(1));
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "idle"));
            }
            let n = out.len().min(self.pending.len());
            for slot in out.iter_mut().take(n) {
                *slot = self.pending.pop_front().unwrap_or(b' ');
            }
            Ok(n)
        }
    }

    #[test]
    fn a_slice_prints_to_a_connected_printer() {
        use slicecraft_io::printer::{HostEvent, spawn_host};
        let Some(mut s) = session_with(cube(5.0)) else { return };
        let lines = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let fake = OkPrinter { pending: Default::default(), lines: std::sync::Arc::clone(&lines) };
        s.attach_printer(spawn_host(fake, "fake".into()));
        let r = s.execute(Command::PrintOverUsb);
        assert!(matches!(r, Ok(Outcome::PrintStarted { .. })), "{r:?}");
        let Ok(Outcome::PrintStarted { lines: count }) = r else { return };
        let start = std::time::Instant::now();
        let mut finished = false;
        while start.elapsed() < std::time::Duration::from_secs(30) && !finished {
            finished = s.printer_events().contains(&HostEvent::PrintFinished);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(finished, "the print did not finish");
        assert!(lines.lock().map(|n| *n).unwrap_or(0) > count, "every line went out");
    }

    #[test]
    fn restoring_a_profile_does_not_mark_changes() {
        let mut s = Session::new();
        let profile = Profile {
            name: "Last".into(),
            settings: PrintSettings { layer_height: 0.3, ..PrintSettings::default() },
        };
        assert!(s.restore_profile(profile).is_ok());
        assert!(!s.is_modified() && !s.can_undo());
        assert_eq!(s.profile_name(), "Last");
        let bad = Profile {
            name: "Bad".into(),
            settings: PrintSettings { layer_height: -1.0, ..PrintSettings::default() },
        };
        assert!(s.restore_profile(bad).is_err());
        assert_eq!(s.document().settings.layer_height, 0.3);
    }

    #[test]
    fn patterns_seams_and_adhesion_all_slice() {
        use slicecraft_doc::{InfillPattern, SeamMode, SolidPattern};
        let Some(mut s) = session_with(hollow_box(30.0, 10.0)) else { return };
        for (pattern, seam) in [
            (InfillPattern::Grid, SeamMode::Aligned),
            (InfillPattern::Triangles, SeamMode::Random),
            (InfillPattern::Concentric, SeamMode::Nearest),
            (InfillPattern::Gyroid, SeamMode::Nearest),
        ] {
            let p = PrintSettings {
                infill_pattern: pattern,
                seam_mode: seam,
                solid_pattern: SolidPattern::Concentric,
                brim_enabled: true,
                skirt_layers: 3,
                ..PrintSettings::default()
            };
            assert!(s.execute(Command::Profile(Box::new(p))).is_ok());
            let r = s.execute(Command::Slice);
            assert!(matches!(&r, Ok(Outcome::Sliced { .. })), "{pattern:?}: {r:?}");
            let Some(out) = s.slice_result() else { return };
            assert!(out.layers[0].paths.iter().any(|p| p.kind == PathKind::Brim));
            assert!(out.layers[2].paths.iter().any(|p| p.kind == PathKind::Skirt));
            assert!(out.layers[20].paths.iter().any(|p| p.kind == PathKind::SparseInfill));
            let warned = out.stats.warnings.iter().any(|w| w.contains("pattern"));
            assert_eq!(warned, !pattern.is_implemented());
        }
    }

    #[test]
    fn export_writes_a_file() {
        let Some(mut s) = session_with(cube(10.0)) else { return };
        let path =
            std::env::temp_dir().join(format!("slicecraft-test-{}.gcode", std::process::id()));
        let r = s.execute(Command::ExportGcode(path.clone()));
        assert!(matches!(r, Ok(Outcome::Exported { bytes, .. }) if bytes > 1000));
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(text.contains("G1 X"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn preview_reports_layers() {
        let Some(s) = session_with(cube(10.0)) else { return };
        let ls = s.preview_layers();
        assert_eq!(ls.len(), 50);
        assert!(ls.iter().all(|l| l.loops.len() == 1));
    }
}
