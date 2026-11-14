//! The bottom panel's input: a compact piano strip (mouse + computer
//! keys) and a row of one-shot sample chips, so every view has
//! something to react to. Held notes are reconciled each frame into
//! on/off events for the audio thread; the computer keys play whether
//! or not the panel is open.

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use eframe::egui::{self, RichText};
use stave_host::note::NoteEvent;

use super::commands::{id, Invocation};
use super::HostApp;
use crate::samples::{self, Sample};
use crate::views;

const VELOCITY: f64 = 0.85;
/// The piano strip's height: compact, a DAW's docked keyboard.
const PIANO_HEIGHT: f32 = 64.0;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    Midi,
    Samples,
}

pub(super) struct Keyboard {
    held: HashSet<i16>,
    mouse: Option<i16>,
    pub tab: Tab,
    bank: Vec<Sample>,
}

impl Keyboard {
    pub fn new() -> Self {
        Self {
            held: HashSet::new(),
            mouse: None,
            tab: Tab::Midi,
            bank: samples::builtin_bank(48_000),
        }
    }

    /// Forget held notes (the plugin they were sent to is gone).
    pub fn release_all(&mut self) {
        self.held.clear();
        self.mouse = None;
    }

    /// The input that suits a plugin: an instrument (no inputs) is
    /// played from the keys, an effect is fed samples.
    pub fn tab_for(inputs: usize) -> Tab {
        if inputs == 0 {
            Tab::Midi
        } else {
            Tab::Samples
        }
    }
}

/// The events that move `held` to `now`: offs for released, ons for new.
pub(super) fn transitions(held: &HashSet<i16>, now: &HashSet<i16>) -> Vec<NoteEvent> {
    let mut offs: Vec<i16> = held.difference(now).copied().collect();
    let mut ons: Vec<i16> = now.difference(held).copied().collect();
    offs.sort_unstable();
    ons.sort_unstable();
    offs.into_iter()
        .map(|key| NoteEvent::off(key, 0.0))
        .chain(ons.into_iter().map(|key| NoteEvent::on(key, VELOCITY)))
        .collect()
}

impl HostApp {
    pub(super) fn push_note(&self, ev: NoteEvent) {
        if self.shared.events.push(ev).is_err() {
            self.shared
                .log
                .dropped_events
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Computer keys + the mouse note → events for what changed.
    pub(super) fn sync_held_notes(&mut self, ctx: &egui::Context) {
        let mut now = if ctx.wants_keyboard_input() {
            HashSet::new() // typing in a field plays nothing
        } else {
            views::piano::keyboard_held(ctx, self.project.octave)
        };
        now.extend(self.keyboard.mouse);
        for ev in transitions(&self.keyboard.held, &now) {
            self.push_note(ev);
        }
        self.keyboard.held = now;
    }

    /// Let go of every note (the Voicing view or the palette has the keys).
    pub(super) fn release_held_notes(&mut self) {
        for ev in transitions(&self.keyboard.held, &HashSet::new()) {
            self.push_note(ev);
        }
        self.keyboard.held.clear();
        self.keyboard.mouse = None;
    }

    pub(super) fn draw_keyboard_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let mut run = None;
        ui.horizontal(|ui| {
            for (tab, label, command) in [(Tab::Midi, "Keyboard", id::KEYBOARD), (Tab::Samples, "Samples", id::SAMPLES)] {
                if ui.selectable_label(self.keyboard.tab == tab, label).clicked() {
                    run = Some(Invocation::new(command));
                }
            }
            ui.separator();
            match self.keyboard.tab {
                Tab::Midi => {
                    if ui.small_button("−").on_hover_text("Octave down  (Z)").clicked() {
                        run = Some(Invocation::new(id::OCTAVE_DOWN));
                    }
                    ui.label(RichText::new(format!("octave {}", self.project.octave)).small());
                    if ui.small_button("+").on_hover_text("Octave up  (X)").clicked() {
                        run = Some(Invocation::new(id::OCTAVE_UP));
                    }
                    ui.separator();
                    let mut held: Vec<i16> = self.keyboard.held.iter().copied().collect();
                    held.sort_unstable();
                    let names: Vec<String> = held.into_iter().map(views::piano::note_name).collect();
                    let text = if names.is_empty() { "A–; play  ·  Z/X octave".to_string() } else { names.join(" ") };
                    ui.label(RichText::new(text).small().color(t.ink_soft));
                }
                Tab::Samples => {
                    ui.label(RichText::new("click a sample to play it through the plugin").small().color(t.ink_soft));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let b = ui.add(egui::Button::new("×").frame(false));
                if b.on_hover_text(self.tip("Hide the panel", id::PANEL, ui.ctx())).clicked() {
                    run = Some(Invocation::new(id::PANEL));
                }
            });
        });
        match self.keyboard.tab {
            Tab::Midi => {
                self.keyboard.mouse =
                    views::piano::draw(ui, self.project.octave, &self.keyboard.held, PIANO_HEIGHT, t.accent);
            }
            Tab::Samples => {
                self.keyboard.mouse = None; // leaving the piano releases its note
                self.draw_sample_chips(ui);
            }
        }
        if let Some(inv) = run {
            self.run(&inv);
        }
    }

    /// One row of chips (scrolls sideways when they don't fit).
    fn draw_sample_chips(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for s in &self.keyboard.bank {
                        let chip = egui::Button::new(format!("▶ {}", s.name)).min_size(egui::vec2(0.0, 26.0));
                        let about = format!("{} ms · {} ch · {} Hz", s.duration_ms(), s.channels, s.sample_rate);
                        if ui.add(chip).on_hover_text(about).clicked() && self.shared.samples.push(s.trigger()).is_err() {
                            self.shared.log.warn("sample queue full — trigger dropped");
                        }
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_host::note::NoteKind;

    fn set(xs: &[i16]) -> HashSet<i16> {
        xs.iter().copied().collect()
    }

    #[test]
    fn offs_before_ons_in_pitch_order() {
        let evs = transitions(&set(&[60, 64]), &set(&[64, 67, 62]));
        let got: Vec<(i16, NoteKind)> = evs.iter().map(|e| (e.key, e.kind)).collect();
        assert_eq!(
            got,
            [(60, NoteKind::Off), (62, NoteKind::On), (67, NoteKind::On)]
        );
    }

    #[test]
    fn no_change_no_events() {
        assert!(transitions(&set(&[1, 2]), &set(&[2, 1])).is_empty());
    }

    #[test]
    fn instruments_get_the_keys_effects_get_samples() {
        assert_eq!(Keyboard::tab_for(0), Tab::Midi);
        assert_eq!(Keyboard::tab_for(2), Tab::Samples);
    }
}
