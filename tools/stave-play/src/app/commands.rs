//! Commands: every action the workbench can take, named by id. The
//! palette lists them, the keymap binds shortcuts to them, and the
//! activity bar, status bar and menus run them, so there is one path
//! from intent to effect whichever surface it came from.
//!
//! A [`CommandSource`] offers commands; the workbench itself is one (its
//! layout, modes, audio settings, plugins, projects). The shared command
//! registry (the CLI's and MCP's commands) plugs in as another source:
//! the palette and the keymap only ever see [`CommandInfo`] and
//! [`Invocation`], never where a command came from.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

/// A command id and its argument (`view.open` + `Spectrum`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Invocation {
    pub id: String,
    pub arg: Option<String>,
}

impl Invocation {
    pub fn new(id: &str) -> Self {
        Self { id: id.into(), arg: None }
    }

    pub fn with(id: &str, arg: impl Into<String>) -> Self {
        Self { id: id.into(), arg: Some(arg.into()) }
    }
}

/// A command as the palette lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandInfo {
    pub invocation: Invocation,
    /// "View", "Layout", "Audio", ...: shown before the title.
    pub category: &'static str,
    pub title: String,
}

impl CommandInfo {
    pub fn new(category: &'static str, title: impl Into<String>, invocation: Invocation) -> Self {
        Self { invocation, category, title: title.into() }
    }

    /// What the palette matches against: "Category: Title".
    pub fn label(&self) -> String {
        format!("{}: {}", self.category, self.title)
    }
}

/// Something that offers commands.
pub trait CommandSource {
    fn commands(&self) -> Vec<CommandInfo>;
}

/// The workbench's command ids.
pub mod id {
    pub const PALETTE: &str = "workbench.palette";
    pub const MODE: &str = "workbench.mode";
    pub const VIEW: &str = "workbench.view";
    pub const LIBRARY: &str = "workbench.toggleLibrary";
    pub const INSPECTOR: &str = "workbench.toggleInspector";
    pub const PANEL: &str = "workbench.togglePanel";
    pub const FOCUS: &str = "workbench.toggleFocus";
    pub const THEME: &str = "workbench.theme";
    pub const KEYBOARD: &str = "input.keyboard";
    pub const SAMPLES: &str = "input.samples";
    pub const OUTPUT: &str = "workbench.output";
    pub const QUEUE: &str = "workbench.toggleQueue";
    pub const NEXT_AUDITION: &str = "listen.nextAudition";
    pub const OCTAVE_UP: &str = "input.octaveUp";
    pub const OCTAVE_DOWN: &str = "input.octaveDown";
    pub const SAMPLE_RATE: &str = "audio.sampleRate";
    pub const BLOCK: &str = "audio.blockSize";
    pub const PLUGIN_OPEN: &str = "plugin.open";
    pub const PLUGIN_BROWSE: &str = "plugin.browse";
    pub const PLUGIN_RELOAD: &str = "plugin.reload";
    pub const PROJECT_NEW: &str = "project.new";
    pub const PROJECT_SAVE: &str = "project.save";
    pub const PROJECT_OPEN: &str = "project.open";
}

/// Shortcuts bound to invocations. Several shortcuts may run one
/// command (the palette has two).
pub struct Keymap {
    bindings: Vec<(KeyboardShortcut, Invocation)>,
}

const CMD: Modifiers = Modifiers::COMMAND;

fn chord(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

impl Default for Keymap {
    /// VS Code's bindings where it has one (palette, side bar, panel),
    /// numbered modes like every tabbed app.
    fn default() -> Self {
        use crate::project::Mode;
        let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5];
        let mut bindings = vec![
            (chord(CMD.plus(Modifiers::SHIFT), Key::P), Invocation::new(id::PALETTE)),
            (chord(CMD, Key::K), Invocation::new(id::PALETTE)),
            (chord(CMD.plus(Modifiers::ALT), Key::B), Invocation::new(id::INSPECTOR)),
            (chord(CMD, Key::B), Invocation::new(id::LIBRARY)),
            (chord(CMD, Key::J), Invocation::new(id::PANEL)),
            (chord(CMD.plus(Modifiers::SHIFT), Key::F), Invocation::new(id::FOCUS)),
            (chord(CMD, Key::R), Invocation::new(id::PLUGIN_RELOAD)),
            (chord(CMD, Key::S), Invocation::new(id::PROJECT_SAVE)),
            (chord(CMD, Key::O), Invocation::new(id::PLUGIN_BROWSE)),
            (chord(Modifiers::NONE, Key::X), Invocation::new(id::OCTAVE_UP)),
            (chord(Modifiers::NONE, Key::Z), Invocation::new(id::OCTAVE_DOWN)),
        ];
        for (m, k) in Mode::ALL.into_iter().zip(digits) {
            bindings.push((chord(CMD, k), Invocation::with(id::MODE, m.label())));
        }
        // Most specific first: egui ignores extra Shift/Alt when matching.
        bindings.sort_by_key(|(s, _)| std::cmp::Reverse(modifier_count(s.modifiers)));
        Self { bindings }
    }
}

fn modifier_count(m: Modifiers) -> u8 {
    u8::from(m.alt) + u8::from(m.shift) + u8::from(m.command || m.ctrl || m.mac_cmd)
}

impl Keymap {
    /// The first shortcut bound to `inv`.
    pub fn shortcut(&self, inv: &Invocation) -> Option<KeyboardShortcut> {
        self.bindings.iter().find(|(_, i)| i == inv).map(|(s, _)| *s)
    }

    /// The shortcut's text for this platform (Shift+Cmd+P on a Mac).
    pub fn hint(&self, ctx: &egui::Context, inv: &Invocation) -> Option<String> {
        // Spelled out: egui's bundled fonts have ⌘ but not ⇧ or ⌥.
        self.shortcut(inv).map(|s| ctx.format_shortcut(&s))
    }

    /// Consume this frame's pressed shortcuts and return what they run.
    /// Bare keys (no Cmd/Ctrl) only fire when `bare_keys` (no text field
    /// has focus and the view doesn't own the letter keys).
    pub fn fired(&self, ctx: &egui::Context, bare_keys: bool) -> Vec<Invocation> {
        let mut out = vec![];
        ctx.input_mut(|i| {
            for (s, inv) in &self.bindings {
                let bare = !(s.modifiers.command || s.modifiers.ctrl || s.modifiers.mac_cmd);
                if bare && (!bare_keys || i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt) {
                    continue;
                }
                if i.consume_shortcut(s) {
                    out.push(inv.clone());
                }
            }
        });
        out
    }
}

/// Fuzzy match `query` against `text`: every query character in order,
/// case-insensitive. Higher is better: runs of consecutive characters
/// and matches at word starts score, gaps cost. `None`: no match.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().collect();
    let mut score = 0;
    let mut qi = 0;
    let mut last: Option<usize> = None;
    for (ti, c) in t.iter().enumerate() {
        if qi == q.len() {
            break;
        }
        if c.to_lowercase().eq(std::iter::once(q[qi])) {
            let word_start = ti == 0 || !t[ti - 1].is_alphanumeric() || (t[ti - 1].is_lowercase() && c.is_uppercase());
            score += 1;
            if word_start {
                score += 8;
            }
            match last {
                Some(l) if l + 1 == ti => score += 5,
                Some(l) => score -= ((ti - l - 1) as i32).min(4),
                None => score -= (ti as i32).min(6),
            }
            last = Some(ti);
            qi += 1;
        }
    }
    (qi == q.len()).then_some(score)
}

/// `items` matching `query`, best first (ties keep their order).
pub fn filter<'a>(query: &str, items: &'a [CommandInfo]) -> Vec<&'a CommandInfo> {
    let mut scored: Vec<(i32, usize, &CommandInfo)> = items
        .iter()
        .enumerate()
        .filter_map(|(n, c)| fuzzy_score(query, &c.label()).map(|s| (s, n, c)))
        .collect();
    scored.sort_by_key(|&(s, n, _)| (std::cmp::Reverse(s), n));
    scored.into_iter().map(|(_, _, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(category: &'static str, title: &str) -> CommandInfo {
        CommandInfo::new(category, title, Invocation::new(title))
    }

    #[test]
    fn fuzzy_needs_every_character_in_order() {
        assert!(fuzzy_score("spc", "Measure: Spectrum").is_some());
        assert!(fuzzy_score("cps", "Measure: Spectrum").is_none());
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert!(fuzzy_score("SPEC", "measure: spectrum").is_some(), "any case");
    }

    #[test]
    fn word_starts_and_runs_win() {
        let items = [
            info("Measure", "Spectrogram"),
            info("Layout", "Toggle panel"),
            info("Input", "Show samples"),
            info("Measure", "Spectrum"),
        ];
        let titles = |q: &str| filter(q, &items).iter().map(|c| c.title.clone()).collect::<Vec<_>>();
        assert_eq!(titles("tp")[0], "Toggle panel", "initials");
        assert_eq!(titles("spectrum")[0], "Spectrum");
        assert_eq!(titles("samp"), ["Show samples"]);
        assert_eq!(titles("").len(), 4, "empty query lists everything");
    }

    #[test]
    fn every_binding_is_distinct_and_specific_chords_match_first() {
        let k = Keymap::default();
        let mut seen = std::collections::HashSet::new();
        for (s, _) in &k.bindings {
            assert!(seen.insert((s.modifiers, s.logical_key)), "{s:?} bound twice");
        }
        let alt_b = k.bindings.iter().position(|(_, i)| i.id == id::INSPECTOR).unwrap();
        let b = k.bindings.iter().position(|(_, i)| i.id == id::LIBRARY).unwrap();
        assert!(alt_b < b, "Cmd+Alt+B must be tried before Cmd+B");
    }

    fn press(ctx: &egui::Context, key: Key, modifiers: Modifiers) -> Vec<Invocation> {
        let input = egui::RawInput {
            modifiers,
            events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }],
            ..Default::default()
        };
        let mut out = vec![];
        let _ = ctx.run(input, |c| out = Keymap::default().fired(c, true));
        out
    }

    #[test]
    fn shortcuts_fire_their_commands() {
        let ctx = egui::Context::default();
        let cmd = Modifiers::COMMAND;
        assert_eq!(press(&ctx, Key::J, cmd), [Invocation::new(id::PANEL)]);
        assert_eq!(press(&ctx, Key::B, cmd.plus(Modifiers::ALT)), [Invocation::new(id::INSPECTOR)]);
        assert_eq!(press(&ctx, Key::B, cmd), [Invocation::new(id::LIBRARY)]);
        assert_eq!(press(&ctx, Key::P, cmd.plus(Modifiers::SHIFT)), [Invocation::new(id::PALETTE)]);
        assert_eq!(press(&ctx, Key::Num3, cmd), [Invocation::with(id::MODE, "Measure")]);
        assert_eq!(press(&ctx, Key::X, Modifiers::NONE), [Invocation::new(id::OCTAVE_UP)]);
        assert!(press(&ctx, Key::J, Modifiers::NONE).is_empty(), "a bare J is a note");
    }

    #[test]
    fn bare_keys_wait_while_a_view_owns_them() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::X,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut out = vec![Invocation::new("sentinel")];
        let _ = ctx.run(input, |c| out = Keymap::default().fired(c, false));
        assert!(out.is_empty());
    }
}
