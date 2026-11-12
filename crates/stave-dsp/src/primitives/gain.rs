//! Decibel ↔ amplitude.

#[inline]
pub fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Amplitudes at or below 1e-9 read as −180 dB, the floor of f32 audio.
#[inline]
pub fn amp_to_db(amp: f32) -> f32 {
    if amp <= 1e-9 {
        -180.0
    } else {
        20.0 * amp.log10()
    }
}

/// The gain that, applied once every `interval`, falls 60 dB over `t60`
/// (both in samples). A t60 of zero is an instant stop: gain 0.
#[inline]
pub fn t60_gain(interval: f32, t60: f32) -> f32 {
    10f32.powf(-3.0 * interval / t60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_floors() {
        for db in [-60.0f32, -6.0, 0.0, 12.0] {
            assert!((amp_to_db(db_to_amp(db)) - db).abs() < 1e-4);
        }
        assert_eq!(amp_to_db(0.0), -180.0);
    }

    #[test]
    fn t60_gain_falls_60_db_over_t60() {
        let g = t60_gain(100.0, 4_800.0);
        assert!((amp_to_db(g.powi(48)) + 60.0).abs() < 1e-3);
        assert_eq!(t60_gain(100.0, 0.0), 0.0);
    }
}
