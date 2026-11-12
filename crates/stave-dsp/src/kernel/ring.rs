//! Fixed integer-delay storage the kernel forms own: the Branch pad and the Loop history both need "hold a sample
//! for exactly N ticks" — nothing fractional, nothing musical. The
//! fractional, modulatable line lives in `primitives/time/delay.rs`; this
//! is deliberately smaller so the kernel depends on nothing below it.

extern crate alloc;
use alloc::vec::Vec;

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
    pub(crate) fn prepare(&mut self, len: usize) {
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
    #[inline]
    pub(crate) fn delayed(&self) -> f32 {
        self.buf[self.idx]
    }

    #[inline]
    pub(crate) fn push(&mut self, x: f32) {
        self.buf[self.idx] = x;
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_by_exactly_len() {
        let mut r = Ring::new();
        r.prepare(3);
        for i in 0..16i32 {
            let y = r.delayed();
            r.push(i as f32);
            let want = if i < 3 { 0.0 } else { (i - 3) as f32 };
            assert_eq!(y, want, "tick {i}");
        }
    }
}
