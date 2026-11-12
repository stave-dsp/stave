//! Circuits speak volts; plugins speak full scale. `AudioAdapter`
//! drives a circuit with `full_scale_in_volts` per unit of audio and
//! reads its output's departure from the quiescent point in units of
//! `full_scale_out_volts`. Subtracting the exact quiescent point removes
//! the circuit's DC without filtering its lows.
use crate::kernel::node::Stage;

/// A circuit that knows where its output quiescents with no input.
pub trait Circuit: Stage {
    fn quiescent_output_volts(&self) -> f64;
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AudioAdapterConfig {
    pub full_scale_in_volts: f64,
    pub full_scale_out_volts: f64,
}

pub struct AudioAdapter<C> {
    pub circuit: C,
    config: AudioAdapterConfig,
    quiescent: f64,
}

impl<C: Circuit> AudioAdapter<C> {
    pub fn new(circuit: C, config: AudioAdapterConfig) -> Self {
        let quiescent = circuit.quiescent_output_volts();
        Self {
            circuit,
            config,
            quiescent,
        }
    }

    /// Call after changing the circuit's component values.
    pub fn configure(&mut self, config: AudioAdapterConfig) {
        self.config = config;
        self.quiescent = self.circuit.quiescent_output_volts();
    }
}

impl<C: Circuit> Stage for AudioAdapter<C> {
    fn prepare(&mut self, rate: f32) {
        self.circuit.prepare(rate);
        self.quiescent = self.circuit.quiescent_output_volts();
    }

    fn reset(&mut self) {
        self.circuit.reset();
    }

    fn tick(&mut self, x: f32) -> f32 {
        let volts = self
            .circuit
            .tick((f64::from(x) * self.config.full_scale_in_volts) as f32);
        ((f64::from(volts) - self.quiescent) / self.config.full_scale_out_volts) as f32
    }

    fn latency_samples(&self) -> u32 {
        self.circuit.latency_samples()
    }

    fn tail_samples(&self) -> Option<u32> {
        self.circuit.tail_samples()
    }
}
