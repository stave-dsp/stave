# Lay out a plugin project

A plugin project has one shape: a crate that depends on `stave`, a root
file that narrates the whole plugin, components that own their state,
and a colocated test suite. This page gives the shape, the rules that
keep it, and which rung of the enforcement ladder holds each rule.

## The shape

`examples/tilt` is the worked example: params, processing and face in
one file. Copy its shape, never its DSP.

```text
<name>/
  Cargo.toml            [lib] crate-type = ["cdylib", "rlib"]
                        [dependencies] stave
                        [dev-dependencies] stave-test
  src/
    lib.rs              the root: params, the Plugin impl, stave::export!
                        (a small plugin is this one file, as Tilt is)
    params.rs           when the table grows: ids are forever
    face.rs             when the face grows: a pure function of the params
    presets.rs          factory presets, a pure function of the params
    dsp/                a bigger plugin: a directory of components
      mod.rs            the component struct and its process(),
                        the orchestration in named phases
      strip.rs          one file per component, named for the
      modulation.rs     interface it defines
      taps.rs           the #[derive(Taps)] table
  tests/
    checks.rs           stave_test::plugin_checks!, a file of its own
    properties.rs       the promises only this plugin can state
    editor.rs           the face's own promises, through FaceDriver
    golden.rs           pictures, when the look matters
```

The `cdylib` is what a DAW loads; the `rlib` is what the tests link.
The crate depends on `stave` alone and starts with
`use stave::prelude::*;`. If a plugin needs something the prelude lacks,
that is a framework gap to close, not an import to add. In
[stave-plugins](https://github.com/stave-dsp/stave-plugins),
`scripts/new-plugin.sh <name>` stamps a complete project in this shape.

## The tree rules

- **The root narrates.** A reader who opens `lib.rs` sees every file
  and the [`Plugin`](stave::Plugin) impl before opening anything else.
- **Components, not scattered `impl` blocks.** A bigger plugin splits
  into types that each own their state. A pass is a function whose
  signature is its access list: what it may touch is its arguments, not
  a reach through `self`. [`process`](stave::Plugin::process) is the one
  place the data flow is visible.
- **State is private to its file.** Siblings never reach into each
  other's fields; ports are methods, dependencies are arguments. A
  constant lives beside the structure that knows it, not in a
  grab-bag module. Plain data records that passes operate on may stay
  open; the rule is for behaviour.
- **File names follow the interfaces they define.** A name guides
  navigation; it never replaces a concrete type or function that makes
  the composition obvious.
- **Names are the glossary's words.** `primary`, `voicing`, `post`, not
  `stage_a` or `filt1`; the audio block is processed, never drawn (that
  word belongs to pixels); taps are a `#[derive(Taps)]` table built in
  [`Plugin::new`](stave::Plugin::new); driver lanes and detector feeds
  are named constants, never bare indices. See the
  [glossary](crate::glossary).
- **No history in comments.** Milestone tags and "step 3" notes are
  provenance; commits and the changelog own that.

**The growth rule.** The single-file plugin is a starting point, not a
destination. When a second concept arrives (a modulator joins a delay,
a splitter joins a compressor), the processing becomes `dsp/`
components that day: drift happens in the gap between "small enough"
and "should have split already".

Project-specific tools and tests live in the project, never in the
framework's `tools/` or `crates/stave-test/tests/`.

## The enforcement ladder

A rule lives on the strongest rung that can hold it, and moves up when
a stronger one opens.

1. **Types.** The failure cannot be written:
   [`Loop`](stave_dsp::Loop) demands a `NonZeroU32` delay;
   [`PerChannel`](stave_dsp::PerChannel) lanes cannot cross;
   [`value()`](stave::params::FloatParam::value) cannot drift from its
   declaration; private component fields make reaching into another
   component a compile error.
2. **Property tests.** The promise is derived and fails the build (see
   [testing](crate::concepts::testing)).
3. **Gates.** Scripts in `scripts/gates.sh` and CI, for what Rust cannot
   express: comment content, file organisation, the choice between two
   type-identical APIs.

The gates that bind a project's tree:

- `scripts/check-plugin-rules.sh`, over `examples/` (and the tree for
  the second check), fails on:
  1. one type's `impl` blocks distributed across several files of a
     project (a check written for the file layout that predates
     [`Plugin`](stave::Plugin); it matches the old type names and
     finds nothing in today's example);
  2. milestone tags such as `(K3)`, `(G6)`, `(Step 5)` or `(ABI 8)` in
     any `.rs` file under `examples/`, `crates/` or `tools/`;
  3. `plain() as f32` in an example's source: a cast beside a read
     restates the declaration;

  and prints, without failing, a note when a processing file under that
  same pre-`Plugin` name passes 200 lines.
- `scripts/check-vocabulary.sh` rejects the retired names everywhere
  and the reserved words on the author surface (see the
  [glossary](crate::glossary)).
- `scripts/check-view-bindings.sh` rejects name-keyed telemetry
  (`probe!`) in an example's source: publish through a declared
  [`Tap`](stave::Tap).
- `scripts/check-file-sizes.sh` prints a signal for source files over
  1000 lines (tests over 1600). Size is a proxy for mixed concerns,
  never a failure: the split decision is human.

Every gate exists because its violation once happened. A convention
without at least a gate is a deferred failure.
