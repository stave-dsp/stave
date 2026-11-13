use super::*;
use crate::effects::dynamics::{GainComputer, Limiter, Peaks, Topology};
use crate::primitives::filters::crossover::Order;
use crate::primitives::spectral::{Complex, Fft};
use crate::units::{db, hz, Hz};

const RATE: f32 = 48_000.0;

/// A band stage: a fixed gain, or a look-ahead limiter that never
/// limits (for its latency).
#[derive(Clone, Debug)]
enum Band {
    Gain(f32),
    Limit(Box<Limiter>),
}

impl Stage for Band {
    fn prepare(&mut self, sample_rate: f32) {
        if let Band::Limit(l) = self {
            l.prepare(sample_rate);
        }
    }
    fn reset(&mut self) {
        if let Band::Limit(l) = self {
            l.reset();
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        match self {
            Band::Gain(g) => *g * x,
            Band::Limit(l) => l.tick(x),
        }
    }
    fn latency(&self) -> u32 {
        match self {
            Band::Gain(_) => 0,
            Band::Limit(l) => l.latency(),
        }
    }
}

fn corners() -> [Hz; 3] {
    [hz(120.0), hz(800.0), hz(5_000.0)]
}

fn impulse(stage: &mut impl Stage, len: usize) -> Vec<f64> {
    (0..len)
        .map(|i| f64::from(stage.tick(if i == 0 { 1.0 } else { 0.0 })))
        .collect()
}

fn worst_flatness(h: &[f64]) -> f64 {
    let fft = Fft::<f64>::new(h.len());
    let mut bins = vec![Complex::new(0.0, 0.0); h.len() / 2 + 1];
    fft.forward_real(h, &mut bins);
    let per_hz = h.len() as f64 / f64::from(RATE);
    bins[(20.0 * per_hz) as usize..(20_000.0 * per_hz) as usize]
        .iter()
        .fold(0.0f64, |w, z| w.max((20.0 * z.norm().log10()).abs()))
}

#[test]
#[cfg_attr(miri, ignore)]
fn unity_bands_reconstruct() {
    let limiter = || Band::Limit(Box::new(Limiter::new(Peaks::Sample, 20.0)));
    // Linkwitz–Riley: the output is the crossover's allpass, flat; with
    // a limiter in one band, the same allpass `latency` samples later.
    let lr = Crossover::<4>::linkwitz_riley(Order::Lr4, &corners());
    let mut mb = Multiband::new(lr.clone(), |_| Band::Gain(1.0));
    mb.prepare(RATE);
    assert_eq!(mb.latency(), 0);
    let worst = worst_flatness(&impulse(&mut mb, 1 << 16));
    assert!(worst < 0.002, "LR4 multiband {worst:.5} dB from flat");

    let mut mb = Multiband::new(lr, |i| if i == 2 { limiter() } else { Band::Gain(1.0) });
    mb.prepare(RATE);
    let d = mb.latency() as usize;
    assert!(d > 0 && d == limiter().latency_prepared(RATE) as usize);
    let h = impulse(&mut mb, (1 << 16) + d);
    assert!(h[..d].iter().all(|v| v.abs() < 1e-9), "sound before the latency");
    let worst = worst_flatness(&h[d..]);
    assert!(worst < 0.002, "aligned LR4 {worst:.5} dB from flat");

    // Linear phase: the impulse lands at the latency, exactly.
    let lin = Crossover::<4>::linear_phase(&corners(), hz(200.0), db(80.0));
    for with_limiter in [false, true] {
        let mut mb = Multiband::new(lin.clone(), |i| {
            if with_limiter && i == 1 {
                limiter()
            } else {
                Band::Gain(1.0)
            }
        });
        mb.prepare(RATE);
        let d = mb.latency() as usize;
        let h = impulse(&mut mb, d + 4_096);
        let worst = h
            .iter()
            .enumerate()
            .map(|(i, &v)| (v - if i == d { 1.0 } else { 0.0 }).abs())
            .fold(0.0f64, f64::max);
        assert!(worst < 2e-6, "linear phase (limiter {with_limiter}) off by {worst:e} at {d}");
    }
}

impl Band {
    fn latency_prepared(mut self, rate: f32) -> u32 {
        self.prepare(rate);
        self.latency()
    }
}

/// `(re, im)` of the component at `f` in `y`, by least squares.
fn level_db(y: &[f32], f: f64) -> f64 {
    let v: Vec<f64> = y.iter().map(|&s| f64::from(s)).collect();
    let (re, im) = crate::primitives::filters::probe::phasor(&v, f64::from(RATE), f);
    20.0 * (re * re + im * im).sqrt().log10()
}

fn tone(f: f64, amp: f64, i: usize) -> f32 {
    (amp * (core::f64::consts::TAU * f * i as f64 / f64::from(RATE)).sin()) as f32
}

fn compressor() -> Compressor {
    let mut c = Compressor::new(
        GainComputer { threshold_db: -30.0, ratio: 4.0, knee_db: 0.0 },
        Topology::FeedForward,
    );
    c.set_attack_ms(1.0);
    c.set_release_ms(50.0);
    c
}

#[test]
#[cfg_attr(miri, ignore)]
fn a_band_compresses_alone() {
    let x3 = Crossover::<3>::linkwitz_riley(Order::Lr4, &[hz(200.0), hz(2_000.0)]);
    let mut mb = Multiband::new(x3.clone(), |_| compressor());
    mb.prepare(RATE);
    // The reference: the same split, each band through a lone compressor.
    let mut split = x3;
    split.prepare(RATE);
    let mut alone = [compressor(), compressor(), compressor()];
    for c in &mut alone {
        c.prepare(RATE);
    }
    let n = RATE as usize;
    let (mut out, mut low) = (Vec::new(), Vec::new());
    let mut worst = 0.0f32;
    for i in 0..n {
        let x = tone(60.0, 0.01, i) + tone(6_000.0, 0.5, i);
        let y = mb.tick(x);
        let bands = split.split(x);
        let want: f32 = bands.iter().zip(&mut alone).map(|(b, c)| c.tick(*b)).sum();
        worst = worst.max((y - want).abs());
        if i >= n / 2 {
            out.push(y);
            low.push(bands.iter().sum());
        }
    }
    assert!(worst < 1e-6, "{worst:e} from three lone compressors");
    // The loud high band is squashed; the quiet low band is untouched.
    assert!(mb.band(2)[0].reduction_db() > 15.0);
    assert_eq!(mb.band(0)[0].reduction_db(), 0.0);
    let quiet = level_db(&out, 60.0) - level_db(&low, 60.0);
    assert!(quiet.abs() < 0.01, "low band moved {quiet:.4} dB");
}

#[test]
#[cfg_attr(miri, ignore)]
fn solo_mute_and_bypass() {
    let gains = [0.5f32, 0.25, 2.0, 4.0];
    let x4 = Crossover::<4>::linkwitz_riley(Order::Lr8, &corners());
    // Per band: (solo, mute, bypass), and the weight each band should
    // carry once the switches have faded.
    /// Per band: (solo, mute, bypass).
    type Switches = [(bool, bool, bool); 4];
    let cases: [(Switches, [f32; 4]); 4] = [
        ([(false, false, false); 4], gains),
        (
            [(false, false, false), (true, false, false), (false, false, false), (true, false, true)],
            [0.0, 0.25, 0.0, 1.0],
        ),
        (
            [(false, true, false), (false, false, false), (false, false, true), (false, false, false)],
            [0.0, 0.25, 1.0, 4.0],
        ),
        (
            [(true, true, false), (false, false, false), (false, false, false), (false, false, false)],
            [0.0; 4],
        ),
    ];
    for (switches, weights) in cases {
        let mut mb = Multiband::new(x4.clone(), |i| Band::Gain(gains[i]));
        mb.prepare(RATE);
        for (i, (solo, mute, bypass)) in switches.into_iter().enumerate() {
            mb.set_solo(i, solo);
            mb.set_mute(i, mute);
            mb.set_bypass(i, bypass);
        }
        let mut split = x4.clone();
        split.prepare(RATE);
        let fade = (FADE_S * RATE) as usize + 1;
        let mut worst = 0.0f32;
        for i in 0..4 * fade {
            let x = tone(90.0, 0.3, i) + tone(1_500.0, 0.3, i) + tone(9_000.0, 0.3, i);
            let y = mb.tick(x);
            let want: f32 = split.split(x).iter().zip(weights).map(|(b, w)| b * w).sum();
            if i > fade {
                worst = worst.max((y - want).abs());
            }
        }
        assert!(worst < 1e-5, "{switches:?}: {worst:e}");
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn link_and_sidechain_reach_every_band() {
    let x3 = Crossover::<3>::linkwitz_riley(Order::Lr4, &[hz(200.0), hz(2_000.0)]);
    // A loud mid tone on the left only.
    for (link, right_squashed) in [(Link::Max, true), (Link::Independent, false)] {
        let mut mb = Multiband::new(x3.clone(), |_| compressor());
        mb.prepare(RATE);
        mb.set_link(link);
        for i in 0..RATE as usize / 4 {
            mb.tick_stereo_linked(tone(700.0, 0.5, i), 0.0);
        }
        let [l, r] = mb.band(1);
        assert!(l.reduction_db() > 10.0);
        if right_squashed {
            assert!((l.reduction_db() - r.reduction_db()).abs() < 1e-6, "{link:?}");
        } else {
            assert_eq!(r.reduction_db(), 0.0, "{link:?}");
        }
        for b in [0, 2] {
            assert!(mb.band(b)[1].reduction_db() < 0.5, "{link:?} band {b}");
        }
    }
    // An external low key ducks the low band alone, mono and stereo.
    let mut mb = Multiband::new(x3.clone(), |_| compressor());
    mb.prepare(RATE);
    let mut stereo = Multiband::new(x3, |_| compressor());
    stereo.prepare(RATE);
    for i in 0..RATE as usize / 4 {
        let x = tone(60.0, 0.01, i) + tone(6_000.0, 0.01, i);
        let key = tone(60.0, 0.9, i);
        mb.tick_keyed(x, key);
        stereo.tick_stereo_keyed(x, x, key, 0.0);
    }
    for m in [&mb, &stereo] {
        assert!(m.band(0)[0].reduction_db() > 10.0);
        assert!(m.band(2)[0].reduction_db() == 0.0);
    }
    assert_eq!(
        stereo.band(0)[0].reduction_db(),
        stereo.band(0)[1].reduction_db()
    );
}
