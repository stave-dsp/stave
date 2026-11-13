//! An optical compressor's gain cell: a divider whose lower leg is a
//! vactrol's cell, the cell lit by the output it divides (Najnudel et
//! al., DAFx-23, Fig. 8).

use super::netlist_of;
use crate::circuit::devices::diode::Shockley;
use crate::circuit::devices::photocell::{Najnudel, Vactrol};
use crate::circuit::methods::dk::Dk;
use crate::circuit::netlist::{ElementKind, Netlist, Node, Variable};
use crate::circuit::{Circuit, Smoothing, VariableResistor};
use crate::kernel::node::{Stage, Tail};
use crate::units::Ohms;

/// The gain cell of Najnudel et al.'s minimal optical compressor
/// (DAFx-23, Fig. 8a, Table 2): source `vin` at node `in`, `R1` = 1 kΩ
/// from `in` to `out`, and the vactrol's cell `RLDR` from `out` to
/// ground, declared a variable resistor (written at the VTL5C3's 10 MΩ
/// dark resistance). Probe `v(out)`.
///
/// # Models
/// A shunt opto attenuator, the audio half of an opto compressor: the
/// T4-style cell of an LA-2A in its simplest form, a series resistor
/// into a light-dependent resistor to ground (the LA-2A puts its T4B's
/// cell across the program the same way, behind a 68 kΩ series leg).
///
/// # Derivation
/// The paper's Fig. 8a, cut at the voltage follower's input (an ideal
/// follower draws no current, so the cut is exact); the other half is
/// [`opto_compressor_side_chain`]. Values from its Table 2.
///
/// # Verified
/// `crates/stave-test/tests/photocell_matches_datasheet.rs::opto_compressor_attacks_faster_and_releases_slower_when_driven_harder`
/// (the paper's Fig. 9 claims, measured) and
/// `crates/stave-test/tests/photocell_matches_datasheet.rs::opto_compressor_netlists_solve_as_the_closed_forms`
/// (the divider and the side chain through `Circuit<Dk>` against their
/// closed forms).
///
/// # Cost
/// Datasheet `"OptoCompressor (VTL5C3)"`, the whole compressor.
///
/// # Aliasing
/// None of its own: a divider; the gain moves at the cell's speed.
///
/// # Valid ranges
/// Any voltage (the cell is linear in voltage; the VTL5C3 is rated to
/// 250 V).
pub fn opto_compressor_cell() -> Netlist {
    let mut netlist = netlist_of(
        "opto compressor gain cell (Najnudel et al. DAFx-23 Fig. 8)
VIN in 0 0
R1 in out 1k
RLDR out 0 10meg
.control
wrdata out v(out)
.endc",
    );
    if let Err(e) = netlist.add_variable(Variable::resistor("rldr")) {
        panic!("{e}");
    }
    netlist
}

/// The side chain of Najnudel et al.'s minimal optical compressor
/// (DAFx-23, Fig. 8a, Table 2): source `vout` (the gain cell's output)
/// at node `s`, an ideal follower `E1` to `vo`, `R2` = 5 Ω into the LED
/// node `led`, `C1` = 4.7 µF across the LED, the LED as its bulk
/// resistance `RS` (the `led` card's `series_ohms`) and junction `D1`
/// (the card without it). Probe `v(led, j)`: the LED current times `RS`.
///
/// # Models
/// The LED drive of an opto compressor: the output, buffered, lighting
/// the vactrol's LED through a small resistor, a capacitor holding the
/// LED's voltage between peaks.
///
/// # Derivation
/// The paper's Fig. 8a from the follower on; the follower is ideal (its
/// supplies infinite, as the paper treats them). The LED is the part's
/// card, its bulk resistance drawn as a resistor so the probe reads the
/// LED's own current.
///
/// # Verified
/// `crates/stave-test/tests/photocell_matches_datasheet.rs::opto_compressor_netlists_solve_as_the_closed_forms`
/// (held DC drives against [`LedDriver`](crate::circuit::devices::photocell::LedDriver)'s closed form).
///
/// # Cost
/// Datasheet `"OptoCompressor (VTL5C3)"`, the whole compressor.
///
/// # Aliasing
/// The LED is a rectifier: its current carries harmonics, but it only
/// lights the cell, which follows at its own (millisecond) speed.
///
/// # Valid ranges
/// The part's LED rating (VTL5C3 40 mA, NSL-32 25 mA); the paper's
/// 6 and 12 V bursts drive the 5 Ω leg past it, as its simulation did.
pub fn opto_compressor_side_chain(led: Shockley) -> Netlist {
    let mut netlist = netlist_of(&format!(
        "opto compressor side chain (Najnudel et al. DAFx-23 Fig. 8)
VOUT s 0 0
E1 vo 0 s 0 1
R2 vo led 5
C1 led 0 4.7u
RS led j {}
.control
wrdata out v(led, j)
.endc",
        led.series_ohms
    ));
    let junction = ElementKind::Diode {
        anode: node(&netlist, "j"),
        cathode: Node::GROUND,
        diode: Shockley {
            series_ohms: 0.0,
            ..led
        },
    };
    if let Err(e) = netlist.add("d1", junction) {
        panic!("{e}");
    }
    netlist
}

fn node(netlist: &Netlist, name: &str) -> Node {
    netlist
        .node(name)
        .unwrap_or_else(|| panic!("a topology names a node it does not have: {name}"))
}

/// Najnudel et al.'s minimal optical compressor, running: the gain cell
/// ([`opto_compressor_cell`]) and side chain ([`opto_compressor_side_chain`])
/// through `Circuit<Dk>`, joined by a [`Vactrol`]: each sample the cell
/// divides the input, the side chain lights the LED from the output,
/// and the vactrol sets the cell's resistance for the next sample.
/// Volts in, volts out.
///
/// ```
/// use stave_dsp::circuit::devices::photocell::Najnudel;
/// use stave_dsp::circuit::topologies::opto_compressor::OptoCompressor;
/// use stave_dsp::Stage;
///
/// let mut comp = OptoCompressor::new(Najnudel::VTL5C3);
/// comp.prepare(48_000.0);
/// let sine = |n: usize, volts: f64| volts * (std::f64::consts::TAU * 1e3 * n as f64 / 48e3).sin();
/// // 1 V passes (the LED is dark below its 1.5 V knee) …
/// let quiet = (0..4_800).map(|n| comp.tick_f64(sine(n, 1.0)).abs()).fold(0.0, f64::max);
/// // … 6 V is held down once the cell has turned on.
/// let loud = (4_800..24_000).map(|n| comp.tick_f64(sine(n, 6.0)).abs()).skip(9_600).fold(0.0, f64::max);
/// assert!(quiet > 0.99 && loud < 3.0, "{quiet} {loud}");
/// ```
///
/// # Models
/// An opto compressor's gain cell and side chain (Najnudel et al.,
/// DAFx-23, Fig. 8): feedback compression whose attack shortens and
/// release lengthens with level, as the cell's carriers make them.
///
/// # Derivation
/// Two netlists through `Circuit<Dk>` (the follower makes the cut
/// exact) and the part's [`Vactrol`]; the optical path closes one
/// sample late (20 µs at 48 kHz against the cell's milliseconds). The
/// cell resistor glides with [`Smoothing::NONE`]: the vactrol is its own
/// smoothing.
///
/// # Verified
/// `crates/stave-test/tests/photocell_matches_datasheet.rs::opto_compressor_attacks_faster_and_releases_slower_when_driven_harder`.
///
/// # Cost
/// Datasheet `"OptoCompressor (VTL5C3)"`.
///
/// # Aliasing
/// None of its own; the gain moves at the cell's speed.
///
/// # Valid ranges
/// As [`opto_compressor_side_chain`]: the part's LED rating.
#[derive(Clone, Debug)]
pub struct OptoCompressor {
    cell: Circuit<Dk>,
    side_chain: Circuit<Dk>,
    ldr: VariableResistor,
    vactrol: Vactrol,
    sense_ohms: f64,
    led_amps: f64,
}

impl OptoCompressor {
    pub fn new(part: Najnudel) -> Self {
        let build = |netlist: &Netlist, source: &str| {
            let mut c = Circuit::<Dk>::new(netlist).unwrap_or_else(|e| panic!("{e}"));
            c.drive(source).unwrap_or_else(|e| panic!("{e}"));
            c
        };
        let mut cell = build(&opto_compressor_cell(), "vin");
        cell.set_smoothing(Smoothing::NONE);
        let ldr = cell
            .variable_resistor("rldr")
            .unwrap_or_else(|e| panic!("{e}"));
        let side_chain = build(&opto_compressor_side_chain(part.led), "vout");
        let mut comp = Self {
            cell,
            side_chain,
            ldr,
            vactrol: Vactrol::new(part),
            sense_ohms: part.led.series_ohms,
            led_amps: 0.0,
        };
        comp.cell.set_ohms(comp.ldr, Ohms::new(part.cell.dark_ohms));
        comp.cell.settle();
        comp
    }

    /// The vactrol, for its state (resistance, light history).
    pub fn vactrol(&self) -> &Vactrol {
        &self.vactrol
    }

    pub fn vactrol_mut(&mut self) -> &mut Vactrol {
        &mut self.vactrol
    }

    /// The LED current of the last sample, amperes.
    pub fn led_amps(&self) -> f64 {
        self.led_amps
    }

    /// One sample, volts in, volts out.
    pub fn tick_f64(&mut self, volts: f64) -> f64 {
        let out = self.cell.tick_f64(volts);
        self.led_amps = (self.side_chain.tick_f64(out) / self.sense_ohms).max(0.0);
        let ohms = self.vactrol.step(self.led_amps);
        self.cell.set_ohms(self.ldr, Ohms::new(ohms));
        out
    }
}

impl Stage for OptoCompressor {
    fn prepare(&mut self, sample_rate: f32) {
        self.cell.prepare(sample_rate);
        self.cell.set_smoothing(Smoothing::NONE);
        self.side_chain.prepare(sample_rate);
        self.vactrol.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.vactrol.reset();
        self.cell.set_ohms(self.ldr, Ohms::new(self.vactrol.ohms()));
        self.cell.settle();
        self.cell.reset();
        self.side_chain.reset();
        self.led_amps = 0.0;
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_f64(f64::from(x)) as f32
    }
    fn tail(&self) -> Tail {
        self.side_chain.tail()
    }
}
