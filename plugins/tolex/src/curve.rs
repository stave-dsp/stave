//! The response curve — ONE truth for the picture and the sound.
//! Assembles the full small-signal magnitude (motor mesh × rear
//! path × voice) from the engine's own display twins: the same
//! derivations, the same files, and a law (tests/curve_properties.rs) that
//! renders audio and pins the picture against it.
//!
//! The population envelope is AGGREGATE STATISTICS over the
//! reference corpus (plugins/tolex/docs/corpus.md; 500-IR sample, 1/6-oct
//! trend re the 1 kHz band) — the territory real cabinets occupy,
//! baked as constants. No vendor content ships.

use crate::engine::{cabinet, drivers, interior, microphone, motor, voice};

/// Display evaluations run at a fixed reference rate — the response
/// below 20 kHz is rate-independent for every element drawn.
pub const DISPLAY_RATE: f32 = 48_000.0;

pub(crate) const ENV_N: usize = 64;
pub(crate) const ENV_F0_HZ: f32 = 50.0;
pub(crate) const ENV_F1_HZ: f32 = 15_000.0;
pub(crate) const ENV_P10: [f32; 64] = [
    -15.9, -13.2, -11.0, -8.9, -6.4, -4.4, -1.8, -1.5, -0.8, -0.8, -0.6, 0.9, -0.1, -1.1, -1.0,
    0.5, 1.5, 1.7, 1.8, 2.0, 1.1, 0.4, 0.0, -0.6, -0.6, -1.6, -3.3, -5.3, -0.2, -1.3, -0.7, -1.4,
    -1.9, -1.7, -2.3, -2.0, -3.0, -5.5, -5.7, -4.0, -3.4, -2.7, -2.5, -3.4, -4.9, -5.0, -2.7, -3.0,
    -3.7, -3.8, -6.0, -8.5, -12.3, -18.0, -24.2, -30.1, -30.3, -29.7, -31.0, -32.4, -35.2, -40.0,
    -44.6, -47.7,
];
pub(crate) const ENV_P90: [f32; 64] = [
    -3.8, -1.0, 0.3, 2.3, 2.9, 4.4, 6.1, 5.6, 6.6, 7.7, 9.1, 9.3, 8.8, 8.4, 8.2, 7.6, 7.2, 6.6,
    5.9, 5.8, 5.0, 4.6, 3.9, 3.5, 3.3, 2.6, 1.7, 1.5, 5.6, 5.1, 3.6, 2.4, 1.6, 0.5, 1.7, 3.4, 2.0,
    0.0, 1.6, 1.7, 2.0, 3.8, 5.6, 4.7, 3.7, 2.7, 4.9, 4.0, 3.6, 4.5, 4.1, 2.6, 0.6, -4.3, -9.7,
    -12.5, -13.6, -15.5, -17.0, -19.4, -23.6, -24.0, -25.3, -30.1,
];

/// The build sheet as the display sees it (plain param values).
#[derive(Clone, Copy)]
pub struct Sheet {
    pub size: usize,
    pub variant: usize,
    pub resonance: f32,
    pub q: f32,
    pub damping: f32,
    pub open: bool,
    pub cab_l: f32,
    pub material: usize,
    pub model: usize,
    pub layout: usize,
    pub mass: f32,
    pub breakup: f32,
    pub doping: f32,
    pub position: f32,
    pub mic_cm: f32,
}

/// The full response in dB at each `hz`, AT AN OPERATING POINT:
/// `excursion` is the motor's live envelope (0 = resting). Bl
/// droops with it (the compression the curve sags by) and the
/// voice softens — the same laws the audio runs. Normalized re the
/// 1 kHz band at rest, so the sag SHOWS instead of renormalizing
/// away.
pub fn response_into(sheet: &Sheet, excursion: f32, hz: &[f32], out: &mut [f32]) {
    let rsrc = motor::rsrc_ohms(sheet.damping);
    let vb = if sheet.open {
        None
    } else {
        Some(sheet.cab_l * 1e-3)
    };
    let mut consts =
        motor::derive_consts(sheet.size, sheet.resonance, sheet.q, rsrc, vb, sheet.mass);
    // The operating point, BOTH large-signal laws (motor.rs): Bl
    // droops (grip loss — alone it would UN-damp the resonance and
    // bloom it, which the breathing law caught) and the suspension
    // hardens, detuning the alignment upward. The box spring does
    // not harden. cal keeps the RESTING normalization so the sag
    // reads as level.
    let env = f64::from(excursion.clamp(0.0, 3.0));
    consts.bl /= 1.0 + env;
    let k_m = consts.k - consts.k_box;
    consts.k = k_m * (1.0 + motor::HARDEN_PER_ENV * env) + consts.k_box;
    let mic_m = (sheet.mic_cm * 0.01).max(0.005);
    for (i, f) in hz.iter().enumerate() {
        out[i] = motor::linear_response_db(&consts, *f)
            + cabinet::response_db(sheet.open, sheet.cab_l, mic_m, *f, DISPLAY_RATE)
            + interior::response_db(
                sheet.cab_l,
                sheet.material,
                sheet.open,
                sheet.layout,
                *f,
                DISPLAY_RATE,
            )
            + microphone::response_db(sheet.model, mic_m, *f, DISPLAY_RATE)
            + drivers::response_db(sheet.layout, mic_m, *f);
    }
    voice::response_add(
        sheet.size,
        sheet.variant,
        sheet.mass,
        sheet.breakup,
        sheet.doping,
        sheet.position,
        excursion,
        DISPLAY_RATE,
        hz,
        out,
    );
    // Normalize re the 1 kHz band (800-1250), the envelope's own
    // convention.
    let (mut acc, mut n) = (0.0f32, 0u32);
    for (i, f) in hz.iter().enumerate() {
        if (800.0..=1_250.0).contains(f) {
            acc += out[i];
            n += 1;
        }
    }
    if n > 0 {
        let ref_db = acc / n as f32;
        for o in out.iter_mut() {
            *o -= ref_db;
        }
    }
}

/// Envelope value (dB re 1 kHz) at `hz` by log interpolation, for
/// each of the two percentile rails.
pub(crate) fn envelope_at(hz: f32, hi: bool) -> f32 {
    let rail = if hi { &ENV_P90 } else { &ENV_P10 };
    let t = (hz / ENV_F0_HZ).ln() / (ENV_F1_HZ / ENV_F0_HZ).ln() * (ENV_N - 1) as f32;
    let t = t.clamp(0.0, (ENV_N - 1) as f32);
    let i = (t as usize).min(ENV_N - 2);
    let frac = t - i as f32;
    rail[i] + (rail[i + 1] - rail[i]) * frac
}
