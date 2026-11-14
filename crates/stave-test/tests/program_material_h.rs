//! H1 and H2 on program material (DSP.md C13): the reference corpus's
//! roles (`stave_test::corpus`, DSP.md R22) through the bucket-brigade
//! delay and the spring tank. The audio is never committed; without the
//! library these tests say so and pass.

use stave_dsp::circuit::devices::bbd::Chip;
use stave_dsp::circuit::topologies::bucket_brigade::BucketBrigade;
use stave_dsp::physical::spring::Tank;
use stave_dsp::units::hz;
use stave_dsp::Stage;
use stave_test::corpus::Corpus;

/// The first item of `role`, mono, at its rate, and its description;
/// `None` (printed why) when the library or the role's material is absent.
fn corpus(role: &str) -> Option<(Vec<f32>, f32, String)> {
    let skip = |why: &dyn std::fmt::Display| println!("{role}: {why}");
    let c = Corpus::open().map_err(|s| skip(&s)).ok()?;
    let item = c.role(role).map_err(|s| skip(&s)).ok()?.into_iter().next()?;
    let m = c.load(&item).unwrap_or_else(|e| panic!("{role}: {e}"));
    Some((m.mono(), m.rate as f32, m.describe()))
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

/// An amped guitar chop (the role's first item) through the BBD at a 40 kHz clock: the output is the
/// input delayed by `stages/(2·clock)` (the cross-correlation peaks
/// there, within the filters' 0.15 ms of group delay), and the
/// compander tracks on real dynamics — its output's level within 1 dB of
/// the line alone.
#[test]
fn guitar_through_the_bucket_brigade() {
    let Some((input, rate, name)) = corpus("guitar_amped") else { return };
    let input = &input[..input.len().min(rate as usize * 4)];
    let run = |compander: bool| -> Vec<f64> {
        let mut bbd = BucketBrigade::new(Chip::MN3007);
        bbd.prepare(rate);
        bbd.line().set_noise(0.0);
        bbd.line().set_feedthrough(0.0);
        bbd.reset();
        bbd.set_compander(compander);
        bbd.set_clock(hz(40_000.0));
        input.iter().map(|&x| bbd.tick_f64(f64::from(x))).collect()
    };
    let (plain, companded) = (run(false), run(true));
    assert!(plain.iter().chain(&companded).all(|y| y.is_finite()));
    let delay = Chip::MN3007.delay(hz(40_000.0)).get() * f64::from(rate);
    // The correlation over lags about the expected delay.
    let lags = (delay as usize).saturating_sub(48)..delay as usize + 48;
    let correlation: Vec<f64> = lags
        .clone()
        .map(|l| input.iter().zip(&plain[l..]).map(|(&x, &y)| f64::from(x) * y).sum())
        .collect();
    let best = (0..correlation.len()).max_by(|&a, &b| correlation[a].total_cmp(&correlation[b])).unwrap();
    let lag = lags.start + best;
    let error_ms = (lag as f64 - delay) / f64::from(rate) * 1e3;
    let level = 20.0 * (rms(&companded) / rms(&plain)).log10();
    println!("{name}: correlation peak {error_ms:+.3} ms from stages/(2·clock); compander {level:+.2} dB re the line");
    assert!((0.0..0.25).contains(&error_ms), "{error_ms} ms");
    assert!(level.abs() < 1.0, "{level} dB");
}

/// A snare (the transients role's first item) into the three-spring tank: nothing before the shortest
/// spring's first arrival, a tail that rings on past the hit, finite.
#[test]
fn snare_through_the_spring_tank() {
    let Some((input, rate, name)) = corpus("transients") else { return };
    let mut tank = Tank::default();
    tank.prepare(rate);
    let len = input.len() + rate as usize * 2;
    let out: Vec<f64> = (0..len)
        .map(|i| f64::from(tank.tick(input.get(i).copied().unwrap_or(0.0))))
        .collect();
    assert!(out.iter().all(|y| y.is_finite()));
    let onset = input.iter().position(|x| x.abs() > 1e-3).unwrap_or(0);
    // The shortest transit's earliest arrival (its DC delay) is past
    // 20 ms; the drive and pickup filters lead it only by their ring.
    let early = &out[..onset + (0.015 * rate) as usize];
    let total = rms(&out);
    let before = rms(early) * (early.len() as f64 / out.len() as f64).sqrt();
    let tail = rms(&out[input.len()..]);
    println!(
        "{name}: before first arrival {:.1} dB, tail after the hit {:.1} dB, re the whole",
        20.0 * (before / total).log10(),
        20.0 * (tail / total).log10()
    );
    assert!(before < total * 1e-3, "energy before the first arrival");
    assert!(tail > total * 0.1, "the tank rings on");
}
