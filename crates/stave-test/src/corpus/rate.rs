//! Changing the rate of program material: windowed-sinc conversion,
//! offline (the corpus's files are 44.1 and 48 kHz; a session or a
//! capture runs at its own rate).

/// Windowed-sinc rate conversion (Blackman window, 32 zero crossings
/// each side, cut-off at 0.95 of the lower Nyquist). Offline, for
/// material only.
pub fn convert_rate(x: &[f32], from: f64, to: f64) -> Vec<f32> {
    const HALF: i64 = 32;
    let ratio = to / from;
    let cutoff = 0.95 * ratio.min(1.0);
    let out_len = (x.len() as f64 * ratio).round() as usize;
    let len = x.len() as i64;
    (0..out_len)
        .map(|n| {
            let t = n as f64 / ratio;
            let centre = t.floor() as i64;
            let reach = (HALF as f64 / cutoff).ceil() as i64;
            let mut acc = 0.0;
            for i in (centre - reach + 1).max(0)..=(centre + reach).min(len - 1) {
                let d = t - i as f64;
                let u = d * cutoff;
                if u.abs() >= HALF as f64 {
                    continue;
                }
                let sinc = if u == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u)
                };
                let w = u / HALF as f64; // −1 … 1
                let win = 0.42 + 0.5 * (std::f64::consts::PI * w).cos()
                    + 0.08 * (2.0 * std::f64::consts::PI * w).cos();
                acc += f64::from(x[i as usize]) * sinc * win * cutoff;
            }
            acc as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_conversion_keeps_a_tone() {
        let (from, to) = (44_100.0, 48_000.0);
        let x: Vec<f32> = (0..44_100)
            .map(|i| (std::f64::consts::TAU * 1_000.0 * f64::from(i) / from).sin() as f32 * 0.5)
            .collect();
        let y = convert_rate(&x, from, to);
        assert_eq!(y.len(), 48_000);
        // Compare against the ideal tone at the new rate, away from the edges.
        let err = (2_000..46_000)
            .map(|i| {
                let ideal = (std::f64::consts::TAU * 1_000.0 * i as f64 / to).sin() * 0.5;
                (f64::from(y[i]) - ideal).abs()
            })
            .fold(0.0, f64::max);
        assert!(err < 1e-3, "{err}");
    }
}
