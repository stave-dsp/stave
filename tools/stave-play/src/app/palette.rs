//! The command palette (Cmd+Shift+P / Cmd+K): every command by fuzzy
//! search, its shortcut beside it, recently run ones first. Up/Down
//! move, Enter runs, Escape (or a click outside) closes.

use eframe::egui::{self, Align2, Key, Modifiers, RichText};

use super::commands::{filter, CommandInfo, Invocation, Keymap};
use super::theme::Tokens;

const WIDTH: f32 = 560.0;
const ROWS: usize = 12;
const RECENT: usize = 5;

#[derive(Default)]
pub struct Palette {
    pub open: bool,
    pub query: String,
    selected: usize,
    /// Most recent first.
    recent: Vec<Invocation>,
    focus_pending: bool,
}

impl Palette {
    pub fn show(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus_pending = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.show();
        }
    }

    /// Remember `inv` as just run.
    pub fn ran(&mut self, inv: &Invocation) {
        self.recent.retain(|r| r != inv);
        self.recent.insert(0, inv.clone());
        self.recent.truncate(RECENT);
    }

    /// The rows for the current query: on an empty query the recent
    /// commands come first (marked), then the rest in order.
    pub fn rows<'a>(&self, items: &'a [CommandInfo]) -> Vec<(&'a CommandInfo, bool)> {
        if !self.query.trim().is_empty() {
            return filter(&self.query, items).into_iter().map(|c| (c, false)).collect();
        }
        let recent = self
            .recent
            .iter()
            .filter_map(|r| items.iter().find(|c| &c.invocation == r))
            .map(|c| (c, true));
        let rest = items.iter().filter(|c| !self.recent.contains(&c.invocation)).map(|c| (c, false));
        recent.chain(rest).collect()
    }

    /// Draw the palette (when open) and return the command picked.
    pub fn ui(&mut self, ctx: &egui::Context, items: &[CommandInfo], keymap: &Keymap, t: &Tokens) -> Option<Invocation> {
        if !self.open {
            return None;
        }
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.close();
            return None;
        }
        let rows = self.rows(items);
        if rows.is_empty() {
            self.selected = 0;
        } else {
            if down {
                self.selected = (self.selected + 1) % rows.len();
            }
            if up {
                self.selected = (self.selected + rows.len() - 1) % rows.len();
            }
            self.selected = self.selected.min(rows.len() - 1);
        }
        let mut picked = enter.then(|| rows.get(self.selected).map(|(c, _)| c.invocation.clone())).flatten();

        let screen = ctx.screen_rect();
        let width = WIDTH.min(screen.width() - 32.0);
        let area = egui::Area::new(egui::Id::new("command-palette"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 56.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                    ui.set_width(width);
                    let edit = egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Type a command (fuzzy: \"tp\" finds Toggle panel)")
                        .desired_width(f32::INFINITY)
                        .margin(egui::vec2(8.0, 6.0));
                    let response = ui.add(edit);
                    if self.focus_pending {
                        response.request_focus();
                        self.focus_pending = false;
                    }
                    if response.changed() {
                        self.selected = 0;
                    }
                    ui.add_space(4.0);
                    if rows.is_empty() {
                        ui.label(RichText::new("No matching commands").color(t.ink_soft));
                        return;
                    }
                    // Keep the selection in the window of rows shown.
                    let start = self.selected.saturating_sub(ROWS - 1);
                    for (n, (c, recent)) in rows.iter().enumerate().skip(start).take(ROWS) {
                        let selected = n == self.selected;
                        let hint = keymap.hint(ctx, &c.invocation);
                        let row = row(ui, c, *recent, hint.as_deref(), selected, t);
                        if row.clicked() {
                            picked = Some(c.invocation.clone());
                        }
                        if row.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            self.selected = n;
                        }
                    }
                    if rows.len() > ROWS {
                        ui.label(
                            RichText::new(format!("{} more: keep typing to narrow", rows.len() - ROWS.min(rows.len())))
                                .small()
                                .color(t.ink_soft),
                        );
                    }
                });
            });
        if area.response.clicked_elsewhere() && picked.is_none() {
            self.close();
        }
        if let Some(inv) = &picked {
            self.ran(inv);
            self.close();
        }
        picked
    }
}

fn row(ui: &mut egui::Ui, c: &CommandInfo, recent: bool, hint: Option<&str>, selected: bool, t: &Tokens) -> egui::Response {
    let height = 26.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 4.0, t.accent.gamma_multiply(0.22));
    }
    let left = rect.left_center() + egui::vec2(8.0, 0.0);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley_cat = ui.painter().layout_no_wrap(format!("{}: ", c.category), font.clone(), t.ink_soft);
    let cat_w = galley_cat.size().x;
    ui.painter().galley(left - egui::vec2(0.0, galley_cat.size().y / 2.0), galley_cat, t.ink_soft);
    ui.painter().text(left + egui::vec2(cat_w, 0.0), Align2::LEFT_CENTER, &c.title, font, t.ink);
    let mut right = rect.right_center() - egui::vec2(8.0, 0.0);
    let small = egui::TextStyle::Small.resolve(ui.style());
    if let Some(h) = hint {
        let r = ui.painter().text(right, Align2::RIGHT_CENTER, h, small.clone(), t.ink_soft);
        right.x = r.left() - 12.0;
    }
    if recent {
        ui.painter().text(right, Align2::RIGHT_CENTER, "recently used", small, t.ink_soft);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::super::commands::id;
    use super::*;

    fn items() -> Vec<CommandInfo> {
        vec![
            CommandInfo::new("Layout", "Toggle library", Invocation::new(id::LIBRARY)),
            CommandInfo::new("Layout", "Toggle panel", Invocation::new(id::PANEL)),
            CommandInfo::new("View", "Measure: Spectrum", Invocation::with(id::VIEW, "Spectrum")),
        ]
    }

    fn key(k: Key) -> egui::Event {
        egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    /// One frame of the palette with `events`; what it picked.
    fn frame(ctx: &egui::Context, p: &mut Palette, events: Vec<egui::Event>) -> Option<Invocation> {
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
            ..Default::default()
        };
        let (items, keymap, t) = (items(), Keymap::default(), Tokens::dark());
        let mut picked = None;
        let _ = ctx.run(input, |c| picked = p.ui(c, &items, &keymap, &t));
        picked
    }

    #[test]
    fn typing_filters_and_enter_runs_the_best_match() {
        let ctx = egui::Context::default();
        let mut p = Palette::default();
        p.show();
        assert_eq!(frame(&ctx, &mut p, vec![]), None, "opens, takes focus");
        assert_eq!(frame(&ctx, &mut p, vec![egui::Event::Text("spec".into())]), None);
        assert_eq!(p.query, "spec");
        assert_eq!(frame(&ctx, &mut p, vec![key(Key::Enter)]), Some(Invocation::with(id::VIEW, "Spectrum")));
        assert!(!p.open, "running a command closes the palette");
    }

    #[test]
    fn arrows_move_the_selection_and_escape_closes() {
        let ctx = egui::Context::default();
        let mut p = Palette::default();
        p.show();
        frame(&ctx, &mut p, vec![]);
        assert_eq!(
            frame(&ctx, &mut p, vec![key(Key::ArrowDown), key(Key::Enter)]),
            Some(Invocation::new(id::PANEL))
        );
        p.show();
        frame(&ctx, &mut p, vec![]);
        assert_eq!(frame(&ctx, &mut p, vec![key(Key::Escape)]), None);
        assert!(!p.open);
    }

    #[test]
    fn recent_commands_lead_an_empty_query() {
        let mut p = Palette::default();
        p.ran(&Invocation::with(id::VIEW, "Spectrum"));
        let items = items();
        let rows = p.rows(&items);
        assert_eq!(rows[0].0.invocation, Invocation::with(id::VIEW, "Spectrum"));
        assert!(rows[0].1);
        assert_eq!(rows.len(), 3, "no duplicates");
    }
}
