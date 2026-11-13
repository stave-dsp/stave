use super::*;

fn input(n: usize) -> Vec<f32> {
    (0..n).map(|i| ((i * 37) % 101) as f32 / 50.0 - 1.0).collect()
}

#[test]
fn no_condition_is_a_pass_through() {
    let mut s = Spacing::new(Condition::NONE, 1);
    assert!(!s.is_enabled());
    assert_eq!(s.latency(), 0);
    for x in input(1000) {
        assert_eq!(s.tick(x).to_bits(), x.to_bits());
    }
}

#[test]
fn between_dropouts_it_is_a_pure_delay() {
    let rare = Condition {
        dropouts_per_minute: 1e-9,
        depth_um: 1.0,
        duration_ms: 5.0,
        ..Condition::NONE
    };
    let mut s = Spacing::new(rare, 2);
    let d = s.latency() as usize;
    assert_eq!(d, 240);
    let x = input(5_000);
    let y: Vec<f32> = x.iter().map(|&v| s.tick(v)).collect();
    for n in d..x.len() {
        assert_eq!(y[n], x[n - d]);
    }
}

#[test]
fn the_taps_are_the_law_at_dc_and_symmetric() {
    let half = 240;
    let window: Vec<f64> = (0..2 * half + 1)
        .map(|j| {
            let x = std::f64::consts::TAU * j as f64 / (2 * half) as f64;
            0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos()
        })
        .collect();
    let mut taps = vec![0.0; 2 * half + 1];
    design(0.0, &window, half, &mut taps);
    assert_eq!(taps.iter().sum::<f64>(), 1.0);
    design(1.5, &window, half, &mut taps);
    for k in 0..half {
        assert!((taps[k] - taps[2 * half - k]).abs() < 1e-15);
    }
    // At Nyquist the law is e^(−πc).
    let nyquist: f64 = taps.iter().enumerate().map(|(j, h)| if j % 2 == 0 { *h } else { -*h }).sum();
    assert!((nyquist.abs().ln() + std::f64::consts::PI * 1.5).abs() < 0.03, "{nyquist}");
}

#[test]
fn dropouts_replay_from_their_seed_and_are_reported() {
    let make = || Spacing::new(Condition::WORN, 7);
    let (mut a, mut b) = (make(), make());
    let x = input(96_000);
    let mut events = 0;
    let ya: Vec<f32> = x
        .iter()
        .map(|&v| {
            let y = a.tick(v);
            events += usize::from(a.started().is_some());
            y
        })
        .collect();
    let yb: Vec<f32> = x.iter().map(|&v| b.tick(v)).collect();
    assert_eq!(ya, yb);
    assert!(events > 0 && events < 10, "{events} in 2 s at one a second");
    a.reset();
    let yc: Vec<f32> = x.iter().map(|&v| a.tick(v)).collect();
    assert_eq!(ya, yc);
    // A joint is reported after the tick that follows it.
    a.splice();
    a.tick(0.0);
    assert!(a.started().is_some_and(|d| d.joint));
    a.tick(0.0);
    assert!(a.started().is_none());
}
