use super::*;

/// gm = Iabc/(2·Vt): the datasheets' 9600 µS at 500 µA, within 1%; the
/// bias pin where each datasheet puts it.
#[test]
fn the_bias_pin_sits_where_the_datasheet_says() {
    for (ota, volts) in [(BipolarPair::CA3080, 0.71), (BipolarPair::LM13700, 1.4)] {
        let gm = ota.transconductance(500e-6) * 1e6;
        assert!((gm / 9600.0 - 1.0).abs() < 0.01, "{}: {gm} µS", ota.name);
        let at = ota.bias_volts_at(500e-6);
        assert!((at - volts).abs() < 1e-6, "{}: {at}", ota.name);
        let c = ota.currents(-ota.offset_volts, volts);
        assert!((c.bias - 500e-6).abs() < 1e-9, "{c:?}");
        assert!(
            (c.output_per_input_volt * 1e6 - gm).abs() < 1e-3 * gm,
            "{c:?}"
        );
    }
}

#[test]
fn the_partials_are_the_derivatives() {
    let ota = BipolarPair::LM13700;
    for (vd, vb) in [(0.0, 1.3), (0.03, 1.38), (-0.2, 1.2), (0.004, 1.42)] {
        let c = ota.currents(vd, vb);
        let h = 1e-7;
        let dvd = (ota.currents(vd + h, vb).output - ota.currents(vd - h, vb).output) / (2.0 * h);
        let dvb = (ota.currents(vd, vb + h).output - ota.currents(vd, vb - h).output) / (2.0 * h);
        let dbb = (ota.currents(vd, vb + h).bias - ota.currents(vd, vb - h).bias) / (2.0 * h);
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-5 * b.abs() + 1e-15;
        assert!(close(c.output_per_input_volt, dvd), "{vd} {vb}");
        assert!(close(c.output_per_bias_volt, dvb), "{vd} {vb}");
        assert!(close(c.bias_per_bias_volt, dbb), "{vd} {vb}");
    }
}

/// The output saturates at ±Iabc: the tanh's limit.
#[test]
fn the_output_saturates_at_the_bias_current() {
    let ota = BipolarPair::CA3080;
    let vb = ota.bias_volts_at(100e-6);
    assert!((ota.currents(1.0, vb).output - 100e-6).abs() < 1e-9);
    assert!((ota.currents(-1.0, vb).output + 100e-6).abs() < 1e-9);
}
