//! The default screen: one big button per source — Dry (the unprocessed
//! input, the control) then A, B, … — switching instantly at the same
//! playhead; play/stop and a seek bar; "Which sounds better?"; and a
//! guided blind test. Plain words only: the measurement vocabulary
//! (loudness units, rating scales, nulls) lives under Advanced.
//!
//! Keys (whenever no text field has focus):
//!
//! ```text
//! space        play / stop
//! D  0         Dry
//! 1–9          A, B, C, …   (and the letter keys A, B, C)
//! blind test:  1 / A  hear A · 2 / B  hear B · 3 / X  the mystery sound
//!              ⇧A ⇧B or ← →  answer
//! ```

use std::path::Path;

use eframe::egui::{self, Align2, Color32, FontId, Key, Modifiers, RichText, Sense, Stroke};
use stave_measure::listening::AbxVerdict;

use super::candidate::Source;
use super::log::{self, Choice, Preference};
use super::takes::Prepared;
use super::trials::{AbxListen, AbxRun};
use super::ui::{take_key, ACCENT, GOOD, PANEL, WARN};
use super::{rng::Rng, Live};

type Status = Option<(String, bool)>;

/// The Dry candidate and the others in letter order (the reference
/// first, then the rest as given).
pub(super) fn sources(p: &Prepared) -> (usize, Vec<usize>) {
    let dry = p
        .candidates
        .iter()
        .position(|c| matches!(c.source, Source::Dry))
        .unwrap_or(0);
    let mut letters: Vec<usize> = (0..p.candidates.len()).filter(|&i| i != dry).collect();
    if let Some(r) = p.reference.filter(|&r| r != dry) {
        letters.retain(|&i| i != r);
        letters.insert(0, r);
    }
    (dry, letters)
}

fn letter(i: usize) -> String {
    char::from(b'A' + i.min(25) as u8).to_string()
}

/// The guided blind test.
pub(super) struct Blind {
    /// The two candidates compared.
    pub pair: (usize, usize),
    pub trials: u32,
    /// The run, once started.
    pub run: Option<AbxRun>,
}

/// What the default screen asks of the view around it.
#[derive(Default)]
pub(super) struct Asks {
    pub status: Status,
    /// "Try another clip".
    pub next_material: bool,
}

/// The letter keys, in letter order (D is Dry's).
const LETTER_KEYS: [Key; 3] = [Key::A, Key::B, Key::C];
const DIGITS: [Key; 9] = [
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

impl Live {
    /// "Dry", "A", "B", … for a candidate.
    pub(super) fn name(&self, c: usize) -> String {
        if c == self.dry {
            return "Dry".into();
        }
        self.letters.iter().position(|&l| l == c).map_or_else(|| format!("{}", c + 1), letter)
    }

    /// What is being compared, in plain words.
    pub(super) fn title(&self) -> String {
        let labels: Vec<&str> = self
            .letters
            .iter()
            .map(|&c| self.prepared.candidates[c].label.as_str())
            .collect();
        if labels.len() == 1 {
            format!("{} vs dry", labels[0])
        } else {
            labels.join(" vs ")
        }
    }

    /// Every playable source in button order: Dry, then the letters.
    pub(super) fn buttons(&self) -> Vec<usize> {
        std::iter::once(self.dry).chain(self.letters.iter().copied()).collect()
    }

    /// Play candidate `c` from where the playhead is.
    pub(super) fn pick(&mut self, c: usize) {
        let take = self.take_of(c);
        self.select(take);
    }

    /// The candidate playing, if a candidate (not an anchor or residual).
    pub(super) fn playing_candidate(&self) -> Option<usize> {
        let t = self.transport.selected();
        (0..self.prepared.candidates.len()).find(|&c| self.take_of(c) == t)
    }

    /// Log an answer to "Which sounds better?".
    pub(super) fn prefer(&mut self, choice: Choice, candidate: Option<usize>, dir: &Path) -> Status {
        let label = match candidate {
            Some(c) => self.prepared.candidates[c].label.clone(),
            None => choice.word().into(),
        };
        self.history.preferences.push(Preference {
            at: log::now_iso(),
            choice,
            candidate,
            label: label.clone(),
            material: self.prepared.material.name.clone(),
            sighted: true,
            notes: std::mem::take(&mut self.pref_notes),
        });
        self.unclear = (choice == Choice::Unclear).then(|| self.history.preferences.len() - 1);
        let mut status = None;
        self.save(dir, &mut status);
        status.or(Some((
            match choice {
                Choice::Tie => "saved: a tie".into(),
                Choice::Unclear => "saved: unclear".into(),
                _ => format!("saved: {} sounds better", self.name(candidate.unwrap_or(0))),
            },
            false,
        )))
    }

    // ---- keys ----------------------------------------------------------

    /// The default screen's keys (space is the view's).
    pub(super) fn listen_keys(&mut self, ctx: &egui::Context, dir: &Path) -> Status {
        if self.blind.as_ref().is_some_and(|b| b.run.is_some()) {
            return self.blind_keys(ctx, dir);
        }
        if self.blind.is_some() {
            return None;
        }
        let mut pick = None;
        if take_key(ctx, Modifiers::NONE, Key::D) || take_key(ctx, Modifiers::NONE, Key::Num0) {
            pick = Some(self.dry);
        }
        for (i, k) in DIGITS.iter().enumerate() {
            if take_key(ctx, Modifiers::NONE, *k) {
                pick = self.letters.get(i).copied().or(pick);
            }
        }
        for (i, k) in LETTER_KEYS.iter().enumerate() {
            if take_key(ctx, Modifiers::NONE, *k) {
                pick = self.letters.get(i).copied().or(pick);
            }
        }
        if let Some(c) = pick {
            self.pick(c);
        }
        None
    }

    fn blind_keys(&mut self, ctx: &egui::Context, dir: &Path) -> Status {
        let mut answer = None;
        if take_key(ctx, Modifiers::SHIFT, Key::A) || take_key(ctx, Modifiers::NONE, Key::ArrowLeft) {
            answer = Some(true);
        }
        if take_key(ctx, Modifiers::SHIFT, Key::B) || take_key(ctx, Modifiers::NONE, Key::ArrowRight) {
            answer = Some(false);
        }
        let mut listen = None;
        if take_key(ctx, Modifiers::NONE, Key::A) || take_key(ctx, Modifiers::NONE, Key::Num1) {
            listen = Some(AbxListen::A);
        }
        if take_key(ctx, Modifiers::NONE, Key::B) || take_key(ctx, Modifiers::NONE, Key::Num2) {
            listen = Some(AbxListen::B);
        }
        if take_key(ctx, Modifiers::NONE, Key::X) || take_key(ctx, Modifiers::NONE, Key::Num3) {
            listen = Some(AbxListen::X);
        }
        if let Some(w) = listen {
            self.blind_listen(w);
        }
        answer.and_then(|a| self.blind_answer(a, dir))
    }

    fn blind_listen(&mut self, w: AbxListen) {
        let Some(run) = self.blind.as_mut().and_then(|b| b.run.as_mut()) else { return };
        if run.finished() {
            return;
        }
        let take = run.listen(w);
        self.select(take);
    }

    /// Answer "the mystery sound is A" (`true`) or B, and move on: the
    /// next mystery sound plays at once.
    pub(super) fn blind_answer(&mut self, is_a: bool, dir: &Path) -> Status {
        let run = self.blind.as_mut()?.run.as_mut()?;
        run.answer(is_a);
        if !run.confirm() {
            return None;
        }
        let done = run.finished();
        let take = if done { run.take_for(AbxListen::A) } else { run.listen(AbxListen::X) };
        self.transport.select(take);
        let run = self.blind.as_ref()?.run.as_ref()?;
        self.history.record_abx(&self.prepared, run);
        let mut status = None;
        self.save(dir, &mut status);
        status
    }

    /// Open the blind test's first page.
    pub(super) fn open_blind(&mut self) {
        let pair = match self.letters.as_slice() {
            [a, b, ..] => (*a, *b),
            [a] => (self.dry, *a),
            [] => (self.dry, self.dry),
        };
        self.blind = Some(Blind {
            pair,
            trials: self.abx_trials,
            run: None,
        });
    }

    // ---- the screen ----------------------------------------------------

    /// Everything below the title.
    pub(super) fn listen_ui(&mut self, ui: &mut egui::Ui, dir: &Path) -> Asks {
        if self.blind.is_some() {
            return self.blind_ui(ui, dir);
        }
        let mut asks = Asks::default();
        self.source_buttons(ui);
        ui.add_space(14.0);
        self.transport_bar(ui);
        ui.add_space(22.0);
        self.question(ui, dir, &mut asks);
        ui.add_space(22.0);
        if ui
            .add(egui::Button::new(RichText::new("Blind test…").size(15.0)).min_size(egui::vec2(150.0, 34.0)))
            .on_hover_text("hear a mystery sound and say which it is, without knowing")
            .clicked()
        {
            self.open_blind();
        }
        asks
    }

    fn source_buttons(&mut self, ui: &mut egui::Ui) {
        let buttons = self.buttons();
        let playing = self.playing_candidate();
        let gap = 12.0;
        let n = buttons.len() as f32;
        let w = ((ui.available_width() - gap * (n - 1.0)) / n).clamp(110.0, 220.0);
        let mut pick = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (i, &c) in buttons.iter().enumerate() {
                let (big, sub, key) = if c == self.dry {
                    ("Dry".to_string(), "unprocessed".to_string(), "D".to_string())
                } else {
                    (self.name(c), self.prepared.candidates[c].label.clone(), format!("{i}"))
                };
                if source_button(ui, &big, &sub, &key, playing == Some(c), egui::vec2(w, 92.0)).clicked() {
                    pick = Some(c);
                }
            }
        });
        if let Some(c) = pick {
            self.pick(c);
        }
    }

    /// Play/stop and a seek bar over the whole clip (it loops).
    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.transport.clone();
        let rate = self.prepared.sample_rate;
        ui.horizontal(|ui| {
            let playing = t.is_playing();
            let label = if playing { "■  Stop" } else { "▶  Play" };
            let b = egui::Button::new(RichText::new(label).size(20.0).strong()).min_size(egui::vec2(130.0, 52.0));
            if ui.add(b).on_hover_text("space").clicked() {
                t.set_playing(!playing);
            }
            ui.add_space(8.0);
            let time = format!("{:.1} / {:.1} s", t.position() as f64 / rate, t.frames() as f64 / rate);
            let width = (ui.available_width() - 110.0).max(120.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 52.0), Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 6.0, PANEL);
            let mid = rect.center().y;
            let h = rect.height() * 0.42;
            let n = self.overview.len().max(1) as f32;
            for (i, (lo, hi)) in self.overview.iter().enumerate() {
                let x = rect.left() + rect.width() * i as f32 / n;
                painter.line_segment(
                    [egui::pos2(x, mid - hi * 2.0 * h), egui::pos2(x, mid - lo * 2.0 * h)],
                    Stroke::new(1.0, Color32::from_gray(90)),
                );
            }
            let frames = t.frames().max(1) as f32;
            let px = rect.left() + rect.width() * t.position() as f32 / frames;
            painter.rect_filled(
                egui::Rect::from_x_y_ranges(rect.left()..=px, rect.y_range()),
                6.0,
                Color32::from_rgba_unmultiplied(140, 200, 255, 40),
            );
            painter.line_segment([egui::pos2(px, rect.top()), egui::pos2(px, rect.bottom())], Stroke::new(2.0, ACCENT));
            if resp.clicked() || resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let f = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * frames;
                    t.seek(f as u64);
                }
            }
            ui.label(RichText::new(time).monospace());
        });
        ui.ctx().request_repaint();
    }

    fn question(&mut self, ui: &mut egui::Ui, dir: &Path, asks: &mut Asks) {
        ui.label(RichText::new("Which sounds better?").size(20.0).strong());
        ui.add_space(6.0);
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            for (i, &c) in self.letters.iter().enumerate() {
                let choice = match i {
                    0 => Choice::A,
                    1 => Choice::B,
                    _ => Choice::Other,
                };
                if ui.add(answer_button(&letter(i))).on_hover_text(&self.prepared.candidates[c].label).clicked() {
                    chosen = Some((choice, Some(c)));
                }
            }
            if ui
                .add(answer_button("Tie"))
                .on_hover_text("heard both: equally good, or no real difference")
                .clicked()
            {
                chosen = Some((Choice::Tie, None));
            }
            if ui
                .add(answer_button("Unclear"))
                .on_hover_text("not enough to go on: this clip or level does not show the difference")
                .clicked()
            {
                chosen = Some((Choice::Unclear, None));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Notes (optional)");
            ui.add(egui::TextEdit::singleline(&mut self.pref_notes).desired_width(360.0));
        });
        if let Some((choice, c)) = chosen {
            asks.status = self.prefer(choice, c, dir);
        }
        // After "Unclear": another clip in one click, and why.
        if let Some(i) = self.unclear.filter(|&i| i < self.history.preferences.len()) {
            ui.add_space(6.0);
            egui::Frame::none().fill(PANEL).rounding(6.0).inner_margin(10.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Not enough to go on?");
                    if ui
                        .add(egui::Button::new(RichText::new("Try another clip  →").strong()).min_size(egui::vec2(160.0, 30.0)))
                        .clicked()
                    {
                        asks.next_material = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Why (optional)");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.history.preferences[i].notes)
                            .hint_text("e.g. the riff never gets loud enough")
                            .desired_width(320.0),
                    );
                    if r.lost_focus() {
                        let mut s = None;
                        self.save(dir, &mut s);
                        asks.status = s.or(asks.status.take());
                    }
                });
            });
        }
        let n = self.history.preferences.len();
        if n > 0 {
            let last = &self.history.preferences[n - 1];
            let said = match last.candidate {
                Some(c) => format!("{} ({})", self.name(c), last.label),
                None => last.label.clone(),
            };
            ui.label(RichText::new(format!("{n} answer(s) saved · last: {said}")).weak());
        }
    }

    // ---- blind test ----------------------------------------------------

    fn blind_ui(&mut self, ui: &mut egui::Ui, dir: &Path) -> Asks {
        let mut asks = Asks::default();
        let Some(b) = &self.blind else { return asks };
        let (a, bb) = b.pair;
        let (na, nb) = (self.name(a), self.name(bb));
        let mut close = false;
        match &b.run {
            None => {
                ui.label(RichText::new("Blind test").size(22.0).strong());
                ui.label("We'll play a mystery sound. You tell us which one it is. We keep score.");
                ui.add_space(10.0);
                ui.label(RichText::new("What do you want to test?").size(16.0));
                let mut pairs: Vec<(usize, usize)> = self.letters.iter().map(|&l| (self.dry, l)).collect();
                for i in 0..self.letters.len() {
                    for j in i + 1..self.letters.len() {
                        pairs.push((self.letters[i], self.letters[j]));
                    }
                }
                let mut pair = b.pair;
                let mut trials = b.trials;
                ui.horizontal_wrapped(|ui| {
                    for p in pairs {
                        let text = format!("{} vs {}", self.name(p.0), self.name(p.1));
                        let on = pair == p;
                        let mut rt = RichText::new(text).size(16.0).strong();
                        if on {
                            rt = rt.color(Color32::BLACK);
                        }
                        let mut btn = egui::Button::new(rt).min_size(egui::vec2(130.0, 44.0));
                        if on {
                            btn = btn.fill(ACCENT);
                        }
                        if ui.add(btn).clicked() {
                            pair = p;
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Rounds");
                    ui.add(egui::DragValue::new(&mut trials).clamp_range(4..=100));
                    ui.label(RichText::new("more rounds can find smaller differences").weak());
                });
                ui.add_space(10.0);
                let mut start = false;
                ui.horizontal(|ui| {
                    let s = egui::Button::new(RichText::new(format!("Start  ·  {trials} rounds")).size(18.0).strong())
                        .min_size(egui::vec2(200.0, 48.0));
                    start = ui.add_enabled(pair.0 != pair.1, s).clicked();
                    close = ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 48.0))).clicked();
                });
                let b = self.blind.as_mut().expect("drawn above");
                b.pair = pair;
                b.trials = trials;
                if start {
                    self.start_blind();
                }
            }
            Some(run) if !run.finished() => {
                let (round, planned, pending_x) = (run.trial_number(), run.planned, run.listening);
                ui.label(RichText::new(format!("Round {round} of {planned}")).size(22.0).strong());
                ui.label(format!(
                    "The mystery sound is playing. Is it {na} or {nb}? Listen to {na} and {nb} as often as you like."
                ));
                ui.add_space(10.0);
                let mut listen = None;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    for (w, text, key) in [
                        (AbxListen::A, format!("Hear {na}"), "1"),
                        (AbxListen::B, format!("Hear {nb}"), "2"),
                        (AbxListen::X, "Mystery".to_string(), "3"),
                    ] {
                        let sub = if w == AbxListen::X { "?" } else { "" };
                        if source_button(ui, &text, sub, key, pending_x == w, egui::vec2(170.0, 80.0)).clicked() {
                            listen = Some(w);
                        }
                    }
                });
                ui.add_space(8.0);
                self.transport_bar(ui);
                ui.add_space(12.0);
                ui.label(RichText::new("The mystery sound is…").size(18.0).strong());
                let mut answer = None;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if ui.add(answer_button(&na).min_size(egui::vec2(150.0, 56.0))).clicked() {
                        answer = Some(true);
                    }
                    if ui.add(answer_button(&nb).min_size(egui::vec2(150.0, 56.0))).clicked() {
                        answer = Some(false);
                    }
                });
                ui.label(RichText::new("keys: 1 2 3 listen · ← → answer · space play/stop").weak().small());
                ui.add_space(8.0);
                close = ui.button("Stop the test").clicked();
                if let Some(w) = listen {
                    self.blind_listen(w);
                }
                if let Some(x) = answer {
                    asks.status = self.blind_answer(x, dir);
                }
            }
            Some(run) => {
                let stats = run.stats();
                let (headline, detail, colour) = verdict_words(stats.verdict, stats.correct, stats.trials);
                ui.label(RichText::new(format!("{na} vs {nb}")).size(16.0).weak());
                ui.label(RichText::new(headline).size(24.0).strong().color(colour));
                ui.label(RichText::new(detail).size(15.0));
                ui.add_space(12.0);
                let mut again = false;
                ui.horizontal(|ui| {
                    again = ui
                        .add(egui::Button::new(RichText::new("Test again").size(16.0)).min_size(egui::vec2(140.0, 44.0)))
                        .clicked();
                    close = ui
                        .add(egui::Button::new(RichText::new("Back to listening").size(16.0).strong()).min_size(egui::vec2(180.0, 44.0)))
                        .clicked();
                });
                if again {
                    if let Some(b) = &mut self.blind {
                        b.run = None;
                    }
                }
            }
        }
        if close {
            if let Some(run) = self.blind.take().and_then(|b| b.run) {
                if !run.trials.is_empty() {
                    self.history.record_abx(&self.prepared, &run);
                    self.save(dir, &mut asks.status);
                }
            }
        }
        asks
    }

    /// Start the blind run: the first mystery sound plays.
    pub(super) fn start_blind(&mut self) {
        let Some(b) = &self.blind else { return };
        let pair = (self.take_of(b.pair.0), self.take_of(b.pair.1));
        let mut run = AbxRun::new(pair, b.trials, Rng::seed_from_clock());
        let x = run.listen(AbxListen::X);
        self.transport.select(x);
        self.transport.set_playing(true);
        if let Some(b) = &mut self.blind {
            b.run = Some(run);
        }
    }
}

/// The result in plain words: headline, one line more, its colour.
pub(super) fn verdict_words(v: AbxVerdict, correct: u32, trials: u32) -> (String, String, Color32) {
    let score = format!("{correct} of {trials} right");
    match v {
        AbxVerdict::Detected { p } => (
            "You could tell them apart.".into(),
            format!("{score}: {}.", chance_words(p)),
            GOOD,
        ),
        AbxVerdict::NotDetected { .. } => (
            "You couldn't reliably tell them apart.".into(),
            format!("{score}: about what guessing gets. Any difference is small."),
            WARN,
        ),
        AbxVerdict::Inconclusive { .. } => (
            "You couldn't reliably tell them apart.".into(),
            format!("{score}: about what guessing gets. A longer test could still find a small difference."),
            WARN,
        ),
        AbxVerdict::Incomplete { done, planned } => (
            "Not finished.".into(),
            format!("{done} of {planned} rounds done."),
            WARN,
        ),
    }
}

/// "very unlikely to be chance (about 1 in 480)".
fn chance_words(p: f64) -> String {
    let odds = (1.0 / p.max(1e-9)).round();
    let how = if p < 0.001 {
        "almost certainly not chance"
    } else if p < 0.01 {
        "very unlikely to be chance"
    } else {
        "unlikely to be chance"
    };
    format!("{how} (guessing does this well about 1 time in {odds:.0})")
}

fn answer_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).size(18.0).strong()).min_size(egui::vec2(110.0, 48.0))
}

/// A big source button: the name large, what it is small, its key in
/// the corner; filled when playing.
fn source_button(ui: &mut egui::Ui, big: &str, sub: &str, key: &str, on: bool, size: egui::Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let v = ui.visuals();
    let fill = if on {
        ACCENT
    } else if resp.hovered() {
        v.widgets.hovered.bg_fill
    } else {
        v.widgets.inactive.bg_fill
    };
    let fg = if on { Color32::BLACK } else { v.strong_text_color() };
    let p = ui.painter();
    p.rect_filled(rect, 10.0, fill);
    if on {
        p.rect_stroke(rect, 10.0, Stroke::new(2.0, Color32::WHITE));
    }
    let big_y = if sub.is_empty() { rect.center().y } else { rect.top() + rect.height() * 0.40 };
    p.text(egui::pos2(rect.center().x, big_y), Align2::CENTER_CENTER, big, FontId::proportional(30.0), fg);
    if !sub.is_empty() {
        let max = ((rect.width() - 16.0) / 7.0) as usize;
        let sub: String = if sub.chars().count() > max {
            sub.chars().take(max.saturating_sub(1)).chain(std::iter::once('…')).collect()
        } else {
            sub.into()
        };
        p.text(
            egui::pos2(rect.center().x, rect.bottom() - 18.0),
            Align2::CENTER_CENTER,
            sub,
            FontId::proportional(13.0),
            fg,
        );
    }
    p.text(
        rect.right_top() + egui::vec2(-8.0, 6.0),
        Align2::RIGHT_TOP,
        key,
        FontId::monospace(11.0),
        fg.gamma_multiply(0.55),
    );
    resp.on_hover_text(format!("key {key}"))
}
