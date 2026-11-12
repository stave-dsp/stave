//! Pictures as test oracles: a canvas compared, within a tolerance, to
//! a golden PNG committed beside the tests.
//!
//! ```text
//! tests/golden/<name>.png      the expected picture (straight-alpha RGBA8)
//! target/golden-diff/<name>.*  on a mismatch: the actual picture and a diff
//! ```
//!
//! `STAVE_BLESS=1 cargo test …` writes (or rewrites) the goldens from what
//! the code draws now; review them like any other change. Goldens are
//! drawn with [`stave_raster::BoxText`] so they never depend on the fonts
//! a machine has, and kept small (1×, compressed; the gate script holds
//! the whole set to a size budget).

use std::path::{Path, PathBuf};

use stave_raster::Canvas;

/// A decoded picture: straight-alpha RGBA8, row 0 at the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

impl Picture {
    /// A canvas's pixels with straight alpha (what a PNG holds).
    pub fn of(canvas: &Canvas) -> Self {
        let mut rgba = Vec::with_capacity(canvas.pixels().len());
        for px in canvas.pixels().chunks_exact(4) {
            let a = px[3];
            if a == 0 || a == 255 {
                rgba.extend_from_slice(px);
            } else {
                let un = |c: u8| ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
                rgba.extend_from_slice(&[un(px[0]), un(px[1]), un(px[2]), a]);
            }
        }
        Picture { w: canvas.w(), h: canvas.h(), rgba }
    }
}

/// How far a picture may stray from its golden: any channel by up to
/// `channel` counts anywhere, and by more at no more than `fraction` of
/// the pixels (anti-aliasing and float order differ across machines).
#[derive(Copy, Clone, Debug)]
pub struct Tolerance {
    pub channel: u8,
    pub fraction: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Tolerance { channel: 3, fraction: 0.001 }
    }
}

/// How two same-sized pictures differ.
#[derive(Clone, Debug, PartialEq)]
pub struct Difference {
    /// The largest channel difference anywhere.
    pub max: u8,
    /// Pixels with a channel past the tolerance.
    pub over: usize,
    pub pixels: usize,
    /// |a − b| per channel, alpha forced opaque so the diff is visible.
    pub map: Picture,
}

impl Difference {
    pub fn within(&self, t: Tolerance) -> bool {
        self.over as f64 <= t.fraction * self.pixels as f64
    }
}

/// Compare two pictures of one size, counting pixels past `t.channel`.
pub fn compare(expected: &Picture, actual: &Picture, t: Tolerance) -> Option<Difference> {
    if (expected.w, expected.h) != (actual.w, actual.h) {
        return None;
    }
    let mut map = Vec::with_capacity(actual.rgba.len());
    let (mut max, mut over) = (0u8, 0usize);
    for (e, a) in expected.rgba.chunks_exact(4).zip(actual.rgba.chunks_exact(4)) {
        let d: [u8; 4] = core::array::from_fn(|c| e[c].abs_diff(a[c]));
        let m = d.into_iter().max().unwrap_or(0);
        max = max.max(m);
        over += usize::from(m > t.channel);
        map.extend_from_slice(&[d[0].max(d[3]), d[1].max(d[3]), d[2].max(d[3]), 255]);
    }
    Some(Difference { max, over, pixels: actual.w * actual.h, map: Picture { w: actual.w, h: actual.h, rgba: map } })
}

/// Assert `canvas` matches the golden `name` under `dir` (a crate's
/// `tests/golden`). With `STAVE_BLESS` set, write it instead.
pub fn assert_golden(dir: impl AsRef<Path>, name: &str, canvas: &Canvas, t: Tolerance) {
    let path = dir.as_ref().join(format!("{name}.png"));
    let actual = Picture::of(canvas);
    if std::env::var_os("STAVE_BLESS").is_some() {
        std::fs::create_dir_all(dir.as_ref()).expect("create the golden dir");
        std::fs::write(&path, encode(&actual)).expect("write the golden");
        return;
    }
    let expected = match std::fs::read(&path) {
        Ok(bytes) => decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        Err(_) => panic!("{}: no golden; run with STAVE_BLESS=1 to write it, then review it", path.display()),
    };
    let out = diff_dir(dir.as_ref());
    let keep = |what: &str, pic: &Picture| {
        let _ = std::fs::create_dir_all(&out);
        let p = out.join(format!("{name}.{what}.png"));
        let _ = std::fs::write(&p, encode(pic));
        p
    };
    match compare(&expected, &actual, t) {
        None => {
            let p = keep("actual", &actual);
            panic!(
                "{name}: {}×{} now, the golden is {}×{} (actual: {})",
                actual.w,
                actual.h,
                expected.w,
                expected.h,
                p.display()
            );
        }
        Some(d) if !d.within(t) => {
            let p = keep("actual", &actual);
            keep("diff", &d.map);
            panic!(
                "{name}: {} of {} pixels differ by more than {} (max {}); actual and diff in {} — \
                 if the change is intended, STAVE_BLESS=1 rewrites the golden",
                d.over,
                d.pixels,
                t.channel,
                d.max,
                p.parent().map_or_else(String::new, |p| p.display().to_string())
            );
        }
        Some(_) => {}
    }
}

/// Assert `canvas` matches `tests/golden/<name>.png` of the calling crate.
#[macro_export]
macro_rules! assert_golden {
    ($name:expr, $canvas:expr) => {
        $crate::image::assert_golden(
            ::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            $name,
            $canvas,
            $crate::image::Tolerance::default(),
        )
    };
}

/// Where mismatches are written: `STAVE_GOLDEN_OUT`, else
/// `target/golden-diff` of the workspace holding `dir`.
fn diff_dir(dir: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("STAVE_GOLDEN_OUT") {
        return PathBuf::from(p);
    }
    let root = dir.ancestors().find(|d| d.join("Cargo.lock").exists()).unwrap_or(dir);
    root.join("target/golden-diff")
}

// ------------------------------------------------------------- PNG

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

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = ((p - i16::from(a)).abs(), (p - i16::from(b)).abs(), (p - i16::from(c)).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Predict byte `i` of `row` under filter `f` (`prev` is the row above).
fn predict(f: u8, row: &[u8], prev: &[u8], i: usize) -> u8 {
    let a = if i >= 4 { row[i - 4] } else { 0 };
    let b = prev[i];
    let c = if i >= 4 { prev[i - 4] } else { 0 };
    match f {
        1 => a,
        2 => b,
        3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
        4 => paeth(a, b, c),
        _ => 0,
    }
}

/// Encode a picture as an 8-bit RGBA PNG: each row under the filter that
/// leaves the smallest residual, then zlib.
pub fn encode(pic: &Picture) -> Vec<u8> {
    let stride = pic.w * 4;
    let zero = vec![0u8; stride];
    let mut raw = Vec::with_capacity(pic.h * (stride + 1));
    let mut best = vec![0u8; stride];
    let mut trial = vec![0u8; stride];
    for y in 0..pic.h {
        let row = &pic.rgba[y * stride..(y + 1) * stride];
        let prev = if y == 0 { &zero[..] } else { &pic.rgba[(y - 1) * stride..y * stride] };
        let (mut best_f, mut best_cost) = (0u8, u64::MAX);
        for f in 0..5u8 {
            let mut cost = 0u64;
            for i in 0..stride {
                trial[i] = row[i].wrapping_sub(predict(f, row, prev, i));
                cost += u64::from((trial[i] as i8).unsigned_abs());
            }
            if cost < best_cost {
                (best_f, best_cost) = (f, cost);
                best.copy_from_slice(&trial);
            }
        }
        raw.push(best_f);
        raw.extend_from_slice(&best);
    }
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 10);
    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(pic.w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(pic.h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Decode an 8-bit, non-interlaced RGBA PNG (what [`encode`] and
/// `stave_raster::png` write).
pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    if bytes.len() < 8 || bytes[..8] != [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'] {
        return Err("not a PNG".into());
    }
    let (mut at, mut size, mut z) = (8, None, Vec::new());
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) as usize;
        let kind = &bytes[at + 4..at + 8];
        let data = bytes.get(at + 8..at + 8 + len).ok_or("truncated chunk")?;
        match kind {
            b"IHDR" => {
                let w = u32::from_be_bytes(data[0..4].try_into().expect("4 bytes")) as usize;
                let h = u32::from_be_bytes(data[4..8].try_into().expect("4 bytes")) as usize;
                if data[8..13] != [8, 6, 0, 0, 0] {
                    return Err("only 8-bit non-interlaced RGBA is read".into());
                }
                size = Some((w, h));
            }
            b"IDAT" => z.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    let (w, h) = size.ok_or("no IHDR")?;
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&z).map_err(|e| format!("inflate: {e:?}"))?;
    let stride = w * 4;
    if raw.len() != h * (stride + 1) {
        return Err(format!("{} bytes of pixels for {w}×{h}", raw.len()));
    }
    let mut rgba = vec![0u8; h * stride];
    let zero = vec![0u8; stride];
    for y in 0..h {
        let f = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (done, rest) = rgba.split_at_mut(y * stride);
        let prev = if y == 0 { &zero[..] } else { &done[(y - 1) * stride..] };
        let row = &mut rest[..stride];
        for i in 0..stride {
            row[i] = src[i].wrapping_add(predict(f, row, prev, i));
        }
    }
    Ok(Picture { w, h, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(w: usize, h: usize) -> Picture {
        let rgba = (0..w * h).flat_map(|i| [(i * 7) as u8, (i / 3) as u8, 200, if i % 5 == 0 { 0 } else { 255 }]).collect();
        Picture { w, h, rgba }
    }

    #[test]
    fn encode_then_decode_is_lossless() {
        let p = pattern(37, 11);
        assert_eq!(decode(&encode(&p)).unwrap(), p);
    }

    #[test]
    fn the_rasters_own_png_reads_back() {
        let mut c = Canvas::new(5, 3);
        c.pixels_mut().iter_mut().enumerate().for_each(|(i, b)| *b = if i % 4 == 3 { 255 } else { i as u8 });
        assert_eq!(decode(&stave_raster::png::encode(&c)).unwrap(), Picture::of(&c));
    }

    #[test]
    fn compare_counts_what_strays_past_the_tolerance() {
        let a = pattern(10, 10);
        let mut b = a.clone();
        b.rgba[0] = b.rgba[0].wrapping_add(2);
        b.rgba[40] = b.rgba[40].wrapping_add(50);
        let d = compare(&a, &b, Tolerance::default()).unwrap();
        assert_eq!((d.max, d.over), (50, 1));
        assert!(!d.within(Tolerance::default()));
        assert!(d.within(Tolerance { channel: 3, fraction: 0.01 }));
        assert!(compare(&a, &pattern(10, 9), Tolerance::default()).is_none());
    }
}
