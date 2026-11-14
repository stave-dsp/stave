//! IEC 60063's preferred numbers: the values resistors and capacitors
//! are made in, so a fitted part can be bought.

/// An E series: `n` values per decade, log-spaced.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Series {
    E3,
    E6,
    E12,
    E24,
    E48,
    E96,
    E192,
}

/// E24 as IEC 60063 lists it (two figures; E3, E6 and E12 are every
/// 8th, 4th and 2nd of it). Several values differ from `10^(i/24)`
/// rounded, so it is a table.
const E24: [f64; 24] = [
    1.0, 1.1, 1.2, 1.3, 1.5, 1.6, 1.8, 2.0, 2.2, 2.4, 2.7, 3.0, 3.3, 3.6, 3.9, 4.3, 4.7, 5.1, 5.6, 6.2, 6.8, 7.5, 8.2, 9.1,
];

impl Series {
    pub fn name(self) -> &'static str {
        match self {
            Self::E3 => "E3",
            Self::E6 => "E6",
            Self::E12 => "E12",
            Self::E24 => "E24",
            Self::E48 => "E48",
            Self::E96 => "E96",
            Self::E192 => "E192",
        }
    }

    /// Values per decade.
    pub fn per_decade(self) -> usize {
        match self {
            Self::E3 => 3,
            Self::E6 => 6,
            Self::E12 => 12,
            Self::E24 => 24,
            Self::E48 => 48,
            Self::E96 => 96,
            Self::E192 => 192,
        }
    }

    /// One decade's values, from 1 up to (not including) 10. E48 and
    /// finer are `10^(i/n)` rounded to three figures, but for E192's
    /// 9.20 (the formula gives 9.19).
    pub fn decade(self) -> Vec<f64> {
        let n = self.per_decade();
        if n <= 24 {
            return E24.iter().step_by(24 / n).copied().collect();
        }
        (0..n)
            .map(|i| {
                let v = (10f64.powf(i as f64 / n as f64) * 100.0).round() / 100.0;
                if n == 192 && (v - 9.19).abs() < 1e-9 {
                    9.20
                } else {
                    v
                }
            })
            .collect()
    }

    /// The preferred values either side of `value` (> 0): the largest
    /// at or below it and the smallest at or above it.
    pub fn bracket(self, value: f64) -> (f64, f64) {
        let decade = value.log10().floor();
        let scale = 10f64.powf(decade);
        let mantissa = value / scale;
        let d = self.decade();
        // Rounding of the mantissa: a preferred value read back exactly.
        let eps = 1e-9;
        let below = d.iter().rev().find(|&&x| x <= mantissa * (1.0 + eps)).copied().unwrap_or(1.0);
        let above = d.iter().find(|&&x| x >= mantissa * (1.0 - eps)).copied().unwrap_or(10.0);
        (below * scale, above * scale)
    }

    /// The preferred value nearest `value` in ratio.
    pub fn nearest(self, value: f64) -> f64 {
        let (lo, hi) = self.bracket(value);
        if (value / lo).ln() <= (hi / value).ln() {
            lo
        } else {
            hi
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decades() {
        assert_eq!(Series::E12.decade(), [1.0, 1.2, 1.5, 1.8, 2.2, 2.7, 3.3, 3.9, 4.7, 5.6, 6.8, 8.2]);
        assert_eq!(Series::E6.decade(), [1.0, 1.5, 2.2, 3.3, 4.7, 6.8]);
        assert_eq!(Series::E3.decade(), [1.0, 2.2, 4.7]);
        let e96 = Series::E96.decade();
        assert_eq!((e96.len(), e96[1], e96[95]), (96, 1.02, 9.76));
        let e192 = Series::E192.decade();
        assert_eq!((e192[185], e192[191]), (9.20, 9.88));
        assert_eq!(Series::E48.decade()[47], 9.53);
    }

    #[test]
    fn snapping() {
        let near = |a: f64, b: f64| (a / b - 1.0).abs() < 1e-12;
        assert!(near(Series::E24.nearest(4_650.0), 4_700.0));
        assert!(near(Series::E12.nearest(5_000.0), 4_700.0));
        let (lo, hi) = Series::E12.bracket(33e-9);
        assert!(near(lo, 33e-9) && near(hi, 33e-9));
        let (lo, hi) = Series::E6.bracket(9.9e-6);
        assert!(near(lo, 6.8e-6) && near(hi, 10e-6));
    }
}
