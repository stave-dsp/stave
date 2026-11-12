//! Analysis windows as data. Every window here is a cosine sum,
//! w(u) = Σₖ (−1)ᵏ aₖ cos(2πk·u) over u ∈ [0, 1), so one formula draws
//! them all and the coherent (DC) gain is simply a₀.

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Window {
    /// No taper: exact for bin-centred tones, leaky for everything else.
    Rectangular,
    /// The general-purpose default.
    #[default]
    Hann,
    Blackman,
    /// 4-term, −92 dB sidelobes: for low-level components near loud ones.
    BlackmanHarris,
    /// Amplitude-accurate between bins (< 0.01 dB scallop): for reading
    /// levels off a spectrum.
    FlatTop,
}

impl Window {
    pub const ALL: [Window; 5] = [
        Self::Rectangular,
        Self::Hann,
        Self::Blackman,
        Self::BlackmanHarris,
        Self::FlatTop,
    ];

    fn cosine_terms(self) -> &'static [f64] {
        match self {
            Self::Rectangular => &[1.0],
            Self::Hann => &[0.5, 0.5],
            Self::Blackman => &[0.42, 0.5, 0.08],
            Self::BlackmanHarris => &[0.358_75, 0.488_29, 0.141_28, 0.011_68],
            Self::FlatTop => &[
                0.215_578_95,
                0.416_631_58,
                0.277_263_158,
                0.083_578_947,
                0.006_947_368,
            ],
        }
    }

    /// The window's value at `position` ∈ [0, 1] across its span.
    pub fn at(self, position: f64) -> f64 {
        self.cosine_terms()
            .iter()
            .enumerate()
            .map(|(k, a)| {
                let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
                sign * a * (std::f64::consts::TAU * k as f64 * position).cos()
            })
            .sum()
    }

    /// `len` periodic coefficients (the DFT-even form analysis wants).
    pub fn coefficients(self, len: usize) -> Vec<f64> {
        (0..len).map(|i| self.at(i as f64 / len as f64)).collect()
    }

    /// The mean of the window: how much it scales a bin-centred tone.
    pub fn coherent_gain(self) -> f64 {
        self.cosine_terms()[0]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Rectangular => "rectangular",
            Self::Hann => "hann",
            Self::Blackman => "blackman",
            Self::BlackmanHarris => "blackman-harris",
            Self::FlatTop => "flat-top",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum::fft;

    #[test]
    fn coherent_gain_is_the_mean_of_the_coefficients() {
        for w in Window::ALL {
            let c = w.coefficients(4096);
            let mean = c.iter().sum::<f64>() / c.len() as f64;
            assert!((mean - w.coherent_gain()).abs() < 1e-9, "{}", w.name());
        }
    }

    #[test]
    fn tapers_start_at_zero_and_peak_in_the_middle() {
        for w in [Window::Hann, Window::Blackman, Window::BlackmanHarris] {
            assert!(w.at(0.0).abs() < 1e-4, "{}", w.name());
            assert!((w.at(0.5) - w.coefficients(2)[1]).abs() < 1e-12);
        }
        assert!((Window::Hann.at(0.5) - 1.0).abs() < 1e-12);
    }

    /// Peak bin amplitude, corrected for coherent gain, of a tone half
    /// a bin off-centre — the worst case for scalloping.
    fn half_bin_reading_db(w: Window) -> f64 {
        let n = 4096;
        let coeffs = w.coefficients(n);
        let mut re: Vec<f64> = (0..n)
            .map(|i| (std::f64::consts::TAU * 100.5 * i as f64 / n as f64).sin() * coeffs[i])
            .collect();
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);
        let peak = (95..106).map(|b| re[b].hypot(im[b])).fold(0.0, f64::max);
        20.0 * (peak * 2.0 / (n as f64 * w.coherent_gain())).log10()
    }

    #[test]
    fn flat_top_reads_amplitude_between_bins_and_hann_scallops() {
        assert!(half_bin_reading_db(Window::FlatTop).abs() < 0.02);
        assert!((half_bin_reading_db(Window::Hann) + 1.42).abs() < 0.05);
    }
}
