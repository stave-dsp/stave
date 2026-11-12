//! Nonlinear-system analyses: the alias floor of a tone capture, the
//! gain-vs-frequency-and-level surface, and hysteresis.

use crate::spectrum::fft;
use crate::system::System;
use crate::tone_window::ToneWindow;

/// Alias-to-signal classification of a bin-centred capture. Signal = bins
/// that are multiples of `m` (the true harmonic series); alias = every
/// other bin in (0, n/2). `audible_db` restricts the alias sum to bins at
/// or below `audible_max_hz` (signal reference is unchanged).
pub struct AliasReport {
    pub full_db: f64,
    pub audible_db: f64,
}

/// Where the signal comb sits in a capture, and where audible ends.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AliasBands {
    pub carrier_bin: usize,
    pub sample_rate: f64,
    pub audible_max_hz: f64,
}

pub fn alias_report(out: &[f64], bands: &AliasBands) -> AliasReport {
    let AliasBands {
        carrier_bin: m,
        sample_rate: fs,
        audible_max_hz,
    } = *bands;
    let n = out.len();
    let mut re = out.to_vec();
    let mut im = vec![0.0f64; n];
    fft(&mut re, &mut im);
    let (mut sig, mut alias_full, mut alias_aud) = (0.0f64, 0.0f64, 0.0f64);
    for b in 1..n / 2 {
        let p = re[b] * re[b] + im[b] * im[b];
        if b % m == 0 {
            sig += p;
        } else {
            alias_full += p;
            if (b as f64) * fs / n as f64 <= audible_max_hz {
                alias_aud += p;
            }
        }
    }
    let sig = sig.max(1e-30);
    AliasReport {
        full_db: 10.0 * (alias_full / sig).log10(),
        audible_db: 10.0 * (alias_aud / sig).log10(),
    }
}

/// The G(frequency, level) surface — the honest "complex curve" of any
/// nonlinear-with-memory system. Each cell is the fundamental's gain (dB)
/// for a steady bin-centred sine at that frequency and input level. A
/// MEMORYLESS nonlinearity produces a surface whose columns are identical
/// (gain compression cannot depend on frequency); interstage filters,
/// state-coupled parameters, and feedback cells all bend the surface —
/// which is exactly what "sounds like an amp / tube EQ" means.
pub struct Surface {
    pub freqs_hz: Vec<f64>,
    pub levels_db: Vec<f64>,
    /// `gains_db[level_idx][freq_idx]`
    pub gains_db: Vec<Vec<f64>>,
}

impl Surface {
    /// Gain compression per frequency: gain at the QUIETEST level minus
    /// gain at the LOUDEST — the knee depth, per column.
    pub fn compression_db(&self) -> Vec<f64> {
        let first = &self.gains_db[0];
        let last = &self.gains_db[self.gains_db.len() - 1];
        first.iter().zip(last).map(|(a, b)| a - b).collect()
    }

    /// Levels down, frequencies across, gains in dB.
    pub fn write_csv(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
        let header: Vec<String> = self.freqs_hz.iter().map(|v| format!("{v:.1}")).collect();
        writeln!(out, "level_db,{}", header.join(","))?;
        for (level, row) in self.levels_db.iter().zip(&self.gains_db) {
            let cells: Vec<String> = row.iter().map(|v| format!("{v:.4}")).collect();
            writeln!(out, "{level:.2},{}", cells.join(","))?;
        }
        Ok(())
    }
}

/// A G(frequency, level) measurement: one bin-centred tone per cell,
/// each through a fresh system (state must not leak between cells).
#[derive(Clone, Debug, PartialEq)]
pub struct GainSurface {
    pub freqs_hz: Vec<f64>,
    pub levels_db: Vec<f64>,
    pub window: usize,
    pub sample_rate: f64,
    pub warmup_windows: usize,
}

impl GainSurface {
    pub fn measure<S: System>(&self, make_system: impl Fn() -> S) -> Surface {
        let tones: Vec<ToneWindow> = self
            .freqs_hz
            .iter()
            .map(|&hz| ToneWindow {
                bin: ToneWindow::bin_near(hz, self.window, self.sample_rate),
                window: self.window,
                sample_rate: self.sample_rate,
                amplitude: 1.0,
                warmup_windows: self.warmup_windows,
            })
            .collect();
        let gains_db = self
            .levels_db
            .iter()
            .map(|&level| {
                let amplitude = 10f64.powf(level / 20.0);
                tones
                    .iter()
                    .map(|t| {
                        ToneWindow { amplitude, ..*t }
                            .gain_db(make_system())
                            .max(-240.0)
                    })
                    .collect()
            })
            .collect();
        Surface {
            freqs_hz: tones.iter().map(ToneWindow::frequency).collect(),
            levels_db: self.levels_db.clone(),
            gains_db,
        }
    }
}

/// Hysteresis loop area (shoelace over one settled cycle of a bin-exact
/// tone): ~0 for any memoryless map; > 0 and roughly frequency-INdependent
/// for true hysteresis; frequency-DEpendent for plain filter lag.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HysteresisTest {
    pub frequency_hz: f64,
    pub sample_rate: f64,
    pub amplitude: f64,
}

impl HysteresisTest {
    pub fn loop_area(&self, run: impl System) -> f64 {
        hysteresis_loop_area(run, self.frequency_hz, self.sample_rate, self.amplitude)
    }
}

fn hysteresis_loop_area(mut run: impl System, f0: f64, fs: f64, amp: f64) -> f64 {
    let period = (fs / f0).round() as usize;
    let sig = |i: usize| amp * (2.0 * core::f64::consts::PI * i as f64 / period as f64).sin();
    for i in 0..period * 8 {
        let _ = run.tick(sig(i) as f32);
    }
    let mut xs = Vec::with_capacity(period);
    let mut ys = Vec::with_capacity(period);
    for i in period * 8..period * 9 {
        xs.push(sig(i));
        ys.push(run.tick(sig(i) as f32) as f64);
    }
    let n = xs.len();
    let mut a = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        a += xs[i] * ys[j] - xs[j] * ys[i];
    }
    (a * 0.5).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_report_classifies_a_known_spectrum() {
        // Synthetic: fundamental bin 100 + harmonic bin 300 (signal) +
        // an interloper at bin 137 at exactly −40 dB — the report must
        // read −40 dB in both bands (137·fs/n is audible at 48 k / 8192).
        let n = 8192usize;
        let fs = 48_000.0;
        let m = 100usize;
        let two_pi = 2.0 * core::f64::consts::PI;
        let buf: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / n as f64;
                (two_pi * 100.0 * t).sin()
                    + 0.1 * (two_pi * 300.0 * t).sin()
                    + 0.01005 * (two_pi * 137.0 * t).sin() // −40 dB vs √(1+0.01)
            })
            .collect();
        let r = alias_report(
            &buf,
            &AliasBands {
                carrier_bin: m,
                sample_rate: fs,
                audible_max_hz: 20_000.0,
            },
        );
        assert!((r.full_db + 40.0).abs() < 0.1, "full {:.2} dB", r.full_db);
        assert!(
            (r.audible_db + 40.0).abs() < 0.1,
            "audible {:.2} dB",
            r.audible_db
        );
    }
}
