//! The Voicing view: setup (material, candidates, options), then the
//! live session (loudness, timeline and loop, the modes), driven from
//! the keyboard.
//!
//! Keys (whenever no text field has focus):
//!
//! ```text
//! space        play / stop
//! 1–9          select a candidate (sighted) or a rating slot
//! A  B  X      sighted: candidates 1 and 2 · ABX: listen to A, B or X
//! ⇧A  ⇧B  ← →  ABX: answer "X is A" / "X is B"
//! ⏎  N         next trial (confirm the answer or the ratings)
//! R            rating: the open reference
//! ↑ ↓          rating: move the playing slot's slider (⇧ by 10)
//! [  ]         loop start / end at the playhead · ⌫ whole material
//! ```

use eframe::egui::{self, Color32, Key, Modifiers, RichText, Sense, Stroke};
use stave_host::opened::Opened;
use stave_measure::listening::{Abx, ALPHA, ASSUMED_HIT_RATE};

use super::candidate::{Candidate, Source};
use super::takes::{LOUDNESS_TOLERANCE_LU, PEAK_CEILING_DBFS};
use super::trials::AbxListen;
use super::{builtin, log, material, Ctx, Live, MaterialChoice, Mode, Row, Voicing, MAX_CANDIDATES};
use crate::app::library;

pub(super) const ACCENT: Color32 = Color32::from_rgb(140, 200, 255);
pub(super) const GOOD: Color32 = Color32::from_rgb(90, 200, 140);
pub(super) const WARN: Color32 = Color32::from_rgb(235, 180, 70);
pub(super) const BAD: Color32 = Color32::from_rgb(235, 90, 80);
const PANEL: Color32 = Color32::from_rgb(16, 18, 22);

/// A key pressed this frame, taken so no focused widget sees it too.
pub(super) fn take_key(ctx: &egui::Context, mods: Modifiers, key: Key) -> bool {
    ctx.input_mut(|i| i.consume_key(mods, key))
}

impl Voicing {
    /// The whole view, once per frame.
    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &mut Ctx) {
        self.poll_prepare(ctx);
        if self.autostart && self.preparing.is_none() && self.live.is_none() {
            self.autostart = false;
            self.prepare(ctx);
        }
        if self.live.is_some() {
            self.ensure_stream(ctx);
        }
        let shortcuts = !ui.ctx().wants_keyboard_input();
        self.header(ui);
        ui.separator();
        if self.live.is_some() {
            if shortcuts {
                self.keys(ui.ctx());
            }
            self.live_ui(ui);
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| self.setup_ui(ui, ctx));
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Voicing");
            if let Some((text, error)) = &self.status {
                ui.separator();
                let c = if *error { BAD } else { ui.visuals().text_color() };
                ui.label(RichText::new(text).color(c));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Open").on_hover_text("show the folder").clicked() {
                    reveal(&self.dir);
                }
                if ui.small_button("Change…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_directory(&self.dir).pick_folder() {
                        self.dir = d;
                    }
                }
                ui.label(
                    RichText::new(format!("logs: {}", self.dir.display()))
                        .weak()
                        .small(),
                );
            });
        });
    }

    // ---- setup ---------------------------------------------------------

    fn setup_ui(&mut self, ui: &mut egui::Ui, ctx: &mut Ctx) {
        let rate = ctx.config.sample_rate.hz();
        section(ui, "Program material", |ui| self.material_ui(ui, rate));
        ui.add_space(8.0);
        section(ui, "Candidates", |ui| self.candidates_ui(ui, ctx));
        ui.add_space(8.0);
        section(ui, "Options", |ui| self.options_ui(ui));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if let Some(p) = &self.preparing {
                let f = Live::progress_of_prepare(p);
                ui.add(egui::ProgressBar::new(f).desired_width(320.0).text("running candidates…"));
                ui.ctx().request_repaint();
            } else {
                let ready = self.setup.rows.len() >= 2;
                let b = egui::Button::new(RichText::new("Prepare  ⏎").strong())
                    .min_size(egui::vec2(140.0, 30.0));
                if ui.add_enabled(ready, b).clicked() || (ready && take_key(ui.ctx(), Modifiers::NONE, Key::Enter)) {
                    self.prepare(ctx);
                }
                ui.label(
                    RichText::new(format!(
                        "runs every candidate over the material, compensates latency, and matches \
                         loudness to {LOUDNESS_TOLERANCE_LU} LU (BS.1770)"
                    ))
                    .weak(),
                );
            }
        });
    }

    fn material_ui(&mut self, ui: &mut egui::Ui, rate: f64) {
        let mut chosen: Option<MaterialChoice> = None;
        let current = match &self.setup.material {
            MaterialChoice::Builtin(n) => format!("built-in: {n}"),
            MaterialChoice::File(p) => format!("file: {}", p.file_name().map_or_else(String::new, |f| f.to_string_lossy().into())),
            MaterialChoice::Role(r, i) => format!("role: {r}#{i}"),
        };
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_source("voicing_material")
                .selected_text(current)
                .width(320.0)
                .show_ui(ui, |ui| {
                    ui.label(RichText::new("BUILT-IN").small().strong());
                    for b in builtin::BUILTINS {
                        let sel = self.setup.material == MaterialChoice::Builtin(b.name.into());
                        if ui.selectable_label(sel, b.name).on_hover_text(b.about).clicked() {
                            chosen = Some(MaterialChoice::Builtin(b.name.into()));
                        }
                    }
                    if let Some(files) = &self.library {
                        if !files.is_empty() {
                            ui.separator();
                            ui.label(RichText::new("SAMPLE LIBRARY · TEST CORPUS").small().strong());
                            for (rel, path) in files {
                                let sel = self.setup.material == MaterialChoice::File(path.clone());
                                if ui.selectable_label(sel, rel).clicked() {
                                    chosen = Some(MaterialChoice::File(path.clone()));
                                }
                            }
                        }
                    }
                });
            if ui.button("Open WAV…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("WAV", &["wav", "WAV"]).pick_file() {
                    chosen = Some(MaterialChoice::File(p));
                }
            }
            match &self.library_root {
                Some(root) if self.library.is_none() => {
                    if ui.button("Scan library").on_hover_text(root.display().to_string()).clicked() {
                        let files = material::library_files(root);
                        if files.is_empty() {
                            self.status = Some((format!("no WAVs under {}/Test Corpus (drive not mounted?)", root.display()), true));
                        }
                        self.library = Some(files);
                    }
                }
                Some(_) => {}
                None => {
                    ui.label(RichText::new("STAVE_SAMPLE_LIBRARY not set: built-ins or any WAV").weak().small());
                }
            }
        });
        if let Some(c) = chosen {
            self.setup.material = c;
        }
        ui.horizontal(|ui| {
            ui.label("input trim");
            ui.add(
                egui::Slider::new(&mut self.setup.trim_db, -24.0..=24.0)
                    .step_by(0.5)
                    .suffix(" dB"),
            )
            .on_hover_text("the level the candidates are driven at (nonlinear models care); every take is loudness-matched afterwards");
        });
        match self.material(rate) {
            Ok(m) => {
                let about = match &m.source {
                    material::Source::Builtin { name } => builtin::find(name).map_or("", |b| b.about).to_string(),
                    material::Source::File { path, .. } => path.display().to_string(),
                    material::Source::Corpus { role, item, .. } => format!("corpus role {role}: {item}"),
                };
                let conv = if (m.original_rate - m.sample_rate).abs() > 0.5 {
                    format!(" (from {} Hz)", m.original_rate)
                } else {
                    String::new()
                };
                ui.label(
                    RichText::new(format!(
                        "{:.1} s · {} ch · {} Hz{conv} · sha256 {}…  —  {about}",
                        m.seconds(),
                        m.channels.len(),
                        m.sample_rate,
                        &m.pcm_sha256[..12]
                    ))
                    .weak(),
                );
            }
            Err(e) => {
                ui.label(RichText::new(e).color(BAD));
            }
        }
    }

    fn candidates_ui(&mut self, ui: &mut egui::Ui, ctx: &mut Ctx) {
        let mut remove = None;
        let mut probe = None;
        let rows = self.setup.rows.len();
        for (i, row) in self.setup.rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}", i + 1)).monospace().strong().color(ACCENT));
                let is_ref = self.setup.reference == Some(i);
                if ui
                    .selectable_label(is_ref, "ref")
                    .on_hover_text("the reference: the open and hidden reference in rating trials, and the anchor's source")
                    .clicked()
                {
                    self.setup.reference = if is_ref { None } else { Some(i) };
                }
                if ui
                    .add(egui::TextEdit::singleline(&mut row.candidate.label).desired_width(260.0))
                    .changed()
                {
                    row.custom_label = true;
                }
                ui.label(RichText::new(source_text(&row.candidate)).weak().small());
                if row.candidate.is_plugin() {
                    let txt = if row.editing { "settings ▾" } else { "settings ▸" };
                    if ui.small_button(txt).clicked() {
                        row.editing = !row.editing;
                        if row.editing && row.params.is_empty() {
                            probe = Some(i);
                        }
                    }
                }
                if ui.small_button("✕").clicked() {
                    remove = Some(i);
                }
            });
            if row.editing {
                settings_editor(ui, row);
            }
        }
        if rows == 0 {
            ui.label(RichText::new("no candidates yet: add two or more below (a dry anchor is a good first)").italics().weak());
        }
        if let Some(i) = probe {
            self.probe_params(i, ctx);
        }
        if let Some(i) = remove {
            self.setup.rows.remove(i);
            self.setup.reference = match self.setup.reference {
                Some(r) if r == i => None,
                Some(r) if r > i => Some(r - 1),
                r => r,
            };
        }
        ui.add_space(4.0);
        let full = self.setup.rows.len() >= MAX_CANDIDATES;
        ui.add_enabled_ui(!full, |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut add: Option<Candidate> = None;
                egui::ComboBox::from_id_source("voicing_add_plugin")
                    .selected_text("+ plugin or stage")
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for p in library::scan_vst3(&library::vst3_dirs()).into_iter().chain(library::stages()) {
                            if ui.selectable_label(false, library::display_name(&p)).clicked() {
                                add = Some(Candidate::plugin(p));
                            }
                        }
                    });
                let current = ctx.main_plugin.as_ref();
                if ui
                    .add_enabled(current.is_some(), egui::Button::new("+ current settings"))
                    .on_hover_text("the plugin loaded in stave-play, with every param as it is set now")
                    .clicked()
                {
                    if let Some((path, params)) = current {
                        let mut c = Candidate::plugin(path.clone());
                        for p in params.iter().filter(|p| !p.read_only && !p.program && !p.flags.hidden) {
                            c.set(&p.name, p.value);
                        }
                        c.label = format!("{} (as set)", library::display_name(path));
                        add = Some(c);
                    }
                }
                if ui.button("+ dry").on_hover_text("the material unprocessed: the bypass anchor").clicked() {
                    add = Some(Candidate::dry());
                }
                if ui.button("+ recording…").on_hover_text("a WAV aligned to the material, e.g. a capture").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("WAV", &["wav", "WAV"]).pick_file() {
                        add = Some(Candidate::recording(p));
                    }
                }
                if ui.button("+ blessed…").on_hover_text("a sound blessed in an earlier session").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_directory(log::blessed_dir(&self.dir))
                        .add_filter("bless record", &["json"])
                        .pick_file()
                    {
                        add = Some(Candidate::blessed(p));
                    }
                }
                if let Some(c) = add {
                    let custom = matches!(c.source, Source::Plugin { .. }) && c.label.ends_with("(as set)");
                    self.setup.rows.push(Row {
                        candidate: c,
                        params: Vec::new(),
                        editing: false,
                        custom_label: custom,
                    });
                }
            });
        });
        if full {
            ui.label(RichText::new(format!("{MAX_CANDIDATES} candidates: one per digit key")).weak().small());
        }
    }

    /// Open a throwaway instance to read a candidate's params.
    fn probe_params(&mut self, i: usize, ctx: &Ctx) {
        let row = &mut self.setup.rows[i];
        let Source::Plugin { path } = &row.candidate.source else { return };
        match Opened::open(path, ctx.config) {
            Ok(mut o) => {
                row.params = o
                    .instance()
                    .params()
                    .into_iter()
                    .filter(|p| !p.read_only && !p.flags.hidden)
                    .collect();
            }
            Err(e) => self.status = Some((crate::errors::with_causes(&e), true)),
        }
    }

    fn options_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("ABX trials");
            ui.add(egui::DragValue::new(&mut self.setup.abx_trials).clamp_range(1..=1_000));
            let n = self.setup.abx_trials;
            let power = Abx::power(n, ALPHA, ASSUMED_HIT_RATE);
            let need = Abx::critical(n, ALPHA).map_or("never significant".to_string(), |k| format!("needs {k}/{n}"));
            let c = if power >= 0.8 { GOOD } else { WARN };
            ui.label(
                RichText::new(format!(
                    "{need} at p < {ALPHA} · {:.0} % power to detect a {ASSUMED_HIT_RATE} hit rate",
                    power * 100.0
                ))
                .color(c),
            );
        });
        ui.horizontal(|ui| {
            ui.label("rating trials");
            ui.add(egui::DragValue::new(&mut self.setup.rating_trials).clamp_range(1..=50));
            ui.checkbox(&mut self.setup.anchor, "low anchor (3.5 kHz low-pass of the reference)")
                .on_hover_text("BS.1534-3's anchor counters range-equalising bias");
        });
    }

    // ---- live ----------------------------------------------------------

    fn live_ui(&mut self, ui: &mut egui::Ui) {
        let dir = self.dir.clone();
        let (mut end, mut retry) = (false, false);
        let Some(l) = &mut self.live else { return };
        ui.horizontal(|ui| {
            let m = &l.prepared.matching;
            let ok = m.max_deviation_lu <= LOUDNESS_TOLERANCE_LU;
            ui.label(RichText::new(&l.prepared.material.name).strong());
            ui.label(RichText::new(format!("trim {:+.1} dB", l.prepared.trim_db)).weak());
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "{} matched at {:.1} LUFS · max deviation {:.3} LU",
                    if ok { "✔" } else { "✘" },
                    m.target_lufs,
                    m.max_deviation_lu
                ))
                .color(if ok { GOOD } else { BAD }),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("End session").on_hover_text("write the log and go back to the setup").clicked() {
                    end = true;
                }
                if l.stream.is_none() && ui.button("Retry audio").clicked() {
                    retry = true;
                }
            });
        });
        egui::CollapsingHeader::new("Takes and loudness")
            .default_open(false)
            .show(ui, |ui| takes_table(ui, l));
        ui.add_space(4.0);
        transport_ui(ui, l);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (mode, name) in [(Mode::Sighted, "Sighted A/B"), (Mode::Abx, "Blind ABX"), (Mode::Rating, "Rating")] {
                if ui.selectable_label(l.mode == mode, RichText::new(name).strong()).clicked() && l.mode != mode {
                    l.mode = mode;
                    l.transport.select(0);
                }
            }
        });
        ui.separator();
        let mut status = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                status = match l.mode {
                    Mode::Sighted => l.sighted_ui(ui, &dir),
                    Mode::Abx => l.abx_ui(ui, &dir),
                    Mode::Rating => l.rating_ui(ui, &dir),
                };
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("session notes");
                    ui.add(egui::TextEdit::singleline(&mut l.history.notes).desired_width(420.0));
                });
                if let Some((path, at)) = &l.saved {
                    ui.label(RichText::new(format!("log saved {at} → {}", path.display())).weak().small());
                }
            });
        if let Some(s) = status {
            self.status = Some(s);
        }
        if end {
            self.end_session();
        }
        if retry {
            self.audio_failed = false;
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let dir = self.dir.clone();
        let Some(l) = &mut self.live else { return };
        let t = l.transport.clone();
        if take_key(ctx, Modifiers::NONE, Key::Space) {
            t.set_playing(!t.is_playing());
        }
        let pos = t.position();
        let (start, end) = t.loop_region();
        let min_len = (0.1 * l.prepared.sample_rate) as u64;
        if take_key(ctx, Modifiers::NONE, Key::OpenBracket) && pos + min_len < end {
            t.set_loop(pos, end);
        }
        if take_key(ctx, Modifiers::NONE, Key::CloseBracket) && pos > start + min_len {
            t.set_loop(start, pos);
        }
        if take_key(ctx, Modifiers::NONE, Key::Backspace) {
            t.set_loop(0, t.frames());
        }
        let digit = digit_pressed(ctx);
        let next = take_key(ctx, Modifiers::NONE, Key::Enter) || take_key(ctx, Modifiers::NONE, Key::N);
        let status = match l.mode {
            Mode::Sighted => {
                let n = l.prepared.candidates.len();
                let mut pick = digit.filter(|&d| d < n);
                if take_key(ctx, Modifiers::NONE, Key::A) {
                    pick = Some(0);
                }
                if take_key(ctx, Modifiers::NONE, Key::B) && n > 1 {
                    pick = Some(1);
                }
                if let Some(d) = pick {
                    l.select(d);
                }
                None
            }
            Mode::Abx => l.abx_keys(ctx, digit, next, &dir),
            Mode::Rating => l.rating_keys(ctx, digit, next, &dir),
        };
        if let Some(s) = status {
            self.status = Some(s);
        }
    }
}

impl Live {
    fn abx_keys(&mut self, ctx: &egui::Context, digit: Option<usize>, next: bool, dir: &std::path::Path) -> Option<(String, bool)> {
        let Some(run) = &mut self.abx else { return None };
        let mut listen = None;
        if take_key(ctx, Modifiers::SHIFT, Key::A) || take_key(ctx, Modifiers::NONE, Key::ArrowLeft) {
            run.answer(true);
        }
        if take_key(ctx, Modifiers::SHIFT, Key::B) || take_key(ctx, Modifiers::NONE, Key::ArrowRight) {
            run.answer(false);
        }
        if take_key(ctx, Modifiers::NONE, Key::A) || digit == Some(0) {
            listen = Some(AbxListen::A);
        }
        if take_key(ctx, Modifiers::NONE, Key::B) || digit == Some(1) {
            listen = Some(AbxListen::B);
        }
        if take_key(ctx, Modifiers::NONE, Key::X) || digit == Some(2) {
            listen = Some(AbxListen::X);
        }
        if let Some(w) = listen {
            let take = run.listen(w);
            self.select(take);
        }
        if next {
            return self.confirm_abx(dir);
        }
        None
    }

    fn rating_keys(&mut self, ctx: &egui::Context, digit: Option<usize>, next: bool, dir: &std::path::Path) -> Option<(String, bool)> {
        let Some(run) = &mut self.rating else { return None };
        if let Some(d) = digit {
            if let Some(take) = run.take_at(d) {
                self.rating_focus = d;
                self.select(take);
            }
        }
        let Some(run) = &mut self.rating else { return None };
        if take_key(ctx, Modifiers::NONE, Key::R) {
            if let Some(take) = run.reference {
                self.rating_focus = usize::MAX;
                self.select(take);
            }
        }
        let Some(run) = &mut self.rating else { return None };
        let step = |up: bool, big: bool| (if big { 10.0 } else { 1.0 }) * if up { 1.0 } else { -1.0 };
        for (key, up) in [(Key::ArrowUp, true), (Key::ArrowDown, false)] {
            // Shift first: a plain pattern also matches a shifted key.
            for (mods, big) in [(Modifiers::SHIFT, true), (Modifiers::NONE, false)] {
                if take_key(ctx, mods, key) {
                    if let Some(v) = run.sliders.get_mut(self.rating_focus) {
                        *v = (*v + step(up, big)).clamp(0.0, 100.0);
                    }
                }
            }
        }
        if next {
            return self.confirm_rating(dir);
        }
        None
    }
}

fn digit_pressed(ctx: &egui::Context) -> Option<usize> {
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
    DIGITS.iter().position(|&k| take_key(ctx, Modifiers::NONE, k))
}

fn source_text(c: &Candidate) -> String {
    match &c.source {
        Source::Dry => "dry material".into(),
        Source::Plugin { path } => {
            let mut s = library::display_name(path);
            if let Some(p) = &c.preset {
                s.push_str(&format!(" · preset {p}"));
            }
            if !c.settings.is_empty() {
                s.push_str(&format!(" · {} setting(s)", c.settings.len()));
            }
            s
        }
        Source::Recording { path } => format!("recording · {}", path.display()),
        Source::Blessed { record } => format!("blessed · {}", record.display()),
    }
}

/// Sliders in each param's own units; a change becomes a setting.
fn settings_editor(ui: &mut egui::Ui, row: &mut Row) {
    egui::Frame::none()
        .fill(PANEL)
        .rounding(4.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            if row.params.is_empty() {
                ui.label(RichText::new("(no writable params)").weak());
            }
            let presets: Vec<String> = row
                .params
                .iter()
                .find(|p| p.program)
                .map(|p| p.step_names.clone())
                .unwrap_or_default();
            if !presets.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("preset");
                    egui::ComboBox::from_id_source(("voicing_preset", row.candidate.label.as_str()))
                        .selected_text(row.candidate.preset.clone().unwrap_or_else(|| "(none)".into()))
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(row.candidate.preset.is_none(), "(none)").clicked() {
                                row.candidate.preset = None;
                            }
                            for name in &presets {
                                if ui.selectable_label(row.candidate.preset.as_deref() == Some(name), name).clicked() {
                                    row.candidate.preset = Some(name.clone());
                                }
                            }
                        });
                });
            }
            let mut changed = false;
            for p in row.params.iter().filter(|p| !p.program) {
                let set = row.candidate.settings.iter().find(|s| s.name == p.name).map(|s| s.value);
                let mut v = set.unwrap_or(p.value);
                ui.horizontal(|ui| {
                    let mut slider = egui::Slider::new(&mut v, p.min..=p.max).text(format!("{} {}", p.leaf(), p.unit));
                    if p.step_count > 1 {
                        slider = slider.step_by((p.max - p.min) / f64::from(p.step_count - 1));
                        if !p.step_names.is_empty() {
                            let names = p.step_names.clone();
                            let (min, max, steps) = (p.min, p.max, p.step_count);
                            slider = slider.custom_formatter(move |x, _| {
                                let i = ((x - min) / (max - min) * f64::from(steps - 1)).round() as usize;
                                names.get(i).cloned().unwrap_or_default()
                            });
                        }
                    } else if p.unit.eq_ignore_ascii_case("hz") && p.min > 0.0 {
                        slider = slider.logarithmic(true);
                    }
                    if ui.add(slider).changed() {
                        row.candidate.set(&p.name, v);
                        changed = true;
                    }
                    if set.is_some() && ui.small_button("↺").on_hover_text("back to default").clicked() {
                        row.candidate.settings.retain(|s| s.name != p.name);
                        changed = true;
                    }
                });
            }
            if changed && !row.custom_label {
                row.candidate.label = row.candidate.default_label();
            }
        });
}

fn takes_table(ui: &mut egui::Ui, l: &Live) {
    let m = &l.prepared.matching;
    egui::Grid::new("voicing_takes").striped(true).num_columns(7).show(ui, |ui| {
        for h in ["take", "loudness raw", "gain", "matched", "peak", "latency", "notes"] {
            ui.label(RichText::new(h).strong());
        }
        ui.end_row();
        for (i, t) in l.prepared.takes.iter().enumerate() {
            ui.label(&t.label);
            ui.label(format!("{:.2} LUFS", m.raw_lufs[i]));
            ui.label(format!("{:+.2} dB", m.gain_db[i]));
            ui.label(format!("{:.2} LUFS", m.matched_lufs[i]));
            let pk = m.matched_peak_dbfs[i];
            ui.label(RichText::new(format!("{pk:.1} dBFS")).color(if pk > PEAK_CEILING_DBFS + 0.01 { BAD } else { ui.visuals().text_color() }));
            ui.label(format!("{} fr", t.reported_latency));
            if t.warnings.is_empty() {
                let note = match t.offset {
                    Some((0, r)) => format!("aligned (r = {r:.2})"),
                    _ => String::new(),
                };
                ui.label(RichText::new(note).weak());
            } else {
                ui.label(RichText::new(t.warnings.join("; ")).color(WARN));
            }
            ui.end_row();
        }
    });
    if m.headroom_db < 0.0 {
        ui.label(RichText::new(format!("all takes turned down {:.1} dB more to keep peaks under {PEAK_CEILING_DBFS} dBFS", -m.headroom_db)).weak().small());
    }
}

/// Play/stop, position, and the timeline: click to seek, drag to loop.
fn transport_ui(ui: &mut egui::Ui, l: &mut Live) {
    let t = l.transport.clone();
    let rate = l.prepared.sample_rate;
    ui.horizontal(|ui| {
        let playing = t.is_playing();
        let label = if playing { "■  stop" } else { "▶  play" };
        if ui.add(egui::Button::new(RichText::new(label).strong()).min_size(egui::vec2(84.0, 26.0))).clicked() {
            t.set_playing(!playing);
        }
        let pos = t.position() as f64 / rate;
        let (s, e) = t.loop_region();
        ui.label(RichText::new(format!("{pos:5.2} s")).monospace());
        ui.label(
            RichText::new(format!("loop {:.2}–{:.2} s", s as f64 / rate, e as f64 / rate))
                .weak()
                .monospace(),
        );
        ui.checkbox(&mut l.restart_on_switch, "restart loop on switch");
        ui.label(RichText::new("click: seek · drag: loop · double-click: whole").weak().small());
    });
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 64.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, PANEL);
    let frames = t.frames().max(1) as f32;
    let x_of = |f: u64| rect.left() + rect.width() * f as f32 / frames;
    let (s, e) = t.loop_region();
    painter.rect_filled(
        egui::Rect::from_x_y_ranges(x_of(s)..=x_of(e), rect.y_range()),
        0.0,
        Color32::from_rgba_unmultiplied(140, 200, 255, 28),
    );
    let mid = rect.center().y;
    let h = rect.height() * 0.45;
    let n = l.overview.len().max(1) as f32;
    for (i, (lo, hi)) in l.overview.iter().enumerate() {
        let x = rect.left() + rect.width() * i as f32 / n;
        painter.line_segment(
            [egui::pos2(x, mid - hi * 2.0 * h), egui::pos2(x, mid - lo * 2.0 * h)],
            Stroke::new(1.0, Color32::from_gray(120)),
        );
    }
    let px = x_of(t.position());
    painter.line_segment([egui::pos2(px, rect.top()), egui::pos2(px, rect.bottom())], Stroke::new(1.5, ACCENT));
    let frame_at = |x: f32| (((x - rect.left()) / rect.width()).clamp(0.0, 1.0) * frames) as u64;
    if resp.double_clicked() {
        t.set_loop(0, t.frames());
    } else if resp.drag_stopped() {
        if let (Some(a), Some(b)) = (ui.ctx().input(|i| i.pointer.press_origin()), resp.interact_pointer_pos()) {
            let (fa, fb) = (frame_at(a.x), frame_at(b.x));
            if fa.abs_diff(fb) as f64 > 0.1 * rate {
                t.set_loop(fa.min(fb), fa.max(fb));
                t.seek(fa.min(fb));
            }
        }
    } else if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            t.seek(frame_at(p.x));
        }
    }
    if resp.dragged() {
        if let (Some(a), Some(b)) = (ui.ctx().input(|i| i.pointer.press_origin()), resp.interact_pointer_pos()) {
            painter.rect_filled(
                egui::Rect::from_x_y_ranges(a.x.min(b.x)..=a.x.max(b.x), rect.y_range()),
                0.0,
                Color32::from_rgba_unmultiplied(140, 200, 255, 50),
            );
        }
    }
    ui.ctx().request_repaint();
}

/// A titled block.
pub(super) fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(title.to_uppercase()).strong().small());
    ui.add_space(2.0);
    body(ui);
}

/// Show a folder in the platform's file browser.
fn reveal(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(dir).spawn();
}

/// "label (n)" for a bless record path in a status line.
pub(super) fn blessed_status(label: &str, path: &std::path::Path) -> String {
    format!("blessed {label} → {}", path.display())
}
