//! A tiny PNG generated in-process for the Android sample document.
//!
//! Our own pixels (a blue field with a red square). Nothing is copied from another product.

/// 64×48 opaque RGB PNG.
#[cfg(any(test, target_os = "android"))]
pub fn sample_png() -> Vec<u8> {
    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 48;
    let mut raw = Vec::with_capacity((1 + WIDTH as usize * 3) * HEIGHT as usize);
    for y in 0..HEIGHT {
        raw.push(0);
        for x in 0..WIDTH {
            let inside = (16..48).contains(&x) && (12..36).contains(&y);
            let (r, g, b) = if inside { (220, 48, 40) } else { ((255 * x / WIDTH) as u8, 90, (255 * y / HEIGHT) as u8) };
            raw.extend_from_slice(&[r, g, b]);
        }
    }
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&WIDTH.to_be_bytes());
    ihdr.extend_from_slice(&HEIGHT.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    push_chunk(&mut png, b"IHDR", &ihdr);
    let compressed = {
        use std::io::Write;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&raw).ok();
        encoder.finish().unwrap_or_default()
    };
    push_chunk(&mut png, b"IDAT", &compressed);
    push_chunk(&mut png, b"IEND", &[]);
    png
}

#[cfg(any(test, target_os = "android"))]
fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_bytes = Vec::with_capacity(4 + data.len());
    crc_bytes.extend_from_slice(kind);
    crc_bytes.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_bytes).to_be_bytes());
}

#[cfg(any(test, target_os = "android"))]
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_png_is_a_png_with_a_valid_header_checksum() {
        let png = sample_png();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "PNG signature");
        assert!(png.len() > 33, "IHDR chunk fits");
        let len = u32::from_be_bytes([png[8], png[9], png[10], png[11]]);
        assert_eq!(len, 13, "IHDR payload length");
        assert_eq!(&png[12..16], b"IHDR");
        let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert!(width > 1 && height > 1, "the sample has an area");
        let crc_at = 16 + 13;
        let stored = u32::from_be_bytes([png[crc_at], png[crc_at + 1], png[crc_at + 2], png[crc_at + 3]]);
        assert_eq!(stored, crc32(&png[12..crc_at]), "IHDR checksum");
        assert!(png.windows(4).any(|word| word == b"IEND"), "image end");
    }
}
