//! Params — one model for a hosted plugin's parameters, whatever told
//! us about them (a VST3 controller's inventory, a linked stave
//! plugin's table, a catalog stage, a test double). Values are in the
//! parameter's own units, never normalized; `normalized` is the
//! plugin's own 0–1 position (its taper). The words and the shared
//! types are stave-core's: [`ParamId`], [`ParamFlags`], and
//! `step_count` as [`AnyParam::step_count`] counts it.

use stave_core::params::AnyParam;
pub use stave_core::params::{ParamFlags, ParamId};

/// One parameter of a hosted plugin, as read at one moment.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Param {
    /// Forever id (a VST3 `ParamID`, a stave [`ParamId`]).
    #[serde(serialize_with = "id_number")]
    pub id: ParamId,
    /// Full title. stave titles are group-prefixed: "Delay · dry".
    pub name: String,
    pub group: String,
    /// The unit's label ("dB", "kΩ"); empty for a bare number.
    pub unit: String,
    /// The value at normalized 0 and 1.
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub value: f64,
    /// `value` as the plugin's own normalized position, 0..1: its taper.
    pub normalized: f64,
    /// Discrete positions; 0 = continuous ([`AnyParam::step_count`]).
    pub step_count: u32,
    /// One label per position, when the plugin names them.
    pub step_names: Vec<String>,
    #[serde(serialize_with = "flag_names")]
    pub flags: ParamFlags,
    /// Output only: the plugin writes it, a host never does (VST3
    /// `kIsReadOnly`, a meter).
    pub read_only: bool,
    /// Selects a program (preset): moving it rewrites other params (VST3
    /// `kIsProgramChange`, a stave plugin's [`ParamId::PROGRAM`]).
    pub program: bool,
}

fn id_number<S: serde::Serializer>(id: &ParamId, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u32(id.0)
}

fn flag_names<S: serde::Serializer>(flags: &ParamFlags, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeStruct;
    let mut out = s.serialize_struct("ParamFlags", 3)?;
    out.serialize_field("bypass", &flags.bypass)?;
    out.serialize_field("non_automatable", &flags.non_automatable)?;
    out.serialize_field("hidden", &flags.hidden)?;
    out.end()
}

impl Param {
    /// A stave parameter as read now (a linked plugin, a catalog stage).
    pub fn of(p: &dyn AnyParam) -> Self {
        let m = p.meta();
        let step_count = p.step_count();
        Self {
            id: m.id,
            name: m.name.to_string(),
            group: m.group.to_string(),
            unit: m.unit.label().to_string(),
            min: p.denormalize(0.0),
            max: p.denormalize(1.0),
            default: p.denormalize(p.default_normalized()),
            value: p.value(),
            normalized: p.normalized(),
            step_count,
            step_names: (0..step_count)
                .filter_map(|i| p.step_name(i))
                .map(str::to_string)
                .collect(),
            flags: m.flags,
            read_only: false,
            program: m.id == ParamId::PROGRAM,
        }
    }

    /// The title after its group prefix ("Delay · dry" → "dry").
    pub fn leaf(&self) -> &str {
        self.name.rsplit('·').next().map_or(&self.name, str::trim)
    }

    /// Whether a host may record it as automation.
    pub fn automatable(&self) -> bool {
        !self.flags.non_automatable && !self.read_only
    }

    /// Values a sweep visits: every position of a discrete param,
    /// otherwise `points` evenly spaced values, both ends included.
    pub fn sweep_values(&self, points: usize) -> impl Iterator<Item = f64> + '_ {
        let last = match self.step_count {
            0 => points.max(2) - 1,
            positions => (positions as usize - 1).max(1),
        };
        (0..=last).map(move |i| self.min + (self.max - self.min) * i as f64 / last as f64)
    }

    /// Whether a sweep may drive this param across its range: it is
    /// writable, not a program switch, and its range is real.
    pub fn sweepable(&self) -> bool {
        !self.read_only && !self.program && self.max.is_finite() && self.min.is_finite()
    }

    /// `t` ∈ [0, 1] across the range (linear in the param's units).
    pub fn lerp(&self, t: f64) -> f64 {
        self.min + (self.max - self.min) * t.clamp(0.0, 1.0)
    }
}

/// Look a param up by name: an exact (case-insensitive) title match
/// wins; otherwise the group-stripped leaf ("dry" finds "Delay · dry").
/// Titles are group-prefixed, and an exact-only match once fell through
/// to a silent no-op (a 32-config sweep measured 32 identical results).
pub fn find<'a>(params: &'a [Param], name: &str) -> Option<&'a Param> {
    let name = name.trim();
    params
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .or_else(|| params.iter().find(|p| p.leaf().eq_ignore_ascii_case(name)))
}

/// Why a param write failed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ParamError {
    #[error("no param named '{0}'")]
    NotFound(String),
    #[error("param '{0}' is read-only")]
    ReadOnly(String),
    #[error("param '{name}' takes {min}..={max}")]
    OutOfRange { name: String, min: f64, max: f64 },
    #[error("param '{name}' takes one of {}", numbered(options))]
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
pub(crate) fn test_param(name: &str, min: f64, max: f64) -> Param {
    Param {
        id: ParamId(0),
        name: name.into(),
        group: String::new(),
        unit: String::new(),
        min,
        max,
        default: min,
        value: min,
        normalized: 0.0,
        step_count: 0,
        step_names: vec![],
        flags: ParamFlags::default(),
        read_only: false,
        program: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_core::params::{BoolParam, FloatParam, Range, Unit};

    #[test]
    fn exact_title_beats_leaf() {
        let ps = [test_param("Delay · mix", 0.0, 1.0), test_param("mix", 0.0, 1.0)];
        assert_eq!(find(&ps, "MIX").unwrap().name, "mix");
        assert_eq!(find(&ps, "delay · mix").unwrap().name, "Delay · mix");
    }

    #[test]
    fn leaf_matches_group_prefixed_titles() {
        let ps = [test_param("Delay · dry", 0.0, 1.0)];
        assert_eq!(find(&ps, " Dry ").unwrap().leaf(), "dry");
        assert!(find(&ps, "wet").is_none());
    }

    #[test]
    fn sweeps_visit_every_step_or_evenly_spaced_values() {
        let mut p = test_param("x", 0.0, 1.0);
        assert_eq!(
            p.sweep_values(5).collect::<Vec<_>>(),
            [0.0, 0.25, 0.5, 0.75, 1.0]
        );
        p.step_count = 3;
        assert_eq!(p.sweep_values(5).collect::<Vec<_>>(), [0.0, 0.5, 1.0]);
    }

    #[test]
    fn sweepable_excludes_read_only_and_program_switches() {
        let mut p = test_param("x", 0.0, 1.0);
        assert!(p.sweepable());
        p.read_only = true;
        assert!(!p.sweepable());
        p.read_only = false;
        p.program = true;
        assert!(!p.sweepable());
        let inf = test_param("y", 0.0, f64::INFINITY);
        assert!(!inf.sweepable());
    }

    #[test]
    fn lerp_clamps_to_range() {
        let p = test_param("g", -24.0, 24.0);
        assert_eq!((p.lerp(0.0), p.lerp(0.5), p.lerp(2.0)), (-24.0, 0.0, 24.0));
    }

    #[test]
    fn a_stave_param_reads_in_the_core_vocabulary() {
        let gain = FloatParam::new(ParamId(4), "Gain", Range::linear(-12.0, 12.0))
            .unit(Unit::Db)
            .default(3.0);
        let p = Param::of(&gain);
        assert_eq!((p.id, p.unit.as_str()), (ParamId(4), "dB"));
        assert_eq!((p.min, p.max, p.default, p.value), (-12.0, 12.0, 3.0, 3.0));
        assert_eq!((p.step_count, p.automatable()), (0, true));
        let bypass = Param::of(&BoolParam::bypass(ParamId(5), "Bypass"));
        assert!(bypass.flags.bypass);
        assert_eq!((bypass.step_count, bypass.step_names.len()), (2, 2));
    }
}
