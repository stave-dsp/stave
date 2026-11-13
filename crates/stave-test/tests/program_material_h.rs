//! H1 and H2 on program material (DSP.md C13): picks from the reference
//! corpus (`$STAVE_SAMPLE_LIBRARY/Test Corpus`) through the bucket-brigade
//! delay and the spring tank. The audio is never committed; without the
//! library these tests say so and pass.

use std::path::PathBuf;

use stave_dsp::circuit::devices::bbd::Chip;
use stave_dsp::circuit::topologies::bucket_brigade::BucketBrigade;
use stave_dsp::physical::spring::Tank;
use stave_dsp::units::hz;
use stave_dsp::Stage;

/// The corpus file `name` (relative to `Test Corpus/`), mono, at its
/// rate; `None` when the library is absent.
fn corpus(name: &str) -> Option<(Vec<f32>, f32)> {
    let root = std::env::var_os("STAVE_SAMPLE_LIBRARY").map(PathBuf::from)?;
    let path = root.join("Test Corpus").join(name);
    if !path.exists() {
        return None;
    }
    let audio = stave_host::wav::read(&path).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
    let n = audio.channels[0].len();
    let mono = (0..n)
        .map(|i| audio.channels.iter().map(|c| c[i]).sum::<f32>() / audio.channels.len() as f32)
        .collect();
    Some((mono, audio.sample_rate as f32))
}

fn skipped(name: &str) {
    println!("skipped: library absent (STAVE_SAMPLE_LIBRARY, Test Corpus/{name})");
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

/// A guitar chop through the BBD at a 40 kHz clock: the output is the
/// input delayed by `stages/(2·clock)` (the cross-correlation peaks
/// there, within the filters' 0.15 ms of group delay), and the
/// compander tracks on real dynamics — its output's level within 1 dB of
/// the line alone.
#[test]
fn guitar_through_the_bucket_brigade() {
    let name = "guitar/Beefy Chop F#.wav";
    let Some((input, rate)) = corpus(name) else {
        return skipped(name);
    };
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
    let delay = Chip::MN3007.delay_s(hz(40_000.0)) * f64::from(rate);
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

/// A snare into the three-spring tank: nothing before the shortest
/// spring's first arrival, a tail that rings on past the hit, finite.
#[test]
fn snare_through_the_spring_tank() {
    let name = "snares/Snare Crack It.wav";
    let Some((input, rate)) = corpus(name) else {
        return skipped(name);
    };
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
