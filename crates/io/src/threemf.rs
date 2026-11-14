//! 3MF: reading the models of a `.3mf` package and writing models as one.
//!
//! A 3MF file is a ZIP package holding an XML model (3MF Core Specification).
//! Reading walks the ZIP central directory itself and inflates entries with
//! `miniz_oxide`; the model is parsed with `roxmltree`. Objects may be meshes or
//! components referring to other objects, each with a 3×4 transform; every
//! build item becomes one model, its transforms applied and its units turned
//! into millimetres. Writing stores (no compression) one object per model.
//!
//! Hostile input: entry sizes are capped before inflating, component nesting
//! is bounded, every index is checked, every vertex goes through `Mesh::new`.

use crate::IoError;
use crate::mesh_in::MAX_TRIANGLES;
use roxmltree::{Document, Node, ParsingOptions};
use slicecraft_geom::{Mesh, Triangle, Vec3};
use std::collections::HashMap;
use std::fmt::Write as _;

/// Largest inflated model part, bytes.
pub const MAX_MODEL_BYTES: usize = 768 << 20;
/// Deepest component nesting followed.
const MAX_DEPTH: usize = 16;
/// Most build items (models) read from one file.
pub const MAX_ITEMS: usize = 1000;

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    let s = b.get(at..at + 2)?;
    Some(usize::from(u16::from_le_bytes([s[0], s[1]])))
}

fn u64_at(b: &[u8], at: usize) -> Option<usize> {
    let s = b.get(at..at + 8)?;
    let v = u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
    usize::try_from(v).ok()
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// One entry of a ZIP central directory.
struct Entry {
    name: String,
    method: usize,
    compressed: usize,
    size: usize,
    local: usize,
}

fn zip_entries(b: &[u8]) -> Result<Vec<Entry>, IoError> {
    let bad = IoError::Damaged("the 3MF package is damaged or uses an unknown ZIP feature");
    // the end-of-central-directory record is in the last 64 KiB + 22 bytes
    let from = b.len().saturating_sub(65_557);
    let eocd = (from..b.len().saturating_sub(21))
        .rev()
        .find(|&i| b.get(i..i + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or(bad)?;
    let mut count = u16_at(b, eocd + 10).ok_or(bad)?;
    let mut at = u32_at(b, eocd + 16).ok_or(bad)?;
    // ZIP64: the real values are in the ZIP64 end record, found through its locator
    if (at == 0xFFFF_FFFF || count == 0xFFFF) && eocd >= 20 {
        let loc = eocd - 20;
        if b.get(loc..loc + 4) == Some(&[0x50, 0x4b, 0x06, 0x07]) {
            let rec = u64_at(b, loc + 8).ok_or(bad)?;
            if b.get(rec..rec + 4) != Some(&[0x50, 0x4b, 0x06, 0x06]) {
                return Err(bad);
            }
            count = u64_at(b, rec + 32).ok_or(bad)?.min(65_535);
            at = u64_at(b, rec + 48).ok_or(bad)?;
        }
    }
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count.min(65_535) {
        if b.get(at..at + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(bad);
        }
        let method = u16_at(b, at + 10).ok_or(bad)?;
        let compressed = u32_at(b, at + 20).ok_or(bad)?;
        let size = u32_at(b, at + 24).ok_or(bad)?;
        let name_len = u16_at(b, at + 28).ok_or(bad)?;
        let extra = u16_at(b, at + 30).ok_or(bad)?;
        let comment = u16_at(b, at + 32).ok_or(bad)?;
        let mut local = u32_at(b, at + 42).ok_or(bad)?;
        let name = b.get(at + 46..at + 46 + name_len).ok_or(bad)?;
        let (mut compressed, mut size) = (compressed, size);
        // ZIP64 extra field (id 1): 8-byte values for each field marked 0xFFFFFFFF
        let mut x = at + 46 + name_len;
        let x_end = x + extra;
        while x + 4 <= x_end {
            let id = u16_at(b, x).ok_or(bad)?;
            let len = u16_at(b, x + 2).ok_or(bad)?;
            if id == 1 {
                let mut v = x + 4;
                for field in [&mut size, &mut compressed, &mut local] {
                    if *field == 0xFFFF_FFFF && v + 8 <= x + 4 + len {
                        *field = u64_at(b, v).ok_or(bad)?;
                        v += 8;
                    }
                }
            }
            x += 4 + len;
        }
        out.push(Entry {
            name: String::from_utf8_lossy(name).into_owned(),
            method,
            compressed,
            size,
            local,
        });
        at = at + 46 + name_len + extra + comment;
    }
    Ok(out)
}

fn zip_read(b: &[u8], e: &Entry) -> Result<Vec<u8>, IoError> {
    let bad = IoError::Damaged("the 3MF package is damaged or uses an unknown ZIP feature");
    if e.size > MAX_MODEL_BYTES {
        return Err(IoError::TooLarge);
    }
    if b.get(e.local..e.local + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
        return Err(bad);
    }
    let name_len = u16_at(b, e.local + 26).ok_or(bad)?;
    let extra = u16_at(b, e.local + 28).ok_or(bad)?;
    let start = e.local + 30 + name_len + extra;
    let data = b.get(start..start.checked_add(e.compressed).ok_or(bad)?).ok_or(bad)?;
    match e.method {
        0 => Ok(data.to_vec()),
        8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, MAX_MODEL_BYTES)
            .map_err(|_| bad),
        _ => Err(IoError::Unsupported("3MF compression other than deflate")),
    }
}

/// A 3×4 affine transform as 3MF writes it (row vectors: p' = p · M).
#[derive(Debug, Clone, Copy)]
struct Affine([f64; 12]);

impl Affine {
    const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);

    fn parse(text: Option<&str>) -> Option<Affine> {
        let Some(text) = text else { return Some(Affine::IDENTITY) };
        let v: Vec<f64> =
            text.split_whitespace().map(|t| t.parse::<f64>()).collect::<Result<_, _>>().ok()?;
        let a: [f64; 12] = v.try_into().ok()?;
        a.iter().all(|x| x.is_finite()).then_some(Affine(a))
    }

    fn apply(&self, p: Vec3) -> Vec3 {
        let m = &self.0;
        Vec3::new(
            p.x * m[0] + p.y * m[3] + p.z * m[6] + m[9],
            p.x * m[1] + p.y * m[4] + p.z * m[7] + m[10],
            p.x * m[2] + p.y * m[5] + p.z * m[8] + m[11],
        )
    }

    /// `self` then `outer`.
    fn then(&self, outer: &Affine) -> Affine {
        let (a, b) = (&self.0, &outer.0);
        let mut m = [0.0; 12];
        for r in 0..4 {
            for c in 0..3 {
                let row = |k: usize| if r < 3 { a[r * 3 + k] } else { a[9 + k] };
                m[r * 3 + c] = row(0) * b[c]
                    + row(1) * b[3 + c]
                    + row(2) * b[6 + c]
                    + if r == 3 { b[9 + c] } else { 0.0 };
            }
        }
        Affine(m)
    }

    fn mirrors(&self) -> bool {
        let m = &self.0;
        let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6]);
        det < 0.0
    }
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == name)
}

struct Object<'a, 'i> {
    node: Node<'a, 'i>,
    name: Option<String>,
}

/// Every model in a 3MF package, as (name, mesh in millimetres).
pub fn read_3mf(bytes: &[u8]) -> Result<Vec<(String, Mesh)>, IoError> {
    let entries = zip_entries(bytes)?;
    let model = entries
        .iter()
        .find(|e| e.name.to_ascii_lowercase().ends_with("3dmodel.model"))
        .or_else(|| entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(".model")))
        .ok_or(IoError::Unsupported("a 3MF package without a model"))?;
    let data = zip_read(bytes, model)?;
    let text = std::str::from_utf8(&data).map_err(|_| IoError::NotUtf8)?;
    let options =
        ParsingOptions { allow_dtd: false, nodes_limit: 40_000_000, ..ParsingOptions::default() };
    let doc = Document::parse_with_options(text, options)
        .map_err(|_| IoError::Unsupported("this 3MF model"))?;
    let root = doc.root_element();
    let unit = match root.attribute("unit").unwrap_or("millimeter") {
        "micron" => 0.001,
        "centimeter" => 10.0,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" => 1000.0,
        _ => 1.0,
    };
    let resources =
        child(root, "resources").ok_or(IoError::Unsupported("a 3MF model without resources"))?;
    let mut objects: HashMap<&str, Object> = HashMap::new();
    for o in resources.children().filter(|c| c.tag_name().name() == "object") {
        if let Some(id) = o.attribute("id") {
            objects.insert(id, Object { node: o, name: o.attribute("name").map(str::to_string) });
        }
    }
    let build = child(root, "build").ok_or(IoError::Unsupported("a 3MF model without a build"))?;
    let scale = Affine([unit, 0.0, 0.0, 0.0, unit, 0.0, 0.0, 0.0, unit, 0.0, 0.0, 0.0]);
    let mut out = Vec::new();
    let mut total = 0usize;
    for item in build.children().filter(|c| c.tag_name().name() == "item").take(MAX_ITEMS) {
        let Some(id) = item.attribute("objectid") else { continue };
        let t = Affine::parse(item.attribute("transform")).ok_or(IoError::NonFinite)?;
        let mut tris = Vec::new();
        collect(&objects, id, &t.then(&scale), 0, &mut tris, &mut total)?;
        if tris.is_empty() {
            continue;
        }
        let name =
            objects.get(id).and_then(|o| o.name.clone()).unwrap_or_else(|| format!("object {id}"));
        out.push((name, Mesh::new(tris).map_err(|_| IoError::NonFinite)?));
    }
    if out.is_empty() {
        return Err(IoError::UnknownFormat);
    }
    Ok(out)
}

fn collect(
    objects: &HashMap<&str, Object>,
    id: &str,
    t: &Affine,
    depth: usize,
    out: &mut Vec<Triangle>,
    total: &mut usize,
) -> Result<(), IoError> {
    if depth > MAX_DEPTH {
        return Err(IoError::Unsupported("3MF components nested this deep"));
    }
    let Some(obj) = objects.get(id) else {
        return Err(IoError::Unsupported("a 3MF item naming no object"));
    };
    if let Some(mesh) = child(obj.node, "mesh") {
        let mut verts = Vec::new();
        if let Some(vs) = child(mesh, "vertices") {
            for v in vs.children().filter(|c| c.tag_name().name() == "vertex") {
                let c = |a: &str| {
                    v.attribute(a).and_then(|s| s.parse::<f64>().ok()).filter(|x| x.is_finite())
                };
                let (Some(x), Some(y), Some(z)) = (c("x"), c("y"), c("z")) else {
                    return Err(IoError::NonFinite);
                };
                verts.push(t.apply(Vec3::new(x, y, z)));
            }
        }
        let flip = t.mirrors();
        if let Some(ts) = child(mesh, "triangles") {
            for tri in ts.children().filter(|c| c.tag_name().name() == "triangle") {
                let idx = |a: &str| tri.attribute(a).and_then(|s| s.parse::<usize>().ok());
                let (Some(a), Some(b), Some(c)) = (idx("v1"), idx("v2"), idx("v3")) else {
                    continue;
                };
                let (Some(&a), Some(&b), Some(&c)) = (verts.get(a), verts.get(b), verts.get(c))
                else {
                    continue;
                };
                *total += 1;
                if *total > MAX_TRIANGLES {
                    return Err(IoError::TooLarge);
                }
                // a mirroring transform turns faces inside out: swap two corners back
                out.push(if flip { Triangle { a, b: c, c: b } } else { Triangle { a, b, c } });
            }
        }
    }
    if let Some(comps) = child(obj.node, "components") {
        for comp in comps.children().filter(|c| c.tag_name().name() == "component") {
            let Some(cid) = comp.attribute("objectid") else { continue };
            let ct = Affine::parse(comp.attribute("transform")).ok_or(IoError::NonFinite)?;
            collect(objects, cid, &ct.then(t), depth + 1, out, total)?;
        }
    }
    Ok(())
}

/// CRC-32 (IEEE), as ZIP and PNG use.
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        let idx = ((crc ^ u32::from(b)) & 0xFF) as usize;
        crc = table.get(idx).copied().unwrap_or(0) ^ (crc >> 8);
    }
    !crc
}

/// A ZIP package of stored (uncompressed) files.
fn zip_store(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let n = name.as_bytes();
        let size = data.len() as u32;
        let header = |sig: &[u8], central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(sig);
            if central {
                h.extend_from_slice(&20u16.to_le_bytes());
            }
            h.extend_from_slice(&20u16.to_le_bytes()); // version needed
            h.extend_from_slice(&0u16.to_le_bytes()); // flags
            h.extend_from_slice(&0u16.to_le_bytes()); // stored
            h.extend_from_slice(&0u32.to_le_bytes()); // time, date
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&size.to_le_bytes());
            h.extend_from_slice(&size.to_le_bytes());
            h.extend_from_slice(&(n.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes()); // extra
            if central {
                h.extend_from_slice(&0u16.to_le_bytes()); // comment
                h.extend_from_slice(&0u16.to_le_bytes()); // disk
                h.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
                h.extend_from_slice(&0u32.to_le_bytes()); // external attributes
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(n);
            h
        };
        out.extend(header(&[0x50, 0x4b, 0x03, 0x04], false));
        out.extend_from_slice(data);
        central.extend(header(&[0x50, 0x4b, 0x01, 0x02], true));
    }
    let start = out.len() as u32;
    let size = central.len() as u32;
    out.extend(central);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
    let count = (files.len() as u16).to_le_bytes();
    out.extend_from_slice(&count);
    out.extend_from_slice(&count);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Models (name, mesh in millimetres) as a 3MF package, one object each.
pub fn write_3mf(models: &[(String, Mesh)]) -> Vec<u8> {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n <resources>\n",
    );
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
    for (k, (name, mesh)) in models.iter().enumerate() {
        let _ = writeln!(
            xml,
            "  <object id=\"{}\" name=\"{}\" type=\"model\"><mesh><vertices>",
            k + 1,
            esc(name)
        );
        for t in mesh.triangles() {
            for v in t.vertices() {
                let _ = writeln!(xml, "   <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", v.x, v.y, v.z);
            }
        }
        xml.push_str("  </vertices><triangles>\n");
        for i in 0..mesh.len() {
            let _ = writeln!(
                xml,
                "   <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>",
                3 * i,
                3 * i + 1,
                3 * i + 2
            );
        }
        xml.push_str("  </triangles></mesh></object>\n");
    }
    xml.push_str(" </resources>\n <build>\n");
    for k in 0..models.len() {
        let _ = writeln!(xml, "  <item objectid=\"{}\"/>", k + 1);
    }
    xml.push_str(" </build>\n</model>\n");
    let types = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>\n".to_vec();
    let rels = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>\n".to_vec();
    zip_store(&[
        ("[Content_Types].xml", types),
        ("_rels/.rels", rels),
        ("3D/3dmodel.model", xml.into_bytes()),
    ])
}

/// Models as one binary STL (all triangles together).
pub fn write_stl(models: &[(String, Mesh)]) -> Vec<u8> {
    let count: usize = models.iter().map(|(_, m)| m.len()).sum();
    let mut out = Vec::with_capacity(84 + count * 50);
    let mut header = [0u8; 80];
    let title = b"SliceCraft export";
    if let Some(h) = header.get_mut(..title.len()) {
        h.copy_from_slice(title);
    }
    out.extend_from_slice(&header);
    out.extend_from_slice(&(count.min(u32::MAX as usize) as u32).to_le_bytes());
    for (_, m) in models {
        for t in m.triangles() {
            let n = t.normal().unwrap_or(Vec3::new(0.0, 0.0, 0.0));
            for c in [n.x, n.y, n.z] {
                out.extend_from_slice(&(c as f32).to_le_bytes());
            }
            for v in t.vertices() {
                for c in [v.x, v.y, v.z] {
                    out.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            out.extend_from_slice(&0u16.to_le_bytes());
        }
    }
    out
}

/// Models as Wavefront OBJ, one group each.
pub fn write_obj(models: &[(String, Mesh)]) -> Vec<u8> {
    let mut out = String::from("# SliceCraft export\n");
    let mut base = 1usize;
    for (name, m) in models {
        let _ = writeln!(out, "o {}", name.replace(['\n', '\r'], " "));
        for t in m.triangles() {
            for v in t.vertices() {
                let _ = writeln!(out, "v {} {} {}", v.x, v.y, v.z);
            }
        }
        for i in 0..m.len() {
            let k = base + 3 * i;
            let _ = writeln!(out, "f {} {} {}", k, k + 1, k + 2);
        }
        base += 3 * m.len();
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_render::fixtures::cube;

    #[test]
    fn written_3mf_reads_back_with_the_same_triangles() {
        let Some(m) = cube(10.0) else { return };
        let bytes =
            write_3mf(&[("box & <co>".to_string(), m.clone()), ("two".to_string(), m.clone())]);
        let back = read_3mf(&bytes);
        assert!(back.is_ok(), "{back:?}");
        let back = back.unwrap_or_default();
        assert_eq!(back.len(), 2);
        assert_eq!(back.first().map(|(n, _)| n.as_str()), Some("box & <co>"));
        assert_eq!(back.first().map(|(_, m)| m.triangles().to_vec()), Some(m.triangles().to_vec()));
    }

    #[test]
    fn units_transforms_and_components_apply() {
        // a model in centimetres: one object used twice through components,
        // moved and mirrored
        let model = r#"<?xml version="1.0"?>
<model unit="centimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
 <resources>
  <object id="1" type="model"><mesh>
   <vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
   <triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="1" v3="99"/></triangles>
  </mesh></object>
  <object id="2" name="pair" type="model"><components>
   <component objectid="1"/>
   <component objectid="1" transform="-1 0 0 0 1 0 0 0 1 5 0 0"/>
  </components></object>
 </resources>
 <build><item objectid="2" transform="1 0 0 0 1 0 0 0 1 0 0 2"/></build>
</model>"#;
        let bytes = zip_store(&[("3D/3dmodel.model", model.as_bytes().to_vec())]);
        let r = read_3mf(&bytes);
        assert!(r.is_ok(), "{r:?}");
        let (name, mesh) = r.unwrap_or_default().into_iter().next().unwrap_or_default();
        assert_eq!(name, "pair");
        assert_eq!(mesh.len(), 2, "the bad index is skipped");
        let b = mesh.bounds();
        // cm → mm: the mirrored copy spans x 40..50, z lifted 2 cm = 20 mm
        assert!((b.max().x - 50.0).abs() < 1e-9 && (b.min().z - 20.0).abs() < 1e-9, "{b:?}");
        // the mirrored triangle still faces up
        assert!(mesh.triangles().iter().all(|t| t.normal().is_some_and(|n| n.z > 0.0)));
    }

    #[test]
    fn hostile_packages_are_refused_not_crashed() {
        for bad in [&b""[..], b"PK\x05\x06", b"not a zip at all"] {
            assert!(read_3mf(bad).is_err());
        }
        let Some(m) = cube(5.0) else { return };
        let good = write_3mf(&[("c".into(), m)]);
        let mut cut = 0;
        while cut < good.len() {
            let _ = read_3mf(good.get(..cut).unwrap_or(&[]));
            cut += 61;
        }
        // components that refer to themselves stop at the depth bound
        let looped = r#"<model><resources><object id="1"><components><component objectid="1"/></components></object></resources><build><item objectid="1"/></build></model>"#;
        assert!(read_3mf(&zip_store(&[("3D/3dmodel.model", looped.as_bytes().to_vec())])).is_err());
    }

    #[test]
    fn stl_and_obj_exports_read_back() {
        let Some(m) = cube(10.0) else { return };
        let models = vec![("a".to_string(), m.clone())];
        let stl = crate::read_stl(&write_stl(&models));
        assert!(stl.is_ok_and(|i| i.mesh.len() == m.len()));
        let obj = crate::read_obj(&write_obj(&models));
        assert!(obj.is_ok_and(|i| i.mesh.len() == m.len()));
    }

    #[test]
    fn zip64_entries_are_read() {
        // a package whose entry carries ZIP64 sizes (written by Python's zipfile)
        let hex = "504b03042d000000080000002100f73432efffffffffffffffff1000140033442f33646d6f64656c2e6d6f64656c010010008e01000000000000dd000000000000007d8f518ec3200c44af827c8090a4dd7eac809e25256ee315860a4894f6f44bc84a9baffe58233d8f3da3ae2b3bb1604c14bc86ae69e16a1487119d983d650d4cce1163c608a2ecfaa461caf9f92d65b213f2901a261b430af7dcd8c0f234f2e0e7fb60f31cc93fa40d1165df765fb2edc1a88829ccd162322adc7ed0664163f90bc20f8c1ade973388fc7a1659431407639a8c2a113355dba67015ab8616c4abcef736e511751fd88eba0393ffd773a4c13fdc518aa5aba6a5af41979386beba0ebb724f29f74e451c7ade6672a3519491c5cef7cadb893f266b59f30b504b01022d002d000000080000002100f73432efdd0000008e01000010000000000000000000000080010000000033442f33646d6f64656c2e6d6f64656c504b050600000000010001003e0000001f0100000000";
        let bytes: Vec<u8> = (0..hex.len() / 2)
            .filter_map(|i| hex.get(2 * i..2 * i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
            .collect();
        let r = read_3mf(&bytes);
        assert!(
            r.as_ref().is_ok_and(
                |m| m.len() == 1 && m.first().is_some_and(|(n, m)| n == "z64" && m.len() == 1)
            ),
            "{r:?}"
        );
    }

    #[test]
    fn crc_matches_the_standard() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
