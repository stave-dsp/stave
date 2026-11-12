//! A TrueColor visual's pixel format, and the canvas in it.

use x11rb::protocol::xproto::{ImageOrder, Screen, Setup, VisualClass};

/// How a pixel is laid out in a `ZPixmap` image of the window's depth.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Format {
    pub depth: u8,
    shifts: [u32; 3],
    /// Bits outside the colour masks (alpha on a 32-bit visual).
    rest: u32,
    big_endian: bool,
}

impl Format {
    /// The root visual of `screen`, when it is 24- or 32-bit TrueColor
    /// with 8-bit channels and 32 bits per pixel (every X server in use).
    pub fn of(setup: &Setup, screen: &Screen) -> Option<Self> {
        let depth = screen.root_depth;
        let bpp = setup.pixmap_formats.iter().find(|f| f.depth == depth)?.bits_per_pixel;
        let visual = screen
            .allowed_depths
            .iter()
            .filter(|d| d.depth == depth)
            .flat_map(|d| d.visuals.iter())
            .find(|v| v.visual_id == screen.root_visual)?;
        if bpp != 32 || visual.class != VisualClass::TRUE_COLOR || visual.bits_per_rgb_value != 8 {
            return None;
        }
        Self::from_masks(depth, [visual.red_mask, visual.green_mask, visual.blue_mask], setup.image_byte_order == ImageOrder::MSB_FIRST)
    }

    /// From channel masks (each 8 contiguous bits).
    pub fn from_masks(depth: u8, masks: [u32; 3], big_endian: bool) -> Option<Self> {
        let mut shifts = [0; 3];
        for (s, m) in shifts.iter_mut().zip(masks) {
            if m == 0 || m >> m.trailing_zeros() != 0xff {
                return None;
            }
            *s = m.trailing_zeros();
        }
        let colour = masks[0] | masks[1] | masks[2];
        let rest = if depth == 32 { !colour } else { 0 };
        Some(Format { depth, shifts, rest, big_endian })
    }

    /// One pixel from premultiplied RGBA8 (shown over black: an X window
    /// of depth 24 has no alpha).
    pub fn pixel(&self, rgba: &[u8]) -> u32 {
        let [r, g, b] = self.shifts;
        (u32::from(rgba[0]) << r) | (u32::from(rgba[1]) << g) | (u32::from(rgba[2]) << b) | self.rest
    }

    /// The canvas's rows `rows` (premultiplied RGBA8, `w` px wide) as
    /// `ZPixmap` bytes, appended to `out`.
    pub fn convert(&self, px: &[u8], w: usize, rows: core::ops::Range<usize>, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(rows.len() * w * 4);
        for p in px[rows.start * w * 4..rows.end * w * 4].chunks_exact(4) {
            let v = self.pixel(p);
            out.extend_from_slice(&if self.big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
        }
    }

    /// Premultiplied RGB of one `ZPixmap` pixel (for tests reading the
    /// window back).
    pub fn rgb(&self, bytes: [u8; 4]) -> [u8; 3] {
        let v = if self.big_endian { u32::from_be_bytes(bytes) } else { u32::from_le_bytes(bytes) };
        self.shifts.map(|s| (v >> s) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_common_visual_is_bgrx_little_endian() {
        let f = Format::from_masks(24, [0xff0000, 0xff00, 0xff], false).unwrap();
        let mut out = Vec::new();
        f.convert(&[10, 20, 30, 255], 1, 0..1, &mut out);
        assert_eq!(out, [30, 20, 10, 0]);
        assert_eq!(f.rgb([30, 20, 10, 0]), [10, 20, 30]);
    }

    #[test]
    fn argb_visuals_are_opaque_and_big_endian_swaps() {
        let f = Format::from_masks(32, [0xff0000, 0xff00, 0xff], true).unwrap();
        let mut out = Vec::new();
        f.convert(&[1, 2, 3, 128], 1, 0..1, &mut out);
        assert_eq!(out, [0xff, 1, 2, 3]);
    }

    #[test]
    fn odd_masks_are_refused() {
        assert!(Format::from_masks(16, [0xf800, 0x7e0, 0x1f], false).is_none());
    }
}
