//! Null tests (DSP.md G5): a candidate against another model or a
//! capture — level and latency aligned, the residual's NMR under the
//! reference judged by R16's verdict, the residual itself played back
//! ("listen to the difference"), and a blind ABX of the pair one click
//! away. In the Voicing tab (the *Null test* mode) and headless
//! (`stave-play --null A B`).
//!
//! # Models
//!
//! What a listener's A/B leaves once loudness and latency are matched:
//! R16's null ([`stave_measure::perceptual::null`]) of each candidate
//! against the session's reference (the `ref` row, else the first),
//! both as heard — the reference at its loudness-matching gain, full
//! scale at 92 dB SPL — on the session's material.
//!
//! # Derivation
//!
//! Prepare already runs every take latency-compensated (each
//! candidate's reported latency dropped) and offset-checked. The null
//! fits one real gain and one delay (up to ±[`MAX_DELAY_SECONDS`]: a
//! capture through hardware brings its own round trip) on the mid
//! channel, `(L + R)/2`, by R16's least squares over the audible band,
//! then applies that one alignment to both channels
//! ([`null::apply`]); the verdict is the worse channel's total and
//! worst-band NMR, inaudible at −10 dB or below on both. The residual
//! (reference − aligned candidate) plays at its true level beside the
//! takes, and raised to the session's matched loudness to hear what
//! it is made of (never to judge how loud it is).
//!
//! # Verified
//!
//! `tools/stave-play/src/voicing/null.rs::tests`: identical takes sit
//! at the floor; a capture of the dry material 37 samples late and
//! 4 dB down aligns to it within 1e-3 dB and 1e-2 samples and is
//! inaudible; a 1 kHz tone added 20 dB under the material is AUDIBLE,
//! 100 dB under inaudible; the residual played back is the added tone;
//! `--null` parses.
//!
//! # Cost
//!
//! Per candidate, one FFT cross-correlation over the material and two
//! channels' NMR (the ear model's frames), off the audio thread while
//! the session prepares.
//!
//! # Aliasing
//!
//! None: an analysis.
//!
//! # Valid ranges
//!
//! Session rates from 32 kHz (the ear model's floor; below it the null
//! reports why there is no verdict). A candidate whose output does not
//! resemble the reference (a different effect) nulls loudly: the
//! verdict says it differs, not how.

use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use serde::Serialize;
use stave_host::config::HostConfig;
use stave_measure::perceptual::null;
use stave_measure::perceptual::Playback;

use super::candidate::Candidate;
use super::takes::{self, Prepared, Stereo, TakeKind};
use super::MaterialChoice;

/// The largest latency a null aligns, either way.
pub const MAX_DELAY_SECONDS: f64 = 0.05;
/// The ear model's lowest rate.
const MIN_RATE: f64 = 32_000.0;

/// One candidate nulled against the reference.
#[derive(Clone, Debug, Serialize)]
pub struct NullTest {
    /// The candidate (A) and the reference (B), as candidate indices.
    pub candidate: usize,
    pub reference: usize,
    pub a: String,
    pub b: String,
    /// The alignment applied to A to match B, the takes as produced
    /// (before the loudness match): gain (dB) and delay (samples; B is
    /// later than A when positive).
    pub gain_db: f64,
    pub delay_samples: f64,
    /// The worse channel's NMR: total, worst band and where.
    pub nmr_total_db: f64,
    pub nmr_max_db: f64,
    pub nmr_max_hz: f64,
    pub inaudible: bool,
    /// The residual's RMS against the reference's, dB.
    pub residual_db: f64,
    /// The gain that raises the residual to the session's loudness
    /// (`None` when it is silent to the meter).
    pub raised_gain_db: Option<f64>,
    /// The residual as heard (the reference's matching gain applied).
    #[serde(skip)]
    pub residual: Arc<Stereo>,
}

impl NullTest {
    pub fn verdict(&self) -> &'static str {
        null::verdict(self.nmr_total_db, self.nmr_max_db)
    }

    /// One line a person reads.
    pub fn describe(&self, sample_rate: f64) -> String {
        format!(
            "{}: {} vs {}: null NMR {:.1} dB total, {:.1} dB worst band ({:.0} Hz); residual {:.1} dB re the reference; aligned {:+.3} dB, {:+.2} samples ({:+.1} µs)",
            self.verdict(),
            self.a,
            self.b,
            self.nmr_total_db,
            self.nmr_max_db,
            self.nmr_max_hz,
            self.residual_db,
            self.gain_db,
            self.delay_samples,
            self.delay_samples / sample_rate * 1e6
        )
    }
}

fn mid(x: &Stereo, gain: f64) -> Vec<f64> {
    x[0].iter().zip(&x[1]).map(|(l, r)| 0.5 * gain * (f64::from(*l) + f64::from(*r))).collect()
}

fn scaled(x: &[f32], gain: f64) -> Vec<f64> {
    x.iter().map(|v| gain * f64::from(*v)).collect()
}

fn power(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64
}

/// Null take `a` against take `b` (both as produced, the same length),
/// each played at its matching gain; `target_lufs` is the session's
/// loudness, for the raised residual.
pub fn between(a: &Stereo, b: &Stereo, gains: (f64, f64), rate: f64, target_lufs: f64) -> Result<Nulled, String> {
    if rate < MIN_RATE {
        return Err(format!("the session runs at {rate:.0} Hz, under the ear model's 32 kHz: no null verdict"));
    }
    let (ga, gb) = gains;
    let playback = Playback::default();
    let alignment = null::align(&mid(b, gb), &mid(a, ga), rate, MAX_DELAY_SECONDS);
    let mut worst = (f64::NEG_INFINITY, f64::NEG_INFINITY, 0.0);
    let (mut residual_power, mut reference_power) = (0.0, 0.0);
    let mut residual: Stereo = [Vec::new(), Vec::new()];
    for ch in 0..2 {
        let reference = scaled(&b[ch], gb);
        let nulled = null::apply(&reference, &scaled(&a[ch], ga), alignment, rate, playback);
        let nmr = &nulled.null.nmr;
        worst = (worst.0.max(nmr.total_db), worst.1.max(nmr.max_db), if nmr.max_db > worst.1 { nmr.max_hz } else { worst.2 });
        residual_power += power(&nulled.residual);
        reference_power += power(&reference);
        residual[ch] = nulled.residual.iter().map(|v| *v as f32).collect();
    }
    let residual_db = 10.0 * (residual_power / reference_power.max(f64::MIN_POSITIVE)).max(1e-30).log10();
    let raised_gain_db = takes::lufs(&residual, 1.0, rate).map(|l| target_lufs - l);
    Ok(Nulled {
        gain_db: alignment.gain_db(),
        delay_samples: alignment.delay * rate,
        nmr_total_db: worst.0,
        nmr_max_db: worst.1,
        nmr_max_hz: worst.2,
        inaudible: null::inaudible(worst.0, worst.1),
        residual_db,
        raised_gain_db,
        residual: Arc::new(residual),
    })
}

/// [`between`]'s result before it is placed in a session.
pub struct Nulled {
    pub gain_db: f64,
    pub delay_samples: f64,
    pub nmr_total_db: f64,
    pub nmr_max_db: f64,
    pub nmr_max_hz: f64,
    pub inaudible: bool,
    pub residual_db: f64,
    pub raised_gain_db: Option<f64>,
    pub residual: Arc<Stereo>,
}

/// The take index of candidate `index`.
fn take_of(p: &Prepared, index: usize) -> Option<usize> {
    p.takes.iter().position(|t| t.kind == TakeKind::Candidate { index })
}

/// Every other candidate nulled against the session's reference (the
/// `ref` row, else the first candidate).
pub fn all(p: &Prepared) -> Result<Vec<NullTest>, String> {
    let reference = p.reference.unwrap_or(0);
    (0..p.candidates.len()).filter(|&i| i != reference).map(|i| pair(p, i, reference)).collect()
}

/// Candidate `a` nulled against candidate `b`.
pub fn pair(p: &Prepared, a: usize, b: usize) -> Result<NullTest, String> {
    let (ta, tb) = (take_of(p, a).ok_or("no take for A")?, take_of(p, b).ok_or("no take for B")?);
    let g = |t: usize| takes::db_to_gain(p.matching.gain_db[t]);
    let n = between(&p.takes[ta].audio, &p.takes[tb].audio, (g(ta), g(tb)), p.sample_rate, p.matching.target_lufs)?;
    Ok(NullTest {
        candidate: a,
        reference: b,
        a: p.candidates[a].label.clone(),
        b: p.candidates[b].label.clone(),
        // As heard the takes are loudness-matched; the gain is reported
        // between the takes as produced.
        gain_db: n.gain_db + p.matching.gain_db[ta] - p.matching.gain_db[tb],
        delay_samples: n.delay_samples,
        nmr_total_db: n.nmr_total_db,
        nmr_max_db: n.nmr_max_db,
        nmr_max_hz: n.nmr_max_hz,
        inaudible: n.inaudible,
        residual_db: n.residual_db,
        raised_gain_db: n.raised_gain_db,
        residual: n.residual,
    })
}

/// The extra player takes for the nulls: each residual at its true
/// level, then raised (its gain capped so it never peaks past the
/// takes' ceiling). Player take `first + 2j` is null `j`'s residual,
/// `first + 2j + 1` the raised one.
pub fn player_takes(nulls: &[NullTest]) -> Vec<(Arc<Stereo>, f32)> {
    nulls
        .iter()
        .flat_map(|n| {
            let peak = n.residual.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()));
            let ceiling = if peak > 0.0 {
                takes::PEAK_CEILING_DBFS - 20.0 * f64::from(peak).log10()
            } else {
                0.0
            };
            let raised = n.raised_gain_db.unwrap_or(0.0).min(ceiling);
            [(n.residual.clone(), 1.0), (n.residual.clone(), takes::db_to_gain(raised) as f32)]
        })
        .collect()
}

/// What `stave-play --null A B` asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct Headless {
    /// The candidate (A) and the reference (B), in `--compare`'s form.
    pub a: String,
    pub b: String,
    pub material: Option<String>,
    pub trim_db: f64,
    /// Write the residual (as heard) here, a 32-bit float WAV.
    pub residual: Option<PathBuf>,
}

/// The null test headless: both candidates prepared as the Voicing tab
/// prepares them (latency-compensated, loudness-matched), A nulled
/// against B. Returns the result and the session's rate.
pub fn headless(h: &Headless, config: HostConfig) -> Result<(NullTest, f64), String> {
    let rate = config.sample_rate.hz();
    let choice = h.material.as_deref().map_or(MaterialChoice::Builtin(super::BUILTINS[0].name.into()), MaterialChoice::parse);
    let material = choice.load(rate).map_err(|e| crate::errors::with_causes(&e))?;
    let parse = |spec: &str| Candidate::parse(spec).map(|c| c.0).map_err(|e| format!("{spec}: {e}"));
    let candidates = vec![parse(&h.a)?, parse(&h.b)?];
    let jobs = takes::open_jobs(&candidates, &material, h.trim_db, config).map_err(|e| crate::errors::with_causes(&e))?;
    let plan = takes::Plan {
        material,
        trim_db: h.trim_db,
        candidates,
        reference: Some(1),
        with_anchor: false,
        block_size: config.max_block.get(),
    };
    let prepared = takes::prepare(plan, jobs, &AtomicU32::new(0)).map_err(|e| crate::errors::with_causes(&e))?;
    let n = pair(&prepared, 0, 1)?;
    if let Some(path) = &h.residual {
        let audio = stave_host::wav::Audio {
            sample_rate: rate.round() as u32,
            channels: n.residual.to_vec(),
        };
        stave_host::wav::write(path, &audio).map_err(|e| crate::errors::with_causes(&e))?;
    }
    Ok((n, rate))
}

#[cfg(test)]
mod tests;
