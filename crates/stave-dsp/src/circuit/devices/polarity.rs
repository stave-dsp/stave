//! Which way a transistor conducts: N (NPN, N-channel) or P (PNP,
//! P-channel). A P device is its N twin with every terminal voltage and
//! current negated, as SPICE evaluates it.

/// N or P: NPN or PNP for a BJT, N- or P-channel for a FET.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Polarity {
    /// NPN, N-channel.
    N,
    /// PNP, P-channel.
    P,
}

impl Polarity {
    /// +1 for N, −1 for P: the factor that maps terminal voltages to the
    /// N twin's, and the N twin's currents back.
    #[inline]
    pub const fn sign(self) -> f64 {
        match self {
            Polarity::N => 1.0,
            Polarity::P => -1.0,
        }
    }
}
