#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! Files in and out.
//!
//! Import: STL (binary and ASCII) and OBJ, from their public descriptions,
//! every reader bounded. Export: G-code for a firmware flavour; the start and
//! end sequences and the flavour table are SliceCraft's own design, informed
//! by public firmware documentation and observed behaviour only.

pub mod fff;
pub mod gcode;
pub mod gcode_in;
pub mod mesh_in;
pub mod printer;
pub mod project;
pub mod threemf;
pub mod thumbnail;

use std::fmt;

pub use fff::{ImportReport, import_fff, looks_like_fff};
pub use gcode::{GcodeOutput, GcodeStats, GcodeWriter, format_duration, path_speed, write_gcode};
pub use mesh_in::{
    Imported, MAX_FACETS_DECLARED, MAX_TRIANGLES, MeshFormat, read_ascii_stl, read_binary_stl,
    read_mesh, read_obj, read_stl,
};
pub use project::{PROJECT_HEADER, Project, read_project, write_project};
pub use slicecraft_doc::Flavor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoError {
    NotUtf8,
    NotStl,
    UnknownFormat,
    Unsupported(&'static str),
    /// A file whose structure is broken (or uses something not handled).
    Damaged(&'static str),
    TooLarge,
    NonFinite,
    /// A project file that cannot be read; `line` is 1-based.
    BadProject {
        line: usize,
        what: &'static str,
    },
    /// A Simplify3D profile that cannot be read.
    BadFff(&'static str),
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoError::NotUtf8 => f.write_str("the file is not UTF-8 text"),
            IoError::NotStl => f.write_str("not an STL file"),
            IoError::UnknownFormat => {
                f.write_str("not a model format SliceCraft reads (STL, OBJ or 3MF)")
            }
            IoError::Unsupported(what) => write!(f, "{what} import is not implemented yet"),
            IoError::Damaged(what) => f.write_str(what),
            IoError::TooLarge => {
                f.write_str("the file declares more triangles than SliceCraft accepts")
            }
            IoError::NonFinite => f.write_str("the file contains a non-finite coordinate"),
            IoError::BadProject { line, what } => write!(f, "line {line} of the project: {what}"),
            IoError::BadFff(what) => write!(f, "Simplify3D profile: {what}"),
        }
    }
}

impl std::error::Error for IoError {}
