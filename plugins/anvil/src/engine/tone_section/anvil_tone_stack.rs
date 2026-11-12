//! Anvil's product-owned component values and control surface around the shared
//! passive-network mechanism.

use stave_dsp::{PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnvilToneStackComponents {
    pub treble_capacitance_farads: f64,
    pub bass_capacitance_farads: f64,
    pub midrange_capacitance_farads: f64,
    pub treble_potentiometer_ohms: f64,
    pub bass_potentiometer_ohms: f64,
    pub midrange_potentiometer_ohms: f64,
    pub slope_resistance_ohms: f64,
}

impl AnvilToneStackComponents {
    pub const fn modern_high_gain() -> Self {
        Self {
            treble_capacitance_farads: 470e-12,
            bass_capacitance_farads: 22e-9,
            midrange_capacitance_farads: 22e-9,
            treble_potentiometer_ohms: 250e3,
            bass_potentiometer_ohms: 1e6,
            midrange_potentiometer_ohms: 33e3,
            slope_resistance_ohms: 39e3,
        }
    }

    fn mechanism_components(self) -> PassiveToneStackComponents {
        PassiveToneStackComponents {
            treble_capacitance_farads: self.treble_capacitance_farads,
            bass_capacitance_farads: self.bass_capacitance_farads,
            midrange_capacitance_farads: self.midrange_capacitance_farads,
            treble_potentiometer_ohms: self.treble_potentiometer_ohms,
            bass_potentiometer_ohms: self.bass_potentiometer_ohms,
            midrange_potentiometer_ohms: self.midrange_potentiometer_ohms,
            slope_resistance_ohms: self.slope_resistance_ohms,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnvilToneControlPositions {
    pub treble: f64,
    pub bass: f64,
    pub midrange: f64,
}

impl AnvilToneControlPositions {
    pub const fn new(treble: f64, bass: f64, midrange: f64) -> Self {
        Self {
            treble,
            bass,
            midrange,
        }
    }

    fn mechanism_positions(self) -> PassiveToneControlPositions {
        PassiveToneControlPositions::new(self.treble, self.bass, self.midrange)
    }
}

pub struct AnvilToneStack {
    mechanism: PassiveToneStack,
}

impl AnvilToneStack {
    pub fn new(components: AnvilToneStackComponents) -> Self {
        Self {
            mechanism: PassiveToneStack::new(components.mechanism_components()),
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.mechanism.prepare(sample_rate);
    }

    pub fn set_controls(&mut self, controls: AnvilToneControlPositions) {
        self.mechanism.set_controls(controls.mechanism_positions());
    }

    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        self.mechanism.process(input)
    }

    pub fn magnitude_at(&self, frequency_hz: f64) -> f64 {
        self.mechanism.magnitude_at(frequency_hz)
    }
}

impl Default for AnvilToneStack {
    fn default() -> Self {
        Self::new(AnvilToneStackComponents::modern_high_gain())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_difference_decibels(high_magnitude: f64, low_magnitude: f64) -> f64 {
        20.0 * (high_magnitude / low_magnitude).log10()
    }

    #[test]
    fn default_is_the_anvil_modern_high_gain_design() {
        let components = AnvilToneStackComponents::modern_high_gain();
        assert_eq!(components.treble_capacitance_farads, 470e-12);
        assert_eq!(components.bass_capacitance_farads, 22e-9);
        assert_eq!(components.midrange_capacitance_farads, 22e-9);
        assert_eq!(components.treble_potentiometer_ohms, 250e3);
        assert_eq!(components.bass_potentiometer_ohms, 1e6);
        assert_eq!(components.midrange_potentiometer_ohms, 33e3);
        assert_eq!(components.slope_resistance_ohms, 39e3);
    }

    #[test]
    fn controls_interact_in_one_network() {
        let mut stack = AnvilToneStack::default();
        stack.set_controls(AnvilToneControlPositions::new(0.5, 0.5, 0.0));
        let treble_with_midrange_down = stack.magnitude_at(5_000.0);
        stack.set_controls(AnvilToneControlPositions::new(0.5, 0.5, 1.0));
        let treble_with_midrange_up = stack.magnitude_at(5_000.0);
        assert!((20.0 * (treble_with_midrange_up / treble_with_midrange_down).log10()).abs() > 0.5);
    }

    #[test]
    fn each_control_has_material_authority_in_its_intended_region() {
        let mut stack = AnvilToneStack::default();

        stack.set_controls(AnvilToneControlPositions::new(0.5, 0.0, 0.5));
        let bass_down = stack.magnitude_at(100.0);
        stack.set_controls(AnvilToneControlPositions::new(0.5, 1.0, 0.5));
        assert!(gain_difference_decibels(stack.magnitude_at(100.0), bass_down) > 6.0);

        stack.set_controls(AnvilToneControlPositions::new(0.5, 0.5, 0.0));
        let midrange_down = stack.magnitude_at(700.0);
        stack.set_controls(AnvilToneControlPositions::new(0.5, 0.5, 1.0));
        assert!(gain_difference_decibels(stack.magnitude_at(700.0), midrange_down) > 6.0);

        stack.set_controls(AnvilToneControlPositions::new(0.0, 0.5, 0.5));
        let treble_down = stack.magnitude_at(5_000.0);
        stack.set_controls(AnvilToneControlPositions::new(1.0, 0.5, 0.5));
        assert!(gain_difference_decibels(stack.magnitude_at(5_000.0), treble_down) > 6.0);
    }
}
