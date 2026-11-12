# stave

A from-scratch Rust framework for VST3 and CLAP audio plugins, and a
transparent host for analysing them. No JUCE, no `nih-plug`, no C or C++
SDKs: the ABIs are written by hand, and testing, measurement and
observability are built into every layer.

**Documentation: <https://stave-dsp.github.io/stave/>** — getting started,
tutorials, concepts, how-tos, the part catalogue, and the API of every
crate. The site is built from this repository on every push to `main`:
its articles are doctests of the `stave-guide` crate, its reference pages
are generated from the code, and nothing on it is written twice.

## In this repository

- `crates/` — the framework. A plugin depends on `stave` alone.
- `examples/tilt` — Stave Tilt, a one-knob tilt EQ: the worked example.
- `tools/` — `stave-bench` (headless host, the agent API), `stave-play`
  (windowed host), the storybook, and the docs site generator.
- `crates/stave-guide` — the guide's articles, in Markdown.

The plugins built on Stave live in
[stave-plugins](https://github.com/stave-dsp/stave-plugins).

## Quick start

```sh
bash scripts/install.sh stave-tilt-vst3 release   # build, bundle, validate, install Tilt
scripts/gates.sh                                  # what every commit runs
scripts/docsite.sh                                # the docs site, into target/site
```

## License

MIT, see [LICENSE](LICENSE).
