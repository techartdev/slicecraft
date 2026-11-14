//! Binary G-code (`.bgcode`), as published in the libbgcode specification:
//! a file header, metadata blocks (INI text), optional PNG thumbnails and the
//! G-code itself in blocks of at most 64 KiB, each block followed by a CRC-32.
//!
//! Written uncompressed and unencoded, which the format allows; the reader
//! here checks structure and checksums (tests, and opening such files).

use crate::IoError;

const MAGIC: &[u8; 4] = b"GCDE";
const VERSION: u32 = 1;
const CHECKSUM_CRC32: u16 = 1;
/// Most G-code bytes in one block.
pub const MAX_BLOCK: usize = 65_535;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum BlockType {
    FileMetadata = 0,
    Gcode = 1,
    SlicerMetadata = 2,
    PrinterMetadata = 3,
    PrintMetadata = 4,
    Thumbnail = 5,
}

impl BlockType {
    fn from_u16(v: u16) -> Option<Self> {
        Some(match v {
            0 => BlockType::FileMetadata,
            1 => BlockType::Gcode,
            2 => BlockType::SlicerMetadata,
            3 => BlockType::PrinterMetadata,
            4 => BlockType::PrintMetadata,
            5 => BlockType::Thumbnail,
            _ => return None,
        })
    }
}

/// What goes into the file besides the G-code.
#[derive(Debug, Clone, Default)]
pub struct BgcodeParts<'a> {
    pub printer: Vec<(String, String)>,
    pub print: Vec<(String, String)>,
    pub slicer: Vec<(String, String)>,
    /// PNG images: (width, height, bytes).
    pub thumbnails: Vec<(u16, u16, &'a [u8])>,
}

fn ini(pairs: &[(String, String)]) -> Vec<u8> {
    let mut s = String::new();
    for (k, v) in pairs {
        let clean = |t: &str| t.replace(['\n', '\r'], " ");
        s.push_str(&clean(k));
        s.push('=');
        s.push_str(&clean(v));
        s.push('\n');
    }
    s.into_bytes()
}

fn block(out: &mut Vec<u8>, kind: BlockType, params: &[u8], data: &[u8]) {
    let start = out.len();
    out.extend_from_slice(&(kind as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // no compression
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(params);
    out.extend_from_slice(data);
    let crc = crate::threemf::crc32(out.get(start..).unwrap_or(&[]));
    out.extend_from_slice(&crc.to_le_bytes());
}

/// The whole file.
pub fn encode(gcode: &str, parts: &BgcodeParts<'_>) -> Vec<u8> {
    let mut out = Vec::with_capacity(gcode.len() + 4096);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&CHECKSUM_CRC32.to_le_bytes());
    let ini_params = 0u16.to_le_bytes(); // encoding: INI
    block(&mut out, BlockType::PrinterMetadata, &ini_params, &ini(&parts.printer));
    for (w, h, png) in &parts.thumbnails {
        let mut p = Vec::with_capacity(6);
        p.extend_from_slice(&0u16.to_le_bytes()); // PNG
        p.extend_from_slice(&w.to_le_bytes());
        p.extend_from_slice(&h.to_le_bytes());
        block(&mut out, BlockType::Thumbnail, &p, png);
    }
    block(&mut out, BlockType::PrintMetadata, &ini_params, &ini(&parts.print));
    block(&mut out, BlockType::SlicerMetadata, &ini_params, &ini(&parts.slicer));
    // G-code in whole lines, each block under the limit
    let bytes = gcode.as_bytes();
    let mut at = 0;
    let plain = 0u16.to_le_bytes(); // encoding: none
    while at < bytes.len() {
        let mut end = (at + MAX_BLOCK).min(bytes.len());
        if end < bytes.len()
            && let Some(nl) = bytes.get(at..end).and_then(|b| b.iter().rposition(|&c| c == b'\n'))
        {
            end = at + nl + 1;
        }
        block(&mut out, BlockType::Gcode, &plain, bytes.get(at..end).unwrap_or(&[]));
        at = end;
    }
    out
}

/// A read file: its blocks in order, and the G-code joined.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decoded {
    pub blocks: Vec<BlockType>,
    pub gcode: String,
    pub print_metadata: String,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// Read a `.bgcode` written without compression or encoding, checking every
/// checksum. Compressed files are refused as not supported.
pub fn decode(b: &[u8]) -> Result<Decoded, IoError> {
    let bad = IoError::Damaged;
    if b.get(..4) != Some(MAGIC.as_slice()) {
        return Err(bad("not a binary G-code file"));
    }
    if u32_at(b, 4) != Some(VERSION) {
        return Err(IoError::Unsupported("this binary G-code version"));
    }
    let crc = u16_at(b, 8).ok_or(bad("the binary G-code header is cut off"))? == CHECKSUM_CRC32;
    let mut at = 10;
    let mut out = Decoded::default();
    let mut gcode = Vec::new();
    while at < b.len() {
        let start = at;
        let kind = u16_at(b, at).and_then(BlockType::from_u16).ok_or(bad("an unknown block"))?;
        let compression = u16_at(b, at + 2).ok_or(bad("a block header is cut off"))?;
        let size = u32_at(b, at + 4).ok_or(bad("a block header is cut off"))? as usize;
        if compression != 0 {
            return Err(IoError::Unsupported("compressed binary G-code"));
        }
        at += 8;
        let params = if kind == BlockType::Thumbnail { 6 } else { 2 };
        let data_at = at + params;
        let data = b.get(data_at..data_at + size).ok_or(bad("a block is cut off"))?;
        at = data_at + size;
        if crc {
            let want = u32_at(b, at).ok_or(bad("a checksum is cut off"))?;
            if crate::threemf::crc32(b.get(start..at).unwrap_or(&[])) != want {
                return Err(bad("a block checksum does not match"));
            }
            at += 4;
        }
        match kind {
            BlockType::Gcode => gcode.extend_from_slice(data),
            BlockType::PrintMetadata => {
                out.print_metadata = String::from_utf8_lossy(data).into_owned()
            }
            _ => {}
        }
        out.blocks.push(kind);
    }
    out.gcode = String::from_utf8(gcode).map_err(|_| IoError::NotUtf8)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_blocks_in_order() {
        let mut text = String::new();
        for i in 0..20_000 {
            text.push_str(&format!("G1 X{i} Y{i} E0.01\n"));
        }
        let png = [0x89u8, b'P', b'N', b'G'];
        let parts = BgcodeParts {
            printer: vec![("printer_model".into(), "test".into())],
            print: vec![("filament used [mm]".into(), "12.3".into())],
            slicer: vec![("layer_height".into(), "0.2".into())],
            thumbnails: vec![(16, 16, &png)],
        };
        let bytes = encode(&text, &parts);
        let d = decode(&bytes);
        assert!(d.is_ok(), "{d:?}");
        let Ok(d) = d else { return };
        assert_eq!(d.gcode, text);
        assert!(d.print_metadata.contains("filament used [mm]=12.3"));
        assert_eq!(d.blocks.first(), Some(&BlockType::PrinterMetadata));
        assert_eq!(d.blocks.get(1), Some(&BlockType::Thumbnail));
        assert!(
            d.blocks.iter().filter(|b| **b == BlockType::Gcode).count() > 5,
            "split into blocks"
        );
    }

    #[test]
    fn damage_is_detected() {
        let bytes = encode("G28\n", &BgcodeParts::default());
        let mut bad = bytes.clone();
        if let Some(b) = bad.last_mut() {
            *b ^= 0xff;
        }
        assert!(decode(&bad).is_err());
        for n in 0..bytes.len() {
            let _ = decode(bytes.get(..n).unwrap_or(&[]));
        }
        assert!(decode(b"GCDE").is_err());
    }
}
