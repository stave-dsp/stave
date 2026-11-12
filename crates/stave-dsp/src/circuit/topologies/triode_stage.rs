//! A common-cathode triode stage, tabulated: Koren's plate law (see
//! [`Koren`]) solved against the plate load line, with the grid driven
//! through the source resistance so grid current (a measured 12AX7 grid
//! law) holds a positive grid near the cathode.
//!
//! The plate node settles in ~10 ns (100 pF into 100 kΩ), ~2000× faster
//! than a sample, so each sample is an equilibrium of
//! `(V_supply − Vpk)/R_plate = Ip(Vgk, Vpk)`: a memoryless map from input
//! to plate swing. It is solved once per table point when the stage is
//! rebuilt, tabulated over grid volts with its antiderivatives, and
//! evaluated with ADAA. Cutoff, bottoming, their asymmetry (2nd-harmonic
//! dominance) and grid-conduction limiting all come from that equation;
//! nothing is shaped by hand. The per-sample solve of the same circuit,
//! cathode node included, is [`CommonCathode`](super::common_cathode).
//!
//! Under [`Bias::Cathode`] the cathode is the circuit's RC, driven by
//! plate plus grid current and moving the bias between samples.
//!
//! Output is the inverted plate swing `(Vpk0 − Vpk)`, scaled by the
//! [`LevelPolicy`].
//!
//! **Rate contract:** `prepare()` takes the rate `tick` runs at
//! — N× the host rate inside an N× oversampler, or the cathode's time
//! constant is off by N.

use crate::behavioral::adaa::{Adaa, AdaaOrder};
use crate::behavioral::curve::{AdaaShape, Curve};
use crate::behavioral::curve::{CurveStage, Frame};
use crate::circuit::devices::triode::dempwolf::Dempwolf;
use crate::circuit::devices::triode::koren::Koren;
use crate::circuit::methods::load_line::LoadLine;
use crate::circuit::methods::newton::NewtonSettings;
use crate::kernel::node::{Stage, Tail};

/// Koren constants + circuit. Defaults are a classic 12AX7 gain stage
/// (300 V supply, 100 kΩ plate load, −1.5 V grid bias).
#[derive(Clone, Copy, Debug)]
pub struct TriodeParams {
    pub tube: Koren,
    pub supply_v: f64,  // B+ rail [V]
    pub r_plate: f64,   // plate load resistor [Ω]
    pub grid_bias: f64, // quiescent grid bias [V]
    /// Between the input and the grid (source plus grid stopper), ohms:
    /// what grid current drops its volts across when the grid conducts.
    pub source_ohms: f64,
}

/// A guitar amp's input grid stopper, the classic 68 kΩ.
pub const INPUT_GRID_STOPPER_OHMS: f64 = 68e3;

/// Koren's plate law has no grid current; the grid-to-cathode diode is a
/// measured 12AX7's (Dempwolf & Zölzer, RSD-1) for every tube type, until
/// per-type grid data is modeled.
const GRID: Dempwolf = Dempwolf::RSD_1;

impl Default for TriodeParams {
    fn default() -> Self {
        Self::for_tube(Koren::T12AX7)
    }
}

impl TriodeParams {
    /// A tube at its typical bias in a common 300 V / 100 kΩ stage.
    /// Override `supply_v` / `r_plate` afterwards for a different circuit.
    pub fn for_tube(tube: Koren) -> Self {
        Self {
            tube,
            supply_v: 300.0,
            r_plate: 100e3,
            grid_bias: tube.typical_grid_bias,
            source_ohms: INPUT_GRID_STOPPER_OHMS,
        }
    }
}

/// Grid-voltage domain the shared [`Curve`] tabulates. The nonlinearity
/// lives in roughly −5..+1 V; the wide range plus the Curve's edge
/// extrapolation covers any drive setting.
const VG_MIN: f64 = -12.0;
const VG_MAX: f64 = 12.0;

/// Grid volts per unit until a drive is set: gentle, near-linear.
const INITIAL_GRID_DRIVE: f64 = 0.4;

/// LEVEL POLICY — mechanism vs policy, made explicit. The stage's
/// physics are identical under both; what differs is what the drive
/// knob does to LEVEL:
///
/// * `Calibrated` (default, the historical behaviour): small-signal
///   gain is normalised to unity at any drive — the knob adds
///   harmonic colour, not level. Right for a saturation EFFECT
///   (forge, a band saturator) where loudness invariance is the
///   product. The knee's OUTPUT level falls as drive rises, so deep
///   cascades collapse toward a collective brick wall — which is
///   precisely why this is the wrong policy for an amp.
/// * `Physical`: the stage behaves like a real gain pot into a fixed
///   tube — small-signal gain FOLLOWS drive, the ceiling stays put.
///   Quiet passages ride up and down with touch; a cascade develops
///   a knee LADDER (later stages clip first) and a decaying note
///   slides back out of saturation stage by stage. This is where
///   amp feel / bloom lives.
///
/// Under either policy the stage REPORTS its small-signal insertion
/// gain (`small_signal_gain_db`) — consumers compute makeup, never
/// fit it.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum LevelPolicy {
    #[default]
    Calibrated,
    /// Unity small-signal gain at `reference_drive` (grid volts per
    /// unit); the gain then follows drive as a real gain pot would.
    Physical { reference_drive: f64 },
}

/// How a stage's grid is biased.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Bias {
    /// From a bias supply: the operating point does not move.
    Fixed,
    /// From its own cathode resistor, bypassed by a capacitor.
    Cathode { bypass_farads: f64 },
}

pub struct Triode {
    p: TriodeParams,

    // Quiescent plate voltage (operating point), solved at prepare/reset.
    vpk0: f64,

    // Effective extra grid bias (more-negative ⇒ less gain) =
    // external (vari-mu sidechain) + cathode self-bias. Both shift the LUT
    // lookup point; both evolve slowly, so the LUT-ADAA antialiasing stays
    // valid (no fast nonlinearity is being modulated out from under it).
    extra_bias: f64,
    external_bias: f64, // smoothed vari-mu / sidechain input

    // === Cathode self-bias — the stateful "memory" ===
    // The cathode voltage follows the plate current through an RC; because
    // the tube curve is convex, the average current (hence the bias) drifts
    // upward with program level, pulling the operating point toward cutoff
    // — the stage compresses, then recovers with the RC time constant. This
    // is the core living/responsive tube behaviour and the basis of a
    // vari-mu / tube-comp character. None under `Bias::Fixed`.
    sample_rate: f64,
    vk: f64,  // cathode voltage state
    vk0: f64, // quiescent cathode voltage (= −grid_bias)
    rk: f64,  // cathode resistance (sets bias from current)
    bias: Bias,
    cathode_coeff: f64, // one-pole coefficient for the cathode RC
    cathode_shift: f64, // vk − vk0, the bias contribution

    // Drive: grid swing volts per normalised input unit, and an output
    // trim whose meaning the level policy decides (see `LevelPolicy`).
    grid_drive: f64,
    output_scale: f64,
    level_policy: LevelPolicy,
    // |dVpk/dVgk| at the operating point (solved in calibrate_output) —
    // the quantity behind the reported small-signal gain.
    gain_q: f64,
    // Max |plate swing| over the tabulated grid range — the raw
    // ceiling behind the reported `ceiling_db`.
    curve_max_f: f64,

    // === Antiderivative anti-aliasing (ADAA) ===
    // The transfer is tabulated in GRID-VOLTAGE space (vg = bias + drive·x),
    // which is drive-independent — so the Curve is built once and
    // `set_drive_db` only re-points its affine frame (no rebuild, no
    // audio-thread allocation). Curve `f` = unscaled plate swing
    // (vpk0 − vpk); `g1` = ∫f dvg; `g2` = ∫g1 dvg. Order guidance (why the
    // default is 1st order) is in the `adaa` module docs.
    curve: Curve,
    adaa: Adaa,
    adaa_on: bool,
}

impl Default for Triode {
    fn default() -> Self {
        Self::new()
    }
}

impl Triode {
    pub fn new() -> Self {
        let p = TriodeParams::default();
        let mut t = Self {
            p,
            vpk0: p.supply_v * 0.7,
            extra_bias: 0.0,
            external_bias: 0.0,
            sample_rate: 48_000.0,
            vk: 0.0,
            vk0: 0.0,
            rk: 0.0,
            bias: Bias::Fixed,
            cathode_coeff: 0.0,
            cathode_shift: 0.0,
            grid_drive: INITIAL_GRID_DRIVE,
            // Calibrated below in `recompute_operating_point` so a 0 dB,
            // small-signal pass is ~unity gain.
            output_scale: 1.0,
            level_policy: LevelPolicy::Calibrated,
            gain_q: 1.0,
            curve_max_f: 1.0,
            curve: Curve::flat(),
            adaa: Adaa::new(AdaaOrder::First),
            adaa_on: true,
        };
        t.recompute_operating_point();
        t
    }

    /// Enable/disable ADAA (default on). Off = direct table evaluation
    /// (still the physical transfer, just aliased) — used for A/B tests.
    pub fn set_adaa(&mut self, on: bool) {
        self.adaa_on = on;
    }

    pub fn set_adaa_order(&mut self, order: AdaaOrder) {
        self.adaa.set_order(order);
    }

    /// How the grid is biased. Under `Cathode` the cathode node is the
    /// circuit's RC (time constant Rk·Ck, Rk from the operating point):
    /// program level rectified through the tube's curve and grid current
    /// raises the cathode, pulling the stage toward cutoff, and silence
    /// lets it recover. The observed recovery is faster than Rk·Ck by the
    /// loop factor 1 + gm·Rk (the shift suppresses the very current that
    /// charges the cathode), ≈2× for a 12AX7, as in hardware.
    pub fn set_bias(&mut self, bias: Bias) {
        self.bias = bias;
        self.update_cathode_coeff();
    }

    pub fn cathode_ohms(&self) -> f64 {
        self.rk
    }

    #[inline]
    fn update_cathode_coeff(&mut self) {
        self.cathode_coeff = match self.bias {
            Bias::Fixed => 0.0,
            Bias::Cathode { bypass_farads } => {
                1.0 - (-1.0 / (self.rk * bypass_farads * self.sample_rate)).exp()
            }
        };
    }

    /// Instantaneous gain reduction (dB, ≤ 0) from the current bias drift —
    /// for metering and tube-compressor use. Compares the small-signal
    /// slope at the current operating point to the quiescent slope.
    pub fn gain_reduction_db(&self) -> f32 {
        let d = 0.05;
        let slope = |v: f64| (self.core_at(v + d) - self.core_at(v - d)).abs() / (2.0 * d);
        let now = slope(self.p.grid_bias - self.extra_bias).max(1e-9);
        let rest = slope(self.p.grid_bias).max(1e-9);
        (20.0 * (now / rest).log10()) as f32
    }

    /// Koren plate current and its derivative wrt plate voltage.
    /// Returns (Ip, dIp/dVpk). Below cutoff both are 0.
    #[inline]
    fn plate_current(&self, vgk: f64, vpk: f64) -> (f64, f64) {
        koren_plate_current(&self.p, vgk, vpk)
    }

    /// Solve the load line `(V_supply − Vpk)/R = Ip(Vgk, Vpk)` for Vpk.
    /// Newton with analytic derivative, bounded, warm-started from `seed`.
    #[inline]
    fn solve_plate(&self, vgk: f64, seed: f64) -> f64 {
        solve_load_line(&self.p, vgk, seed)
    }

    /// Re-solve the quiescent operating point, (re)build the ADAA curve,
    /// and recalibrate output. Allocation-free (the Curve rebuilds in
    /// place); costs ~1k Newton solves, so it runs at construction /
    /// prepare / structural param change only.
    fn recompute_operating_point(&mut self) {
        let vgk0 = grid_volts_at(&self.p, self.p.grid_bias);
        self.vpk0 = self.solve_plate(vgk0, self.p.supply_v * 0.6);
        // Tabulate the physical transfer (plate swing vs grid volts) with
        // antiderivatives referenced to the quiescent bias.
        let p = self.p;
        let vpk0 = self.vpk0;
        self.curve.rebuild(VG_MIN, VG_MAX, p.grid_bias, |vg| {
            core_transfer_at(&p, vpk0, vg)
        });
        // The raw output ceiling: max |plate swing| over the table —
        // behind `ceiling_db`, the third reported quantity (gain +
        // ceiling = the knee, so consumers can COMPUTE their staging).
        let mut m = 0.0f64;
        for i in 0..=256 {
            let vg = VG_MIN + (VG_MAX - VG_MIN) * i as f64 / 256.0;
            m = m.max(self.curve.f_at(vg).abs());
        }
        self.curve_max_f = m.max(1e-9);
        self.calibrate_output();
        // Cathode self-bias operating point: pick Rk so the quiescent
        // cathode voltage equals the design grid bias (vk0 = −grid_bias),
        // i.e. the cathode resistor *creates* the bias from the idle plate
        // current — self-consistent, no jump when the dynamic is enabled.
        let i_op = ((self.p.supply_v - self.vpk0) / self.p.r_plate).max(1e-9);
        self.vk0 = -self.p.grid_bias;
        self.rk = self.vk0 / i_op;
        self.vk = self.vk0;
        self.cathode_shift = 0.0;
        self.extra_bias = self.external_bias;
        self.update_cathode_coeff();
        self.adaa.reset();
    }

    /// Unscaled transfer at an input (source-side) grid voltage: the plate
    /// AC swing `vpk0 − vpk`, including grid current. This is the
    /// fixed shape the Curve tabulates (independent of drive / output
    /// level).
    #[cfg_attr(not(test), allow(dead_code))] // test/inspection surface
    #[inline]
    fn core_transfer(&self, vg_raw: f64) -> f64 {
        core_transfer_at(&self.p, self.vpk0, vg_raw)
    }

    /// Raw curve lookup in grid-voltage space (used by the cathode-bias
    /// dynamics and the gain-reduction meter, which live in vg space).
    #[inline]
    fn core_at(&self, vg: f64) -> f64 {
        self.curve.f_at(vg)
    }

    /// The affine frame mapping normalised input to the tabulated curve:
    /// vg = (grid_bias − extra_bias) + grid_drive·x, output × output_scale.
    /// This is the `Modulation`-shaped surface program-dependent dynamics
    /// re-point. (`tick` constructs it inline for borrow
    /// disjointness with the ADAA state; this accessor serves tests.)
    #[cfg_attr(not(test), allow(dead_code))]
    #[inline]
    fn framed(&self) -> crate::behavioral::curve::Framed<'_> {
        self.curve.framed(
            self.p.grid_bias - self.extra_bias,
            self.grid_drive,
            self.output_scale,
        )
    }

    /// TRUE static transfer at the current frame (idle slow states):
    /// the exact curve the ADAA path rides, exposed so product models can
    /// walk REAL levels and slopes instead of soft-knee fictions (the
    /// slope arc's lesson: a 9 dB softplus prices levels fine and
    /// derivatives terribly).
    #[inline]
    pub fn static_transfer(&self, x: f64) -> f64 {
        self.framed().f(x)
    }

    // Map a normalised input to grid voltage (used by the grid-excess
    // sense tap; must agree with `framed`'s offset/scale).
    #[inline]
    fn vg_of(&self, x: f64) -> f64 {
        self.p.grid_bias - self.extra_bias + x * self.grid_drive
    }
    #[cfg_attr(not(test), allow(dead_code))]
    #[inline]
    fn f_eval(&self, x: f64) -> f64 {
        self.framed().f(x)
    }
    #[cfg_attr(not(test), allow(dead_code))]
    #[inline]
    fn f1_eval(&self, x: f64) -> f64 {
        self.framed().f1(x)
    }
    #[cfg_attr(not(test), allow(dead_code))]
    #[inline]
    fn f2_eval(&self, x: f64) -> f64 {
        self.framed().f2(x)
    }

    /// Solve the operating-point gain and set `output_scale` per the
    /// level policy (see `LevelPolicy`):
    ///
    /// * Calibrated — small-signal gain normalised to unity at ANY
    ///   drive (knob = colour, not level). The saturated-regime loss is
    ///   intentionally NOT compensated (level-dependent; the host
    ///   saturator's level-following makeup restores it).
    /// * Physical — output scale is anchored at the REFERENCE drive
    ///   (0 dB), then held: raising drive raises small-signal gain
    ///   exactly as a hotter input hits a fixed stage, and the knee's
    ///   output ceiling stays put.
    fn calibrate_output(&mut self) {
        let vgk0 = self.p.grid_bias - self.extra_bias;
        let dv = 1e-3;
        let vp_a = self.solve_plate(vgk0 + dv, self.vpk0);
        let vp_b = self.solve_plate(vgk0 - dv, self.vpk0);
        // |dVpk/dVgk|: below 1 is real (a stage biased toward cutoff).
        self.gain_q = ((vp_b - vp_a) / (2.0 * dv)).abs();
        let anchor = match self.level_policy {
            LevelPolicy::Calibrated => self.grid_drive,
            LevelPolicy::Physical { reference_drive } => reference_drive,
        };
        self.output_scale = 1.0 / (self.gain_q * anchor).max(1e-6);
    }

    /// The stage's small-signal insertion gain (dB) at the current
    /// operating point — REPORTED, so consumers compute makeup instead
    /// of fitting ladders. Calibrated ≈ 0 dB by construction; Physical
    /// follows drive.
    pub fn small_signal_gain_db(&self) -> f32 {
        let g = (self.gain_q * self.grid_drive * self.output_scale).max(1e-9);
        (20.0 * g.log10()) as f32
    }

    /// The stage's REPORTED output ceiling (dBFS): the largest swing
    /// the plate can deliver at the current output scale. Together
    /// with `small_signal_gain_db` this locates the knee — the two
    /// numbers a consumer needs to compute staging and makeup rather
    /// than fit them. Drive-invariant under `Physical` (the contract
    /// `the_ceiling_obeys_the_policy` pins); falls with drive under
    /// `Calibrated`.
    pub fn ceiling_db(&self) -> f32 {
        let c = (self.curve_max_f * self.output_scale).max(1e-9);
        (20.0 * c.log10()) as f32
    }

    /// Level policy (default `Calibrated` — existing consumers keep
    /// their behaviour). Idempotent; switching recalibrates the output
    /// anchor only, never the physics.
    pub fn set_level_policy(&mut self, policy: LevelPolicy) {
        if self.level_policy != policy {
            self.level_policy = policy;
            self.calibrate_output();
        }
    }

    /// Grid swing per unit of input, volts: how hard the stage is driven.
    /// Low drive stays near the bias (2nd-harmonic warmth); high drive
    /// reaches cutoff and grid conduction. A product's drive knob maps
    /// onto this with its own law. Under `Calibrated` the output is
    /// re-levelled so the small-signal gain stays at unity.
    pub fn set_grid_drive(&mut self, volts_per_unit: f64) {
        // Idempotent: a re-bind with unchanged values must not perturb
        // a settled operating point.
        if volts_per_unit == self.grid_drive {
            return;
        }
        self.grid_drive = volts_per_unit;
        self.calibrate_output();
    }

    /// External / vari-mu sidechain: a detector envelope (≥0) shifts the
    /// grid more negative, dropping gain — drive this from your own
    /// detector to use the stage as a controllable compressor. Adds on top
    /// of the internal cathode self-bias. Smoothed to avoid zipper noise.
    pub fn set_dynamic_grid_bias(&mut self, bias: f64) {
        const SMOOTH: f64 = 0.95;
        self.external_bias = SMOOTH * self.external_bias + (1.0 - SMOOTH) * bias;
        self.extra_bias = self.external_bias + self.cathode_shift;
    }

    pub fn set_params(&mut self, params: TriodeParams) {
        self.p = params;
        self.recompute_operating_point();
    }

    /// Grid-cathode voltage above the conduction threshold (≈0 V) for a
    /// given input, i.e. how hard the grid is being driven into conduction.
    /// Proportional to grid current — used by inter-stage coupling to model
    /// blocking distortion (grid current charging the coupling cap).
    #[inline]
    pub fn grid_excess(&self, input: f32) -> f32 {
        self.vg_of(input as f64).max(0.0) as f32
    }

    /// Inject an external grid-bias offset (volts, ≥0 ⇒ more negative grid)
    /// WITHOUT the `set_dynamic_grid_bias` smoother — for callers (e.g. the
    /// coupling network) that supply an already-smoothed control voltage.
    /// Composes additively with the internal cathode self-bias.
    #[inline]
    pub fn set_grid_bias_direct(&mut self, bias: f64) {
        self.external_bias = bias;
        self.extra_bias = self.external_bias + self.cathode_shift;
    }

    pub fn with_tube(tube: Koren) -> Self {
        let mut s = Self::new();
        s.set_tube(tube);
        s
    }

    /// Swap the tube — its Koren parameters and typical bias, in the same
    /// 300 V / 100 kΩ circuit — and rebuild the tables. A no-op when
    /// unchanged, so a tube-selector parameter may call it every block.
    pub fn set_tube(&mut self, tube: Koren) {
        if tube != self.p.tube {
            self.p = TriodeParams::for_tube(tube);
            self.recompute_operating_point();
        }
    }

    pub fn tube(&self) -> Koren {
        self.p.tube
    }

    /// Process one normalised sample (−1..1) → saturated sample.
    ///
    /// Test/inspection: quiescent plate voltage.
    pub fn operating_point(&self) -> f64 {
        self.vpk0
    }
    /// Test/inspection: current total bias shift (V) = sidechain + cathode.
    /// The cathode's own drift from its quiescent voltage (volts, positive
    /// = colder), for products that build on it.
    pub fn cathode_shift(&self) -> f64 {
        self.cathode_shift
    }
    pub fn bias_shift(&self) -> f64 {
        self.extra_bias
    }
    /// Test/inspection: raw Koren plate current, amps, at (Vgk, Vpk).
    pub fn plate_current_at(&self, vgk: f64, vpk: f64) -> f64 {
        self.plate_current(vgk, vpk).0
    }

    /// Export this triode's physical transfer as a [`CurveStage`] for
    /// Chain-based topologies: the
    /// Koren load-line table with a GRID-CONDUCTION sense column
    /// (positive grid excess, pre-clamp — what blocking keys on), and the
    /// frame set from the current drive calibration. The stage carries no
    /// cathode/blocking state — those become drivers + routes.
    pub fn to_curve_stage(&self) -> CurveStage {
        let p = self.p;
        let vpk0 = self.vpk0;
        let mut cs = CurveStage::from_fn_with_sense(
            VG_MIN,
            VG_MAX,
            p.grid_bias,
            |vg| core_transfer_at(&p, vpk0, vg),
            |vg| vg.max(0.0),
        );
        cs.set_pending(Frame {
            bias: p.grid_bias as f32,
            drive: self.grid_drive as f32,
            gain: self.output_scale as f32,
        });
        cs
    }

    /// Current affine frame (bias, drive, output scale) — drive-dependent
    /// but table-independent, so callers can track `set_drive_db` without
    /// re-tabulating.
    pub fn frame_params(&self) -> (f64, f64, f64) {
        (self.p.grid_bias, self.grid_drive, self.output_scale)
    }

    /// Cathode self-bias constants for driver-based topologies:
    /// (rk, vk0, i_op, r_plate) — plate current is affine in the stage's
    /// unscaled output: ip = i_op + core/r_plate.
    pub fn cathode_constants(&self) -> (f64, f64, f64, f64) {
        let i_op = ((self.p.supply_v - self.vpk0) / self.p.r_plate).max(1e-9);
        (self.rk, self.vk0, i_op, self.p.r_plate)
    }
}

// --- Free-standing physics (parameter-explicit so the Curve rebuild
// closure can evaluate them without aliasing &mut self) ---

#[inline]
pub(crate) fn koren_plate_current(p: &TriodeParams, vgk: f64, vpk: f64) -> (f64, f64) {
    p.tube.plate_current(vgk, vpk)
}

/// Solve the load line `(V_supply − Vpk)/R = Ip(Vgk, Vpk)` for Vpk.
/// Newton with analytic derivative, bounded, warm-started from `seed`.
#[inline]
pub(crate) fn solve_load_line(p: &TriodeParams, vgk: f64, seed: f64) -> f64 {
    let line = LoadLine {
        supply_volts: p.supply_v,
        load_ohms: p.r_plate,
    };
    let tube = p.tube;
    line.solve(
        |vpk| tube.plate_current(vgk, vpk),
        seed,
        &NewtonSettings::default(),
    )
    .value
}

/// The grid's own voltage for a source-side voltage `input`: Kirchhoff at
/// the grid, `vg + Rs·Ig(vg) = input`. Grid current rises monotonically,
/// so there is exactly one solution; above 0 V it is what holds a driven
/// grid near the cathode.
#[inline]
pub(crate) fn grid_volts_at(p: &TriodeParams, input: f64) -> f64 {
    let residual = |vg: f64| {
        let (ig, dig) = GRID.grid_current(vg);
        (vg + p.source_ohms * ig - input, 1.0 + p.source_ohms * dig)
    };
    NewtonSettings::default()
        .solve_scalar(input.min(0.0), residual, |candidate, _| candidate)
        .value
}

/// Grid current at a source-side grid voltage.
#[inline]
pub(crate) fn grid_amps_at(p: &TriodeParams, input: f64) -> f64 {
    GRID.grid_current(grid_volts_at(p, input)).0
}

/// The tabulated shape: plate AC swing at a source-side grid voltage.
#[inline]
pub(crate) fn core_transfer_at(p: &TriodeParams, vpk0: f64, input: f64) -> f64 {
    vpk0 - solve_load_line(p, grid_volts_at(p, input), vpk0)
}

impl Stage for Triode {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate as f64;
        self.recompute_operating_point();
    }

    fn reset(&mut self) {
        self.external_bias = 0.0;
        self.cathode_shift = 0.0;
        self.extra_bias = 0.0;
        self.vk = self.vk0;
        self.adaa.reset();
    }

    /// Evaluates the (tabulated) physical transfer through antiderivative
    /// anti-aliasing, then advances the cathode self-bias state. The
    /// nonlinearity is LUT-ADAA'd (cheap, no per-sample solve); the cathode
    /// bias is a slow one-pole feedback of the plate current, so it adds
    /// living/responsive dynamics without breaking the antialiasing.
    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let x0 = input as f64;
        // The frame (bias/drive/scale) is frozen for this evaluation; the
        // cathode update below commits its bias shift AFTER, between
        // difference quotients — the ordering ADAA requires.
        let y = {
            let framed = self.curve.framed(
                self.p.grid_bias - self.extra_bias,
                self.grid_drive,
                self.output_scale,
            );
            if self.adaa_on {
                self.adaa.tick(&framed, x0)
            } else {
                self.adaa.push(x0);
                framed.f(x0)
            }
        };

        // Cathode self-bias: track the cathode current (plate plus grid)
        // through the RC and drift the operating point. The tube curve's
        // convexity and grid conduction rectify the signal, so the averaged
        // current (hence bias) rises with program level → compression that
        // recovers with `cathode_tau_ms`.
        if let Bias::Cathode { .. } = self.bias {
            let input = self.vg_of(x0);
            let vpk = self.vpk0 - self.core_at(input);
            let ip = (self.p.supply_v - vpk) / self.p.r_plate;
            let ig = GRID.grid_current(grid_volts_at(&self.p, input)).0;
            let vk_target = (ip + ig) * self.rk;
            self.vk += (vk_target - self.vk) * self.cathode_coeff;
            self.cathode_shift = self.vk - self.vk0;
            self.extra_bias = self.external_bias + self.cathode_shift;
        }
        y as f32
    }

    /// The cathode RC under `Bias::Cathode` (an upper bound: the loop
    /// factor 1 + gm·Rk shortens the observed recovery); memoryless
    /// under `Bias::Fixed`.
    fn tail(&self) -> Tail {
        match self.bias {
            Bias::Fixed => Tail::None,
            Bias::Cathode { bypass_farads } => {
                Tail::from_time_constant(self.rk * bypass_farads, self.sample_rate as f32)
            }
        }
    }
}

#[cfg(test)]
mod tests;
