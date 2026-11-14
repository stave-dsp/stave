# stave

A from-scratch Rust framework for VST3, CLAP and Audio Unit plugins, and a
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
- `examples/pluck` — Stave Pluck, a plucked-string instrument: the worked
  example of an instrument.
- `tools/` — `stave-bench` (headless host, the agent API), `stave-play`
  (windowed host), the storybook, and the docs site generator.
- `crates/stave-guide` — the guide's articles, in Markdown.
- `OVERHAUL.md` — the project's working ledger.
- `DSP.md` — the DSP roadmap: models, verification loops and profiling.
- `FRAMEWORK.md` — the framework roadmap: formats, hosts, shipping, GUI and
  MIDI, and where stave stands against JUCE.

The plugins built on Stave live in
[stave-plugins](https://github.com/stave-dsp/stave-plugins).

## Quick start

```sh
bash scripts/install.sh stave-tilt-vst3 release   # build, bundle, validate, install Tilt
cargo run -p stave-tilt-vst3 --features standalone # Tilt as its own app (Tab: audio settings)
scripts/package.sh stave-tilt-vst3                 # the signed download strangers install
scripts/gates.sh                                  # what every commit runs
scripts/affected.sh                               # only the gates your change affects
scripts/docsite.sh                                # the docs site, into target/site
```

## License

MIT, see [LICENSE](LICENSE).
