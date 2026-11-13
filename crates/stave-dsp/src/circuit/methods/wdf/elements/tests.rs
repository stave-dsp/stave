use super::*;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// A source into a resistor, terminated by an open circuit (b = a):
/// the divider has no load, so the open end reads the source.
#[test]
fn an_unloaded_source_reads_its_voltage() {
    let mut tree = Parallel::new(
        ResistiveSource {
            ohms: 1e3,
            volts: 2.0,
        },
        Resistor { ohms: 1e12 },
    );
    let v = sample(&mut tree, &mut Open);
    assert!(close(v, 2.0, 1e-6), "{v}");
}

/// RC low-pass to a step: the bilinear capacitor tracks the exact
/// 1 − exp(−t/RC) closely when RC spans many samples.
#[test]
fn rc_step_follows_its_time_constant() {
    let (rate, r, c) = (192_000.0f32, 1e3, 100e-9);
    let mut cap = Capacitor::new(c);
    cap.prepare(rate);
    let mut tree = Parallel::new(
        ResistiveSource {
            ohms: r,
            volts: 1.0,
        },
        cap,
    );
    let mut worst: f64 = 0.0;
    for n in 1..=960 {
        let v = sample(&mut tree, &mut Open);
        let t = n as f64 / f64::from(rate);
        worst = worst.max((v - (1.0 - (-(t - 0.5 / f64::from(rate)) / (r * c)).exp())).abs());
    }
    assert!(worst < 2e-3, "{worst}");
}

/// An RL divider: a 1 V step through R into L; the inductor's voltage
/// decays as exp(−tR/L), its current rises to 1/R.
#[test]
fn rl_step_follows_its_time_constant() {
    let (rate, r, l) = (192_000.0f32, 100.0, 10e-3);
    let mut coil = Inductor::new(l);
    coil.prepare(rate);
    let mut tree = Series::new(
        ResistiveSource {
            ohms: r,
            volts: 1.0,
        },
        Inverter { inner: coil },
    );
    // The series pair closed on itself: a short root.
    let mut worst: f64 = 0.0;
    for n in 1..=1920 {
        let a = tree.reflected();
        let b = Short.reflect(a, tree.resistance());
        tree.incident(b);
        // The inverted coil's port voltage, from the series adaptor's
        // split; the coil's own voltage is its negative.
        let (rl, rr) = (tree.left.resistance(), tree.right.resistance());
        let alpha = tree.right_wave + rr * (b - tree.up_wave) / (rl + rr);
        let v_l = -0.5 * (alpha + tree.right_wave);
        let t = n as f64 / f64::from(rate);
        let exact = (-(t - 0.5 / f64::from(rate)) * r / l).exp();
        worst = worst.max((v_l - exact).abs());
    }
    assert!(worst < 2e-3, "{worst}");
}

/// A series pair is one resistor of the sum, and its split matches the
/// voltage divider.
#[test]
fn series_divides_like_a_divider() {
    let mut tree = Series::new(
        ResistiveSource {
            ohms: 1e3,
            volts: 3.0,
        },
        Resistor { ohms: 2e3 },
    );
    assert_eq!(tree.resistance(), 3e3);
    // Closed by a short: the loop current is 3 V / 3 kΩ.
    let a = tree.reflected();
    let b = Short.reflect(a, tree.resistance());
    tree.incident(b);
    // The resistor's arriving wave α = v + R·j with v = R·j' … read its
    // voltage through the adaptor: v_r = (α_r + 0)/2.
    let alpha_r = tree.right_wave + 2e3 * (b - tree.up_wave) / 3e3;
    assert!(close(0.5 * alpha_r, -2.0, 1e-12), "{alpha_r}");
}

/// The inverter flips a source's polarity.
#[test]
fn an_inverter_turns_a_source_round() {
    let mut tree = Inverter {
        inner: ResistiveSource {
            ohms: 50.0,
            volts: 1.5,
        },
    };
    assert!(close(sample(&mut tree, &mut Open), -1.5, 1e-15));
}

/// A 1:4 transformer: a source behind 100 Ω drives 1.6 kΩ on the
/// secondary — matched (1.6k/16 = 100), so the load sees n·V/2.
#[test]
fn a_transformer_matches_and_steps_up() {
    let mut tree = Series::new(
        ResistiveSource {
            ohms: 100.0,
            volts: 1.0,
        },
        Transformer {
            secondary: Resistor { ohms: 1_600.0 },
            turns: 4.0,
        },
    );
    assert!(close(tree.right.resistance(), 100.0, 1e-12));
    let a = tree.reflected();
    let b = Short.reflect(a, tree.resistance());
    tree.incident(b);
    // The primary's arriving wave; the secondary's is n times it.
    let alpha_primary = tree.right_wave + 100.0 * (b - tree.up_wave) / 200.0;
    let v_secondary = 0.5 * (4.0 * alpha_primary + 0.0);
    assert!(close(v_secondary.abs(), 2.0, 1e-12), "{v_secondary}");
}

/// Ideal sources and the passive roots read their defining laws.
#[test]
fn roots_hold_their_laws() {
    let ohms = 470.0;
    let tree = || ResistiveSource { ohms, volts: 1.0 };
    // An ideal voltage source of 2 V at the root of a 1 V source: 2 V.
    assert!(close(sample(&mut tree(), &mut VoltageSource { volts: 2.0 }), 2.0, 1e-15));
    // A short holds 0 V; an open reads the Thévenin 1 V.
    assert!(close(sample(&mut tree(), &mut Short), 0.0, 1e-15));
    assert!(close(sample(&mut tree(), &mut Open), 1.0, 1e-15));
    // 1 mA pushed into the tree: v = 1 V + R·1 mA.
    let v = sample(&mut tree(), &mut CurrentSource { amps: 1e-3 });
    assert!(close(v, 1.0 + ohms * 1e-3, 1e-12), "{v}");
    let mut switch = Switch { closed: true };
    assert!(close(sample(&mut tree(), &mut switch), 0.0, 1e-15));
    switch.closed = false;
    assert!(close(sample(&mut tree(), &mut switch), 1.0, 1e-15));
}

/// A resistive current source is the Norton twin of a resistive voltage
/// source: 2 mA in 1 kΩ reads 2 V open.
#[test]
fn a_resistive_current_source_is_its_norton_twin() {
    let mut tree = ResistiveCurrentSource {
        ohms: 1e3,
        amps: 2e-3,
    };
    assert!(close(sample(&mut tree, &mut Open), 2.0, 1e-12));
    // Shorted, it delivers its 2 mA: j into it is −2 mA.
    let a = tree.reflected();
    let b = Short.reflect(a, 1e3);
    assert!(close((b - a) / 2e3, -2e-3, 1e-15));
}

/// The bilinear capacitor and inductor in parallel, rung from a charge:
/// an LC tank's energy stays put (the trapezoidal rule is lossless).
#[test]
fn an_lc_tank_keeps_its_energy() {
    let rate = 48_000.0f32;
    let (l, c) = (10e-3, 1e-6);
    let mut cap = Capacitor::new(c);
    cap.prepare(rate);
    cap.set_volts(1.0);
    let mut coil = Inductor::new(l);
    coil.prepare(rate);
    let mut tank = Parallel::new(cap, coil);
    let mut peak: f64 = 0.0;
    let mut tail: f64 = 0.0;
    for n in 0..48_000 {
        let v = sample(&mut tank, &mut Open);
        if n < 1_000 {
            peak = peak.max(v.abs());
        } else if n >= 47_000 {
            tail = tail.max(v.abs());
        }
    }
    assert!(close(tail, peak, 1e-3 * peak), "{peak} {tail}");
}
