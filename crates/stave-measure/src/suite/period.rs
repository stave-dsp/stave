//! Coherent periods and collision-free bins (DSP.md measurement
//! principle 1).
//!
//! A [`Period`] is a whole number of samples at every rate a model is
//! measured at, so a tone on bin `k` (`k / seconds` Hz) completes `k`
//! whole cycles in it: one rectangular DFT gives exact amplitudes and
//! phases. Periods are multiples of 1/300 s, which every shipping rate
//! (44.1, 48, 88.2, 96, 192 kHz) and every power-of-two multiple of
//! 48 kHz divides.
//!
//! A tone's bin is then chosen so that its harmonics — and their aliases
//! above Nyquist at each rate — land on distinct bins ([`tone_bin`]), and a
//! two-tone pair so that its intermodulation products do too
//! ([`two_tone_bins`]): harmonics, aliases and noise separate exactly.

/// The rates a model ships at, which every plan's bins must keep
/// collision-free.
pub const SHIPPING_RATES: [f64; 3] = [44_100.0, 48_000.0, 96_000.0];

/// One period unit per second: gcd(44 100, 48 000) = 300.
pub const UNITS_PER_SECOND: u32 = 300;

/// A measurement period: `units` / 300 seconds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Period {
    units: u32,
}

impl Period {
    /// `units` / 300 s (6 → 20 ms, 30 → 100 ms).
    pub const fn units(units: u32) -> Self {
        assert!(units > 0, "a period is at least one unit");
        Self { units }
    }

    /// The period nearest `seconds` (at least one unit).
    pub fn near(seconds: f64) -> Self {
        Self::units(((seconds * f64::from(UNITS_PER_SECOND)).round() as u32).max(1))
    }

    pub const fn count(self) -> u32 {
        self.units
    }

    pub fn seconds(self) -> f64 {
        f64::from(self.units) / f64::from(UNITS_PER_SECOND)
    }

    /// The frequency of bin `bin`: `bin` cycles per period.
    pub fn hz(self, bin: u32) -> f64 {
        f64::from(bin) / self.seconds()
    }

    /// The bin nearest `hz` (at least 1).
    pub fn bin_near(self, hz: f64) -> u32 {
        ((hz * self.seconds()).round() as u32).max(1)
    }

    /// Samples in one period at `rate`. Panics unless whole: the rate must
    /// be a multiple of 300 Hz (every audio rate is).
    pub fn samples(self, rate: f64) -> usize {
        let n = rate * self.seconds();
        let whole = n.round();
        assert!(
            (n - whole).abs() < 1e-6 && whole >= 1.0,
            "{rate} Hz holds {n} samples per {} s period: not coherent",
            self.seconds()
        );
        whole as usize
    }

    /// Whether `rate` gives this period a whole number of samples.
    pub fn fits(self, rate: f64) -> bool {
        let n = rate * self.seconds();
        (n - n.round()).abs() < 1e-6 && n >= 1.0
    }
}

/// Where bin `bin` appears in a period of `n` samples: folded about
/// Nyquist into `0..=n/2`.
pub fn folded(bin: u64, n: u64) -> u64 {
    let r = bin % n;
    r.min(n - r)
}

/// Whether `bins` stay distinct, off DC and off Nyquist (whose phase is
/// lost) when folded into a period of `n` samples.
pub fn distinct_when_folded(bins: &[u64], n: u64) -> bool {
    let mut seen: Vec<u64> = bins.iter().map(|&b| folded(b, n)).collect();
    if seen.iter().any(|&b| b == 0 || 2 * b == n) {
        return false;
    }
    seen.sort_unstable();
    seen.windows(2).all(|w| w[0] != w[1])
}

/// The harmonics `1..=harmonics` of bin `bin`.
pub fn harmonic_bins(bin: u32, harmonics: u32) -> Vec<u64> {
    (1..=u64::from(harmonics))
        .map(|h| h * u64::from(bin))
        .collect()
}

/// A bin near `target_hz` whose harmonics 1..=`harmonics` land on
/// distinct bins at every rate in `rates` (each folded about its
/// Nyquist), none on DC or Nyquist: aliases never sit on a harmonic or on
/// each other. Preferred, where one is near: a bin coprime to every
/// rate's period length, so that no alias of *any* harmonic below the
/// period length lands on a harmonic bin — everything off the harmonic
/// grid is aliasing (or noise). Searches outward from the nearest bin,
/// within a third of an octave; `None` if no bin there qualifies.
pub fn tone_bin(period: Period, target_hz: f64, harmonics: u32, rates: &[f64]) -> Option<u32> {
    let centre = period.bin_near(target_hz);
    let lengths: Vec<u64> = rates.iter().map(|&r| period.samples(r) as u64).collect();
    let distinct = |k: u32| {
        let wanted = harmonic_bins(k, harmonics);
        lengths.iter().all(|&n| distinct_when_folded(&wanted, n))
    };
    let coprime = |k: u32| lengths.iter().all(|&n| gcd(u64::from(k), n) == 1);
    search(centre, target_hz, period, |k| distinct(k) && coprime(k))
        .or_else(|| search(centre, target_hz, period, distinct))
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

fn search(centre: u32, target_hz: f64, period: Period, fine: impl Fn(u32) -> bool) -> Option<u32> {
    let reach = ((target_hz * period.seconds() * 0.26) as u32).max(2);
    (0..=reach)
        .flat_map(|d| [centre + d, centre.saturating_sub(d)])
        .filter(|&k| k >= 1 && (period.hz(k) / target_hz).log2().abs() < 1.0 / 3.0)
        .find(|&k| fine(k))
}

/// The intermodulation products `m·a + n·b` of order `|m| + |n|` from 1
/// to `order`, as positive bins with their `(m, n)`, each product once.
pub fn products(a: u32, b: u32, order: u32) -> Vec<(i32, i32, u64)> {
    let o = order as i32;
    let mut out: Vec<(i32, i32, u64)> = Vec::new();
    for m in -o..=o {
        for n in -o..=o {
            if m.abs() + n.abs() == 0 || m.abs() + n.abs() > o {
                continue;
            }
            let bin = i64::from(m) * i64::from(a) + i64::from(n) * i64::from(b);
            // A product and its negative are one component.
            if bin > 0 {
                out.push((m, n, bin as u64));
            }
        }
    }
    out.sort_by_key(|p| (p.0.abs() + p.1.abs(), p.2));
    out
}

/// A two-tone pair near `(low_hz, high_hz)` whose intermodulation
/// products up to `order` are distinct bins, at every rate in `rates`
/// folded or not (none on DC or Nyquist): each product reads alone.
pub fn two_tone_bins(
    period: Period,
    low_hz: f64,
    high_hz: f64,
    order: u32,
    rates: &[f64],
) -> Option<(u32, u32)> {
    let low = period.bin_near(low_hz);
    let fine_pair = |a: u32, b: u32| {
        let p: Vec<u64> = products(a, b, order).iter().map(|p| p.2).collect();
        let mut unique = p.clone();
        unique.sort_unstable();
        unique.dedup();
        unique.len() == p.len()
            && rates
                .iter()
                .all(|&r| distinct_when_folded(&p, period.samples(r) as u64))
    };
    let reach = ((low_hz * period.seconds() * 0.26) as u32).max(2);
    (0..=reach)
        .flat_map(|d| [low + d, low.saturating_sub(d)])
        .filter(|&a| a >= 1)
        .find_map(|a| {
            search(period.bin_near(high_hz), high_hz, period, |b| {
                b != a && fine_pair(a, b)
            })
            .map(|b| (a, b))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_are_whole_at_every_audio_rate() {
        for units in [1, 6, 11, 30] {
            let p = Period::units(units);
            for rate in [44_100.0, 48_000.0, 88_200.0, 96_000.0, 192_000.0, 384_000.0] {
                assert!(p.fits(rate));
                let n = p.samples(rate);
                assert!((n as f64 - rate * p.seconds()).abs() < 1e-9);
            }
        }
        assert!(!Period::units(1).fits(44_000.0 + 50.0));
    }

    /// The proof the plan relies on: for every nominal tone of the suite
    /// (C4, C5) and every period the plans use, harmonics 2…15, their
    /// aliases at 44.1/48/96 kHz and each other land on distinct bins.
    #[test]
    fn tone_bins_keep_harmonics_and_aliases_apart() {
        for units in [1, 3, 6, 11, 15, 30] {
            let period = Period::units(units);
            for target in [82.0, 100.0, 330.0, 1_000.0, 3_000.0, 5_000.0, 8_000.0] {
                if target * period.seconds() < 2.0 {
                    continue;
                }
                let k = tone_bin(period, target, 15, &SHIPPING_RATES)
                    .unwrap_or_else(|| panic!("no bin near {target} Hz in {units} units"));
                assert!((period.hz(k) / target).log2().abs() < 1.0 / 3.0);
                if target * period.seconds() >= 20.0 {
                    // Room to choose: coprime to every period length, so
                    // any harmonic's alias lands off the harmonic grid.
                    for rate in SHIPPING_RATES {
                        assert_eq!(gcd(u64::from(k), period.samples(rate) as u64), 1, "{target} Hz");
                    }
                }
                for rate in SHIPPING_RATES {
                    let n = period.samples(rate) as u64;
                    let mut seen: Vec<u64> = (1..=15).map(|h| folded(h * u64::from(k), n)).collect();
                    assert!(seen.iter().all(|&b| b != 0 && 2 * b != n));
                    seen.sort_unstable();
                    let before = seen.len();
                    seen.dedup();
                    assert_eq!(seen.len(), before, "{target} Hz at {rate}: a collision");
                }
            }
        }
    }

    #[test]
    fn a_bin_dividing_the_period_collides_and_is_refused() {
        // 1 kHz in 20 ms is bin 20; 48 kHz holds 960 samples: harmonic 48
        // folds to DC and 49 onto the fundamental. Within 15 harmonics at
        // 8 kHz-ish (bin 160) folding collides, and tone_bin moves off it.
        let p = Period::units(6);
        let n = p.samples(48_000.0) as u64;
        assert_eq!(folded(49 * 20, n), 20);
        let wanted = harmonic_bins(160, 15);
        assert!(!distinct_when_folded(&wanted, n));
        let k = tone_bin(p, 8_000.0, 15, &[48_000.0]).unwrap();
        assert_ne!(k, 160);
        assert!(distinct_when_folded(&harmonic_bins(k, 15), n));
    }

    #[test]
    fn two_tone_products_are_distinct() {
        let p = Period::units(15); // 50 ms: 60 Hz is bin 3
        for (lo, hi) in [(60.0, 7_000.0), (19_000.0, 20_000.0)] {
            let (a, b) = two_tone_bins(p, lo, hi, 4, &SHIPPING_RATES).unwrap();
            let prods = products(a, b, 4);
            let mut bins: Vec<u64> = prods.iter().map(|q| q.2).collect();
            let len = bins.len();
            bins.sort_unstable();
            bins.dedup();
            assert_eq!(bins.len(), len, "{lo}/{hi}: products coincide");
            for rate in SHIPPING_RATES {
                assert!(distinct_when_folded(&bins, p.samples(rate) as u64));
            }
        }
        // (2, −1) of 3 and 5 is bin 1, and (−1, 2)·… : every product once.
        let p = products(3, 5, 2);
        assert!(p.contains(&(1, 0, 3)) && p.contains(&(-1, 1, 2)) && p.contains(&(1, 1, 8)));
        assert!(!p.iter().any(|q| q.0 == 1 && q.1 == -1), "5 − 3 is listed as (−1, 1)");
    }
}
