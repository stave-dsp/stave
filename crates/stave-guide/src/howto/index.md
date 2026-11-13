# How-to guides

Task-oriented recipes for work around a plugin: driving the host tools,
laying out a project, measuring performance, placing a DSP model, and
cutting a release.
Each assumes you know the [concepts](crate::concepts).

- [Use the host tools](crate::howto::host_tools): measure, validate and
  play any VST3 with `stave-host`, `stave-bench`, `stave-play` and
  `stave-events`.
- [Lay out a plugin project](crate::howto::project_layout): the shape,
  the tree rules, and the gates that hold them.
- [Local configuration](crate::howto::local_config): per-machine hosts,
  sample libraries and captures, and building on another machine.
- [Measure performance](crate::howto::performance): the perf report,
  honest timings, the CPU tap, profiling.
- [Place a new DSP model](crate::howto::model_placement): where a model
  goes in stave-dsp, what it is called, and how it is verified.
- [Cut a release](crate::howto::release): the release script, semver
  checks, and moving plugin suites to a tag.
