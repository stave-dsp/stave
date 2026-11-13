//! **Layer 5 — physical.** Models derived from mechanics or acoustics
//! rather than a schematic: springs, plates, rotors, tape transport,
//! loudspeakers, rooms and instruments.
//!
//! *Claim:* the model solves the stated physics (a dispersive spring,
//! a thin plate's modes, a rotor's Doppler) to a stated accuracy.
//!
//! *Verified by:* published modes and measurements, high-precision
//! solutions of the same equations, and hardware captures, each with an
//! error budget.
//!
//! | Module | Models |
//! |---|---|
//! | [`bar`] | [`Bar`](bar::Bar) (a struck uniform bar, free or clamped: Euler–Bernoulli modes as Mathews–Smith phasors) |
//! | [`cabinet`] | [`Captured`](cabinet::Captured) (a cabinet from captured impulse responses: microphones placed between their captures by Kistler & Wightman's shape-plus-delay split, mixed as captured or time-aligned), [`CloseMic`](cabinet::CloseMic) (a first-order microphone on a baffled piston: exact on-axis pressure and particle velocity, so the proximity effect) |
//! | [`membrane`] | [`Membrane`](membrane::Membrane) (a struck circular drumhead: its Bessel-zero modes as Mathews–Smith phasors) |
//! | [`plate`] | [`Plate`](plate::Plate) (the modes of a thin, tensioned plate: Ducceschi & Gabrielli's DAFx-26 model as Mathews–Smith phasors), [`Geometry::EMT140`](plate::Geometry::EMT140) |
//! | [`rotary`] | [`Cabinet`](rotary::Cabinet) (a rotary speaker: horn and drum [`Radiator`](rotary::Radiator)s with Smith et al.'s Doppler, directivity and cabinet reflections), rotors [`Rotor::HORN_122H`](rotary::Rotor::HORN_122H), [`Rotor::DRUM_122H`](rotary::Rotor::DRUM_122H) |
//! | [`speaker`] | [`Driver`](speaker::Driver) (a moving-coil loudspeaker: Thiele–Small with an LR-2 coil and Klippel's large-signal `Bl(x)`, `Kms(x)`, `Le(x)`), parts [`ThieleSmall::JENSEN_C12N`](speaker::ThieleSmall::JENSEN_C12N) and three more, each with its fitted `_LR2` |
//! | [`spring`] | [`Tank`](spring::Tank), [`Spring`](spring::Spring) (Välimäki, Parker & Abel's dispersive chirps as waveguides), [`Chirp`](spring::Chirp) |
//! | [`string`] | [`Waveguide`](string::Waveguide) (a plucked, struck or bowed stiff string: Smith's digital waveguide with dispersion, loss and a friction-curve [`Bow`](string::Bow)) |
//! | [`tape`] | [`Chowdhury`](tape::Chowdhury) (a tape machine: [`Recording`](tape::Recording), [`Flutter`](tape::Flutter), [`Playback`](tape::Playback)); machine [`Machine::TC260`](tape::Machine::TC260), transport [`Transport::AKAI_4000D`](tape::Transport::AKAI_4000D) |
//! | [`tube`] | [`Waveguide`](tube::Waveguide) (a cylindrical or conical bore blown through a reed or the lips: a waveguide with the open end's radiation and a quasi-static [`Valve`](tube::Valve)) |
//!
//! It may use `units`, `kernel`, `primitives`, `behavioral` and
//! `circuit`. A bucket-brigade delay is not here: the chip is a
//! schematic part, so it lives in
//! [`circuit::devices::bbd`](crate::circuit::devices::bbd) though its
//! transfer is a sampling process.

pub mod bar;
pub mod cabinet;
mod lanes;
pub mod membrane;
mod modal;
pub mod plate;
#[cfg(test)]
mod probe;
pub mod rotary;
pub mod speaker;
pub mod spring;
pub mod string;
pub mod tape;
pub mod tube;
