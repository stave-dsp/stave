//! PushPull — the class-AB power pair: two identical single-ended
//! halves driven in antiphase, sharing a cathode.
//!
//! Every high-gain reference head (Rectifier, Uberschall, 5150,
//! SLO) is a class-AB push-pull power section; none of the feel
//! those amps have survives without it (the atlas's class-A null:
//! a class-A stage's average plate current is constant by design,
//! so nothing upstream can drive per-hit supply movement).
//!
//! Each half is the triode's own transfer, tabulated once over grid
//! volts and antialiased by its own ADAA at the pair's live bias:
//! audio y = f(b + x) − f(b − x) with b = class point − cathode drift
//! − any external offset, so a moving bias is evaluated exactly. The
//! plate DRAW i(b + x) + i(b − x) RISES with drive in class AB (idle
//! current is small, conduction peaks are not): the honest
//! `PowerSupply` load class A could never provide. Bias depth sets the class
//! (0 = class A, deeper = AB with a crossover region); a declared
//! imbalance leaks even harmonics back in (real pairs are only
//! approximately matched — character, chosen not inherited). Under
//! `Bias::Cathode` the shared cathode is the circuit's RC on the pair's
//! cathode current: a class-AB pair driven hard draws more, the cathode
//! rises, the class point cools.
//!
//! Level policy: Physical from birth — drive scales grid volts
//! honestly into a FIXED ceiling; small-signal gain and ceiling
//! are REPORTED (`small_signal_gain_db`, `ceiling_db`).

use crate::behavioral::adaa::{Adaa, AdaaOrder};
use crate::behavioral::curve::Curve;
use crate::circuit::devices::triode::koren::Koren;
use crate::circuit::methods::newton::{NewtonSettings, NewtonStats, Solved};
use crate::circuit::topologies::triode_stage::{
    core_transfer_at, grid_amps_at, grid_volts_at, solve_load_line, Bias, TriodeParams,
};
use crate::kernel::node::{Stage, Tail};

/// Grid-volt domain of the tabulated halves (absolute): deep cutoff to
/// well into grid conduction, for any class point and drive.
const VG_MIN: f64 = -40.0;
const VG_MAX: f64 = 16.0;
/// Input span (grid volts either side of the class point) over which
/// the composed transfer is normalized and its flank gain read.
const X_SPAN: f64 = 16.0;

pub struct PushPull {
    p: TriodeParams,
    /// Plate swing (quiescent minus plate) of one half vs grid volts.
    half: Curve,
    /// Plate current of one half vs grid volts (the supply's draw).
    plate_amps: Curve,
    /// Cathode current (plate + grid) of one half vs grid volts.
    cathode_amps: Curve,
    adaa_pos: Adaa,
    adaa_neg: Adaa,
    /// Volts COLDER than the tube's own bias each half idles at:
    /// 0 = class A overlap, 2..3 = deep AB (crossover appears).
    bias_cold: f64,
    /// The class point, grid volts.
    class_point: f64,
    /// Fractional gain mismatch of the negative half (real pairs
    /// are approximately matched; evens leak in proportion).
    imbalance: f64,
    grid_drive: f64,
    out_scale: f64,
    supply_scale: f64,
    gain_q: f64,
    idle_plate_amps: f64,
    draw_last: f32,
    drive_db_last: f32,
    /// Loop-gain target (dB of small-signal gain reduction at
    /// stiff supply); the divider beta is solved from the report.
    nfb_db: f32,
    nfb_beta: f64,
    /// Warm start for the implicit feedback solve.
    u_prev: f64,
    stats: NewtonStats,
    bias: Bias,
    /// Shared cathode resistor and its quiescent voltage.
    rk: f64,
    vk0: f64,
    vk: f64,
    cathode_coeff: f64,
    /// Volts colder from an outside source (a product's own effect).
    bias_offset: f64,
    sample_rate: f32,
}

impl Default for PushPull {
    fn default() -> Self {
        Self::new()
    }
}

/// A push-pull pair's grids are driven by the phase inverter, not an
/// input jack: a long-tailed pair's plate output is about rp ‖ Ra, ~40 kΩ
/// for a 12AX7 into 82–100 kΩ.
const PHASE_INVERTER_OHMS: f64 = 40e3;

fn driven_pair(t: Koren) -> TriodeParams {
    TriodeParams {
        source_ohms: PHASE_INVERTER_OHMS,
        ..TriodeParams::for_tube(t)
    }
}

impl PushPull {
    pub fn new() -> Self {
        let mut pp = Self {
            p: driven_pair(Koren::T12AX7),
            half: Curve::flat(),
            plate_amps: Curve::flat(),
            cathode_amps: Curve::flat(),
            adaa_pos: Adaa::new(AdaaOrder::First),
            adaa_neg: Adaa::new(AdaaOrder::First),
            bias_cold: 2.0,
            class_point: 0.0,
            imbalance: 0.0,
            grid_drive: 1.0,
            out_scale: 1.0,
            supply_scale: 1.0,
            gain_q: 1.0,
            idle_plate_amps: 1.0,
            draw_last: 1.0,
            drive_db_last: f32::NAN,
            nfb_db: 0.0,
            nfb_beta: 0.0,
            u_prev: 0.0,
            stats: NewtonStats::default(),
            bias: Bias::Fixed,
            rk: 0.0,
            vk0: 0.0,
            vk: 0.0,
            cathode_coeff: 0.0,
            bias_offset: 0.0,
            sample_rate: 48_000.0,
        };
        pp.rebuild();
        pp
    }

    pub fn set_tube(&mut self, t: Koren) {
        let p = driven_pair(t);
        if p.tube.mu != self.p.tube.mu || p.r_plate != self.p.r_plate {
            self.p = p;
            self.rebuild();
        }
    }

    /// Class setting, volts colder than the tube's own bias (negative =
    /// hotter). A product's knob range belongs to its parameter.
    pub fn set_class_ab(&mut self, volts_cold: f64) {
        debug_assert!(volts_cold.is_finite());
        let v = volts_cold;
        if (v - self.bias_cold).abs() > 1e-9 {
            self.bias_cold = v;
            self.rebuild();
        }
    }

    /// Active bias-class setting, in volts colder than the tube's bias.
    /// Exposed for design inspection and measurement; live drive controls
    /// should not silently alter it.
    pub fn class_ab(&self) -> f64 {
        self.bias_cold
    }

    /// Pair mismatch (fraction; 0 = ideal matched pair). Both halves
    /// conduct only for |frac| < 1.
    pub fn set_imbalance(&mut self, frac: f64) {
        debug_assert!(
            frac.abs() < 1.0,
            "imbalance {frac}: a half would not conduct"
        );
        let v = frac;
        if (v - self.imbalance).abs() > 1e-9 {
            self.imbalance = v;
            self.rebuild();
        }
    }

    /// Grid drive in dB re 1 V-per-unit input (idempotent).
    pub fn set_drive_db(&mut self, db: f32) {
        if db == self.drive_db_last {
            return;
        }
        self.drive_db_last = db;
        self.grid_drive = 10f64.powf(f64::from(db) / 20.0);
        self.update_beta();
    }

    /// B+ relative to its settled idle voltage (1 = stiff/idle): moves ceiling
    /// and gain together — the operating point the player feels.
    pub fn set_supply_scale(&mut self, s: f32) {
        debug_assert!(s > 0.0, "supply scale {s}: B+ must be positive");
        self.supply_scale = f64::from(s);
    }

    /// Normalized plate draw at the last processed sample (1 = the
    /// pair's own idle). It is a supply's load; in class AB this
    /// RISES with drive — the per-hit sag driver.
    pub fn draw(&self) -> f32 {
        self.draw_last
    }

    /// How the pair is biased: `Fixed` from a bias supply, or
    /// `Cathode` through a shared, bypassed cathode resistor whose
    /// voltage follows the pair's cathode current at Rk·Ck.
    pub fn set_bias(&mut self, bias: Bias) {
        self.bias = bias;
        self.update_cathode_coeff();
    }

    pub fn cathode_ohms(&self) -> f64 {
        self.rk
    }

    /// The shared cathode's drift from rest (volts, positive = colder).
    pub fn cathode_shift(&self) -> f64 {
        match self.bias {
            Bias::Fixed => 0.0,
            Bias::Cathode { .. } => self.vk - self.vk0,
        }
    }

    /// Volts colder applied from outside the circuit (a product's own
    /// effect, e.g. a declared bias walk); adds to the cathode's drift.
    pub fn set_bias_offset(&mut self, volts_colder: f64) {
        self.bias_offset = volts_colder;
    }

    fn update_cathode_coeff(&mut self) {
        self.cathode_coeff = match self.bias {
            Bias::Fixed => 0.0,
            Bias::Cathode { bypass_farads } => {
                1.0 - (-1.0 / (self.rk * bypass_farads * f64::from(self.sample_rate))).exp()
            }
        };
    }

    /// Global negative feedback around the pair, expressed as the
    /// loop's small-signal gain reduction (dB) at stiff supply —
    /// the divider beta is solved from the REPORTED flank gain, so
    /// the report machinery stays the truth closed-loop. 0 = open
    /// loop, bit-identical to no feedback. The reactivity is the
    /// loop's own state: as the pair saturates its incremental
    /// gain collapses, the loop loses grip, and the knee arrives
    /// all at once; a sagged supply lowers the loop gain WITH the
    /// rail, so a drooped amp genuinely plays looser.
    pub fn set_feedback_db(&mut self, db: f32) {
        debug_assert!(
            db >= 0.0,
            "feedback {db} dB: negative feedback reduces gain"
        );
        let v = db;
        if v != self.nfb_db {
            self.nfb_db = v;
            self.update_beta();
        }
    }

    /// TRUE static peak response at the current frame (bias at its
    /// current drift, feedback loop solved) — the exact curve the
    /// audio rides, for honest slope staging upstream.
    pub fn static_peak_out(&self, in_peak: f64) -> f64 {
        let x0 = in_peak;
        let u = if self.nfb_beta > 0.0 {
            self.solve_feedback(x0, 0.0).value
        } else {
            x0
        };
        self.s_at(u).abs()
    }

    /// S(u) and its slope, analytic from the half's table.
    fn s_and_slope_at(&self, u: f64) -> (f64, f64) {
        let (bias, scale) = (self.live_bias(), self.out_scale * self.supply_scale);
        let x = u * self.grid_drive;
        let (pos, pos_slope) = self.half.f_and_slope_at(bias + x);
        let (neg, neg_slope) = self.half.f_and_slope_at(bias - x);
        let m = 1.0 + self.imbalance;
        (
            (pos - m * neg) * scale,
            (pos_slope + m * neg_slope) * self.grid_drive * scale,
        )
    }

    /// The feedback node, solved implicitly (zero delay — a z⁻¹ loop goes
    /// unstable past unity loop gain): u + β·S(u) = x. S is monotone, so
    /// the root is unique.
    fn solve_feedback(&self, x: f64, start: f64) -> Solved {
        let beta = self.nfb_beta;
        NewtonSettings::DEFAULT.solve_scalar(
            start,
            |u| {
                let (s, slope) = self.s_and_slope_at(u);
                (u + beta * s - x, 1.0 + beta * slope)
            },
            |new, _| new,
        )
    }

    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    fn update_beta(&mut self) {
        let g = (self.gain_q * self.grid_drive * self.out_scale).max(1e-9);
        self.nfb_beta = if self.nfb_db > 0.0 {
            (10f64.powf(f64::from(self.nfb_db) / 20.0) - 1.0) / g
        } else {
            0.0
        };
    }

    pub fn small_signal_gain_db(&self) -> f32 {
        let g = (self.gain_q * self.grid_drive * self.out_scale * self.supply_scale).max(1e-9);
        let g_cl = g / (1.0 + self.nfb_beta * g);
        (20.0 * g_cl.log10()) as f32
    }

    /// Output ceiling (dBFS) at the current supply (0 dBFS stiff).
    pub fn ceiling_db(&self) -> f32 {
        (20.0 * self.supply_scale.max(1e-9).log10()) as f32
    }

    /// The pair's composed transfer at bias `b`, grid swing `x`.
    #[inline]
    fn pair_at(&self, b: f64, x: f64) -> f64 {
        self.half.f_at(b + x) - (1.0 + self.imbalance) * self.half.f_at(b - x)
    }

    fn rebuild(&mut self) {
        let p = self.p;
        let vq = p.grid_bias - self.bias_cold;
        self.class_point = vq;
        let vpk0 = solve_load_line(&p, grid_volts_at(&p, vq), p.supply_v * 0.7);
        self.half
            .rebuild(VG_MIN, VG_MAX, vq, |vg| core_transfer_at(&p, vpk0, vg));
        let plate =
            |vg: f64| (p.supply_v - solve_load_line(&p, grid_volts_at(&p, vg), vpk0)) / p.r_plate;
        self.plate_amps.rebuild(VG_MIN, VG_MAX, vq, plate);
        self.cathode_amps
            .rebuild(VG_MIN, VG_MAX, vq, |vg| plate(vg) + grid_amps_at(&p, vg));
        // Normalize the ceiling to 0 dBFS at stiff supply.
        let span = |i: usize| -X_SPAN + 2.0 * X_SPAN * i as f64 / 256.0;
        let m = (0..=256)
            .map(|i| self.pair_at(vq, span(i)).abs())
            .fold(0.0, f64::max);
        self.out_scale = 1.0 / m.max(1e-9);
        // FLANK gain, not crossover gain: in deep AB the slope at
        // x = 0 is the dead zone — the one place the curve's slope
        // is meaningless for staging (the anvil integration drove
        // the pair by that number and the wall went clean). The
        // report is the max slope the signal actually rides.
        let d = X_SPAN / 256.0;
        self.gain_q = (1..256)
            .map(|i| {
                let x = span(i);
                (self.pair_at(vq, x + d) - self.pair_at(vq, x - d)).abs() / (2.0 * d)
            })
            .fold(0.0, f64::max);
        self.idle_plate_amps = 2.0 * plate(vq).max(1e-12);
        // The shared cathode makes the class point: Rk carries both
        // halves' idle cathode current.
        let idle_cathode_amps = 2.0 * self.cathode_amps.f_at(vq);
        self.vk0 = -vq;
        self.rk = self.vk0 / idle_cathode_amps.max(1e-12);
        self.vk = self.vk0;
        self.update_cathode_coeff();
        self.adaa_pos.reset();
        self.adaa_neg.reset();
        self.u_prev = 0.0;
        self.update_beta();
    }

    /// The live bias: class point less the cathode's drift and any
    /// external offset.
    #[inline]
    fn live_bias(&self) -> f64 {
        self.class_point - self.cathode_shift() - self.bias_offset
    }

    /// The static transfer at the current bias/supply frame (idle slow
    /// states, no feedback) — the curve the ADAA path rides.
    pub fn static_transfer(&self, u: f64) -> f64 {
        self.s_at(u)
    }

    /// The static transfer at the current bias/supply frame — the
    /// implicit feedback solve's S(u).
    #[inline]
    fn s_at(&self, u: f64) -> f64 {
        self.pair_at(self.live_bias(), u * self.grid_drive) * self.out_scale * self.supply_scale
    }
}

impl Stage for PushPull {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.update_cathode_coeff();
        self.reset();
    }

    fn reset(&mut self) {
        self.adaa_pos.reset();
        self.adaa_neg.reset();
        self.draw_last = 1.0;
        self.u_prev = 0.0;
        self.vk = self.vk0;
    }

    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let x0 = f64::from(input);
        let u = if self.nfb_beta > 0.0 {
            let solved = self.solve_feedback(x0, self.u_prev);
            self.stats.record_solve(&solved);
            self.u_prev = solved.value;
            solved.value
        } else {
            x0
        };
        // Each half antialiased on its own, at this sample's bias (the
        // cathode commits its drift after, between difference quotients).
        let bias = self.live_bias();
        let scale = self.out_scale * self.supply_scale;
        let drive = self.grid_drive;
        let pos = self.adaa_pos.tick(&self.half.framed(bias, drive, scale), u);
        let neg = self
            .adaa_neg
            .tick(&self.half.framed(bias, -drive, scale), u);
        let y = pos - (1.0 + self.imbalance) * neg;
        let x = u * drive;
        let plate = self.plate_amps.f_at(bias + x) + self.plate_amps.f_at(bias - x);
        self.draw_last = (plate / self.idle_plate_amps) as f32;
        if let Bias::Cathode { .. } = self.bias {
            let cathode = self.cathode_amps.f_at(bias + x) + self.cathode_amps.f_at(bias - x);
            self.vk += self.cathode_coeff * (cathode * self.rk - self.vk);
        }
        y as f32
    }

    /// The shared cathode RC under `Bias::Cathode` (an upper bound);
    /// memoryless under `Bias::Fixed`.
    fn tail(&self) -> Tail {
        match self.bias {
            Bias::Fixed => Tail::None,
            Bias::Cathode { bypass_farads } => {
                Tail::from_time_constant(self.rk * bypass_farads, self.sample_rate)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::topologies::triode_stage::Bias;

    fn render(pp: &mut PushPull, amp: f64, n: usize) -> Vec<f32> {
        let fs = 48_000.0f64;
        (0..n)
            .map(|i| {
                pp.tick((amp * (2.0 * core::f64::consts::PI * 220.0 * i as f64 / fs).sin()) as f32)
            })
            .collect()
    }
    fn goertzel(buf: &[f32], f: f64, fs: f64) -> f64 {
        let w = 2.0 * core::f64::consts::PI * f / fs;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in buf {
            let s0 = f64::from(x) + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let re = s1 - s2 * w.cos();
        let im = s2 * w.sin();
        (re * re + im * im).sqrt() / (buf.len() as f64 * 0.5)
    }

    /// THE PAIR CANCELS EVENS — matched: h2 far below h3; a declared
    /// imbalance leaks h2 back (character, chosen not inherited).
    #[test]
    fn the_pair_cancels_evens() {
        let mut pp = PushPull::new();
        pp.prepare(48_000.0);
        pp.set_class_ab(2.0);
        pp.set_drive_db(12.0);
        let out = render(&mut pp, 0.5, 16_384);
        let h2 = goertzel(&out[8_192..], 440.0, 48_000.0);
        let h3 = goertzel(&out[8_192..], 660.0, 48_000.0);
        // 0.25: the tabulation floor — the crossover (sharpest
        // curvature) sits exactly at x = 0 and interpolation error
        // there rectifies to ~-52 dBc of evens. Real matched pairs
        // leak far more; the contract is ORDER, not perfection.
        assert!(
            h2 < h3 * 0.25,
            "matched pair leaks evens: h2 {h2:.5} vs h3 {h3:.5}"
        );
        let mut pp2 = PushPull::new();
        pp2.prepare(48_000.0);
        pp2.set_class_ab(2.0);
        pp2.set_drive_db(12.0);
        pp2.set_imbalance(0.12);
        let out2 = render(&mut pp2, 0.5, 16_384);
        let h2b = goertzel(&out2[8_192..], 440.0, 48_000.0);
        assert!(
            h2b > h2 * 3.0,
            "imbalance bought no evens ({h2:.6} -> {h2b:.6})"
        );
    }

    /// THE CLASS-AB DRAW CURVE — the reason this topology exists: in AB
    /// the pair's average draw RISES with drive (the per-hit sag
    /// driver); in class A it barely moves (the measured null).
    #[test]
    fn the_class_ab_draw_curve() {
        let avg_draw = |cold: f64, amp: f64| -> f64 {
            let mut pp = PushPull::new();
            pp.prepare(48_000.0);
            pp.set_class_ab(cold);
            pp.set_drive_db(12.0);
            let n = 4_800;
            let mut acc = 0.0f64;
            for i in 0..n {
                let x = amp * (2.0 * core::f64::consts::PI * 220.0 * i as f64 / 48_000.0).sin();
                pp.tick(x as f32);
                acc += f64::from(pp.draw());
            }
            acc / n as f64
        };
        let ab = avg_draw(2.5, 0.6) / avg_draw(2.5, 0.001);
        let a = avg_draw(0.0, 0.6) / avg_draw(0.0, 0.001);
        assert!(ab > 2.0, "class-AB draw rose only {ab:.2}x with drive");
        assert!(a < 1.5, "class-A draw should barely move (rose {a:.2}x)");
        assert!(ab > a * 1.8, "AB ({ab:.2}x) must out-draw A ({a:.2}x)");
    }

    /// THE LOOP LINEARIZES — global NFB buys distortion reduction
    /// on the order of the loop gain at matched output level, and
    /// the closed-loop report stays the truth.
    #[test]
    fn the_loop_linearizes() {
        let thd3 = |fb: f32| -> f64 {
            let mut pp = PushPull::new();
            pp.prepare(48_000.0);
            pp.set_class_ab(1.0);
            pp.set_drive_db(12.0);
            pp.set_feedback_db(fb);
            // Land the fundamental near -14 dBFS either way (fair
            // fight: same OUTPUT level, not same input).
            let amp = 10f64.powf((-14.0 - f64::from(pp.small_signal_gain_db())) / 20.0);
            let out = render(&mut pp, amp, 16_384);
            let h1 = goertzel(&out[8_192..], 220.0, 48_000.0);
            let h3 = goertzel(&out[8_192..], 660.0, 48_000.0);
            h3 / h1.max(1e-12)
        };
        let open = thd3(0.0);
        let closed = thd3(10.0);
        assert!(
            closed < open * 0.5,
            "10 dB of loop bought too little: h3/h1 open {open:.5} closed {closed:.5}"
        );
        // The closed-loop report is the truth.
        let mut pp = PushPull::new();
        pp.prepare(48_000.0);
        pp.set_class_ab(1.0);
        pp.set_drive_db(12.0);
        pp.set_feedback_db(10.0);
        let reported = f64::from(pp.small_signal_gain_db());
        let out = render(&mut pp, 0.0005, 16_384);
        let g = goertzel(&out[8_192..], 220.0, 48_000.0) / 0.0005;
        let measured = 20.0 * g.max(1e-12).log10();
        assert!(
            (measured - reported).abs() < 2.0,
            "closed-loop report lies: reported {reported:.2}, measured {measured:.2}"
        );
    }

    /// THE LOOP LETS GO — feedback holds the gain flat right up to
    /// the wall and then hands the player the knee all at once:
    /// gain droop at a -6 dBFS operating point is smaller
    /// closed-loop than open-loop.
    #[test]
    fn the_loop_lets_go() {
        let droop = |fb: f32| -> f64 {
            let gain_at = |fb: f32, out_db: f64| -> f64 {
                let mut pp = PushPull::new();
                pp.prepare(48_000.0);
                pp.set_class_ab(1.0);
                pp.set_drive_db(12.0);
                pp.set_feedback_db(fb);
                let amp = 10f64.powf((out_db - f64::from(pp.small_signal_gain_db())) / 20.0);
                let out = render(&mut pp, amp, 16_384);
                let h1 = goertzel(&out[8_192..], 220.0, 48_000.0);
                20.0 * (h1 / amp).max(1e-12).log10()
            };
            gain_at(fb, -30.0) - gain_at(fb, -6.0)
        };
        let open = droop(0.0);
        let closed = droop(10.0);
        assert!(
            open > 0.3,
            "probe degenerate: open-loop droop only {open:.2} dB"
        );
        assert!(
            closed < open * 0.6,
            "the loop failed to hold: droop open {open:.2} dB, closed {closed:.2} dB"
        );
    }

    fn self_biased(pp: &mut PushPull, rc_ms: f64) {
        let bypass_farads = rc_ms * 1e-3 / pp.cathode_ohms();
        pp.set_bias(Bias::Cathode { bypass_farads });
    }

    /// THE SHARED CATHODE RIDES THE DRAW — a class-AB pair driven hard
    /// draws more, so its shared cathode rises and the class point
    /// cools (+1.37 V measured after half a second at 0.6); silence lets
    /// it return. A fixed-biased pair never moves.
    #[test]
    fn the_shared_cathode_rides_the_draw() {
        let mut pp = PushPull::new();
        pp.prepare(48_000.0);
        pp.set_class_ab(2.5);
        pp.set_drive_db(12.0);
        self_biased(&mut pp, 40.0);
        for _ in 0..4_800 {
            pp.tick(0.0);
        }
        assert!(pp.cathode_shift().abs() < 0.01, "moved on silence");
        render(&mut pp, 0.6, 24_000);
        let slammed = pp.cathode_shift();
        assert!(
            slammed > 1.0,
            "half a second of slam moved only {slammed:.3} V"
        );
        for _ in 0..48_000 {
            pp.tick(0.0);
        }
        assert!(
            pp.cathode_shift() < 0.05,
            "stuck at {:.3} V",
            pp.cathode_shift()
        );
        let mut fixed = PushPull::new();
        fixed.prepare(48_000.0);
        render(&mut fixed, 0.6, 24_000);
        assert_eq!(fixed.cathode_shift(), 0.0);
    }

    /// THE CATHODE CHANGES THE SHAPE — tone and level co-vary from the
    /// circuit's own state, and the direction is the class point's
    /// (measured): a cathode-biased AB pair GRINDS (the cooling class
    /// point deepens the crossover: h3/h1 0.064 → 0.32, level 0.70 →
    /// 0.22); a cathode-biased warm pair SOFTENS (the signal recedes
    /// from the saturation knee: h3/h1 0.10 → 0.045, level 0.92 → 0.73).
    #[test]
    fn the_cathode_changes_the_shape() {
        let late = |class: f64, cathode: bool| -> (f64, f64) {
            let mut pp = PushPull::new();
            pp.prepare(48_000.0);
            pp.set_class_ab(class);
            pp.set_drive_db(12.0);
            if cathode {
                self_biased(&mut pp, 30.0);
            }
            let out = render(&mut pp, 0.8, 48_000);
            let tail = &out[32_768..];
            let h1 = goertzel(tail, 220.0, 48_000.0);
            (h1, goertzel(tail, 660.0, 48_000.0) / h1)
        };
        let (fixed, cathode) = (late(2.0, false), late(2.0, true));
        assert!(
            cathode.0 < 0.5 * fixed.0,
            "AB lost no level: {fixed:?} {cathode:?}"
        );
        assert!(
            cathode.1 > 3.0 * fixed.1,
            "AB did not grind: {fixed:?} {cathode:?}"
        );
        let (fixed, cathode) = (late(1.0, false), late(1.0, true));
        assert!(
            cathode.0 < 0.9 * fixed.0,
            "warm lost no level: {fixed:?} {cathode:?}"
        );
        assert!(
            cathode.1 < 0.6 * fixed.1,
            "warm did not soften: {fixed:?} {cathode:?}"
        );
    }

    /// THE REPORT IS THE TRUTH — small-signal gain and ceiling.
    #[test]
    fn the_pushpull_report_is_the_truth() {
        let mut pp = PushPull::new();
        pp.prepare(48_000.0);
        pp.set_class_ab(1.0);
        pp.set_drive_db(6.0);
        let reported = f64::from(pp.small_signal_gain_db());
        let out = render(&mut pp, 0.0005, 16_384);
        let g = goertzel(&out[8_192..], 220.0, 48_000.0) / 0.0005;
        let measured = 20.0 * g.max(1e-12).log10();
        assert!(
            (measured - reported).abs() < 1.5,
            "report lies: reported {reported:.2}, measured {measured:.2}"
        );
        // Ceiling: slam it, peak must sit near reported ceiling.
        let mut pp2 = PushPull::new();
        pp2.prepare(48_000.0);
        pp2.set_class_ab(1.0);
        pp2.set_drive_db(24.0);
        let out2 = render(&mut pp2, 1.0, 16_384);
        let pk = out2[8_192..].iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(
            (20.0 * f64::from(pk).log10() - f64::from(pp2.ceiling_db())).abs() < 1.5,
            "ceiling report off: peak {:.2} dB vs reported {:.2}",
            20.0 * f64::from(pk).log10(),
            pp2.ceiling_db()
        );
    }
}
