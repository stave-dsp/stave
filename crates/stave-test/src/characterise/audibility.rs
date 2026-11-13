//! A characterisation by audibility (DSP.md R16): the null residual of
//! a model's table against its reference's, run by run, judged by its
//! noise-to-mask ratio under the reference
//! ([`stave_measure::perceptual::null`]).
//!
//! Each run of the tier that a listener could hear as such — the
//! multisines, the tones, the two-tone pairs; not a supply's load steps,
//! nor a burst's recovery, which C8 holds — is resynthesised from the
//! lines both tables carry (above DC, up to 20 kHz, under the model's
//! Nyquist) at 48 kHz, one period tiled to a quarter second. One level
//! and latency match is fitted over every run at once (a listening
//! session's single level match: a model whose gain moves with level
//! keeps that difference), the reference minus the aligned model is the
//! residual, and its NMR under the reference is read at the playback
//! level the fingerprint states: the loudest T0 run of the reference
//! plays as a full-scale sine at 92 dB SPL
//! ([`playback`]), so a run 40 dB under the knee plays 40 dB quieter.
//!
//! A table holds only the plan's measured lines, so the residual is the
//! one those lines carry: aliasing onto other bins is C10's (the ANMR).

use stave_measure::perceptual::null::{self, Alignment, Line, BAND_HZ};
use stave_measure::perceptual::{tiled, Playback, PLAYBACK_DB_SPL};
use stave_measure::perceptual::nmr::NmrReport;

use super::plan::{Kind, Plan, Run, Tier};
use super::table::Table;

/// The rate the residual is heard at.
pub const ANALYSIS_RATE: f64 = 48_000.0;
/// The delay searched either way: a millisecond (the models answer on
/// time — a latent black box is fed early, [`super::model::Model::Delayed`]
/// — so this finds sub-sample offsets and a stray sample or two).
pub const MAX_DELAY: f64 = 1e-3;
/// Each run's period, tiled to this long for the ear model.
const SECONDS: f64 = 0.25;

/// One run's null.
#[derive(Clone, Debug, PartialEq)]
pub struct RunNull {
    pub run: String,
    pub nmr: NmrReport,
}

/// The null of a model's table against a reference's over a tier.
#[derive(Clone, Debug, PartialEq)]
pub struct Audibility {
    pub alignment: Alignment,
    pub playback: Playback,
    pub runs: Vec<RunNull>,
}

impl Audibility {
    /// `model` against `reference` over `plan`'s `tier`, heard at
    /// `playback`; `None` when no run applies (a supply).
    pub fn of(plan: &Plan, tier: Tier, reference: &Table, model: &Table, playback: Playback) -> Option<Self> {
        let n = |run: &Run| run.period().samples(ANALYSIS_RATE);
        let runs: Vec<(&Run, Vec<Line>)> = plan
            .runs(tier)
            .filter(|run| matches!(run.kind, Kind::Tone { .. } | Kind::Multisine { .. } | Kind::TwoTone { .. }))
            .filter_map(|run| {
                let (r, m) = (&reference.runs.get(&run.id)?.periods[0], &model.runs.get(&run.id)?.periods[0]);
                let lines: Vec<Line> = m
                    .bins
                    .iter()
                    .filter(|(k, _)| *k > 0 && 2 * (*k as usize) < n(run) && run.period().hz(*k) <= BAND_HZ)
                    .filter_map(|&(k, cm)| Some((run.period().hz(k), r.get(k)?, cm)))
                    .collect();
                (!lines.is_empty()).then_some((run, lines))
            })
            .collect();
        if runs.is_empty() {
            return None;
        }
        let all: Vec<Line> = runs.iter().flat_map(|(_, l)| l.iter().copied()).collect();
        let alignment = Alignment::fit(&all, MAX_DELAY);
        let len = (SECONDS * ANALYSIS_RATE) as usize;
        let runs = runs
            .into_iter()
            .map(|(run, lines)| {
                let r = null::periodic(lines.iter().map(|l| (l.0, l.1)), n(run), ANALYSIS_RATE);
                let a = null::periodic(lines.iter().map(|l| (l.0, l.2 * alignment.factor(l.0))), n(run), ANALYSIS_RATE);
                RunNull {
                    run: run.id.clone(),
                    nmr: null::residual_nmr(&tiled(&r, len), &tiled(&a, len), ANALYSIS_RATE, playback),
                }
            })
            .collect();
        Some(Self {
            alignment,
            playback,
            runs,
        })
    }

    /// The worst total NMR over the runs, and its run.
    pub fn worst_total(&self) -> (f64, &str) {
        self.runs
            .iter()
            .map(|r| (r.nmr.total_db, r.run.as_str()))
            .fold((f64::NEG_INFINITY, ""), |a, b| if b.0 > a.0 { b } else { a })
    }

    /// The worst band over the runs: its NMR, its run and its centre.
    pub fn worst_band(&self) -> (f64, &str, f64) {
        self.runs
            .iter()
            .map(|r| (r.nmr.max_db, r.run.as_str(), r.nmr.max_hz))
            .fold((f64::NEG_INFINITY, "", 0.0), |a, b| if b.0 > a.0 { b } else { a })
    }

    /// Every run within the inaudible budget (total and worst band at
    /// most −10 dB).
    pub fn inaudible(&self) -> bool {
        null::inaudible(self.worst_total().0, self.worst_band().0)
    }

    /// One line: the verdict, the worst figures and the alignment.
    pub fn summary(&self) -> String {
        let (total, at) = self.worst_total();
        let (band, band_at, hz) = self.worst_band();
        format!(
            "{}: null NMR {total:.1} dB total ({at}), {band:.1} dB worst band ({band_at}, {hz:.0} Hz); aligned {:+.4} dB, {:+.3} µs",
            null::verdict(total, band),
            self.alignment.gain_db(),
            self.alignment.delay * 1e6
        )
    }
}

/// The playback level a characterisation is heard at: the largest peak
/// (about its mean) of `table`'s T0 runs, resynthesised at `rate` from
/// the lines under its Nyquist, plays as a full-scale sine at 92 dB SPL.
pub fn playback(plan: &Plan, table: &Table, rate: f64) -> Playback {
    let mut full_scale = 0.0f64;
    for run in plan.runs(Tier::T0) {
        if let Some(m) = table.runs.get(&run.id) {
            let n = run.period().samples(rate);
            let lines = m.periods[0].bins.iter().filter(|(k, _)| 2 * (*k as usize) < n).map(|&(k, c)| (run.period().hz(k), c));
            let x = null::periodic(lines, n, rate);
            let mean = x.iter().sum::<f64>() / x.len() as f64;
            full_scale = full_scale.max(x.iter().fold(0.0f64, |p, v| p.max((v - mean).abs())));
        }
    }
    Playback {
        full_scale: significant(if full_scale > 0.0 { full_scale } else { 1.0 }),
        db_spl: PLAYBACK_DB_SPL,
    }
}

/// Nine significant digits (a fingerprint's text reads back exactly).
fn significant(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    format!("{x:.8e}").parse().unwrap()
}
