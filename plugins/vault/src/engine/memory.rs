//! THE REMEMBERING FIELD — the MEMORY topology: the tail is not
//! delayed copies of the input, it is THE CHORD STILL RINGING IN
//! THE ROOM. A dense bank of resonators (eighteen per octave,
//! seven octaves) charges from the program; sustained tones wake
//! the resonators near their pitch and keep sounding after the
//! source stops. Transients barely charge anything — that is the
//! driven-gain-flat physics (k = 2(1−r)·sin(w): an impulse cannot
//! integrate a high-Q body), and it is the point: a freeze/cloud
//! machine with ZERO transient smear, because there is nothing to
//! smear with.
//!
//! THE ASCEND LADDER: each resonator leaks a little of its ring
//! into CHARGING the resonator one octave up. As the memory
//! sustains, its energy climbs — the remembered chord transposes
//! itself. Shimmer with no pitch shifter, and unconditionally
//! stable: the ladder only goes up, and off the top it goes out.
//!
//! The shared dials, reinterpreted: DECAY is how long the memory
//! holds; LOW/HIGH ENERGY bend that hold across the spectrum;
//! TONE tilts the memory's brightness; DIFFUSE smears the strike
//! before it charges; SIZE slides the whole grid down (a bigger
//! memory remembers deeper); WIDTH spreads the per-resonator
//! pickups (0 = mono, by geometry).

use stave_dsp::primitives::random::{unit_f32, wang_hash};
use stave_dsp::{t60_gain, DelayLine};

/// Eighteen per octave x seven octaves, 55 Hz up.
const PER_OCT: usize = 18;
const OCTAVES: usize = 7;
const N: usize = PER_OCT * OCTAVES;
const F0: f32 = 55.0;

/// Input diffusion (the plate's lesson): the strike smears before
/// it charges, so even the charging is soft-handed.
const IN_DIFF: usize = 4;
const IN_DIFF_MS: [f32; IN_DIFF] = [1.31, 3.11, 5.77, 9.13];
const IN_DIFF_G: f32 = 0.62;

/// Charge level (driven-gain-flat scale) and output level.
const CHARGE: f32 = 5.0;
const PICKUP: f32 = 0.6;

struct Res {
    a1: f32,
    a2: f32,
    /// Slow frequency drift (the cloud-maker): a STATIC dense
    /// sine bank is a line spectrum — the metal meter read
    /// flatness 0.000, a bell; breathing lines become bands.
    drift_ph: f32,
    drift_rate: f32,
    /// Per-source charge couplings (true stereo: L and R wake the
    /// memory differently).
    k_a: f32,
    k_b: f32,
    y1: f32,
    y2: f32,
    /// Pickup weights (WIDTH spreads them; glided per sample).
    w_l: f32,
    w_l_step: f32,
    w_r: f32,
    w_r_step: f32,
}

pub(super) struct Memory {
    res: [Res; N],
    in_diff: [[DelayLine; IN_DIFF]; 2],
    in_len: [usize; IN_DIFF],
    /// THE CLOUD: a deep slow 4-voice modulated-delay smear on
    /// the bank's output — a discrete resonator bank is a LINE
    /// SPECTRUM (the metal meter read flatness 0.000, a bell);
    /// modulated delays turn each held partial into a breathing
    /// band. DIFFUSE is the cloud amount.
    cloud: [DelayLine; 2],
    cloud_ph: [f32; 4],
    cloud_rate: [f32; 4],
    cloud_mix: f32,
    cloud_spread: f32,
    /// The ladder's per-sample leak (from ASCEND).
    k_up: f32,
    sr: f32,
    cold: bool,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            res: core::array::from_fn(|i| Res {
                a1: 0.0,
                a2: 0.0,
                drift_ph: (i as f32) * 2.399_963,
                drift_rate: 0.05 + 0.22 * ((i as f32 * 0.618_034) % 1.0),
                k_a: 0.0,
                k_b: 0.0,
                y1: 0.0,
                y2: 0.0,
                w_l: 0.7,
                w_l_step: 0.0,
                w_r: 0.7,
                w_r_step: 0.0,
            }),
            in_diff: core::array::from_fn(|_| core::array::from_fn(|_| DelayLine::new())),
            in_len: [48; IN_DIFF],
            cloud: [DelayLine::new(), DelayLine::new()],
            cloud_ph: [0.3, 1.7, 3.1, 4.9],
            cloud_rate: [0.183, 0.295, 0.410, 0.522],
            cloud_mix: 0.5,
            cloud_spread: 1.0,
            k_up: 0.0,
            sr: 48_000.0,
            cold: true,
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        for (ch, chain) in self.in_diff.iter_mut().enumerate() {
            for (k, d) in chain.iter_mut().enumerate() {
                let det = if ch == 0 { 1.0 } else { 1.07 };
                self.in_len[k] = (IN_DIFF_MS[k] * det * 0.001 * sr).round() as usize;
                d.prepare(self.in_len[k] + 8);
            }
        }
        for c in self.cloud.iter_mut() {
            c.prepare((0.05 * sr) as usize + 16);
        }
        self.reset();
        self.cold = true;
    }

    pub fn reset(&mut self) {
        for (i, r) in self.res.iter_mut().enumerate() {
            r.y1 = 0.0;
            r.y2 = 0.0;
            r.drift_ph = (i as f32) * 2.399_963;
            r.w_l_step = 0.0;
            r.w_r_step = 0.0;
        }
        for chain in self.in_diff.iter_mut() {
            for d in chain.iter_mut() {
                d.reset();
            }
        }
        for c in self.cloud.iter_mut() {
            c.reset();
        }
        self.cloud_ph = [0.3, 1.7, 3.1, 4.9];
    }

    /// Block-rate: aim every resonator. `t60` is the memory's hold;
    /// `lo`/`hi_damp` bend it (low = the LOW ENERGY multiplier
    /// below ~350 Hz; hi_damp = the HIGH ENERGY dial 0..1, eating
    /// the top progressively); `tone` tilts pickup brightness;
    /// `size` slides the grid down; `width` spreads the pickups;
    /// `ascend` sets the ladder's leak.
    #[allow(clippy::too_many_arguments)]
    pub fn tune(
        &mut self,
        size: f32,
        t60: f32,
        lo: f32,
        hi_damp: f32,
        tone: f32,
        diffuse: f32,
        width: f32,
        ascend: f32,
        frames: usize,
    ) {
        let n = frames.max(1) as f32;
        let block_s = n / self.sr;
        // A bigger memory remembers DEEPER: the whole grid slides
        // down with size (an octave down at size 4).
        let f_scale = 1.0 / size.clamp(0.3, 3.0).sqrt();
        self.cloud_mix = (diffuse / 0.7).clamp(0.0, 1.0);
        self.cloud_spread = (width * 0.5).clamp(0.0, 1.0);
        for (v, ph) in self.cloud_ph.iter_mut().enumerate() {
            *ph = (*ph + core::f32::consts::TAU * self.cloud_rate[v] * block_s)
                % core::f32::consts::TAU;
        }
        // The ladder: enough leak that a held note audibly climbs
        // in a couple of seconds at full ASCEND.
        self.k_up = 2.2e-3 * ascend * ascend;
        let spread = (width * 0.5).clamp(0.0, 1.0);
        let tone_tilt = tone.clamp(-1.0, 1.0);
        for (i, r) in self.res.iter_mut().enumerate() {
            r.drift_ph = (r.drift_ph + core::f32::consts::TAU * r.drift_rate * block_s)
                % core::f32::consts::TAU;
            let drift = 1.0 + 0.002 * r.drift_ph.sin();
            let f = F0 * f_scale * drift * 2f32.powf(i as f32 / PER_OCT as f32);
            let w = core::f32::consts::TAU * (f / self.sr).min(0.45);
            // The hold, bent per band: LOW ENERGY below 350 Hz,
            // HIGH ENERGY eats progressively above 1 kHz.
            let lo_bend = if f < 350.0 { lo } else { 1.0 };
            let hi_bend = if f > 1_000.0 {
                let octs = (f / 1_000.0).log2();
                (1.0 - 0.28 * hi_damp * octs).max(0.06)
            } else {
                1.0
            };
            let t = (t60 * lo_bend * hi_bend).max(0.05);
            let pole = t60_gain(1.0, t * self.sr);
            r.a1 = 2.0 * pole * w.cos();
            r.a2 = -pole * pole;
            // Driven-gain-flat charge (the modal lesson) with a
            // slight per-resonator hash so no two charge alike.
            let g = CHARGE * (0.8 + 0.4 * unit_f32(wang_hash(i as u32)));
            let k = 2.0 * (1.0 - pole) * w.sin() * g;
            // True stereo: L and R wake each resonator with their
            // own coupling (hash-fixed positions on the "string").
            let th = unit_f32(wang_hash(i as u32 ^ 0x9E37)) * core::f32::consts::TAU;
            r.k_a = k * (0.6 + 0.4 * th.sin());
            r.k_b = k * (0.6 + 0.4 * th.cos());
            // The pickups: hash pan, spread by WIDTH, tone-tilted.
            let pan = (unit_f32(wang_hash(i as u32 ^ 0x51DE)) * 2.0 - 1.0) * spread;
            let bright = 2f32.powf(tone_tilt * (i as f32 / N as f32 - 0.5) * 2.0);
            let wl = (0.5 * (1.0 + pan)).sqrt() * PICKUP * bright;
            let wr = (0.5 * (1.0 - pan)).sqrt() * PICKUP * bright;
            if self.cold {
                r.w_l = wl;
                r.w_l_step = 0.0;
                r.w_r = wr;
                r.w_r_step = 0.0;
            } else {
                r.w_l_step = (wl - r.w_l) / n;
                r.w_r_step = (wr - r.w_r) / n;
            }
        }
        self.cold = false;
    }

    /// One sample: smear the strike, charge the memory, let the
    /// ladder climb, read the pickups.
    #[inline]
    pub fn tick(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let mut inj = [in_l, in_r];
        for (ch, chain) in self.in_diff.iter_mut().enumerate() {
            let mut v = inj[ch];
            for (k, d) in chain.iter_mut().enumerate() {
                let y = d.read_int(self.in_len[k]) - IN_DIFF_G * v;
                d.write(v + IN_DIFF_G * y);
                d.advance();
                v = y;
            }
            inj[ch] = v;
        }
        let (xl, xr) = (inj[0], inj[1]);
        let k_up = self.k_up;
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        for i in 0..N {
            // The ladder feeds THIS resonator from the one an
            // octave below — RECTIFIED: a linear leak cannot
            // transpose (the 220 ring drives the 440 body at 220,
            // off-resonance — measured a dead ladder), but |y| is
            // rich in 2f while staying amplitude-LINEAR, so the
            // octave breathes with the memory. Deliberate, honest
            // nonlinearity — the arc's kind. Feed-forward up the
            // grid: it can never loop.
            let up = if i >= PER_OCT {
                k_up * self.res[i - PER_OCT].y1.abs()
            } else {
                0.0
            };
            let r = &mut self.res[i];
            let y = r.a1 * r.y1 + r.a2 * r.y2 + r.k_a * xl + r.k_b * xr + up;
            r.y2 = r.y1;
            r.y1 = y;
            out_l += r.w_l * y;
            out_r += r.w_r * y;
            r.w_l += r.w_l_step;
            r.w_r += r.w_r_step;
        }
        // THE CLOUD: two deep slow voices per channel — each held
        // partial becomes a breathing band.
        self.cloud[0].write(out_l);
        self.cloud[0].advance();
        self.cloud[1].write(out_r);
        self.cloud[1].advance();
        if self.cloud_mix > 1e-4 {
            let d0 = (16.0 + 13.0 * self.cloud_ph[0].sin()) * 0.001 * self.sr;
            let d1 = (24.0 + 18.0 * self.cloud_ph[1].sin()) * 0.001 * self.sr;
            let d2 = (19.0 + 15.0 * self.cloud_ph[2].sin()) * 0.001 * self.sr;
            let d3 = (28.0 + 20.0 * self.cloud_ph[3].sin()) * 0.001 * self.sr;
            // The R voices CONVERGE onto the L delays as WIDTH
            // closes (independent voices broke width-0 mono:
            // corr 0.79 measured).
            let sp = self.cloud_spread;
            let dr0 = d0 + sp * (d2 - d0);
            let dr1 = d1 + sp * (d3 - d1);
            let cl = 0.5 * (self.cloud[0].read(d0) + self.cloud[0].read(d1));
            let cr = 0.5 * (self.cloud[1].read(dr0) + self.cloud[1].read(dr1));
            let m = self.cloud_mix;
            (out_l + m * (cl - out_l), out_r + m * (cr - out_r))
        } else {
            (out_l, out_r)
        }
    }
}
