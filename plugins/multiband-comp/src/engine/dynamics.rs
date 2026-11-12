//! The gain computer: reduction in dB for an envelope, with a
//! smoothstep soft knee.

use stave_dsp::amp_to_db;

/// How many dB the gain computer pulls for envelope `env` (linear).
#[inline]
pub(crate) fn band_reduction_db(env: f32, threshold_db: f32, ratio: f32, knee_db: f32) -> f32 {
    let env_db = amp_to_db(env);
    let over = env_db - threshold_db;
    // Soft knee: within ±knee/2 of threshold, smoothly transition
    // from linear (no compression) to full-ratio. smoothstep weight.
    if knee_db <= 0.0 {
        if over > 0.0 {
            over * (1.0 - 1.0 / ratio)
        } else {
            0.0
        }
    } else {
        let h = knee_db * 0.5;
        if over <= -h {
            0.0
        } else if over >= h {
            over * (1.0 - 1.0 / ratio)
        } else {
            // smoothstep on (over+h)/(2h).
            let u = (over + h) / (2.0 * h);
            let w = u * u * (3.0 - 2.0 * u);
            // Reduction at the upper edge of the knee is
            // h * (1 - 1/ratio); ramp from 0 to that.
            let full = over * (1.0 - 1.0 / ratio);
            w * full
        }
    }
}
