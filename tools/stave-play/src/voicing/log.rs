//! The session log: one JSON document per session (settings, material by
//! name and checksum, candidates, verdicts, statistics) and a short human
//! summary beside it, rewritten after every verdict so a crash loses at
//! most the trial in hand. Logs live under the user's local data
//! directory unless configured elsewhere; never in a repository.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::bless::MaterialRecord;
use super::candidate::Candidate;
use super::takes::{Matching, Prepared, ResolvedParam, TakeKind};
use super::trials::{AbxRun, AbxStats, AbxTrial, RatingRun, RatingStats, RatingTrial, Stimulus};

pub const FORMAT: &str = "stave-play voicing session";

/// The environment variable that moves the voicing directory.
pub const DIR_ENV: &str = "STAVE_VOICING_DIR";

/// Where sessions and bless records go by default:
/// `<local data>/stave-play/voicing` (`~/Library/Application Support`
/// on macOS, `$XDG_DATA_HOME` on Linux).
pub fn default_dir() -> PathBuf {
    if let Some(d) = std::env::var_os(DIR_ENV) {
        return PathBuf::from(d);
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("stave-play")
        .join("voicing")
}

pub fn sessions_dir(root: &Path) -> PathBuf {
    root.join("sessions")
}

pub fn blessed_dir(root: &Path) -> PathBuf {
    root.join("blessed")
}

#[derive(Clone, Debug, Serialize)]
pub struct TakeRecord {
    pub label: String,
    pub reported_latency: u32,
    /// Residual offset (frames) and correlation against the dry input.
    pub measured_offset: Option<(i64, f64)>,
    pub resolved: Vec<ResolvedParam>,
    pub warnings: Vec<String>,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CandidateRecord {
    pub index: usize,
    pub candidate: Candidate,
    pub reference: bool,
    pub take: TakeRecord,
}

#[derive(Clone, Debug, Serialize)]
pub struct AbxRecord {
    pub a: String,
    pub b: String,
    pub pair: (usize, usize),
    pub seed: u64,
    pub trials: Vec<AbxTrial>,
    pub stats: AbxStats,
}

#[derive(Clone, Debug, Serialize)]
pub struct RatingRecord {
    pub seed: u64,
    /// Labels by stimulus index.
    pub labels: Vec<String>,
    pub stimuli: Vec<Stimulus>,
    pub trials: Vec<RatingTrial>,
    pub stats: RatingStats,
}

#[derive(Clone, Debug, Serialize)]
pub struct Preference {
    pub at: String,
    pub candidate: usize,
    pub label: String,
    /// Sighted: the listener knew which was which.
    pub sighted: bool,
    pub notes: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlessedRef {
    pub at: String,
    pub candidate: usize,
    pub label: String,
    pub record: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct Playback {
    pub crossfade_ms: f64,
    pub loop_seconds: [f64; 2],
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionLog {
    pub format: &'static str,
    pub version: u32,
    pub id: String,
    pub started: String,
    pub updated: String,
    pub stave_play_version: &'static str,
    pub sample_rate: f64,
    pub block_size: usize,
    pub material: MaterialRecord,
    pub material_channels: usize,
    pub material_original_rate: f64,
    pub playback: Playback,
    pub loudness: Matching,
    pub candidates: Vec<CandidateRecord>,
    pub anchor: Option<TakeRecord>,
    pub abx: Vec<AbxRecord>,
    pub ratings: Vec<RatingRecord>,
    pub preferences: Vec<Preference>,
    pub blessed: Vec<BlessedRef>,
    /// Each candidate nulled against the reference (DSP.md G5).
    pub nulls: Vec<super::null::NullTest>,
    pub notes: String,
}

/// What a session has done so far, beside its prepared takes.
#[derive(Default)]
pub struct History {
    pub abx: Vec<AbxRecord>,
    pub ratings: Vec<RatingRecord>,
    pub preferences: Vec<Preference>,
    pub blessed: Vec<BlessedRef>,
    pub notes: String,
}

impl History {
    pub fn record_abx(&mut self, prepared: &Prepared, run: &AbxRun) {
        let label = |t: usize| prepared.takes[t].label.clone();
        let rec = AbxRecord {
            a: label(run.pair.0),
            b: label(run.pair.1),
            pair: run.pair,
            seed: run.seed,
            trials: run.trials.clone(),
            stats: run.stats(),
        };
        // A run in progress replaces its own earlier snapshot.
        match self.abx.iter_mut().find(|r| r.seed == run.seed) {
            Some(r) => *r = rec,
            None => self.abx.push(rec),
        }
    }

    pub fn record_rating(&mut self, prepared: &Prepared, run: &RatingRun) {
        let rec = RatingRecord {
            seed: run.seed,
            labels: run
                .stimuli
                .iter()
                .map(|s| stimulus_label(prepared, s))
                .collect(),
            stimuli: run.stimuli.clone(),
            trials: run.trials.clone(),
            stats: run.summary(),
        };
        match self.ratings.iter_mut().find(|r| r.seed == run.seed) {
            Some(r) => *r = rec,
            None => self.ratings.push(rec),
        }
    }
}

pub fn stimulus_label(prepared: &Prepared, s: &Stimulus) -> String {
    match s {
        Stimulus::Candidate { take, .. } => prepared.takes[*take].label.clone(),
        Stimulus::HiddenReference { take } => format!("hidden reference ({})", prepared.takes[*take].label),
        Stimulus::Anchor { take } => prepared.takes[*take].label.clone(),
    }
}

fn take_record(t: &super::takes::Take) -> TakeRecord {
    TakeRecord {
        label: t.label.clone(),
        reported_latency: t.reported_latency,
        measured_offset: t.offset,
        resolved: t.resolved.clone(),
        warnings: t.warnings.clone(),
        sha256: t.sha256.clone(),
    }
}

/// The session's identity and timing.
pub struct Meta {
    pub id: String,
    pub started: String,
    pub crossfade_ms: f64,
    pub loop_seconds: [f64; 2],
}

pub fn build(prepared: &Prepared, meta: &Meta, history: &History) -> SessionLog {
    let m = &prepared.material;
    let candidates = prepared
        .takes
        .iter()
        .filter_map(|t| match t.kind {
            TakeKind::Candidate { index } => Some(CandidateRecord {
                index,
                candidate: prepared.candidates[index].clone(),
                reference: prepared.reference == Some(index),
                take: take_record(t),
            }),
            TakeKind::Anchor { .. } => None,
        })
        .collect();
    SessionLog {
        format: FORMAT,
        version: 1,
        id: meta.id.clone(),
        started: meta.started.clone(),
        updated: now_iso(),
        stave_play_version: env!("CARGO_PKG_VERSION"),
        sample_rate: prepared.sample_rate,
        block_size: prepared.block_size,
        material: MaterialRecord {
            name: m.name.clone(),
            source: m.source.clone(),
            pcm_sha256: m.pcm_sha256.clone(),
            seconds: m.seconds(),
            trim_db: prepared.trim_db,
        },
        material_channels: m.channels.len(),
        material_original_rate: m.original_rate,
        playback: Playback {
            crossfade_ms: meta.crossfade_ms,
            loop_seconds: meta.loop_seconds,
        },
        loudness: prepared.matching.clone(),
        candidates,
        anchor: prepared.anchor.map(|a| take_record(&prepared.takes[a])),
        abx: history.abx.clone(),
        ratings: history.ratings.clone(),
        preferences: history.preferences.clone(),
        blessed: history.blessed.clone(),
        nulls: prepared.nulls.clone().unwrap_or_default(),
        notes: history.notes.clone(),
    }
}

/// A few lines a person reads.
pub fn summary(log: &SessionLog) -> String {
    let mut s = String::new();
    let mut line = |t: String| {
        s.push_str(&t);
        s.push('\n');
    };
    line(format!("{} {}", FORMAT, log.id));
    line(format!("started {}, updated {}", log.started, log.updated));
    let src = match &log.material.source {
        super::material::Source::Builtin { .. } => "built-in".to_string(),
        super::material::Source::File { path, .. } => path.display().to_string(),
        super::material::Source::Corpus { role, item, files, .. } => {
            let sums: Vec<&str> = files.iter().map(|(_, s)| s.as_deref().map_or("unlocked", |s| &s[..12])).collect();
            format!("corpus role {role}: {item}, sha256 {}", sums.join(" "))
        }
    };
    line(format!(
        "material: {} ({src}), {:.1} s, sha256 {}…, trim {:+.1} dB, {} Hz",
        log.material.name,
        log.material.seconds,
        &log.material.pcm_sha256[..12],
        log.material.trim_db,
        log.sample_rate
    ));
    line(format!(
        "loudness: every take at {:.2} LUFS (max deviation {:.3} LU; headroom {:+.1} dB)",
        log.loudness.target_lufs, log.loudness.max_deviation_lu, log.loudness.headroom_db
    ));
    line("candidates:".into());
    for (i, c) in log.candidates.iter().enumerate() {
        let r = if c.reference { "  [reference]" } else { "" };
        line(format!(
            "  {}. {}  gain {:+.2} dB, latency {}{r}",
            i + 1,
            c.candidate.label,
            log.loudness.gain_db.get(i).copied().unwrap_or(0.0),
            c.take.reported_latency
        ));
        for w in &c.take.warnings {
            line(format!("     warning: {w}"));
        }
    }
    for n in &log.nulls {
        line(format!("null {}", n.describe(log.sample_rate)));
    }
    for a in &log.abx {
        let st = &a.stats;
        line(format!(
            "ABX {} vs {}: {}/{} correct (planned {}), p = {:.4}, d′ = {:.2}, power {:.0} % at {} → {}",
            a.a,
            a.b,
            st.correct,
            st.trials,
            st.planned,
            st.p_value,
            st.d_prime,
            st.power * 100.0,
            st.assumed_hit_rate,
            st.verdict.describe()
        ));
    }
    for r in &log.ratings {
        line(format!("rating ({} of {} trials):", r.stats.trials, r.stats.planned));
        for (i, (_, sum)) in r.stats.per_stimulus.iter().enumerate() {
            match sum {
                Some(x) => line(format!(
                    "  {:<40} mean {:5.1}{}  median {:5.1}",
                    r.labels[i],
                    x.mean,
                    x.ci95.map_or(String::new(), |c| format!(" ± {c:4.1}")),
                    x.median
                )),
                None => line(format!("  {:<40} (no ratings)", r.labels[i])),
            }
        }
        if !r.stats.hidden_reference_ratings.is_empty() && !r.stats.passes_screening {
            line("  the hidden reference scored below 90 on more than 15 % of trials (BS.1534-3 would exclude this run)".into());
        }
    }
    for p in &log.preferences {
        let how = if p.sighted { "sighted" } else { "blind" };
        let notes = if p.notes.is_empty() { String::new() } else { format!(": {}", p.notes) };
        line(format!("preferred ({how}): {}{notes}", p.label));
    }
    for b in &log.blessed {
        line(format!("blessed: {} → {}", b.label, b.record.display()));
    }
    if !log.notes.is_empty() {
        line(format!("notes: {}", log.notes));
    }
    s
}

/// Write the JSON and the summary; returns the JSON's path.
pub fn write(log: &SessionLog, root: &Path) -> std::io::Result<PathBuf> {
    let dir = sessions_dir(root);
    std::fs::create_dir_all(&dir)?;
    let json = dir.join(format!("{}.json", log.id));
    let text = serde_json::to_string_pretty(log).expect("a session log serialises");
    write_atomically(&json, text.as_bytes())?;
    write_atomically(&dir.join(format!("{}.txt", log.id)), summary(log).as_bytes())?;
    Ok(json)
}

/// Write via a temporary file and a rename, so a reader never sees half
/// a file.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("partial");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Seconds since the epoch, as UTC civil time.
fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's days-from-civil, inverted.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3_600) as u32, (rem % 3_600 / 60) as u32, (rem % 60) as u32)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `2026-11-13T08:15:00Z`.
pub fn now_iso() -> String {
    iso(now_secs())
}

fn iso(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// `20261113T081500Z`: sortable, file-name safe.
pub fn compact_stamp() -> String {
    let (y, mo, d, h, mi, s) = civil(now_secs());
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_time_matches_known_instants() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso(1_794_556_800), "2026-11-13T08:00:00Z");
    }

    #[test]
    fn atomic_writes_leave_no_partial_file() {
        let dir = std::env::temp_dir().join(format!("stave-voicing-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.json");
        write_atomically(&p, b"{}").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"{}");
        assert!(!p.with_extension("partial").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
