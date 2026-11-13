# Where does my model go?

Before you write a new model in [`stave_dsp`], decide five things in
order: its **claim**, its **layer**, its **folder**, its **name** and its
**verification**. Each answer follows from the one before, so a model
placed this way is easy to find and comes with its test. The rules
behind this page are in `DSP.md` (Architecture, naming rules 1–7).

## The decision tree

```text
1. Claim: what does the model say about the world?
   ├─ it is a quantity (a frequency, a level, a resistance) ... units
   ├─ it is the Stage contract or a way to wire stages ........ kernel
   ├─ it is textbook linear DSP (filter, delay, FFT, osc) ..... primitives
   ├─ it is a shape, chosen or fitted, with no hardware claim . behavioral
   ├─ it is derived from a schematic .......................... circuit
   ├─ it is derived from mechanics or acoustics ............... physical
   ├─ it was trained from captures of real hardware ........... learned
   ├─ it is an algorithmic design that models no hardware ..... effects
   └─ it is a composition of other stages ..................... structures
2. Layer: the claim above. A layer uses only the layers before it
   (scripts/check-view-bindings.sh enforces this).
3. Folder: inside circuit,
   ├─ one device's published equation ......... circuit::devices::<family>
   ├─ a way to solve any circuit .............. circuit::methods::<method>
   └─ a named circuit (a netlist) ............. circuit::topologies
   Everywhere else: one concept per file, tests beside it (foo.rs + foo/tests.rs).
4. Name: what it models, never how it is built.
5. Verification: the layer's reference, and a test that fails when
   the model drifts from it.
```

## One diode, four places

A "diode" can live in four places. The path shows which claim each
one makes:

```rust
use stave_dsp::behavioral::shapes::knee;
use stave_dsp::circuit::devices::diode::Shockley;
use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::topologies::diode_clipper::{DiodeClipper, DiodeClipperConfig, PairSolver};
use stave_dsp::circuit::{Circuit, Netlist};
use stave_dsp::learned::{nam, LoadError};
use stave_dsp::Stage;

// circuit::devices: the junction equation of a device model; a part
// is a constant of it, named by part number.
let d = Shockley::D1N4148;
assert!(d.current(0.7) > 1e-3);

// circuit::topologies: a named circuit that uses the device, held to
// ngspice on the same netlist.
let mut clipper = DiodeClipper::new(DiodeClipperConfig {
    input_ohms: 2.2e3,
    capacitor_farads: 10e-9,
    diode: Shockley::D1N4148,
    solver: PairSolver::default(),
});
clipper.prepare(48_000.0);
assert!(clipper.tick(10.0).abs() < 1.0); // clips near a diode drop

// The same netlist, solved by a method chosen as a type parameter.
let deck = "diode clipper
V1 in 0 0
R1 in out 2.2k
C1 out 0 10n
D1 out 0 DI
D2 0 out DI
.model DI D(Is=2.52n N=1.752)
.control
wrdata out v(out)
.endc";
let mut general = Circuit::<Dk>::new(&Netlist::parse_spice(deck).unwrap()).unwrap();
general.drive("v1").unwrap();
general.prepare(48_000.0);
let _ = general.tick(10.0);

// behavioral::shapes: a curve a designer chooses instead. It claims
// no diode, so it is named by its maths.
let soft = knee(2.4);
assert!((soft(0.0)).abs() < 1e-12);

// learned: a network trained on captures of a real clipper pedal.
fn load_capture(text: &str) -> Result<nam::Nam, LoadError> {
    nam::parse(text) // off the audio thread; hand the network over
}
let _ = load_capture;
```

## Naming

Name the thing, not how it is built. These rules come from `DSP.md`;
`scripts/check-vocabulary.sh` rejects rule 1 in stave-dsp's public API.

1. No implementation word (`Model`, `Impl`, `Helper`, `V2`, `New`,
   `Simple`, `Fast` and the rest of the [glossary](crate::glossary)'s
   list) as a prefix or suffix.
2. The approach goes in the path or a type parameter, never in a
   compound name: `devices::diode::Shockley`, not `ShockleyDiodeModel`;
   `Circuit::<Dk>`, not `DkCircuit`.
3. A published model is named after its model or its authors
   (`Koren`, `Dempwolf`, `JilesAtherton`, `LinkwitzRiley4`). A part is
   named after its part number (`Shockley::D1N4148`). A shape is named
   after its maths (`knee`, `fold`, `chebyshev`).

## Verification and the model catalogue

Each layer has its own kind of reference: ngspice on the same netlist
for `circuit`, high-precision solutions or published modes for
`physical`, the reference implementation's output for `learned`, and
analytic properties for `behavioral`, `primitives` and `effects`. Every
public model's rustdoc has the same six sections, and the docs site's
model catalogue is generated from them:

```rust
use stave_dsp::{Stage, Tail};

/// A one-pole lowpass whose corner is fixed at build time.
///
/// # Models
/// The RC lowpass, `y' = (x − y) / τ`; no hardware beyond it.
///
/// # Derivation
/// Textbook: impulse invariance, `a = 1 − exp(−1 / (τ · sample_rate))`.
///
/// # Verified
/// `crates/my-crate/src/smoother.rs::step_reaches_63_percent_in_tau`:
/// within 1e-6 of the analytic step response.
///
/// # Cost
/// One multiply-add per sample; no datasheet line yet.
///
/// # Aliasing
/// None: linear and time-invariant.
///
/// # Valid ranges
/// `τ` > 0; any sample rate.
pub struct Smoother { a: f32, y: f32 }

impl Stage for Smoother {
    fn prepare(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) { self.y = 0.0; }
    fn tick(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
    fn tail(&self) -> Tail { Tail::Infinite }
}
# let mut s = Smoother { a: 0.5, y: 0.0 };
# assert_eq!(s.tick(1.0), 0.5);
```

The catalogue lists each model with its layer, derivation, the test
named in *Verified* (linked to its source), cost and aliasing. The
docs build fails when a model in stave-dsp is missing a section, or
when *Verified* names a test that is not a `#[test]` in the workspace.
Write the test as `crates/<crate>/<path>.rs::<test_name>`. In *Cost*,
quote the model's stage in the datasheet (`crates/stave-dsp/datasheet.json`)
with its double quotes inside the backticks, such as `"Slew"`; the
catalogue shows the committed ns/sample beside it, and fails on a stage
the datasheet does not measure. Don't type the number into the docs: it
would drift.
