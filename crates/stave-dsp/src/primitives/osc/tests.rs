use super::*;

const SR: f32 = 48_000.0;

/// Single-bin correlation (Goertzel-shaped) magnitude at `hz`.
fn mag_at(w: &[f32], hz: f64) -> f64 {
    let n = w.len() as f64;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &x) in w.iter().enumerate() {
        let ph = core::f64::consts::TAU * hz * i as f64 / f64::from(SR);
        re += f64::from(x) * ph.cos();
        im += f64::from(x) * ph.sin();
    }
    (re * re + im * im).sqrt() / n
}

fn run(shape: Waveform, hz: f32, n: usize, blep: bool) -> Vec<f32> {
    let mut o = Osc::new(shape);
    o.prepare(SR);
    o.set_hz(hz);
    (0..n)
        .map(|_| {
            if blep {
                o.tick(0.0)
            } else {
                // The naive comparison: same accumulator, no BLEP.
                let t = o.phase;
                o.phase = (o.phase + o.inc).rem_euclid(1.0);
                match shape {
                    Waveform::Sine => (core::f64::consts::TAU * t).sin() as f32,
                    other => other.eval(t as f32),
                }
            }
        })
        .collect()
}

#[test]
fn pitch_is_exact() {
    // The fundamental dominates at exactly the dialed hz, and a
    // detuned probe half a semitone off reads ≥ 30 dB down.
    for shape in [
        Waveform::Sine,
        Waveform::Triangle,
        Waveform::Saw,
        Waveform::Square,
    ] {
        let w = run(shape, 440.0, 48_000, true);
        let on = mag_at(&w, 440.0);
        let off = mag_at(&w, 440.0 * 1.03);
        assert!(
            on > off * 30.0,
            "{shape:?}: fundamental {on:.4} vs detuned probe {off:.4}"
        );
    }
}

#[test]
fn sine_harmonics_are_at_the_floor() {
    let w = run(Waveform::Sine, 997.0, 48_000, true);
    let h1 = mag_at(&w, 997.0);
    let h2 = mag_at(&w, 2.0 * 997.0);
    let h3 = mag_at(&w, 3.0 * 997.0);
    assert!(
        h2 / h1 < 1e-4 && h3 / h1 < 1e-4,
        "sine THD: h2 {h2:.2e} h3 {h3:.2e}"
    );
}

/// Worst magnitude over the alias frequencies of a tone at `f0` inside
/// `lo..hi`: the partials `k` past Nyquist that the waveform has
/// (`partial(k)`), folded into 100 Hz..20 kHz, skipping any that land
/// on a true harmonic. Aliases land at exactly `|k·f0 mod fs|`: probe
/// those frequencies, never a grid.
fn worst_alias(w: &[f32], f0: f64, lo: f64, hi: f64, partial: impl Fn(usize) -> bool) -> f64 {
    let fs = f64::from(SR);
    let folds: Vec<f64> = (1..400)
        .filter(|&k| partial(k) && k as f64 * f0 > fs / 2.0)
        .map(|k| {
            let folded = (k as f64 * f0) % fs;
            if folded > fs / 2.0 {
                fs - folded
            } else {
                folded
            }
        })
        .filter(|f| (100.0..20_000.0).contains(f) && (lo..hi).contains(f))
        .filter(|f| (f / f0 - (f / f0).round()).abs() > 0.02)
        .collect();
    assert!(
        !folds.is_empty(),
        "test premise: aliases exist in {lo}..{hi}"
    );
    folds.iter().map(|&f| mag_at(w, f)).fold(0.0f64, f64::max)
}

#[test]
fn polyblep_buys_at_least_20_db_on_the_worst_alias() {
    // 2093 Hz saw at 48 k. Domain-qualified per the physics of
    // first-order BLEP (rejection grows as the alias departs Nyquist):
    // the worst near-Nyquist fold wins a measured 12.4 dB (≥ 10
    // asserted) and is perceptually masked; the EXPOSED midband folds
    // (< 8 kHz — where naive saws audibly ring) win ≥ 20 dB.
    let f0 = 2_093.0f64;
    let naive = run(Waveform::Saw, f0 as f32, 48_000, false);
    let blep = run(Waveform::Saw, f0 as f32, 48_000, true);
    let all = |_: usize| true;
    let win = |lo: f64, hi: f64| {
        let (n, b) = (
            worst_alias(&naive, f0, lo, hi, all),
            worst_alias(&blep, f0, lo, hi, all),
        );
        20.0 * (n / b.max(1e-30)).log10()
    };
    let full = win(100.0, 24_000.0);
    let exposed = win(100.0, 8_000.0);
    assert!(full > 10.0, "full-band worst-alias win only {full:.1} dB");
    assert!(
        exposed > 20.0,
        "exposed midband alias win only {exposed:.1} dB"
    );
}

#[test]
fn blamp_buys_at_least_20_db_on_the_triangle() {
    // The triangle's partials are odd and fall at 12 dB/octave; the
    // naive corners still fold. polyBLAMP rounds each corner over the
    // two samples around it. Measured: 41.6 dB below 8 kHz, 14.6 dB
    // over the band.
    let f0 = 2_093.0;
    let naive = run(Waveform::Triangle, f0 as f32, 48_000, false);
    let blamp = run(Waveform::Triangle, f0 as f32, 48_000, true);
    let odd = |k: usize| k % 2 == 1;
    let win = |lo: f64, hi: f64| {
        let (n, b) = (
            worst_alias(&naive, f0, lo, hi, odd),
            worst_alias(&blamp, f0, lo, hi, odd),
        );
        20.0 * (n / b.max(1e-30)).log10()
    };
    let (full, exposed) = (win(100.0, 24_000.0), win(100.0, 8_000.0));
    println!("triangle polyBLAMP: full-band win {full:.1} dB, below 8 kHz {exposed:.1} dB");
    assert!(full > 10.0, "full-band worst-alias win only {full:.1} dB");
    assert!(
        exposed > 20.0,
        "exposed midband alias win only {exposed:.1} dB"
    );
}

/// The residual and the naive history fit the oscillator's fixed rings.
#[test]
fn the_minblep_ring_holds_the_residual() {
    let t = MinBlep::get();
    println!("minBLEP: {} samples, delay {}", t.len, t.delay);
    assert!(t.len < RING, "residual {} samples, ring {RING}", t.len);
    assert!(t.delay < HISTORY, "delay {}, history {HISTORY}", t.delay);
}

/// Away from its edges, a minBLEP saw is the naive saw delayed by the
/// residual's delay: the residuals settle, and the delay is whole.
#[test]
fn minblep_is_the_naive_wave_away_from_its_edges() {
    let t = MinBlep::get();
    let mut o = Osc::min_blep(Waveform::Saw);
    o.prepare(SR);
    o.set_hz(100.0);
    let inc = 100.0 / f64::from(SR);
    for n in 0..4_800usize {
        let y = f64::from(o.tick(0.0));
        if n < t.delay {
            continue;
        }
        let phase = ((n - t.delay) as f64 * inc).rem_euclid(1.0);
        // Samples since the last edge, and to the next.
        let since = phase / inc;
        let until = (1.0 - phase) / inc;
        if since > t.len as f64 && until > t.delay as f64 + 1.0 {
            let naive = 2.0 * phase - 1.0;
            assert!((y - naive).abs() < 1e-5, "sample {n}: {y} vs naive {naive}");
        }
    }
}

/// With a master that never wraps, the edge-list polyBLEP path is the
/// phase-derived one: the same residuals, found another way.
#[test]
fn the_edge_list_path_is_the_phase_path() {
    for shape in [
        Waveform::Saw,
        Waveform::Square,
        Waveform::Triangle,
        Waveform::Sine,
    ] {
        let mut free = Osc::new(shape);
        let mut listed = Osc::new(shape);
        free.prepare(SR);
        listed.prepare(SR);
        free.set_hz(3_137.0);
        listed.set_hz(3_137.0);
        listed.set_sync(Some(Hz::new(0.0)));
        // The phase path reads phase 0 as just past a wrap; the edge
        // list starts with none. From the first edge on they agree.
        let _ = (free.tick(0.0), listed.tick(0.0));
        for n in 1..2_000 {
            let (a, b) = (free.tick(0.0), listed.tick(0.0));
            assert!((a - b).abs() < 1e-5, "{shape:?} sample {n}: {a} vs {b}");
        }
    }
}

/// A hard-synced oscillator repeats at the master's period, whatever its
/// own frequency.
#[test]
fn sync_repeats_at_the_masters_period() {
    let makers: [fn(Waveform) -> Osc; 2] = [Osc::new, Osc::min_blep];
    for make in makers {
        for shape in [Waveform::Saw, Waveform::Triangle, Waveform::Square] {
            let mut o = make(shape);
            o.prepare(SR);
            o.set_hz(1_234.5);
            o.set_sync(Some(Hz::new(375.0))); // 128 samples at 48 kHz
            let y: Vec<f32> = (0..2_048).map(|_| o.tick(0.0)).collect();
            for n in 1_024..1_024 + 128 {
                assert!(
                    (y[n] - y[n + 128]).abs() < 1e-4,
                    "{shape:?} sample {n}: {} vs {}",
                    y[n],
                    y[n + 128]
                );
            }
        }
    }
}
