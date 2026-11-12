//! Cascaded RC-coupled triode preamp — the tube topology as DATA on the
//! Stage/Chain layer.
//!
//! Each stage is a coupling high-pass ([`Coupler`] — the physical RC) into the triode's
//! physical transfer ([`CurveStage`] exported from the Koren load-line
//! solve, carrying a grid-conduction SENSE column). The two behaviours
//! that used to be bespoke state machines are now envelope drivers:
//!
//!  1. **Blocking distortion**: grid conduction (the sense tap) charges a
//!     fast-attack cell whose release is the coupling RC — the same RC as
//!     the high-pass, exactly as the physics dictates. Its state pushes
//!     the stage's frame bias toward cutoff: the choke, then recovery.
//!  2. **Cathode self-bias**: plate current is affine in the stage's
//!     output tap (ip = i_op + core/R), so a symmetric cell following
//!     ip·Rk reproduces the bias walk; its state also shifts the frame.
//!
//! Public API and behaviour match the previous bespoke implementation
//! (its tests below are unchanged); the cascade math now lives in
//! Chain/CurveStage/EnvelopeFollower.
//!
//! **Rate contract:** `prepare()` must be given the rate `process_sample`
//! actually runs at — for a cascade inside a nested 4× oversampler that
//! is 4× the host rate.

use stave_dsp::db_to_amp;
use stave_dsp::{
    Coupler, CurveStage, EnvelopeFollower, EnvelopeTimes, Frame, Koren, LevelPolicy, Stage, Triode,
};

/// Maximum cascade depth (fixed so there's no audio-thread allocation).
pub const MAXIMUM_STAGE_COUNT: usize = 4;

pub struct RcCoupledTriodeCascade {
    /// Table/calibration factory: holds the Koren solve + drive taper.
    triode_prototype: Triode,
    /// FLAT cascade (the perf deep-dive, 2026-08-05): the Chain
    /// dispatch layer cost ~2x the curve math itself per sample
    /// (measured: preamp stage 954 profile samples vs the pair's
    /// direct-call 288 for the same ADAA work). Same cells, same
    /// order, same math — direct arrays, no per-sample enum
    /// matches, no id indirection. Topology-as-data stays the
    /// container story (Chain lives on); the INNER loop of a hot
    /// product model is direct.
    couplers: [Coupler; MAXIMUM_STAGE_COUNT],
    curves: [CurveStage; MAXIMUM_STAGE_COUNT],
    /// Per-stage outputs from the last tick (the old chain senses).
    stage_outputs: [f32; MAXIMUM_STAGE_COUNT],
    /// Blocking cells: fast charge from grid conduction, release = the
    /// coupling RC.
    blocking_envelopes: [EnvelopeFollower; MAXIMUM_STAGE_COUNT],
    /// Cathode cells: symmetric τ follower of ip·Rk.
    cathode_envelopes: [EnvelopeFollower; MAXIMUM_STAGE_COUNT],
    blocking_voltages: [f32; MAXIMUM_STAGE_COUNT],
    cathode_bias_shifts: [f32; MAXIMUM_STAGE_COUNT],
    stage_count: usize,
    sample_rate: f32,
    coupling_hz: f32,
    /// Per-stage coupler override (0 = the cascade-wide corner).
    /// The convention of high-gain design: couplers SHRINK downstream —
    /// the tightness is the couplers.
    stage_coupling_frequencies_hz: [f32; MAXIMUM_STAGE_COUNT],
    /// Per-stage grid-bias offset, volts COLDER (the SLO/5150/Recto
    /// cold-clipper stage: asymmetric cutoff clipping, the
    /// character stage). 0 = the tube's own operating point.
    stage_bias_offsets_volts: [f32; MAXIMUM_STAGE_COUNT],
    blocking_depth: f32,
    /// Blocking recovery override (ms). 0 = derive from the coupling
    /// RC (the historical tie). Physically the grid-leak R and the
    /// coupling corner are DIFFERENT quantities — the atlas showed
    /// the tie caps recovery at ~20 ms and forecloses the
    /// 100-400 ms region where inter-chug articulation lives.
    blocking_recovery_milliseconds: f32,
    interstage_gain_amplitude: f32,
    cathode_depth: f32,
    cathode_time_milliseconds: f32,
    /// Per-stage cathode network overrides (depth, tau ms); tau 0 =
    /// use the cascade-wide values. Real preamps MIX cathode
    /// treatments per stage (full bypass, bright partial bypass,
    /// unbypassed) — the network is a per-stage design choice, and
    /// at bypass-scale tau (0.2-10 ms — corners 100-700 Hz, NOT
    /// the 40 ms envelope scale) the ODE's signal-rate tracking
    /// makes the classic dynamic shelf EMERGE: gain and brightness
    /// ride the stage's own conduction into the next knee.
    stage_cathode_settings: [(f32, f32); MAXIMUM_STAGE_COUNT],
    /// Cathode resistance, idle cathode voltage, idle plate current, and
    /// plate resistance from the prototype's operating point.
    cathode_constants: (f64, f64, f64, f64),
    /// External supply scale (B+ relative to idle): multiplies
    /// every stage's output frame — B+ droop moves ceiling and gain
    /// together. 1 = stiff/idle.
    supply_scale: f32,
    /// Normalized aggregate plate current, last sample (1 = idle).
    /// THE honest sag driver: conduction depth follows the input
    /// even when the output level is pinned, so a supply fed from
    /// here moves the operating point PER HIT — tone and level
    /// co-vary, nothing is overlaid.
    normalized_supply_draw: f32,
}

impl Default for RcCoupledTriodeCascade {
    fn default() -> Self {
        Self::new()
    }
}

impl RcCoupledTriodeCascade {
    pub fn new() -> Self {
        let triode_prototype = Triode::new();
        let couplers = core::array::from_fn(|_| Coupler::new(8.0));
        let curves = core::array::from_fn(|_| triode_prototype.to_curve_stage());
        let cathode_constants = triode_prototype.cathode_constants();
        let mut cascade = Self {
            triode_prototype,
            couplers,
            curves,
            stage_outputs: [0.0; MAXIMUM_STAGE_COUNT],
            blocking_envelopes: core::array::from_fn(|_| {
                EnvelopeFollower::new(EnvelopeTimes {
                    attack_ms: 1.0,
                    release_ms: 20.0,
                    slow_release_ms: None,
                })
            }),
            cathode_envelopes: core::array::from_fn(|_| {
                EnvelopeFollower::new(EnvelopeTimes {
                    attack_ms: 30.0,
                    release_ms: 30.0,
                    slow_release_ms: None,
                })
            }),
            blocking_voltages: [0.0; MAXIMUM_STAGE_COUNT],
            cathode_bias_shifts: [0.0; MAXIMUM_STAGE_COUNT],
            stage_count: 2,
            sample_rate: 48_000.0,
            coupling_hz: 8.0,
            stage_coupling_frequencies_hz: [0.0; MAXIMUM_STAGE_COUNT],
            stage_bias_offsets_volts: [0.0; MAXIMUM_STAGE_COUNT],
            blocking_depth: 1.0,
            blocking_recovery_milliseconds: 0.0,
            interstage_gain_amplitude: 1.0,
            cathode_depth: 0.0,
            cathode_time_milliseconds: 30.0,
            stage_cathode_settings: [(0.0, 0.0); MAXIMUM_STAGE_COUNT],
            cathode_constants,
            supply_scale: 1.0,
            normalized_supply_draw: 1.0,
        };
        cascade.rebuild_stages();
        cascade
    }

    /// (Re)build the chain's stages from the triode_prototype: active stages get the
    /// coupling filter + triode curve; inactive slots are bypassed.
    /// Block-rate cost (tube/stage-count changes only).
    fn rebuild_stages(&mut self) {
        self.cathode_constants = self.triode_prototype.cathode_constants();
        for stage_index in 0..MAXIMUM_STAGE_COUNT {
            // The physical coupler: first-order RC, f64 state.
            let coupling_frequency_hz = if self.stage_coupling_frequencies_hz[stage_index] > 0.0 {
                self.stage_coupling_frequencies_hz[stage_index]
            } else {
                self.coupling_hz
            };
            self.couplers[stage_index] = Coupler::new(coupling_frequency_hz);
            Stage::prepare(&mut self.couplers[stage_index], self.sample_rate);
        }
        // ONE table set for the whole cascade: every stage is the
        // same tube; frames differ per stage, tables never do.
        // Clone = Arc bump — four stages read the same memory (the
        // deep-dive's cache finding).
        let curve_stage_prototype = self.triode_prototype.to_curve_stage();
        for stage_index in 0..MAXIMUM_STAGE_COUNT {
            self.curves[stage_index] = curve_stage_prototype.clone();
            Stage::prepare(&mut self.curves[stage_index], self.sample_rate);
        }
        self.update_time_constants();
    }

    fn update_time_constants(&mut self) {
        // Blocking: ~1 ms grid-current charge; release defaults to the
        // coupling RC, or the explicit grid-leak recovery if set.
        let blocking_recovery_milliseconds = if self.blocking_recovery_milliseconds > 0.0 {
            self.blocking_recovery_milliseconds
        } else {
            1000.0 / (2.0 * core::f32::consts::PI * self.coupling_hz)
        };
        for blocking_envelope in self.blocking_envelopes.iter_mut() {
            blocking_envelope.set_times(EnvelopeTimes {
                attack_ms: 1.0,
                release_ms: blocking_recovery_milliseconds,
                slow_release_ms: None,
            });
            blocking_envelope.retime(self.sample_rate);
        }
        for (stage_index, cathode_envelope) in self.cathode_envelopes.iter_mut().enumerate() {
            let cathode_time_milliseconds = if self.stage_cathode_settings[stage_index].1 > 0.0 {
                self.stage_cathode_settings[stage_index].1
            } else {
                self.cathode_time_milliseconds
            };
            cathode_envelope.set_times(EnvelopeTimes {
                attack_ms: cathode_time_milliseconds,
                release_ms: cathode_time_milliseconds,
                slow_release_ms: None,
            });
            cathode_envelope.retime(self.sample_rate);
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.triode_prototype.prepare(sample_rate);
        self.rebuild_stages();
        self.reset();
    }

    pub fn reset(&mut self) {
        for stage_index in 0..MAXIMUM_STAGE_COUNT {
            Stage::reset(&mut self.couplers[stage_index]);
            Stage::reset(&mut self.curves[stage_index]);
        }
        self.stage_outputs = [0.0; MAXIMUM_STAGE_COUNT];
        for envelope in self
            .blocking_envelopes
            .iter_mut()
            .chain(self.cathode_envelopes.iter_mut())
        {
            envelope.reset();
        }
        self.blocking_voltages = [0.0; MAXIMUM_STAGE_COUNT];
        self.cathode_bias_shifts = [0.0; MAXIMUM_STAGE_COUNT];
    }

    /// Coupling cutoff (Hz). Sets both the DC-block high-pass AND the
    /// blocking-distortion recovery time (`Rg·Cc = 1/(2π·fc)`) — they're
    /// the same RC. ~5–20 Hz is typical.
    pub fn set_coupling_hz(&mut self, frequency_hz: f32) {
        self.coupling_hz = frequency_hz.clamp(1.0, 200.0);
        for stage_index in 0..self.stage_count {
            if self.stage_coupling_frequencies_hz[stage_index] <= 0.0 {
                self.couplers[stage_index].set_corner(self.coupling_hz, self.sample_rate);
            }
        }
        self.update_time_constants();
    }

    pub fn set_stage_count(&mut self, stage_count: usize) {
        let stage_count = stage_count.clamp(1, MAXIMUM_STAGE_COUNT);
        if stage_count != self.stage_count {
            self.stage_count = stage_count;
            self.rebuild_stages();
        }
    }
    pub fn set_tube(&mut self, tube: Koren) {
        if tube != self.triode_prototype.tube() {
            self.triode_prototype.set_tube(tube);
            self.rebuild_stages();
        }
    }
    /// Per-stage coupler corner (Hz; 0 restores the cascade-wide
    /// corner). Rebuilds stages — block-rate safe like `set_stage_count`.
    pub fn set_stage_coupling_hz(&mut self, stage_index: usize, frequency_hz: f32) {
        if stage_index < MAXIMUM_STAGE_COUNT
            && self.stage_coupling_frequencies_hz[stage_index] != frequency_hz
        {
            self.stage_coupling_frequencies_hz[stage_index] = frequency_hz;
            self.rebuild_stages();
        }
    }
    /// Per-stage bias offset in volts colder — the cold clipper.
    pub fn set_stage_bias_offset(&mut self, stage_index: usize, volts: f32) {
        if stage_index < MAXIMUM_STAGE_COUNT {
            self.stage_bias_offsets_volts[stage_index] = volts.max(0.0);
        }
    }
    pub fn set_drive_db(&mut self, drive_decibels: f32) {
        // Drive re-frames; the table is drive-independent (no rebuild).
        self.triode_prototype
            .set_grid_drive(drive_volts(drive_decibels));
    }
    /// Level policy for every stage (see `LevelPolicy` on the triode):
    /// `Calibrated` = drive buys colour at constant level (effects);
    /// `Physical` = drive buys gain into a fixed ceiling (amps — the
    /// knee LADDER a cascade needs for envelopes to survive it).
    pub fn set_level_policy(&mut self, policy: LevelPolicy) {
        self.triode_prototype.set_level_policy(policy);
    }
    /// The cascade's REPORTED output ceiling (dBFS) — the last stage's
    /// plate ceiling, which IS the cascade's (everything upstream is
    /// re-limited by it). With `linear_gain_db` this locates the rig's
    /// knee: consumers compute makeup as min(level+gain, ceiling).
    pub fn ceiling_db(&self) -> f32 {
        self.triode_prototype.ceiling_db()
    }
    /// The cascade's REPORTED small-signal insertion gain (dB) at the
    /// current settings: n·(stage gain) + (n−1)·interstage. Consumers
    /// compute makeup from this — never fit it (the cell law
    /// `the_report_is_the_truth` backs the per-stage term).
    pub fn linear_gain_db(&self) -> f32 {
        let stage = self.stage_gain_db();
        let inter = self.interstage_db();
        self.stage_count as f32 * stage + (self.stage_count.saturating_sub(1)) as f32 * inter
    }
    /// One stage's REPORTED small-signal gain (dB) — for consumers
    /// walking a level through the cascade stage by stage (cumulative
    /// knee compression, which a single lumped knee can't model).
    pub fn stage_gain_db(&self) -> f32 {
        self.triode_prototype.small_signal_gain_db()
    }
    /// The interstage divider (dB), as set.
    pub fn interstage_db(&self) -> f32 {
        20.0 * self.interstage_gain_amplitude.max(1e-9).log10()
    }
    /// Active stage count.
    pub fn stage_count(&self) -> usize {
        self.stage_count
    }
    /// Normalized aggregate plate current at the last sample
    /// (1 = idle; conduction pushes above, cutoff below). Feed a
    /// supply from this — the physical B+ draw.
    pub fn supply_draw(&self) -> f32 {
        self.normalized_supply_draw
    }
    /// Supply scale (B+ relative to idle) (per-sample cheap: it lands
    /// in the frames the next `process_sample` writes anyway). Sag
    /// is THE mechanism that moves a pinned cascade's envelope.
    pub fn set_supply_scale(&mut self, supply_scale: f32) {
        self.supply_scale = supply_scale.clamp(0.05, 1.0);
    }
    /// Blocking-distortion severity: 0 = off (clean cascade), 1 ≈ physical,
    /// >1 exaggerated.
    pub fn set_blocking_depth(&mut self, depth: f32) {
        self.blocking_depth = depth.max(0.0);
    }
    /// Blocking recovery (grid-leak) time, ms; 0 restores the
    /// coupling-RC tie. Independent by design — see the atlas.
    pub fn set_blocking_recovery_ms(&mut self, recovery_milliseconds: f32) {
        self.blocking_recovery_milliseconds = recovery_milliseconds.max(0.0);
        self.update_time_constants();
    }
    /// Inter-stage gain (dB) — escalates level into successive stages so
    /// downstream stages overdrive, the way a real cascaded preamp does.
    pub fn set_interstage_gain_db(&mut self, gain_decibels: f32) {
        self.interstage_gain_amplitude = db_to_amp(gain_decibels);
    }
    /// TRUE static peak response of the cascade (idle slow states,
    /// current supply): half the peak-to-peak swing walked stage by
    /// stage through the REAL curve. The honest basis for slope
    /// staging — the report's derivative is the cascade's, not a
    /// knee model's.
    pub fn static_peak_out(&self, input_peak: f64) -> f64 {
        let mut stage_peak = input_peak;
        for stage_index in 0..self.stage_count {
            let stage_input_gain = if stage_index > 0 {
                f64::from(self.interstage_gain_amplitude)
            } else {
                1.0
            };
            let positive_output = self
                .triode_prototype
                .static_transfer(stage_input_gain * stage_peak);
            let negative_output = self
                .triode_prototype
                .static_transfer(-stage_input_gain * stage_peak);
            stage_peak =
                0.5 * (positive_output - negative_output).abs() * f64::from(self.supply_scale);
        }
        stage_peak
    }

    /// Per-stage cathode self-bias (applied to every stage).
    pub fn set_cathode_bias(&mut self, depth: f32, time_milliseconds: f32) {
        self.cathode_depth = depth.max(0.0);
        self.cathode_time_milliseconds = time_milliseconds.max(0.1);
        self.update_time_constants();
    }
    /// Per-stage cathode NETWORK override: this stage's bypass
    /// depth and RC. A zero time returns the stage to the
    /// cascade-wide setting. Composable per stage, like the
    /// hardware it models.
    pub fn set_stage_cathode(&mut self, stage_index: usize, depth: f32, time_milliseconds: f32) {
        if stage_index < MAXIMUM_STAGE_COUNT {
            self.stage_cathode_settings[stage_index] = (depth.max(0.0), time_milliseconds.max(0.0));
            self.update_time_constants();
        }
    }

    /// Process one sample through the cascade.
    #[inline]
    pub fn process_sample(&mut self, input_sample: f32) -> f32 {
        let (grid_bias, grid_drive, output_scale) = self.triode_prototype.frame_params();
        // 1) Frames from LAST sample's driver states (slow states; the
        //    one-sample lag is immaterial and keeps ADAA commit clean).
        for stage_index in 0..self.stage_count {
            let drive = grid_drive as f32
                * if stage_index > 0 {
                    self.interstage_gain_amplitude
                } else {
                    1.0
                };
            let frame = Frame {
                bias: grid_bias as f32
                    - self.blocking_voltages[stage_index]
                    - self.cathode_bias_shifts[stage_index]
                    - self.stage_bias_offsets_volts[stage_index],
                drive,
                // The supply scale rides the output frame: B+ droop
                // moves ceiling and gain together (sag = the only
                // mechanism that moves a pinned cascade's envelope).
                gain: output_scale as f32 * self.supply_scale,
            };
            self.curves[stage_index].set_pending(frame);
        }
        // 2) The cascade — flat: coupler then curve, per stage, the
        // exact slot order the chain ran.
        let mut output_sample = input_sample;
        for stage_index in 0..self.stage_count {
            output_sample = Stage::tick(&mut self.couplers[stage_index], output_sample);
            output_sample = Stage::tick(&mut self.curves[stage_index], output_sample);
            self.stage_outputs[stage_index] = output_sample;
        }
        // 3) Advance drivers from this sample's senses/taps.
        let (
            cathode_resistance_ohms,
            idle_cathode_voltage,
            idle_plate_current,
            plate_resistance_ohms,
        ) = self.cathode_constants;
        let mut aggregate_plate_current = 0.0f64;
        for stage_index in 0..self.stage_count {
            let (excess_grid_conduction, normalized_stage_output) = {
                let stage_output = f64::from(self.stage_outputs[stage_index]);
                (
                    f64::from(self.curves[stage_index].sense_tap()),
                    stage_output / output_scale.max(1e-9),
                )
            };
            // Grid current saturates (clamp + source resistance) — the
            // same saturating map as the previous implementation.
            let grid_current = excess_grid_conduction / (1.0 + 0.5 * excess_grid_conduction);
            let blocking_target = (grid_current * self.blocking_depth as f64).min(8.0);
            self.blocking_voltages[stage_index] =
                self.blocking_envelopes[stage_index].tick(blocking_target as f32);
            aggregate_plate_current +=
                (idle_plate_current + normalized_stage_output / plate_resistance_ohms).max(0.0);
            let cathode_depth = if self.stage_cathode_settings[stage_index].1 > 0.0 {
                self.stage_cathode_settings[stage_index].0
            } else {
                self.cathode_depth
            };
            if cathode_depth > 0.0 {
                let plate_current =
                    (idle_plate_current + normalized_stage_output / plate_resistance_ohms).max(0.0);
                let cathode_voltage = self.cathode_envelopes[stage_index]
                    .tick((plate_current * cathode_resistance_ohms) as f32);
                self.cathode_bias_shifts[stage_index] = (cathode_depth
                    * (cathode_voltage - idle_cathode_voltage as f32))
                    .clamp(-1.0, 6.0);
            } else {
                self.cathode_bias_shifts[stage_index] = 0.0;
            }
        }
        self.normalized_supply_draw = (aggregate_plate_current
            / (idle_plate_current * self.stage_count.max(1) as f64))
            .max(0.0) as f32;
        output_sample
    }
    /// Test/inspection: current blocking voltage at a stage.
    pub fn blocking_voltage(&self, stage_index: usize) -> f64 {
        self.blocking_voltages[stage_index.min(MAXIMUM_STAGE_COUNT - 1)] as f64
    }
    /// Gain reduction (dB, ≤0) at a stage from its current bias shift —
    /// the audible "choke" of blocking distortion (and cathode bias).
    pub fn stage_gain_reduction_db(&mut self, stage_index: usize) -> f32 {
        let stage_index = stage_index.min(MAXIMUM_STAGE_COUNT - 1);
        let (grid_bias, _, _) = self.triode_prototype.frame_params();
        let bias_shift =
            (self.blocking_voltages[stage_index] + self.cathode_bias_shifts[stage_index]) as f64;
        let differentiation_interval = 0.05;
        let curve = self.curves[stage_index].curve();
        let slope = |voltage: f64| {
            (curve.f_at(voltage + differentiation_interval)
                - curve.f_at(voltage - differentiation_interval))
            .abs()
                / (2.0 * differentiation_interval)
        };
        let now = slope(grid_bias - bias_shift).max(1e-9);
        let rest = slope(grid_bias).max(1e-9);
        (20.0 * (now / rest).log10()) as f32
    }
}

/// The drive knob's clean end: grid volts per unit at 0 dB.
const DRIVE_CLEAN_VOLTS: f64 = 0.4;

/// Unity small-signal gain at the drive knob's clean end; gain follows
/// the knob from there.
pub(super) const UNITY_AT_CLEAN: LevelPolicy = LevelPolicy::Physical {
    reference_drive: DRIVE_CLEAN_VOLTS,
};

/// Anvil's drive knob law: 0–20 dB onto grid volts per unit of input,
/// an anchored exponential — clean at 0 (0.4 V), saturation audible by
/// ~20 % of the knob, extreme at the top (110 V).
fn drive_volts(drive_db: f32) -> f64 {
    const MAX_DB: f64 = 20.0;
    const SPAN_VOLTS: f64 = 110.0;
    const CURVE: f64 = 1.5;
    let position = (f64::from(drive_db) / MAX_DB).clamp(0.0, 1.0);
    DRIVE_CLEAN_VOLTS
        + SPAN_VOLTS * (10f64.powf(CURVE * position) - 1.0) / (10f64.powf(CURVE) - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_dsp::Oversampler2xFir;

    fn goertzel(buf: &[f32], f: f64, sample_rate: f64) -> f64 {
        let w = 2.0 * core::f64::consts::PI * f / sample_rate;
        let cw = w.cos();
        let coeff = 2.0 * cw;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in buf {
            let s0 = x as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let re = s1 - s2 * cw;
        let im = s2 * w.sin();
        (re * re + im * im).sqrt() / (buf.len() as f64 * 0.5)
    }
    fn rms(buf: &[f32]) -> f64 {
        (buf.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / buf.len() as f64).sqrt()
    }
    fn thd(buf: &[f32], f0: f64, sample_rate: f64) -> f64 {
        let h1 = goertzel(buf, f0, sample_rate);
        (2..=8)
            .map(|k| goertzel(buf, k as f64 * f0, sample_rate))
            .map(|h| h * h)
            .sum::<f64>()
            .sqrt()
            / h1.max(1e-9)
    }

    /// THE CASCADE REPORT IS THE TRUTH — `linear_gain_db` matches the
    /// measured small-signal insertion gain under both policies, at a
    /// probe frequency well above the couplers. The assembly-level
    /// counterpart of the triode's law: makeup is computed, not fitted.
    #[test]
    fn the_cascade_report_is_the_truth() {
        let sample_rate = 48_000.0;
        let f0 = 2_000.0;
        let n = 32_768;
        for (policy, drive, inter) in [
            (LevelPolicy::Calibrated, 8.0f32, 2.5f32),
            (UNITY_AT_CLEAN, 0.0, 0.0),
            (UNITY_AT_CLEAN, 6.0, -14.0),
            (UNITY_AT_CLEAN, 10.0, -20.0),
        ] {
            let mut p = RcCoupledTriodeCascade::new();
            p.set_stage_count(3);
            p.prepare(sample_rate as f32);
            p.set_level_policy(policy);
            p.set_drive_db(drive);
            p.set_interstage_gain_db(inter);
            let reported = f64::from(p.linear_gain_db());
            // Small enough that even the reported-gain-amplified level
            // stays sub-knee at every stage.
            let amp = 10f64.powf((-66.0 - reported.max(0.0)) / 20.0);
            let mut out = vec![0.0f32; n];
            for i in 0..n {
                let s = (amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / sample_rate).sin())
                    as f32;
                out[i] = p.process_sample(s);
            }
            let measured = 20.0
                * (goertzel(&out[n / 2..], f0, sample_rate) / amp)
                    .max(1e-12)
                    .log10();
            println!("{policy:?} drive {drive} inter {inter}: reported {reported:.2} measured {measured:.2}");
            assert!(
                (measured - reported).abs() < 2.0,
                "{policy:?} drive {drive}: reported {reported:.2} dB, measured {measured:.2} dB"
            );
        }
    }

    /// ATLAS EXPLORER — the blocking curve family:
    /// gain dip and recovery after a loud->quiet step, vs blocking
    /// depth, coupling corner (which currently OWNS the recovery
    /// RC), and how deep past the knee the loud passage sat. Run:
    /// cargo test -p stave-dsp --lib -- --ignored blocking_step_family --nocapture
    #[test]
    #[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
    fn blocking_step_family() {
        let sample_rate = 48_000.0f32;
        let f0 = 220.0f64;
        for &policy in &[UNITY_AT_CLEAN] {
            for &depth_db in &[0.0f32, 6.0, 12.0] {
                for &block in &[1.0f32, 2.0] {
                    {
                        let &hz = &12.0f32;
                        let _ = hz;
                    }
                    for &rec in &[0.0f32, 80.0, 200.0, 400.0] {
                        let hz = 12.0f32;
                        let mut p = RcCoupledTriodeCascade::new();
                        p.set_stage_count(1);
                        p.prepare(sample_rate);
                        p.set_level_policy(policy);
                        p.set_drive_db(10.0);
                        p.set_coupling_hz(hz);
                        p.set_blocking_depth(block);
                        p.set_blocking_recovery_ms(rec);
                        // Place the LOUD level depth_db past the knee:
                        // knee input = ceiling - gain.
                        let knee_in = p.ceiling_db() - p.linear_gain_db();
                        let loud = 10f64.powf(f64::from(knee_in + depth_db) / 20.0);
                        let quiet = loud * 10f64.powf(-30.0 / 20.0);
                        // 300 ms loud, then quiet; track quiet-side gain vs
                        // a never-loud baseline.
                        let n_loud = (0.3 * sample_rate as f64) as usize;
                        let n_tail = (0.4 * sample_rate as f64) as usize;
                        let render = |p: &mut RcCoupledTriodeCascade, do_loud: bool| -> Vec<f32> {
                            let mut out = vec![0.0f32; n_loud + n_tail];
                            for i in 0..n_loud + n_tail {
                                let amp = if i < n_loud {
                                    if do_loud {
                                        loud
                                    } else {
                                        quiet
                                    }
                                } else {
                                    quiet
                                };
                                let x = (amp
                                    * (2.0 * core::f64::consts::PI * f0 * i as f64
                                        / f64::from(sample_rate))
                                    .sin()) as f32;
                                out[i] = p.process_sample(x);
                            }
                            out
                        };
                        let stepped = render(&mut p, true);
                        let mut p2 = RcCoupledTriodeCascade::new();
                        p2.set_stage_count(1);
                        p2.prepare(sample_rate);
                        p2.set_level_policy(policy);
                        p2.set_drive_db(10.0);
                        p2.set_coupling_hz(hz);
                        p2.set_blocking_depth(block);
                        p2.set_blocking_recovery_ms(rec);
                        let base = render(&mut p2, false);
                        // Tone-locked (Goertzel at f0): broadband rms
                        // reads the bias-recovery DC RAMP as energy and
                        // inverted the sign of the whole family — the
                        // probe must measure the SIGNAL's gain.
                        let g_at = |y: &[f32], ms: f32| {
                            let a = n_loud + (ms * 0.001 * sample_rate) as usize;
                            let w = (0.02 * sample_rate) as usize;
                            20.0 * (goertzel(&y[a..a + w], f0, f64::from(sample_rate))
                                / goertzel(&base[a..a + w], f0, f64::from(sample_rate)).max(1e-12))
                            .log10()
                        };
                        // Gain trajectory after the drop (40 ms on —
                        // past the coupler's own step transient).
                        let tr: Vec<f64> = [40.0f32, 100.0, 200.0, 350.0]
                            .iter()
                            .map(|&ms| g_at(&stepped, ms))
                            .collect();
                        println!(
                            "depth+{depth_db:2.0} block {block:4.2} rec_set {rec:3.0} | gain @40/100/200/350ms: {:6.1} {:6.1} {:6.1} {:6.1} dB",
                            tr[0], tr[1], tr[2], tr[3]
                        );
                    }
                }
            }
        }
    }

    /// ATLAS EXPLORER — the dynamics-absorption family: static
    /// input->output transfer of one Physical stage and of a
    /// 4-stage cascade (anvil-ish dividers), Goertzel-locked.
    /// The plateau's slope deficit IS the absorption; cascades
    /// accumulate it. Run:
    /// cargo test -p stave-dsp --lib -- --ignored absorption_family --nocapture
    #[test]
    #[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
    fn absorption_family() {
        let sample_rate = 48_000.0f32;
        let f0 = 220.0f64;
        let n = 16_384usize;
        for (label, stages, inter) in [("1 stage ", 1usize, 0.0f32), ("4 stages", 4, -40.0)] {
            let mut line = format!("{label} drive10 | out dB at in -36..0: ");
            for in_db in [-36i32, -30, -24, -18, -12, -6, 0] {
                let mut p = RcCoupledTriodeCascade::new();
                p.set_stage_count(stages);
                p.prepare(sample_rate);
                p.set_level_policy(UNITY_AT_CLEAN);
                p.set_drive_db(10.0);
                p.set_interstage_gain_db(inter);
                p.set_blocking_depth(0.25);
                let amp = 10f64.powf(f64::from(in_db) / 20.0);
                let mut out = vec![0.0f32; n];
                for i in 0..n {
                    let x = (amp
                        * (2.0 * core::f64::consts::PI * f0 * i as f64 / f64::from(sample_rate))
                            .sin()) as f32;
                    out[i] = p.process_sample(x);
                }
                let g = goertzel(&out[n / 2..], f0, f64::from(sample_rate));
                line.push_str(&format!("{:6.1}", 20.0 * g.max(1e-12).log10()));
            }
            println!("{line}");
        }
    }

    #[test]
    fn coupling_blocks_dc() {
        // The coupling caps must remove DC (a sustained offset → ~0 mean).
        let sample_rate = 48_000.0;
        let mut p = RcCoupledTriodeCascade::new();
        p.set_stage_count(2);
        p.prepare(sample_rate as f32);
        p.set_drive_db(6.0);
        let n = sample_rate as usize; // 1 s to settle the high-pass
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            out[i] = p.process_sample(0.5); // constant DC
        }
        let tail = &out[n / 2..];
        let mean = tail.iter().map(|&x| x as f64).sum::<f64>() / tail.len() as f64;
        println!("DC through 2-stage cascade: residual mean {mean:.2e}");
        assert!(mean.abs() < 1e-3, "coupling did not block DC: {mean}");
    }

    #[test]
    fn cascade_compounds_saturation() {
        // More stages ⇒ more harmonic content for the same drive.
        let sample_rate = 48_000.0;
        let f0 = 1_000.0;
        let n = 16_384;
        let thd_for = |stages: usize| -> f64 {
            let mut p = RcCoupledTriodeCascade::new();
            p.set_stage_count(stages);
            p.prepare(sample_rate as f32);
            p.set_drive_db(5.0);
            p.set_interstage_gain_db(3.0);
            p.set_blocking_depth(0.0); // isolate saturation
            for i in 0..n {
                let _ = p.process_sample(
                    0.3 * (2.0 * core::f64::consts::PI * f0 * i as f64 / sample_rate).sin() as f32,
                );
            }
            let mut out = vec![0.0f32; n];
            for i in 0..n {
                out[i] = p.process_sample(
                    0.3 * (2.0 * core::f64::consts::PI * f0 * i as f64 / sample_rate).sin() as f32,
                );
            }
            thd(&out, f0, sample_rate)
        };
        let one = thd_for(1);
        let three = thd_for(3);
        println!(
            "THD: 1 stage {:.1}%, 3 stages {:.1}%",
            one * 100.0,
            three * 100.0
        );
        assert!(
            three > one * 1.2,
            "cascade should compound saturation ({one} → {three})"
        );
    }

    #[test]
    fn blocking_distortion_chokes_and_recovers() {
        // The headline behaviour. Drive the grid into conduction: the
        // blocking voltage must build (choke) and, on silence, recover with
        // ~the coupling RC. With depth 0 it must NOT build (clean cascade).
        let sample_rate = 48_000.0;
        let f0 = 300.0;
        let coupling_hz = 8.0;
        let measure = |depth: f32| -> (f64, f64, f64) {
            let mut p = RcCoupledTriodeCascade::new();
            p.set_stage_count(2);
            p.prepare(sample_rate as f32);
            p.set_coupling_hz(coupling_hz);
            p.set_drive_db(16.0);
            p.set_blocking_depth(depth);
            p.set_interstage_gain_db(6.0);
            // Sustained loud burst.
            let burst = (sample_rate * 0.2) as usize;
            let fund = |buf: &[f32]| goertzel(buf, f0, sample_rate);
            let win = (sample_rate * 0.02) as usize;
            let mut early = vec![0.0f32; win];
            let mut late = vec![0.0f32; win];
            for i in 0..burst {
                let s =
                    0.8 * (2.0 * core::f64::consts::PI * f0 * i as f64 / sample_rate).sin() as f32;
                let y = p.process_sample(s);
                if i < win {
                    early[i] = y;
                } else if i >= burst - win {
                    late[i - (burst - win)] = y;
                }
            }
            let _ = (&early, &late, fund); // (fundamental droop is confounded by clipping)
            let vpeak = p.blocking_voltage(1);
            let gr = p.stage_gain_reduction_db(1); // audible choke (bias → less gain)
                                                   // Recovery: feed silence, time blocking_voltages to fall to 37 % of peak.
            let mut recover_samples = sample_rate as usize; // default: didn't recover in 1 s
            for k in 0..(sample_rate as usize) {
                let _ = p.process_sample(0.0);
                if p.blocking_voltage(1) < vpeak * 0.37 {
                    recover_samples = k;
                    break;
                }
            }
            (
                vpeak,
                gr as f64,
                recover_samples as f64 / sample_rate * 1000.0,
            )
        };

        let (v0, gr0, _) = measure(0.0);
        let (v3, gr3, recover_ms) = measure(3.0);
        println!("blocking: depth0 blocking_voltages {v0:.3} GR {gr0:+.1} dB | depth3 blocking_voltages {v3:.2} GR {gr3:+.1} dB recover {recover_ms:.0} ms");

        // Depth 0: clean cascade, no bias drift, no choke.
        assert!(v0 < 0.05, "no blocking expected at depth 0: {v0}");
        assert!(gr0.abs() < 0.3, "no choke expected at depth 0: {gr0}");
        // Depth 3: blocking builds and chokes the stage (gain reduction).
        assert!(v3 > 0.5, "blocking voltage should build: {v3}");
        assert!(gr3 < -1.0, "blocking should choke the stage (GR {gr3})");
        // Recovery ≈ the coupling RC (τ = 1/(2π·fc) ≈ 20 ms here).
        let tau_ms = 1000.0 / (2.0 * core::f64::consts::PI * coupling_hz as f64);
        println!("expected recovery τ ≈ {tau_ms:.0} ms");
        assert!(
            recover_ms > tau_ms * 0.4 && recover_ms < tau_ms * 3.0,
            "recovery τ off: {recover_ms} ms vs {tau_ms}"
        );
    }

    #[test]
    fn cascade_is_stable_at_extremes() {
        let sample_rate = 48_000.0;
        let f0 = 220.0;
        let mut p = RcCoupledTriodeCascade::new();
        p.set_stage_count(MAXIMUM_STAGE_COUNT);
        p.prepare(sample_rate as f32);
        p.set_drive_db(20.0);
        p.set_blocking_depth(5.0);
        p.set_interstage_gain_db(12.0);
        let mut mx = 0.0f32;
        for i in 0..(sample_rate as usize) {
            let s = 0.95 * (2.0 * core::f64::consts::PI * f0 * i as f64 / sample_rate).sin() as f32;
            let y = p.process_sample(s);
            assert!(y.is_finite(), "non-finite in extreme cascade");
            mx = mx.max(y.abs());
        }
        assert!(mx < 8.0, "cascade runaway: {mx}");
    }

    #[test]
    fn cascade_in_oversampler_is_alias_clean() {
        // Three saturating stages inside one 2× oversampler at a low
        // fundamental: all harmonics are in-band, so the noise floor (the
        // non-harmonic energy) must stay low — the cascade doesn't fold.
        let sample_rate = 48_000.0;
        let n = 8192usize;
        let m = 38usize; // ~222 Hz, bin-centred
                         // Realistic substantial setting (2 stages, moderate drive),
                         // through the linear-phase FIR half-band stack that replaced the
                         // Butterworth ceiling. Prepared at 4× — the rate the cascade
                         // actually ticks inside the nested oversamplers (rate contract).
        let mut p = RcCoupledTriodeCascade::new();
        p.set_stage_count(2);
        p.prepare(sample_rate as f32 * 4.0);
        p.set_drive_db(8.0);
        p.set_blocking_depth(0.5);
        // A heavy cascade generates far more HF than a single stage, so it
        // needs 4× (nested 2×) — the standard for cascaded amp sims.
        let mut os = Oversampler2xFir::new();
        os.prepare(sample_rate as f32);
        let mut os2 = Oversampler2xFir::new();
        os2.prepare(sample_rate as f32 * 2.0);
        let sample = |p: &mut RcCoupledTriodeCascade,
                      os: &mut Oversampler2xFir,
                      os2: &mut Oversampler2xFir,
                      s: f32|
         -> f32 {
            os.process_one(s, |a| os2.process_one(a, |b| p.process_sample(b)))
        };
        let render = |p: &mut RcCoupledTriodeCascade,
                      os: &mut Oversampler2xFir,
                      os2: &mut Oversampler2xFir|
         -> Vec<f32> {
            for i in 0..n {
                let s = 0.5
                    * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32;
                let _ = sample(p, os, os2, s);
            }
            let mut out = vec![0.0f32; n];
            for i in 0..n {
                let s = 0.5
                    * (2.0 * core::f64::consts::PI * m as f64 * i as f64 / n as f64).sin() as f32;
                out[i] = sample(p, os, os2, s);
            }
            out
        };
        let out = render(&mut p, &mut os, &mut os2);
        let bin_hz = |b: usize| b as f64 * sample_rate / n as f64;
        let h1 = goertzel(&out, bin_hz(m), sample_rate);
        let mut noise = 0.0f64;
        // Energy at EXACT non-harmonic integer bins (not multiples of m).
        // For a signal periodic in N these bins are zero except for alias,
        // so this isolates true aliasing (no spectral leakage from the
        // strong harmonics, which evaluating at off-bin Hz would catch).
        for &b in &[255usize, 511, 1021, 2043, 3071] {
            debug_assert!(b % m != 0);
            noise += goertzel(&out, bin_hz(b), sample_rate).powi(2);
        }
        let nf = 10.0 * (noise.sqrt() / h1.max(1e-9)).log10();
        println!(
            "2-stage cascade noise floor (non-harmonic): {nf:.1} dB | out_rms {:.3}",
            rms(&out)
        );
        assert!(nf < -45.0, "cascade aliases: {nf} dB");
    }
}
