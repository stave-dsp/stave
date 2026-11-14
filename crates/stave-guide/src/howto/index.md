# How-to guides

Task-oriented recipes for work around a plugin: driving the host tools,
laying out a project, measuring performance, voicing by ear, pinning a
model's sound with fingerprints, placing a
DSP model, capturing and fitting block models, designing a circuit from
its parts, cutting a release, and shipping a plugin to strangers.
Each assumes you know the [concepts](crate::concepts).

- [Use the host tools](crate::howto::host_tools): measure, validate and
  play any VST3 with `stave-host`, `stave-bench`, `stave-play` and
  `stave-events`.
- [Lay out a plugin project](crate::howto::project_layout): the shape,
  the tree rules, and the gates that hold them.
- [Local configuration](crate::howto::local_config): per-machine hosts,
  sample libraries and captures, building on another machine, and
  cleaning up after jobs and agents.
- [Gate what a change affects](crate::howto::affected): only the
  packages and gates a change can reach, and a tree never gated twice.
- [Measure performance](crate::howto::performance): the CPU budget
  and the perf check, honest timings, the CPU and cost taps, profiling.
- [Voicing and listening tests](crate::howto::voicing): loudness-matched
  A/B, blind ABX and rating trials in `stave-play`, session logs, and
  blessing a sound.
- [Fingerprints and perceptual metrics](crate::howto::fingerprints):
  pinning a model's measured character in CI, re-blessing it, and the
  NMR, ANMR, ERB-band error, Rnonlin and GedLee, and budgets stated as
  the null test's audibility.
- [Place a new DSP model](crate::howto::model_placement): where a model
  goes in stave-dsp, what it is called, and how it is verified.
- [Capture and fit a block model](crate::howto::block_models): the
  stimulus to play through a device, a Wiener–Hammerstein or polynomial
  Hammerstein fit with `stave-fit`, where block models fall short, and
  nonlinear kernels (Hammerstein, Volterra) as an analysis.
- [Design a circuit](crate::howto::design_tools): import a KiCad
  netlist, find the part that moves the response most, and the spread
  of built units across part tolerances.
- [Cut a release](crate::howto::release): the release script, semver
  checks, and moving plugin suites to a tag.
- [Ship your plugin](crate::howto::ship): the standalone app, one
  download per OS, Developer ID signing and notarisation, and the release
  workflow.
