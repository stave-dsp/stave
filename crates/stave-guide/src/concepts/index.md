# Concepts

The ideas Stave is built on, one article each. Read them in order the
first time; each stands alone after that.

- [Lifecycle](crate::concepts::lifecycle) — how a plugin is built,
  prepared, processed and torn down, and which calls may allocate.
- [Params and smoothing](crate::concepts::params) — the typed parameter
  table, forever ids, units, and declared smoothing.
- [Faces, parts and themes](crate::concepts::faces) — the editor as a
  declaration: parts in a layout tree, drawn from a theme.
- [Telemetry and taps](crate::concepts::telemetry) — what a running
  plugin publishes, who reads it, and the event trace.
- [Testing](crate::concepts::testing) — `PluginHarness`, `FaceDriver`,
  `plugin_checks!`, golden pictures and property tests.
- [Verification](crate::concepts::verification) — the ladder from types
  to model checking, and where each check runs.
- [DSP](crate::concepts::dsp) — the `Stage` contract and the layers of
  the DSP vocabulary.
- [Architecture](crate::concepts::architecture) — the crates, the format
  wrappers, and how they fit together.
