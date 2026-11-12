//! Deterministic randomness: a seeded stream and stateless hashes.
//! Engines stay reproducible from a seed or a key, never from a clock.

/// Marsaglia's xorshift32 (13, 17, 5): period 2³² − 1 over nonzero
/// states.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Xorshift32 {
    state: u32,
}

impl Xorshift32 {
    /// Zero is the generator's fixed point, so a zero seed takes the
    /// golden-ratio constant instead.
    pub const fn new(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 0x9E37_79B9 } else { seed },
        }
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniform in [−1, 1), 24-bit resolution.
    #[inline]
    pub fn next_bipolar(&mut self) -> f32 {
        bipolar_f32(self.next_u32())
    }

    /// Uniform in [0, 1), 24-bit resolution.
    #[inline]
    pub fn next_unit(&mut self) -> f32 {
        unit_f32(self.next_u32())
    }
}

/// Thomas Wang's 32-bit integer hash.
#[inline]
pub const fn wang_hash(mut x: u32) -> u32 {
    x = (x ^ 61) ^ (x >> 16);
    x = x.wrapping_mul(9);
    x ^= x >> 4;
    x = x.wrapping_mul(0x27d4_eb2d);
    x ^ (x >> 15)
}

/// The splitmix64 output mix (Steele, Lea & Flood 2014).
#[inline]
pub const fn splitmix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The top 24 bits as a uniform value in [0, 1).
#[inline]
pub fn unit_f32(x: u32) -> f32 {
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The top 24 bits as a uniform value in [−1, 1).
#[inline]
pub fn bipolar_f32(x: u32) -> f32 {
    (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift32_matches_marsaglia() {
        // Marsaglia (2003), seed 2463534242: first output 723471715.
        let mut r = Xorshift32::new(2_463_534_242);
        assert_eq!(r.next_u32(), 723_471_715);
    }

    #[test]
    fn zero_seed_does_not_absorb() {
        let mut r = Xorshift32::new(0);
        assert_ne!(r.next_u32(), 0);
    }

    #[test]
    fn float_maps_cover_their_ranges() {
        assert_eq!(unit_f32(0), 0.0);
        assert!(unit_f32(u32::MAX) < 1.0);
        assert_eq!(bipolar_f32(0), -1.0);
        assert!(bipolar_f32(u32::MAX) < 1.0);
    }

    #[test]
    fn splitmix64_matches_reference() {
        // splitmix64 with state 0: first output is mix(0x9E3779B97F4A7C15).
        assert_eq!(splitmix64(0x9E37_79B9_7F4A_7C15), 0xE220_A839_7B1D_CDAF);
    }
}
