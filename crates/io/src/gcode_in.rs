//! Reading G-code for preview: moves become layers of toolpaths, with each
//! path's feature taken from the comments SliceCraft, Simplify3D (4.x and
//! 5.x), Cura and PrusaSlicer write.
//!
//! The file is hostile input: its size and the number of moves are capped, and
//! numbers that do not parse or are not finite are ignored.

use crate::IoError;
use crate::gcode::GcodeStats;
use slicecraft_geom::Point2;
use slicecraft_render::{LayerPaths, PathKind, ToolPath};

/// Most moves read from one file.
pub const MAX_MOVES: usize = 50_000_000;

/// A feature named in a comment, or `None` when the comment names none.
pub fn feature_from_comment(comment: &str) -> Option<PathKind> {
    let c = comment.trim().trim_start_matches(';').trim();
    let lower = c.to_ascii_lowercase();
    // ";TYPE:..." (SliceCraft, Cura, PrusaSlicer) or "; feature ..." (Simplify3D 5)
    let name = if let Some(rest) = lower.strip_prefix("type:") {
        rest.trim().to_string()
    } else if let Some(rest) = lower.strip_prefix("feature ") {
        rest.trim().to_string()
    } else {
        lower.clone()
    };
    let kind = match name.as_str() {
        // ours (labels) and Simplify3D 4 bare comments share these names
        "outer perimeter" | "wall-outer" | "external perimeter" | "overhang perimeter" => {
            PathKind::OuterPerimeter
        }
        "inner perimeter" | "wall-inner" | "perimeter" => PathKind::InnerPerimeter,
        "solid infill" | "solid layer" | "skin" | "internal solid infill" => PathKind::SolidInfill,
        "sparse infill" | "infill" | "fill" | "internal infill" => PathKind::SparseInfill,
        "top surface" | "top solid infill" => PathKind::TopSolid,
        "bridge" | "bridge infill" => PathKind::Bridge,
        "gap fill" => PathKind::GapFill,
        "ironing" => PathKind::Ironing,
        "support" | "support material" => PathKind::Support,
        "dense support" | "support-interface" | "support material interface" => {
            PathKind::SupportInterface
        }
        "skirt" | "skirt/brim" => PathKind::Skirt,
        "brim" => PathKind::Brim,
        "raft" | "prime pillar" => PathKind::Raft,
        _ => return None,
    };
    Some(kind)
}

/// What a G-code file holds, as toolpaths.
#[derive(Debug, Clone, Default)]
pub struct ReadGcode {
    pub layers: Vec<LayerPaths>,
    pub stats: GcodeStats,
}

struct Reader {
    layers: Vec<LayerPaths>,
    current: Vec<ToolPath>,
    path: Vec<Point2>,
    kind: PathKind,
    layer_z: Option<f64>,
    stats: GcodeStats,
}

impl Reader {
    fn end_path(&mut self) {
        if self.path.len() >= 2 {
            self.current.push(ToolPath {
                kind: self.kind,
                points: std::mem::take(&mut self.path),
                closed: false,
            });
        }
        self.path.clear();
    }

    fn end_layer(&mut self) {
        self.end_path();
        if let Some(z) = self.layer_z
            && !self.current.is_empty()
        {
            let below = self.layers.last().map_or(0.0, |l| l.z);
            self.layers.push(LayerPaths {
                index: self.layers.len(),
                z,
                height: (z - below).max(0.01),
                paths: std::mem::take(&mut self.current),
                inside_travel: Vec::new(),
                travel_via: Vec::new(),
            });
        }
        self.current.clear();
    }
}

/// Read G-code text into layers of toolpaths.
pub fn read_gcode(text: &str) -> Result<ReadGcode, IoError> {
    let mut r = Reader {
        layers: Vec::new(),
        current: Vec::new(),
        path: Vec::new(),
        kind: PathKind::SparseInfill,
        layer_z: None,
        stats: GcodeStats::default(),
    };
    let (mut x, mut y, mut z, mut e) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut relative_e = false;
    let mut relative_xyz = false;
    let mut feed = 1500.0f64; // mm/min
    let mut moves = 0usize;
    for raw in text.lines() {
        let (code, comment) = match raw.split_once(';') {
            Some((c, m)) => (c, Some(m)),
            None => (raw, None),
        };
        if let Some(m) = comment
            && let Some(kind) = feature_from_comment(m)
        {
            if kind != r.kind {
                r.end_path();
            }
            r.kind = kind;
        }
        let Some(cmd) = code.split_whitespace().next() else { continue };
        let cmd = cmd.to_ascii_uppercase();
        let val = |letter: char| -> Option<f64> {
            code.split_whitespace()
                .find(|w| w.starts_with(letter) || w.starts_with(letter.to_ascii_lowercase()))
                .and_then(|w| w.get(1..)?.parse::<f64>().ok())
                .filter(|v| v.is_finite())
        };
        match cmd.as_str() {
            "M82" => relative_e = false,
            "M83" => relative_e = true,
            "G90" => {
                relative_xyz = false;
                relative_e = false;
            }
            "G91" => relative_xyz = true,
            "G92" => {
                if let Some(v) = val('E') {
                    e = v;
                }
            }
            "G0" | "G1" | "G00" | "G01" => {
                moves += 1;
                if moves > MAX_MOVES {
                    return Err(IoError::TooLarge);
                }
                if let Some(f) = val('F').filter(|f| *f > 0.0) {
                    feed = f;
                }
                let (nx, ny, nz) = if relative_xyz {
                    (
                        x + val('X').unwrap_or(0.0),
                        y + val('Y').unwrap_or(0.0),
                        z + val('Z').unwrap_or(0.0),
                    )
                } else {
                    (val('X').unwrap_or(x), val('Y').unwrap_or(y), val('Z').unwrap_or(z))
                };
                let de = match val('E') {
                    Some(v) if relative_e => v,
                    Some(v) => {
                        let d = v - e;
                        e = v;
                        d
                    }
                    None => 0.0,
                };
                if relative_e {
                    e += de;
                }
                let d = ((nx - x).powi(2) + (ny - y).powi(2)).sqrt();
                r.stats.estimated_seconds += d.max((nz - z).abs()) / (feed / 60.0);
                if de > 0.0 && d > 1e-6 {
                    // extrusion: a new layer starts when printing at a new height
                    if r.layer_z.is_none_or(|lz| (nz - lz).abs() > 1e-4) {
                        r.end_layer();
                        r.layer_z = Some(nz);
                    }
                    if r.path.is_empty() {
                        r.path.push(Point2::new(x, y));
                    }
                    r.path.push(Point2::new(nx, ny));
                    r.stats.extruded_mm += d;
                    r.stats.filament_mm += de;
                } else if d > 1e-6 {
                    r.end_path();
                    r.stats.travel_mm += d;
                } else if de < 0.0 {
                    r.stats.retractions += 1;
                }
                x = nx;
                y = ny;
                z = nz;
            }
            _ => {}
        }
    }
    r.end_layer();
    r.stats.layers = r.layers.len();
    if r.layers.is_empty() {
        return Err(IoError::Damaged("the file has no extrusion moves"));
    }
    Ok(ReadGcode { layers: r.layers, stats: r.stats })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gcode::write_gcode;
    use slicecraft_doc::{Bed, PrintSettings};

    #[test]
    fn our_own_gcode_reads_back_layer_by_layer() {
        let line = |a: (f64, f64), b: (f64, f64), kind| ToolPath {
            kind,
            points: vec![Point2::new(a.0, a.1), Point2::new(b.0, b.1)],
            closed: false,
        };
        let layers: Vec<LayerPaths> = (0..3)
            .map(|i| LayerPaths {
                index: i,
                z: 0.3 + i as f64 * 0.2,
                height: 0.2,
                paths: vec![
                    line((10.0, 10.0), (50.0, 10.0), PathKind::OuterPerimeter),
                    line((10.0, 40.0), (50.0, 40.0), PathKind::SparseInfill),
                ],
                inside_travel: Vec::new(),
                travel_via: Vec::new(),
            })
            .collect();
        let out = write_gcode(&layers, &PrintSettings::default(), &Bed::default());
        let back = read_gcode(&out.text);
        assert!(back.is_ok(), "{back:?}");
        let back = back.unwrap_or_default();
        assert_eq!(back.layers.len(), 3);
        assert!((back.stats.filament_mm - out.stats.filament_mm).abs() < 1e-3);
        let kinds: Vec<PathKind> =
            back.layers.iter().flat_map(|l| l.paths.iter().map(|p| p.kind)).collect();
        assert!(
            kinds.contains(&PathKind::OuterPerimeter) && kinds.contains(&PathKind::SparseInfill)
        );
        assert!(back.layers.iter().all(|l| l.paths.len() == 2));
    }

    #[test]
    fn other_slicers_comments_name_features() {
        assert_eq!(
            feature_from_comment("; feature outer perimeter"),
            Some(PathKind::OuterPerimeter)
        );
        assert_eq!(feature_from_comment(" inner perimeter"), Some(PathKind::InnerPerimeter));
        assert_eq!(feature_from_comment("TYPE:WALL-OUTER"), Some(PathKind::OuterPerimeter));
        assert_eq!(feature_from_comment("TYPE:Top solid infill"), Some(PathKind::TopSolid));
        assert_eq!(feature_from_comment(" solid layer"), Some(PathKind::SolidInfill));
        assert_eq!(feature_from_comment("layer 3, Z = 0.6"), None);
    }

    #[test]
    fn relative_extrusion_and_z_hops() {
        let text = "M83\nG1 Z0.2 F600\nG1 X10 Y0 E1\nG1 Z0.6\nG1 X20 Y0\nG1 Z0.2\nG1 X30 Y0 E1\nG1 Z0.4\nG1 X40 Y0 E1\n";
        let r = read_gcode(text).unwrap_or_default();
        // the hop to 0.6 printed nothing: two layers, at 0.2 and 0.4
        assert_eq!(
            r.layers.iter().map(|l| (l.z * 10.0).round() as i64).collect::<Vec<_>>(),
            vec![2, 4]
        );
        assert!((r.stats.filament_mm - 3.0).abs() < 1e-9);
        assert!(read_gcode("G28\nM104 S200\n").is_err());
        // numbers that do not parse or overflow are ignored, not trusted
        let _ = read_gcode("G1 XNaN Y1 E1\nG1 X1e400 Y2 E2");
    }
}
