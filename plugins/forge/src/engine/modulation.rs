//! Modulation — the plane that displaces structure from what the
//! knobs said: four driver cells (detectors and an oscillator) feed
//! a route matrix whose targets are stage parameters. Routes are
//! DATA, rebuilt per block from the MEMORY mode; base frames are
//! the knob values that routes displace from, so automation and
//! modulation never fight.

use stave_dsp::{Driver, EnvelopeFollower, EnvelopeTimes, FilterSpec, ModMatrix, Route, Target};

use super::strip::AmpChain;

/// The mod matrix's driver lanes, by NAME (the wiring below and the
/// per-frame detector feed in render.rs both read these).
pub(super) mod driver {
    /// Slow supply cell — sag and squeeze follow program level.
    pub(in crate::engine) const LEVEL: usize = 0;
    /// Fast blocking cell — fed the primary curve's conduction sense.
    pub(in crate::engine) const CONDUCTION: usize = 1;
    /// Env-scaled 120 Hz supply ripple.
    pub(in crate::engine) const RIPPLE: usize = 2;
    /// SubOctave divider — fed the SIGNED mid, needs zero crossings.
    pub(in crate::engine) const SUB: usize = 3;
}

/// The modulation plane: four driver cells (named in `driver`),
/// routes as data rebuilt per block, and the block-rate state the
/// apply pass reads. Base frames are the knob values that routes
/// displace from — automation and modulation never fight.
pub(super) struct Modulation {
    matrix: ModMatrix<4, 6>,
    sub_mix: f32,
    sub_mod: f32,
    active: bool,
    base_tone: FilterSpec,
    memory_on: bool,
    blocking_on: bool,
    width: f32,
}

impl Modulation {
    /// The four driver cells, wired to their named lanes (see
    /// `driver`): slow supply, fast blocking, env-scaled ripple, and
    /// the sub-octave divider.
    pub(super) fn new() -> Self {
        Self {
            matrix: ModMatrix::new([
                Driver::Env(EnvelopeFollower::new(EnvelopeTimes {
                    attack_ms: 12.0,
                    release_ms: 220.0,
                    slow_release_ms: None,
                })),
                Driver::Env(EnvelopeFollower::new(EnvelopeTimes {
                    attack_ms: 1.0,
                    release_ms: 120.0,
                    slow_release_ms: None,
                })),
                Driver::lfo_env_scaled(120.0, 8.0, 80.0),
                Driver::sub(),
            ]),
            sub_mix: 0.0,
            sub_mod: 0.0,
            active: false,
            base_tone: FilterSpec::HighShelf { hz: 900.0, db: 0.0 },
            memory_on: false,
            blocking_on: false,
            width: 0.5,
        }
    }
}

impl Modulation {
    /// Block-rate state handoff from the binding pass — the wiring
    /// mix levels and the interstage tone routes displace from.
    pub(super) fn begin_block(
        &mut self,
        width: f32,
        sub_mix: f32,
        sub_mod: f32,
        base_tone: FilterSpec,
    ) {
        self.width = width;
        self.sub_mix = sub_mix;
        self.sub_mod = sub_mod;
        self.base_tone = base_tone;
    }
    pub(super) fn set_active(&mut self, active: bool) {
        self.active = active;
    }
    /// Is any modulation live this block? (The frame loop skips the
    /// whole plane when not.)
    #[inline]
    pub(super) fn is_active(&self) -> bool {
        self.active
    }
    #[inline]
    pub(super) fn memory_on(&self) -> bool {
        self.memory_on
    }
    #[inline]
    pub(super) fn blocking_on(&self) -> bool {
        self.blocking_on
    }
    #[inline]
    pub(super) fn sub_mix(&self) -> f32 {
        self.sub_mix
    }
    /// Advance the driver cells one frame from their named feed.
    #[inline]
    pub(super) fn tick(&mut self, feed: &[f32; 4]) -> [f32; 4] {
        self.matrix.tick_states(feed)
    }
    /// Apply this frame's driver states to one strip: base frames
    /// displace on the nonlinear slots, tilt on the voicing filter.
    #[inline]
    pub(super) fn displace(&self, states: &[f32; 4], strip: &mut super::strip::Strip) {
        let frames_base = [
            (strip.amp().primary, strip.primary_base),
            (strip.amp().second, strip.second_base),
        ];
        let tilt: &[(stave_dsp::StageId, FilterSpec)] = &[(strip.amp().voicing, self.base_tone)];
        self.matrix
            .apply(states, strip.amp_mut().as_chain_mut(), &frames_base, tilt);
    }
    pub(super) fn prepare(&mut self, rate: f32) {
        self.matrix.prepare(rate);
    }
}

impl Modulation {
    /// Route table for the current MEMORY mode. Targets follow the
    /// stack: when primary is the screamer RC (no Frame), frame routes
    /// land on second instead.
    pub(super) fn rebuild_routes(
        &mut self,
        amp: &AmpChain,
        memory: usize,
        depth: f32,
        stack: usize,
        sub_mod: f32,
    ) {
        self.matrix.clear_routes();
        self.memory_on = memory != 0 && depth > 1e-3;
        self.blocking_on = memory == 2 && self.memory_on;
        // The divider modulates whichever primary stage carries a Frame.
        if sub_mod > 1e-3 {
            let frame_stage = if stack == 2 { amp.second } else { amp.primary };
            self.matrix.push_route(Route {
                driver: driver::SUB,
                stage: frame_stage,
                target: Target::Gain,
                amount: 0.5 * sub_mod,
            });
        }
        if !self.memory_on {
            return;
        }
        let d = depth;
        // Frame routes only bind to Curve stages.
        let frame_stage = if stack == 2 { amp.second } else { amp.primary };
        match memory {
            1 => {
                // SAG — supply give: sustained level pulls drive down,
                // clamps the output swing (lower B+ = lower max swing),
                // and darkens the interstage voicing.
                self.matrix.drivers()[driver::LEVEL].set_times(EnvelopeTimes {
                    attack_ms: 12.0,
                    release_ms: 220.0,
                    slow_release_ms: None,
                });
                self.matrix.push_route(Route {
                    driver: driver::LEVEL,
                    stage: frame_stage,
                    target: Target::Drive,
                    amount: -0.5 * d,
                });
                self.matrix.push_route(Route {
                    driver: driver::LEVEL,
                    stage: frame_stage,
                    target: Target::Gain,
                    amount: 0.35 * d,
                });
                self.matrix.push_route(Route {
                    driver: driver::LEVEL,
                    stage: amp.voicing,
                    target: Target::TiltDb,
                    amount: -8.0 * d,
                });
            }
            2 => {
                // BLOCKING — conduction charge chokes the bias (fast
                // attack, program-release). Detector = the curve's own
                // one-sided conduction sense column.
                self.matrix.push_route(Route {
                    driver: driver::CONDUCTION,
                    stage: frame_stage,
                    target: Target::Bias,
                    amount: -0.6 * d,
                });
            }
            3 => {
                // SQUEEZE — envelope gain compression + a push into the
                // first stage: breathing, LA-style.
                self.matrix.drivers()[driver::LEVEL].set_times(EnvelopeTimes {
                    attack_ms: 8.0,
                    release_ms: 180.0,
                    slow_release_ms: None,
                });
                self.matrix.push_route(Route {
                    driver: driver::LEVEL,
                    stage: frame_stage,
                    target: Target::Gain,
                    amount: 0.55 * d,
                });
                self.matrix.push_route(Route {
                    driver: driver::LEVEL,
                    stage: amp.voicing,
                    target: Target::TiltDb,
                    amount: -3.0 * d,
                });
            }
            _ => {
                // RIPPLE — env-scaled 120 Hz supply wobble on gain and
                // operating point: ghost sidebands at n·f0 ± 120 Hz that
                // ride program heat.
                self.matrix.push_route(Route {
                    driver: driver::RIPPLE,
                    stage: frame_stage,
                    target: Target::Gain,
                    amount: 0.28 * d,
                });
                // Small bias wobble: shifts the operating point AND
                // leaks a gated hum line at the ripple frequency itself
                // (bias modulation is additive to first order — real
                // supply ripple hums the same way). Kept below the
                // multiplicative sidebands.
                self.matrix.push_route(Route {
                    driver: driver::RIPPLE,
                    stage: frame_stage,
                    target: Target::Bias,
                    amount: 0.05 * d,
                });
            }
        }
    }
}
