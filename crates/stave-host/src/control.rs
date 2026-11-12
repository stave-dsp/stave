//! Controls — one model for a hosted plugin's parameters, whatever
//! told us about them (a VST3 controller's inventory, a stave plugin's
//! telemetry, a test double). Values are PLAIN (in the parameter's own
//! units), never normalized.

/// What a control is allowed to be used for.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct ControlFlags {
    pub automatable: bool,
    pub read_only: bool,
    pub hidden: bool,
    /// The plugin's own bypass switch.
    pub bypass: bool,
    /// Selects a program (preset) — moving it rewrites other controls.
    pub program_change: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Control {
    /// Host-format id (VST3 ParamId).
    pub id: u32,
    /// Full title. stave titles are group-prefixed: "Delay · dry".
    pub name: String,
    pub group: String,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub value: f64,
    /// `value` as the plugin's own normalized position, 0..1: its taper.
    pub normalized: f64,
    /// Discrete positions minus one (VST3 `stepCount`); 0 = continuous.
    pub steps: u32,
    /// One label per position, when the plugin names them.
    pub step_names: Vec<String>,
    pub flags: ControlFlags,
}

impl Control {
    /// The title after its group prefix ("Delay · dry" → "dry").
    pub fn leaf(&self) -> &str {
        self.name.rsplit('·').next().map_or(&self.name, str::trim)
    }

    /// Values a sweep visits: every position of a discrete control,
    /// otherwise `points` evenly spaced values, both ends included.
    pub fn sweep_values(&self, points: usize) -> impl Iterator<Item = f64> + '_ {
        let last = match self.steps {
            0 => points.max(2) - 1,
            steps => steps as usize,
        };
        (0..=last).map(move |i| self.min + (self.max - self.min) * i as f64 / last as f64)
    }

    /// Whether a sweep may drive this control across its range: it is
    /// writable, not a program switch, and its range is real.
    pub fn sweepable(&self) -> bool {
        !self.flags.read_only
            && !self.flags.program_change
            && self.max.is_finite()
            && self.min.is_finite()
    }

    /// `t` ∈ [0, 1] across the plain range (linear in plain units).
    pub fn lerp(&self, t: f64) -> f64 {
        self.min + (self.max - self.min) * t.clamp(0.0, 1.0)
    }
}

/// Look a control up by name: an exact (case-insensitive) title match
/// wins; otherwise the group-stripped leaf ("dry" finds "Delay · dry").
/// Titles are group-prefixed, and an exact-only match once fell through
/// to a silent no-op (a 32-config sweep measured 32 identical results).
pub fn find<'a>(controls: &'a [Control], name: &str) -> Option<&'a Control> {
    let name = name.trim();
    controls
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            controls
                .iter()
                .find(|c| c.leaf().eq_ignore_ascii_case(name))
        })
}

/// Why a control write failed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ControlError {
    #[error("no control named '{0}'")]
    NotFound(String),
    #[error("control '{0}' is read-only")]
    ReadOnly(String),
    #[error("control '{name}' takes {min}..={max}")]
    OutOfRange { name: String, min: f64, max: f64 },
    #[error("control '{name}' takes one of {}", numbered(options))]
    NotAnOption { name: String, options: Vec<String> },
}

fn numbered(options: &[String]) -> String {
    let listed: Vec<String> = options
        .iter()
        .enumerate()
        .map(|(i, o)| format!("{i} = {o}"))
        .collect();
    listed.join(", ")
}

#[cfg(test)]
pub(crate) fn test_control(name: &str, min: f64, max: f64) -> Control {
    Control {
        id: 0,
        name: name.into(),
        group: String::new(),
        unit: String::new(),
        min,
        max,
        default: min,
        value: min,
        normalized: 0.0,
        steps: 0,
        step_names: vec![],
        flags: ControlFlags {
            automatable: true,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_title_beats_leaf() {
        let cs = [
            test_control("Delay · mix", 0.0, 1.0),
            test_control("mix", 0.0, 1.0),
        ];
        assert_eq!(find(&cs, "MIX").unwrap().name, "mix");
        assert_eq!(find(&cs, "delay · mix").unwrap().name, "Delay · mix");
    }

    #[test]
    fn leaf_matches_group_prefixed_titles() {
        let cs = [test_control("Delay · dry", 0.0, 1.0)];
        assert_eq!(find(&cs, " Dry ").unwrap().leaf(), "dry");
        assert!(find(&cs, "wet").is_none());
    }

    #[test]
    fn sweeps_visit_every_step_or_evenly_spaced_values() {
        let mut c = test_control("x", 0.0, 1.0);
        assert_eq!(
            c.sweep_values(5).collect::<Vec<_>>(),
            [0.0, 0.25, 0.5, 0.75, 1.0]
        );
        c.steps = 2;
        assert_eq!(c.sweep_values(5).collect::<Vec<_>>(), [0.0, 0.5, 1.0]);
    }

    #[test]
    fn sweepable_excludes_read_only_and_program_switches() {
        let mut c = test_control("x", 0.0, 1.0);
        assert!(c.sweepable());
        c.flags.read_only = true;
        assert!(!c.sweepable());
        c.flags = ControlFlags {
            program_change: true,
            ..Default::default()
        };
        assert!(!c.sweepable());
        let inf = test_control("y", 0.0, f64::INFINITY);
        assert!(!inf.sweepable());
    }

    #[test]
    fn lerp_clamps_to_range() {
        let c = test_control("g", -24.0, 24.0);
        assert_eq!((c.lerp(0.0), c.lerp(0.5), c.lerp(2.0)), (-24.0, 0.0, 24.0));
    }
}
