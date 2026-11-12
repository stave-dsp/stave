//! Wave digital filters (Fettweis): every element is a port with a
//! resistance R, exchanging voltage waves a (arriving) and b (leaving),
//! so v = (a + b) / 2 and i = (a − b) / (2R). An adaptor joins ports
//! into one adapted port that reflects without waiting on its parent, so
//! a tree computes in one pass up and one pass down per sample.

/// One port of a tree, seen from its parent.
pub trait OnePort {
    fn resistance(&self) -> f64;
    /// The wave this port sends to its parent.
    fn reflected(&mut self) -> f64;
    /// The wave its parent sends back.
    fn incident(&mut self, wave: f64);
}

pub struct Resistor {
    pub ohms: f64,
}

impl OnePort for Resistor {
    fn resistance(&self) -> f64 {
        self.ohms
    }
    fn reflected(&mut self) -> f64 {
        0.0
    }
    fn incident(&mut self, _: f64) {}
}

/// An ideal voltage source behind a series resistance.
pub struct ResistiveSource {
    pub ohms: f64,
    pub volts: f64,
}

impl OnePort for ResistiveSource {
    fn resistance(&self) -> f64 {
        self.ohms
    }
    fn reflected(&mut self) -> f64 {
        self.volts
    }
    fn incident(&mut self, _: f64) {}
}

/// Discretized by the bilinear transform: R = T / 2C, `b[n] = a[n−1]`.
pub struct Capacitor {
    farads: f64,
    sample_rate: f64,
    ohms: f64,
    state: f64,
}

impl Capacitor {
    pub fn new(farads: f64) -> Self {
        Self {
            farads,
            sample_rate: 1.0,
            ohms: 1.0 / (2.0 * farads),
            state: 0.0,
        }
    }

    /// Re-derive the port resistance for `sample_rate`; clears state.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = f64::from(sample_rate);
        self.set_farads(self.farads);
        self.reset();
    }

    pub const fn farads(&self) -> f64 {
        self.farads
    }

    /// A value change keeps the stored wave: no allocation, no reset.
    pub fn set_farads(&mut self, farads: f64) {
        self.farads = farads;
        self.ohms = 1.0 / (2.0 * farads * self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.state = 0.0;
    }
}

impl OnePort for Capacitor {
    fn resistance(&self) -> f64 {
        self.ohms
    }
    fn reflected(&mut self) -> f64 {
        self.state
    }
    fn incident(&mut self, wave: f64) {
        self.state = wave;
    }
}

/// Two ports sharing one voltage.
pub struct Parallel<A, B> {
    pub left: A,
    pub right: B,
    left_wave: f64,
    right_wave: f64,
    up_wave: f64,
}

impl<A: OnePort, B: OnePort> Parallel<A, B> {
    pub fn new(left: A, right: B) -> Self {
        Self {
            left,
            right,
            left_wave: 0.0,
            right_wave: 0.0,
            up_wave: 0.0,
        }
    }

    fn left_share(&self) -> f64 {
        let (gl, gr) = (1.0 / self.left.resistance(), 1.0 / self.right.resistance());
        gl / (gl + gr)
    }
}

impl<A: OnePort, B: OnePort> OnePort for Parallel<A, B> {
    fn resistance(&self) -> f64 {
        let (rl, rr) = (self.left.resistance(), self.right.resistance());
        rl * rr / (rl + rr)
    }
    fn reflected(&mut self) -> f64 {
        self.left_wave = self.left.reflected();
        self.right_wave = self.right.reflected();
        let share = self.left_share();
        self.up_wave = share * self.left_wave + (1.0 - share) * self.right_wave;
        self.up_wave
    }
    fn incident(&mut self, wave: f64) {
        self.left.incident(wave + self.up_wave - self.left_wave);
        self.right.incident(wave + self.up_wave - self.right_wave);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source into a resistor, terminated by an open circuit (b = a):
    /// the divider has no load, so the open end reads the source.
    #[test]
    fn an_unloaded_source_reads_its_voltage() {
        let mut tree = Parallel::new(
            ResistiveSource {
                ohms: 1e3,
                volts: 2.0,
            },
            Resistor { ohms: 1e12 },
        );
        let a = tree.reflected();
        tree.incident(a);
        assert!((a - 2.0).abs() < 1e-6);
    }

    /// RC low-pass to a step: the bilinear capacitor tracks the exact
    /// 1 − exp(−t/RC) closely when RC spans many samples.
    #[test]
    fn rc_step_follows_its_time_constant() {
        let (rate, r, c) = (192_000.0f32, 1e3, 100e-9);
        let mut cap = Capacitor::new(c);
        cap.prepare(rate);
        let mut tree = Parallel::new(
            ResistiveSource {
                ohms: r,
                volts: 1.0,
            },
            cap,
        );
        let mut worst: f64 = 0.0;
        for n in 1..=960 {
            let a = tree.reflected();
            // Open-circuit root: the adapted port sees no load.
            tree.incident(a);
            let v = a;
            let t = n as f64 / f64::from(rate);
            worst = worst.max((v - (1.0 - (-(t - 0.5 / f64::from(rate)) / (r * c)).exp())).abs());
        }
        assert!(worst < 2e-3, "{worst}");
    }
}
