//! Circuit quantities for netlists and devices: [`Volts`], [`Amps`],
//! [`Ohms`], [`Farads`], [`Henries`], all `f64` like the solvers that
//! use them (a Newton step on a diode junction resolves currents across
//! twelve decades; `f32` cannot). SI-prefix constructors read as a
//! datasheet or schematic writes them: `kohm(4.7)`, `nf(22.0)`,
//! `uf(1.0)`, `mh(100.0)`, `mv(250.0)`.
//!
//! Ohm's law is in the types: `Volts / Ohms = Amps`, `Volts / Amps =
//! Ohms`, `Amps * Ohms = Volts`; and the two time constants are
//! [`Seconds`]: `Ohms * Farads` (τ = RC) and `Henries / Ohms` (τ = L/R).
//!
//! ```
//! use stave_dsp::units::{kohm, ma, ms, nf, volts};
//!
//! assert_eq!(volts(9.0) / kohm(4.5), ma(2.0));
//! assert!(((kohm(10.0) * nf(100.0)) - ms(1.0)).get().abs() < 1e-15);
//! assert_eq!(kohm(4.7).to_string(), "4.70 kΩ");
//! assert_eq!(nf(22.0).to_string(), "22.0 nF");
//! ```

use core::fmt;

use super::text;
use super::time::Seconds;

unit! {
    /// A potential difference in volts, `f64`. Signed; NaN is a
    /// debug-build panic.
    Volts, f64, |v| !v.is_nan(), "Volts is NaN"
}
linear!(Volts, f64);

unit! {
    /// A current in amperes, `f64`. Signed; NaN is a debug-build panic.
    Amps, f64, |v| !v.is_nan(), "Amps is NaN"
}
linear!(Amps, f64);

unit! {
    /// A resistance in ohms, `f64`. Values add in series
    /// ([`Ohms::parallel`] for the other way). Zero (a short) and `inf`
    /// (an open) are allowed; negative and NaN are debug-build panics.
    Ohms, f64, |v| v >= 0.0, "Ohms is negative or NaN"
}
linear!(Ohms, f64);

unit! {
    /// A capacitance in farads, `f64`. Values add in parallel. Negative
    /// and NaN are debug-build panics.
    Farads, f64, |v| v >= 0.0, "Farads is negative or NaN"
}
linear!(Farads, f64);

unit! {
    /// An inductance in henries, `f64`. Values add in series. Negative
    /// and NaN are debug-build panics.
    Henries, f64, |v| v >= 0.0, "Henries is negative or NaN"
}
linear!(Henries, f64);

/// Constructors, one per SI prefix a datasheet uses for the quantity.
macro_rules! prefixed {
    ($ty:ident: $($(#[$doc:meta])* $f:ident = $scale:expr;)*) => {
        $(
            $(#[$doc])*
            #[inline]
            #[track_caller]
            pub const fn $f(v: f64) -> $ty {
                $ty::new(v * $scale)
            }
        )*
    };
}

prefixed! { Volts:
    /// `v` volts.
    volts = 1.0;
    /// `v` millivolts.
    mv = 1e-3;
    /// `v` microvolts.
    uv = 1e-6;
}
prefixed! { Amps:
    /// `v` amperes.
    amps = 1.0;
    /// `v` milliamperes.
    ma = 1e-3;
    /// `v` microamperes.
    ua = 1e-6;
    /// `v` nanoamperes.
    na = 1e-9;
}
prefixed! { Ohms:
    /// `v` ohms.
    ohms = 1.0;
    /// `v` kilohms.
    kohm = 1e3;
    /// `v` megohms (spelled out: `mohm` would read as milliohms).
    megohm = 1e6;
}
prefixed! { Farads:
    /// `v` farads.
    farads = 1.0;
    /// `v` microfarads.
    uf = 1e-6;
    /// `v` nanofarads.
    nf = 1e-9;
    /// `v` picofarads.
    pf = 1e-12;
}
prefixed! { Henries:
    /// `v` henries.
    henries = 1.0;
    /// `v` millihenries.
    mh = 1e-3;
    /// `v` microhenries.
    uh = 1e-6;
}

quotient!(Volts / Ohms = Amps);
quotient!(Volts / Amps = Ohms);
product!(Amps * Ohms = Volts);
product!(Ohms * Farads = Seconds);
quotient!(Henries / Ohms = Seconds);

impl Ohms {
    /// Two resistances in parallel, `R₁R₂ / (R₁ + R₂)`. A short in
    /// parallel is a short; two opens are an open.
    #[inline]
    pub fn parallel(self, other: Ohms) -> Ohms {
        if self.0 == 0.0 || other.0 == 0.0 {
            return Ohms(0.0);
        }
        Ohms(1.0 / (1.0 / self.0 + 1.0 / other.0))
    }
}

impl fmt::Display for Volts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        text::engineering(f, self.0, "V")
    }
}

impl fmt::Display for Amps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        text::engineering(f, self.0, "A")
    }
}

impl fmt::Display for Ohms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        text::engineering(f, self.0, "Ω")
    }
}

impl fmt::Display for Farads {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        text::engineering(f, self.0, "F")
    }
}

impl fmt::Display for Henries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        text::engineering(f, self.0, "H")
    }
}
