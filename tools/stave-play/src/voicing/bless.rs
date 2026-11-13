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
//! When fingerprints (DSP.md R18) land, blessing also writes the
//! candidate's fingerprint into the record's `fingerprint` field, the one
//! hook below, and CI diffs it; until then the record and its take are the
//! pin.

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
    /// DSP.md R18's fingerprint of the candidate; `null` until
    /// fingerprints land (see [`fingerprint`]).
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

/// The R18 hook: the candidate's fingerprint, once fingerprints exist.
pub fn fingerprint(_candidate: &Candidate) -> Option<serde_json::Value> {
    None
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
        fingerprint: fingerprint(candidate),
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
    Ok(Verification {
        record: record.into(),
        label: rec.candidate.label,
        residual_db,
        bit_identical: identical,
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
