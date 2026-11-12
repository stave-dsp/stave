//! The MATRIX — chime's modulation semantics (routes are params):
//! the route table
//! snapshotted from the params, the DECLARED displacement ranges
//! (the laws measure these exact numbers), and the per-voice
//! evaluation. The route VALUE semantics (shaping chain, via,
//! OFF) are the kernel's [`RouteSlot`]; this file owns only chime's SOURCE
//! TABLE and TARGET arms. Application (the ~5 ms glides) lives
//! with the voice.

use stave_dsp::RouteSlot;

/// Modulation displacement bounds — the DECLARED target semantics.
pub(super) const PITCH_RANGE_ST: f32 = 12.0;
pub(super) const LEVEL_RANGE_DB: f32 = 12.0;
/// Decay stretches by 2^(±DECAY_RANGE_OCT).
pub(super) const DECAY_RANGE_OCT: f32 = 2.0;

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
/// every one normalized BIPOLAR ±1: LFO, VEL, KEY, ENV, SPREAD,
/// HEAT, SYMP.
pub(super) type SrcValues = [f32; 7];

/// Evaluate the table for one voice. Returns the displacement
/// targets (pitch st, level dB, decay octaves).
pub(super) fn evaluate(routes: &Routes, sv: &SrcValues, lags: &mut [f32; 4]) -> (f32, f32, f32) {
    let (mut pitch, mut level_db, mut decay_oct) = (0.0f32, 0.0f32, 0.0f32);
    for (i, slot) in routes.slots.iter().enumerate() {
        let Some(v) = slot.value(sv, &mut lags[i]) else {
            continue;
        };
        match slot.tgt {
            0 => pitch += v * PITCH_RANGE_ST,
            1 => level_db += v * LEVEL_RANGE_DB,
            _ => decay_oct += v * DECAY_RANGE_OCT,
        }
    }
    (pitch, level_db, decay_oct)
}
