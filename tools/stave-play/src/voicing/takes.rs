//! Takes: each candidate run over the material offline, latency
//! compensated, then every take loudness-matched (BS.1770 integrated) to
//! within 0.1 LU before anyone listens.
//!
//! - **Pre-roll.** Settings are applied, then half a second of silence
//!   runs before the material, so param smoothers, DC blockers and
//!   envelopes start settled instead of gliding in under the first note.
//! - **Latency.** Each instance runs `latency()` frames past the
//!   material's end and the take drops its first `latency()` frames, so
//!   every take lines up with the dry material sample for sample. The
//!   residual offset is then measured by cross-correlation against the
//!   dry input, and a plugin that misreports its latency is flagged.
//! - **Loudness.** Every take is turned down to the quietest one, then
//!   all by a common amount if any would peak above −1 dBFS, and the
//!   match is re-measured until each is within 0.01 LU (the claim is
//!   0.1 LU). Gains are applied at playback, never baked in, so the
//!   take stays what the candidate produced.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde::Serialize;
use stave_host::config::HostConfig;
use stave_host::instance::{apply_preset, Instance};
use stave_host::opened::Opened;
use stave_measure::loudness::{LevelMeasure, Lufs, Outside, StreamSpec};

use super::bless;
use super::candidate::{Candidate, Source};
use super::material::{convert_rate, Material};
use super::sha256;

/// Silence run before the material.
pub const PREROLL_SECONDS: f64 = 0.5;
/// No matched take peaks above this (−1 dBFS sample peak).
pub const PEAK_CEILING_DBFS: f64 = -1.0;
/// The match every comparison is held to.
pub const LOUDNESS_TOLERANCE_LU: f64 = 0.1;
/// What the refinement converges to (well inside the tolerance).
const REFINE_LU: f64 = 0.01;
/// BS.1534-3's low anchor: the reference low-passed here.
pub const ANCHOR_HZ: f64 = 3_500.0;
/// The lag range the latency check searches, frames.
const OFFSET_SEARCH: i64 = 1_024;

/// A param as the candidate ran it.
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
pub struct ResolvedParam {
    pub name: String,
    pub value: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
}

/// What a candidate turns into before processing.
pub enum Job {
    Instance {
        instance: Box<dyn Instance + Send>,
        resolved: Vec<ResolvedParam>,
    },
    Dry,
    /// Audio already at the session rate (a recording or blessed take).
    Audio { channels: Vec<Vec<f32>> },
}

/// Stereo audio, equal-length channels.
pub type Stereo = [Vec<f32>; 2];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TakeKind {
    Candidate { index: usize },
    /// The BS.1534 low anchor, made from the take at `from`.
    Anchor { from: usize },
}

/// One candidate's output over the material.
#[derive(Clone, Debug)]
pub struct Take {
    pub kind: TakeKind,
    pub label: String,
    /// As the candidate produced it (no gain).
    pub audio: Arc<Stereo>,
    pub reported_latency: u32,
    /// Residual offset against the dry input after compensation, with
    /// its normalised correlation (when the output resembles the input).
    pub offset: Option<(i64, f64)>,
    pub resolved: Vec<ResolvedParam>,
    pub warnings: Vec<String>,
    /// SHA-256 of the take's samples: equal hashes, the same sound.
    pub sha256: String,
}

/// The loudness match over a set of takes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Matching {
    pub measure: &'static str,
    pub target_lufs: f64,
    pub raw_lufs: Vec<f64>,
    pub gain_db: Vec<f64>,
    pub matched_lufs: Vec<f64>,
    pub matched_peak_dbfs: Vec<f64>,
    pub max_deviation_lu: f64,
    /// The common turn-down for headroom (0 when none was needed).
    pub headroom_db: f64,
}

impl Matching {
    pub fn gain(&self, take: usize) -> f32 {
        db_to_gain(self.gain_db[take]) as f32
    }
}

/// Everything a session plays: the material, its takes, their match.
pub struct Prepared {
    pub material: Material,
    pub trim_db: f64,
    pub candidates: Vec<Candidate>,
    pub reference: Option<usize>,
    pub takes: Vec<Take>,
    /// The anchor's index in `takes`, when one was made.
    pub anchor: Option<usize>,
    pub matching: Matching,
    pub sample_rate: f64,
    pub block_size: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    #[error("{label}: could not open")]
    Open {
        label: String,
        #[source]
        source: stave_host::opened::OpenError,
    },
    #[error("{label}: {message}")]
    Candidate { label: String, message: String },
    #[error("{label} is silent on this material (no integrated loudness above the −70 LUFS gate)")]
    Silent { label: String },
    #[error("loudness match missed: {deviation:.3} LU > {LOUDNESS_TOLERANCE_LU} LU")]
    Match { deviation: f64 },
    #[error("nothing to compare: add at least two candidates")]
    TooFew,
}

fn candidate_err(c: &Candidate, message: impl Into<String>) -> PrepareError {
    PrepareError::Candidate {
        label: c.label.clone(),
        message: message.into(),
    }
}

/// Open and configure every candidate (on the calling thread: plugin
/// factories expect the thread that loaded them).
pub fn open_jobs(
    candidates: &[Candidate],
    material: &Material,
    trim_db: f64,
    config: HostConfig,
) -> Result<Vec<Job>, PrepareError> {
    candidates
        .iter()
        .map(|c| open_job(c, material, trim_db, config))
        .collect()
}

fn open_job(
    c: &Candidate,
    material: &Material,
    trim_db: f64,
    config: HostConfig,
) -> Result<Job, PrepareError> {
    match &c.source {
        Source::Dry => Ok(Job::Dry),
        Source::Plugin { path } => {
            let opened = Opened::open(path, config).map_err(|source| PrepareError::Open {
                label: c.label.clone(),
                source,
            })?;
            let mut instance: Box<dyn Instance + Send> = opened.into_send();
            if instance.layout().inputs == 0 {
                return Err(candidate_err(
                    c,
                    "an instrument (no audio input) cannot process program material",
                ));
            }
            if let Some(preset) = &c.preset {
                apply_preset(instance.as_mut(), preset).map_err(|e| candidate_err(c, e.to_string()))?;
            }
            for s in &c.settings {
                instance
                    .set_param(&s.name, s.value)
                    .map_err(|e| candidate_err(c, format!("{}: {e}", s.name)))?;
            }
            let resolved = instance
                .params()
                .into_iter()
                .filter(|p| !p.read_only)
                .map(|p| ResolvedParam {
                    name: p.name,
                    value: p.value,
                    unit: p.unit,
                })
                .collect();
            Ok(Job::Instance { instance, resolved })
        }
        Source::Recording { path } => {
            let m = Material::file(path, material.sample_rate).map_err(|e| candidate_err(c, e.to_string()))?;
            Ok(Job::Audio {
                channels: fit_length(m.channels, material.frames()),
            })
        }
        Source::Blessed { record } => {
            let (rec, audio) = bless::load(record).map_err(|e| candidate_err(c, e.to_string()))?;
            if (rec.material.trim_db - trim_db).abs() > 1e-9 {
                return Err(candidate_err(
                    c,
                    format!(
                        "blessed at input trim {:+.1} dB; set the trim to match (now {trim_db:+.1} dB)",
                        rec.material.trim_db
                    ),
                ));
            }
            if rec.material.pcm_sha256 != material.pcm_sha256 {
                return Err(candidate_err(
                    c,
                    format!(
                        "blessed on different material ({}, {}…); pick that material to compare against it",
                        rec.material.name,
                        &rec.material.pcm_sha256[..12.min(rec.material.pcm_sha256.len())]
                    ),
                ));
            }
            let mut channels = audio.channels;
            if (f64::from(audio.sample_rate) - material.sample_rate).abs() > 0.5 {
                channels = channels
                    .iter()
                    .map(|ch| convert_rate(ch, f64::from(audio.sample_rate), material.sample_rate))
                    .collect();
            }
            Ok(Job::Audio {
                channels: fit_length(channels, material.frames()),
            })
        }
    }
}

fn fit_length(mut channels: Vec<Vec<f32>>, frames: usize) -> Vec<Vec<f32>> {
    for c in &mut channels {
        c.resize(frames, 0.0);
    }
    channels
}

/// The material after the input trim, as each candidate hears it.
pub fn trimmed(material: &Material, trim_db: f64) -> Vec<Vec<f32>> {
    let g = db_to_gain(trim_db) as f32;
    material
        .channels
        .iter()
        .map(|c| c.iter().map(|v| v * g).collect())
        .collect()
}

/// What to prepare: the material, its trim, the candidates and options.
pub struct Plan {
    pub material: Material,
    pub trim_db: f64,
    pub candidates: Vec<Candidate>,
    pub reference: Option<usize>,
    pub with_anchor: bool,
    pub block_size: usize,
}

/// Run every job (one per candidate, from [`open_jobs`]) and match the
/// takes. `progress` counts to 1000.
pub fn prepare(plan: Plan, jobs: Vec<Job>, progress: &AtomicU32) -> Result<Prepared, PrepareError> {
    let Plan {
        material,
        trim_db,
        candidates,
        reference,
        with_anchor,
        block_size,
    } = plan;
    if candidates.len() < 2 {
        return Err(PrepareError::TooFew);
    }
    let input = trimmed(&material, trim_db);
    let frames = material.frames();
    let rate = material.sample_rate;
    let work: usize = jobs
        .iter()
        .map(|j| usize::from(matches!(j, Job::Instance { .. })))
        .sum::<usize>()
        .max(1);
    let mut done = 0;
    let mut takes = Vec::with_capacity(candidates.len() + 1);
    for (index, (c, job)) in candidates.iter().zip(jobs).enumerate() {
        let take = match job {
            Job::Dry => Take {
                kind: TakeKind::Candidate { index },
                label: c.label.clone(),
                audio: Arc::new(to_stereo(&input, frames)),
                reported_latency: 0,
                offset: None,
                resolved: Vec::new(),
                warnings: Vec::new(),
                sha256: String::new(),
            },
            Job::Audio { channels } => Take {
                kind: TakeKind::Candidate { index },
                label: c.label.clone(),
                audio: Arc::new(to_stereo(&channels, frames)),
                reported_latency: 0,
                offset: None,
                resolved: Vec::new(),
                warnings: Vec::new(),
                sha256: String::new(),
            },
            Job::Instance { mut instance, resolved } => {
                let base = done;
                let report = |f: f64| {
                    let p = (base as f64 + f) / work as f64;
                    progress.store((p * 1000.0) as u32, Ordering::Relaxed);
                };
                let latency = instance.latency();
                let audio = run(instance.as_mut(), &input, rate, block_size, report);
                done += 1;
                let offset = measure_offset(&input, &audio[0]);
                let mut warnings = Vec::new();
                if let Some((lag, r)) = offset {
                    if lag != 0 {
                        warnings.push(format!(
                            "output is offset {lag:+} frames beyond its reported latency ({latency}) \
                             (correlation {r:.2}): it may misreport its latency, which comb-filters \
                             the switch"
                        ));
                    }
                }
                if audio.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(candidate_err(c, "produced NaN or infinite samples"));
                }
                Take {
                    kind: TakeKind::Candidate { index },
                    label: c.label.clone(),
                    audio: Arc::new(audio),
                    reported_latency: latency,
                    offset,
                    resolved,
                    warnings,
                    sha256: String::new(),
                }
            }
        };
        takes.push(take);
    }
    let anchor = if with_anchor {
        let from = reference.unwrap_or(0);
        takes.push(Take {
            kind: TakeKind::Anchor { from },
            label: format!("anchor ({} kHz low-pass)", ANCHOR_HZ / 1000.0),
            audio: Arc::new(low_pass(&takes[from].audio, ANCHOR_HZ, rate)),
            reported_latency: 0,
            offset: None,
            resolved: Vec::new(),
            warnings: Vec::new(),
            sha256: String::new(),
        });
        Some(takes.len() - 1)
    } else {
        None
    };
    for t in &mut takes {
        t.sha256 = sha256::of_audio(&t.audio[..]);
    }
    let audio: Vec<&Stereo> = takes.iter().map(|t| &*t.audio).collect();
    let matching = match_loudness(&audio, rate).map_err(|e| match e {
        MatchError::Silent(i) => PrepareError::Silent {
            label: takes[i].label.clone(),
        },
        MatchError::Missed(deviation) => PrepareError::Match { deviation },
    })?;
    progress.store(1000, Ordering::Relaxed);
    Ok(Prepared {
        material,
        trim_db,
        candidates,
        reference,
        takes,
        anchor,
        matching,
        sample_rate: rate,
        block_size,
    })
}

fn to_stereo(channels: &[Vec<f32>], frames: usize) -> Stereo {
    let get = |c: usize| {
        let mut v = channels[c.min(channels.len() - 1)].clone();
        v.resize(frames, 0.0);
        v
    };
    [get(0), get(1)]
}

/// Run `inst` over `input` (pre-roll, latency compensation), returning
/// stereo output as long as the input.
pub fn run(
    inst: &mut dyn Instance,
    input: &[Vec<f32>],
    sample_rate: f64,
    block_size: usize,
    mut progress: impl FnMut(f64),
) -> Stereo {
    let layout = inst.layout();
    let block = block_size.min(layout.max_block).max(1);
    let frames = input.first().map_or(0, Vec::len);
    let latency = inst.latency() as usize;
    let preroll = (PREROLL_SECONDS * sample_rate) as usize;
    let total = preroll + frames + latency;
    let (n_in, n_out) = (layout.inputs, layout.outputs.max(1));
    let mut ins = vec![vec![0.0f32; block]; n_in];
    let mut outs = vec![vec![0.0f32; block]; n_out];
    let mut out: Stereo = [Vec::with_capacity(frames), Vec::with_capacity(frames)];
    let mut pos = 0;
    while pos < total {
        let n = (total - pos).min(block);
        for (c, buf) in ins.iter_mut().enumerate() {
            for (i, s) in buf[..n].iter_mut().enumerate() {
                let f = (pos + i).wrapping_sub(preroll);
                *s = if pos + i < preroll || f >= frames {
                    0.0
                } else {
                    input_sample(input, c, n_in, f)
                };
            }
        }
        {
            let in_refs: Vec<&[f32]> = ins.iter().map(|b| &b[..n]).collect();
            let mut out_refs: Vec<&mut [f32]> = outs.iter_mut().map(|b| &mut b[..n]).collect();
            inst.process(&in_refs, &mut out_refs, &[]);
        }
        for i in 0..n {
            let at = pos + i;
            if at >= preroll + latency && out[0].len() < frames {
                out[0].push(outs[0][i]);
                out[1].push(outs[1.min(n_out - 1)][i]);
            }
        }
        pos += n;
        progress(pos as f64 / total as f64);
    }
    out
}

/// The material sample input channel `c` of `n_in` hears: mono material
/// feeds every input; stereo feeds L/R (summed for a mono input).
fn input_sample(input: &[Vec<f32>], c: usize, n_in: usize, f: usize) -> f32 {
    match (input.len(), n_in) {
        (1, _) => input[0][f],
        (_, 1) => 0.5 * (input[0][f] + input[1][f]),
        (m, _) => input[c % m][f],
    }
}

/// The residual offset of `take` against the dry input (mono mix), by
/// normalised cross-correlation over the loudest stretch; `None` when
/// the output no longer resembles the input (|r| < 0.5).
pub fn measure_offset(input: &[Vec<f32>], take: &[f32]) -> Option<(i64, f64)> {
    const LEN: usize = 8_192;
    let frames = take.len();
    if frames < LEN + 2 * OFFSET_SEARCH as usize {
        return None;
    }
    let dry: Vec<f32> = (0..frames)
        .map(|f| input.iter().map(|c| c[f]).sum::<f32>() / input.len() as f32)
        .collect();
    // The loudest LEN-frame stretch, searched in hops.
    let lo = OFFSET_SEARCH as usize;
    let hi = frames - LEN - OFFSET_SEARCH as usize;
    let start = (lo..hi)
        .step_by(1_024)
        .max_by(|&a, &b| energy(&dry[a..a + LEN]).total_cmp(&energy(&dry[b..b + LEN])))?;
    let d = &dry[start..start + LEN];
    let ed = energy(d);
    let (mut best, mut best_r) = (0i64, 0.0f64);
    for lag in -OFFSET_SEARCH..=OFFSET_SEARCH {
        let s = (start as i64 + lag) as usize;
        let t = &take[s..s + LEN];
        let et = energy(t);
        if ed <= 0.0 || et <= 0.0 {
            continue;
        }
        let dot: f64 = d.iter().zip(t).map(|(a, b)| f64::from(*a) * f64::from(*b)).sum();
        let r = dot / (ed * et).sqrt();
        if r.abs() > best_r.abs() {
            (best, best_r) = (lag, r);
        }
    }
    (best_r.abs() >= 0.5).then_some((best, best_r))
}

fn energy(x: &[f32]) -> f64 {
    x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum()
}

pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// BS.1770 integrated loudness of `audio` scaled by `gain`.
pub fn lufs(audio: &Stereo, gain: f64, sample_rate: f64) -> Option<f64> {
    let mut meter = Lufs::integrated().meter(&StreamSpec {
        channels: 2,
        sample_rate,
        outside: Outside::Silence,
    });
    let g = gain as f32;
    let (mut l, mut r) = (vec![0.0f32; 4_096], vec![0.0f32; 4_096]);
    for (a, b) in audio[0].chunks(4_096).zip(audio[1].chunks(4_096)) {
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            l[i] = x * g;
            r[i] = y * g;
        }
        meter.push(&[&l[..a.len()], &r[..a.len()]]);
    }
    meter.level()
}

fn peak(audio: &Stereo) -> f64 {
    audio
        .iter()
        .flatten()
        .fold(0.0f32, |m, v| m.max(v.abs()))
        .into()
}

#[derive(Debug, PartialEq)]
pub enum MatchError {
    /// The take at this index has no integrated loudness.
    Silent(usize),
    /// The best match still deviates this far (LU).
    Missed(f64),
}

/// Gains that bring every take to the quietest one's loudness (less a
/// common turn-down for headroom), refined by re-measuring.
pub fn match_loudness(takes: &[&Stereo], sample_rate: f64) -> Result<Matching, MatchError> {
    let raw: Vec<f64> = takes
        .iter()
        .enumerate()
        .map(|(i, t)| lufs(t, 1.0, sample_rate).ok_or(MatchError::Silent(i)))
        .collect::<Result<_, _>>()?;
    let quietest = raw.iter().copied().fold(f64::INFINITY, f64::min);
    let mut gain_db: Vec<f64> = raw.iter().map(|l| quietest - l).collect();
    let loudest_peak = takes
        .iter()
        .zip(&gain_db)
        .map(|(t, g)| 20.0 * (peak(t) * db_to_gain(*g)).max(1e-12).log10())
        .fold(f64::NEG_INFINITY, f64::max);
    let headroom_db = (PEAK_CEILING_DBFS - loudest_peak).min(0.0);
    let target = quietest + headroom_db;
    for g in &mut gain_db {
        *g += headroom_db;
    }
    let mut matched = vec![0.0; takes.len()];
    for (i, t) in takes.iter().enumerate() {
        // The gates make loudness only nearly linear in gain: correct
        // until the measurement agrees.
        for _ in 0..6 {
            let l = lufs(t, db_to_gain(gain_db[i]), sample_rate).ok_or(MatchError::Silent(i))?;
            matched[i] = l;
            if (l - target).abs() <= REFINE_LU {
                break;
            }
            gain_db[i] += target - l;
        }
    }
    let max_deviation_lu = matched
        .iter()
        .map(|l| (l - target).abs())
        .fold(0.0, f64::max);
    if max_deviation_lu > LOUDNESS_TOLERANCE_LU {
        return Err(MatchError::Missed(max_deviation_lu));
    }
    let matched_peak_dbfs = takes
        .iter()
        .zip(&gain_db)
        .map(|(t, g)| 20.0 * (peak(t) * db_to_gain(*g)).max(1e-12).log10())
        .collect();
    Ok(Matching {
        measure: "BS.1770-4 integrated loudness (lufs-i), stereo",
        target_lufs: target,
        raw_lufs: raw,
        gain_db,
        matched_lufs: matched,
        matched_peak_dbfs,
        max_deviation_lu,
        headroom_db,
    })
}

/// A 4th-order Butterworth low-pass (two RBJ biquads), for the anchor.
pub fn low_pass(audio: &Stereo, hz: f64, sample_rate: f64) -> Stereo {
    let w = std::f64::consts::TAU * hz / sample_rate;
    let biquad = |q: f64| {
        let alpha = w.sin() / (2.0 * q);
        let cw = w.cos();
        let a0 = 1.0 + alpha;
        let b0 = (1.0 - cw) / 2.0 / a0;
        [b0, 2.0 * b0, b0, -2.0 * cw / a0, (1.0 - alpha) / a0]
    };
    let sections = [biquad(0.541_196_1), biquad(1.306_563)];
    let run = |x: &[f32]| -> Vec<f32> {
        let mut y: Vec<f64> = x.iter().map(|v| f64::from(*v)).collect();
        for [b0, b1, b2, a1, a2] in sections {
            let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
            for s in &mut y {
                let x0 = *s;
                let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
                (x2, x1, y2, y1) = (x1, x0, y1, y0);
                *s = y0;
            }
        }
        y.into_iter().map(|v| v as f32).collect()
    };
    [run(&audio[0]), run(&audio[1])]
}

#[cfg(test)]
mod tests;
