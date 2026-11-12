//! One of halo's frequency-selective reservoirs: a supply-like droop
//! that falls under draw at one time constant and recovers at another.
//! Part of halo's declared impossible supply (see `dynamic_headroom`),
//! not a model of a circuit.

pub(super) struct Reservoir {
    voltage: f32,
    droop: f32,
    fall_ms: f32,
    recovery_ms: f32,
    sample_rate: f32,
    fall: f32,
    recovery: f32,
    draw: f32,
    draw_smoothing: f32,
}

/// Draw is smoothed before the reservoir responds: raw draw would make
/// the fall/recovery asymmetry an audio-rate nonlinearity.
const DRAW_SMOOTHING_MS: f32 = 8.0;

impl Reservoir {
    pub fn new() -> Self {
        Self {
            voltage: 1.0,
            droop: 0.0,
            fall_ms: 25.0,
            recovery_ms: 150.0,
            sample_rate: 48_000.0,
            fall: 0.0,
            recovery: 0.0,
            draw: 0.0,
            draw_smoothing: 0.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update();
        self.voltage = 1.0;
        self.draw = 0.0;
    }

    /// `droop`: how far full draw pulls the reservoir; the fall and
    /// recovery time constants in milliseconds.
    pub fn set(&mut self, droop: f32, fall_ms: f32, recovery_ms: f32) {
        (self.droop, self.fall_ms, self.recovery_ms) = (droop, fall_ms, recovery_ms);
        self.update();
    }

    fn update(&mut self) {
        let coefficient = |ms: f32| 1.0 - (-1.0 / (ms * 0.001 * self.sample_rate)).exp();
        self.fall = coefficient(self.fall_ms);
        self.recovery = coefficient(self.recovery_ms);
        self.draw_smoothing = coefficient(DRAW_SMOOTHING_MS);
    }

    /// Advance one sample at `draw` (0 idle, ~1 full); returns the
    /// reservoir's voltage scale.
    #[inline]
    pub fn tick(&mut self, draw: f32) -> f32 {
        self.draw += (draw - self.draw) * self.draw_smoothing;
        let target = 1.0 - self.droop * self.draw;
        let rate = if target < self.voltage {
            self.fall
        } else {
            self.recovery
        };
        self.voltage += (target - self.voltage) * rate;
        self.voltage
    }
}
