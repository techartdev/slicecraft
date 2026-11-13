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
use slicecraft_doc::{Axis, DocError, Document, PrintSettings, SceneObject, Transform};
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
    MAX_LAYERS, Quiet, SliceObserver, SliceOutput, SliceStats, Stage, slice_document,
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
    planned("lay-flat", "Lay Face Flat"),
    planned("split-parts", "Split into Parts"),
    planned("merge-models", "Merge Models"),
    planned("group-models", "Group Models"),
    planned("model-role", "Use Model as Support or Cutter"),
    planned("export-models", "Export Models (STL, OBJ, 3MF)"),
    // projects and profiles
    ready("profile", "Print Settings"),
    ready("load-profile", "Load Profile"),
    ready("save-profile", "Save Profile"),
    ready("new-project", "New Project"),
    ready("open-project", "Open Project"),
    ready("save-project", "Save Project"),
    planned("multi-profile", "Several Profiles per Job"),
    planned("setting-zones", "Height and Region Setting Zones"),
    planned("profile-wizard", "Printer Setup Assistant"),
    // mesh tools
    planned("repair-mesh", "Repair Mesh"),
    ready("analyze-mesh", "Analyse Mesh"),
    planned("fix-normals", "Fix Inverted Faces"),
    planned("find-intersections", "Find Self-Intersections"),
    planned("reduce-mesh", "Reduce Triangle Count"),
    planned("fill-holes", "Fill Holes"),
    planned("hollow", "Hollow Model"),
    planned("cut-plane", "Cut with a Plane"),
    // support
    ready("supports", "Generate Supports"),
    planned("support-edit", "Add or Remove Support by Hand"),
    planned("support-clear", "Clear Supports"),
    planned("support-blockers", "Support Blockers"),
    // slicing and output
    ready("preview", "Layer Preview"),
    ready("slice", "Slice"),
    ready("export-gcode", "Export G-code"),
    ready("gcode-preview", "Toolpath Preview"),
    planned("open-gcode", "Open G-code for Preview"),
    planned("variable-layers", "Variable Layer Heights"),
    // machine
    planned("machine-control", "Machine Control Panel"),
    planned("print-usb", "Print over USB"),
    planned("print-network", "Send to a Network Printer"),
    planned("firmware-terminal", "Firmware Terminal"),
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
    /// Slice now, on the calling thread.
    Slice,
    /// Write G-code to a file, slicing first if the slice is stale.
    ExportGcode(PathBuf),
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
            Command::Slice => "slice",
            Command::ExportGcode(_) => "export-gcode",
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
        if !matches!(command, Command::Place { .. } | Command::Profile(_)) {
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
                self.doc = doc;
                self.profile_name = project.profile.name;
                self.reset_history(Some(path.clone()));
                Ok(Outcome::ProjectOpened { path, objects, warnings: project.warnings })
            }
            Command::SaveProject(path) => {
                let text = write_project(&self.profile(), &self.doc.objects);
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
            Command::Slice => {
                let out = self.slice_now()?;
                Ok(Outcome::Sliced { layers: out.layers.len(), stats: out.stats.clone() })
            }
            Command::ExportGcode(path) => {
                let out = match self.slice_result() {
                    Some(out) => out,
                    None => self.slice_now()?,
                };
                std::fs::write(&path, out.gcode.as_bytes()).map_err(|e| {
                    EngineError::File(format!("cannot write {}: {e}", path.display()))
                })?;
                Ok(Outcome::Exported { path, bytes: out.gcode.len() })
            }
        }
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
        ];
        for c in samples {
            assert_eq!(s.command(c.id()).map(|spec| spec.state), Some(CommandState::Ready));
        }
        assert_eq!(s.command("support-edit").map(|c| c.state), Some(CommandState::Planned));
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
        assert!(out.layers[98].paths.iter().any(|p| p.kind == PathKind::SolidInfill));
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
            let bead = slicecraft_io::gcode::bead_area(w, h);
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
        assert!(
            out.layers.iter().any(|l| l.paths.iter().any(|p| p.kind == PathKind::SupportInterface))
        );
        assert!(out.gcode.contains(";TYPE:support"));
        // and it is undoable like any other edit
        assert!(s.execute(Command::Undo).is_ok());
        assert!(!s.document().settings.support_enabled);
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
