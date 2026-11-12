pub mod dempwolf;
pub mod koren;

/// ln(1 + eᶻ) and its derivative, the logistic sigmoid, in the stable
/// form that needs no overflow cutoff: max(z, 0) + ln(1 + e^−|z|).
#[inline]
pub(crate) fn softplus(z: f64) -> (f64, f64) {
    let tail = (-z.abs()).exp();
    let value = z.max(0.0) + tail.ln_1p();
    let slope = if z >= 0.0 {
        1.0 / (1.0 + tail)
    } else {
        tail / (1.0 + tail)
    };
    (value, slope)
}

#[cfg(test)]
mod tests {
    use super::softplus;

    #[test]
    fn softplus_holds_its_identities_with_no_overflow_anywhere() {
        assert!((softplus(0.0).0 - std::f64::consts::LN_2).abs() < 1e-16);
        for z in [-800.0, -40.0, -3.0, -1e-9, 0.5, 3.0, 40.0, 800.0] {
            let ((v, s), (mirror, _)) = (softplus(z), softplus(-z));
            assert!(v.is_finite() && v >= 0.0, "{z}");
            assert!((v - mirror - z).abs() <= 1e-15 * z.abs().max(1.0), "{z}");
            let h = 1e-6 * z.abs().max(1.0);
            let numeric = (softplus(z + h).0 - softplus(z - h).0) / (2.0 * h);
            assert!((s - numeric).abs() < 1e-6, "{z}");
        }
        assert!((softplus(-40.0).0 - f64::exp(-40.0)).abs() < 1e-30);
    }
}

/// Currents into a triode's plate and grid (cathode current is their
/// sum) and their partial derivatives, for voltages grid-to-cathode and
/// plate-to-cathode.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct TriodeCurrents {
    pub plate: f64,
    pub grid: f64,
    pub plate_per_grid_volt: f64,
    pub plate_per_plate_volt: f64,
    pub grid_per_grid_volt: f64,
}

/// A triode model; a topology takes any, as a SPICE element takes any
/// model level.
pub trait TriodeModel: Copy {
    fn currents(&self, vgk: f64, vpk: f64) -> TriodeCurrents;
}

impl TriodeModel for koren::Koren {
    #[inline]
    fn currents(&self, vgk: f64, vpk: f64) -> TriodeCurrents {
        let p = self.plate(vgk, vpk);
        TriodeCurrents {
            plate: p.amps,
            grid: 0.0,
            plate_per_grid_volt: p.per_grid_volt,
            plate_per_plate_volt: p.per_plate_volt,
            grid_per_grid_volt: 0.0,
        }
    }
}

/// Any triode model, chosen at run time (a catalog setting).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Tube {
    /// No grid current.
    Koren(koren::Koren),
    /// With grid current.
    Dempwolf(dempwolf::Dempwolf),
}

impl Tube {
    pub const ALL: [Self; 8] = [
        Self::Koren(koren::Koren::T12AX7),
        Self::Koren(koren::Koren::T12AT7),
        Self::Koren(koren::Koren::T12AU7),
        Self::Koren(koren::Koren::T6DJ8),
        Self::Koren(koren::Koren::T6SN7),
        Self::Dempwolf(dempwolf::Dempwolf::RSD_1),
        Self::Dempwolf(dempwolf::Dempwolf::RSD_2),
        Self::Dempwolf(dempwolf::Dempwolf::EHX_1),
    ];
    pub const NAMES: [&'static str; 8] = {
        let mut out = [""; 8];
        let mut i = 0;
        while i < out.len() {
            out[i] = Self::ALL[i].name();
            i += 1;
        }
        out
    };

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Koren(t) => t.name,
            Self::Dempwolf(t) => t.name,
        }
    }
}

impl TriodeModel for Tube {
    #[inline]
    fn currents(&self, vgk: f64, vpk: f64) -> TriodeCurrents {
        match self {
            Self::Koren(t) => t.currents(vgk, vpk),
            Self::Dempwolf(t) => t.currents(vgk, vpk),
        }
    }
}
