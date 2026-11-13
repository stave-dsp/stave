use super::*;
use crate::circuit::devices::diode::Shockley;
use crate::circuit::devices::thermal_volts;

#[test]
fn expo_drifts_as_kt_over_q() {
    let reference = 27.0;
    let expo = Expo::uncompensated(reference);
    for octaves in [-2.0, 1.0, 3.0, 5.0] {
        // The control voltage the converter was trimmed to deliver
        // `octaves` with, through exp(ΔV/Vt) at each temperature.
        let dv = octaves * thermal_volts(reference) * std::f64::consts::LN_2;
        for celsius in [-20.0, 0.0, 27.0, 45.0, 80.0] {
            let direct = (dv / thermal_volts(celsius)).exp();
            let ratio = expo.ratio(octaves, celsius);
            assert!((ratio / direct - 1.0).abs() < 1e-12, "{octaves} oct at {celsius} °C");
        }
        // −1200·n/T₀ cents per kelvin at the reference.
        let slope = (expo.cents(octaves, reference + 0.01) - expo.cents(octaves, reference - 0.01)) / 0.02;
        let expect = -1200.0 * octaves / (reference + ZERO_CELSIUS);
        assert!((slope / expect - 1.0).abs() < 1e-4, "{octaves} oct: {slope} cents/K against {expect}");
    }
    // An ideal tempco cancels the 1/T exactly; a +3500 ppm/K part (above
    // the ideal 3332 at 27 °C) overcompensates, sharp when warm.
    let ideal = Expo::new(reference, Expo::ideal_tempco_ppm(reference));
    for celsius in [-20.0, 10.0, 60.0] {
        assert!(ideal.cents(4.0, celsius).abs() < 1e-9, "{celsius}");
    }
    let real = Expo::new(reference, 3500.0);
    let warm = real.cents(4.0, 37.0);
    let expect = 1200.0 * 4.0 * (300.15 / 310.15 * (1.0 + 3500e-6 * 10.0) - 1.0);
    assert!((warm - expect).abs() < 1e-9 && warm > 0.0, "{warm} against {expect}");
}

#[test]
fn junctions_drift_as_spice_sets_their_temperature() {
    // At its own temperature a card is unchanged.
    let d = Shockley::N4148;
    let same = d.at_celsius(27.0);
    assert!((same.saturation_amps / d.saturation_amps - 1.0).abs() < 1e-12);
    assert!((same.thermal_volts / d.thermal_volts - 1.0).abs() < 1e-12);
    // A silicon junction at a fixed current loses about 2 mV per kelvin.
    let forward = |s: &Shockley, amps: f64| s.knee_volts() * (amps / s.saturation_amps + 1.0).ln();
    for amps in [1e-4, 1e-3, 1e-2] {
        let slope = (forward(&d.at_celsius(37.0), amps) - forward(&d.at_celsius(17.0), amps)) / 20.0;
        assert!((-2.6e-3..-1.4e-3).contains(&slope), "{amps} A: {slope} V/K");
    }
    // SPICE's law, written out: Is·exp((T/T₀ − 1)·EG/(N·Vt))·(T/T₀)^(XTI/N).
    let t0 = 300.15;
    let t = 323.15;
    let vt = 1.380_649e-23 * t / 1.602_176_634e-19;
    let n = d.emission;
    let expect = d.saturation_amps * ((t / t0 - 1.0) * 1.11 / (n * vt)).exp() * (t / t0).powf(3.0 / n);
    let hot = d.at_celsius(50.0);
    assert!((hot.saturation_amps / expect - 1.0).abs() < 1e-12);
}
