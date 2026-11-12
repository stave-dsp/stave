//! Unit newtypes: a value whose meaning is in its type, so a mix-up is a
//! compile error rather than a wrong sound. Zero cost (`repr(transparent)`).

/// A parameter position, 0–1 in the parameter's own taper: the language
/// of hosts and faces. Built clamped, so it is always in range (NaN
/// becomes 0).
///
/// ```
/// use stave_core::Normalized;
///
/// assert_eq!(Normalized::new(1.5).get(), 1.0);
/// assert_eq!(Normalized::new(f64::NAN), Normalized::ZERO);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Normalized(f64);

impl Normalized {
    pub const ZERO: Normalized = Normalized(0.0);
    pub const ONE: Normalized = Normalized(1.0);

    /// `value` clamped into 0–1; NaN is 0.
    pub fn new(value: f64) -> Self {
        if value.is_nan() {
            Self::ZERO
        } else {
            Normalized(value.clamp(0.0, 1.0))
        }
    }

    /// The position, in 0–1.
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl From<Normalized> for f64 {
    fn from(n: Normalized) -> f64 {
        n.0
    }
}
