//! The input panel: a piano (mouse + computer keys) and one-shot sample
//! pads, so every analysis view has something to react to. Held notes
//! are reconciled each frame into on/off events for the audio thread.

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use eframe::egui::{self, Color32, RichText};
use stave_host::note::{NoteEvent, NoteKind};

use super::HostApp;
use crate::samples::{self, Sample};
use crate::views;

const VELOCITY: f32 = 0.85;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Tab {
    Midi,
    Samples,
}

pub(super) struct Keyboard {
    held: HashSet<i16>,
    mouse: Option<i16>,
    tab: Tab,
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
}

/// The events that move `held` to `now`: offs for released, ons for new.
pub(super) fn transitions(held: &HashSet<i16>, now: &HashSet<i16>) -> Vec<NoteEvent> {
    let mut offs: Vec<i16> = held.difference(now).copied().collect();
    let mut ons: Vec<i16> = now.difference(held).copied().collect();
    offs.sort_unstable();
    ons.sort_unstable();
    let event = |kind, velocity| {
        move |pitch| NoteEvent {
            offset: 0,
            pitch,
            velocity,
            kind,
        }
    };
    offs.into_iter()
        .map(event(NoteKind::Off, 0.0))
        .chain(ons.into_iter().map(event(NoteKind::On, VELOCITY)))
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

    /// z / x shift the piano's octave.
    pub(super) fn octave_shortcuts(&mut self, ctx: &egui::Context) {
        let (down, up) = ctx.input(|i| (i.key_pressed(egui::Key::Z), i.key_pressed(egui::Key::X)));
        if down || up {
            self.project.octave =
                (self.project.octave + i32::from(up) - i32::from(down)).clamp(0, 8);
            self.mark_dirty();
        }
    }

    /// Computer keys + the mouse note → events for what changed.
    pub(super) fn sync_held_notes(&mut self, ctx: &egui::Context) {
        let mut now = views::piano::keyboard_held(ctx, self.project.octave);
        now.extend(self.keyboard.mouse);
        for ev in transitions(&self.keyboard.held, &now) {
            self.push_note(ev);
        }
        self.keyboard.held = now;
    }

    pub(super) fn draw_keyboard_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let kb = &mut self.keyboard;
            if ui.selectable_label(kb.tab == Tab::Midi, "MIDI").clicked() {
                kb.tab = Tab::Midi;
            }
            if ui
                .selectable_label(kb.tab == Tab::Samples, "Samples")
                .clicked()
            {
                kb.tab = Tab::Samples;
            }
            ui.separator();
            match kb.tab {
                Tab::Midi => {
                    let mut octave = self.project.octave;
                    if ui.button("octave −").clicked() {
                        octave -= 1;
                    }
                    ui.label(format!("octave {}", self.project.octave));
                    if ui.button("octave +").clicked() {
                        octave += 1;
                    }
                    if octave != self.project.octave {
                        self.project.octave = octave.clamp(0, 8);
                        self.dirty = true;
                    }
                    ui.separator();
                    let mut names: Vec<String> = kb
                        .held
                        .iter()
                        .map(|&n| views::piano::note_name(n))
                        .collect();
                    names.sort();
                    ui.label(if names.is_empty() {
                        "—".into()
                    } else {
                        names.join(" ")
                    });
                }
                Tab::Samples => {
                    ui.label(format!("{} samples loaded", kb.bank.len()));
                    ui.separator();
                    ui.label(RichText::new("click to trigger one-shot").weak());
                }
            }
        });
        ui.add_space(4.0);
        match self.keyboard.tab {
            Tab::Midi => {
                self.keyboard.mouse =
                    views::piano::draw(ui, self.project.octave, &self.keyboard.held);
            }
            Tab::Samples => {
                self.keyboard.mouse = None; // leaving the piano releases its note
                self.draw_sample_pads(ui);
            }
        }
    }

    fn draw_sample_pads(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for s in &self.keyboard.bank {
                        let label = RichText::new(format!(
                            "▶  {}\n{} ms · {} ch · {} Hz",
                            s.name,
                            s.duration_ms(),
                            s.channels,
                            s.sample_rate
                        ))
                        .monospace();
                        let pad = egui::Button::new(label)
                            .min_size(egui::vec2(160.0, 56.0))
                            .fill(Color32::from_rgb(38, 60, 88));
                        if ui.add(pad).clicked() && self.shared.samples.push(s.trigger()).is_err() {
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

    fn set(xs: &[i16]) -> HashSet<i16> {
        xs.iter().copied().collect()
    }

    #[test]
    fn offs_before_ons_in_pitch_order() {
        let evs = transitions(&set(&[60, 64]), &set(&[64, 67, 62]));
        let got: Vec<(i16, NoteKind)> = evs.iter().map(|e| (e.pitch, e.kind)).collect();
        assert_eq!(
            got,
            [(60, NoteKind::Off), (62, NoteKind::On), (67, NoteKind::On)]
        );
    }

    #[test]
    fn no_change_no_events() {
        assert!(transitions(&set(&[1, 2]), &set(&[2, 1])).is_empty());
    }
}
