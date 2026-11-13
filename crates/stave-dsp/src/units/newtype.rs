//! The two macros every unit is declared with, so each type carries the
//! same zero-cost shape: `#[repr(transparent)]` over its scalar, a
//! `get()` back to the scalar, and arithmetic only where it reads as
//! physics.

/// Declare a unit: the newtype, `get`, the raw-from-unit `From`, and a
/// constructor that checks the value's range in debug builds only
/// (`debug_assert!` — a release build never panics on the audio path).
///
/// `unit!(Name, f32, "doc", |v| !v.is_nan(), "message")`: the closure is the
/// range check (a `const` expression over `v`).
macro_rules! unit {
    ($(#[$doc:meta])* $name:ident, $raw:ty, |$v:ident| $check:expr, $msg:literal) => {
        $(#[$doc])*
        #[derive(Copy, Clone, Debug, Default, PartialEq, PartialOrd)]
        #[repr(transparent)]
        pub struct $name(pub(super) $raw);

        impl $name {
            /// The value, range-checked in debug builds only.
            #[inline]
            #[track_caller]
            pub const fn new($v: $raw) -> Self {
                debug_assert!($check, $msg);
                Self($v)
            }

            /// The bare scalar, in the unit's base (SI) scale.
            #[inline]
            pub const fn get(self) -> $raw {
                self.0
            }
        }

        impl From<$name> for $raw {
            #[inline]
            fn from(u: $name) -> $raw {
                u.0
            }
        }
    };
}

/// Linear arithmetic for a unit whose values add: `+`, `-` and negation
/// between two of it, scaling by its scalar (either side), division by
/// its scalar, and `unit / unit` as the bare ratio (or, with
/// `quotient_elsewhere`, a quotient the unit declares itself). Results skip the
/// constructor's range check: a difference may be negative.
macro_rules! linear {
    // `unit / unit` is declared beside the unit (`Hz / Hz` is a `Ratio`).
    ($name:ident, $raw:ty, quotient_elsewhere) => {
        impl core::ops::Add for $name {
            type Output = $name;
            #[inline]
            fn add(self, rhs: $name) -> $name {
                $name(self.0 + rhs.0)
            }
        }
        impl core::ops::Sub for $name {
            type Output = $name;
            #[inline]
            fn sub(self, rhs: $name) -> $name {
                $name(self.0 - rhs.0)
            }
        }
        impl core::ops::Neg for $name {
            type Output = $name;
            #[inline]
            fn neg(self) -> $name {
                $name(-self.0)
            }
        }
        impl core::ops::AddAssign for $name {
            #[inline]
            fn add_assign(&mut self, rhs: $name) {
                self.0 += rhs.0;
            }
        }
        impl core::ops::SubAssign for $name {
            #[inline]
            fn sub_assign(&mut self, rhs: $name) {
                self.0 -= rhs.0;
            }
        }
        impl core::ops::Mul<$raw> for $name {
            type Output = $name;
            #[inline]
            fn mul(self, k: $raw) -> $name {
                $name(self.0 * k)
            }
        }
        impl core::ops::Mul<$name> for $raw {
            type Output = $name;
            #[inline]
            fn mul(self, u: $name) -> $name {
                $name(self * u.0)
            }
        }
        impl core::ops::Div<$raw> for $name {
            type Output = $name;
            #[inline]
            fn div(self, k: $raw) -> $name {
                $name(self.0 / k)
            }
        }
    };
    ($name:ident, $raw:ty) => {
        linear!($name, $raw, quotient_elsewhere);
        impl core::ops::Div for $name {
            type Output = $raw;
            #[inline]
            fn div(self, rhs: $name) -> $raw {
                self.0 / rhs.0
            }
        }
    };
}

/// `impl Mul<$b> for $a = $out` and its mirror `$b * $a`, over the raw
/// values (used for products that are another unit, like `A × Ω = V`).
macro_rules! product {
    ($a:ident * $b:ident = $out:ident) => {
        impl core::ops::Mul<$b> for $a {
            type Output = $out;
            #[inline]
            fn mul(self, rhs: $b) -> $out {
                $out(self.0 * rhs.0)
            }
        }
        impl core::ops::Mul<$a> for $b {
            type Output = $out;
            #[inline]
            fn mul(self, rhs: $a) -> $out {
                $out(self.0 * rhs.0)
            }
        }
    };
}

/// `impl Div<$b> for $a = $out` over the raw values (`V / Ω = A`).
macro_rules! quotient {
    ($a:ident / $b:ident = $out:ident) => {
        impl core::ops::Div<$b> for $a {
            type Output = $out;
            #[inline]
            fn div(self, rhs: $b) -> $out {
                $out(self.0 / rhs.0)
            }
        }
    };
}
