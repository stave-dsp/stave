//! A stand-in plugin for the storybook: an in-memory parameter table the
//! face edits, and probes that move with time.

use std::cell::Cell;

use stave_face::{Bind, Host};

/// One demo parameter.
pub struct Param {
    pub bind: Bind,
    pub value: Cell<f64>,
    pub default: f64,
    /// Option names for a stepped parameter (empty = continuous).
    pub steps: &'static [&'static str],
    /// Formats a normalized value for display.
    pub format: fn(f64) -> String,
}

pub struct StoryHost {
    pub params: Vec<Param>,
    pub time: Cell<f64>,
}

fn percent(v: f64) -> String {
    format!("{:.0} %", v * 100.0)
}

fn ms(v: f64) -> String {
    format!("{:.0} ms", 10.0 * (200.0f64).powf(v))
}

fn db(v: f64) -> String {
    format!("{:+.1} dB", -24.0 + 48.0 * v)
}

fn hz(v: f64) -> String {
    format!("{:.2} Hz", 0.05 * (400.0f64).powf(v))
}

fn none(_: f64) -> String {
    String::new()
}

macro_rules! p {
    ($id:expr, $name:expr, $v:expr, steps $s:expr) => {
        Param {
            bind: Bind::new($id, $name),
            value: Cell::new($v),
            default: $v,
            steps: $s,
            format: none,
        }
    };
    ($id:expr, $name:expr, $v:expr, $fmt:expr) => {
        Param {
            bind: Bind::new($id, $name),
            value: Cell::new($v),
            default: $v,
            steps: &[],
            format: $fmt,
        }
    };
}

pub mod ids {
    pub const DRIVE: u32 = 1;
    pub const TONE: u32 = 2;
    pub const MIX: u32 = 3;
    pub const TIME: u32 = 4;
    pub const RATE: u32 = 5;
    pub const DEPTH: u32 = 6;
    pub const SHAPE: u32 = 7;
    pub const SYNC: u32 = 8;
    pub const LEVEL: u32 = 9;
    pub const BYPASS: u32 = 10;
    pub const HOLD: u32 = 11;
    pub const WIDTH: u32 = 12;
}

impl StoryHost {
    pub fn new() -> Self {
        use ids::*;
        StoryHost {
            params: vec![
                p!(DRIVE, "DRIVE", 0.62, percent),
                p!(TONE, "TONE", 0.5, db),
                p!(MIX, "MIX", 0.4, percent),
                p!(TIME, "TIME", 0.55, ms),
                p!(RATE, "RATE", 0.35, hz),
                p!(DEPTH, "DEPTH", 0.7, percent),
                p!(SHAPE, "SHAPE", 0.5, steps & ["SINE", "SQUARE", "DRIFT"]),
                p!(SYNC, "SYNC", 0.0, steps & ["FREE", "TEMPO"]),
                p!(LEVEL, "LEVEL", 0.5, db),
                p!(BYPASS, "BYPASS", 0.0, steps & ["ON", "OFF"]),
                p!(HOLD, "HOLD", 0.0, steps & ["OFF", "ON"]),
                p!(WIDTH, "WIDTH", 0.66, percent),
            ],
            time: Cell::new(0.0),
        }
    }

    pub fn bind(&self, id: u32) -> Bind {
        self.param(id).bind
    }

    fn param(&self, id: u32) -> &Param {
        self.params.iter().find(|p| p.bind.id == id).expect("demo param")
    }
}

impl Host for StoryHost {
    fn normalized(&self, id: u32) -> f64 {
        self.param(id).value.get()
    }
    fn default_normalized(&self, id: u32) -> f64 {
        self.param(id).default
    }
    fn steps(&self, id: u32) -> u32 {
        self.param(id).steps.len() as u32
    }
    fn step_name(&self, id: u32, index: u32) -> Option<String> {
        self.param(id).steps.get(index as usize).map(|s| s.to_string())
    }
    fn value_text(&self, id: u32) -> String {
        let p = self.param(id);
        if p.steps.is_empty() {
            (p.format)(p.value.get())
        } else {
            let n = p.steps.len() as u32;
            p.steps[stave_face::step_of(p.value.get(), n) as usize].to_string()
        }
    }
    fn probe(&self, name: &str) -> f64 {
        let t = self.time.get();
        match name {
            "level" => 0.5 + 0.45 * (t * 1.3).sin(),
            "phase" => t,
            _ => 0.0,
        }
    }
    fn begin(&mut self, _id: u32) {}
    fn set(&mut self, id: u32, normalized: f64) {
        let p = self.param(id);
        let v = if p.steps.is_empty() {
            normalized
        } else {
            let n = p.steps.len() as u32;
            stave_face::step_value(stave_face::step_of(normalized, n), n)
        };
        p.value.set(v.clamp(0.0, 1.0));
    }
    fn end(&mut self, _id: u32) {}
}
