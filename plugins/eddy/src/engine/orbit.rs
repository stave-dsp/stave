//! ORBIT — the echoes move through the stereo IMAGE, not just time:
//! each pass through the loop rotates the wet (mid, side) pair by the
//! dialed angle, so repeat n sits at n·θ around the image — center,
//! wide, inverted-center, back — an orbit with a period of 360/θ
//! repeats (a law). Rotation is orthogonal, so it never adds energy:
//! the train still decays at exactly the dialed feedback (a law).
//!
//! Stateless — the whole algorithm is this frame law over Echo's lane
//! ports (it must read BOTH lanes before either writes back, which
//! the shared frame functions can't express).

use stave_dsp::{ms_decode, ms_encode};

use super::diffusion::Diffusion;
use super::echo::{Echo, MID, SIDE};

/// One stereo frame: classic reads, rotated regeneration.
#[inline]
pub(super) fn frame(
    echo: &mut Echo,
    diffusion: &mut Diffusion,
    cx: super::FrameCtx,
    degrees: f32,
) -> (f32, f32) {
    let (m64, s64) = ms_encode(cx.l, cx.r);
    let (m_in, s_in) = (m64 as f32, s64 as f32);
    let wet_m = echo.read(MID, cx.base + cx.wobble * (1.0 - cx.spread));
    let wet_s = echo.read(SIDE, cx.base - cx.wobble * cx.spread);
    // The field rides the read pair — DIFFUSE for the ear, BLEED
    // for the loop — and the orbit rotates the REGENERATION onward.
    let ((hear_m, hear_s), (regen_m, regen_s)) =
        diffusion.couple(wet_m, wet_s, cx.diffuse, cx.bleed);
    let th = degrees.to_radians();
    let (sin, cos) = th.sin_cos();
    let rot_m = cos * regen_m - sin * regen_s;
    let rot_s = sin * regen_m + cos * regen_s;
    echo.commit(MID, m_in + rot_m * cx.fb);
    echo.commit(SIDE, s_in + rot_s * cx.fb);
    ms_decode(hear_m as f64, hear_s as f64)
}
