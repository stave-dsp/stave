//! "Bless this sound": pin a chosen candidate — its settings, the
//! material it was judged on, and the take it produced — so a later
//! change to the model shows up instead of drifting silently.
//!
//! A bless record is a JSON file with its take beside it as a 32-bit
//! float WAV (the take as the candidate produced it, before loudness
//! matching). Two things read it:
//!
//! - a later session, as a `blessed:` candidate (the earlier version's
//!   sound, on the same material), and
//! - `stave-play --verify-blessed`, which runs the candidate again with
//!   the current build and reports the null residual against the take.
//!
//! Blessing a plugin or stage candidate also writes its **fingerprint**
//! (DSP.md R18) into the record's `fingerprint` field: the T0
//! characterisation of the candidate as a black box at the session's
//! rate — its knee, the multisine and tone runs re its knee, the
//! response, harmonics, compression and aliasing, and its own R19
//! metrics (Rnonlin, GedLee, ANMR) — in the fingerprint format the
//! repository commits (`stave_test::characterise::fingerprint`). The take
//! pins the sound on one piece of material; the fingerprint pins it on
//! the measurement suite, and `--verify-blessed` re-measures it and names
//! every characteristic that moved.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use stave_host::config::HostConfig;
use stave_host::wav;

use super::candidate::Candidate;
use super::log::{now_iso, write_atomically};
use super::material::{Material, Source as MaterialSource};
use super::takes::{self, Job, Prepared, ResolvedParam};

pub const FORMAT: &str = "stave-play bless";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialRecord {
    pub name: String,
    pub source: MaterialSource,
    pub pcm_sha256: String,
    pub seconds: f64,
    pub trim_db: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlessRecord {
    pub format: String,
    pub version: u32,
    pub blessed_at: String,
    /// The session the choice was made in.
    pub session: String,
    pub candidate: Candidate,
    pub resolved: Vec<ResolvedParam>,
    pub material: MaterialRecord,
    pub sample_rate: f64,
    pub block_size: usize,
    pub reported_latency: u32,
    /// The take's file, beside this record.
    pub take_wav: String,
    pub take_sha256: String,
    pub take_lufs: f64,
    /// DSP.md R18's fingerprint of the candidate (see [`fingerprint`]);
    /// `null` for a recording or blessed take, which cannot be re-run.
    pub fingerprint: Option<serde_json::Value>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, thiserror::Error)]
pub enum BlessError {
    #[error("could not write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write the take")]
    Wav(#[from] wav::WavError),
    #[error("could not read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{}: not a bless record", path.display())]
    Format {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("only a candidate can be blessed, not the anchor")]
    NotACandidate,
}

/// The fingerprint's JSON: its format, case, knee and the fingerprint
/// file's text.
pub const FINGERPRINT_FORMAT: &str = "stave fingerprint v1";

/// The R18 hook: the candidate's fingerprint at `sample_rate`, as JSON
/// (`{format, case, knee, text}`), or `None` for a candidate that does
/// not run (dry, a recording, a blessed take) or fails to open. Knee
/// `None` finds the knee; a blessed record's knee re-measures at the
/// same levels.
pub fn fingerprint_at(
    candidate: &Candidate,
    material: &Material,
    trim_db: f64,
    sample_rate: f64,
    knee: Option<stave_measure::suite::levels::Knee>,
) -> Option<stave_test::characterise::Fingerprint> {
    use stave_test::characterise::black_box::maker;
    use stave_test::characterise::fingerprint::of_black_box;
    if !matches!(candidate.source, super::candidate::Source::Plugin { .. }) {
        return None;
    }
    let config = HostConfig {
        sample_rate: stave_host::config::SampleRate::new(sample_rate).ok()?,
        max_block: std::num::NonZeroUsize::new(1).expect("non-zero"),
        ..HostConfig::default()
    };
    let mut jobs = takes::open_jobs(std::slice::from_ref(candidate), material, trim_db, config).ok()?;
    let Some(Job::Instance { instance, .. }) = jobs.pop() else { return None };
    let model = format!("stave-play candidate `{}`", candidate.label);
    let shared = std::rc::Rc::new(std::cell::RefCell::new(instance));
    let (fp, plan) = of_black_box(&slug(&candidate.label), &model, None, maker(shared, 1), sample_rate, knee);
    Some(fp.derived(&plan, None))
}

/// [`fingerprint_at`] as the record's JSON.
pub fn fingerprint(candidate: &Candidate, material: &Material, trim_db: f64, sample_rate: f64) -> Option<serde_json::Value> {
    let fp = fingerprint_at(candidate, material, trim_db, sample_rate, None)?;
    Some(serde_json::json!({
        "format": FINGERPRINT_FORMAT,
        "case": fp.case,
        "knee": fp.knee.map(|k| k.amplitude),
        "text": fp.describe(),
    }))
}

/// Bless candidate `index` of `prepared` into `dir`; returns the record's
/// path.
pub fn bless(
    prepared: &Prepared,
    index: usize,
    session: &str,
    notes: &str,
    dir: &Path,
) -> Result<PathBuf, BlessError> {
    let take = prepared
        .takes
        .iter()
        .position(|t| t.kind == takes::TakeKind::Candidate { index })
        .ok_or(BlessError::NotACandidate)?;
    let t = &prepared.takes[take];
    let candidate = &prepared.candidates[index];
    std::fs::create_dir_all(dir).map_err(|source| BlessError::Write {
        path: dir.into(),
        source,
    })?;
    let stem = format!("{}-{}", slug(&candidate.label), super::log::compact_stamp());
    let wav_name = format!("{stem}.wav");
    wav::write(
        &dir.join(&wav_name),
        &wav::Audio {
            sample_rate: prepared.sample_rate.round() as u32,
            channels: t.audio.to_vec(),
        },
    )?;
    let m = &prepared.material;
    let record = BlessRecord {
        format: FORMAT.into(),
        version: 1,
        blessed_at: now_iso(),
        session: session.into(),
        candidate: candidate.clone(),
        resolved: t.resolved.clone(),
        material: MaterialRecord {
            name: m.name.clone(),
            source: m.source.clone(),
            pcm_sha256: m.pcm_sha256.clone(),
            seconds: m.seconds(),
            trim_db: prepared.trim_db,
        },
        sample_rate: prepared.sample_rate,
        block_size: prepared.block_size,
        reported_latency: t.reported_latency,
        take_wav: wav_name,
        take_sha256: t.sha256.clone(),
        take_lufs: prepared.matching.raw_lufs[take],
        fingerprint: fingerprint(candidate, m, prepared.trim_db, prepared.sample_rate),
        notes: notes.into(),
    };
    let path = dir.join(format!("{stem}.json"));
    let json = serde_json::to_string_pretty(&record).expect("a bless record serialises");
    write_atomically(&path, json.as_bytes()).map_err(|source| BlessError::Write {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// A record and its take.
pub fn load(record: &Path) -> Result<(BlessRecord, wav::Audio), BlessError> {
    let text = std::fs::read_to_string(record).map_err(|source| BlessError::Read {
        path: record.into(),
        source,
    })?;
    let rec: BlessRecord = serde_json::from_str(&text).map_err(|source| BlessError::Format {
        path: record.into(),
        source,
    })?;
    let wav_path = record.with_file_name(&rec.take_wav);
    let audio = wav::read(&wav_path)?;
    Ok((rec, audio))
}

/// What re-running a blessed candidate with this build gives.
#[derive(Debug)]
pub struct Verification {
    pub record: PathBuf,
    pub label: String,
    /// Residual energy relative to the blessed take, dB (−∞ when
    /// bit-identical).
    pub residual_db: f64,
    pub bit_identical: bool,
    /// The characteristics whose fingerprint moved (DSP.md R18), each
    /// with its size; empty when it holds, `None` when the record has no
    /// fingerprint.
    pub fingerprint_moved: Option<Vec<String>>,
}

/// Run the blessed candidate again and null it against its take.
pub fn verify(record: &Path) -> Result<Verification, String> {
    let (rec, audio) = load(record).map_err(|e| crate::errors::with_causes(&e))?;
    let material = Material::from_source(&rec.material.source, rec.sample_rate).map_err(|e| crate::errors::with_causes(&e))?;
    if material.pcm_sha256 != rec.material.pcm_sha256 {
        return Err(format!(
            "the material '{}' has changed since it was blessed (checksum differs)",
            rec.material.name
        ));
    }
    let config = HostConfig {
        sample_rate: stave_host::config::SampleRate::new(rec.sample_rate).map_err(|e| e.to_string())?,
        max_block: std::num::NonZeroUsize::new(rec.block_size.max(1)).expect("non-zero"),
        ..HostConfig::default()
    };
    let mut jobs = takes::open_jobs(std::slice::from_ref(&rec.candidate), &material, rec.material.trim_db, config)
        .map_err(|e| crate::errors::with_causes(&e))?;
    let Some(Job::Instance { mut instance, .. }) = jobs.pop() else {
        return Err("only plugin and stage candidates can be re-run".into());
    };
    let input = takes::trimmed(&material, rec.material.trim_db);
    let now = takes::run(instance.as_mut(), &input, rec.sample_rate, rec.block_size, |_| {});
    let (mut diff, mut sig) = (0.0f64, 0.0f64);
    let mut identical = true;
    for (c, ch) in now.iter().enumerate() {
        let then = &audio.channels[c.min(audio.channels.len() - 1)];
        for (a, b) in ch.iter().zip(then) {
            identical &= a.to_bits() == b.to_bits();
            diff += f64::from(a - b).powi(2);
            sig += f64::from(*b).powi(2);
        }
    }
    let residual_db = if diff == 0.0 {
        f64::NEG_INFINITY
    } else {
        10.0 * (diff / sig.max(1e-30)).log10()
    };
    let fingerprint_moved = rec.fingerprint.as_ref().and_then(|f| f["text"].as_str()).map(|text| {
        use stave_test::characterise::fingerprint::{diff, Fingerprint};
        match Fingerprint::parse(text) {
            Ok(then) => match fingerprint_at(&rec.candidate, &material, rec.material.trim_db, rec.sample_rate, then.knee) {
                Some(now) => {
                    let plan = stave_test::characterise::fingerprint::stage_plan(&then.case, then.knee.expect("a black box's knee"), then.settle.unwrap_or(0.05));
                    diff(&then, &now, &plan)
                }
                None => vec!["the candidate no longer opens".to_string()],
            },
            Err(e) => vec![format!("the record's fingerprint does not parse: {e}")],
        }
    });
    Ok(Verification {
        record: record.into(),
        label: rec.candidate.label,
        residual_db,
        bit_identical: identical,
        fingerprint_moved,
    })
}

/// Every bless record under `path` (a record, or a folder of them).
pub fn records(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.into()];
    }
    let mut out: Vec<PathBuf> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    out.sort();
    out
}

/// A file-name-safe version of a label.
pub fn slug(label: &str) -> String {
    let s: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let s: String = s.chars().take(48).collect();
    if s.is_empty() {
        "take".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_file_safe() {
        assert_eq!(slug("stage:clipper/diode drive=0.6"), "stage-clipper-diode-drive-0-6");
        assert_eq!(slug("…"), "take");
    }
}
