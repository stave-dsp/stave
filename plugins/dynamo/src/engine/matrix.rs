//! The MATRIX — dynamo's modulation semantics (routes-as-params +
//! MOTION rows). The route VALUE semantics are the kernel's
//! [`RouteSlot`]; this file owns dynamo's SOURCE TABLE and TARGET
//! arms. Voice targets (pitch/level/decay) evaluate per voice;
//! MACRO targets (force/mass/memory/couple/steam/chaos) are
//! GLOBAL-scope — the engine evaluates them once per block with
//! ensemble source values (one motion animates the machine, not
//! sixteen private copies).

use stave_dsp::RouteSlot;

/// Modulation displacement bounds — the DECLARED target semantics.
pub(super) const PITCH_RANGE_ST: f32 = 12.0;
pub(super) const LEVEL_RANGE_DB: f32 = 12.0;
pub(super) const DECAY_RANGE_OCT: f32 = 2.0;
/// MASS displaces ±2 octaves (multiplicative); the unit macros
/// (force/memory/couple/steam/chaos) displace across their 0..1.
pub(super) const MASS_RANGE_OCT: f32 = 2.0;

pub(super) const SLOTS: usize = 10;

/// One block's route table (4 free slots + 6 motion rows).
#[derive(Copy, Clone, Default)]
pub(super) struct Routes {
    pub slots: [RouteSlot; SLOTS],
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

/// Everything one evaluation displaces.
#[derive(Copy, Clone, Default)]
pub(super) struct Disp {
    pub pitch_st: f32,
    pub level_db: f32,
    pub decay_oct: f32,
    pub force: f32,
    pub mass_oct: f32,
    pub memory: f32,
    pub couple: f32,
    pub steam: f32,
    pub chaos: f32,
}

/// Evaluate the table for one scope (a voice, or the engine's
/// global macro pass — same math, different source values and lag
/// stores).
pub(super) fn evaluate(routes: &Routes, sv: &SrcValues, lags: &mut [f32; SLOTS]) -> Disp {
    let mut d = Disp::default();
    for (i, slot) in routes.slots.iter().enumerate() {
        let Some(v) = slot.value(sv, &mut lags[i]) else {
            continue;
        };
        match slot.tgt {
            0 => d.pitch_st += v * PITCH_RANGE_ST,
            1 => d.level_db += v * LEVEL_RANGE_DB,
            2 => d.decay_oct += v * DECAY_RANGE_OCT,
            3 => d.force += v,
            4 => d.mass_oct += v * MASS_RANGE_OCT,
            5 => d.memory += v,
            6 => d.couple += v,
            7 => d.steam += v,
            _ => d.chaos += v,
        }
    }
    d
}
