//! The op-amp macromodel: one gain stage with a dominant pole, a slew
//! limit and rail clipping, in the spirit of Boyle, Cohn, Pederson and
//! Solomon's macromodel (IEEE JSSC SC-9(6), 1974) reduced to what a
//! datasheet's first page specifies.

use crate::circuit::devices::triode::softplus;
use crate::units::{Hz, Volts};

/// An op-amp from its datasheet's open-loop figures: DC gain,
/// gain-bandwidth, slew rate, input offset and output swing.
///
/// # Models
///
/// A voltage-feedback op-amp's large-signal open-loop behaviour: the
/// input stage's limited current charging the compensation capacitor
/// (slew limit), the dominant pole and finite DC gain (gain-bandwidth),
/// the input offset, and the output stage stopping short of each supply
/// rail. Input bias current, common-mode limits, output resistance and
/// current limit, noise and the second pole are not modelled.
///
/// # Derivation
///
/// An internal node `n` (to ground) carries the compensation capacitor
/// `C` = [`Self::COMPENSATION_FARADS`] and a conductance `G`; the input
/// stage drives it with a current, and the output is a unity buffer of
/// `v(n)`:
///
/// ```text
///     i(n) = Is·tanh(gm·(v₊ − v₋ + Vos)/Is) + G·Vmid
///            − Is·K·[softplus((v(n) − Vhi)/Vk) − softplus((Vlo − v(n))/Vk)]
///     gm = 2π·GBW·C      Is = SR·C      G = gm/A0
///     Vhi = V₊rail − headroom      Vlo = V₋rail + headroom      Vmid = (Vhi + Vlo)/2
/// ```
///
/// so the small-signal gain is `A0` at DC with one pole, crossing unity
/// at `GBW`; the output slews at most `SR` (the input stage's current
/// `Is` into `C`); and past a rail the clamp, `K` = [`Self::CLAMP`]
/// times the slew current per knee width `Vk` = [`Self::KNEE_VOLTS`],
/// holds `v(n)` there without winding up, as an output stage that
/// saturates does. The tanh is a bipolar or JFET differential pair's
/// shape; its linear range `Is/gm = SR/(2π·GBW)` follows from the two
/// datasheet figures. The deck `stave-test/spice/opamp_slew.cir` writes
/// these equations as ngspice behavioural sources.
///
/// # Verified
///
/// `crates/stave-test/tests/dk_matches_ngspice.rs::opamp_slew_and_clip_matches_ngspice`
/// (the JRC4558 slewing and clipping in a gain-of-11 stage at 20 kHz:
/// normalized RMS 2.6e-4 at 128×, budget 5e-4, converging as h² from 64×)
/// and `crates/stave-test/tests/dk_matches_ngspice.rs::tube_screamer_with_a_jrc4558_matches_ngspice`
/// (the TS808 clipping stage on a 9 V supply: 3.1e-5 at 16×, budget 6e-5),
/// each the element against ngspice's behavioural form of the same
/// equations;
/// `crates/stave-dsp/src/circuit/devices/opamp/tests.rs::a_follower_reads_back_the_datasheet_gain_and_bandwidth`
/// (every part's DC gain within 1% and its −3 dB point within 0.01 of
/// 1/√2 at the datasheet's GBW, through `Circuit<Dk>`).
///
/// Against the parts (DSP.md R13):
/// `crates/stave-test/tests/matches_datasheet.rs::opamps::tl072` and
/// `crates/stave-test/tests/matches_datasheet.rs::opamps::jrc4558` —
/// gain-bandwidth and the tables' slew rates read back from a follower;
/// the TL072's rising edge (sheet ~10 V/µs) and this revision's 20 V/µs
/// table figure, both open-loop plots and the JRC4558's full-power swing
/// are known gaps.
///
/// # Cost
///
/// The datasheet's `"Circuit<Dk> Tube Screamer (JRC4558)"`: a
/// two-control nonlinearity in the DK solve (the gain stage's input and
/// its node), beside the circuit's diodes.
///
/// # Aliasing
///
/// Rail clipping and slew limiting are hard nonlinearities: run the
/// circuit oversampled (the tests run 16×).
///
/// # Valid ranges
///
/// Outputs within the rails; inputs within the supplies (common-mode
/// limits and phase reversal are not modelled). Supplies are the
/// netlist element's, not the part's.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Macromodel {
    pub name: &'static str,
    /// DC differential gain, volts per volt.
    pub gain: f64,
    /// Unity-gain frequency.
    pub gain_bandwidth: Hz,
    /// Volts per second.
    pub slew_rate: f64,
    /// Input offset voltage, added to `v(+) − v(−)`.
    pub offset: Volts,
    /// How far short of each supply rail the output swings.
    pub headroom: Volts,
}

impl Macromodel {
    /// The internal compensation capacitor. Only the products `gm`, `Is`
    /// and `G` it scales are observable, so it is a fixed internal value.
    pub const COMPENSATION_FARADS: f64 = 1e-9;
    /// The rail clamp's knee width: the output stops within a few of these
    /// of its swing limit.
    pub const KNEE_VOLTS: f64 = 0.02;
    /// The rail clamp's strength, in slew currents per knee width.
    pub const CLAMP: f64 = 10.0;

    /// Texas Instruments TL072, SLOS080J (March 2005), typical at ±15 V,
    /// 25 °C: unity-gain bandwidth 3 MHz, slew rate 13 V/µs, AVD 200 V/mV,
    /// VIO 3 mV, VOM ±13.5 V into 10 kΩ (headroom 1.5 V). Later revisions
    /// (SLOS080W, 2025) list a newer die at 5.25 MHz and 20 V/µs; these
    /// are the classic part's.
    pub const TL072: Self = Self {
        name: "TL072",
        gain: 200e3,
        gain_bandwidth: Hz::new(3e6),
        slew_rate: 13e6,
        offset: Volts::new(3e-3),
        headroom: Volts::new(1.5),
    };

    /// New Japan Radio NJM4558 (the JRC4558D of the Tube Screamer),
    /// Nisshinbo datasheet NJM4558_E, typical at ±15 V, 25 °C: GB 3 MHz,
    /// SR 1 V/µs, Av 100 dB, VIO 0.5 mV, VOM ±14 V into 10 kΩ (headroom
    /// 1 V).
    pub const JRC4558: Self = Self {
        name: "JRC4558",
        gain: 100e3,
        gain_bandwidth: Hz::new(3e6),
        slew_rate: 1e6,
        offset: Volts::new(0.5e-3),
        headroom: Volts::new(1.0),
    };

    /// National Semiconductor LM308 (LM108/LM208/LM308 datasheet,
    /// December 1994) compensated by 30 pF, as in the ProCo RAT: AVOL
    /// 300 V/mV, VOS 2 mV, output ±14 V into 10 kΩ (headroom 1 V) from the
    /// tables; the table gives no slew rate or bandwidth, so those are
    /// read from its Cf = 30 pF plots — about 0.3 V/µs (the
    /// voltage-follower pulse response) and about 0.4 MHz (the open-loop
    /// gain, 52 dB at 1 kHz). Both scale as 1/Cf.
    pub const LM308: Self = Self {
        name: "LM308",
        gain: 300e3,
        gain_bandwidth: Hz::new(0.4e6),
        slew_rate: 0.3e6,
        offset: Volts::new(2e-3),
        headroom: Volts::new(1.0),
    };

    pub const ALL: [Self; 3] = [Self::TL072, Self::JRC4558, Self::LM308];

    /// The constants the gain stage runs on between supply rails
    /// `negative` and `positive` volts.
    pub fn prepare(&self, negative: f64, positive: f64) -> Prepared {
        let c = Self::COMPENSATION_FARADS;
        let gm = std::f64::consts::TAU * f64::from(self.gain_bandwidth.get()) * c;
        let slew_amps = self.slew_rate * c;
        let headroom = self.headroom.get();
        let (low, high) = (negative + headroom, positive - headroom);
        Prepared {
            gm,
            slew_amps,
            leak: gm / self.gain,
            offset: self.offset.get(),
            low,
            high,
            middle: 0.5 * (low + high),
        }
    }
}

/// A [`Macromodel`] between its supplies: the gain stage's constants, as
/// a circuit solver evaluates it every iterate.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Prepared {
    /// Input transconductance, siemens.
    pub gm: f64,
    /// The input stage's largest current, amps.
    pub slew_amps: f64,
    /// The conductance from the internal node to ground that sets the DC
    /// gain, siemens.
    pub leak: f64,
    pub offset: f64,
    /// The output's swing limits, volts.
    pub low: f64,
    pub high: f64,
    pub middle: f64,
}

/// The current into the internal node and its partials.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct StageCurrent {
    pub amps: f64,
    /// ∂i/∂(v₊ − v₋).
    pub per_input_volt: f64,
    /// ∂i/∂v(n).
    pub per_node_volt: f64,
}

impl Prepared {
    /// The current into the internal node at differential input
    /// `input` and node voltage `node` (the leak conductance excluded:
    /// it is a linear element of the network).
    #[inline]
    pub fn current(&self, input: f64, node: f64) -> StageCurrent {
        let is = self.slew_amps;
        let t = (self.gm * (input + self.offset) / is).tanh();
        let k = Macromodel::CLAMP * is;
        let vk = Macromodel::KNEE_VOLTS;
        let (up, up_slope) = softplus((node - self.high) / vk);
        let (down, down_slope) = softplus((self.low - node) / vk);
        StageCurrent {
            amps: is * t + self.leak * self.middle - k * (up - down),
            per_input_volt: self.gm * (1.0 - t * t),
            per_node_volt: -k * (up_slope + down_slope) / vk,
        }
    }
}
