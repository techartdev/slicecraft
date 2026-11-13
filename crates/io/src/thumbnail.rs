//! A small picture of the job for printers and hosts that show one: the
//! toolpaths seen from above at an angle, as a PNG in the G-code's comments
//! (`; thumbnail begin WxH LEN` … `; thumbnail end`, the convention Cura and
//! PrusaSlicer use).

use slicecraft_render::{LayerPaths, PathKind};

/// Largest side drawn, px.
pub const MAX_SIZE: u32 = 512;
/// Most segments drawn; past it every n-th one.
const MAX_SEGMENTS: usize = 400_000;

fn colour(kind: PathKind) -> [u8; 3] {
    match kind {
        PathKind::OuterPerimeter => [0xff, 0x98, 0x30],
        PathKind::InnerPerimeter => [0xf0, 0xd0, 0x50],
        PathKind::Support | PathKind::SupportInterface => [0x9a, 0xa4, 0xae],
        PathKind::TopSolid | PathKind::SolidInfill => [0xb8, 0x6b, 0xe8],
        _ => [0xe0, 0x56, 0x56],
    }
}

/// Draw the toolpaths into an RGBA image `size` px square.
pub fn render(layers: &[LayerPaths], size: u32) -> Vec<u8> {
    let size = size.clamp(16, MAX_SIZE) as usize;
    let mut img = vec![0u8; size * size * 4];
    // isometric-ish view: x right, y away, z up
    let project = |x: f64, y: f64, z: f64| (x - y * 0.5, -(y * 0.35) - z);
    let (mut lo_u, mut hi_u, mut lo_v, mut hi_v) =
        (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    let mut count = 0usize;
    for l in layers {
        for p in &l.paths {
            for q in &p.points {
                let (u, v) = project(q.x, q.y, l.z);
                lo_u = lo_u.min(u);
                hi_u = hi_u.max(u);
                lo_v = lo_v.min(v);
                hi_v = hi_v.max(v);
            }
            count += p.points.len();
        }
    }
    if !(lo_u.is_finite() && hi_u > lo_u) {
        return img;
    }
    let span = (hi_u - lo_u).max(hi_v - lo_v).max(1e-6);
    let scale = (size as f64 - 4.0) / span;
    let (off_u, off_v) = (
        2.0 + (size as f64 - 4.0 - (hi_u - lo_u) * scale) / 2.0,
        2.0 + (size as f64 - 4.0 - (hi_v - lo_v) * scale) / 2.0,
    );
    let stride = count.div_ceil(MAX_SEGMENTS).max(1);
    let top = layers.last().map_or(1.0, |l| l.z.max(1e-6));
    let mut k = 0usize;
    // bottom to top: upper layers cover lower ones
    for l in layers {
        let light = 0.55 + 0.45 * (l.z / top);
        for p in &l.paths {
            let c = colour(p.kind);
            let rgb = c.map(|ch| (f64::from(ch) * light).min(255.0) as u8);
            for w in p.points.windows(2) {
                k += 1;
                if !k.is_multiple_of(stride) {
                    continue;
                }
                let [a, b] = w else { continue };
                let (au, av) = project(a.x, a.y, l.z);
                let (bu, bv) = project(b.x, b.y, l.z);
                let (x0, y0) = ((au - lo_u) * scale + off_u, (av - lo_v) * scale + off_v);
                let (x1, y1) = ((bu - lo_u) * scale + off_u, (bv - lo_v) * scale + off_v);
                let steps =
                    ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as usize).clamp(1, 4 * size);
                for s in 0..=steps {
                    let t = s as f64 / steps as f64;
                    let (x, y) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                    if x < 0.0 || y < 0.0 {
                        continue;
                    }
                    let (xi, yi) = (x as usize, y as usize);
                    if xi < size && yi < size {
                        let at = (yi * size + xi) * 4;
                        if let Some(px) = img.get_mut(at..at + 4) {
                            px.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                        }
                    }
                }
            }
        }
    }
    img
}

/// RGBA pixels as a PNG file.
pub fn png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in rgba.chunks(width as usize * 4).take(height as usize) {
        raw.push(0); // no filter
        raw.extend_from_slice(row);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crate::threemf::crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &compressed);
    chunk(b"IEND", &[]);
    out
}

/// The G-code comment block for a thumbnail.
pub fn gcode_block(layers: &[LayerPaths], size: u32) -> String {
    let size = size.clamp(16, MAX_SIZE);
    let data = crate::project::base64_encode(&png(&render(layers, size), size, size));
    let mut out = format!("; thumbnail begin {size}x{size} {}\n", data.len());
    for chunk in data.as_bytes().chunks(78) {
        out.push_str("; ");
        out.push_str(&String::from_utf8_lossy(chunk));
        out.push('\n');
    }
    out.push_str("; thumbnail end\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_geom::Point2;
    use slicecraft_render::ToolPath;

    #[test]
    fn png_has_a_valid_structure_and_the_block_wraps_it() {
        let img = vec![255u8; 4 * 4 * 4];
        let p = png(&img, 4, 4);
        assert_eq!(p.get(..8), Some(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..]));
        assert_eq!(p.get(12..16), Some(&b"IHDR"[..]));
        // the IHDR chunk's CRC matches
        let crc = u32::from_be_bytes([p[29], p[30], p[31], p[32]]);
        assert_eq!(crc, crate::threemf::crc32(&p[12..29]));
        let layers = vec![LayerPaths {
            index: 0,
            z: 0.2,
            height: 0.2,
            paths: vec![ToolPath {
                kind: PathKind::OuterPerimeter,
                points: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(20.0, 0.0),
                    Point2::new(20.0, 20.0),
                ],
                closed: true,
                tool: 0,
            }],
            inside_travel: Vec::new(),
            travel_via: Vec::new(),
        }];
        let block = gcode_block(&layers, 32);
        assert!(
            block.starts_with("; thumbnail begin 32x32 ") && block.ends_with("; thumbnail end\n")
        );
        assert!(block.lines().all(|l| l.len() <= 80));
        // something was drawn
        assert!(render(&layers, 32).chunks(4).any(|px| px[3] == 255));
    }
}
