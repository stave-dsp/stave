//! The listening modes' panels: sighted A/B (switch, prefer, bless),
//! blind ABX (trials, running statistics, verdict), rating (randomised
//! sliders with hidden reference and anchor) and the null test (each
//! candidate against the reference: alignment, NMR verdict, residual).

use std::path::Path;

use eframe::egui::{self, Color32, RichText};
use stave_measure::listening::{Abx, AbxVerdict, ALPHA, ASSUMED_HIT_RATE};

use super::log::{self, BlessedRef, Preference};
use super::null::NullTest;
use super::takes::TakeKind;
use super::trials::{AbxListen, AbxRun, RatingRun, Stimulus};
use super::ui::{section, ACCENT, BAD, GOOD, WARN};
use super::{bless, rng::Rng, Live};

type Status = Option<(String, bool)>;

/// One revealed rating result.
struct ResultRow {
    label: String,
    /// The candidate, for a candidate stimulus.
    index: Option<usize>,
    mean: f64,
    ci95: Option<f64>,
    median: f64,
}

fn big(text: impl Into<String>, selected: bool) -> egui::Button<'static> {
    let mut t = RichText::new(text.into()).size(16.0).strong();
    if selected {
        t = t.color(Color32::BLACK);
    }
    let b = egui::Button::new(t).min_size(egui::vec2(120.0, 40.0));
    if selected {
        b.fill(ACCENT)
    } else {
        b
    }
}

impl Live {
    fn candidate_count(&self) -> usize {
        self.prepared.candidates.len()
    }

    fn take_of(&self, index: usize) -> usize {
        self.prepared
            .takes
            .iter()
            .position(|t| t.kind == TakeKind::Candidate { index })
            .unwrap_or(index)
    }

    fn bless_candidate(&mut self, index: usize, dir: &Path) -> Status {
        let notes = self.pref_notes.clone();
        match bless::bless(&self.prepared, index, &self.meta.id, &notes, &log::blessed_dir(dir)) {
            Ok(path) => {
                let label = self.prepared.candidates[index].label.clone();
                let status = super::ui::blessed_status(&label, &path);
                self.history.blessed.push(BlessedRef {
                    at: log::now_iso(),
                    candidate: index,
                    label,
                    record: path,
                });
                let mut s = None;
                self.save(dir, &mut s);
                s.or(Some((status, false)))
            }
            Err(e) => Some((format!("bless failed: {}", crate::errors::with_causes(&e)), true)),
        }
    }

    // ---- sighted -------------------------------------------------------

    pub(super) fn sighted_ui(&mut self, ui: &mut egui::Ui, dir: &Path) -> Status {
        let mut status = None;
        let selected = self.transport.selected();
        ui.label(
            RichText::new("Sighted: you know what you hear. Use it to find a sound, then confirm it blind.")
                .weak(),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for i in 0..self.candidate_count() {
                let take = self.take_of(i);
                let label = format!("{}  {}", i + 1, self.prepared.candidates[i].label);
                if ui.add(big(label, selected == take)).clicked() {
                    self.select(take);
                }
            }
        });
        ui.label(RichText::new("1–9 select · A/B the first two · space plays").weak().small());
        ui.add_space(8.0);
        let current = (0..self.candidate_count()).find(|&i| self.take_of(i) == selected);
        if let Some(i) = current {
            let label = self.prepared.candidates[i].label.clone();
            let can_bless = !matches!(self.prepared.candidates[i].source, super::candidate::Source::Dry);
            let (mut prefer, mut bless) = (false, false);
            section(ui, "Verdict", |ui| {
                ui.horizontal(|ui| {
                    ui.label("notes");
                    ui.add(egui::TextEdit::singleline(&mut self.pref_notes).desired_width(360.0));
                });
                ui.horizontal(|ui| {
                    prefer = ui.button(format!("Prefer “{label}”")).clicked();
                    bless = ui
                        .add_enabled(can_bless, egui::Button::new(format!("Bless “{label}”")))
                        .on_hover_text("pin its settings, material and take so a later change shows up")
                        .clicked();
                });
            });
            if prefer {
                self.history.preferences.push(Preference {
                    at: log::now_iso(),
                    candidate: i,
                    label: label.clone(),
                    sighted: true,
                    notes: std::mem::take(&mut self.pref_notes),
                });
                self.save(dir, &mut status);
                status = status.or(Some((format!("preferred {label} (sighted)"), false)));
            }
            if bless {
                status = self.bless_candidate(i, dir);
            }
        }
        if !self.history.preferences.is_empty() || !self.history.blessed.is_empty() {
            ui.add_space(6.0);
            for p in &self.history.preferences {
                let how = if p.sighted { "sighted" } else { "blind" };
                ui.label(RichText::new(format!("preferred ({how}) {}  {}", p.label, p.notes)).weak());
            }
            for b in &self.history.blessed {
                ui.label(RichText::new(format!("blessed {} → {}", b.label, b.record.display())).color(GOOD));
            }
        }
        status
    }

    // ---- ABX -----------------------------------------------------------

    pub(super) fn abx_ui(&mut self, ui: &mut egui::Ui, dir: &Path) -> Status {
        let mut status = None;
        let running = self.abx.as_ref().is_some_and(|r| !r.finished());
        if !running {
            self.abx_setup(ui);
        }
        let mut listen = None;
        let mut answer = None;
        let mut next = false;
        let mut new_run = false;
        if let Some(run) = &self.abx {
            ui.add_space(6.0);
            if !run.finished() {
                ui.label(RichText::new(format!("Trial {} of {}", run.trial_number(), run.planned)).size(18.0).strong());
                ui.horizontal(|ui| {
                    for (w, name) in [(AbxListen::A, "A"), (AbxListen::B, "B"), (AbxListen::X, "X")] {
                        if ui.add(big(name, run.listening == w)).clicked() {
                            listen = Some(w);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("X is");
                    if ui.add(big("A", run.pending == Some(true))).clicked() {
                        answer = Some(true);
                    }
                    if ui.add(big("B", run.pending == Some(false))).clicked() {
                        answer = Some(false);
                    }
                    let b = egui::Button::new(RichText::new("Next trial  ⏎").strong()).min_size(egui::vec2(130.0, 40.0));
                    next = ui.add_enabled(run.pending.is_some() || answer.is_some(), b).clicked();
                });
                ui.label(RichText::new("A B X listen · ⇧A ⇧B or ← → answer · ⏎ next").weak().small());
            }
            ui.add_space(6.0);
            ui.checkbox(&mut self.show_score, "show the running score")
                .on_hover_text("watching the score is feedback; hide it to avoid steering");
            if self.show_score || run.finished() {
                abx_stats(ui, run);
            }
            new_run = run.finished() && ui.button("New ABX run").clicked();
        }
        if let Some(run) = &mut self.abx {
            if let Some(w) = listen {
                let take = run.listen(w);
                self.select(take);
            }
        }
        if let (Some(a), Some(run)) = (answer, &mut self.abx) {
            run.answer(a);
        }
        if next {
            status = self.confirm_abx(dir);
        }
        if new_run {
            self.abx = None;
        }
        for rec in self.history.abx.iter().rev().filter(|r| self.abx.as_ref().is_none_or(|a| a.seed != r.seed)) {
            ui.label(
                RichText::new(format!(
                    "earlier: {} vs {}: {}/{} → {}",
                    rec.a,
                    rec.b,
                    rec.stats.correct,
                    rec.stats.trials,
                    rec.stats.verdict.describe()
                ))
                .weak(),
            );
        }
        status
    }

    fn abx_setup(&mut self, ui: &mut egui::Ui) {
        let n = self.candidate_count();
        let label = |i: usize| self.prepared.candidates[i].label.clone();
        let (mut a, mut b) = self.abx_pair;
        ui.horizontal(|ui| {
            ui.label("A");
            egui::ComboBox::from_id_source("abx_a").selected_text(label(a)).show_ui(ui, |ui| {
                for i in 0..n {
                    ui.selectable_value(&mut a, i, label(i));
                }
            });
            ui.label("B");
            egui::ComboBox::from_id_source("abx_b").selected_text(label(b)).show_ui(ui, |ui| {
                for i in 0..n {
                    ui.selectable_value(&mut b, i, label(i));
                }
            });
        });
        self.abx_pair = (a, b);
        let planned = self.abx_trials;
        let power = Abx::power(planned, ALPHA, ASSUMED_HIT_RATE);
        ui.horizontal(|ui| {
            let start = egui::Button::new(RichText::new(format!("Start ABX · {planned} trials")).strong())
                .min_size(egui::vec2(170.0, 30.0));
            if ui.add_enabled(a != b, start).clicked() {
                let pair = (self.take_of(a), self.take_of(b));
                let run = AbxRun::new(pair, planned, Rng::seed_from_clock());
                self.transport.select(pair.0);
                self.abx = Some(run);
            }
            let c = if power >= 0.8 { GOOD } else { WARN };
            ui.label(
                RichText::new(format!(
                    "{} needed · {:.0} % power at a {ASSUMED_HIT_RATE} hit rate",
                    Abx::critical(planned, ALPHA).map_or("never".into(), |k| format!("{k}/{planned}")),
                    power * 100.0
                ))
                .color(c),
            );
        });
        if a == b {
            ui.label(RichText::new("pick two different candidates").color(WARN));
        }
    }

    pub(super) fn confirm_abx(&mut self, dir: &Path) -> Status {
        let run = self.abx.as_mut()?;
        if !run.confirm() {
            return None;
        }
        let done = run.finished();
        let take = run.listen(AbxListen::A);
        self.transport.select(take);
        if self.restart_on_switch {
            self.transport.seek(self.transport.loop_region().0);
        }
        let run = self.abx.as_ref()?;
        self.history.record_abx(&self.prepared, run);
        let verdict = run.stats().verdict;
        let mut status = None;
        self.save(dir, &mut status);
        if done {
            status = status.or(Some((format!("ABX done: {}", verdict.describe()), false)));
        }
        status
    }

    // ---- rating --------------------------------------------------------

    pub(super) fn rating_stimuli(&self) -> (Vec<Stimulus>, Option<usize>) {
        let reference = self.prepared.reference;
        let mut s = Vec::new();
        for i in 0..self.candidate_count() {
            if Some(i) != reference {
                s.push(Stimulus::Candidate { index: i, take: self.take_of(i) });
            }
        }
        let open = reference.map(|r| self.take_of(r));
        if let Some(t) = open {
            s.push(Stimulus::HiddenReference { take: t });
        }
        if let Some(a) = self.prepared.anchor {
            s.push(Stimulus::Anchor { take: a });
        }
        (s, open)
    }

    pub(super) fn rating_ui(&mut self, ui: &mut egui::Ui, dir: &Path) -> Status {
        let mut status = None;
        let running = self.rating.as_ref().is_some_and(|r| !r.finished());
        if !running {
            let (stimuli, open) = self.rating_stimuli();
            let parts = [
                Some(format!("{} candidate(s)", stimuli.iter().filter(|s| matches!(s, Stimulus::Candidate { .. })).count())),
                open.map(|_| "the hidden reference".to_string()),
                self.prepared.anchor.map(|_| "the low anchor".to_string()),
            ];
            let parts: Vec<String> = parts.into_iter().flatten().collect();
            ui.label(format!("Rate {} on one screen, in a fresh random order each trial, 0–100.", parts.join(", ")));
            if open.is_none() {
                ui.label(RichText::new("no reference marked: this is a preference rating (mark one in the setup for MUSHRA proper)").weak());
            }
            let planned = self.rating_trials;
            let start = egui::Button::new(RichText::new(format!("Start rating · {planned} trial(s)")).strong())
                .min_size(egui::vec2(180.0, 30.0));
            if ui.add_enabled(stimuli.len() >= 2, start).clicked() {
                let run = RatingRun::new(stimuli, open, planned, Rng::seed_from_clock());
                if let Some(t) = run.take_at(0) {
                    self.transport.select(t);
                }
                self.rating_focus = 0;
                self.rating = Some(run);
            }
        }
        let playing = self.transport.selected();
        let focus = self.rating_focus;
        let mut select = None;
        let mut confirm = false;
        if let Some(run) = &mut self.rating {
            if !run.finished() {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Trial {} of {}", run.trials.len() + 1, run.planned)).size(18.0).strong());
                if let Some(r) = run.reference {
                    if ui.add(big("Reference  (R)", playing == r && focus == usize::MAX)).clicked() {
                        select = Some((r, usize::MAX));
                    }
                }
                ui.label(RichText::new("bad 0–20 · poor 20–40 · fair 40–60 · good 60–80 · excellent 80–100").weak().small());
                for slot in 0..run.order.len() {
                    ui.horizontal(|ui| {
                        let take = run.take_at(slot).unwrap_or(0);
                        let on = playing == take && focus == slot;
                        if ui.add(big(format!("{}", slot + 1), on).min_size(egui::vec2(48.0, 32.0))).clicked() {
                            select = Some((take, slot));
                        }
                        ui.spacing_mut().slider_width = (ui.available_width() - 80.0).max(200.0);
                        ui.add(egui::Slider::new(&mut run.sliders[slot], 0.0..=100.0).step_by(1.0).show_value(true));
                    });
                }
                if run.missing_top_rating() {
                    ui.label(RichText::new("one stimulus is the hidden reference: rate the one you think it is 100").color(WARN));
                }
                let b = egui::Button::new(RichText::new("Next trial  ⏎").strong()).min_size(egui::vec2(130.0, 32.0));
                if ui.add(b).clicked() {
                    confirm = true;
                }
                ui.label(RichText::new("1–9 play a slot · R reference · ↑↓ move its slider (⇧ by 10) · ⏎ next").weak().small());
            }
        }
        if let Some((take, slot)) = select {
            self.rating_focus = slot;
            self.select(take);
        }
        if confirm {
            status = self.confirm_rating(dir);
        }
        if let Some(run) = &self.rating {
            if run.finished() {
                status = status.or(self.rating_results(ui, dir));
            }
        }
        status
    }

    fn rating_results(&mut self, ui: &mut egui::Ui, dir: &Path) -> Status {
        let run = self.rating.as_ref()?;
        let summary = run.summary();
        let mut rows: Vec<ResultRow> = summary
            .per_stimulus
            .iter()
            .filter_map(|(s, sum)| {
                let sum = sum.as_ref()?;
                let index = match s {
                    Stimulus::Candidate { index, .. } => Some(*index),
                    _ => None,
                };
                Some(ResultRow {
                    label: log::stimulus_label(&self.prepared, s),
                    index,
                    mean: sum.mean,
                    ci95: sum.ci95,
                    median: sum.median,
                })
            })
            .collect();
        rows.sort_by(|a, b| b.mean.total_cmp(&a.mean));
        ui.add_space(6.0);
        section(ui, "Results (revealed)", |ui| {
            egui::Grid::new("rating_results").striped(true).show(ui, |ui| {
                for h in ["stimulus", "mean", "95 % CI", "median"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for r in &rows {
                    ui.label(&r.label);
                    ui.label(format!("{:.1}", r.mean));
                    ui.label(r.ci95.map_or("—".into(), |c| format!("± {c:.1}")));
                    ui.label(format!("{:.1}", r.median));
                    ui.end_row();
                }
            });
            if !summary.hidden_reference_ratings.is_empty() && !summary.passes_screening {
                ui.label(RichText::new("the hidden reference scored below 90 on more than 15 % of trials: BS.1534-3 would exclude this run").color(BAD));
            }
        });
        let top = rows.iter().find_map(|r| r.index.map(|i| (i, r.label.clone())));
        let mut status = None;
        ui.horizontal(|ui| {
            if let Some((i, label)) = &top {
                if ui.button(format!("Bless top-rated “{label}”")).clicked() {
                    status = Some(*i);
                }
            }
            if ui.button("New rating run").clicked() {
                self.rating = None;
            }
        });
        status.and_then(|i| self.bless_candidate(i, dir))
    }

    pub(super) fn confirm_rating(&mut self, dir: &Path) -> Status {
        let run = self.rating.as_mut()?;
        if run.finished() {
            return None;
        }
        run.confirm();
        let done = run.finished();
        if let Some(t) = run.take_at(0) {
            self.rating_focus = 0;
            self.transport.select(t);
        }
        let run = self.rating.as_ref()?;
        self.history.record_rating(&self.prepared, run);
        let mut status = None;
        self.save(dir, &mut status);
        if done {
            status = status.or(Some(("rating done: results below".into(), false)));
        }
        status
    }

    // ---- null test -----------------------------------------------------

    /// Each candidate against the reference: the alignment, the NMR
    /// verdict, the residual to play, and the pair's ABX one click away.
    pub(super) fn null_ui(&mut self, ui: &mut egui::Ui) -> Status {
        ui.label(
            RichText::new(
                "Null test: each candidate against the reference, matched in level and latency. The residual is \
                 what is left; its NMR under the reference predicts whether it is heard (−10 dB or below, total and \
                 worst band). A prediction: confirm it blind.",
            )
            .weak(),
        );
        ui.add_space(4.0);
        let rows: Vec<(usize, NullTest)> = match &self.prepared.nulls {
            Ok(n) if n.is_empty() => {
                ui.label(RichText::new("nothing to null: add a candidate beside the reference").color(WARN));
                return None;
            }
            Ok(n) => n.iter().cloned().enumerate().collect(),
            Err(e) => {
                ui.label(RichText::new(format!("no null test: {e}")).color(WARN));
                return None;
            }
        };
        let selected = self.transport.selected();
        let rate = self.prepared.sample_rate;
        let mut pick = None;
        let mut abx = None;
        egui::Grid::new("voicing_nulls").striped(true).num_columns(7).show(ui, |ui| {
            for h in ["candidate vs reference", "aligned", "null NMR", "worst band", "residual", "verdict", "listen"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (j, n) in &rows {
                ui.label(format!("{}  vs  {}", n.a, n.b));
                ui.label(format!("{:+.3} dB, {:+.2} fr ({:+.1} µs)", n.gain_db, n.delay_samples, n.delay_samples / rate * 1e6));
                ui.label(format!("{:.1} dB", n.nmr_total_db));
                ui.label(format!("{:.1} dB @ {:.0} Hz", n.nmr_max_db, n.nmr_max_hz));
                ui.label(format!("{:.1} dB re ref", n.residual_db));
                ui.label(RichText::new(n.verdict()).strong().color(if n.inaudible { GOOD } else { BAD }));
                ui.horizontal(|ui| {
                    let (ta, tb) = (self.take_of(n.candidate), self.take_of(n.reference));
                    let (true_level, raised) = (self.residual_take(*j, false), self.residual_take(*j, true));
                    for (take, label, hover) in [
                        (ta, "A", "the candidate"),
                        (tb, "B", "the reference"),
                        (true_level, "A − B", "the residual at its true level: what the null leaves"),
                        (raised, "A − B raised", "the residual raised to the session's loudness: what it is made of"),
                    ] {
                        if ui.selectable_label(selected == take, label).on_hover_text(hover).clicked() {
                            pick = Some(take);
                        }
                    }
                    if ui.button("ABX").on_hover_text("blind ABX of this pair").clicked() {
                        abx = Some((n.candidate, n.reference, n.a.clone(), n.b.clone()));
                    }
                });
                ui.end_row();
            }
        });
        ui.label(RichText::new("space plays · the residual plays in the loop like any take").weak().small());
        if let Some(t) = pick {
            self.select(t);
        }
        if let Some((a, b, la, lb)) = abx {
            self.abx_pair = (a, b);
            self.abx = None;
            self.mode = super::Mode::Abx;
            self.transport.select(self.take_of(a));
            return Some((format!("ABX: {la} vs {lb} — press Start"), false));
        }
        None
    }
}

fn abx_stats(ui: &mut egui::Ui, run: &AbxRun) {
    let s = run.stats();
    ui.label(
        RichText::new(format!(
            "{}/{} correct · p = {:.4} · d′ = {:.2} · hit rate 95 % CI {:.2}–{:.2}",
            s.correct, s.trials, s.p_value, s.d_prime, s.hit_rate_ci95[0], s.hit_rate_ci95[1]
        ))
        .monospace(),
    );
    ui.label(
        RichText::new(format!(
            "plan: {} trials, {} for p < {ALPHA}, {:.0} % power at a {ASSUMED_HIT_RATE} hit rate",
            s.planned,
            s.critical.map_or("never".into(), |k| format!("{k}")),
            s.power * 100.0
        ))
        .weak(),
    );
    let (text, color) = match s.verdict {
        AbxVerdict::Incomplete { .. } => (s.verdict.describe(), ui.visuals().weak_text_color()),
        AbxVerdict::Detected { .. } => (s.verdict.describe(), GOOD),
        AbxVerdict::Inconclusive { .. } | AbxVerdict::NotDetected { .. } => (s.verdict.describe(), WARN),
    };
    ui.label(RichText::new(text).strong().color(color));
}
