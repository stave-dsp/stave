//! Circuits speak volts; plugins speak full scale. `AudioAdapter`
//! drives a circuit with `full_scale_in_volts` per unit of audio and
//! reads its output's departure from the quiescent point in units of
//! `full_scale_out_volts`. Subtracting the exact quiescent point removes
//! the circuit's DC without filtering its lows.
use crate::kernel::node::{Stage, Tail};

/// A circuit stage that knows where its output quiescents with no input
/// (re-exported at the crate root as `Circuit` until K3 prunes the root:
/// in the circuit layer that name is the netlist runtime's).
pub trait Quiescent: Stage {
    fn quiescent_output_volts(&self) -> f64;
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AudioAdapterConfig {
    pub full_scale_in_volts: f64,
    pub full_scale_out_volts: f64,
}

#[derive(Clone)]
pub struct AudioAdapter<C> {
    pub circuit: C,
    config: AudioAdapterConfig,
    quiescent: f64,
}

impl<C: Quiescent> AudioAdapter<C> {
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

impl<C: Quiescent> Stage for AudioAdapter<C> {
    fn prepare(&mut self, sample_rate: f32) {
        self.circuit.prepare(sample_rate);
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

    fn latency(&self) -> u32 {
        self.circuit.latency()
    }

    fn tail(&self) -> Tail {
        self.circuit.tail()
    }
}
