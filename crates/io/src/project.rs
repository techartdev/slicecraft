//! SliceCraft project files (`.scproj`).
//!
//! A project is one self-contained UTF-8 text file: a header, the profile in
//! the same text form as a `.scprofile` file, then one `[object]` block per
//! model with its name, visibility, placement and mesh. The mesh is the
//! model's triangles as little-endian `f64` corners, base64-encoded, so a
//! project saved, opened and saved again is byte-identical.
//!
//! ```text
//! # SliceCraft project 1
//! [profile]
//! # SliceCraft profile 1
//! name = "Standard 0.20 mm"
//! layer_height = 0.2
//! …
//! [object]
//! name = "bracket"
//! visible = true
//! translate = 110 100 0
//! rotate = 0 0 45
//! scale = 1 1 1
//! triangles = 1284
//! mesh = AAAAAAAA…
//! ```
//!
//! Everything read is hostile: the header must match, sizes are checked before
//! anything is allocated, every coordinate goes through `Mesh::new`, and every
//! transform through `Transform::is_valid`. Unknown keys and sections are
//! skipped with a warning so a newer file still opens.

use crate::IoError;
use crate::mesh_in::MAX_TRIANGLES;
use slicecraft_doc::profile::{self, Profile};
use slicecraft_doc::{MAX_OBJECTS, SceneObject, Transform};
use slicecraft_geom::{Mesh, Triangle, Vec3};

/// First line of every project file.
pub const PROJECT_HEADER: &str = "# SliceCraft project 1";
/// Bytes per triangle in the mesh encoding: 3 corners × 3 coordinates × 8.
const TRIANGLE_BYTES: usize = 72;

/// A project as read from a file.
#[derive(Debug, Clone)]
pub struct Project {
    pub profile: Profile,
    pub objects: Vec<SceneObject>,
    /// One note per line that was skipped.
    pub warnings: Vec<String>,
}

/// The project as text.
pub fn write_project(profile: &Profile, objects: &[SceneObject]) -> String {
    let mut out = String::new();
    out.push_str(PROJECT_HEADER);
    out.push('\n');
    out.push_str("[profile]\n");
    out.push_str(&profile.to_text());
    for o in objects {
        let t = &o.transform;
        out.push_str("[object]\n");
        out.push_str(&format!("name = {}\n", profile::quote(&o.name)));
        out.push_str(&format!("visible = {}\n", o.visible));
        out.push_str(&format!("translate = {}\n", vec3(t.translate)));
        out.push_str(&format!("rotate = {}\n", vec3(t.rotate)));
        out.push_str(&format!("scale = {}\n", vec3(t.scale)));
        out.push_str(&format!("triangles = {}\n", o.mesh.len()));
        out.push_str("mesh = ");
        out.push_str(&encode_mesh(&o.mesh));
        out.push('\n');
    }
    out
}

/// `f64`'s `Display` is the shortest text that reads back to the same value.
fn vec3(v: Vec3) -> String {
    format!("{} {} {}", v.x, v.y, v.z)
}

fn bad(line: usize, what: &'static str) -> IoError {
    IoError::BadProject { line, what }
}

/// An `[object]` block being read.
struct ObjectBlock {
    line: usize,
    name: String,
    visible: bool,
    transform: Transform,
    triangles: Option<usize>,
    mesh: Option<Mesh>,
}

impl ObjectBlock {
    fn new(line: usize) -> Self {
        Self {
            line,
            name: "model".into(),
            visible: true,
            transform: Transform::default(),
            triangles: None,
            mesh: None,
        }
    }

    fn finish(self) -> Result<SceneObject, IoError> {
        let mesh = self.mesh.ok_or(bad(self.line, "an object has no mesh"))?;
        if mesh.is_empty() {
            return Err(bad(self.line, "an object has an empty mesh"));
        }
        let mut transform = self.transform;
        transform.translate.z = 0.0;
        if !transform.is_valid() {
            return Err(bad(self.line, "an object's placement is out of range"));
        }
        let mut o = SceneObject::new(self.name, mesh);
        o.transform = transform;
        o.visible = self.visible;
        Ok(o)
    }
}

enum Section {
    Top,
    Profile,
    Object(ObjectBlock),
    Unknown,
}

/// Read a project. The profile is parsed but not validated; the caller
/// validates it the same way as a profile file.
pub fn read_project(text: &str) -> Result<Project, IoError> {
    let mut lines = text.lines().enumerate();
    match lines.next() {
        Some((_, first)) if first.trim() == PROJECT_HEADER => {}
        _ => return Err(bad(1, "not a SliceCraft project")),
    }
    let mut profile_text = String::new();
    let mut profile_line = 0;
    let mut objects: Vec<SceneObject> = Vec::new();
    let mut warnings = Vec::new();
    let mut section = Section::Top;

    for (i, raw) in lines {
        let n = i + 1;
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            if let Section::Object(block) = std::mem::replace(&mut section, Section::Unknown) {
                push_object(&mut objects, block)?;
            }
            section = match line {
                "[profile]" if profile_line == 0 => {
                    profile_line = n;
                    Section::Profile
                }
                "[profile]" => return Err(bad(n, "a second [profile] section")),
                "[object]" => Section::Object(ObjectBlock::new(n)),
                _ => {
                    warnings.push(format!("line {n}: unknown section {line} skipped"));
                    Section::Unknown
                }
            };
            continue;
        }
        match &mut section {
            Section::Profile => {
                profile_text.push_str(raw);
                profile_text.push('\n');
            }
            Section::Object(block) => object_line(block, line, n, &mut warnings)?,
            Section::Top => {
                if !line.is_empty() && !line.starts_with('#') {
                    warnings.push(format!("line {n}: text before the first section skipped"));
                }
            }
            Section::Unknown => {}
        }
    }
    if let Section::Object(block) = section {
        push_object(&mut objects, block)?;
    }
    if profile_line == 0 {
        return Err(bad(1, "the project has no [profile] section"));
    }
    let (profile, profile_warnings) = Profile::from_text(&profile_text)
        .map_err(|_| bad(profile_line, "the [profile] section is not a SliceCraft profile"))?;
    warnings.extend(profile_warnings.into_iter().map(|w| format!("profile {w}")));
    Ok(Project { profile, objects, warnings })
}

fn push_object(objects: &mut Vec<SceneObject>, block: ObjectBlock) -> Result<(), IoError> {
    if objects.len() >= MAX_OBJECTS {
        return Err(bad(block.line, "the project has more models than SliceCraft accepts"));
    }
    objects.push(block.finish()?);
    Ok(())
}

fn object_line(
    block: &mut ObjectBlock,
    line: &str,
    n: usize,
    warnings: &mut Vec<String>,
) -> Result<(), IoError> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(());
    }
    let Some((key, value)) = line.split_once('=') else {
        warnings.push(format!("line {n}: no '='"));
        return Ok(());
    };
    let (key, value) = (key.trim(), value.trim());
    match key {
        "name" => block.name = profile::unquote(value).ok_or(bad(n, "a name is not quoted"))?,
        "visible" => {
            block.visible = match value {
                "true" => true,
                "false" => false,
                _ => return Err(bad(n, "visible is neither true nor false")),
            }
        }
        "translate" => block.transform.translate = parse_vec3(value, n)?,
        "rotate" => block.transform.rotate = parse_vec3(value, n)?,
        "scale" => block.transform.scale = parse_vec3(value, n)?,
        "triangles" => {
            let count: usize = value.parse().map_err(|_| bad(n, "triangles is not a count"))?;
            if count == 0 || count > MAX_TRIANGLES {
                return Err(bad(n, "triangles is outside the accepted range"));
            }
            block.triangles = Some(count);
        }
        "mesh" => {
            let count = block.triangles.ok_or(bad(n, "mesh comes before triangles"))?;
            block.mesh = Some(decode_mesh(value, count, n)?);
        }
        _ => warnings.push(format!("line {n}: unknown key {key} skipped")),
    }
    Ok(())
}

fn parse_vec3(value: &str, n: usize) -> Result<Vec3, IoError> {
    let mut parts = value.split_whitespace().map(str::parse::<f64>);
    let mut next = || match parts.next() {
        Some(Ok(v)) if v.is_finite() => Ok(v),
        _ => Err(bad(n, "expected three finite numbers")),
    };
    let v = Vec3::new(next()?, next()?, next()?);
    if parts.next().is_some() {
        return Err(bad(n, "expected three finite numbers"));
    }
    Ok(v)
}

fn encode_mesh(mesh: &Mesh) -> String {
    let mut bytes = Vec::with_capacity(mesh.len().saturating_mul(TRIANGLE_BYTES));
    for t in mesh.triangles() {
        for v in t.vertices() {
            for c in [v.x, v.y, v.z] {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    base64_encode(&bytes)
}

fn decode_mesh(text: &str, count: usize, n: usize) -> Result<Mesh, IoError> {
    let expected = count.checked_mul(TRIANGLE_BYTES).ok_or(bad(n, "the mesh is too large"))?;
    // check the size from the text before decoding anything
    if base64_decoded_len(text) != Some(expected) {
        return Err(bad(n, "the mesh length does not match its triangle count"));
    }
    let bytes = base64_decode(text).ok_or(bad(n, "the mesh is not valid base64"))?;
    let mut coords = bytes.chunks_exact(8).map(|c| {
        let mut b = [0u8; 8];
        b.copy_from_slice(c);
        f64::from_le_bytes(b)
    });
    let mut corner =
        || -> Option<Vec3> { Some(Vec3::new(coords.next()?, coords.next()?, coords.next()?)) };
    let mut tris = Vec::with_capacity(count);
    for _ in 0..count {
        let (Some(a), Some(b), Some(c)) = (corner(), corner(), corner()) else {
            return Err(bad(n, "the mesh ends early"));
        };
        tris.push(Triangle { a, b, c });
    }
    Mesh::new(tris).map_err(|_| bad(n, "the mesh has a non-finite or out-of-range coordinate"))
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sym = |shift: u32| {
            char::from(ALPHABET.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A'))
        };
        out.push(sym(18));
        out.push(sym(12));
        out.push(if chunk.len() > 1 { sym(6) } else { '=' });
        out.push(if chunk.len() > 2 { sym(0) } else { '=' });
    }
    out
}

/// Decoded length of well-formed padded base64, from its length alone.
fn base64_decoded_len(text: &str) -> Option<usize> {
    let len = text.len();
    if !len.is_multiple_of(4) {
        return None;
    }
    let pad = text.bytes().rev().take(2).filter(|&b| b == b'=').count();
    (len / 4).checked_mul(3)?.checked_sub(pad)
}

fn base64_value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(base64_decoded_len(text)?);
    let quads = bytes.len() / 4;
    for (q, chunk) in bytes.chunks_exact(4).enumerate() {
        let last = q + 1 == quads;
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for (k, &c) in chunk.iter().enumerate() {
            let v = if k >= 4 - pad { 0 } else { base64_value(c)? };
            n = (n << 6) | v;
        }
        let b = n.to_be_bytes();
        out.push(b[1]);
        if pad < 2 {
            out.push(b[2]);
        }
        if pad < 1 {
            out.push(b[3]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_doc::PrintSettings;
    use slicecraft_render::fixtures::{cube, octahedron};

    fn sample() -> (Profile, Vec<SceneObject>) {
        let settings = PrintSettings {
            layer_height: 0.16,
            infill_percentage: 35.0,
            ..PrintSettings::default()
        };
        let profile = Profile { name: "Test \"quoted\" profile".into(), settings };
        let mut a = SceneObject::new("cube", cube(20.0).unwrap_or_default());
        a.transform.translate = Vec3::new(80.0, 90.5, 0.0);
        a.transform.rotate = Vec3::new(0.0, 0.0, 33.3);
        a.transform.scale = Vec3::new(1.5, 1.0, 0.75);
        let mut b = SceneObject::new("odd name = [x]", octahedron(10.0).unwrap_or_default());
        b.visible = false;
        b.mirror(slicecraft_doc::Axis::X).ok();
        (profile, vec![a, b])
    }

    #[test]
    fn base64_round_trips_every_length() {
        let data: Vec<u8> = (0..=255u8).collect();
        for len in 0..40 {
            let slice = data.get(..len).unwrap_or(&[]);
            let enc = base64_encode(slice);
            assert_eq!(base64_decoded_len(&enc), Some(len));
            assert_eq!(base64_decode(&enc).as_deref(), Some(slice));
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert!(base64_decode("TW=u").is_none());
        assert!(base64_decode("TWE").is_none());
        assert!(base64_decode("T*Fu").is_none());
    }

    #[test]
    fn project_round_trip_is_byte_identical() {
        let (profile, objects) = sample();
        let text = write_project(&profile, &objects);
        let p = read_project(&text).ok().unwrap_or_else(|| Project {
            profile: profile.clone(),
            objects: Vec::new(),
            warnings: vec!["read failed".into()],
        });
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(p.profile, profile);
        assert_eq!(p.objects.len(), 2);
        let again = write_project(&p.profile, &p.objects);
        assert_eq!(again, text);
        let b = &p.objects[1];
        assert_eq!(b.name, "odd name = [x]");
        assert!(!b.visible);
        assert_eq!(p.objects[0].transform, objects[0].transform);
        assert_eq!(p.objects[0].mesh.triangles(), objects[0].mesh.triangles());
    }

    #[test]
    fn hostile_projects_are_refused_not_crashed() {
        let (profile, objects) = sample();
        let text = write_project(&profile, &objects);
        // every prefix either reads or is refused
        let mut cut = 0;
        while cut < text.len() {
            if text.is_char_boundary(cut) {
                let _ = read_project(text.get(..cut).unwrap_or(""));
            }
            cut += 97;
        }
        let cases = [
            "",
            "# not a project\n",
            "# SliceCraft project 1\n[object]\nname = \"x\"\n",
            "# SliceCraft project 1\n[profile]\n# SliceCraft profile 1\n[object]\ntriangles = 99999999999\n",
            "# SliceCraft project 1\n[profile]\n# SliceCraft profile 1\n[object]\ntriangles = 1\nmesh = AAAA\n",
            "# SliceCraft project 1\n[profile]\n# SliceCraft profile 1\n[object]\nmesh = AAAA\n",
            "# SliceCraft project 1\n[profile]\n# SliceCraft profile 1\n[object]\nscale = 1 NaN 1\n",
            "# SliceCraft project 1\n[profile]\n# SliceCraft profile 1\n[object]\nscale = 0 0 0\ntriangles = 1\n",
            "# SliceCraft project 1\n[profile]\n[profile]\n",
        ];
        for c in cases {
            assert!(read_project(c).is_err(), "accepted: {c:?}");
        }
    }

    #[test]
    fn a_mesh_with_a_huge_coordinate_is_refused() {
        let (profile, mut objects) = sample();
        objects.truncate(1);
        let text = write_project(&profile, &objects);
        // replace the mesh with one triangle whose first coordinate is 1e300
        let mut bytes = Vec::new();
        for c in [1e300f64, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let start = text.find("triangles = ").unwrap_or(0);
        let hostile = format!(
            "{}triangles = 1\nmesh = {}\n",
            text.get(..start).unwrap_or(""),
            base64_encode(&bytes)
        );
        assert!(read_project(&hostile).is_err());
    }

    #[test]
    fn unknown_keys_and_sections_are_skipped_with_warnings() {
        let (profile, objects) = sample();
        let text = write_project(&profile, &objects).replace(
            "[object]\nname = \"cube\"",
            "[future]\nwhatever = 1\n[object]\ncolour = red\nname = \"cube\"",
        );
        let p = read_project(&text);
        assert!(p.as_ref().is_ok_and(|p| p.objects.len() == 2 && p.warnings.len() == 2), "{p:?}");
    }
}
