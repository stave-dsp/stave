//! The editor area: whichever view is active.

use eframe::egui::{self, RichText};

use super::HostApp;
use crate::audio::MAX_EVENTS;
use crate::project::ViewId;
use crate::views;

impl HostApp {
    pub(super) fn draw_central(&mut self, ui: &mut egui::Ui, dt: f32) {
        if self.project.active_view == ViewId::Voicing {
            self.draw_audition_banner(ui);
            return self.draw_voicing(ui);
        }
        let rate = self.project.sample_rate.hz() as f32;
        let config = self.host_config();
        let (input, output) = (&self.shared.input_tap, &self.shared.output_tap);
        let telemetry = self.session.as_ref().and_then(|s| s.telemetry());
        let v = &mut self.views;
        match self.project.active_view {
            ViewId::Plugin => {
                // Reserve the whole rect; the native editor floats over it
                // (placed after the panels lay out — see `place_editor`).
                let rect = ui.available_rect_before_wrap();
                self.editor_rect = rect;
                match self.session.as_ref().and_then(|s| s.editor()) {
                    Some(ed)
                        if rect.width() < ed.width as f32 || rect.height() < ed.height as f32 =>
                    {
                        let need = egui::vec2(
                            (ed.width as f32 - rect.width()).max(0.0),
                            (ed.height as f32 - rect.height()).max(0.0),
                        );
                        ui.vertical_centered(|ui| {
                            ui.add_space(rect.height() * 0.3);
                            ui.label(
                                RichText::new(format!(
                                    "The editor needs {} × {}: the window is {} × {} short.",
                                    ed.width as i32, ed.height as i32, need.x as i32, need.y as i32
                                ))
                                .weak(),
                            );
                            if ui.button("Grow the window to fit").clicked() {
                                let size = ui.ctx().screen_rect().size() + need + egui::vec2(8.0, 8.0);
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                            }
                        });
                    }
                    Some(_) => {}
                    None => {
                        ui.centered_and_justified(|ui| {
                            ui.label(RichText::new("this plugin ships no editor view").weak());
                        });
                    }
                }
            }
            ViewId::Profiles => {
                let params = self
                    .session
                    .as_ref()
                    .map(|s| s.params())
                    .unwrap_or_default();
                let source = views::profiles::Source {
                    bundle: &self.project.plugin_path,
                    config,
                    params,
                };
                views::profiles::ui(ui, source, &mut v.profiles)
            }
            ViewId::Response => views::response::ui(ui, input, output, rate, &mut v.response),
            ViewId::Spectrogram => views::spectrogram::ui(ui, output, rate, &mut v.spectrogram),
            ViewId::Scope => views::scope::ui(ui, output, &mut v.scope),
            ViewId::Spectrum => views::spectrum::ui(ui, output, rate, &mut v.spectrum),
            ViewId::Meters => views::meters::ui(ui, output, &mut v.meters, dt),
            ViewId::Phase => views::phase::ui(ui, output, rate, &mut v.phase),
            ViewId::Debugger => views::debugger::ui(ui, telemetry),
            ViewId::Validate => {
                views::validate::ui(ui, &self.project.plugin_path, config, &mut v.validate)
            }
            ViewId::Perf => views::perf::ui(
                ui,
                views::perf::PerfInfo {
                    log: &self.shared.log,
                    sample_rate: rate,
                    telemetry,
                    state: &mut v.perf,
                },
            ),
            ViewId::Voicing => {}
            ViewId::Debug => views::debug::ui(
                ui,
                views::debug::DebugInfo {
                    log: &self.shared.log,
                    ring: output,
                    plugin_path: self.project.plugin_path.display().to_string(),
                    device_name: self.device_name.clone(),
                    sample_rate: rate,
                    block_size: self.project.block_size.get(),
                    output_channels: self.session.as_ref().map_or(0, |s| s.outputs),
                    event_queue_len: self.shared.events.len(),
                    event_queue_cap: MAX_EVENTS,
                },
            ),
        }
    }

    /// Float the native editor over the Plugin view's rect (hidden on
    /// every other view).
    pub(super) fn place_editor(&self, ctx: &egui::Context) {
        let Some(ed) = self.session.as_ref().and_then(|s| s.editor()) else {
            return;
        };
        let r = self.editor_rect;
        // A native view floats above egui: hide it while the palette
        // (drawn over the editor area) is open.
        let hidden = self.project.active_view != ViewId::Plugin || r == egui::Rect::NOTHING || self.palette.open;
        ed.place(
            f64::from(r.left()),
            f64::from(r.top()),
            f64::from(r.width()),
            f64::from(r.height()),
            f64::from(ctx.screen_rect().height()),
            hidden,
        );
    }
}
