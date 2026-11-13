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
///     i(n) = I±·tanh(gm·(v₊ − v₋ + Vos)/I±) + G·Vmid
///            − Is·K·[softplus((v(n) − Vhi)/Vk) − softplus((Vlo − v(n))/Vk)]
///     gm = 2π·GBW·C      Is = I+ = SR·C      I− = SR↓·C      G = gm/A0
///     Vhi = V₊rail − headroom      Vlo = V₋rail + negative_headroom      Vmid = (Vhi + Vlo)/2
/// ```
///
/// so the small-signal gain is `A0` at DC with one pole, crossing unity
/// at `GBW`; the output slews at most `SR` rising (the input stage's
/// current `I+` into `C`) and `SR↓` falling (`I−`, the branch taken when
/// the tanh's argument is negative: an input stage that sinks more than
/// it sources, as the TL072's drawn pulse shows; for a symmetric part
/// the two are one tanh); and past a rail the clamp, `K` = [`Self::CLAMP`]
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
/// (`JRC4558_TABLE` slewing and clipping in a gain-of-11 stage at 20 kHz:
/// normalized RMS 2.6e-4 at 128×, budget 5e-4, converging as h² from 64×)
/// and `crates/stave-test/tests/dk_matches_ngspice.rs::tube_screamer_with_a_jrc4558_matches_ngspice`
/// (the TS808 clipping stage on a 9 V supply: 3.1e-5 at 16×, budget 6e-5),
/// each the element against ngspice's behavioural form of the same
/// equations;
/// `crates/stave-dsp/src/circuit/devices/opamp/tests.rs::a_follower_reads_back_the_datasheet_gain_and_bandwidth`
/// (every part's DC gain within 1% and its −3 dB point within 0.01 of
/// 1/√2 at the datasheet's GBW, through `Circuit<Dk>`).
///
/// Against the parts (DSP.md R13, E11):
/// `crates/stave-test/tests/matches_datasheet.rs::opamps::tl072`,
/// `crates/stave-test/tests/matches_datasheet.rs::opamps::tl072_slos080j`,
/// `crates/stave-test/tests/matches_datasheet.rs::opamps::jrc4558`,
/// `crates/stave-test/tests/matches_datasheet.rs::opamps::jrc4558_table`,
/// `crates/stave-test/tests/matches_datasheet.rs::opamps::lm741` and
/// `crates/stave-test/tests/matches_datasheet.rs::opamps::njm2904` —
/// gain-bandwidth and slew read back from a follower within 10%. The
/// curve-fitted TL072 holds both drawn pulse edges and the JRC4558 its
/// open-loop plot; known gaps: each sheet's table slew figure that its
/// own curves contradict, the TL072's open-loop plot (drawn as
/// asymptotes, 22%) and the JRC4558's swing plot (a slew that climbs with
/// frequency, 7.4%). In circuits, each part against ngspice in
/// `crates/stave-test/tests/classics_match_ngspice.rs` (TS-808, RAT,
/// Phase 90, DS-1, Klon).
///
/// # Cost
///
/// The datasheet's `"Circuit<Dk> Tube Screamer (JRC4558)"`: a
/// two-control nonlinearity in the DK solve (the gain stage's input and
/// its node), beside the circuit's diodes; five of them in
/// `"Circuit<Dk> MXR Phase 90 (LM741)"`.
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
/// netlist element's, not the part's. Output swing is a fixed distance
/// from each rail: its dependence on load and sink current (the LM358
/// family's rising low limit) and crossover distortion are not
/// modelled.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Macromodel {
    pub name: &'static str,
    /// DC differential gain, volts per volt.
    pub gain: f64,
    /// Unity-gain frequency.
    pub gain_bandwidth: Hz,
    /// Volts per second, the output rising.
    pub slew_rate: f64,
    /// Volts per second, the output falling: [`slew_rate`](Self::slew_rate)
    /// for a part that slews symmetrically.
    pub slew_rate_falling: f64,
    /// Input offset voltage, added to `v(+) − v(−)`.
    pub offset: Volts,
    /// How far short of the positive supply rail the output swings.
    pub headroom: Volts,
    /// How far short of the negative supply rail the output swings:
    /// `headroom` for a part with a symmetric output stage; near zero for
    /// a single-supply part (the LM358 family) whose output reaches its
    /// negative rail.
    pub negative_headroom: Volts,
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

    /// Texas Instruments TL072 (the classic TL07xC die), fitted to
    /// SLOS080W's typical curves (revised July 2025) by least squares
    /// (`crates/stave-test/tests/fit_datasheet.rs::tl072`): Figure 5-55's
    /// voltage-follower pulse gives the slew, rising and falling, and
    /// Figure 5-46's open-loop gain the DC gain and gain-bandwidth; VIO
    /// 3 mV and VOM ±13.5 V into 10 kΩ (headroom 1.5 V) from the table.
    ///
    /// Until E11 this constant was SLOS080J's table figures (symmetric
    /// 13 V/µs, 200 V/mV, 3 MHz), kept as [`Self::TL072_SLOS080J`]. This
    /// revision's table prints 20 V/µs, which its own drawn pulse does
    /// not show; the constant follows the drawing.
    pub const TL072: Self = Self {
        name: "TL072",
        gain: 241.2226e3,
        gain_bandwidth: Hz::new(3.269979e6),
        slew_rate: 9.859077e6,
        slew_rate_falling: 12.83777e6,
        offset: Volts::new(3e-3),
        headroom: Volts::new(1.5),
        negative_headroom: Volts::new(1.5),
    };

    /// Texas Instruments TL072, SLOS080J (March 2005), typical at ±15 V,
    /// 25 °C: unity-gain bandwidth 3 MHz, slew rate 13 V/µs, AVD 200 V/mV,
    /// VIO 3 mV, VOM ±13.5 V into 10 kΩ (headroom 1.5 V): the table's
    /// figures, slewing symmetrically. [`Self::TL072`] until E11.
    pub const TL072_SLOS080J: Self = Self {
        name: "TL072 (SLOS080J)",
        gain: 200e3,
        gain_bandwidth: Hz::new(3e6),
        slew_rate: 13e6,
        slew_rate_falling: 13e6,
        offset: Volts::new(3e-3),
        headroom: Volts::new(1.5),
        negative_headroom: Volts::new(1.5),
    };

    /// New Japan Radio NJM4558 (the JRC4558D of the Tube Screamer),
    /// fitted to Nisshinbo datasheet NJM4558_E's typical curves by least
    /// squares (`crates/stave-test/tests/fit_datasheet.rs::jrc4558`): the
    /// open-loop gain plot gives the DC gain and gain-bandwidth, the
    /// maximum output swing against frequency the slew rate; VIO 0.5 mV
    /// and VOM ±14 V into 10 kΩ (headroom 1 V) from the table (the swing
    /// plot's plateau is into 2 kΩ, a load the macromodel cannot see).
    ///
    /// The sheet contradicts itself: its plots show 110 dB and a
    /// slew-limited swing implying ~1.8 V/µs, its table 100 dB and
    /// 1 V/µs. The curves are what a typical part does to a signal, so
    /// this constant follows them. Until E11 it was the table's figures,
    /// kept as [`Self::JRC4558_TABLE`] (which the ngspice decks
    /// `opamp_slew.cir` and `tube_screamer.cir` write out).
    pub const JRC4558: Self = Self {
        name: "JRC4558",
        gain: 343.256e3,
        gain_bandwidth: Hz::new(3.162246e6),
        slew_rate: 1.900817e6,
        slew_rate_falling: 1.900817e6,
        offset: Volts::new(0.5e-3),
        headroom: Volts::new(1.0),
        negative_headroom: Volts::new(1.0),
    };

    /// New Japan Radio NJM4558, Nisshinbo datasheet NJM4558_E's table,
    /// typical at ±15 V, 25 °C: GB 3 MHz, SR 1 V/µs, Av 100 dB, VIO
    /// 0.5 mV, VOM ±14 V into 10 kΩ (headroom 1 V). [`Self::JRC4558`]
    /// until E11; the ngspice decks `opamp_slew.cir` and
    /// `tube_screamer.cir` write these figures.
    pub const JRC4558_TABLE: Self = Self {
        name: "JRC4558 (table)",
        gain: 100e3,
        gain_bandwidth: Hz::new(3e6),
        slew_rate: 1e6,
        slew_rate_falling: 1e6,
        offset: Volts::new(0.5e-3),
        headroom: Volts::new(1.0),
        negative_headroom: Volts::new(1.0),
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
        slew_rate_falling: 0.3e6,
        offset: Volts::new(2e-3),
        headroom: Volts::new(1.0),
        negative_headroom: Volts::new(1.0),
    };

    /// Texas Instruments LM741C, SNOSC25D (October 2015), the LM741C
    /// table, typical at ±15 V, 25 °C: large-signal voltage gain
    /// 200 V/mV, slew rate 0.5 V/µs (unity gain), VOS 2 mV, output swing
    /// ±14 V into 10 kΩ (headroom 1 V). The LM741C rows print no
    /// bandwidth, only the unity-gain rise time, 0.3 µs; the sheet's own
    /// note derives bandwidth as 0.35/rise time, 1.17 MHz, which for the
    /// one-pole model is its gain-bandwidth (the LM741A's table prints
    /// 1.5 MHz typical, 0.437 MHz minimum).
    pub const LM741: Self = Self {
        name: "LM741",
        gain: 200e3,
        gain_bandwidth: Hz::new(1.17e6),
        slew_rate: 0.5e6,
        slew_rate_falling: 0.5e6,
        offset: Volts::new(2e-3),
        headroom: Volts::new(1.0),
        negative_headroom: Volts::new(1.0),
    };

    /// New Japan Radio NJM2904 (the Boss DS-1's op-amp since 2006; the
    /// LM2904/LM358 family), Nisshinbo datasheet NJM2904 Ver.20260722,
    /// typical: AV 100 dB, SR 0.5 V/µs and fT 0.6 MHz (both at ±15 V),
    /// VIO 2 mV. A single-supply part: its output stage reaches the
    /// negative rail, and the "Maximum Output Voltage Swing vs.
    /// Frequency" plot's plateau is 14 Vpp on a 15 V supply — 1 V short
    /// of the positive rail (headroom 1 V, negative headroom 0). The
    /// family's crossover distortion and its sink-current-dependent low
    /// limit are not modelled.
    pub const NJM2904: Self = Self {
        name: "NJM2904",
        gain: 100e3,
        gain_bandwidth: Hz::new(0.6e6),
        slew_rate: 0.5e6,
        slew_rate_falling: 0.5e6,
        offset: Volts::new(2e-3),
        headroom: Volts::new(1.0),
        negative_headroom: Volts::new(0.0),
    };

    pub const ALL: [Self; 7] = [
        Self::TL072,
        Self::TL072_SLOS080J,
        Self::JRC4558,
        Self::JRC4558_TABLE,
        Self::LM308,
        Self::LM741,
        Self::NJM2904,
    ];

    /// The constants the gain stage runs on between supply rails
    /// `negative` and `positive` volts.
    pub fn prepare(&self, negative: f64, positive: f64) -> Prepared {
        let c = Self::COMPENSATION_FARADS;
        let gm = std::f64::consts::TAU * f64::from(self.gain_bandwidth.get()) * c;
        let slew_amps = self.slew_rate * c;
        let fall_amps = self.slew_rate_falling * c;
        let (low, high) = (
            negative + self.negative_headroom.get(),
            positive - self.headroom.get(),
        );
        Prepared {
            gm,
            slew_amps,
            fall_amps,
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
    /// The input stage's largest current sourced (the output rising),
    /// amps; the rail clamp is scaled by it.
    pub slew_amps: f64,
    /// The input stage's largest current sunk (the output falling), amps.
    pub fall_amps: f64,
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
        let u = self.gm * (input + self.offset);
        // Each branch saturates at its own current: sourcing, the rising
        // slew current; sinking, the falling one. Both have slope gm at
        // the origin, so the joint is smooth.
        let limit = if u < 0.0 { self.fall_amps } else { is };
        let t = (u / limit).tanh();
        let k = Macromodel::CLAMP * is;
        let vk = Macromodel::KNEE_VOLTS;
        let (up, up_slope) = softplus((node - self.high) / vk);
        let (down, down_slope) = softplus((self.low - node) / vk);
        StageCurrent {
            amps: limit * t + self.leak * self.middle - k * (up - down),
            per_input_volt: self.gm * (1.0 - t * t),
            per_node_volt: -k * (up_slope + down_slope) / vk,
        }
    }
}
