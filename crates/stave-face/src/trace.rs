//! A face session as data: every pointer and key event a person (or a
//! test) sent a face, in order, as plain text lines. The runtime replays
//! a trace exactly ([`Runtime::apply`](crate::Runtime::apply)), so a
//! recorded session becomes a regression test, and a test's own trace is
//! the reproduction when it fails.
//!
//! ```text
//! hover 120 88
//! press 120 88 clicks=2 fine
//! move 120 48 fine
//! release
//! wheel 40 60 3
//! key up
//! exit
//! ```

use core::fmt;
use core::str::FromStr;

use stave_ui::scene::{pt, Pt};

use crate::runtime::{Key, Mods};

/// One event sent to a face (points, y down).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Act {
    /// The pointer moved with no button down.
    Hover(Pt),
    /// Button down; `clicks` is the click count (2 = double).
    Press { at: Pt, mods: Mods, clicks: u32 },
    /// The pointer moved with the button down.
    Move { at: Pt, mods: Mods },
    /// Button up.
    Release,
    /// The wheel, in lines (positive = up).
    Wheel { at: Pt, lines: f64, mods: Mods },
    Key { key: Key, mods: Mods },
    /// The pointer left the face.
    Exit,
}

/// A session: its events in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trace(pub Vec<Act>);

fn write_mods(f: &mut fmt::Formatter<'_>, m: Mods) -> fmt::Result {
    if m.fine {
        f.write_str(" fine")?;
    }
    if m.reset {
        f.write_str(" reset")?;
    }
    Ok(())
}

fn key_name(k: Key) -> &'static str {
    match k {
        Key::Up => "up",
        Key::Down => "down",
        Key::Left => "left",
        Key::Right => "right",
        Key::Home => "home",
        Key::End => "end",
        Key::Escape => "escape",
    }
}

impl fmt::Display for Act {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Act::Hover(p) => write!(f, "hover {} {}", p.x, p.y),
            Act::Press { at, mods, clicks } => {
                write!(f, "press {} {}", at.x, at.y)?;
                if clicks != 1 {
                    write!(f, " clicks={clicks}")?;
                }
                write_mods(f, mods)
            }
            Act::Move { at, mods } => {
                write!(f, "move {} {}", at.x, at.y)?;
                write_mods(f, mods)
            }
            Act::Release => f.write_str("release"),
            Act::Wheel { at, lines, mods } => {
                write!(f, "wheel {} {} {lines}", at.x, at.y)?;
                write_mods(f, mods)
            }
            Act::Key { key, mods } => {
                write!(f, "key {}", key_name(key))?;
                write_mods(f, mods)
            }
            Act::Exit => f.write_str("exit"),
        }
    }
}

impl fmt::Display for Trace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for act in &self.0 {
            writeln!(f, "{act}")?;
        }
        Ok(())
    }
}

/// Why a trace line did not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceError {
    pub line: usize,
    pub text: String,
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "trace line {}: cannot read `{}`", self.line, self.text)
    }
}

impl std::error::Error for TraceError {}

impl FromStr for Act {
    type Err = ();
    fn from_str(line: &str) -> Result<Self, ()> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| words.get(i).and_then(|w| w.parse::<f64>().ok()).filter(|v| v.is_finite()).ok_or(());
        let at = || Ok::<Pt, ()>(pt(num(1)?, num(2)?));
        let (mut mods, mut clicks) = (Mods::default(), 1);
        let first_flag = match words.first().copied() {
            Some("hover" | "press" | "move") => 3,
            Some("wheel") => 4,
            Some("key") => 2,
            _ => words.len(),
        };
        for w in words.iter().skip(first_flag) {
            match *w {
                "fine" => mods.fine = true,
                "reset" => mods.reset = true,
                w => clicks = w.strip_prefix("clicks=").and_then(|n| n.parse().ok()).ok_or(())?,
            }
        }
        Ok(match words.first().copied().ok_or(())? {
            "hover" => Act::Hover(at()?),
            "press" => Act::Press { at: at()?, mods, clicks },
            "move" => Act::Move { at: at()?, mods },
            "release" => Act::Release,
            "wheel" => Act::Wheel { at: at()?, lines: num(3)?, mods },
            "key" => {
                let key = match words.get(1).copied().ok_or(())? {
                    "up" => Key::Up,
                    "down" => Key::Down,
                    "left" => Key::Left,
                    "right" => Key::Right,
                    "home" => Key::Home,
                    "end" => Key::End,
                    "escape" => Key::Escape,
                    _ => return Err(()),
                };
                Act::Key { key, mods }
            }
            "exit" => Act::Exit,
            _ => return Err(()),
        })
    }
}

impl FromStr for Trace {
    type Err = TraceError;
    /// One event per line; blank lines and `#` comments are skipped.
    fn from_str(text: &str) -> Result<Self, TraceError> {
        let mut acts = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            acts.push(line.parse().map_err(|()| TraceError { line: i + 1, text: line.to_string() })?);
        }
        Ok(Trace(acts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trace_reads_back_what_it_writes() {
        let fine = Mods { fine: true, ..Mods::default() };
        let t = Trace(vec![
            Act::Hover(pt(1.5, 2.0)),
            Act::Press { at: pt(1.5, 2.0), mods: Mods { reset: true, fine: true }, clicks: 2 },
            Act::Move { at: pt(1.5, -40.25), mods: fine },
            Act::Release,
            Act::Wheel { at: pt(3.0, 4.0), lines: -2.5, mods: Mods::default() },
            Act::Key { key: Key::Escape, mods: fine },
            Act::Exit,
        ]);
        assert_eq!(t.to_string().parse::<Trace>(), Ok(t));
    }

    #[test]
    fn a_bad_line_is_named() {
        let e = "hover 1 2\n# a comment\npress one 2\n".parse::<Trace>().unwrap_err();
        assert_eq!(e.line, 3);
    }
}
