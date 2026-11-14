//! Golden digests (DSP.md R9): a stage's output on one fixed stimulus,
//! committed as a compact time–frequency summary, so a refactor that
//! changes the sound of a time-varying, stochastic or linear stage fails
//! CI until it is re-blessed on purpose. The nonlinear, level-dependent
//! stages are pinned by their fingerprints instead (R18,
//! [`characterise::fingerprint`](crate::characterise::fingerprint));
//! a digest pins everything that plan cannot hold still: reverbs and
//! their tails, modulation, pitch, grains, dynamics, the physical models
//! and instruments.
//!
//! **The stimulus** ([`stimulus`]): 32 768 samples at 48 kHz (0.68 s),
//! in four parts — a −6 dBFS impulse and 170 ms of its tail; white noise
//! at −30 dBFS RMS then 85 ms of silence; the same at −6 dBFS (so a
//! level-dependent stage shows its two levels) then silence; and an
//! exponential sweep 20 Hz–20 kHz at −12 dBFS. Seeded, generated in
//! `f64`, the same on every platform.
//!
//! **The digest** ([`Digest`]): the output in 33 Hann frames of 2048
//! samples, hop 1024 (the first centred on sample 0, so every sample
//! weighs), each frame's mean-square power in 11 bands (DC–22 Hz, then
//! octaves from 22 Hz to 11.3 kHz, then to Nyquist) in dB re full scale
//! (a full-scale sine reads −3 dB), plus the whole output's RMS and
//! sample peak. 363 numbers, a 4 KB text file, written with three
//! decimals:
//!
//! ```text
//! # stave golden v1: effects/reverb/fdn_householder
//! model Fdn, 8 lines, Householder mixing, T60 2 s
//! stimulus v1 48000 32768 hop 1024 frame 2048 hann
//! bands 0 22.1 44.2 88.4 176.8 353.6 707.1 1414.2 2828.4 5656.9 11313.7 24000
//! rms -27.153
//! peak -14.981
//! frame 0 -105.120 -93.774 … (one dB value per band)
//! …
//! end
//! ```
//!
//! **Comparison** ([`Digest::diff`]): each band of each frame, against
//! the committed one, within a relative tolerance ([`RELATIVE_DB`]) plus
//! an absolute one at [`FLOOR_DB`] below the loudest band of the whole
//! digest (expressed in dB at the band's level, it is negligible for a
//! loud band and grows toward the floor; below the floor nothing is
//! compared). Rounding differences between platforms' `libm`s pass; any
//! change in the sound fails, and the failure names the frames (as
//! times) and bands (as frequencies) that moved, by how much against
//! what tolerance, largest first. The digest is magnitude only: a change
//! of phase alone (an allpass's) shows only where it moves energy
//! between frames or bands.
//!
//! **Re-blessing**: `STAVE_BLESS=1` writes the digest instead of
//! comparing (`STAVE_BLESS=1 cargo test -p stave-test --test golden`),
//! and the diff of the committed file is the review.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use stave_dsp::primitives::spectral::{Complex, Fft};
use stave_dsp::Stage;

/// The first line of every golden digest, before its case.
pub const HEADER: &str = "# stave golden v1: ";
/// The rate every digest runs at.
pub const RATE: f32 = 48_000.0;
/// Stimulus and output length, in samples.
pub const LEN: usize = 32_768;
/// Frame hop; a frame is two hops, Hann-windowed.
pub const HOP: usize = 1_024;
const FRAME: usize = 2 * HOP;
/// Frames: the first centred on sample 0, the last on sample `LEN`.
pub const FRAMES: usize = LEN / HOP + 1;
/// Band edges in Hz: DC to 22 Hz, octaves (centred on 31.25 Hz … 8 kHz),
/// then to Nyquist.
pub const EDGES: [f64; 12] = [
    0.0, 22.097, 44.194, 88.388, 176.777, 353.553, 707.107, 1_414.214, 2_828.427, 5_656.854, 11_313.708, 24_000.0,
];
/// Bands per frame.
pub const BANDS: usize = EDGES.len() - 1;
/// What the stimulus line names: change it (and re-bless) when the
/// stimulus or the analysis changes.
const STIMULUS: &str = "stimulus v1 48000 32768 hop 1024 frame 2048 hann";
/// The lowest level written (an all-zero band).
const SILENT_DB: f64 = -300.0;

/// The relative tolerance of every band and of the RMS and peak: a
/// band moving more than this (plus the absolute part, [`FLOOR_DB`])
/// fails. Set where the macOS (aarch64) and Linux (x86_64) runs agree:
/// the largest difference seen between them is printed by the
/// `golden` test target's summary and recorded in DSP.md R9.
pub const RELATIVE_DB: f64 = 0.01;
/// The absolute tolerance, in dB below the loudest band of the digest:
/// an error of that size is allowed at any level, and bands below it
/// are not compared.
pub const FLOOR_DB: f64 = 100.0;

/// `crates/stave-test/golden/<case>.golden`.
pub fn path(case: &str) -> PathBuf {
    root().join(format!("{case}.golden"))
}

/// Where the committed digests live.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("golden")
}

/// Re-bless on purpose: `STAVE_BLESS=1`.
pub fn blessing() -> bool {
    std::env::var_os("STAVE_BLESS").is_some()
}

/// Numerical Recipes' LCG, as uniform in [−1, 1).
fn lcg(state: &mut u32) -> f64 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    f64::from(*state >> 8) / f64::from(1u32 << 23) - 1.0
}

/// The fixed stimulus (see the module docs), `LEN` samples at 48 kHz.
pub fn stimulus() -> Vec<f32> {
    let mut x = vec![0.0f64; LEN];
    // An impulse at −6 dBFS, its tail to 8192.
    x[0] = 0.5;
    // Uniform noise: RMS = amplitude / √3.
    let mut noise = |range: core::ops::Range<usize>, rms_db: f64, seed: u32| {
        let a = 10f64.powf(rms_db / 20.0) * 3f64.sqrt();
        let mut s = seed;
        for v in &mut x[range] {
            *v = a * lcg(&mut s);
        }
    };
    noise(8_192..12_288, -30.0, 0x5EED_0001);
    noise(16_384..20_480, -6.0, 0x5EED_0002);
    // An exponential sweep 20 Hz–20 kHz at −12 dBFS, 5 ms fades.
    let (start, n) = (24_576, 8_192);
    let (f0, f1) = (20.0f64, 20_000.0f64);
    let t = n as f64 / f64::from(RATE);
    let k = (f1 / f0).ln();
    let amplitude = 10f64.powf(-12.0 / 20.0);
    let fade = (0.005 * f64::from(RATE)) as usize;
    for i in 0..n {
        let s = i as f64 / f64::from(RATE);
        let phase = core::f64::consts::TAU * f0 * t / k * ((s / t * k).exp() - 1.0);
        let edge = i.min(n - 1 - i);
        let g = if edge < fade { 0.5 - 0.5 * (core::f64::consts::PI * edge as f64 / fade as f64).cos() } else { 1.0 };
        x[start + i] = amplitude * g * phase.sin();
    }
    x.into_iter().map(|v| v as f32).collect()
}

/// The time–frequency summary of one output.
#[derive(Clone, Debug, PartialEq)]
pub struct Digest {
    pub case: String,
    /// What ran, at which settings.
    pub model: String,
    /// The whole output's RMS, dBFS.
    pub rms_db: f64,
    /// The whole output's sample peak, dBFS.
    pub peak_db: f64,
    /// Per frame, each band's mean-square power, dB re full scale.
    pub bands: Vec<[f64; BANDS]>,
}

fn db_power(p: f64) -> f64 {
    if p > 0.0 {
        (10.0 * p.log10()).max(SILENT_DB)
    } else {
        SILENT_DB
    }
}

/// Rounded as written, so a fresh digest compares like the parsed one.
fn round3(x: f64) -> f64 {
    (x * 1e3).round() / 1e3
}

impl Digest {
    /// The digest of `output` (`LEN` samples). Panics on a non-finite
    /// sample, naming it: a stage that blows up is never pinned.
    pub fn of(case: &str, model: &str, output: &[f32]) -> Self {
        assert_eq!(output.len(), LEN, "golden `{case}`: {} samples, not {LEN}", output.len());
        if let Some(n) = output.iter().position(|v| !v.is_finite()) {
            panic!("golden `{case}`: sample {n} ({:.4} s) is {}", n as f64 / f64::from(RATE), output[n]);
        }
        let fft = Fft::<f64>::new(FRAME);
        let window: Vec<f64> =
            (0..FRAME).map(|n| 0.5 - 0.5 * (core::f64::consts::TAU * n as f64 / FRAME as f64).cos()).collect();
        let energy: f64 = window.iter().map(|w| w * w).sum();
        let bin_hz = f64::from(RATE) / FRAME as f64;
        let band_of: Vec<usize> = (0..=FRAME / 2)
            .map(|k| {
                let f = k as f64 * bin_hz;
                EDGES[1..].iter().position(|&e| f < e).unwrap_or(BANDS - 1)
            })
            .collect();
        let mut frame = vec![0.0f64; FRAME];
        let mut spectrum = vec![Complex::new(0.0, 0.0); FRAME / 2 + 1];
        let bands = (0..FRAMES)
            .map(|f| {
                for (n, v) in frame.iter_mut().enumerate() {
                    let at = (f * HOP + n).checked_sub(HOP);
                    let x = at.and_then(|i| output.get(i)).map_or(0.0, |&s| f64::from(s));
                    *v = x * window[n];
                }
                fft.forward_real(&frame, &mut spectrum);
                let mut power = [0.0f64; BANDS];
                for (k, z) in spectrum.iter().enumerate() {
                    // One-sided: the interior bins stand for two.
                    let weight = if k == 0 || k == FRAME / 2 { 1.0 } else { 2.0 };
                    power[band_of[k]] += weight * z.norm_sqr();
                }
                power.map(|p| round3(db_power(p / (energy * FRAME as f64))))
            })
            .collect();
        let ms = output.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / LEN as f64;
        let peak = output.iter().fold(0.0f64, |m, &v| m.max(f64::from(v).abs()));
        Self {
            case: case.into(),
            model: model.into(),
            rms_db: round3(db_power(ms)),
            peak_db: round3(db_power(peak * peak)),
            bands,
        }
    }

    /// The text written to the committed file.
    pub fn describe(&self) -> String {
        let mut s = format!("{HEADER}{}\n", self.case);
        s.push_str("# Written by STAVE_BLESS=1 (crates/stave-test/tests/golden.rs); CI compares it (DSP.md R9).\n");
        let _ = writeln!(s, "model {}", self.model);
        let _ = writeln!(s, "{STIMULUS}");
        let edges: Vec<String> = EDGES.iter().map(|e| format!("{e:.1}")).collect();
        let _ = writeln!(s, "bands {}", edges.join(" "));
        let _ = writeln!(s, "rms {:.3}", self.rms_db);
        let _ = writeln!(s, "peak {:.3}", self.peak_db);
        for (f, row) in self.bands.iter().enumerate() {
            let cells: Vec<String> = row.iter().map(|v| format!("{v:.3}")).collect();
            let _ = writeln!(s, "frame {f} {}", cells.join(" "));
        }
        s.push_str("end\n");
        s
    }

    /// Read a committed digest back.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        let case = lines
            .next()
            .and_then(|l| l.strip_prefix(HEADER))
            .ok_or_else(|| format!("the first line is not `{HEADER}<case>`"))?
            .to_string();
        let (mut model, mut rms, mut peak, mut stimulus, mut ended) = (None, None, None, false, false);
        let mut bands = Vec::new();
        for line in lines {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
            let number = |s: &str| s.parse::<f64>().map_err(|e| format!("`{line}`: {e}"));
            match key {
                "model" => model = Some(rest.to_string()),
                "stimulus" => {
                    if line != STIMULUS {
                        return Err(format!("written for `{line}`, the test runs `{STIMULUS}`: re-bless with STAVE_BLESS=1"));
                    }
                    stimulus = true;
                }
                "bands" => {}
                "rms" => rms = Some(number(rest)?),
                "peak" => peak = Some(number(rest)?),
                "frame" => {
                    let mut it = rest.split(' ');
                    let f: usize = it.next().and_then(|v| v.parse().ok()).ok_or_else(|| format!("`{line}`: no frame index"))?;
                    if f != bands.len() {
                        return Err(format!("`{line}`: frame {f} out of order"));
                    }
                    let mut row = [0.0; BANDS];
                    for (b, cell) in row.iter_mut().enumerate() {
                        *cell = number(it.next().ok_or_else(|| format!("`{line}`: band {b} missing"))?)?;
                    }
                    if it.next().is_some() {
                        return Err(format!("`{line}`: more than {BANDS} bands"));
                    }
                    bands.push(row);
                }
                "end" => ended = true,
                _ => return Err(format!("unknown line `{line}`")),
            }
        }
        if !ended || !stimulus || bands.len() != FRAMES {
            return Err(format!("incomplete: {} of {FRAMES} frames, stimulus line {stimulus}, end {ended}", bands.len()));
        }
        Ok(Self {
            case,
            model: model.ok_or("no model line")?,
            rms_db: rms.ok_or("no rms line")?,
            peak_db: peak.ok_or("no peak line")?,
            bands,
        })
    }

    /// The committed digest of `case`, if any.
    pub fn committed(case: &str) -> Option<Result<Self, String>> {
        let text = std::fs::read_to_string(path(case)).ok()?;
        Some(Self::parse(&text))
    }

    pub fn write(&self) {
        let p = path(&self.case);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, self.describe()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    }

    /// The loudest band of the digest, dB.
    fn loudest(&self) -> f64 {
        self.bands.iter().flatten().fold(SILENT_DB, |m, &v| m.max(v))
    }

    /// The largest move of any compared value against its tolerance:
    /// `(|Δ| dB, tolerance dB)` of the cell nearest to failing (the
    /// cross-platform check reads it).
    pub fn closest(&self, now: &Self) -> (f64, f64) {
        self.moves(now).into_iter().map(|m| (m.delta.abs(), m.tolerance)).fold((0.0, RELATIVE_DB), |a, b| {
            if b.0 - b.1 > a.0 - a.1 {
                b
            } else {
                a
            }
        })
    }

    fn moves(&self, now: &Self) -> Vec<Move> {
        let floor = self.loudest() - FLOOR_DB;
        let mut out = Vec::new();
        for (what, a, b) in [("RMS", self.rms_db, now.rms_db), ("peak", self.peak_db, now.peak_db)] {
            out.push(Move { frame: None, band: what.into(), was: a, now: b, delta: b - a, tolerance: RELATIVE_DB + 1e-3 });
        }
        for (f, (old, new)) in self.bands.iter().zip(&now.bands).enumerate() {
            for b in 0..BANDS {
                let (a, c) = (old[b], new[b]);
                if a <= floor && c <= floor {
                    continue;
                }
                let (a2, c2) = (a.max(floor), c.max(floor));
                // The absolute error `floor` dB, as dB at this level.
                let absolute = 20.0 * (1.0 + 10f64.powf((floor - a2.max(c2)) / 20.0)).log10();
                // 1e-3: the written values' rounding.
                let tolerance = RELATIVE_DB + absolute + 1e-3;
                let band = format!("{:.0}–{:.0} Hz", EDGES[b], EDGES[b + 1]);
                out.push(Move { frame: Some(f), band, was: a, now: c, delta: c2 - a2, tolerance });
            }
        }
        out
    }

    /// Every value of `now` that moved from `self` (the committed digest)
    /// beyond its tolerance, largest overshoot first, as lines for a
    /// failure message.
    pub fn diff(&self, now: &Self) -> Vec<String> {
        let mut moved: Vec<Move> = self.moves(now).into_iter().filter(|m| m.delta.abs() > m.tolerance).collect();
        moved.sort_by(|a, b| (b.delta.abs() - b.tolerance).total_cmp(&(a.delta.abs() - a.tolerance)));
        let total = moved.len();
        let mut lines: Vec<String> = moved.iter().take(16).map(Move::line).collect();
        if total > 16 {
            lines.push(format!("… and {} more of {} compared values", total - 16, FRAMES * BANDS + 2));
        }
        if self.model != now.model {
            lines.push(format!("(the model line changed too: `{}` → `{}`)", self.model, now.model));
        }
        lines
    }
}

/// One compared value.
struct Move {
    frame: Option<usize>,
    band: String,
    was: f64,
    now: f64,
    delta: f64,
    tolerance: f64,
}

impl Move {
    fn line(&self) -> String {
        let at = match self.frame {
            Some(f) => {
                let centre = (f * HOP) as f64 / f64::from(RATE);
                let half = HOP as f64 / f64::from(RATE);
                format!("{:.3}–{:.3} s, {}", (centre - half).max(0.0), centre + half, self.band)
            }
            None => self.band.clone(),
        };
        format!(
            "{at}: {:.3} → {:.3} dB ({:+.3} dB, tolerance {:.3})",
            self.was, self.now, self.delta, self.tolerance
        )
    }
}

/// Run `tick` over the [`stimulus`] and hold its digest to the committed
/// one of `case` (or, with `STAVE_BLESS=1`, write it). `tick` is
/// whatever runs the stage at 48 kHz, prepared.
#[track_caller]
pub fn pin(case: &str, model: &str, mut tick: impl FnMut(f32) -> f32) {
    let output: Vec<f32> = stimulus().into_iter().map(&mut tick).collect();
    let now = Digest::of(case, model, &output);
    if blessing() {
        now.write();
        return;
    }
    let committed = match Digest::committed(case) {
        Some(Ok(c)) => c,
        Some(Err(e)) => panic!("golden `{case}`: {e}"),
        None => panic!("golden `{case}`: none committed at {} (write it with STAVE_BLESS=1)", path(case).display()),
    };
    let moved = committed.diff(&now);
    if let Ok(dir) = std::env::var("STAVE_GOLDEN_REPORT") {
        // The cross-platform check: each case's closest approach to its
        // tolerance, one line per case.
        let (delta, tolerance) = committed.closest(&now);
        let line = format!("{case} {delta:.4} {tolerance:.4}\n");
        let file = Path::new(&dir).join(case.replace('/', "__"));
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(file, line);
    }
    assert!(
        moved.is_empty(),
        "golden `{case}` moved — the stage sounds different from the one pinned. \
         If that is intended, re-bless on purpose with STAVE_BLESS=1 and review the diff:\n  {}",
        moved.join("\n  ")
    );
}

/// [`pin`] for a `Stage`: prepared at 48 kHz, ticked.
#[track_caller]
pub fn pin_stage(case: &str, model: &str, mut stage: impl Stage) {
    stage.prepare(RATE);
    pin(case, model, |x| stage.tick(x));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(scale: f32) -> Digest {
        let x: Vec<f32> = stimulus().iter().map(|v| v * scale).collect();
        Digest::of("unit/scale", "a gain", &x)
    }

    #[test]
    fn digests_round_trip_through_text() {
        let d = digest_of(0.5);
        assert_eq!(Digest::parse(&d.describe()).unwrap(), d);
    }

    #[test]
    fn the_stimulus_reads_as_designed() {
        let d = digest_of(1.0);
        assert!((d.peak_db - -1.249).abs() < 0.05, "peak {}", d.peak_db);
        // The loud noise burst: −6 dB RMS spread over the bands.
        let loud = 10.0 * d.bands[18].iter().map(|v| 10f64.powf(v / 10.0)).sum::<f64>().log10();
        assert!((loud - -6.0).abs() < 0.5, "loud burst {loud}");
    }

    #[test]
    fn a_change_in_sound_is_named_and_noise_is_not() {
        let d = digest_of(1.0);
        assert!(d.diff(&digest_of(1.0 + 1e-6)).is_empty());
        let moved = d.diff(&digest_of(1.01));
        assert!(!moved.is_empty());
        // 1 % is +0.086 dB everywhere (the written values' rounding aside).
        assert!(moved.iter().filter(|l| !l.starts_with('…')).all(|l| l.contains("(+0.08")), "{moved:?}");
    }
}
