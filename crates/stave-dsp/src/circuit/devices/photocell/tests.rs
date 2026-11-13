use super::*;

fn run(cell: &mut Vactrol, amps: f64, samples: usize) -> f64 {
    (0..samples).map(|_| cell.step(amps)).last().unwrap_or(cell.ohms())
}

/// The first sample at which `cell` crosses `ohms` (rising or falling).
fn time_to(cell: &mut Vactrol, amps: f64, ohms: f64, rate: f64) -> f64 {
    let start = cell.ohms();
    for n in 1..(rate as usize * 2) {
        let r = cell.step(amps);
        if (start < ohms) == (r >= ohms) {
            return n as f64 / rate;
        }
    }
    f64::INFINITY
}

#[test]
fn rest_is_where_the_dynamics_settle() {
    for part in [Najnudel::VTL5C3, Najnudel::NSL32] {
        for amps in [0.5e-3, 5e-3, 20e-3] {
            let mut a = Vactrol::new(part);
            let settled = run(&mut a, amps, 96_000);
            let rest = Vactrol::new(part).settle(amps);
            // The light history drifts a little in the two seconds run.
            assert!((settled / rest - 1.0).abs() < 1e-4, "{amps}: {settled} vs {rest}");
            assert_eq!(a.capped(), 0);
        }
    }
}

#[test]
fn attack_is_faster_than_release_and_release_remembers_the_light() {
    let rate = 48_000.0;
    let mut cell = Vactrol::new(Najnudel::VTL5C3);
    cell.prepare(rate as f32);
    // On at 10 mA: within milliseconds.
    let on = time_to(&mut cell, 0.010, 10e3, rate);
    run(&mut cell, 0.010, 48_000);
    // Off: 100 kohm takes longer than the attack took to reach 10k.
    let off_10 = time_to(&mut cell, 0.0, 100e3, rate);
    assert!(off_10 > 2.0 * on, "on {on}, off {off_10}");
    // After 40 mA the release to 100 kohm is slower still (program dependence).
    run(&mut cell, 0.0, 480_000);
    run(&mut cell, 0.040, 48_000);
    let off_40 = time_to(&mut cell, 0.0, 100e3, rate);
    assert!(off_40 > off_10, "from 40 mA {off_40}, from 10 mA {off_10}");
}

#[test]
fn the_cell_converges_with_rate() {
    // A turn-on then turn-off, at 4 rates; TR-BDF2 is second order: the
    // error against a 16× finer run falls about 4× per doubling.
    let trace = |rate: f64| {
        let mut cell = Vactrol::new(Najnudel::VTL5C3);
        cell.prepare(rate as f32);
        let n = (rate * 0.005) as usize;
        let on = run(&mut cell, 0.020, n);
        let off = run(&mut cell, 0.0, n);
        (on.ln(), off.ln())
    };
    let fine = trace(48_000.0 * 32.0);
    let errors: Vec<f64> = [24_000.0, 48_000.0, 96_000.0]
        .iter()
        .map(|&r| {
            let (on, off) = trace(r);
            (on - fine.0).abs().max((off - fine.1).abs())
        })
        .collect();
    assert!(errors[0] < 0.02, "{errors:?}");
    for w in errors.windows(2) {
        assert!(w[1] < w[0] / 3.0, "second order: {errors:?}");
    }
}

#[test]
fn light_history_raises_the_resistance_by_the_parts_ratio() {
    let mut dark = Vactrol::new(Najnudel::VTL5C3);
    let mut light = Vactrol::new(Najnudel::VTL5C3);
    light.set_adaptation(1.0);
    let ratio = |amps: f64, a: &mut Vactrol, b: &mut Vactrol| b.settle(amps) / a.settle(amps);
    let low = ratio(0.3e-3, &mut dark, &mut light);
    let high = ratio(25e-3, &mut dark, &mut light);
    assert!(low > high && (1.2..1.45).contains(&low) && (1.08..1.25).contains(&high), "{low} {high}");
    // The NSL-32's sheet has no light-history curve: none.
    let mut nsl = Vactrol::new(Najnudel::NSL32);
    let r0 = nsl.settle(1e-3);
    nsl.set_adaptation(1.0);
    assert_eq!(nsl.settle(1e-3), r0);
}

#[test]
fn adaptation_drifts_over_hours() {
    let mut cell = Vactrol::new(Najnudel::VTL5C3);
    cell.prepare(100.0); // a slow rate: the history's clock is hours
    run(&mut cell, 0.040, 100 * 3600);
    let after_an_hour = cell.adaptation();
    run(&mut cell, 0.040, 100 * 3600 * 23);
    assert!((0.15..0.25).contains(&after_an_hour), "{after_an_hour}");
    // 40 mA is g/(g + g₁) = 0.97 of the way to fully light adapted.
    assert!(cell.adaptation() > 0.96, "{}", cell.adaptation());
}

#[test]
fn led_driver_closes_the_junction_and_its_resistance() {
    let driver = LedDriver::new(Najnudel::VTL5C3.led, 100.0);
    for volts in [1.6, 2.0, 3.0, 5.0, 12.0] {
        let i = driver.amps(volts);
        let back = i * 100.0 + Najnudel::VTL5C3.led_volts(i);
        assert!((back - volts).abs() < 1e-9 * volts, "{volts}: {back}");
    }
    assert_eq!(driver.amps(0.0), 0.0);
}
