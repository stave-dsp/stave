//! THE ROTOR — the oscillator with character.
//! Rotation rate IS pitch, and rate changes have MASS: notes spin
//! UP (torque-limited one-pole toward the commanded speed) and
//! freewheel DOWN slower. The body's energy LOADS the machine —
//! sag flattens the pitch under load, the forge supply physics as
//! pitch behavior. Timbre is the COMMUTATION: pole-count scallops
//! amplitude-modulate the fundamental (harmonics without leaving
//! the pitch), and the ENCODER WHEEL — a 16-slot pattern on the
//! shaft — gates the cycle, sweeping analog-pure to PCM-gnarly
//! inside one metaphor. Hard wheel edges alias ON PURPOSE: that IS
//! the digital domain of the machine's voice.

/// Declared sag depth: full load at sag=1 flattens this much.
pub(super) const SAG_RANGE_ST: f32 = 3.0;

/// Pole-count table (the POLES enum's steps).
pub(super) const POLE_COUNTS: [f32; 7] = [1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0];

/// Encoder wheel patterns (the WHEELS enum's steps): 16 slots per
/// revolution, MSB first. SOLID never gates — the analog machine.
pub(super) const WHEEL_BITS: [u16; 8] = [
    0xFFFF, // SOLID
    0xFF00, // HALF
    0xF0F0, // QUARTERS
    0xAAAA, // ALTERNATE
    0xEEE0, // RATCHET
    0x8420, // SPARSE
    0xEA6A, // MORSE
    0x9B3D, // PCM
];

/// The commutated waveform, pure (the laws sweep it): fundamental
/// sine AM'd by the pole scallops, gated by the wheel. `phase` in
/// revolutions (0..1).
#[inline]
pub(super) fn wave(phase: f32, poles: f32, wheel: u16) -> f32 {
    let slot = ((phase * 16.0) as usize).min(15);
    if wheel & (1 << (15 - slot)) == 0 {
        return 0.0;
    }
    let fund = (core::f32::consts::TAU * phase).sin();
    let scallop = (core::f32::consts::PI * phase * poles).sin().abs();
    fund * (0.3 + 0.7 * scallop)
}

/// The rotating mass. Rate persists across notes on the same voice
/// — the machine keeps spinning; reset() is the power switch.
#[derive(Clone, Copy)]
pub(super) struct Rotor {
    /// Current rotation rate, Hz.
    rate: f32,
    /// Commanded rate (note pitch × matrix displacement × sag).
    target: f32,
    phase: f32,
    /// Per-sample one-pole coefficients: torque-limited up,
    /// freewheel down (3× slower — flywheels coast).
    k_up: f32,
    k_down: f32,
    poles: f32,
    wheel: u16,
}

impl Rotor {
    pub fn new() -> Self {
        Self {
            rate: 0.0,
            target: 0.0,
            phase: 0.0,
            k_up: 1.0,
            k_down: 1.0,
            poles: 4.0,
            wheel: 0xFFFF,
        }
    }

    /// Block-rate voicing: the MASS knob is the spin-up time.
    pub fn set(&mut self, mass_ms: f32, poles: f32, wheel: u16, sr: f32) {
        let tau = (mass_ms.max(0.1)) * 0.001 * sr;
        self.k_up = 1.0 - (-1.0 / tau).exp();
        self.k_down = 1.0 - (-1.0 / (3.0 * tau)).exp();
        self.poles = poles;
        self.wheel = wheel;
    }

    pub fn command(&mut self, hz: f32) {
        self.target = hz;
    }

    /// Power off: the shaft stops dead (reproducibility's reset).
    pub fn reset(&mut self) {
        self.rate = 0.0;
        self.target = 0.0;
        self.phase = 0.0;
    }

    /// Spin fraction (current rate / commanded), the diagram's
    /// spin-up needle. 1.0 when idle (nothing commanded).
    pub fn spin(&self) -> f32 {
        if self.target > 1e-9 {
            (self.rate / self.target).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// Shaft angle in revolutions (the diagram's strobe spoke).
    pub fn shaft(&self) -> f32 {
        self.phase
    }

    /// Rotation rate in revolutions/sample (the bearing's drive).
    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// Brush-transition gate: near the commutation scallop's zeros
    /// — where physical brushes cross segments — this opens (the
    /// LIFE system's hiss bursts land where the machine says).
    pub fn brush_gate(&self) -> f32 {
        let scallop = (core::f32::consts::PI * self.phase * self.poles)
            .sin()
            .abs();
        let g = 1.0 - scallop;
        g * g
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        let k = if self.target > self.rate {
            self.k_up
        } else {
            self.k_down
        };
        self.rate += k * (self.target - self.rate);
        self.phase += self.rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        wave(self.phase, self.poles, self.wheel)
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    /// SOLID is the analog machine: the wheel at 0xFFFF is
    /// BIT-IDENTICAL to the ungated waveform.
    #[test]
    fn the_solid_wheel_never_gates() {
        for i in 0..1_000 {
            let phase = i as f32 / 1_000.0;
            let gated = wave(phase, 4.0, 0xFFFF);
            let fund = (core::f32::consts::TAU * phase).sin();
            let scallop = (core::f32::consts::PI * phase * 4.0).sin().abs();
            assert_eq!(gated.to_bits(), (fund * (0.3 + 0.7 * scallop)).to_bits());
        }
    }

    /// Every non-SOLID wheel gates SOMETHING: the pattern is
    /// audible by construction.
    #[test]
    fn every_other_wheel_bites() {
        for &bits in &WHEEL_BITS[1..] {
            let gated = (0..1_000)
                .filter(|&i| wave(i as f32 / 1_000.0 + 0.0005, 4.0, bits) == 0.0)
                .count();
            assert!(
                gated > 50,
                "wheel {bits:#06x} gated only {gated}/1000 samples"
            );
        }
    }
}
