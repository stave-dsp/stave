# Design a circuit: schematics, sensitivities and tolerances

A circuit drawn in a schematic editor runs in stave as it is drawn, and
stave can then answer the questions a designer asks before ordering
parts: which part moves the tone most, how far apart two built units
will sound, and which part's grade is worth paying for. This page
covers the three tools (DSP.md G1–G3): importing a KiCad netlist,
sensitivity studies, and Monte Carlo over tolerances.

## Import a KiCad netlist

Export the schematic's netlist from KiCad (*File → Export → Netlist…*,
the *KiCad* format, a `.net` file) and read it with
[`Netlist::parse_kicad`](stave_dsp::circuit::netlist::Netlist::parse_kicad),
or with [`Import`](stave_dsp::circuit::netlist::kicad::Import) when
parts name SPICE libraries or the circuit needs analyses. Each part's
simulation model comes from the fields KiCad's simulator keeps on the
symbol (`Sim.Device`, `Sim.Type`, `Sim.Params`, `Sim.Pins`,
`Sim.Library`, `Sim.Name`); resistors, capacitors and inductors with
none are inferred from their value, as KiCad infers them. Values are
KiCad's notation (`4k7`, `1M` is mega, `100n`).

```
use stave_dsp::circuit::netlist::kicad::Import;

let net = r#"(export (version "E")
  (components
    (comp (ref "V1") (value "SIN")
      (property (name "Sim.Device") (value "V"))
      (property (name "Sim.Type") (value "SIN"))
      (property (name "Sim.Params") (value "dc=0 ampl=1 f=1k ac=1")))
    (comp (ref "R1") (value "4k7"))
    (comp (ref "D1") (value "1N4148")
      (property (name "Sim.Device") (value "D"))
      (property (name "Sim.Library") (value "models/diodes.lib"))
      (property (name "Sim.Name") (value "D1N4148"))
      (property (name "Sim.Pins") (value "1=K 2=A"))))
  (nets
    (net (code "1") (name "/in") (node (ref "V1") (pin "1")) (node (ref "R1") (pin "1")))
    (net (code "2") (name "/out") (node (ref "R1") (pin "2")) (node (ref "D1") (pin "2")))
    (net (code "3") (name "GND") (node (ref "V1") (pin "2")) (node (ref "D1") (pin "1")))))"#;
let clipper = Import::new(net)
    .library("diodes.lib", ".model D1N4148 D(Is=2.52n N=1.752)")
    .directives(".tran 10u 2m\n.control\nwrdata out v(out)\n.endc")
    .netlist()
    .unwrap();
assert_eq!(clipper.elements().len(), 3);
assert!(clipper.probe.is_some());
```

Net names become node names (`/out` is `out`, `GND` is ground). A part
stave cannot run — a subcircuit, an XSPICE model, a VBIC transistor, a
pulse source — is an error naming its reference, never a silent
omission. `crates/stave-test/tests/kicad_import.rs` holds KiCad
netlists of the diode clipper and the Fuzz Face to their ngspice decks:
the same elements on the same nodes, and the same output bit for bit.

## Which part moves the response most

A study runs a [`Design`](stave_measure::design::Design): a model and
the parameters it is built from. A netlist's design is
[`Parts`](stave_measure::design::Parts) (its resistors, capacitors,
inductors and pots); any other model — a hand topology, any `Stage`, a
closure — is a [`Parameters`](stave_measure::design::Parameters).
[`sensitivity`](stave_measure::design::sensitivity) reports, for each
part and each [`Quantity`](stave_measure::design::Quantity) (small-signal
response or phase at a frequency, a sine's gain, its THD), the change
per 1 % change of the part's value:

```
use stave_dsp::circuit::netlist::tolerance::Tolerances;
use stave_dsp::circuit::netlist::Netlist;
use stave_measure::design::{sensitivity, Bench, Parts, Quantity};

let rlc = Netlist::parse_spice(
    "* series RLC\nV1 in 0 AC 1\nR1 in a 100\nL1 a out 10m\nC1 out 0 1u\n.control\nwrdata o v(out)\n.endc",
)
.unwrap();
let design = Parts::new(&rlc, &Tolerances::new());
let at = [Quantity::ResponseDb { hz: 1591.5 }, Quantity::PhaseDegrees { hz: 1591.5 }];
let s = sensitivity(&design, &at, &Bench::default()).unwrap();
// At resonance the peak is set by R alone: −0.087 dB per % of R.
assert!((s.per_percent("r1", 0).unwrap() + 0.0869).abs() < 1e-3);
// L and C swing the phase there (0.57° per %), so R ranks last.
assert_eq!(s.ranked().last().unwrap().0, "r1");
```

[`sweep`](stave_measure::design::sweep) runs a design over a grid of
values instead (every combination of the axes given). Small-signal
quantities of a circuit come from its linearisation at the operating
point, exactly; anything else is measured with coherent sines on the
[`Bench`](stave_measure::design::Bench). A study runs on the bench's
`threads` (one by default; stave-test's `threads()` gives the cap
`RAYON_NUM_THREADS` or `STAVE_OFFLOAD_JOBS` sets), and its results never
depend on how many.

## How far apart built units are

Give each part a grade with
[`Tolerances`](stave_dsp::circuit::netlist::tolerance::Tolerances) —
per kind, or per part; a pot's track is one part — and
[`monte_carlo`](stave_measure::design::monte_carlo) builds a batch of
units, each part drawn from its grade by a seeded draw (the same seed
builds the same units on every machine), and measures each one. The
[`Spread`](stave_measure::design::Spread) gives percentiles across the
units, and its advice ranks the parts by the share of the spread their
grade causes:

```
use stave_dsp::circuit::netlist::tolerance::{Tolerance, Tolerances};
use stave_dsp::circuit::netlist::Netlist;
use stave_measure::design::{monte_carlo, Bench, Parts, Quantity};

let rc = Netlist::parse_spice(
    "* rc\nV1 in 0 AC 1\nR1 in out 1k\nC1 out 0 100n\n.control\nwrdata o v(out)\n.endc",
)
.unwrap();
let grades = Tolerances::new()
    .resistors(Tolerance::uniform(0.01))
    .capacitors(Tolerance::uniform(0.10));
let corner = Quantity::ResponseDb { hz: 1591.5 };
let spread = monte_carlo(&Parts::new(&rc, &grades), &[corner], 200, 1, &Bench::default()).unwrap();
let (low, high) = (spread.percentile(0, 5.0), spread.percentile(0, 95.0));
assert!(low < -3.01 && high > -3.01, "the corner's −3 dB sits inside the band");
// The 10 % capacitor causes almost all of it: buy a better capacitor.
assert_eq!(spread.advice[0].parameter, "c1");
assert!(spread.advice[0].share > 0.98);
```

The same draws build a unit as a netlist with
[`Tolerances::unit`](stave_dsp::circuit::netlist::tolerance::Tolerances::unit),
to run, listen to, or characterise like any other circuit.

## On the docs site

`crates/stave-test/tests/design_studies.rs` runs a sensitivity study
and a Monte Carlo batch of the Fender AB763 tone stack and the diode
clipper, commits them as `crates/stave-test/design/*.design`, and fails
CI when either moves (re-bless on purpose with `STAVE_BLESS=1`). The
reference's *Design studies* page draws them: the response band across
built units, each part's sensitivity across the audio band, the grade
that dominates, and the THD spread.
