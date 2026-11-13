//! The modal bank laid out for SIMD: Mathews–Smith phasor resonators in
//! groups of [`LANES`], run mode-major over blocks so each group's
//! coefficients and state stay in registers.

/// Modes per group: one AVX register of `f32`, two NEON ones.
pub(super) const LANES: usize = 8;
/// Samples between denormal flushes of the state, and the longest run.
pub(super) const BLOCK: usize = 64;

type V = [f32; LANES];

/// Per group: the rotation `r·e^(jωT)`, two pickups' output weights on
/// the state's real and imaginary parts, and the state.
#[derive(Clone, Debug, Default)]
pub(super) struct Lanes {
    rot_re: Vec<V>,
    rot_im: Vec<V>,
    /// Left real, left imaginary, right real, right imaginary.
    weights: [Vec<V>; 4],
    z_re: Vec<V>,
    z_im: Vec<V>,
    /// Samples run since the last flush.
    since_flush: usize,
}

impl Lanes {
    /// Room for `modes` modes, all silent (allocates).
    pub(super) fn allocate(&mut self, modes: usize) {
        let groups = modes.div_ceil(LANES);
        let zero = || vec![[0.0f32; LANES]; groups];
        self.rot_re = zero();
        self.rot_im = zero();
        self.weights = [zero(), zero(), zero(), zero()];
        self.z_re = zero();
        self.z_im = zero();
        self.since_flush = 0;
    }

    /// Mode `i`'s rotation (`r cos ωT`, `r sin ωT`).
    pub(super) fn set_rotation(&mut self, i: usize, re: f32, im: f32) {
        let (g, l) = (i / LANES, i % LANES);
        self.rot_re[g][l] = re;
        self.rot_im[g][l] = im;
    }

    /// Mode `i`'s output weights on `Re z` and `Im z`, left and right.
    pub(super) fn set_weights(&mut self, i: usize, w: [f32; 4]) {
        let (g, l) = (i / LANES, i % LANES);
        for (k, v) in w.into_iter().enumerate() {
            self.weights[k][g][l] = v;
        }
    }

    pub(super) fn reset(&mut self) {
        self.z_re.fill([0.0; LANES]);
        self.z_im.fill([0.0; LANES]);
        self.since_flush = 0;
    }

    /// How many samples the next run may take before a flush.
    pub(super) fn room(&self) -> usize {
        BLOCK - self.since_flush
    }

    /// Run `input` (at most [`room`](Self::room) samples) through every
    /// mode: `z ← rot·z + x`, each pickup `Σ w_re Re z + w_im Im z`,
    /// written to `left` (and `right` when `STEREO`) unscaled. One
    /// sample at a time and a block at a time give the same bits: every
    /// lane's operations run in the same order either way.
    #[inline]
    pub(super) fn run<const STEREO: bool>(&mut self, input: &[f32], left: &mut [f32], right: &mut [f32]) {
        let n = input.len();
        debug_assert!(n <= self.room() && left.len() >= n && (!STEREO || right.len() >= n));
        let mut acc_l = [[0.0f32; LANES]; BLOCK];
        let mut acc_r = [[0.0f32; LANES]; BLOCK];
        let groups = self.z_re.len();
        for g in 0..groups {
            let (cr, ci) = (self.rot_re[g], self.rot_im[g]);
            let (wlr, wli) = (self.weights[0][g], self.weights[1][g]);
            let (wrr, wri) = (self.weights[2][g], self.weights[3][g]);
            let (mut zr, mut zi) = (self.z_re[g], self.z_im[g]);
            for (k, &x) in input.iter().enumerate() {
                let (al, ar) = (&mut acc_l[k], &mut acc_r[k]);
                for l in 0..LANES {
                    let re = cr[l] * zr[l] - ci[l] * zi[l] + x;
                    let im = ci[l] * zr[l] + cr[l] * zi[l];
                    zr[l] = re;
                    zi[l] = im;
                    al[l] += wlr[l] * re + wli[l] * im;
                    if STEREO {
                        ar[l] += wrr[l] * re + wri[l] * im;
                    }
                }
            }
            self.z_re[g] = zr;
            self.z_im[g] = zi;
        }
        let reduce = |a: &V| ((a[0] + a[1]) + (a[2] + a[3])) + ((a[4] + a[5]) + (a[6] + a[7]));
        for k in 0..n {
            left[k] = reduce(&acc_l[k]);
            if STEREO {
                right[k] = reduce(&acc_r[k]);
            }
        }
        self.since_flush += n;
        if self.since_flush >= BLOCK {
            self.flush();
        }
    }

    /// Flush tiny and non-finite state to zero, so a silent plate's
    /// decaying modes never reach subnormals (and one bad sample cannot
    /// poison the state).
    fn flush(&mut self) {
        for v in self.z_re.iter_mut().chain(self.z_im.iter_mut()) {
            for s in v.iter_mut() {
                *s = crate::kernel::denormal::flush(*s);
            }
        }
        self.since_flush = 0;
    }
}
