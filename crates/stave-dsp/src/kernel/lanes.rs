//! Channel structure as TYPES.
//!
//! `PerChannel<T, N>` is the mono→N lift: one lane of DSP state per
//! channel, constructed per-lane, iterated per-lane. Engines that
//! kept parallel `[T; 2]` arrays for every stateful field could cross
//! indices (`chain[0]` fed through `os[1]`); grouping ALL of a lane's
//! state into one `T` makes that hazard unwritable — a lane is
//! borrowed whole, so its parts can never pair with another lane's.
//!
//! `ChannelLayout` names what the lanes MEAN. Semantic, not a count:
//! Stereo and MidSide are both two lanes, and confusing them is a
//! real bug the type prevents. Adapters between layouts are explicit
//! (never implicit up/down-mixes).

use crate::kernel::node::{Stage, Tail};

/// What the lanes of a [`PerChannel`] (or a buffer) denote.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    /// Sum/difference domain — lane 0 = mid, lane 1 = side.
    MidSide,
}

impl ChannelLayout {
    pub const fn lanes(self) -> usize {
        match self {
            ChannelLayout::Mono => 1,
            ChannelLayout::Stereo | ChannelLayout::MidSide => 2,
        }
    }
}

/// Stereo → MidSide, the explicit adapter. The M/S lanes are
/// **f64 by contract**, and the exactness invariant is DOMAIN-QUALIFIED
/// (measured honestly, like the taper and cubic-delay limits): when
/// the two lanes sit within 28 octaves (≈168 dB) of each other —
/// wider than any audio program — `l+r` and `l−r` are exact in f64,
/// the rotation loses NOTHING, and `ms_decode(ms_encode(l, r)) ==
/// (l, r)` bit-for-bit; the goldens pin it. A zero lane is always
/// exact. Beyond the span the quieter lane absorbs at the f64 noise
/// floor (>168 dB below the louder, which still round-trips
/// exactly) — no finite-precision rotation can carry an unbounded
/// exponent gap. Sole in-span exception: a NEGATIVE zero returns
/// positive (IEEE `x−x = +0`), value-identical. An f32 M/S pair
/// cannot make the exactness promise even in-span (the rotated pair
/// re-rounds), so any narrowing is the caller's explicit choice,
/// never the adapter's. Mid is the pinned stereo→mono downmix
/// (coefficients 0.5/0.5): one concept, not two.
#[inline]
pub fn ms_encode(l: f32, r: f32) -> (f64, f64) {
    let (l, r) = (l as f64, r as f64);
    (0.5 * (l + r), 0.5 * (l - r))
}

/// MidSide → Stereo. Exact inverse of [`ms_encode`] (see its
/// contract); results are f32-representable whenever the M/S pair
/// came from an f32 stereo pair.
#[inline]
pub fn ms_decode(m: f64, s: f64) -> (f32, f32) {
    ((m + s) as f32, (m - s) as f32)
}

/// 5.1 → Stereo, the explicit downmix adapter (lane order
/// L R C LFE Ls Rs — `Arrangement::Surround51`). Coefficients are
/// ITU-R BS.775, PINNED by the goldens: centre and surrounds fold in
/// at −3 dB (1/√2 exactly, as f64), LFE is omitted (the bass-manage
/// decision belongs to playback, not a fold-down). f64 interior like
/// the M/S pair; narrowing is the caller's explicit choice.
#[inline]
pub fn downmix_51(l: f32, r: f32, c: f32, _lfe: f32, ls: f32, rs: f32) -> (f64, f64) {
    const G: f64 = core::f64::consts::FRAC_1_SQRT_2;
    let (l, r, c, ls, rs) = (l as f64, r as f64, c as f64, ls as f64, rs as f64);
    (l + G * c + G * ls, r + G * c + G * rs)
}

/// One `T` of DSP state per channel lane. `N` is monomorphized —
/// no allocation, no dynamic lane count.
#[derive(Clone, Debug, PartialEq)]
pub struct PerChannel<T, const N: usize = 2> {
    lanes: [T; N],
}

impl<T, const N: usize> PerChannel<T, N> {
    /// The lift: construct each lane from its index. State that must
    /// differ per lane (seeds, phases) takes it from `lane`.
    pub fn new(make: impl FnMut(usize) -> T) -> Self {
        Self {
            lanes: core::array::from_fn(make),
        }
    }

    pub const fn lanes(&self) -> usize {
        N
    }

    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.lanes.iter()
    }

    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, T> {
        self.lanes.iter_mut()
    }

    /// Apply `f` to every lane (prepare/reset cascades).
    pub fn each_mut(&mut self, mut f: impl FnMut(&mut T)) {
        for lane in &mut self.lanes {
            f(lane);
        }
    }
}

/// A lane of [`Stage`]s: the whole bank prepares, resets and reports as
/// one (lanes are parallel, so latency and tail are the longest lane's).
impl<T: Stage, const N: usize> PerChannel<T, N> {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.each_mut(|lane| lane.prepare(sample_rate));
    }

    pub fn reset(&mut self) {
        self.each_mut(Stage::reset);
    }

    pub fn latency(&self) -> u32 {
        self.iter().map(Stage::latency).max().unwrap_or(0)
    }

    pub fn tail(&self) -> Tail {
        self.iter().fold(Tail::None, |t, lane| t.max(lane.tail()))
    }
}

impl<T> PerChannel<T, 2> {
    /// Both lanes at once, distinctly borrowed — the shape for
    /// cross-lane work (M/S encode, width) that still cannot alias.
    pub fn pair_mut(&mut self) -> (&mut T, &mut T) {
        let [a, b] = &mut self.lanes;
        (a, b)
    }
}

impl<T, const N: usize> core::ops::Index<usize> for PerChannel<T, N> {
    type Output = T;
    fn index(&self, lane: usize) -> &T {
        &self.lanes[lane]
    }
}

impl<T, const N: usize> core::ops::IndexMut<usize> for PerChannel<T, N> {
    fn index_mut(&mut self, lane: usize) -> &mut T {
        &mut self.lanes[lane]
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a mut PerChannel<T, N> {
    type Item = &'a mut T;
    type IntoIter = core::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.lanes.iter_mut()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a PerChannel<T, N> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.lanes.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lift_constructs_per_lane() {
        let pc: PerChannel<u64, 4> = PerChannel::new(|lane| 10 + lane as u64);
        assert_eq!(pc.lanes(), 4);
        assert_eq!(pc.iter().copied().collect::<Vec<_>>(), vec![10, 11, 12, 13]);
    }

    #[test]
    fn pairwise_borrows_are_distinct() {
        let mut pc: PerChannel<i32, 2> = PerChannel::new(|l| l as i32);
        let (m, s) = pc.pair_mut();
        core::mem::swap(m, s);
        assert_eq!((pc[0], pc[1]), (1, 0));
    }

    #[test]
    fn iter_mut_reaches_every_lane() {
        let mut pc: PerChannel<f32, 2> = PerChannel::new(|_| 0.0);
        pc.each_mut(|x| *x += 1.0);
        for lane in &pc {
            assert_eq!(*lane, 1.0);
        }
    }

    #[test]
    fn layout_is_semantic_not_a_count() {
        assert_eq!(
            ChannelLayout::Stereo.lanes(),
            ChannelLayout::MidSide.lanes()
        );
        assert_ne!(ChannelLayout::Stereo, ChannelLayout::MidSide);
        assert_eq!(ChannelLayout::Mono.lanes(), 1);
    }
}
