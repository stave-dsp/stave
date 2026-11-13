//! Test measurements for the instruments: a Hann-windowed phasor, a
//! frequency from its phase advance, a T60 from its level's slope, and
//! the strongest peak near a frequency.

use core::f64::consts::TAU;

/// The Hann-windowed DFT of `y[start..start + len]` at `hz`, scaled so a
/// sinusoid of amplitude `A` reads `A`: `(amplitude, phase)`.
pub(crate) fn hann(y: &[f64], fs: f64, hz: f64, start: usize, len: usize) -> (f64, f64) {
    let (mut re, mut im, mut sum) = (0.0, 0.0, 0.0);
    let w = TAU * hz / fs;
    for (i, v) in y[start..start + len].iter().enumerate() {
        let win = 0.5 - 0.5 * (TAU * (i as f64 + 0.5) / len as f64).cos();
        let (s, c) = (w * (start + i) as f64).sin_cos();
        re += win * v * c;
        im -= win * v * s;
        sum += win;
    }
    (2.0 * (re * re + im * im).sqrt() / sum, im.atan2(re))
}

/// The frequency of the component near `hz` in `y`, from its phase
/// advance between Hann windows of `len` samples at `t1` and `t2`
/// (`|f − hz|·(t2 − t1)/fs` must stay under half a cycle).
pub(crate) fn frequency(y: &[f64], fs: f64, hz: f64, t1: usize, t2: usize, len: usize) -> f64 {
    let (_, p1) = hann(y, fs, hz, t1, len);
    let (_, p2) = hann(y, fs, hz, t2, len);
    // The phasor is referred to absolute time: a component exactly at
    // `hz` keeps its phase.
    let mut d = p2 - p1;
    d -= TAU * (d / TAU).round();
    hz + d / (TAU * (t2 - t1) as f64 / fs)
}

/// `a` relative to `b`, in cents.
pub(crate) fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

/// The T60 of the component at `hz`, by a least-squares line through
/// its level (dB) in Hann windows of `len` samples hopping by `len/2`
/// from `from` to `to` (samples).
pub(crate) fn t60(y: &[f64], fs: f64, hz: f64, len: usize, from: usize, to: usize) -> f64 {
    let (mut n, mut st, mut sl, mut stt, mut stl) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut start = from;
    while start + len <= to.min(y.len()) {
        let (a, _) = hann(y, fs, hz, start, len);
        let t = (start + len / 2) as f64 / fs;
        let l = 20.0 * a.max(1e-300).log10();
        n += 1.0;
        st += t;
        sl += l;
        stt += t * t;
        stl += t * l;
        start += len / 2;
    }
    let slope = (n * stl - st * sl) / (n * stt - st * st);
    -60.0 / slope
}

/// The frequency of the largest Hann-windowed magnitude within `±span`
/// Hz of `hz` (searched on a fine grid, refined by a parabola).
pub(crate) fn peak(y: &[f64], fs: f64, hz: f64, span: f64, start: usize, len: usize) -> f64 {
    let steps = 200;
    let at = |k: i32| hz + span * f64::from(k) / f64::from(steps);
    let mag = |f: f64| hann(y, fs, f, start, len).0;
    let (mut best, mut bk) = (-1.0, 0);
    for k in -steps..=steps {
        let m = mag(at(k));
        if m > best {
            best = m;
            bk = k;
        }
    }
    let (l, c, r) = (mag(at(bk - 1)), best, mag(at(bk + 1)));
    let denom = l - 2.0 * c + r;
    let shift = if denom.abs() > 0.0 { 0.5 * (l - r) / denom } else { 0.0 };
    at(bk) + shift * span / f64::from(steps)
}
