//! `ClockDomain` — the bridge between the host's sample rate and the
//! core's CLOCK. The pedal's wet path is a 32 kHz DSP whose clock can
//! be pulled: hold Tap/Shift and the whole chip slows or speeds up,
//! so everything already in its buffers drops or rises in pitch and
//! every time stretches. That is modelled literally — the core ticks
//! at `CORE_HZ × clock` in real time, and this bridge resamples in
//! and out at whatever ratio that makes against the host:
//!
//! * IN: a 4th-order low-pass at 0.45 × the slower of the two rates
//!   (the anti-alias filter in front of the converter), then each
//!   core tick samples the host signal at its own instant (linear
//!   interpolation between host frames).
//! * OUT: Hermite interpolation of the core's output history at each
//!   host instant, then the same 4th-order low-pass (the
//!   reconstruction filter) — at clock 1× that is the pedal's ~14 kHz
//!   wet bandwidth; dived to 0.5×, the wet goes dark with it.

use stave_dsp::Svf;

use super::programs::CORE_HZ;

/// Butterworth-4 as two SVF sections (Q 0.54 and 1.31 → Svf resonance).
const RES: [f32; 2] = [0.076, 0.617];

pub(super) struct ClockDomain {
    host_hz: f32,
    /// Core-tick position owed, fractional, in [0, 1).
    acc: f32,
    prev_in: [f32; 2],
    /// Last four core outputs per lane, oldest first.
    hist: [[f32; 4]; 2],
    aa_in: [Svf; 2],
    aa_out: [Svf; 2],
}

impl ClockDomain {
    pub(super) fn new() -> Self {
        Self {
            host_hz: 48_000.0,
            acc: 0.0,
            prev_in: [0.0; 2],
            hist: [[0.0; 4]; 2],
            aa_in: [Svf::new(), Svf::new()],
            aa_out: [Svf::new(), Svf::new()],
        }
    }

    pub(super) fn prepare(&mut self, host_hz: f32) {
        self.host_hz = host_hz;
        for f in self.aa_in.iter_mut().chain(self.aa_out.iter_mut()) {
            f.prepare(host_hz);
        }
        self.reset();
        self.set_clock(1.0);
    }

    pub(super) fn reset(&mut self) {
        self.acc = 0.0;
        self.prev_in = [0.0; 2];
        self.hist = [[0.0; 4]; 2];
        for f in self.aa_in.iter_mut().chain(self.aa_out.iter_mut()) {
            f.reset();
        }
    }

    /// Re-voice the converter filters for a clock (block rate).
    pub(super) fn set_clock(&mut self, clock: f32) {
        let corner = 0.45 * (CORE_HZ * clock).min(self.host_hz);
        for (i, f) in self.aa_in.iter_mut().enumerate() {
            f.set(corner, RES[i]);
        }
        for (i, f) in self.aa_out.iter_mut().enumerate() {
            f.set(corner, RES[i]);
        }
    }

    /// One host frame. `clock` is the core speed (1 = 32 kHz);
    /// `core` runs one tick per call and may run 0..n times.
    #[inline]
    pub(super) fn process(
        &mut self,
        x: [f32; 2],
        clock: f32,
        mut core: impl FnMut([f32; 2]) -> [f32; 2],
    ) -> [f32; 2] {
        let mut xi = [0.0; 2];
        for c in 0..2 {
            let s = self.aa_in[0].process_lp(x[c], c);
            xi[c] = self.aa_in[1].process_lp(s, c);
        }
        let ratio = CORE_HZ * clock / self.host_hz;
        let start = self.acc;
        let end = start + ratio;
        let mut k = 1.0;
        while k <= end {
            let f = (k - start) / ratio;
            let input = [
                self.prev_in[0] + (xi[0] - self.prev_in[0]) * f,
                self.prev_in[1] + (xi[1] - self.prev_in[1]) * f,
            ];
            let y = core(input);
            for c in 0..2 {
                let h = &mut self.hist[c];
                *h = [h[1], h[2], h[3], y[c]];
            }
            k += 1.0;
        }
        self.acc = end - (k - 1.0);
        self.prev_in = xi;

        // Read two ticks behind the newest, at the fractional offset —
        // four real neighbours for the Hermite, ~60 µs of wet latency.
        let t = self.acc;
        let mut out = [0.0; 2];
        for c in 0..2 {
            let h = self.hist[c];
            let y = hermite(h[0], h[1], h[2], h[3], t);
            let s = self.aa_out[0].process_lp(y, c);
            out[c] = self.aa_out[1].process_lp(s, c);
        }
        out
    }
}

/// 4-point, 3rd-order Hermite between `x1` and `x2` at `t` ∈ [0, 1).
#[inline]
fn hermite(x0: f32, x1: f32, x2: f32, x3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (x2 - x0);
    let c2 = x0 - 2.5 * x1 + 2.0 * x2 - 0.5 * x3;
    let c3 = 0.5 * (x3 - x0) + 1.5 * (x1 - x2);
    ((c3 * t + c2) * t + c1) * t + x1
}
