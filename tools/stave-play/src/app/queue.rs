//! Listen mode's queue: auditions agents asked a person to hear
//! (`stave listen queue add`, see `stave_tools::listen`), listed in a
//! side bar. Clicking one loads its comparison into the Voicing view
//! with what to listen for above it; the answer given there ("Which
//! sounds better?": A, B, tie, unclear, and its notes) marks it done in
//! the queue file, where the agent reads it back.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use stave_tools::listen::{self, Answer, Item, Queue};

use super::panels::section;
use super::HostApp;
use crate::project::Mode;
use crate::voicing::{self, Voicing};

/// How often the file is re-read (agents add to it from outside).
const REFRESH: Duration = Duration::from_secs(2);

pub struct Auditions {
    pub path: PathBuf,
    pub queue: Queue,
    read_at: Option<Instant>,
    /// The audition loaded into the Voicing view.
    pub current: Option<String>,
    /// What was last written for it (answer, notes).
    pub recorded: Option<(Answer, String)>,
    pub error: Option<String>,
}

impl Auditions {
    pub fn new(path: PathBuf) -> Self {
        Self { path, queue: Queue::default(), read_at: None, current: None, recorded: None, error: None }
    }

    /// Re-read the file when it is due.
    pub fn refresh(&mut self) {
        if self.read_at.is_some_and(|t| t.elapsed() < REFRESH) {
            return;
        }
        self.read_at = Some(Instant::now());
        match Queue::load(&self.path) {
            Ok(q) => {
                self.queue = q;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
        }
    }

    pub fn current_item(&self) -> Option<&Item> {
        self.current.as_deref().and_then(|id| self.queue.get(id))
    }

    /// Write `answer` and `notes` for the current audition, unless that
    /// is what was last written. Whether it wrote.
    pub fn record(&mut self, answer: Answer, notes: &str) -> Result<bool, String> {
        let Some(id) = self.current.clone() else { return Ok(false) };
        if self.recorded.as_ref().is_some_and(|(a, n)| *a == answer && n == notes) {
            return Ok(false);
        }
        let q = listen::update(&self.path, |q| {
            q.answer(&id, answer, notes);
            q.clone()
        })
        .map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.queue = q;
        self.read_at = Some(Instant::now());
        self.recorded = Some((answer, notes.to_string()));
        Ok(true)
    }
}

/// The queue's answer for the Voicing view's ("other": a third
/// candidate's letter, which the queue doesn't name).
pub fn answer_of(word: &str) -> Option<Answer> {
    Answer::parse(word)
}

/// The Voicing view's launch for an audition (its log goes where the
/// view's own logs go).
pub fn launch_for(item: &Item, dir: PathBuf) -> voicing::Launch {
    voicing::Launch {
        candidates: item.candidates.clone(),
        material: item.material.clone(),
        dir: Some(dir),
        ..voicing::Launch::default()
    }
}

impl HostApp {
    /// Load `item` into the Voicing view and go there.
    pub(super) fn load_audition(&mut self, item: &Item) {
        self.sync_audition(true);
        let launch = launch_for(item, self.voicing.dir.clone());
        match Voicing::new(&launch) {
            Ok(v) => {
                self.voicing.release_device();
                self.voicing = v;
                self.auditions.current = Some(item.id.clone());
                self.auditions.recorded = None;
                self.open_mode(Mode::Listen);
                self.say(format!("{}: {}", item.id, item.title));
            }
            Err(e) => self.fail(format!("{}: {e}", item.id)),
        }
    }

    pub(super) fn next_audition(&mut self) {
        self.auditions.read_at_now();
        match self.auditions.queue.next().cloned() {
            Some(item) => self.load_audition(&item),
            None => self.say("no auditions waiting".into()),
        }
    }

    /// Copy the Voicing view's latest answer into the queue: a new
    /// answer at once; edited notes only when `settle` (leaving the
    /// audition), so typing doesn't write the file per keystroke.
    pub(super) fn sync_audition(&mut self, settle: bool) {
        if self.auditions.current.is_none() {
            return;
        }
        let Some((answer, notes)) = self.voicing.answers().pop().and_then(|(w, n)| answer_of(w).map(|a| (a, n))) else {
            return;
        };
        let changed_answer = self.auditions.recorded.as_ref().map(|(a, _)| *a) != Some(answer);
        if !(changed_answer || settle) {
            return;
        }
        match self.auditions.record(answer, &notes) {
            Ok(true) => {
                let left = self.auditions.queue.pending().count();
                self.say(format!("recorded {} · {left} waiting", answer.word()));
            }
            Ok(false) => {}
            Err(e) => self.fail(e),
        }
    }

    /// The queue side bar (Listen mode).
    pub(super) fn draw_queue(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        self.auditions.refresh();
        let pending: Vec<Item> = self.auditions.queue.pending().cloned().collect();
        let answered: Vec<Item> = self.auditions.queue.items.iter().filter(|i| !i.pending()).rev().cloned().collect();
        ui.label(RichText::new(format!("LISTENING QUEUE · {}", pending.len())).small().strong().color(t.ink_soft));
        if let Some(e) = &self.auditions.error {
            ui.label(RichText::new(e).small().color(t.bad));
        }
        let mut load = None;
        egui::ScrollArea::vertical().id_source("queue-scroll").auto_shrink([false, false]).show(ui, |ui| {
            if pending.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Nothing waiting. Agents queue auditions with `stave listen queue add`.")
                        .small()
                        .color(t.ink_soft),
                );
            }
            for item in &pending {
                let current = self.auditions.current.as_deref() == Some(item.id.as_str());
                let r = ui.selectable_label(current, RichText::new(format!("{}  {}", item.id, item.title)).strong());
                ui.label(RichText::new(&item.listen_for).small().color(t.ink_soft));
                if let Some(by) = &item.requested_by {
                    ui.label(RichText::new(format!("asked by {by}")).small().color(t.ink_soft));
                }
                ui.add_space(4.0);
                if r.clicked() {
                    load = Some(item.clone());
                }
            }
            if !answered.is_empty() {
                ui.add_space(8.0);
                section(ui, &t, &format!("Answered · {}", answered.len()));
                for item in answered.iter().take(30) {
                    let a = item.answer.map_or("", Answer::word);
                    ui.label(RichText::new(format!("{}  {}: {a}", item.id, item.title)).small());
                    if !item.notes.is_empty() {
                        ui.label(RichText::new(&item.notes).small().italics().color(t.ink_soft));
                    }
                }
            }
        });
        if let Some(item) = load {
            self.load_audition(&item);
        }
    }

    /// Above the Voicing view while an audition is loaded: what to
    /// listen for, and whether the answer has been recorded.
    pub(super) fn draw_audition_banner(&mut self, ui: &mut egui::Ui) {
        self.sync_audition(false);
        let t = self.tokens;
        let Some(item) = self.auditions.current_item().cloned() else { return };
        let waiting = self.auditions.queue.pending().filter(|i| i.id != item.id).count();
        let mut next = false;
        egui::Frame::none()
            .fill(t.accent.gamma_multiply(0.12))
            .rounding(6.0)
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&item.id).strong().color(t.accent));
                    ui.label(RichText::new("Listen for:").color(t.ink_soft));
                    ui.label(RichText::new(&item.listen_for).strong());
                });
                ui.horizontal(|ui| {
                    match &self.auditions.recorded {
                        Some((a, _)) => {
                            ui.label(RichText::new(format!("Recorded: {}", a.word())).small().strong().color(t.good));
                        }
                        None => {
                            ui.label(
                                RichText::new("Answer \"Which sounds better?\" below: it goes back to whoever asked.")
                                    .small()
                                    .color(t.ink_soft),
                            );
                        }
                    }
                    if waiting > 0 && ui.small_button(format!("Next audition ({waiting} waiting)")).clicked() {
                        next = true;
                    }
                });
            });
        ui.add_space(8.0);
        if next {
            self.next_other_audition();
        }
    }

    /// The oldest audition waiting that isn't the current one.
    fn next_other_audition(&mut self) {
        let current = self.auditions.current.clone();
        let item = self.auditions.queue.pending().find(|i| Some(&i.id) != current.as_ref()).cloned();
        if let Some(item) = item {
            self.load_audition(&item);
        }
    }
}

impl Auditions {
    /// Force the next refresh to read the file.
    fn read_at_now(&mut self) {
        self.read_at = None;
        self.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_tools::listen::NewItem;

    #[test]
    fn answering_marks_done_and_offers_the_next() {
        let path = std::env::temp_dir().join(format!("stave-play-queue-{}", std::process::id())).join("q.json");
        listen::update(&path, |q| {
            for n in ["first", "second"] {
                q.add(NewItem { title: Some(n.into()), candidates: vec!["dry".into()], listen_for: "x".into(), ..NewItem::default() });
            }
        })
        .unwrap();
        let mut a = Auditions::new(path.clone());
        a.refresh();
        a.current = Some("L1".into());
        assert!(a.record(Answer::A, "brighter").unwrap());
        assert!(!a.record(Answer::A, "brighter").unwrap(), "unchanged: not rewritten");
        let next = a.queue.next().cloned().unwrap();
        assert_eq!(next.id, "L2");
        assert_eq!(answer_of("tie"), Some(Answer::Tie));
        assert_eq!(answer_of("other"), None);
        let q = Queue::load(&path).unwrap();
        assert_eq!((q.items[0].answer, q.items[0].notes.as_str()), (Some(Answer::A), "brighter"));
        let launch = launch_for(&next, PathBuf::from("/v"));
        assert!(launch.wants_voicing());
        assert_eq!(launch.dir, Some(PathBuf::from("/v")));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
