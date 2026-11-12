//! MATERIAL RE-RADIATION — walls that SING BACK (worklist 5).
//! A real surface is not a mirror with a gain: struck by the
//! field, it stores energy in its own body resonances and
//! re-radiates them. Metal rings inharmonic in the 0.8-3 kHz
//! body; wood thumps its panel modes at 150-400 Hz; glass sings
//! 1-4 kHz; plaster and drape stay silent (massive/limp — their
//! answer is absorption, already in the wall gains). This is the
//! material's VOICE, not its filter — the timbre that survives
//! after the specular field fades.
//!
//! The layer is LINEAR (resonators — the linearity laws stay
//! green; real nonlinearity in rooms is rattles, a future life
//! layer, on record). It is driven by the EARLY FIELD per ear —
//! wall-incident energy — and its output joins the ER path, so
//! it wears the same early gain, coherence fold and lean as the
//! reflections that excited it.

use stave_dsp::t60_gain;

/// One ringing body per entry: (Hz, T60 s, level). Frequencies
/// deliberately inharmonic (a plate's modes are not a harmonic
/// series). Order matches params::MATERIALS / scene::material.
const VOICES: [&[(f32, f32, f32)]; 6] = [
    // WOOD: panel thumps, low and quick.
    &[(163.0, 0.30, 1.0), (271.0, 0.26, 0.8), (389.0, 0.22, 0.6)],
    // PLASTER: massive, silent — the baseline stays the baseline.
    &[],
    // STONE: nearly rigid; a faint knock, mostly silent.
    &[(1_070.0, 0.12, 0.35)],
    // GLASS: bright ping, 1-4 kHz.
    &[
        (1_130.0, 0.35, 0.8),
        (2_090.0, 0.30, 0.7),
        (3_610.0, 0.24, 0.5),
    ],
    // DRAPE: limp — silent.
    &[],
    // METAL: the plate's body, dense and inharmonic.
    &[
        (830.0, 0.55, 0.9),
        (1_373.0, 0.50, 0.85),
        (2_207.0, 0.45, 0.7),
        (2_957.0, 0.40, 0.55),
    ],
];

/// The loudest material voice sits well under the reflections
/// that excite it — a coloration, not an instrument.
const RERAD_LEVEL: f32 = 0.20;

const MAX_VOICES: usize = 4;

#[derive(Clone, Copy, Default)]
struct Body {
    a1: f32,
    a2: f32,
    k: f32,
    y1: f32,
    y2: f32,
}

/// One ear's set of ringing bodies.
pub(super) struct Rerad {
    bodies: [[Body; MAX_VOICES]; 2],
    active: usize,
    material: usize,
    sr: f32,
}

impl Rerad {
    pub fn new() -> Self {
        Self {
            bodies: [[Body::default(); MAX_VOICES]; 2],
            active: 0,
            material: usize::MAX,
            sr: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.material = usize::MAX;
        self.reset();
    }

    pub fn reset(&mut self) {
        for ear in self.bodies.iter_mut() {
            for b in ear.iter_mut() {
                b.y1 = 0.0;
                b.y2 = 0.0;
            }
        }
    }

    /// Block-rate: retarget the voice set when the material
    /// changes (states drain through the new poles — no snap).
    pub fn tune(&mut self, material: usize) {
        let material = material.min(5);
        if material == self.material {
            return;
        }
        self.material = material;
        let voices = VOICES[material];
        self.active = voices.len();
        for (vi, (hz, t60, level)) in voices.iter().enumerate() {
            let w = core::f32::consts::TAU * hz / self.sr;
            let r = t60_gain(1.0, t60 * self.sr);
            // Driven-gain-flat (the modal lesson): k = 2(1−r)·sin(w)
            // keeps the driven response level across Q — sustained
            // energy cannot integrate a high-Q body to a scream.
            let k = 2.0 * (1.0 - r) * w.sin() * level * RERAD_LEVEL;
            for ear in self.bodies.iter_mut() {
                let b = &mut ear[vi];
                b.a1 = 2.0 * r * w.cos();
                b.a2 = -r * r;
                b.k = k;
            }
        }
    }

    /// The walls answer: per-ear early-field energy in, ring out.
    #[inline]
    pub fn tick(&mut self, xl: f32, xr: f32) -> (f32, f32) {
        let mut out = [0.0f32; 2];
        for (ei, x) in [xl, xr].into_iter().enumerate() {
            for b in self.bodies[ei][..self.active].iter_mut() {
                let y = b.a1 * b.y1 + b.a2 * b.y2 + b.k * x;
                b.y2 = b.y1;
                b.y1 = y;
                out[ei] += y;
            }
        }
        (out[0], out[1])
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    fn goertzel(w: &[f32], hz: f32, sr: f32) -> f64 {
        let n = w.len();
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &x) in w.iter().enumerate() {
            let hann = 0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / n as f64).cos();
            let ph = core::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(sr);
            re += f64::from(x) * hann * ph.cos();
            im += f64::from(x) * hann * ph.sin();
        }
        (re * re + im * im).sqrt()
    }

    /// Struck once, metal RINGS at its declared body frequencies
    /// and plaster stays silent; the ring decays (a voice, not a
    /// drone).
    #[test]
    fn the_walls_sing_their_declared_voice() {
        let ring = |material: usize| -> Vec<f32> {
            let mut rr = Rerad::new();
            rr.prepare(48_000.0);
            rr.tune(material);
            (0..24_000)
                .map(|i| {
                    let x = if i == 0 { 1.0 } else { 0.0 };
                    rr.tick(x, x).0
                })
                .collect()
        };
        let metal = ring(5);
        let plaster = ring(1);
        assert!(
            plaster.iter().all(|x| x.abs() < 1e-9),
            "plaster must stay silent"
        );
        let early = goertzel(&metal[0..9_600], 1_373.0, 48_000.0);
        let off_peak = goertzel(&metal[0..9_600], 1_800.0, 48_000.0);
        assert!(
            early > 4.0 * off_peak,
            "metal must ring AT its body: on {early:.2e} vs off {off_peak:.2e}"
        );
        let late = goertzel(&metal[14_400..24_000], 1_373.0, 48_000.0);
        assert!(
            late < 0.25 * early,
            "the ring must decay: early {early:.2e} vs late {late:.2e}"
        );
    }
}
