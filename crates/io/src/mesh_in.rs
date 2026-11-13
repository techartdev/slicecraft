//! Mesh import: STL (binary and ASCII, detected by content) and OBJ.
//!
//! Every reader is bounded: triangle counts are capped, sizes are checked
//! before allocation, and facets with non-finite or out-of-range coordinates
//! are skipped and counted instead of aborting the file. Formats follow their
//! public descriptions.

use crate::IoError;
use slicecraft_geom::{Mesh, Triangle, Vec3};

/// Hard caps for import. A file is hostile input; these are the first defence.
pub const MAX_TRIANGLES: usize = 5_000_000;
pub const MAX_FACETS_DECLARED: usize = 50_000_000;
/// Most vertices an OBJ file may declare.
pub const MAX_OBJ_VERTICES: usize = 3 * MAX_TRIANGLES;

const STL_HEADER: usize = 80;
const STL_FACET: usize = 50;

/// Imported model plus how much was skipped.
#[derive(Debug, Clone)]
pub struct Imported {
    pub mesh: Mesh,
    pub skipped_facets: usize,
    pub format: MeshFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshFormat {
    StlBinary,
    StlAscii,
    Obj,
    ThreeMf,
}

impl MeshFormat {
    pub fn name(self) -> &'static str {
        match self {
            MeshFormat::StlBinary => "binary STL",
            MeshFormat::StlAscii => "ASCII STL",
            MeshFormat::Obj => "OBJ",
            MeshFormat::ThreeMf => "3MF",
        }
    }
}

/// Read a mesh, choosing the format from `extension` (without the dot) and,
/// for STL, from the bytes themselves.
pub fn read_mesh(bytes: &[u8], extension: Option<&str>) -> Result<Imported, IoError> {
    let ext = extension.map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("obj") => read_obj(bytes),
        Some("3mf") => {
            // one mesh: every model of the package together
            let models = crate::threemf::read_3mf(bytes)?;
            let mut tris = Vec::new();
            for (_, m) in models {
                tris.extend_from_slice(m.triangles());
            }
            let mesh = Mesh::new(tris).map_err(|_| IoError::NonFinite)?;
            Ok(Imported { mesh, skipped_facets: 0, format: MeshFormat::ThreeMf })
        }
        Some("stl") | None => read_stl(bytes),
        Some(_) => read_stl(bytes).map_err(|_| IoError::UnknownFormat),
    }
}

/// Read STL, binary or ASCII. A file is binary when its declared facet count
/// matches its size exactly, even if its header starts with `solid` (many
/// exporters write that); otherwise text starting with `solid` is ASCII.
pub fn read_stl(bytes: &[u8]) -> Result<Imported, IoError> {
    if let Some(n) = declared_binary_count(bytes) {
        let exact = STL_HEADER + 4 + n.saturating_mul(STL_FACET);
        if bytes.len() == exact {
            return read_binary_stl(bytes);
        }
    }
    let starts_solid = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .and_then(|i| bytes.get(i..i + 5))
        .is_some_and(|w| w == b"solid");
    if starts_solid {
        return read_ascii_stl(bytes);
    }
    if bytes.len() >= STL_HEADER + 4 {
        return read_binary_stl(bytes);
    }
    Err(IoError::NotStl)
}

fn declared_binary_count(bytes: &[u8]) -> Option<usize> {
    let c = bytes.get(STL_HEADER..STL_HEADER + 4)?;
    let n = u32::from_le_bytes([*c.first()?, *c.get(1)?, *c.get(2)?, *c.get(3)?]);
    usize::try_from(n).ok()
}

fn f32_at(bytes: &[u8], at: usize) -> Option<f64> {
    let b = bytes.get(at..at + 4)?;
    Some(f64::from(f32::from_le_bytes([*b.first()?, *b.get(1)?, *b.get(2)?, *b.get(3)?])))
}

fn push_checked(
    tris: &mut Vec<Triangle>,
    skipped: &mut usize,
    a: Vec3,
    b: Vec3,
    c: Vec3,
) -> Result<(), IoError> {
    if !(a.is_in_range() && b.is_in_range() && c.is_in_range()) {
        *skipped += 1;
        return Ok(());
    }
    if tris.len() >= MAX_TRIANGLES {
        return Err(IoError::TooLarge);
    }
    tris.push(Triangle { a, b, c });
    Ok(())
}

/// Binary STL: 80-byte header, u32 count, 50 bytes per facet. A truncated file
/// yields the complete facets it has; the missing ones are counted as skipped.
pub fn read_binary_stl(bytes: &[u8]) -> Result<Imported, IoError> {
    let declared = declared_binary_count(bytes).ok_or(IoError::NotStl)?;
    if declared > MAX_FACETS_DECLARED {
        return Err(IoError::TooLarge);
    }
    let available = bytes.len().saturating_sub(STL_HEADER + 4) / STL_FACET;
    let n = declared.min(available);
    if n > MAX_TRIANGLES {
        return Err(IoError::TooLarge);
    }
    let mut tris = Vec::with_capacity(n);
    let mut skipped = declared - n;
    for i in 0..n {
        let base = STL_HEADER + 4 + i * STL_FACET + 12; // skip the stored normal
        let v = |k: usize| -> Option<Vec3> {
            let o = base + k * 12;
            Some(Vec3::new(f32_at(bytes, o)?, f32_at(bytes, o + 4)?, f32_at(bytes, o + 8)?))
        };
        match (v(0), v(1), v(2)) {
            (Some(a), Some(b), Some(c)) => push_checked(&mut tris, &mut skipped, a, b, c)?,
            _ => skipped += 1,
        }
    }
    let mesh = Mesh::new(tris).map_err(|_| IoError::NonFinite)?;
    Ok(Imported { mesh, skipped_facets: skipped, format: MeshFormat::StlBinary })
}

/// Parse an ASCII STL file: `solid`, `facet normal`, three `vertex`, `endfacet`.
///
/// Unparseable facets are skipped and counted rather than aborting the file.
pub fn read_ascii_stl(bytes: &[u8]) -> Result<Imported, IoError> {
    let text = std::str::from_utf8(bytes).map_err(|_| IoError::NotUtf8)?;
    if !text.trim_start().starts_with("solid") {
        return Err(IoError::NotStl);
    }
    let mut tris = Vec::new();
    let mut skipped = 0usize;
    let mut pending: Vec<Vec3> = Vec::with_capacity(3);
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(head) = parts.next() else { continue };
        match head {
            "vertex" => {
                let coords: Vec<f64> =
                    parts.take(3).filter_map(|p| p.parse::<f64>().ok()).collect();
                match coords.as_slice() {
                    [x, y, z] if pending.len() < 3 => pending.push(Vec3::new(*x, *y, *z)),
                    _ => pending.push(Vec3::new(f64::NAN, 0.0, 0.0)),
                }
            }
            "endfacet" => {
                if let [a, b, c] = pending.as_slice() {
                    push_checked(&mut tris, &mut skipped, *a, *b, *c)?;
                } else {
                    skipped += 1;
                }
                pending.clear();
            }
            _ => {}
        }
    }
    let mesh = Mesh::new(tris).map_err(|_| IoError::NonFinite)?;
    Ok(Imported { mesh, skipped_facets: skipped, format: MeshFormat::StlAscii })
}

/// OBJ: `v x y z` and `f a b c …` (with `/t/n` suffixes and negative indices).
/// Polygons are fan-triangulated. Everything else is ignored.
pub fn read_obj(bytes: &[u8]) -> Result<Imported, IoError> {
    let text = String::from_utf8_lossy(bytes);
    let mut verts: Vec<Vec3> = Vec::new();
    let mut tris = Vec::new();
    let mut skipped = 0usize;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("v") => {
                if verts.len() >= MAX_OBJ_VERTICES {
                    return Err(IoError::TooLarge);
                }
                let c: Vec<f64> = parts.take(3).filter_map(|p| p.parse::<f64>().ok()).collect();
                // keep numbering intact even for a bad vertex; faces using it are skipped
                verts.push(match c.as_slice() {
                    [x, y, z] => Vec3::new(*x, *y, *z),
                    _ => Vec3::new(f64::NAN, 0.0, 0.0),
                });
            }
            Some("f") => {
                let idx: Vec<Option<usize>> = parts.map(|p| obj_index(p, verts.len())).collect();
                let ids: Option<Vec<usize>> = idx.into_iter().collect();
                let Some(ids) = ids.filter(|v| v.len() >= 3) else {
                    skipped += 1;
                    continue;
                };
                let Some(a) = ids.first().and_then(|&i| verts.get(i)).copied() else {
                    skipped += 1;
                    continue;
                };
                for w in ids.windows(2).skip(1) {
                    let (Some(b), Some(c)) = (
                        w.first().and_then(|&i| verts.get(i)).copied(),
                        w.get(1).and_then(|&i| verts.get(i)).copied(),
                    ) else {
                        skipped += 1;
                        continue;
                    };
                    push_checked(&mut tris, &mut skipped, a, b, c)?;
                }
            }
            _ => {}
        }
    }
    if tris.is_empty() && skipped == 0 {
        return Err(IoError::UnknownFormat);
    }
    let mesh = Mesh::new(tris).map_err(|_| IoError::NonFinite)?;
    Ok(Imported { mesh, skipped_facets: skipped, format: MeshFormat::Obj })
}

/// Resolve one OBJ face token (`7`, `7/1`, `7//3`, `-1`) to a 0-based index.
fn obj_index(token: &str, count: usize) -> Option<usize> {
    let first = token.split('/').next()?;
    let i: i64 = first.parse().ok()?;
    let count_i = i64::try_from(count).ok()?;
    let zero_based = if i > 0 { i - 1 } else { count_i + i };
    if zero_based < 0 || zero_based >= count_i {
        return None;
    }
    usize::try_from(zero_based).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASCII: &str = "solid t\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 0\n   vertex 10 0 0\n   vertex 0 10 0\n  endloop\n endfacet\nendsolid t\n";

    fn binary(facets: &[[f32; 9]], declared: u32) -> Vec<u8> {
        let mut b = vec![0u8; 80];
        b[..5].copy_from_slice(b"solid"); // a header that lies, as many do
        b.extend_from_slice(&declared.to_le_bytes());
        for f in facets {
            b.extend_from_slice(&[0u8; 12]);
            for v in f {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend_from_slice(&[0u8; 2]);
        }
        b
    }

    const TRI: [f32; 9] = [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0];

    #[test]
    fn ascii_stl_parses() {
        let r = read_stl(ASCII.as_bytes());
        assert!(r.as_ref().is_ok_and(|i| i.mesh.len() == 1
            && i.skipped_facets == 0
            && i.format == MeshFormat::StlAscii));
    }

    #[test]
    fn binary_stl_with_solid_header_is_binary() {
        let bytes = binary(&[TRI, TRI], 2);
        let r = read_stl(&bytes);
        assert!(r.as_ref().is_ok_and(|i| i.mesh.len() == 2 && i.format == MeshFormat::StlBinary));
    }

    #[test]
    fn truncated_binary_stl_keeps_complete_facets() {
        let mut bytes = binary(&[TRI, TRI, TRI], 3);
        bytes.truncate(bytes.len() - 20);
        let r = read_binary_stl(&bytes);
        assert!(r.as_ref().is_ok_and(|i| i.mesh.len() == 2 && i.skipped_facets == 1));
        // every prefix of a file must be refused or read, never panic
        let full = binary(&[TRI, TRI], 2);
        for n in 0..full.len() {
            let _ = read_stl(&full[..n]);
            let _ = read_mesh(&full[..n], Some("stl"));
        }
    }

    #[test]
    fn absurd_declared_count_is_refused_without_allocating() {
        let bytes = binary(&[TRI], u32::MAX);
        assert_eq!(read_binary_stl(&bytes).err(), Some(IoError::TooLarge));
    }

    #[test]
    fn non_finite_binary_facet_is_skipped() {
        let bad = [f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let bytes = binary(&[TRI, bad], 2);
        let r = read_stl(&bytes);
        assert!(r.as_ref().is_ok_and(|i| i.mesh.len() == 1 && i.skipped_facets == 1));
    }

    #[test]
    fn junk_is_refused() {
        assert_eq!(read_stl(b"\x00\x01\x02").err(), Some(IoError::NotStl));
        assert_eq!(read_ascii_stl(b"solid \xff").err(), Some(IoError::NotUtf8));
        assert_eq!(
            read_mesh(b"PK\x03\x04", Some("3mf")).err(),
            Some(IoError::Damaged("the 3MF package is damaged or uses an unknown ZIP feature"))
        );
    }

    #[test]
    fn incomplete_ascii_facet_is_skipped() {
        let s = "solid t\nfacet normal 0 0 1\nvertex 0 0 0\nendfacet\nendsolid\n";
        let r = read_ascii_stl(s.as_bytes());
        assert!(r.as_ref().is_ok_and(|i| i.mesh.is_empty() && i.skipped_facets == 1));
    }

    #[test]
    fn obj_quads_and_negative_indices() {
        let s = "# square\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1/1/1 2/2/2 3/3/3 4/4/4\nf -4 -3 -2\nf 1 2 99\n";
        let r = read_mesh(s.as_bytes(), Some("OBJ"));
        assert!(r.as_ref().is_ok_and(|i| i.mesh.len() == 3 && i.skipped_facets == 1));
    }
}
