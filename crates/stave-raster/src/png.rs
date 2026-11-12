//! A minimal PNG writer for canvases (screenshots, golden images):
//! 8-bit RGBA, unfiltered rows, zlib "stored" blocks. Files are larger
//! than a real compressor makes, and nothing else is needed.

use std::vec::Vec;

use crate::Canvas;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encode `canvas` (premultiplied RGBA8, row 0 at the top) as a PNG with
/// straight alpha.
pub fn encode(canvas: &Canvas) -> Vec<u8> {
    let (w, h) = (canvas.w, canvas.h);
    let mut raw = Vec::with_capacity(h * (w * 4 + 1));
    for row in canvas.px.chunks(w * 4).take(h) {
        raw.push(0); // filter: none
        for px in row.chunks(4) {
            let a = px[3];
            if a == 0 || a == 255 {
                raw.extend_from_slice(px);
            } else {
                let un = |c: u8| ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
                raw.extend_from_slice(&[un(px[0]), un(px[1]), un(px[2]), a]);
            }
        }
    }
    let mut z = std::vec![0x78, 0x01];
    let blocks = raw.chunks(65_535).count().max(1);
    for (i, block) in raw.chunks(65_535).enumerate() {
        z.push(u8::from(i + 1 == blocks));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = std::vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_adler_match_known_values() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn header_and_size_are_right() {
        let mut c = Canvas::new(3, 2);
        c.px.iter_mut().for_each(|b| *b = 255);
        let png = encode(&c);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']);
        assert_eq!(&png[16..24], &[0, 0, 0, 3, 0, 0, 0, 2]);
    }
}
