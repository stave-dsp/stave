//! The 4-operator FM core — a dense cross-coupled LEAF (the kernel
//! doctrine: cores like this are nodes, never forced into combinator
//! algebra), with the WIRING as data: each algorithm is a mod
//! bitmask table + a carrier mask, nothing else. Op 4 sits at the
//! top of every stack and owns the self-feedback, its loop memory
//! averaged over the last two samples (the DX7 trick — a raw
//! one-sample loop develops period-2 hunting at high β).
//!
//! Depth calibration: a full-level modulator swings its consumer's
//! phase by β = MOD_DEPTH radians. 2π reaches the classic bright-DX
//! register while keeping single-modulator spectra inside Carson
//! bandwidth at audible ratios; the sideband laws in tests/properties.rs
//! pin the calibration.

use stave_dsp::Adsr;

pub(super) const MOD_DEPTH: f32 = core::f32::consts::TAU;
pub(super) const FB_DEPTH: f32 = core::f32::consts::TAU;

/// One algorithm: who modulates whom (`mods[i]` = bitmask of ops
/// feeding op i's phase) and who reaches the output. Ops only ever
/// consume HIGHER-numbered ops, so one top-down pass per frame
/// resolves every route with no delay anywhere but op 4's declared
/// self-loop.
pub(super) struct Wiring {
    pub mods: [u8; 4],
    pub carriers: u8,
}

/// The eight algorithms (params::ALGOS is the label truth).
pub(super) const WIRINGS: [Wiring; 8] = [
    // CHAIN: 4→3→2→1
    Wiring {
        mods: [0b0010, 0b0100, 0b1000, 0],
        carriers: 0b0001,
    },
    // FORK: (3,4)→2→1
    Wiring {
        mods: [0b0010, 0b1100, 0b0000, 0],
        carriers: 0b0001,
    },
    // TWIN: (2, 3←4)→1
    Wiring {
        mods: [0b0110, 0b0000, 0b1000, 0],
        carriers: 0b0001,
    },
    // STACK2: 2→1, 4→3 — two independent pairs
    Wiring {
        mods: [0b0010, 0b0000, 0b1000, 0],
        carriers: 0b0101,
    },
    // TRIPLE: 4→(1,2,3)
    Wiring {
        mods: [0b1000, 0b1000, 0b1000, 0],
        carriers: 0b0111,
    },
    // BRIGHT: (2,3,4)→1
    Wiring {
        mods: [0b1110, 0b0000, 0b0000, 0],
        carriers: 0b0001,
    },
    // ORGAN2: 2→1 + ops 3,4 pure
    Wiring {
        mods: [0b0010, 0b0000, 0b0000, 0],
        carriers: 0b1101,
    },
    // ORGAN: additive, no modulation
    Wiring {
        mods: [0b0000, 0b0000, 0b0000, 0],
        carriers: 0b1111,
    },
];

/// The wiring tables as flat display data for the editor's
/// AlgoDiagram — DERIVED from WIRINGS at compile time, so the
/// diagram can never drift from the sound.
pub(crate) const DIAGRAM_MODS: [u8; 32] = {
    let mut m = [0u8; 32];
    let mut a = 0;
    while a < 8 {
        let mut i = 0;
        while i < 4 {
            m[a * 4 + i] = WIRINGS[a].mods[i];
            i += 1;
        }
        a += 1;
    }
    m
};
pub(crate) const DIAGRAM_CARRIERS: [u8; 8] = {
    let mut c = [0u8; 8];
    let mut a = 0;
    while a < 8 {
        c[a] = WIRINGS[a].carriers;
        a += 1;
    }
    c
};

/// The per-voice knob snapshot (copied at block rate by the engine's
/// apply pass — voices never read the table).
#[derive(Copy, Clone)]
pub(super) struct OpSettings {
    pub ratios: [f32; 4],
    pub levels: [f32; 4],
    pub algo: usize,
    pub feedback: f32,
    pub vel_mod: f32,
}

impl OpSettings {
    pub fn new() -> Self {
        Self {
            ratios: [1.0; 4],
            levels: [1.0, 0.5, 0.0, 0.0],
            algo: 0,
            feedback: 0.0,
            vel_mod: 0.5,
        }
    }
}

/// The oscillator half of a voice: four phase accumulators + the
/// feedback memory. Envelopes live beside it in the voice (per-voice
/// control instances — the tier-2 shape the mini-FM law proved).
pub(super) struct FmCore {
    phase: [f64; 4],
    fb1: f32,
    fb2: f32,
}

impl FmCore {
    pub fn new() -> Self {
        Self {
            phase: [0.0; 4],
            fb1: 0.0,
            fb2: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.phase = [0.0; 4];
        self.fb1 = 0.0;
        self.fb2 = 0.0;
    }

    /// One frame. `envs` tick inside (per-op envelopes are part of
    /// the operator, not decoration); `vel_carrier` scales what you
    /// hear, `vel_index` scales modulator depth (velocity →
    /// brightness).
    #[inline]
    pub fn tick(
        &mut self,
        set: &OpSettings,
        envs: &mut [Adsr; 4],
        inc: [f64; 4],
        vel_carrier: f32,
        vel_index: f32,
    ) -> f32 {
        let w = &WIRINGS[set.algo.min(WIRINGS.len() - 1)];
        let mut out = [0.0f32; 4];
        let mut sum = 0.0f32;
        let mut n_carriers = 0.0f32;
        // Top-down: every mod route reads a HIGHER op, already
        // computed this frame.
        for i in (0..4).rev() {
            let mut pm = 0.0f32;
            let m = w.mods[i];
            for (j, o) in out.iter().enumerate().skip(i + 1) {
                if m & (1 << j) != 0 {
                    pm += o;
                }
            }
            if i == 3 {
                pm += set.feedback * 0.5 * (self.fb1 + self.fb2);
            }
            let env = envs[i].tick();
            let s = core::f64::consts::TAU * self.phase[i] + f64::from(MOD_DEPTH * pm);
            let y = s.sin() as f32 * set.levels[i] * env;
            let is_carrier = w.carriers & (1 << i) != 0;
            out[i] = if is_carrier { y } else { y * vel_index };
            if is_carrier {
                sum += y * vel_carrier;
                n_carriers += 1.0;
            }
            self.phase[i] += inc[i];
            if self.phase[i] >= 1.0 {
                self.phase[i] -= 1.0;
            }
        }
        self.fb2 = self.fb1;
        self.fb1 = out[3] * FB_DEPTH / MOD_DEPTH;
        sum / n_carriers.max(1.0)
    }
}
