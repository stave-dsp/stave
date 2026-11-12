//! `Wet` — the wet path's last stage, at the host rate: TONE (a
//! synth-style resonant low-pass — closed lets only lows through,
//! open passes the whole band), WIDTH (the image's side level: 0 =
//! mono, the middle = as the core made it, up = wider), and a soft
//! ceiling so a maxed feedback network saturates instead of clipping
//! the host.

use stave_dsp::{ms_decode, ms_encode, Svf};

use super::path::soft;

const TONE_LO_HZ: f32 = 120.0;
const TONE_HI_HZ: f32 = 18_000.0;
const TONE_RES: f32 = 0.3;
/// The ceiling's knee: below it the stage is linear.
const CEILING: f32 = 1.5;

pub(super) struct Wet {
    tone: Svf,
}

impl Wet {
    pub(super) fn new() -> Self {
        Self { tone: Svf::new() }
    }

    pub(super) fn prepare(&mut self, host_hz: f32) {
        self.tone.prepare(host_hz);
        self.tone.reset();
    }

    #[inline]
    pub(super) fn process(&mut self, x: [f32; 2], tone: f32, width: f32) -> [f32; 2] {
        // Per frame: TONE's glide is part of the instrument (a filter
        // sweep), so the corner follows the smoothed knob exactly.
        self.tone.set(TONE_LO_HZ * (TONE_HI_HZ / TONE_LO_HZ).powf(tone), TONE_RES);
        let l = self.tone.process_lp(x[0], 0);
        let r = self.tone.process_lp(x[1], 1);
        let (m, s) = ms_encode(l, r);
        let (l, r) = ms_decode(m, s * f64::from(2.0 * width));
        [ceiling(l), ceiling(r)]
    }
}

#[inline]
fn ceiling(x: f32) -> f32 {
    if x.abs() <= CEILING {
        x
    } else {
        let over = (x.abs() - CEILING) / CEILING;
        x.signum() * CEILING * (1.0 + soft(over) * 0.5)
    }
}
