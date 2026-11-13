//! Levels relative to the circuit (DSP.md measurement principle 3):
//! the knee, and the fixed offsets from it every nonlinear measurement
//! runs at, plus the real-world anchors measured at calibrated volts.

/// The offsets from the knee the full grid runs at (T1), in dB.
pub const KNEE_OFFSETS_DB: [f64; 8] = [-40.0, -20.0, -10.0, -3.0, 0.0, 6.0, 12.0, 20.0];

/// The three offsets the fast tier runs at (T0): clean, the knee,
/// saturated.
pub const FAST_OFFSETS_DB: [f64; 3] = [-20.0, 0.0, 12.0];

/// 0 dBu: 1 mW into 600 Ω, in volts RMS.
pub const DBU_VOLTS_RMS: f64 = 0.774_596_669_241_483_4;

/// Volts RMS of `dbu` dBu.
pub fn dbu(dbu: f64) -> f64 {
    DBU_VOLTS_RMS * 10f64.powf(dbu / 20.0)
}

/// Volts RMS of `dbv` dBV.
pub fn dbv(dbv: f64) -> f64 {
    10f64.powf(dbv / 20.0)
}

/// A sine's peak for an RMS.
pub fn peak_of_rms(rms: f64) -> f64 {
    rms * std::f64::consts::SQRT_2
}

/// Real-world input levels, as sine peaks in volts.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Anchor {
    pub name: &'static str,
    pub peak_volts: f64,
}

/// Instrument and line anchors (DSP.md principle 3).
pub fn anchors() -> [Anchor; 6] {
    [
        Anchor {
            name: "single coil",
            peak_volts: 0.1,
        },
        Anchor {
            name: "hot humbucker",
            peak_volts: 1.0,
        },
        Anchor {
            name: "dug-in pick",
            peak_volts: 2.0,
        },
        Anchor {
            name: "-10 dBV line",
            peak_volts: peak_of_rms(dbv(-10.0)),
        },
        Anchor {
            name: "+4 dBu line",
            peak_volts: peak_of_rms(dbu(4.0)),
        },
        Anchor {
            name: "+24 dBu hot line",
            peak_volts: peak_of_rms(dbu(24.0)),
        },
    ]
}

/// What set a knee.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Criterion {
    /// The gain moved 1 dB from its small-signal value.
    Compression,
    /// THD reached 1 %.
    Distortion,
    /// Neither, up to the largest level tried: the knee is that level.
    Unreached,
}

/// A circuit's knee: the input sine peak where it stops being clean.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Knee {
    pub amplitude: f64,
    pub criterion: Criterion,
}

impl Knee {
    /// The amplitude `offset_db` from the knee.
    pub fn at(self, offset_db: f64) -> f64 {
        self.amplitude * 10f64.powf(offset_db / 20.0)
    }

    /// The knee rounded to a 0.1 dB grid (re 1 V): a plan stays the same
    /// across platforms whose last bits differ.
    pub fn rounded(self) -> Self {
        let db = (20.0 * self.amplitude.log10() * 10.0).round() / 10.0;
        Self {
            amplitude: 10f64.powf(db / 20.0),
            ..self
        }
    }
}

/// Find the knee from a level measurement: `measure(peak)` returns the
/// fundamental's gain in dB and the THD as a ratio at a sine of that
/// peak. Starts at `small` (taken as the small-signal gain), climbs in
/// 1 dB steps to `large`, then bisects the crossing to 0.05 dB: the
/// lowest level where the gain has moved 1 dB or THD reaches 1 %.
pub fn find_knee(mut measure: impl FnMut(f64) -> (f64, f64), small: f64, large: f64) -> Knee {
    let (g0, _) = measure(small);
    let mut past = |peak: f64| -> Option<Criterion> {
        let (g, thd) = measure(peak);
        if (g - g0).abs() >= 1.0 {
            Some(Criterion::Compression)
        } else if thd >= 0.01 {
            Some(Criterion::Distortion)
        } else {
            None
        }
    };
    let step = 10f64.powf(1.0 / 20.0);
    let mut below = small;
    let mut above = small * step;
    let criterion = loop {
        if above > large {
            return Knee {
                amplitude: large,
                criterion: Criterion::Unreached,
            };
        }
        if let Some(c) = past(above) {
            break c;
        }
        below = above;
        above *= step;
    };
    let mut found = criterion;
    while 20.0 * (above / below).log10() > 0.05 {
        let mid = (above * below).sqrt();
        match past(mid) {
            Some(c) => {
                above = mid;
                found = c;
            }
            None => below = mid,
        }
    }
    Knee {
        amplitude: above,
        criterion: found,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibrated_anchors() {
        assert!((dbu(4.0) - 1.227_7).abs() < 1e-4);
        assert!((dbv(-10.0) - 0.316_23).abs() < 1e-5);
        assert_eq!(anchors()[1].peak_volts, 1.0);
    }

    /// tanh's fundamental: gain −1 dB where a·(1 − a²/4 …) falls 11 %:
    /// near a peak of 0.7 at unit gain. Found against a closed form of
    /// the fundamental (the first Fourier coefficient, numerically).
    #[test]
    fn the_knee_of_tanh_is_where_it_compresses_one_db() {
        let fundamental = |a: f64| {
            let n = 4096;
            let s: f64 = (0..n)
                .map(|i| {
                    let p = std::f64::consts::TAU * i as f64 / n as f64;
                    (a * p.sin()).tanh() * p.sin()
                })
                .sum();
            2.0 * s / n as f64
        };
        let knee = find_knee(
            |a| (20.0 * (fundamental(a) / a).log10(), 0.0),
            1e-3,
            100.0,
        );
        assert_eq!(knee.criterion, Criterion::Compression);
        let g = 20.0 * (fundamental(knee.amplitude) / knee.amplitude).log10();
        assert!((g + 1.0).abs() < 0.02, "gain at the knee {g} dB");
        assert!((0.6..0.8).contains(&knee.amplitude), "{}", knee.amplitude);
        let unreached = find_knee(|_| (0.0, 0.0), 1e-3, 1.0);
        assert_eq!(unreached.criterion, Criterion::Unreached);
        assert!((knee.rounded().at(0.0) / knee.amplitude).log10().abs() * 20.0 <= 0.05);
    }
}
