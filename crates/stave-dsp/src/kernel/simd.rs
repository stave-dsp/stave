//! **Lanes** — the crate's one SIMD vocabulary (DSP.md P4): a value of
//! `N` scalars that every operation touches lane by lane.
//!
//! `std::simd` is not stable and the crate forbids `unsafe`, so there are
//! no intrinsics and no target-feature dispatch: a [`Lanes`] is a
//! fixed-size array, every operation a loop over its `N` lanes, and each
//! is `#[inline(always)]` so that, once a hot loop is monomorphised, LLVM
//! sees straight-line lane arithmetic and maps it onto the target's
//! vectors — NEON's 128-bit registers on Apple silicon, SSE2's (AVX's
//! with a wider target) on x86. The width is the caller's: eight `f32`
//! lanes are two NEON registers, eight `f64` lanes four.
//!
//! **What it promises about numbers.** Each lane computes exactly what
//! the same scalar expression computes: there is no fused multiply-add
//! (Rust never contracts `a * b + c`), no reordering inside a lane, and
//! lanes never mix except in [`Lanes::sum`], whose order is a fixed
//! pairwise tree. So a loop vectorised *across* independent outputs
//! (eight half-band outputs, eight convolution outputs, eight FFT
//! butterflies) is bit-identical to the scalar loop it replaces; only a
//! loop that splits *one* sum across lanes reassociates it, and then the
//! order is still fixed, so tick and block paths agree.
//!
//! The users: the half-band oversampler on every oversampled circuit's
//! hot path (`kernel::resample`), the FFT's radix-4 butterflies (and so
//! every partitioned convolution and FIR), the convolver's direct head,
//! and the M1 networks' convolutions (`learned::conv`). Where the
//! compiler already vectorises a plain loop well, the loop stays: the
//! convolver's frequency-domain multiply-accumulate streams one
//! partition at a time over flat split spectra, and written on lanes
//! (one or four partitions a pass) it measured 1.2–2.6× the instructions
//! on 16 k and 96 k taps.

use core::ops::{Add, AddAssign, Mul, Neg, Sub};

/// The scalars a [`Lanes`] holds: `f32` and `f64` in practice.
pub(crate) trait Scalar:
    Copy + Default + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self>
{
}

impl<T: Copy + Default + Add<Output = T> + Sub<Output = T> + Mul<Output = T>> Scalar for T {}

/// `N` scalars operated on lane by lane.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(transparent)]
pub(crate) struct Lanes<T, const N: usize>(pub(crate) [T; N]);

impl<T: Scalar, const N: usize> Default for Lanes<T, N> {
    #[inline(always)]
    fn default() -> Self {
        Self([T::default(); N])
    }
}

impl<T: Scalar, const N: usize> Lanes<T, N> {
    /// Every lane `x`.
    #[inline(always)]
    pub(crate) fn splat(x: T) -> Self {
        Self([x; N])
    }

    /// The first `N` scalars of `s` (panics if it is shorter: one bounds
    /// check for the whole load).
    #[inline(always)]
    pub(crate) fn load(s: &[T]) -> Self {
        let mut v = [T::default(); N];
        v.copy_from_slice(&s[..N]);
        Self(v)
    }

    /// The lanes into the first `N` scalars of `s`.
    #[inline(always)]
    pub(crate) fn store(self, s: &mut [T]) {
        s[..N].copy_from_slice(&self.0);
    }

    /// `self + a · b`, lane by lane: a multiply, then an add (never
    /// fused, so it is the scalar expression's value).
    #[inline(always)]
    pub(crate) fn mul_add(self, a: Self, b: Self) -> Self {
        let mut v = self.0;
        for l in 0..N {
            v[l] = v[l] + a.0[l] * b.0[l];
        }
        Self(v)
    }

    /// The lanes summed as a pairwise tree, `((l0 + l1) + (l2 + l3)) + …`
    /// for `N` a power of two: one fixed order, so every path that
    /// reduces the same lanes agrees bit for bit.
    #[inline(always)]
    pub(crate) fn sum(self) -> T {
        let mut v = self.0;
        let mut width = N;
        while width > 1 {
            let half = width / 2;
            for l in 0..half {
                v[l] = v[2 * l] + v[2 * l + 1];
            }
            if width % 2 == 1 {
                v[half] = v[width - 1];
                width = half + 1;
            } else {
                width = half;
            }
        }
        v[0]
    }
}

impl<const N: usize> Lanes<f64, N> {
    /// The first `N` scalars of `s`, widened to `f64` (exact).
    #[inline(always)]
    pub(crate) fn widen(s: &[f32]) -> Self {
        let s = &s[..N];
        let mut v = [0.0; N];
        for l in 0..N {
            v[l] = f64::from(s[l]);
        }
        Self(v)
    }

    /// The lanes rounded to `f32`.
    #[inline(always)]
    pub(crate) fn narrow(self) -> Lanes<f32, N> {
        let mut v = [0.0; N];
        for l in 0..N {
            v[l] = self.0[l] as f32;
        }
        Lanes(v)
    }
}

impl<T: Scalar, const N: usize> Add for Lanes<T, N> {
    type Output = Self;
    #[inline(always)]
    fn add(self, o: Self) -> Self {
        let mut v = self.0;
        for l in 0..N {
            v[l] = v[l] + o.0[l];
        }
        Self(v)
    }
}

impl<T: Scalar, const N: usize> AddAssign for Lanes<T, N> {
    #[inline(always)]
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl<T: Scalar, const N: usize> Sub for Lanes<T, N> {
    type Output = Self;
    #[inline(always)]
    fn sub(self, o: Self) -> Self {
        let mut v = self.0;
        for l in 0..N {
            v[l] = v[l] - o.0[l];
        }
        Self(v)
    }
}

impl<T: Scalar, const N: usize> Mul for Lanes<T, N> {
    type Output = Self;
    #[inline(always)]
    fn mul(self, o: Self) -> Self {
        let mut v = self.0;
        for l in 0..N {
            v[l] = v[l] * o.0[l];
        }
        Self(v)
    }
}

impl<T: Scalar + Neg<Output = T>, const N: usize> Neg for Lanes<T, N> {
    type Output = Self;
    #[inline(always)]
    fn neg(self) -> Self {
        let mut v = self.0;
        for l in 0..N {
            v[l] = -v[l];
        }
        Self(v)
    }
}

/// Every lane times one scalar.
impl<T: Scalar, const N: usize> Mul<T> for Lanes<T, N> {
    type Output = Self;
    #[inline(always)]
    fn mul(self, k: T) -> Self {
        let mut v = self.0;
        for l in 0..N {
            v[l] = v[l] * k;
        }
        Self(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_lane_is_the_scalar_expression() {
        let a = Lanes([1.5f32, -2.0, 3.25, 0.1, 7.0, -0.3, 1e-8, 4.0]);
        let b = Lanes([0.7f32, 0.2, -1.1, 3.3, 0.9, 2.5, 1e8, -0.25]);
        let c = Lanes::splat(0.3f32);
        let r = c.mul_add(a, b);
        for l in 0..8 {
            assert_eq!(r.0[l].to_bits(), (0.3f32 + a.0[l] * b.0[l]).to_bits());
            assert_eq!((a + b).0[l], a.0[l] + b.0[l]);
            assert_eq!((a - b).0[l], a.0[l] - b.0[l]);
            assert_eq!((a * b).0[l], a.0[l] * b.0[l]);
            assert_eq!((a * 2.5).0[l], a.0[l] * 2.5);
        }
        let w = Lanes::<f64, 8>::widen(&a.0);
        assert_eq!(w.narrow(), a);
    }

    #[test]
    fn the_sum_is_a_fixed_pairwise_tree() {
        let v = [1e8f32, 1.0, -1e8, 1.0, 3.0, 5.0, 7.0, 11.0];
        let tree = ((v[0] + v[1]) + (v[2] + v[3])) + ((v[4] + v[5]) + (v[6] + v[7]));
        assert_eq!(Lanes(v).sum().to_bits(), tree.to_bits());
        let odd = [1.0f64, 2.0, 3.0];
        assert_eq!(Lanes(odd).sum(), (1.0 + 2.0) + 3.0);
        assert_eq!(Lanes([4.0f64]).sum(), 4.0);
    }
}
