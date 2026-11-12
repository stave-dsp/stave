//! THE WIRES — the body with memory. Each voice
//! owns a detuned PAIR of wires: waveguide strings (delay line +
//! damping one-pole + stiffness allpass, all shipped cells) driven
//! by the rotor's commutation. The waveguide beat a modal
//! bank: one delay beats six SVFs per wire,
//! and the rotor SPINNING UP THROUGH the wire's partials — comb
//! notches sweeping into lock — is the attack character a static
//! filter bank cannot make.
//!
//! MEMORY is the wire's own physics, measured into shape twice:
//! the forge tape head could not serve — its AC coupling DRAINS
//! remanence in silence by design, and IN the loop its ~0.4×
//! small-signal gain ate the ring in five round trips (−100 dB in
//! 40 ms, measured). So the magnetized wire is: (1) EDDY BRAKING —
//! loud playing magnetizes the wire (a slow store, ~4 s
//! demagnetization τ that SURVIVES silence) and a magnetized wire
//! rings SHORTER (eddy-current damping — the real reason you don't
//! put magnets on strings); (2) magnetization-driven SOFT
//! SATURATION in the loop (unity small-signal gain — the ring is
//! safe; loud passages compress and darken). History audibly
//! shapes the present across silence. STIFFNESS is a 4-stage
//! allpass cascade — piano-string dispersion stretching upper
//! partials sharp.
//!
//! The wire keeps RINGING after the envelope closes (a struck body,
//! not a VCA) — the voice stays alive while any wire holds energy.

use stave_dsp::{t60_gain, DelayLine};

/// Detune between the pair, in cents (SPREAD-adjacent chorus).
pub(super) const PAIR_CENTS: f32 = 4.0;
/// Loop damping (HF loss per trip): fixed gentle brightness decay.
const DAMP: f32 = 0.25;
/// Ring-energy gate for `alive()` (~ −80 dBFS).
const ALIVE_FLOOR: f32 = 1e-4;

pub(super) struct Wire {
    delay: DelayLine,
    damp_z: f32,
    /// Loop DC blocker (~20 Hz): a lowpass-damped loop is RESONANT
    /// AT 0 Hz — a unipolar strike left a decaying displacement
    /// hump that never crossed zero (measured: the escapement's
    /// crossing detector starved; the 'ring' was partly a DC sag).
    /// A real string cannot hold displacement — the bridge
    /// terminates it.
    dc_x1: f32,
    dc_y1: f32,
    ap_z: [f32; 4],
    len: f32,
    g: f32,
    stiff_a: f32,
    memory: f32,
    ring_env: f32,
    /// Remanent magnetization (the memory store): CHARGES slowly
    /// (τ ≈ 0.8 s — a note barely magnetizes itself; measured the
    /// 10 ms mistake braking its own body 15×), demagnetizes over
    /// ~4 s, and SURVIVES silence.
    mag: f32,
    mag_rise: f32,
    demag: f32,
}

impl Wire {
    pub fn new() -> Self {
        Self {
            delay: DelayLine::new(),
            damp_z: 0.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
            ap_z: [0.0; 4],
            len: 100.0,
            g: 0.9,
            stiff_a: 0.0,
            memory: 0.0,
            ring_env: 0.0,
            mag: 0.0,
            mag_rise: 0.0,
            demag: 1.0,
        }
    }

    /// Allocate for the lowest playable note (20 Hz). Prepare-time
    /// only — the audio path never allocates (the no-alloc check).
    pub fn prepare(&mut self, sr: f32) {
        self.delay.prepare((sr / 20.0) as usize + 8);
        self.mag_rise = 1.0 - (-1.0 / (0.8 * sr)).exp();
        self.demag = (-1.0 / (4.0 * sr)).exp();
        self.reset();
    }

    pub fn reset(&mut self) {
        self.delay.reset();
        self.damp_z = 0.0;
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        self.ap_z = [0.0; 4];
        self.ring_env = 0.0;
        self.mag = 0.0;
    }

    /// Retune to a note. `detune_cents` splits the pair; `ring_s`
    /// is the dialed T60; `stiff` 0..1 stretches the partials.
    pub fn tune(&mut self, hz: f32, detune_cents: f32, ring_s: f32, stiff: f32, sr: f32) {
        let hz = (hz * (detune_cents / 1_200.0).exp2()).clamp(20.0, sr * 0.45);
        let period = sr / hz;
        // Dispersion needs a CASCADE with the pole NEAR +1: one
        // stage at a=0.54 moved the partial ratio 0.0000 (measured),
        // and backing the pole off for short strings killed it again
        // (0.0000 twice) — the differential group delay lives in
        // (1−a)². Four stages at a→0.97 give piano-grade stretch;
        // short strings clamp the COMPENSATION instead (a very stiff
        // short wire tunes bar-like — that is what stiffness means).
        self.stiff_a = 0.97 * stiff.clamp(0.0, 1.0);
        // Loop-filter compensation: the damping one-pole is worth
        // ~DAMP samples of group delay; each allpass its DC group
        // delay (1+a)/(1−a) — under-compensating tunes the whole
        // wire flat (measured: −1 Hz on partial 2).
        let ap_delay = 4.0 * (1.0 + self.stiff_a) / (1.0 - self.stiff_a).max(0.05);
        self.len = (period - 1.0 - DAMP - ap_delay.min(period * 0.5)).max(2.0);
        // Per-trip loss for a T60 of `ring_s` seconds.
        self.g = t60_gain(period, ring_s * sr);
    }

    pub fn set_memory(&mut self, memory: f32) {
        self.memory = memory.clamp(0.0, 1.0);
    }

    /// Per-trip termination loss (1−g) — the bridge's admittance:
    /// energy leaves a string through its bridge in proportion to
    /// what the loop loses per pass, which makes the sympathetic
    /// transfer's LOOP GAIN independent of the dialed ring (a flat
    /// coupling constant regenerated at long rings — measured as a
    /// drone that outlived its chord).
    pub fn loss(&self) -> f32 {
        1.0 - self.g
    }

    /// Ringing beyond the envelope? (the struck-body contract)
    pub fn alive(&self) -> bool {
        self.ring_env > ALIVE_FLOOR
    }

    /// The wire's current energy (the diagram's glow).
    pub fn energy(&self) -> f32 {
        self.ring_env
    }

    /// Remanent magnetization (the crackle's aging term).
    pub fn magnetization(&self) -> f32 {
        self.mag
    }

    /// One sample: read the string, lose, disperse, magnetize,
    /// re-inject the exciter, advance.
    #[inline]
    pub fn tick(&mut self, exciter: f32) -> f32 {
        let r = self.delay.read(self.len);
        // Damping one-pole: the string loses brightness per trip.
        self.damp_z += (1.0 - DAMP) * (r - self.damp_z);
        // The bridge terminates displacement: DC out of the loop.
        let dc = self.damp_z - self.dc_x1 + 0.9974 * self.dc_y1;
        self.dc_x1 = self.damp_z;
        self.dc_y1 = dc;
        // The stiffness cascade: dispersion, partials stretch sharp.
        let mut x = dc;
        for z in self.ap_z.iter_mut() {
            let ap = -self.stiff_a * x + *z;
            *z = x + self.stiff_a * ap;
            x = ap;
        }
        // EDDY BRAKING: remanence damps the ring. brake is exactly
        // 1.0 at memory 0 (bit-transparent — the law).
        let brake = 1.0 - 0.35 * self.memory * self.mag;
        let looped = x * self.g * brake;
        // Magnetized soft saturation: unity gain at small signal
        // (the ring is SAFE — the head that wasn't, measured), and
        // |sat(x)| ≤ |x| keeps the loop passive.
        let fed = if self.memory < 1e-3 {
            looped
        } else {
            let d = 1.0 + 2.0 * self.memory * self.mag;
            let x = (looped * d).clamp(-1.5, 1.5);
            (x - x * x * x / 6.75) / d
        };
        // Magnetize on loud signal; demagnetize slowly (τ ≈ 4 s) —
        // this survives silence, unlike the head's AC-coupled loop.
        let amp = fed.abs().min(1.0);
        self.mag = if amp > self.mag {
            self.mag + self.mag_rise * (amp - self.mag)
        } else {
            self.mag * self.demag
        };
        self.delay.write(exciter + fed);
        self.delay.advance();
        self.ring_env = (self.ring_env * 0.99995).max(fed.abs());
        // The wire's VOICE is its resonance. (Returning fed +
        // exciter double-counted the drive against the dry path
        // and let the escapement's push edges bleed straight to
        // the output — measured as broadband buzz swamping the
        // sustained mode.)
        fed
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    /// The waveguide is PASSIVE: struck once, it only ever decays
    /// (energy never grows — the loop gain contract).
    #[test]
    fn the_wire_is_passive() {
        let sr = 48_000.0;
        let mut w = Wire::new();
        w.prepare(sr);
        w.tune(220.0, 0.0, 2.0, 0.5, sr);
        w.set_memory(1.0);
        // Strike: one period of drive, then silence.
        for i in 0..220 {
            w.tick(if i < 110 { 0.8 } else { 0.0 });
        }
        let early: f32 = (0..4_800).map(|_| w.tick(0.0).abs()).fold(0.0, f32::max);
        let late: f32 = (0..4_800).map(|_| w.tick(0.0).abs()).fold(0.0, f32::max);
        assert!(late <= early * 1.05, "energy grew: {early} → {late}");
    }

    /// At memory 0 the loop is BIT-transparent to the head: two
    /// wires, one with the head forced deep then dialed to zero,
    /// ring identically.
    #[test]
    fn memory_zero_is_a_clean_wire() {
        let sr = 48_000.0;
        let run = |pre_dial: f32| {
            let mut w = Wire::new();
            w.prepare(sr);
            w.tune(220.0, 0.0, 1.0, 0.2, sr);
            w.set_memory(pre_dial);
            w.set_memory(0.0);
            let mut out = [0.0f32; 2_000];
            for (i, o) in out.iter_mut().enumerate() {
                *o = w.tick(if i < 100 { 0.5 } else { 0.0 });
            }
            out
        };
        let a = run(0.0);
        let b = run(1.0);
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            assert!(x.to_bits() == y.to_bits(), "sample {i}: {x} vs {y}");
        }
    }
}
