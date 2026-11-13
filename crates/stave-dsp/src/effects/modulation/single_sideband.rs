use crate::primitives::filters::hilbert::Hilbert;

/// Which sideband [`SingleSideband`] keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sideband {
    /// The sum frequencies, `carrier + signal`.
    Upper,
    /// The difference frequencies, `carrier − signal`.
    Lower,
}

/// Single-sideband modulation of a signal by any carrier: one sideband of
/// the ring-modulated pair, the other cancelled.
///
/// # Models
/// SSB modulation by the phasing method: with both inputs' analytic
/// signals `a` (signal) and `c` (carrier), the upper sideband is
/// `Re{a·c}` and the lower `Re{a·c̄}` — ring modulation's
/// `a·c` with one of its two sidebands removed. No hardware claim.
///
/// # Derivation
/// Two identical [`Hilbert`] pairs (90° ± 0.5° from 20 Hz by default), so
/// the common allpass phase they share is the same on both inputs, and
/// four multiplies.
///
/// # Verified
/// `crates/stave-dsp/src/effects/modulation/tests.rs::single_sideband_keeps_one_sideband`:
/// a 1 kHz carrier modulated by tones from 50 Hz to 8 kHz at 48 kHz: the
/// kept sideband at unity (twice ring modulation's) within 1e-3 dB, the
/// other at least 40 dB below it (measured 44.2 dB).
///
/// # Cost
/// Datasheet `"SingleSideband"` at 48 kHz.
///
/// # Aliasing
/// A sum above Nyquist folds; a difference below 0 Hz reflects.
///
/// # Valid ranges
/// Both inputs inside the Hilbert band (20 Hz to fs/2 − 20 Hz by default).
#[derive(Clone, Debug)]
pub struct SingleSideband {
    sideband: Sideband,
    signal: Hilbert,
    carrier: Hilbert,
}

impl SingleSideband {
    /// A modulator keeping `sideband`, on the default [`Hilbert`] design.
    pub fn new(sideband: Sideband) -> Self {
        Self::with_hilbert(sideband, Hilbert::default())
    }

    /// The same on a [`Hilbert`] of the caller's design, used for both
    /// inputs.
    pub fn with_hilbert(sideband: Sideband, hilbert: Hilbert) -> Self {
        Self {
            sideband,
            signal: hilbert.clone(),
            carrier: hilbert,
        }
    }

    pub fn set_sideband(&mut self, sideband: Sideband) {
        self.sideband = sideband;
    }

    /// Design both Hilbert pairs for `sample_rate` and clear them.
    pub fn prepare(&mut self, sample_rate: f32) {
        use crate::kernel::node::Stage;
        self.signal.prepare(sample_rate);
        self.carrier.prepare(sample_rate);
    }

    pub fn reset(&mut self) {
        use crate::kernel::node::Stage;
        self.signal.reset();
        self.carrier.reset();
    }

    /// One sample of `signal` modulating `carrier`.
    #[inline]
    pub fn modulate(&mut self, signal: f32, carrier: f32) -> f32 {
        let (ar, ai) = self.signal.analytic_f64(f64::from(signal));
        let (cr, ci) = self.carrier.analytic_f64(f64::from(carrier));
        match self.sideband {
            Sideband::Upper => (ar * cr - ai * ci) as f32,
            Sideband::Lower => (ar * cr + ai * ci) as f32,
        }
    }
}
