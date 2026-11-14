//! The application icon, drawn in code: a rounded blue tile with a stack of
//! printed layers shaped like a vase, and a nozzle above them. No image file
//! is shipped; the window icon and the `.ico` for installers are rendered
//! from this one description.

/// Side lengths written into the `.ico` file (px).
pub const ICO_SIZES: [u32; 5] = [16, 24, 32, 48, 256];
/// Samples per pixel along each axis, for smooth edges.
const SUPERSAMPLE: u32 = 4;
/// The largest icon drawn (px).
const MAX_SIDE: u32 = 1024;

#[derive(Clone, Copy)]
struct Rgba([f32; 4]);

fn over(dst: Rgba, src: Rgba) -> Rgba {
    let a = src.0[3] + dst.0[3] * (1.0 - src.0[3]);
    if a <= 0.0 {
        return Rgba([0.0; 4]);
    }
    let mix = |i: usize| (src.0[i] * src.0[3] + dst.0[i] * dst.0[3] * (1.0 - src.0[3])) / a;
    Rgba([mix(0), mix(1), mix(2), a])
}

/// Inside a rounded rectangle centred at (cx, cy), half sizes (hx, hy), corner radius r.
fn in_rounded(x: f32, y: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> bool {
    let dx = ((x - cx).abs() - (hx - r)).max(0.0);
    let dy = ((y - cy).abs() - (hy - r)).max(0.0);
    dx * dx + dy * dy <= r * r
}

/// The colour of the icon at a point of the unit square (y down).
fn sample(x: f32, y: f32) -> Rgba {
    let mut c = Rgba([0.0; 4]);
    // the tile, a vertical blue gradient
    if in_rounded(x, y, 0.5, 0.5, 0.47, 0.47, 0.2) {
        let t = y.clamp(0.0, 1.0);
        let top = [0.24, 0.56, 1.0];
        let bottom = [0.10, 0.36, 0.84];
        let m = |i: usize| top[i] + (bottom[i] - top[i]) * t;
        c = Rgba([m(0), m(1), m(2), 1.0]);
    } else {
        return c;
    }
    // printed layers: a vase profile, wider at the belly, narrower at the neck
    let layers = 7;
    let (y_top, y_bottom) = (0.40, 0.82);
    let pitch = (y_bottom - y_top) / layers as f32;
    for k in 0..layers {
        let t = k as f32 / (layers - 1) as f32; // 0 at the top
        let cy = y_top + pitch * (k as f32 + 0.5);
        let half = 0.17 + 0.12 * (std::f32::consts::PI * (0.15 + 0.75 * t)).sin();
        let hy = pitch * 0.36;
        if in_rounded(x, y, 0.5, cy, half, hy, hy) {
            // the newest layer, at the top, is brightest
            let shade = 1.0 - 0.10 * t;
            c = over(c, Rgba([shade, shade, shade, 0.96]));
        }
    }
    // the nozzle: a small block and a cone pointing at the top layer
    let in_block = in_rounded(x, y, 0.5, 0.17, 0.085, 0.05, 0.02);
    let cone_top = 0.21;
    let cone_tip = 0.33;
    let in_cone = y >= cone_top && y <= cone_tip && {
        let half = 0.075 * (cone_tip - y) / (cone_tip - cone_top);
        (x - 0.5).abs() <= half
    };
    if in_block || in_cone {
        c = over(c, Rgba([1.0, 0.78, 0.30, 1.0]));
    }
    c
}

/// The icon as RGBA bytes (straight alpha), `side` × `side` pixels.
pub fn rgba(side: u32) -> Vec<u8> {
    let side = side.clamp(1, MAX_SIDE);
    let n = SUPERSAMPLE;
    let mut out = Vec::with_capacity((side * side * 4) as usize);
    for py in 0..side {
        for px in 0..side {
            let mut acc = [0.0f32; 4];
            for sy in 0..n {
                for sx in 0..n {
                    let x = (px as f32 + (sx as f32 + 0.5) / n as f32) / side as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / n as f32) / side as f32;
                    let c = sample(x, y);
                    // average premultiplied, so edges do not darken
                    for (i, a) in acc.iter_mut().enumerate().take(3) {
                        *a += c.0[i] * c.0[3];
                    }
                    acc[3] += c.0[3];
                }
            }
            let count = (n * n) as f32;
            let a = acc[3] / count;
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            if a > 0.0 {
                out.extend_from_slice(&[
                    byte(acc[0] / count / a),
                    byte(acc[1] / count / a),
                    byte(acc[2] / count / a),
                    byte(a),
                ]);
            } else {
                out.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    out
}

/// A Windows `.ico` file holding the icon at every size in [`ICO_SIZES`],
/// each as a 32-bit bitmap with alpha.
pub fn ico() -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = ICO_SIZES.iter().map(|&s| (s, bitmap(s))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // icon
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (side, data) in &images {
        let dim = if *side >= 256 { 0 } else { *side as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(&data);
    }
    out
}

/// One `.ico` entry: a BITMAPINFOHEADER, BGRA rows bottom-up, and an
/// all-clear AND mask (the alpha channel does the masking).
fn bitmap(side: u32) -> Vec<u8> {
    let px = rgba(side);
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(side as i32).to_le_bytes());
    out.extend_from_slice(&((side * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    for row in (0..side).rev() {
        for col in 0..side {
            let i = ((row * side + col) * 4) as usize;
            let p = px.get(i..i + 4).unwrap_or(&[0, 0, 0, 0]);
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    let mask_row = side.div_ceil(32) * 4;
    out.extend(std::iter::repeat_n(0u8, (mask_row * side) as usize));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_has_a_tile_a_clear_corner_and_white_layers() {
        let side = 64;
        let px = rgba(side);
        assert_eq!(px.len(), (side * side * 4) as usize);
        let at = |x: u32, y: u32| {
            let i = ((y * side + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(0, 0)[3], 0, "the corner outside the rounded tile is clear");
        let edge = at(4, 32);
        assert!(edge[3] == 255 && edge[2] > edge[0], "blue tile: {edge:?}");
        // the middle of the fifth layer from the top: y = 0.40 + 0.06 * 4.5
        let layer = at(32, 43);
        assert!(layer[0] > 200 && layer[1] > 200, "a white layer: {layer:?}");
    }

    #[test]
    fn the_ico_file_lists_every_size_with_valid_offsets() {
        let f = ico();
        let count = u16::from_le_bytes([f[4], f[5]]) as usize;
        assert_eq!(count, ICO_SIZES.len());
        for k in 0..count {
            let e = 6 + 16 * k;
            let len = u32::from_le_bytes([f[e + 8], f[e + 9], f[e + 10], f[e + 11]]) as usize;
            let off = u32::from_le_bytes([f[e + 12], f[e + 13], f[e + 14], f[e + 15]]) as usize;
            assert!(off + len <= f.len());
            // each entry starts with a 40-byte bitmap header
            assert_eq!(u32::from_le_bytes([f[off], f[off + 1], f[off + 2], f[off + 3]]), 40);
        }
        assert_eq!(rgba(0).len(), 4, "a zero size is drawn as one pixel");
    }
}
