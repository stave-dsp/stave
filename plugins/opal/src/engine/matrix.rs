//! The MATRIX — opal's modulation semantics (the chime pattern with
//! per-op destinations). The route VALUE semantics
//! (shaping chain, via, OFF) are the kernel's [`RouteSlot`];
//! this file owns only opal's SOURCE TABLE and TARGET arms. The FM-musical
//! targets are the modulator LEVELS — a route into LEVEL 2 moves
//! BRIGHTNESS, which is what modulating FM means.

use stave_dsp::RouteSlot;

/// Declared displacement bounds.
pub(super) const PITCH_RANGE_ST: f32 = 12.0;
/// Levels and feedback displace across their full 0..1 range.
pub(super) const LEVEL_RANGE: f32 = 1.0;
pub(super) const FEEDBACK_RANGE: f32 = 1.0;
/// RATIO 2 bends ±1 semitone — inharmonicity, not transposition
/// (FM chorus lives in the CENTS; a whole tone is a new timbre).
pub(super) const RATIO2_RANGE_ST: f32 = 1.0;
/// LFO2 RATE stretches ±2 octaves (Absynth's cross-LFO motion).
pub(super) const LFO2_RANGE_OCT: f32 = 2.0;

/// One block's route table.
#[derive(Copy, Clone, Default)]
pub(super) struct Routes {
    pub slots: [RouteSlot; 4],
}

impl Routes {
    pub fn any(&self) -> bool {
        self.slots.iter().any(RouteSlot::active)
    }
}

/// One voice's source values in SOURCES enum order (OFF omitted),
/// every one normalized BIPOLAR ±1: LFO 1, VEL, KEY, ENV, SPREAD,
/// HEAT, SYMP, LFO 2.
pub(super) type SrcValues = [f32; 8];

/// The standard sources followed by LFO 2.
pub(super) fn with_lfo2(standard: [f32; 7], lfo2: f32) -> SrcValues {
    let mut sv = [0.0; 8];
    sv[..7].copy_from_slice(&standard);
    sv[7] = lfo2;
    sv
}

/// Per-voice displacement targets, one evaluation's output.
#[derive(Copy, Clone, Default)]
pub(super) struct Displacements {
    pub pitch_st: f32,
    /// Offsets for op 2/3/4 levels.
    pub levels: [f32; 3],
    pub feedback: f32,
    /// Op 1's level too (the Operator lesson: carrier volume IS a
    /// destination — tremolo belongs in the matrix).
    pub level1: f32,
    /// Op 2's ratio, in semitones of bend (inharmonicity).
    pub ratio2_st: f32,
    /// LFO 2's rate, in octaves — GLOBAL-scope target: the engine
    /// evaluates it once per block with ensemble source values.
    pub lfo2_oct: f32,
}

/// Evaluate for one voice.
pub(super) fn evaluate(routes: &Routes, sv: &SrcValues, lags: &mut [f32; 4]) -> Displacements {
    let mut d = Displacements::default();
    for (i, slot) in routes.slots.iter().enumerate() {
        let Some(v) = slot.value(sv, &mut lags[i]) else {
            continue;
        };
        match slot.tgt {
            0 => d.pitch_st += v * PITCH_RANGE_ST,
            1 => d.levels[0] += v * LEVEL_RANGE,
            2 => d.levels[1] += v * LEVEL_RANGE,
            3 => d.levels[2] += v * LEVEL_RANGE,
            4 => d.feedback += v * FEEDBACK_RANGE,
            5 => d.level1 += v * LEVEL_RANGE,
            6 => d.ratio2_st += v * RATIO2_RANGE_ST,
            _ => d.lfo2_oct += v * LFO2_RANGE_OCT,
        }
    }
    d
}
