//! D4 and S7's datasheet lines: the N-band crossover (Linkwitz–Riley and
//! linear phase), the multiband frame with a compressor per band, the
//! formant filter, and the tuned resonators.

use super::sheet::bench;
use stave_dsp::effects::dynamics::{Compressor, GainComputer, Multiband, Topology};
use stave_dsp::primitives::filters::crossover::{Crossover, Order};
use stave_dsp::primitives::filters::formant::{Formants, Speaker, Vowel};
use stave_dsp::primitives::resonators::{Bank, Comb, KarplusStrong, Mode};
use stave_dsp::primitives::time::interpolators::Lagrange3;
use stave_dsp::units::{db, hz, seconds};
use stave_dsp::Stage;

pub fn datasheet(sample_rate: f32, input: &[f32]) {
    let corners = [hz(120.0), hz(800.0), hz(5_000.0)];
    for (name, mut split) in [
        ("Crossover (4 bands, LR4)", Crossover::<4>::linkwitz_riley(Order::Lr4, &corners)),
        ("Crossover (4 bands, LR8)", Crossover::<4>::linkwitz_riley(Order::Lr8, &corners)),
        // 200 Hz transitions at 80 dB: 1 207 taps, 603 samples of latency.
        (
            "Crossover (4 bands, linear phase)",
            Crossover::<4>::linear_phase(&corners, hz(200.0), db(80.0)),
        ),
    ] {
        split.prepare(sample_rate);
        bench(name, input, |x| split.tick(x));
    }

    let curve = GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0 };
    let mut mb = Multiband::new(Crossover::<4>::linkwitz_riley(Order::Lr4, &corners), |_| {
        Compressor::new(curve, Topology::FeedForward)
    });
    mb.prepare(sample_rate);
    bench("Multiband (4 bands LR4, compressors)", input, |x| mb.tick(x * 4.0));

    let mut vowel = Formants::new(Vowel::Aa.formants(Speaker::Man));
    vowel.prepare(sample_rate);
    bench("Formants (3, Peterson & Barney)", input, |x| vowel.tick(x));

    let mut comb = Comb::<Lagrange3>::new(hz(40.0), hz(110.0), seconds(2.0));
    comb.prepare(sample_rate);
    bench("Comb resonator<Lagrange3>", input, |x| comb.tick(x));

    let mut string = KarplusStrong::<Lagrange3>::new(hz(40.0), hz(110.0));
    string.set_decay(seconds(3.0), seconds(0.5));
    string.prepare(sample_rate);
    bench("KarplusStrong<Lagrange3>", input, |x| string.tick(x * 0.01));

    let modes: Vec<Mode> = (1..=32)
        .map(|k| Mode {
            frequency: hz(110.0 * k as f32 * (1.0 + 0.001 * (k * k) as f32)),
            decay: seconds(2.0 / f64::from(k).sqrt()),
            gain: 1.0 / k as f32,
        })
        .collect();
    let mut bank = Bank::new(&modes);
    bank.prepare(sample_rate);
    bench("Bank (32 modes)", input, |x| bank.tick(x));
}
