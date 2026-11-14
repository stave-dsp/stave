//! The window's controller: the plugin's own [`PluginController`] for
//! its parameters, plus the audio settings as stepped choices a settings
//! face binds (ids at [`SETTINGS_BASE`] and up, past any plugin's). The
//! settings take effect when a gesture ends: the device restarts, the
//! choices are saved, and each choice shows what actually runs.
//!
//! Everything here is the UI thread's (the face calls it there); the
//! audio callback and the MIDI thread only see the [`Instance`].

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use stave_core::params::{AnyParam, ParamFlags, ParamMeta, Unit, ValueText};
use stave_core::{Controller, GestureSink, ParamId, PluginController, Preset, TapId};
use stave_face::prelude::*;

use crate::audio::{self, Running};
use crate::midi;
use crate::session::Instance;
use crate::settings::{Device, Midi, Settings, Store};

/// The first settings choice's id (the rest follow).
pub const SETTINGS_BASE: u32 = 0xFFFF_FF00;
const OUTPUT: ParamId = ParamId(SETTINGS_BASE);
const INPUT: ParamId = ParamId(SETTINGS_BASE + 1);
const RATE: ParamId = ParamId(SETTINGS_BASE + 2);
const BUFFER: ParamId = ParamId(SETTINGS_BASE + 3);
const MIDI: ParamId = ParamId(SETTINGS_BASE + 4);

/// Names live as long as the process (a step name is `&'static str`);
/// each distinct name is leaked once.
fn intern(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let mut names = NAMES.get_or_init(Default::default).lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(&n) = names.iter().find(|&&n| n == name) {
        return n;
    }
    let n: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(n);
    n
}

/// A stepped parameter whose options change at run time (devices come
/// and go).
pub(crate) struct Choice {
    meta: ParamMeta,
    names: Mutex<Vec<&'static str>>,
    index: AtomicU32,
}

impl Choice {
    fn new(id: ParamId, name: &'static str) -> Self {
        Choice {
            meta: ParamMeta {
                id,
                name,
                group: "",
                unit: Unit::Plain { decimals: 0 },
                flags: ParamFlags { non_automatable: true, ..ParamFlags::default() },
            },
            names: Mutex::new(vec!["—"]),
            index: AtomicU32::new(0),
        }
    }

    fn count(&self) -> u32 {
        self.names.lock().unwrap_or_else(PoisonError::into_inner).len().max(1) as u32
    }

    fn index(&self) -> usize {
        self.index.load(Ordering::Acquire) as usize
    }

    fn offer(&self, names: &[String], at: usize) {
        let names: Vec<&'static str> = names.iter().map(|n| intern(n)).collect();
        let at = at.min(names.len().saturating_sub(1));
        *self.names.lock().unwrap_or_else(PoisonError::into_inner) = names;
        self.index.store(at as u32, Ordering::Release);
    }

    fn span(&self) -> f64 {
        f64::from(self.count().saturating_sub(1).max(1))
    }
}

impl AnyParam for Choice {
    fn meta(&self) -> &ParamMeta {
        &self.meta
    }
    fn value(&self) -> f64 {
        self.index() as f64
    }
    fn set_value(&self, value: f64) {
        let i = if value.is_nan() { 0 } else { value.round().max(0.0) as u32 };
        self.index.store(i.min(self.count() - 1), Ordering::Release);
    }
    fn normalized(&self) -> f64 {
        self.value() / self.span()
    }
    fn set_normalized(&self, n: f64) {
        self.set_value(n.clamp(0.0, 1.0) * self.span());
    }
    fn default_normalized(&self) -> f64 {
        0.0
    }
    fn normalize(&self, value: f64) -> f64 {
        (value / self.span()).clamp(0.0, 1.0)
    }
    fn denormalize(&self, n: f64) -> f64 {
        (n.clamp(0.0, 1.0) * self.span()).round()
    }
    fn format(&self, value: f64) -> ValueText {
        use core::fmt::Write as _;
        let names = self.names.lock().unwrap_or_else(PoisonError::into_inner);
        let i = (value.round().max(0.0) as usize).min(names.len().saturating_sub(1));
        let mut t = ValueText::default();
        let _ = t.write_str(names.get(i).copied().unwrap_or(""));
        t
    }
    fn parse(&self, text: &str) -> Option<f64> {
        let names = self.names.lock().unwrap_or_else(PoisonError::into_inner);
        names.iter().position(|n| n.eq_ignore_ascii_case(text.trim())).map(|i| i as f64)
    }
    fn step_count(&self) -> u32 {
        self.count()
    }
    fn step_name(&self, index: u32) -> Option<&'static str> {
        self.names.lock().unwrap_or_else(PoisonError::into_inner).get(index as usize).copied()
    }
}

/// What each choice's options mean, in order.
#[derive(Default)]
struct Options {
    outputs: Vec<Device>,
    /// `None`: input off.
    inputs: Vec<Option<Device>>,
    rates: Vec<u32>,
    buffers: Vec<u32>,
    midi: Vec<Midi>,
}

/// No DAW to tell: the table already holds the edit, and the saver
/// (see `run`) notices it there.
struct Edits;

impl GestureSink for Edits {
    fn begin(&self, _id: ParamId) {}
    fn perform(&self, _id: ParamId, _value: f64) {}
    fn end(&self, _id: ParamId) {}
}

pub(crate) struct Window {
    pub(crate) instance: Arc<dyn Instance>,
    plugin: PluginController,
    store: Option<Store>,
    name: &'static str,
    settings: RefCell<Settings>,
    choices: [Choice; 5],
    options: RefCell<Options>,
    audio: RefCell<Option<Running>>,
    midi: RefCell<Option<midi::Input>>,
}

impl Window {
    pub(crate) fn new(instance: Arc<dyn Instance>, store: Option<Store>, name: &'static str) -> Self {
        let settings = store.as_ref().map_or_else(|| Settings::first_run(instance.takes_notes()), |s| s.settings(instance.takes_notes()));
        Window {
            plugin: instance.controller(Box::new(Edits)),
            instance,
            store,
            name,
            settings: RefCell::new(settings),
            choices: [
                Choice::new(OUTPUT, "OUTPUT"),
                Choice::new(INPUT, "INPUT"),
                Choice::new(RATE, "SAMPLE RATE"),
                Choice::new(BUFFER, "BUFFER"),
                Choice::new(MIDI, "MIDI IN"),
            ],
            options: RefCell::default(),
            audio: RefCell::new(None),
            midi: RefCell::new(None),
        }
    }

    /// (Re)start the audio device on the current settings. A failure
    /// leaves the window up, silent, saying why on stderr.
    pub(crate) fn start_audio(&self) {
        drop(self.audio.borrow_mut().take());
        let settings = self.settings.borrow().clone();
        match audio::start(self.instance.clone(), &settings) {
            Ok(running) => {
                let buffer = running.buffer.map_or_else(|| "device buffer".into(), |n| format!("{n} frames"));
                let input = running.input.as_deref().unwrap_or("input off");
                eprintln!("{}: audio: {} ({input}) at {} Hz, {buffer}", self.name, running.output, running.sample_rate);
                *self.audio.borrow_mut() = Some(running);
            }
            Err(e) => eprintln!("{}: audio: {e}", self.name),
        }
    }

    pub(crate) fn start_midi(&self) {
        drop(self.midi.borrow_mut().take());
        let instance = self.instance.clone();
        let choice = self.settings.borrow().midi.clone();
        match midi::open(&choice, self.name, Box::new(move |m| instance.midi(m))) {
            Ok(input) => *self.midi.borrow_mut() = input,
            Err(e) => eprintln!("{}: MIDI: {e}", self.name),
        }
    }

    /// Stop the device, then save the state (quitting).
    pub(crate) fn finish(&self) {
        drop(self.midi.borrow_mut().take());
        drop(self.audio.borrow_mut().take());
        if let Some(store) = &self.store {
            if let Err(e) = store.save_state(&self.instance.save()) {
                eprintln!("{}: state not saved: {e}", self.name);
            }
        }
    }

    /// Re-list devices and point each choice at what runs now.
    pub(crate) fn refresh(&self) {
        let devices = audio::devices();
        let settings = self.settings.borrow().clone();
        let running = self.audio.borrow();
        let running = running.as_ref();
        let mut o = Options::default();

        let default_out = format!("Default ({})", devices.default_output.as_deref().unwrap_or("none"));
        let mut names = vec![default_out];
        o.outputs.push(Device::Default);
        for d in &devices.outputs {
            names.push(d.clone());
            o.outputs.push(Device::Named(d.clone()));
        }
        let at = o.outputs.iter().position(|d| *d == settings.output).unwrap_or(0);
        self.choices[0].offer(&names, at);

        let (plugin_in, _) = self.instance.channels();
        let mut names = vec!["Off".to_string()];
        o.inputs.push(None);
        if plugin_in > 0 {
            names.push(format!("Default ({})", devices.default_input.as_deref().unwrap_or("none")));
            o.inputs.push(Some(Device::Default));
            for d in &devices.inputs {
                names.push(d.clone());
                o.inputs.push(Some(Device::Named(d.clone())));
            }
        }
        let want = settings.input_on.then(|| settings.input.clone());
        let at = o.inputs.iter().position(|d| *d == want).unwrap_or(0);
        self.choices[1].offer(&names, at);

        let now = running.map_or_else(|| "off".to_string(), |r| format!("{} Hz", r.sample_rate));
        let mut names = vec![format!("Device ({now})")];
        o.rates.push(0);
        for r in audio::rates(&settings) {
            names.push(format!("{r} Hz"));
            o.rates.push(r);
        }
        let at = o.rates.iter().position(|&r| r == settings.sample_rate).unwrap_or(0);
        self.choices[2].offer(&names, at);

        let mut names = vec!["Device default".to_string()];
        o.buffers.push(0);
        for b in audio::BUFFERS {
            names.push(format!("{b} frames"));
            o.buffers.push(b);
        }
        let at = o.buffers.iter().position(|&b| b == settings.buffer).unwrap_or(0);
        self.choices[3].offer(&names, at);

        let mut names = vec!["None".to_string(), "All inputs".to_string()];
        o.midi.extend([Midi::None, Midi::All]);
        for s in midi::sources() {
            names.push(s.clone());
            o.midi.push(Midi::Named(s));
        }
        let at = o.midi.iter().position(|m| *m == settings.midi).unwrap_or(0);
        self.choices[4].offer(&names, at);
        *self.options.borrow_mut() = o;
    }

    fn choice(&self, id: ParamId) -> Option<&Choice> {
        self.choices.iter().find(|c| c.meta.id == id)
    }

    /// A settings gesture ended: take the choice, restart what it
    /// affects, save, and show what runs.
    fn apply(&self, id: ParamId) {
        let Some(choice) = self.choice(id) else { return };
        let i = choice.index();
        let before = self.settings.borrow().clone();
        {
            let o = self.options.borrow();
            let mut s = self.settings.borrow_mut();
            match id {
                OUTPUT => s.output = o.outputs.get(i).cloned().unwrap_or_default(),
                INPUT => match o.inputs.get(i).cloned().flatten() {
                    Some(d) => (s.input, s.input_on) = (d, true),
                    None => s.input_on = false,
                },
                RATE => s.sample_rate = o.rates.get(i).copied().unwrap_or(0),
                BUFFER => s.buffer = o.buffers.get(i).copied().unwrap_or(0),
                _ => s.midi = o.midi.get(i).cloned().unwrap_or(Midi::None),
            }
        }
        let after = self.settings.borrow().clone();
        if after == before {
            return;
        }
        if id == MIDI {
            self.start_midi();
        } else {
            self.start_audio();
        }
        if let Some(store) = &self.store {
            if let Err(e) = store.save_settings(&after) {
                eprintln!("{}: settings not saved: {e}", self.name);
            }
        }
        self.refresh();
    }

    /// The audio settings as a face (Tab swaps it with the plugin's).
    pub(crate) fn settings_face(&self) -> Face {
        let theme = Theme::graphite();
        let stepper = |id: ParamId, name: &'static str, width: f64| Stepper::new(Bind::new(id, name)).width(width).node();
        let header = Title::new("AUDIO").tagline("tab: back to the plugin").node();
        let devices = Plate::titled("DEVICES").around(
            column(vec![stepper(OUTPUT, "OUTPUT", 300.0), stepper(INPUT, "INPUT", 300.0)]).gap(theme.space(2)).align(Align::Center),
        );
        let clock = Plate::titled("CLOCK").around(
            row(vec![stepper(RATE, "SAMPLE RATE", 150.0), stepper(BUFFER, "BUFFER", 150.0)]).gap(theme.space(2)).align(Align::Center),
        );
        let notes = Plate::titled("MIDI").around(stepper(MIDI, "MIDI IN", 300.0));
        let body = column(vec![header, devices, clock, notes]).gap(theme.space(3)).align(Align::Stretch);
        Face::new(theme, Panel::default().around(body))
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.finish();
    }
}

/// The controller a `FaceView` owns: the shared window.
pub(crate) struct Handle(pub(crate) std::rc::Rc<Window>);

impl Controller for Handle {
    fn param(&self, id: ParamId) -> Option<&dyn AnyParam> {
        match self.0.choice(id) {
            Some(c) => Some(c),
            None => self.0.plugin.param(id),
        }
    }
    fn begin(&self, id: ParamId) {
        if self.0.choice(id).is_none() {
            self.0.plugin.begin(id);
        }
    }
    fn set(&self, id: ParamId, normalized: stave_core::Normalized) {
        match self.0.choice(id) {
            Some(c) => c.set_normalized(normalized.get()),
            None => self.0.plugin.set(id, normalized),
        }
    }
    fn end(&self, id: ParamId) {
        match self.0.choice(id) {
            Some(_) => self.0.apply(id),
            None => self.0.plugin.end(id),
        }
    }
    fn tap(&self, id: TapId) -> f64 {
        self.0.plugin.tap(id)
    }
    fn capture(&self, channel: usize, out: &mut [f32]) -> usize {
        self.0.plugin.capture(channel, out)
    }
    fn curve(&self) -> Vec<f32> {
        self.0.plugin.curve()
    }
    fn presets(&self) -> &[Preset] {
        self.0.plugin.presets()
    }
    fn preset(&self) -> Option<usize> {
        self.0.plugin.preset()
    }
    fn apply_preset(&self, index: usize) {
        self.0.plugin.apply_preset(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_steps_through_its_current_options() {
        let c = Choice::new(OUTPUT, "OUTPUT");
        c.offer(&["Default (Speakers)".into(), "Speakers".into(), "Interface".into()], 2);
        assert_eq!((c.step_count(), c.value(), c.normalized()), (3, 2.0, 1.0));
        assert_eq!(c.format(c.value()).as_str(), "Interface");
        c.set_normalized(0.5);
        assert_eq!(c.step_name(c.index() as u32), Some("Speakers"));
        c.offer(&["Only".into()], 5);
        assert_eq!((c.index(), c.normalized(), c.parse("only")), (0, 0.0, Some(0.0)));
    }

    #[test]
    fn settings_ids_sit_past_every_plugin_id() {
        const { assert!(SETTINGS_BASE > 0xFFFF_0000 && MIDI.0 > OUTPUT.0) };
    }
}
