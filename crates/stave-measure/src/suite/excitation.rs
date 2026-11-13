//! One stimulus type for the whole suite: a DC level plus tones on a
//! [`Period`]'s bins, each optionally gated (a burst), the whole faded in
//! from silence so the run starts at the operating point.
//!
//! An [`Excitation`] is a function of continuous time ([`Excitation::at`]),
//! so the same description is sampled at any rate and as an ngspice
//! source; [`Excitation::describe`] and [`Excitation::parse`] are its text
//! form, which the plan files carry to `scripts/ngspice-simulate.py`.

use std::f64::consts::{PI, TAU};

use super::period::{two_tone_bins, Period, SHIPPING_RATES};

/// When a tone sounds: from `on` (inclusive) to `off` (exclusive) seconds.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Gate {
    pub on: f64,
    pub off: f64,
}

impl Gate {
    pub const ALWAYS: Self = Self {
        on: 0.0,
        off: f64::INFINITY,
    };

    pub fn contains(self, t: f64) -> bool {
        t >= self.on && t < self.off
    }
}

/// `amplitude · sin(2π·bin·t/period + phase)` while the gate is open.
/// Absolute time, so every period starts each tone at the same phase.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Tone {
    pub amplitude: f64,
    pub bin: u32,
    /// Radians, of the sine.
    pub phase: f64,
    pub gate: Gate,
}

impl Tone {
    pub fn new(amplitude: f64, bin: u32) -> Self {
        Self {
            amplitude,
            bin,
            phase: 0.0,
            gate: Gate::ALWAYS,
        }
    }

    pub fn phased(self, phase: f64) -> Self {
        Self { phase, ..self }
    }

    pub fn gated(self, on: f64, off: f64) -> Self {
        Self {
            gate: Gate { on, off },
            ..self
        }
    }
}

/// A stimulus: `dc + fade(t) · Σ tones`, `fade` a raised cosine from 0 to
/// 1 over the first `fade_seconds` (none when 0).
#[derive(Clone, Debug, PartialEq)]
pub struct Excitation {
    pub period: Period,
    pub dc: f64,
    pub tones: Vec<Tone>,
    pub fade_seconds: f64,
}

impl Excitation {
    pub fn new(period: Period, tones: Vec<Tone>) -> Self {
        Self {
            period,
            dc: 0.0,
            tones,
            fade_seconds: 0.0,
        }
    }

    /// Silence (a DC level alone, or nothing): the idle run.
    pub fn idle(period: Period) -> Self {
        Self::new(period, Vec::new())
    }

    /// One sine of `amplitude` peak on `bin`, from zero phase.
    pub fn sine(period: Period, bin: u32, amplitude: f64) -> Self {
        Self::new(period, vec![Tone::new(amplitude, bin)])
    }

    /// Two sines, `(bin, amplitude)` each.
    pub fn two_tone(period: Period, low: (u32, f64), high: (u32, f64)) -> Self {
        Self::new(
            period,
            vec![Tone::new(low.1, low.0), Tone::new(high.1, high.0)],
        )
    }

    /// SMPTE RP120 intermodulation: 60 Hz and 7 kHz at 4:1, `peak` the
    /// sum's peak, on bins whose products to fourth order are distinct at
    /// every shipping rate.
    pub fn smpte(period: Period, peak: f64) -> Option<Self> {
        let (a, b) = two_tone_bins(period, 60.0, 7_000.0, 4, &SHIPPING_RATES)?;
        Some(Self::two_tone(period, (a, peak * 0.8), (b, peak * 0.2)))
    }

    /// CCIF (ITU-R) intermodulation: 19 and 20 kHz at 1:1, `peak` the
    /// sum's peak.
    pub fn ccif(period: Period, peak: f64) -> Option<Self> {
        let (a, b) = two_tone_bins(period, 19_000.0, 20_000.0, 3, &SHIPPING_RATES)?;
        Some(Self::two_tone(period, (a, peak * 0.5), (b, peak * 0.5)))
    }

    pub fn with_dc(self, dc: f64) -> Self {
        Self { dc, ..self }
    }

    pub fn with_tone(mut self, tone: Tone) -> Self {
        self.tones.push(tone);
        self
    }

    pub fn faded_in(self, seconds: f64) -> Self {
        Self {
            fade_seconds: seconds,
            ..self
        }
    }

    /// Every tone's amplitude times `gain`.
    pub fn scaled(mut self, gain: f64) -> Self {
        for t in &mut self.tones {
            t.amplitude *= gain;
        }
        self
    }

    /// The value at `t` seconds.
    pub fn at(&self, t: f64) -> f64 {
        let w = TAU / self.period.seconds();
        let sum: f64 = self
            .tones
            .iter()
            .filter(|tone| tone.gate.contains(t))
            .map(|tone| tone.amplitude * (w * f64::from(tone.bin) * t + tone.phase).sin())
            .sum();
        let fade = if t < self.fade_seconds {
            0.5 - 0.5 * (PI * t / self.fade_seconds).cos()
        } else {
            1.0
        };
        self.dc + fade * sum
    }

    /// The RMS of the steady (ungated, faded-in) tones over a period.
    pub fn rms(&self) -> f64 {
        self.tones
            .iter()
            .map(|t| t.amplitude * t.amplitude / 2.0)
            .sum::<f64>()
            .sqrt()
    }

    /// The largest |value − dc| over one period, sampled finely (4096
    /// points per cycle of the highest tone, at least 65 536 a period).
    pub fn peak(&self) -> f64 {
        let top = self.tones.iter().map(|t| t.bin).max().unwrap_or(1);
        let n = (u64::from(top) * 4096).max(65_536);
        let mut quiet = self.clone();
        quiet.fade_seconds = 0.0;
        quiet.dc = 0.0;
        for t in &mut quiet.tones {
            t.gate = Gate::ALWAYS;
        }
        (0..n)
            .map(|i| quiet.at(i as f64 * self.period.seconds() / n as f64).abs())
            .fold(0.0, f64::max)
    }

    /// The text form: `period <units> dc <v> fade <s>` then one
    /// `tone <amplitude> <bin> <phase> <on> <off>` per tone, all on one
    /// line, numbers to 17 significant digits (exact round trip).
    pub fn describe(&self) -> String {
        let mut s = format!(
            "period {} dc {:e} fade {:e}",
            self.period.count(),
            self.dc,
            self.fade_seconds
        );
        for t in &self.tones {
            let off = if t.gate.off.is_finite() {
                format!("{:e}", t.gate.off)
            } else {
                "inf".into()
            };
            s.push_str(&format!(
                " tone {:e} {} {:e} {:e} {off}",
                t.amplitude, t.bin, t.phase, t.gate.on
            ));
        }
        s
    }

    /// Read [`Excitation::describe`]'s form back.
    pub fn parse(text: &str) -> Result<Self, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let num = |i: usize| -> Result<f64, String> {
            let w = words.get(i).ok_or("excitation: truncated")?;
            if *w == "inf" {
                return Ok(f64::INFINITY);
            }
            w.parse::<f64>().map_err(|e| format!("excitation: {w}: {e}"))
        };
        let key = |i: usize, k: &str| -> Result<(), String> {
            (words.get(i) == Some(&k))
                .then_some(())
                .ok_or(format!("excitation: expected `{k}` at word {i} of `{text}`"))
        };
        key(0, "period")?;
        key(2, "dc")?;
        key(4, "fade")?;
        let units = num(1)? as u32;
        let mut e = Self::new(Period::units(units), Vec::new())
            .with_dc(num(3)?)
            .faded_in(num(5)?);
        let mut i = 6;
        while i < words.len() {
            key(i, "tone")?;
            e.tones.push(
                Tone::new(num(i + 1)?, num(i + 2)? as u32)
                    .phased(num(i + 3)?)
                    .gated(num(i + 4)?, num(i + 5)?),
            );
            i += 6;
        }
        Ok(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_sits_on_its_bin_and_fades_in() {
        let p = Period::units(6);
        let e = Excitation::sine(p, 21, 0.5).faded_in(0.01);
        assert_eq!(e.at(0.0), 0.0);
        let t = 0.04 + 0.25 / 1_050.0; // a quarter cycle into a whole period
        assert!((e.at(t) - 0.5).abs() < 1e-9);
        assert!((e.rms() - 0.5 / 2f64.sqrt()).abs() < 1e-15);
        assert!((e.peak() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn gates_make_bursts() {
        let p = Period::units(3);
        let e = Excitation::idle(p).with_tone(Tone::new(1.0, 2).gated(0.01, 0.02));
        assert_eq!(e.at(0.005), 0.0);
        assert_eq!(e.at(0.025), 0.0);
        assert!((e.at(0.01125) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_text_form_round_trips() {
        let p = Period::units(15);
        let e = Excitation::smpte(p, 1.0)
            .unwrap()
            .with_dc(-1.5)
            .faded_in(0.02)
            .with_tone(Tone::new(0.1, 7).phased(0.3).gated(0.1, 0.2));
        assert_eq!(Excitation::parse(&e.describe()).unwrap(), e);
        assert!(Excitation::parse("period 6 dc 0 fade 0 tone 1").is_err());
    }

    #[test]
    fn intermodulation_pairs_hold_their_ratios() {
        let p = Period::units(15);
        let s = Excitation::smpte(p, 1.0).unwrap();
        assert!((s.tones[0].amplitude / s.tones[1].amplitude - 4.0).abs() < 1e-12);
        assert!((p.hz(s.tones[0].bin) - 60.0).abs() < 15.0);
        let c = Excitation::ccif(p, 1.0).unwrap();
        assert!((p.hz(c.tones[1].bin) - p.hz(c.tones[0].bin) - 1_000.0).abs() < 300.0);
    }
}
