//! The load trace: every step of bringing a plugin up, with its outcome
//! and cost — the first thing to read when a plugin loads wrong.
//! Set `STAVE_HOST_TRACE=1` to stream steps to stderr as they happen
//! (the only record when a plugin crashes the process mid-load).

use std::fmt::Display;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct TraceStep {
    pub step: &'static str,
    pub ok: bool,
    pub detail: String,
    pub micros: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(transparent)]
pub struct LoadTrace {
    steps: Vec<TraceStep>,
    #[serde(skip)]
    live: bool,
}

impl LoadTrace {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            live: std::env::var_os("STAVE_HOST_TRACE").is_some(),
        }
    }

    pub fn steps(&self) -> &[TraceStep] {
        &self.steps
    }

    /// Run `f` as step `step`; its `Ok` is described by `describe`.
    pub fn step<T, E: Display>(
        &mut self,
        step: &'static str,
        f: impl FnOnce() -> Result<T, E>,
        describe: impl FnOnce(&T) -> String,
    ) -> Result<T, E> {
        let t0 = Instant::now();
        let r = f();
        let micros = t0.elapsed().as_micros() as u64;
        let (ok, detail) = match &r {
            Ok(v) => (true, describe(v)),
            Err(e) => (false, e.to_string()),
        };
        self.push(TraceStep {
            step,
            ok,
            detail,
            micros,
        });
        r
    }

    /// Record an informational step (no work timed).
    pub fn note(&mut self, step: &'static str, ok: bool, detail: impl Into<String>) {
        self.push(TraceStep {
            step,
            ok,
            detail: detail.into(),
            micros: 0,
        });
    }

    fn push(&mut self, s: TraceStep) {
        if self.live {
            let mark = if s.ok { "ok " } else { "ERR" };
            eprintln!(
                "[load] {mark} {:<22} {:>7}µs  {}",
                s.step, s.micros, s.detail
            );
        }
        self.steps.push(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_success_failure_and_notes_in_order() {
        let mut t = LoadTrace::default();
        let a: Result<u32, String> = t.step("open", || Ok(7), |v| format!("got {v}"));
        let b: Result<(), String> = t.step("init", || Err("boom".into()), |_| String::new());
        t.note("controller", true, "split");
        assert_eq!((a, b), (Ok(7), Err("boom".into())));
        let got: Vec<_> = t
            .steps()
            .iter()
            .map(|s| (s.step, s.ok, s.detail.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("open", true, "got 7"),
                ("init", false, "boom"),
                ("controller", true, "split")
            ]
        );
    }

    #[test]
    fn serializes_as_a_plain_list() {
        let mut t = LoadTrace::default();
        t.note("x", true, "y");
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v[0]["step"], "x");
    }
}
