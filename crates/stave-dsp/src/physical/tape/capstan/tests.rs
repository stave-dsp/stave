use super::*;

fn input(n: usize) -> Vec<f32> {
    (0..n).map(|i| ((i * 37) % 101) as f32 / 50.0 - 1.0).collect()
}

#[test]
fn unconfigured_is_a_pass_through() {
    let mut c = Capstan::new(1);
    assert!(!c.is_enabled());
    assert_eq!(c.latency(), 0);
    for x in input(1000) {
        assert_eq!(c.tick(x).to_bits(), x.to_bits());
    }
}

#[test]
fn at_speed_it_is_a_pure_delay_of_its_latency() {
    // A motor and splices configured but idle: the read sits on whole
    // samples, so the cubic returns them exactly.
    let mut c = Capstan::new(2).with_motor(Motor::STUDIO);
    c.prepare(48_000.0);
    let d = c.latency() as usize;
    assert!(d > 4_800, "the spin-up's 0.1 s is in the base: {d}");
    let x = input(20_000);
    let y: Vec<f32> = x.iter().map(|&v| c.tick(v)).collect();
    for n in d..x.len() {
        assert_eq!(y[n], x[n - d], "{n}");
    }
}

#[test]
fn a_seed_replays() {
    let make = || Capstan::new(9).with_drift(Drift::LOOSE).with_splices(Splices { per_minute: 600.0, ..Splices::EDITED });
    let (mut a, mut b) = (make(), make());
    let x = input(48_000);
    let ya: Vec<f32> = x.iter().map(|&v| a.tick(v)).collect();
    let yb: Vec<f32> = x.iter().map(|&v| b.tick(v)).collect();
    assert_eq!(ya, yb);
    a.reset();
    let yc: Vec<f32> = x.iter().map(|&v| a.tick(v)).collect();
    assert_eq!(ya, yc);
    assert_eq!(a.clamps(), 0);
}

#[test]
fn stopping_falls_silent_and_starting_comes_back_in_time() {
    let mut c = Capstan::new(3).with_motor(Motor::CONSUMER);
    c.prepare(8_000.0);
    let d = c.latency() as usize;
    let x = input(200_000);
    let mut y = Vec::with_capacity(x.len());
    for (n, &v) in x.iter().enumerate() {
        if n == 10_000 {
            c.stop();
        }
        if n == 60_000 {
            assert!(c.is_stopped());
            c.start();
        }
        y.push(c.tick(v));
    }
    assert!(y[40_000..60_000].iter().all(|&v| v == 0.0));
    // 25 spin-up time constants later the lag is back on its base.
    let settled = 60_000 + 25 * 2_400;
    assert!((c.lag() * 8_000.0 - d as f64).abs() < 0.05, "{} vs {d}", c.lag() * 8_000.0);
    for n in settled..x.len() {
        assert!((y[n] - x[n - d]).abs() < 2e-3, "{n}: {} vs {}", y[n], x[n - d]);
    }
}

#[test]
fn a_splice_crosses_the_gap_in_its_time_and_jumps() {
    // A 45° cut across quarter-inch tape at 7.5 ips crosses the gap in
    // w/(v·tan θ) = 33.3 ms; a ramp input shows the jump exactly (the
    // cubic is exact on a line).
    let mut c = Capstan::new(5).with_splices(Splices::EDITED);
    c.prepare(48_000.0);
    let crossing = (Splices::EDITED.crossing(Machine::ips(7.5)).get() * 48_000.0).round() as usize;
    assert_eq!(crossing, 1600);
    let mut n = 0.0f32;
    for _ in 0..4_000 {
        c.tick(n);
        n += 1.0;
    }
    let before = c.lag();
    c.splice();
    let mut ticks = 0;
    loop {
        c.tick(n);
        n += 1.0;
        ticks += 1;
        if (c.lag() - before).abs() > 1e-6 {
            break;
        }
    }
    assert_eq!(ticks, crossing);
    let jump = before - c.lag();
    assert!(jump.abs() <= 0.02 && jump != 0.0, "{jump}");
    let y = c.tick(n);
    let want = f64::from(n) - c.lag() * 48_000.0;
    assert!((f64::from(y) - want).abs() < 0.01, "{y} vs {want}");
}
