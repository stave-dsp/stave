//! Coppola's parametric orthogonal feedback matrices: Kronecker products
//! of 2×2 rotations and reflections, one angle each.

/// Lines the matrix mixes (`2³`).
const LINES: usize = 8;
/// Kernels: one per factor of two.
const LEVELS: usize = 3;

/// A 2×2 orthogonal kernel's form.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Kernel {
    /// `[[cos θ, −sin θ], [sin θ, cos θ]]`: identity at 0, an equal mix
    /// at π/4.
    Rotation,
    /// `[[cos θ, sin θ], [sin θ, −cos θ]]`: an axis flip at 0, `H₂` at
    /// π/4.
    #[default]
    Reflection,
}

/// An orthogonal 8×8 feedback matrix `K₃ ⊗ K₂ ⊗ K₁`, each 2×2 kernel a
/// rotation or reflection by its own angle: orthogonal at every angle,
/// so an [`Fdn`](super::Fdn) mixing through it is lossless whatever the
/// angles, and the angles can move continuously, at audio rate.
///
/// # Models
/// Coppola's fast parametric matrices (DAFx-26), after Das, Canfield-
/// Dafilou & Abel's Kronecker-parameterised mixing. Kernel 1 is the
/// finest level (it pairs neighbouring lines), kernel 3 the coarsest (it
/// couples the two halves); an angle of 0 decouples its level, so `θ₃ =
/// 0` splits the network into lines 0–3 and 4–7, `θ₁ = 0` into the even
/// and odd lines. Every kernel a reflection at π/4 is the Hadamard
/// matrix `H₈/√8`.
///
/// # Derivation
/// The in-place iterative Kronecker transform (Coppola's Algorithm 2):
/// level `c` applies kernel `c` to every pair `(j, j + 2^(c−1))`,
/// `O(N log N)`, the shape of the fast Walsh–Hadamard butterfly.
///
/// # Verified
/// `crates/stave-dsp/src/effects/reverb/tests.rs::kronecker_matrices_are_orthogonal_at_every_angle`
/// (`AᵀA = I` within 1e-6 over a grid of angles and both kernels),
/// `crates/stave-dsp/src/effects/reverb/tests.rs::kronecker_at_quarter_pi_is_hadamard`,
/// and in the network, `crates/stave-dsp/src/effects/reverb/tests.rs::freeze_preserves_energy`.
///
/// # Cost
/// Part of the `"Fdn (8 lines, Kronecker, modulated)"` datasheet line.
///
/// # Aliasing
/// None (a linear map).
///
/// # Valid ranges
/// Any finite angles, in radians.
///
/// ```
/// use stave_dsp::effects::reverb::{Kernel, Kronecker};
///
/// let mut m = Kronecker::hadamard();
/// m.set_angle(2, 0.0); // decouple the two halves
/// let mut v = [1.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
/// m.mix(&mut v);
/// assert!(v[4..].iter().all(|&x| x.abs() < 1e-7)); // nothing crosses
/// assert_eq!(m.kernel(0), Kernel::Reflection);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Kronecker {
    kernels: [Kernel; LEVELS],
    angles: [f32; LEVELS],
    /// `(cos θ, sin θ)` per level.
    trig: [(f32, f32); LEVELS],
}

impl Default for Kronecker {
    fn default() -> Self {
        Self::hadamard()
    }
}

impl Kronecker {
    /// Kernels (levels) in the matrix.
    pub const LEVELS: usize = LEVELS;

    /// Every kernel a reflection at π/4: `H₈/√8`.
    pub fn hadamard() -> Self {
        Self::new(
            [Kernel::Reflection; LEVELS],
            [core::f32::consts::FRAC_PI_4; LEVELS],
        )
    }

    /// The matrix of these kernels at these angles (radians), finest
    /// level first.
    pub fn new(kernels: [Kernel; LEVELS], angles: [f32; LEVELS]) -> Self {
        let mut m = Self {
            kernels,
            angles,
            trig: [(1.0, 0.0); LEVELS],
        };
        for level in 0..LEVELS {
            m.set_angle(level, angles[level]);
        }
        m
    }

    /// Level `level`'s angle, radians (`level` from 0, the finest).
    pub fn set_angle(&mut self, level: usize, angle: f32) {
        self.angles[level] = angle;
        self.trig[level] = (angle.cos(), angle.sin());
    }

    /// Level `level`'s angle, radians.
    pub fn angle(&self, level: usize) -> f32 {
        self.angles[level]
    }

    /// Level `level`'s kernel form.
    pub fn kernel(&self, level: usize) -> Kernel {
        self.kernels[level]
    }

    /// Multiply `v` by the matrix in place.
    #[inline]
    pub fn mix(&self, v: &mut [f32; LINES]) {
        let mut h = 1;
        for level in 0..LEVELS {
            let (c, s) = self.trig[level];
            // Rotation [[c, −s], [s, c]]; reflection [[c, s], [s, −c]].
            let (k12, k22) = match self.kernels[level] {
                Kernel::Rotation => (-s, c),
                Kernel::Reflection => (s, -c),
            };
            for base in (0..LINES).step_by(2 * h) {
                for j in base..base + h {
                    let (a, b) = (v[j], v[j + h]);
                    v[j] = c * a + k12 * b;
                    v[j + h] = s * a + k22 * b;
                }
            }
            h *= 2;
        }
    }
}
