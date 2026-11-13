//! SplitMix64: a small seeded generator, the same sequence on every
//! machine. Trial randomisation logs its seed, so a session can be
//! replayed exactly; the built-in material uses fixed seeds.

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// A seed from the clock and the process (sessions differ; the seed
    /// is logged, so any one can be replayed).
    pub fn seed_from_clock() -> u64 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        let mut r = Rng(nanos ^ (u64::from(std::process::id()) << 32));
        r.next_u64()
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [−1, 1).
    pub fn bipolar(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }

    pub fn coin(&mut self) -> bool {
        self.next_u64() >> 63 == 1
    }

    /// Uniform in 0..n (n > 0), without modulo bias.
    pub fn below(&mut self, n: usize) -> usize {
        let n = n as u64;
        let zone = u64::MAX - u64::MAX % n;
        loop {
            let v = self.next_u64();
            if v < zone {
                return (v % n) as usize;
            }
        }
    }

    /// A Fisher–Yates shuffle.
    pub fn shuffle<T>(&mut self, xs: &mut [T]) {
        for i in (1..xs.len()).rev() {
            xs.swap(i, self.below(i + 1));
        }
    }
}
