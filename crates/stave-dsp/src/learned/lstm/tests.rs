use super::*;

/// A one-unit LSTM by hand: gates from x only, states start at (0.1, 0.2).
fn one_unit() -> Lstm {
    // W (4 × 2: [x, h]) rows i f g o, b (4), h0, c0, head w, head b.
    #[rustfmt::skip]
    let w = [
        1.0, 0.0,   // i
        0.0, 0.0,   // f
        2.0, 0.0,   // g
        0.0, 0.0,   // o
        0.0, 0.0, 0.0, 0.0,
        0.1, 0.2,
        3.0, 0.5,
    ];
    Lstm::build(1, 1, 1, &w, 48_000.0).unwrap()
}

#[test]
fn one_step_follows_the_cell_equations() {
    let mut net = one_unit();
    let x = 0.3f32;
    let sigmoid = |v: f32| 1.0 / (1.0 + (-v).exp());
    let c = 0.5 * 0.2 + sigmoid(x) * (2.0 * x).tanh();
    let h = 0.5 * c.tanh();
    assert!((net.tick(x) - (3.0 * h + 0.5)).abs() < 1e-6);
    net.reset();
    assert!(
        (net.tick(x) - (3.0 * h + 0.5)).abs() < 1e-6,
        "reset returns to h0, c0"
    );
}

#[test]
fn weight_count_is_checked() {
    assert_eq!(
        Lstm::build(1, 2, 1, &[0.0; 10], 48_000.0).unwrap_err(),
        // 4·2·3 + 8 + 2 + 2 + 2 + 1
        LoadError::Weights {
            expected: 39,
            found: 10
        }
    );
    assert!(matches!(
        Lstm::build(2, 2, 1, &[], 48_000.0),
        Err(LoadError::Unsupported(_))
    ));
}

#[test]
fn a_foreign_rate_is_silence_until_chosen() {
    let mut net = one_unit();
    net.prepare(96_000.0);
    assert_eq!(net.tick(0.3), 0.0);
    net.set_rate_policy(RatePolicy::Any);
    assert_ne!(net.tick(0.3), 0.0);
    assert_eq!(net.tail(), Tail::Infinite);
}
