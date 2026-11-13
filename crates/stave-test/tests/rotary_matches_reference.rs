//! The rotary speaker (DSP.md H4) against its published figures: the
//! Leslie 122H/142H owner's manual's rotor speeds and rise and fall
//! times (`crates/stave-test/datasheet/hammond_122h_rotors.txt`), held
//! by the rotor's inertia at every host rate, and heard in the output.
//! The Doppler shift and the directivity's amplitude modulation are held
//! to their geometry in `physical/rotary/tests.rs`.

use stave_dsp::physical::rotary::{Directivity, Mic, Radiator, Rotor, Speed, REACH};
use stave_dsp::Stage;
use stave_test::reference::Datasheet;

/// A curve's single value at `x`.
fn value(sheet: &Datasheet, wanted: &[(&str, f64)], x: f64) -> f64 {
    let r = &sheet.curve(wanted).reference;
    let i = r.abscissa().iter().position(|&a| a == x).expect("point");
    r.values()[i]
}

/// Seconds from a speed switch until the rotor has covered [`REACH`] of
/// the change.
fn ramp(rotor: Rotor, from: Speed, to: Speed, rate: f64) -> f64 {
    let mut r: Radiator = Radiator::new(rotor, Directivity::OMNI);
    r.prepare(rate as f32);
    r.set_speed(from);
    r.settle();
    let start = r.rate();
    r.set_speed(to);
    let end = match to {
        Speed::Stop => 0.0,
        Speed::Chorale => f64::from(rotor.slow.get()),
        Speed::Tremolo => f64::from(rotor.fast.get()),
    };
    let goal = start + REACH * (end - start);
    let mut previous = start;
    for n in 1..(30.0 * rate) as usize {
        r.tick(0.0);
        let now = r.rate();
        if (now - goal) * (previous - goal) <= 0.0 {
            // Interpolate the crossing inside the sample.
            return (n as f64 - 1.0 + (goal - previous) / (now - previous)) / rate;
        }
        previous = now;
    }
    panic!("never reached");
}

#[test]
fn rotors_ramp_in_the_published_times() {
    let sheet = Datasheet::load("hammond_122h_rotors");
    for (key, rotor) in [("horn", Rotor::HORN_122H), ("drum", Rotor::DRUM_122H)] {
        let rpm = |x| value(&sheet, &[("speed", 1.0), (key, 1.0)], x) / 60.0;
        assert!((f64::from(rotor.slow.get()) - rpm(0.0)).abs() < 1e-6, "{key} slow");
        assert!((f64::from(rotor.fast.get()) - rpm(1.0)).abs() < 1e-6, "{key} fast");
        let rise = value(&sheet, &[("rise", 1.0), (key, 1.0)], 0.0);
        let fall = value(&sheet, &[("fall", 1.0), (key, 1.0)], 0.0);
        for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            let up = ramp(rotor, Speed::Chorale, Speed::Tremolo, rate);
            let down = ramp(rotor, Speed::Tremolo, Speed::Chorale, rate);
            let stop = ramp(rotor, Speed::Tremolo, Speed::Stop, rate);
            // The exact exponential step: within a sample's share of the ramp.
            for (got, want, what) in [(up, rise, "rise"), (down, fall, "fall"), (stop, fall, "stop")] {
                assert!((got / want - 1.0).abs() < 2e-4, "{key} {what} at {rate}: {got} s vs {want} s");
            }
        }
        eprintln!("{key}: rise {rise} s, fall {fall} s held at 44.1–192 kHz");
    }
}

/// The rotor's turn as heard: a horn's Doppler swing rises through its
/// mean once a revolution, so the time between those crossings is the
/// period of the speed the manual gives.
#[test]
fn the_output_turns_at_the_published_speed() {
    let rate = 48_000.0;
    let f = 2_000.0;
    let rotor = Rotor::HORN_122H;
    let mut r: Radiator = Radiator::new(rotor, Directivity::OMNI);
    r.prepare(rate as f32);
    r.set_speed(Speed::Tremolo);
    r.settle();
    assert!(r.set_mic(0, Mic::at(0.0, 1.0)));
    let n = (1.0 * rate) as usize;
    let y: Vec<f32> = (0..n)
        .map(|i| r.tick((std::f64::consts::TAU * f * i as f64 / rate).sin() as f32))
        .collect();
    // Rising zero crossings, then the frequency over ten periods.
    let ups: Vec<f64> = (1..n)
        .filter(|&i| y[i - 1] < 0.0 && y[i] >= 0.0)
        .map(|i| {
            let (a, b) = (f64::from(y[i - 1]), f64::from(y[i]));
            (i as f64 - 1.0 + a / (a - b)) / rate
        })
        .collect();
    let freq: Vec<(f64, f64)> = ups.windows(11).map(|w| (0.5 * (w[0] + w[10]), 10.0 / (w[10] - w[0]))).collect();
    // Once a revolution the swing rises through its mean: the crossings,
    // with hysteresis (armed only from the swing's lower half).
    let mean = freq.iter().map(|p| p.1).sum::<f64>() / freq.len() as f64;
    let top = freq.iter().map(|p| p.1).fold(0.0, f64::max);
    let mut armed = false;
    let mut peaks = Vec::new();
    for w in freq.windows(2) {
        if w[1].1 < mean - 0.5 * (top - mean) {
            armed = true;
        }
        if armed && w[0].1 < mean && w[1].1 >= mean {
            peaks.push(w[0].0 + (mean - w[0].1) / (w[1].1 - w[0].1) * (w[1].0 - w[0].0));
            armed = false;
        }
    }
    let period = (peaks[peaks.len() - 1] - peaks[0]) / (peaks.len() - 1) as f64;
    let rpm = 60.0 / period;
    assert!((rpm / 402.0 - 1.0).abs() < 0.01, "{rpm} rpm");
    eprintln!("horn heard turning at {rpm:.1} rpm (manual: 402)");
}
