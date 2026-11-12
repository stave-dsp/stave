//! Fixed integer-delay storage the kernel forms own: the Branch pad and
//! the Loop history both need "hold a sample for exactly N ticks" —
//! nothing fractional, nothing musical. The fractional, modulatable line
//! is [`DelayLine`](crate::DelayLine); this is deliberately smaller so
//! the kernel depends on nothing below it.

pub(crate) struct Ring {
    buf: Vec<f32>,
    idx: usize,
}

impl Ring {
    pub(crate) const fn new() -> Self {
        Self {
            buf: Vec::new(),
            idx: 0,
        }
    }

    /// Size for exactly `len` samples of delay (allocates — message
    /// thread). Flushes on every call.
    pub(crate) fn allocate(&mut self, len: usize) {
        self.buf.clear();
        self.buf.resize(len, 0.0);
        self.idx = 0;
    }

    pub(crate) fn reset(&mut self) {
        self.buf.fill(0.0);
        self.idx = 0;
    }

    /// The sample written `len` ticks ago (the oldest). Callers that
    /// need read-before-write (Loop) peek, compute, then `push`.
    /// Before `allocate` (empty ring) reads `0.0` — never a panic.
    #[inline]
    pub(crate) fn delayed(&self) -> f32 {
        self.buf.get(self.idx).copied().unwrap_or(0.0)
    }

    /// Store `x`; a no-op before `allocate` (empty ring).
    #[inline]
    pub(crate) fn push(&mut self, x: f32) {
        if let Some(slot) = self.buf.get_mut(self.idx) {
            *slot = x;
            self.idx += 1;
            if self.idx == self.buf.len() {
                self.idx = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_by_exactly_len() {
        let mut r = Ring::new();
        r.allocate(3);
        for i in 0..16i32 {
            let y = r.delayed();
            r.push(i as f32);
            let want = if i < 3 { 0.0 } else { (i - 3) as f32 };
            assert_eq!(y, want, "tick {i}");
        }
    }

    #[test]
    fn unprepared_ring_reads_zero_and_ignores_pushes() {
        let mut r = Ring::new();
        for i in 0..4 {
            assert_eq!(r.delayed(), 0.0);
            r.push(i as f32 + 1.0);
        }
        r.reset();
        assert_eq!(r.delayed(), 0.0);
    }
}

/// Kani: for every length up to four and every tick count, the ring
/// delays by exactly its length, and an unallocated ring reads zero.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(11)]
    fn delays_by_exactly_its_length() {
        let len: usize = kani::any_where(|l: &usize| *l <= 4);
        let mut r = Ring::new();
        if len > 0 {
            r.allocate(len);
        }
        let mut seen = [0.0f32; 10];
        for i in 0..10 {
            let x: f32 = kani::any();
            let y = r.delayed();
            if len == 0 || i < len {
                assert_eq!(y.to_bits(), 0.0f32.to_bits());
            } else {
                assert_eq!(y.to_bits(), seen[i - len].to_bits());
            }
            r.push(x);
            seen[i] = x;
        }
    }
}
