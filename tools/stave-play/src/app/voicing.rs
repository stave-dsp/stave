//! The Voicing view's place in the app: it borrows the audio device from
//! the main plugin while a comparison plays, and gives it back when the
//! view is left.

use eframe::egui;

use super::HostApp;
use crate::voicing::Ctx;

impl HostApp {
    pub(super) fn draw_voicing(&mut self, ui: &mut egui::Ui) {
        let main_plugin = self
            .session
            .as_ref()
            .map(|s| (self.project.plugin_path.clone(), s.params()));
        let config = self.host_config();
        let session = &mut self.session;
        let suspended = &mut self.suspended_for_voicing;
        let status = &mut self.status;
        let status_error = &mut self.status_error;
        let mut suspend = || {
            if session.take().is_some() {
                *suspended = true;
                *status = "main plugin paused while listening (leave Listen to resume)".into();
                *status_error = false;
            }
        };
        let mut ctx = Ctx {
            config,
            shared: &self.shared,
            main_plugin,
            suspend_main: &mut suspend,
        };
        self.voicing.ui(ui, &mut ctx);
    }

    /// Stop the comparison's stream and bring the main plugin back.
    pub(super) fn leave_voicing(&mut self) {
        let had_device = self.voicing.is_playing_device();
        self.voicing.release_device();
        if self.suspended_for_voicing || (had_device && self.session.is_none()) {
            self.suspended_for_voicing = false;
            self.reload();
        }
    }
}
