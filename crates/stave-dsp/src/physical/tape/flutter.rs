//! The tape transport's speed fluctuation (wow and flutter) as a
//! modulated delay driven by a measured spectrum.

use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;

/// Control updates per second: the trajectory's spectrum stops at 50 Hz,
/// so a 1 kHz control rate with linear interpolation carries it.
const CONTROL_HZ: f64 = 1_000.0;
/// The constant part of the delay, seconds: room for the fluctuation
/// (a few tenths of a millisecond) on either side.
const BASE_SECONDS: f64 = 0.002;
/// Each component's phase random walk per control step, radians: lines
/// a few hundredths of a hertz wide, so the trajectory never repeats.
const DRIFT: f64 = 0.02;
/// The generator's seed: a run repeats exactly.
const SEED: u64 = 0x5eed_7a9e;

/// A tape transport's measured speed fluctuation: the long-term
/// spectrum of the record-to-playback delay (its magnitude in dB on a
/// uniform grid of `bin_hz`, from `first_bin`) and the delay's rms
/// deviation. DSP.md H5 extends it (drift, splices, dropouts).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Transport {
    pub bin_hz: f64,
    pub first_bin: usize,
    pub spectrum_db: &'static [f64],
    pub rms_seconds: f64,
}

impl Transport {
    /// Akai 4000D at 7.5 ips (Maxell tape), measured by Mikkonen,
    /// Wright, Moliner & Välimäki (DAFx-23, Figs. 11–12): the delay
    /// between the heads tracked by a 100 Hz pulse train, its averaged
    /// spectrum from 0.59 to 49.6 Hz on a 0.195 Hz grid, 0.109 ms rms.
    /// Digitised in `crates/stave-test/datasheet/akai4000d_flutter.txt`.
    pub const AKAI_4000D: Self = Self {
        bin_hz: 100.0 / 512.0,
        first_bin: 3,
        spectrum_db: &AKAI_4000D_DB,
        rms_seconds: 1.09e-4,
    };
}

/// The Akai 4000D's delay spectrum, dB, bins 3 to 254 of 100/512 Hz.
#[rustfmt::skip]
const AKAI_4000D_DB: [f64; 252] = [
    1.28, -6.31, -8.77, -7.97, -5.04, 2.33, -9.42, -12.09, -12.45, -13.44,
    -12.67, -3.43, -4.15, -3.63, -9.68, -12.12, -13.15, -13.08, -10.98, -16.15,
    -11.38, -14.58, -20.23, -17.85, -13.29, -21.94, -24.06, -24.96, -25.24, -25.30,
    -24.98, -24.18, -23.17, -22.36, -20.19, -15.41, -6.74, -8.22, -16.23, -19.60,
    -22.45, -23.83, -24.89, -20.50, -16.19, -24.68, -27.89, -28.44, -28.76, -28.42,
    -22.89, -20.30, -22.65, -26.40, -30.82, -31.57, -31.83, -32.31, -32.46, -32.74,
    -33.03, -32.97, -33.26, -33.25, -33.24, -32.42, -31.29, -24.45, -24.73, -30.84,
    -33.22, -33.22, -33.86, -33.62, -32.41, -31.75, -27.34, -23.18, -26.52, -25.98,
    -33.45, -33.80, -33.17, -33.41, -33.00, -32.94, -35.48, -35.58, -35.68, -36.01,
    -35.51, -35.90, -36.28, -36.15, -35.91, -36.00, -35.52, -36.00, -35.62, -35.55,
    -34.22, -34.40, -34.78, -33.66, -32.54, -32.09, -31.52, -33.07, -31.81, -31.30,
    -33.49, -33.18, -29.46, -28.17, -29.64, -29.77, -28.95, -31.48, -34.98, -35.86,
    -35.25, -34.37, -35.13, -35.89, -35.77, -35.25, -34.67, -32.26, -29.86, -27.50,
    -28.09, -27.95, -26.65, -26.17, -27.47, -27.62, -26.74, -27.53, -28.33, -28.84,
    -29.85, -29.75, -31.74, -31.86, -33.04, -34.22, -34.92, -36.03, -36.33, -36.99,
    -37.64, -37.68, -38.17, -38.39, -38.11, -38.18, -37.64, -37.58, -37.19, -37.14,
    -37.46, -37.59, -37.02, -36.69, -37.36, -37.12, -37.39, -37.05, -37.52, -37.07,
    -37.54, -37.31, -37.27, -37.80, -37.90, -38.85, -37.93, -38.23, -38.64, -39.06,
    -38.39, -38.77, -39.40, -39.32, -39.03, -39.28, -39.81, -39.43, -39.76, -39.04,
    -39.01, -39.15, -39.12, -39.09, -39.36, -38.81, -39.76, -39.22, -39.90, -39.58,
    -40.19, -39.73, -39.97, -40.40, -39.92, -39.82, -39.26, -40.07, -40.44, -39.99,
    -40.32, -40.64, -40.74, -40.84, -40.06, -40.79, -40.89, -40.99, -41.59, -41.02,
    -41.07, -41.29, -41.20, -41.66, -41.47, -41.75, -41.69, -41.50, -41.32, -41.80,
    -41.65, -41.50, -41.66, -42.00, -41.91, -42.04, -41.92, -42.02, -41.84, -42.39,
    -42.35, -42.61, -42.43, -42.51, -42.47, -42.61, -42.75, -42.80, -42.84, -42.90,
    -42.84, -43.00,
];

/// Wow and flutter: a delay modulated by a trajectory with a
/// transport's measured spectrum.
///
/// # Models
/// A tape transport's speed fluctuation as the record-to-playback delay
/// it causes (wow below about 4 Hz, flutter above), with the spectrum
/// and rms of a measured machine ([`Transport::AKAI_4000D`]).
///
/// # Derivation
/// The trajectory is a sum of sinusoids, one per bin of the measured
/// spectrum, each at the bin's amplitude with a random phase that walks
/// slowly (lines a few hundredths of a hertz wide, so it never repeats),
/// scaled so the sum's rms is the measured one; updated at 1 kHz and
/// interpolated linearly, read from a delay line by cubic interpolation
/// around a constant 2 ms. Chowdhury (DAFx-19, §4.4) drives the same
/// modulated delay from a TC-260's measured periodic timing error,
/// which the paper does not publish; Mikkonen et al. publish their
/// Akai's spectrum.
///
/// # Verified
/// `crates/stave-test/tests/tape_matches_reference.rs::flutter_matches_the_measured_spectrum`
/// (the generated trajectory's averaged spectrum against the table,
/// and its rms).
///
/// # Cost
/// Datasheet `"Flutter (Akai 4000D)"`.
///
/// # Aliasing
/// A modulated delay: its sidebands are the signal's, a few hertz
/// apart; cubic interpolation's own loss above a quarter of the rate.
///
/// # Valid ranges
/// `depth` 0 to 5 (5× the measured fluctuation fits the 2 ms line).
#[derive(Clone, Debug)]
pub struct Flutter {
    transport: Transport,
    depth: f64,
    /// Each component's phasor and its rotation per control step.
    phasors: Vec<(f64, f64)>,
    rotations: Vec<(f64, f64)>,
    amplitudes: Vec<f64>,
    seed: u64,
    line: DelayLine,
    rate: f64,
    period: usize,
    countdown: usize,
    /// The trajectory now (seconds), and its step per sample.
    current: f64,
    slope: f64,
}

impl Flutter {
    /// Prepared at 48 kHz.
    pub fn new(transport: Transport) -> Self {
        let n = transport.spectrum_db.len();
        let power: f64 = transport.spectrum_db.iter().map(|db| 10f64.powf(db / 10.0)).sum();
        let scale = transport.rms_seconds / (0.5 * power).sqrt();
        let amplitudes = transport
            .spectrum_db
            .iter()
            .map(|db| scale * 10f64.powf(db / 20.0))
            .collect();
        let mut f = Self {
            transport,
            depth: 1.0,
            phasors: vec![(1.0, 0.0); n],
            rotations: vec![(1.0, 0.0); n],
            amplitudes,
            seed: SEED,
            line: DelayLine::new(),
            rate: 0.0,
            period: 1,
            countdown: 0,
            current: 0.0,
            slope: 0.0,
        };
        f.prepare(48_000.0);
        f
    }

    /// The fluctuation's scale: 1 is the measured machine, 0 none.
    pub fn set_depth(&mut self, depth: f64) {
        self.depth = depth.clamp(0.0, 5.0);
    }

    /// The trajectory now: the delay's deviation at depth 1, seconds.
    pub fn deviation(&self) -> f64 {
        self.current
    }

    fn random(&mut self) -> f64 {
        // xorshift64*.
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        let x = self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    fn restart(&mut self) {
        self.seed = SEED;
        for k in 0..self.phasors.len() {
            let phase = std::f64::consts::TAU * self.random();
            self.phasors[k] = (phase.cos(), phase.sin());
        }
        self.current = self.trajectory();
        self.slope = 0.0;
        self.countdown = 0;
    }

    /// The sum of the components now, seconds.
    fn trajectory(&self) -> f64 {
        self.phasors.iter().zip(&self.amplitudes).map(|(p, a)| a * p.1).sum()
    }

    /// One control step: rotate, walk the phases, aim at the next point.
    fn control(&mut self) {
        for k in 0..self.phasors.len() {
            let walk = DRIFT * (2.0 * self.random() - 1.0);
            let (c, s) = self.rotations[k];
            let (wc, ws) = (walk.cos(), walk.sin());
            let (rc, rs) = (c * wc - s * ws, c * ws + s * wc);
            let (pc, ps) = self.phasors[k];
            let (nc, ns) = (pc * rc - ps * rs, pc * rs + ps * rc);
            // Renormalise: a product of unit phasors drifts in length.
            let norm = 1.0 / (nc * nc + ns * ns).sqrt();
            self.phasors[k] = (nc * norm, ns * norm);
        }
        let next = self.trajectory();
        self.slope = (next - self.current) / self.period as f64;
        self.countdown = self.period;
    }
}

impl Stage for Flutter {
    fn prepare(&mut self, sample_rate: f32) {
        self.rate = f64::from(sample_rate);
        self.period = (self.rate / CONTROL_HZ).round().max(1.0) as usize;
        let step = self.period as f64 / self.rate;
        for (k, r) in self.rotations.iter_mut().enumerate() {
            let hz = (self.transport.first_bin + k) as f64 * self.transport.bin_hz;
            let w = std::f64::consts::TAU * hz * step;
            *r = (w.cos(), w.sin());
        }
        self.line.allocate((self.rate * 2.0 * BASE_SECONDS) as usize + 8);
        self.restart();
    }
    fn reset(&mut self) {
        self.line.reset();
        self.restart();
    }
    fn tick(&mut self, x: f32) -> f32 {
        if self.countdown == 0 {
            self.control();
        }
        self.countdown -= 1;
        self.current += self.slope;
        self.line.write(x);
        let delay = (BASE_SECONDS + self.depth * self.current) * self.rate;
        let y = self.line.read_cubic(delay as f32);
        self.line.advance();
        y
    }
    fn latency(&self) -> u32 {
        (BASE_SECONDS * self.rate).round() as u32
    }
    fn tail(&self) -> Tail {
        Tail::Samples((2.0 * BASE_SECONDS * self.rate) as u32 + 4)
    }
}
