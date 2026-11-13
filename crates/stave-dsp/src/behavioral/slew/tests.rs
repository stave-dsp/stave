use super::*;

#[test]
fn steps_ramp_at_the_set_rates_and_slow_signals_pass() {
    // The definition: a full-scale step up ramps at exactly
    // `rise_per_ms` full-scale units per millisecond, a step down at
    // `fall_per_ms`, at any rate; a sine whose steepest slope is under
    // both limits passes untouched. Budget 1e-4 (f32 accumulation over the ramp).
    for fs in [44_100.0f32, 48_000.0, 96_000.0] {
        let (rise, fall) = (2.0f32, 0.5f32);
        let mut s = Slew::new(rise, fall);
        s.prepare(fs);
        let per_sample = |per_ms: f32| per_ms * 1000.0 / fs;
        let up = (1.0 / per_sample(rise)) as usize;
        for n in 1..=up {
            let y = s.tick(1.0);
            let want = n as f32 * per_sample(rise);
            assert!((y - want).abs() < 1e-4, "{fs} Hz rise, sample {n}: {y} vs {want}");
        }
        for _ in 0..4 {
            s.tick(1.0);
        }
        assert_eq!(s.tick(1.0), 1.0, "settles on the input");
        let down = (1.0 / per_sample(fall)) as usize;
        for n in 1..=down {
            let y = s.tick(0.0);
            let want = 1.0 - n as f32 * per_sample(fall);
            assert!((y - want).abs() < 1e-4, "{fs} Hz fall, sample {n}: {y} vs {want}");
        }

        // 100 Hz at 0.5: steepest slope 2π·100·0.5 ≈ 314 /s = 0.31 /ms,
        // under both limits.
        let mut s = Slew::new(rise, fall);
        s.prepare(fs);
        for i in 0..fs as usize / 10 {
            let x = 0.5 * (core::f32::consts::TAU * 100.0 * i as f32 / fs).sin();
            let y = s.tick(x);
            assert!((y - x).abs() < 1e-5, "{fs} Hz: slow sine slewed at {i}: {y} vs {x}");
        }
    }
}
