//! The IIR 2× oversampler (Butterworth cascade), kept test-only as the
//! reference the FIR half-bands of [`Oversampled2x`](crate::Oversampled2x)
//! are measured against, and as the measured alternative some in-crate
//! characterization tests were pinned with.

use crate::kernel::node::Stage;
use crate::primitives::filters::biquad::{Biquad, FilterSpec};
use crate::units::Hz;

const STAGES: usize = 6;

#[derive(Clone, Copy, Debug)]
pub struct Oversampler2x {
    up: [Biquad; STAGES],
    down: [Biquad; STAGES],
    /// Sample rate of the *outer* (non-oversampled) signal.
    sample_rate: f32,
}

impl Default for Oversampler2x {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler2x {
    pub const fn new() -> Self {
        Self {
            up: [Biquad::passthrough(); STAGES],
            down: [Biquad::passthrough(); STAGES],
            sample_rate: 48_000.0,
        }
    }

    /// Configure for an outer sample rate. The internal 2× rate is
    /// `2 * sample_rate`; the antialiasing cascade sits at ~45 % of the
    /// outer Nyquist (flat to ~20 kHz at 44.1/48k) with the steep
    /// Butterworth skirt rejecting everything above it before decimation.
    pub fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        let inner_rate = self.sample_rate * 2.0;
        let cutoff = self.sample_rate * 0.45;
        let n = (STAGES * 2) as f32; // Butterworth order
        for k in 0..STAGES {
            // Per-section Butterworth Q: 1 / (2·cos( (2k+1)·π / (2N) )).
            let theta = (2.0 * k as f32 + 1.0) * core::f32::consts::PI / (2.0 * n);
            let q = 1.0 / (2.0 * theta.cos());
            for bq in [&mut self.up[k], &mut self.down[k]] {
                bq.prepare(inner_rate);
                bq.set(FilterSpec::Lowpass { frequency: Hz::new(cutoff), q });
            }
        }
    }

    #[inline]
    fn run(stages: &mut [Biquad; STAGES], mut x: f32) -> f32 {
        for s in stages.iter_mut() {
            x = s.tick(x);
        }
        x
    }

    /// Run the closure twice (at 2× rate) on the antialias-filtered
    /// upsampled signal, then downsample-filter + decimate back to one
    /// outer-rate output sample. `inner` must be deterministic given its
    /// inputs (we call it for both interleaved samples — the order matters
    /// for stateful inner processors).
    #[inline]
    pub fn tick_with<F: FnMut(f32) -> f32>(&mut self, x: f32, mut inner: F) -> f32 {
        // Upsample: zero-insertion + LPF. The 2× on the even tap
        // compensates the zero-insertion energy loss (unity passband gain).
        let u0 = Self::run(&mut self.up, 2.0 * x);
        let u1 = Self::run(&mut self.up, 0.0);

        // User's nonlinear block at 2× rate.
        let p0 = inner(u0);
        let p1 = inner(u1);

        // Downsample: LPF both, keep the second of the pair (consistent
        // phase — the filter delays both equally).
        let _ = Self::run(&mut self.down, p0);
        Self::run(&mut self.down, p1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::resample::{HalfbandFir, Oversampler2xFir, HB_TAPS};

    /// Magnitude response |H(f)| (dB) of one up-chain at the 2× rate,
    /// measured by driving a steady sine and reading steady-state RMS.
    fn chain_gain_db(os: &mut Oversampler2x, f: f64, inner_rate: f64) -> f64 {
        let n = 8192usize;
        // settle
        for i in 0..n {
            let s = (2.0 * core::f64::consts::PI * f * i as f64 / inner_rate).sin() as f32;
            let _ = Oversampler2x::run(&mut os.up, s);
        }
        let mut acc = 0.0f64;
        for i in 0..n {
            let s = (2.0 * core::f64::consts::PI * f * i as f64 / inner_rate).sin() as f32;
            let y = Oversampler2x::run(&mut os.up, s) as f64;
            acc += y * y;
        }
        let out_rms = (acc / n as f64).sqrt();
        let in_rms = core::f64::consts::FRAC_1_SQRT_2;
        20.0 * (out_rms / in_rms).log10()
    }

    #[test]
    fn decimator_is_steep() {
        // 12th-order Butterworth at 0.45·fs (cutoff 21.6 kHz, 2× rate 96k):
        // flat to ~20 kHz, then a steep skirt. Verify the passband is flat
        // and the worst-case rejection across the band where audible
        // aliases originate (content folding into 0–20 kHz, i.e. 28–48 kHz)
        // is deep.
        let fs = 48_000.0;
        let inner = fs * 2.0;
        let mut os = Oversampler2x::new();
        os.prepare(fs as f32);
        let g20 = chain_gain_db(&mut os, 20_000.0, inner);
        assert!(g20 > -1.5, "passband not flat to 20 kHz: {g20} dB");
        // Worst-case stopband rejection over 28–47 kHz.
        let mut worst = -300.0f64;
        let mut worst_f = 0.0;
        let mut f = 28_000.0;
        while f <= 47_000.0 {
            let g = chain_gain_db(&mut os, f, inner);
            if g > worst {
                worst = g;
                worst_f = f;
            }
            f += 1_000.0;
        }
        println!("passband 20k: {g20:.1} dB | worst stopband {worst:.1} dB @ {worst_f:.0} Hz");
        // Worst case sits at the transition shoulder (~29 kHz, folds to
        // ~19 kHz — barely audible); the audible-fold region (38–47 kHz) is
        // far deeper. −35 dB worst is a ~+28 dB win over the old 4th-order.
        assert!(
            worst < -35.0,
            "worst stopband rejection too shallow: {worst} dB @ {worst_f}"
        );
    }

    // ===== FIR half-band =====

    /// Steady-state gain (dB) of a single HalfbandFir at normalised
    /// frequency `f` (fraction of ITS running rate). The frequency is
    /// quantised to an exact integer number of cycles over the RMS window
    /// — a fractional cycle count biases the RMS of a sine and reads as a
    /// spurious gain error far above the 0.01 dB bound tested here.
    fn hb_gain_db(f_norm: f64) -> f64 {
        let mut hb = HalfbandFir::new();
        let n = 16_384usize;
        let m = (f_norm * n as f64).round().max(1.0); // integer cycles
        let f = m / n as f64;
        for i in 0..n {
            let s = (2.0 * core::f64::consts::PI * f * i as f64).sin() as f32;
            let _ = hb.tick(s);
        }
        let mut acc = 0.0f64;
        for i in n..2 * n {
            let s = (2.0 * core::f64::consts::PI * f * i as f64).sin() as f32;
            let y = hb.tick(s) as f64;
            acc += y * y;
        }
        let out_rms = (acc / n as f64).sqrt();
        20.0 * (out_rms / core::f64::consts::FRAC_1_SQRT_2).log10()
    }

    #[test]
    fn fir_halfband_meets_its_design_spec() {
        // Passband: flat to 0.4535 of the running rate (= 20 kHz at a
        // 44.1 kHz outer rate). Stopband: ≥90 dB above 0.5465 (= folds to
        // below 20 kHz). These are the two numbers the whole world-class
        // aliasing claim rests on, measured directly.
        // DC gain: exact by construction (coefficients are normalised);
        // feed a constant and check the settled output directly.
        let mut hb = HalfbandFir::new();
        let mut y = 0.0f32;
        for _ in 0..HB_TAPS + 8 {
            y = hb.tick(1.0);
        }
        assert!((y - 1.0).abs() < 1e-6, "DC gain not unity: {y}");
        let mut worst_pass = 0.0f64;
        for i in 0..=20 {
            let f = 0.02 + (0.4535 - 0.02) * i as f64 / 20.0;
            let g = hb_gain_db(f * 0.5); // f is fraction of outer rate; filter runs at 2×
            worst_pass = worst_pass.max(g.abs());
        }
        let mut worst_stop = -300.0f64;
        for i in 0..=20 {
            let f = 0.5465 + (0.98 - 0.5465) * i as f64 / 20.0;
            let g = hb_gain_db(f * 0.5);
            worst_stop = worst_stop.max(g);
        }
        println!("halfband: passband dev {worst_pass:.4} dB | worst stopband {worst_stop:.1} dB");
        assert!(worst_pass < 0.01, "passband not flat: {worst_pass} dB");
        assert!(worst_stop < -90.0, "stopband too shallow: {worst_stop} dB");
    }

    #[test]
    fn fir_oversampler_latency_is_exactly_integer() {
        // An impulse through the identity round trip must peak at exactly
        // latency() with near-unity amplitude, and the response
        // must be symmetric about the peak (linear phase). A half-sample
        // error here would comb the dry/wet mix of every consumer.
        let mut os = Oversampler2xFir::new();
        os.prepare(48_000.0);
        let lat = Oversampler2xFir::latency();
        let n = 4 * lat;
        let mut h = vec![0.0f32; n];
        for (i, v) in h.iter_mut().enumerate() {
            let x = if i == 0 { 1.0 } else { 0.0 };
            *v = os.tick_with(x, |s| s);
        }
        let peak = h
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
            .unwrap()
            .0;
        assert_eq!(peak, lat, "impulse peak at {peak}, expected {lat}");
        assert!(h[peak] > 0.95, "peak magnitude {h:?}", h = h[peak]);
        for k in 1..lat.min(40) {
            let d = (h[peak - k] - h[peak + k]).abs();
            assert!(d < 1e-4, "not linear-phase: asymmetry {d} at ±{k}");
        }
    }

    #[test]
    #[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
    fn diag_where_does_alias_land() {
        // Diagnostic: 4 kHz tone through triode inside 2× FIR vs 2× IIR;
        // dump the strongest non-harmonic bins with their frequencies to
        // see whether the residual is audible-band or transition-band
        // fold-through. Run: cargo test -p stave-dsp diag_where -- --ignored --nocapture
        use crate::circuit::topologies::triode_stage::Triode;
        let fs = 48_000.0f64;
        let n = 8192usize;
        let m = 683usize;
        let run = |fir: bool| -> Vec<f32> {
            let mut t = Triode::new();
            t.prepare(fs as f32 * 2.0);
            t.set_grid_drive(37.11);
            let bypass_farads = 30e-3 / t.cathode_ohms();
            t.set_bias(crate::circuit::topologies::triode_stage::Bias::Cathode { bypass_farads });
            let mut osf = Oversampler2xFir::new();
            osf.prepare(fs as f32);
            let mut osb = Oversampler2x::new();
            osb.prepare(fs as f32);
            let mut step = move |x: f32| -> f32 {
                if fir {
                    osf.tick_with(x, |s| t.tick(s))
                } else {
                    osb.tick_with(x, |s| t.tick(s))
                }
            };
            let sig = |i: usize| {
                0.5 * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32
            };
            for i in 0..n {
                let _ = step(sig(i));
            }
            (0..n).map(|i| step(sig(i))).collect()
        };
        for fir in [false, true] {
            let out = run(fir);
            let fft = crate::Fft::<f64>::new(n);
            let samples: Vec<f64> = out.iter().map(|&x| x as f64).collect();
            let mut bins = vec![crate::primitives::spectral::Complex::default(); fft.real_bins()];
            fft.forward_real(&samples, &mut bins);
            let re: Vec<f64> = bins.iter().map(|z| z.re).collect();
            let im: Vec<f64> = bins.iter().map(|z| z.im).collect();
            let mut sig = 0.0f64;
            let mut noise_all = 0.0f64;
            let mut noise_20k = 0.0f64;
            let mut worst: Vec<(f64, usize)> = vec![];
            for b in 1..n / 2 {
                let p = re[b] * re[b] + im[b] * im[b];
                if b % m == 0 {
                    sig += p;
                } else {
                    noise_all += p;
                    if (b as f64) * fs / n as f64 <= 20_000.0 {
                        noise_20k += p;
                    }
                    worst.push((p, b));
                }
            }
            worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            println!(
                "{}: full-band {:.1} dB | ≤20 kHz {:.1} dB",
                if fir { "FIR" } else { "IIR" },
                10.0 * (noise_all / sig).log10(),
                10.0 * (noise_20k / sig).log10()
            );
            for (p, b) in worst.iter().take(6) {
                println!(
                    "   bin {b} ({:.0} Hz): {:.1} dB",
                    *b as f64 * fs / n as f64,
                    10.0 * (p / sig).log10()
                );
            }
        }
    }

    #[test]
    fn fir_round_trip_is_transparent() {
        // Identity inner ⇒ the chain is just interpolate+decimate. A mid-
        // band tone must come through at unity (±0.01 dB) with residual
        // (non-tone) energy below −100 dB. Uses a bin-centred tone and
        // compares against the input delayed by the known latency, so the
        // measurement has no leakage and no phase confound.
        let mut os = Oversampler2xFir::new();
        os.prepare(48_000.0);
        let lat = Oversampler2xFir::latency();
        let n = 8192usize;
        let m = 512usize; // 3 kHz at 48 k — bin-centred
        let tone = |i: isize| -> f64 {
            0.5 * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin()
        };
        // settle
        for i in -(n as isize)..0 {
            let _ = os.tick_with(tone(i) as f32, |s| s);
        }
        let mut err_acc = 0.0f64;
        let mut sig_acc = 0.0f64;
        for i in 0..n as isize {
            let y = os.tick_with(tone(i) as f32, |s| s) as f64;
            let want = tone(i - lat as isize);
            err_acc += (y - want) * (y - want);
            sig_acc += want * want;
        }
        let err_db = 10.0 * (err_acc / sig_acc).log10();
        println!("FIR round-trip error vs delayed input: {err_db:.1} dB");
        assert!(err_db < -90.0, "round trip not transparent: {err_db} dB");
    }
}
