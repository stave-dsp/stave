//! Pitch helpers: the equal-temperament map from MIDI keys to hertz.

/// Equal-temperament pitch for a MIDI key, A4 (key 69) = 440 Hz. A
/// helper, not policy — bridges that tune differently pass their own hz.
#[inline]
pub fn hz_of_key(key: i16) -> f32 {
    440.0 * ((f32::from(key) - 69.0) / 12.0).exp2()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_440_and_octaves_double() {
        assert_eq!(hz_of_key(69), 440.0);
        assert_eq!(hz_of_key(81), 880.0);
        assert_eq!(hz_of_key(57), 220.0);
    }
}
