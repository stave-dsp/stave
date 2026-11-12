//! Compressor styles: each names a classic gain-reduction element and
//! how it shapes the detector and the gain computer.

use stave_core::params::ParamEnum;
use stave_dsp::amp_to_db;

pub const STYLES: &[&str] = &["VCA", "FET", "OPTO", "VMU"];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Style {
    Vca,
    /// 1176: fast, and faster still on transients.
    Fet,
    /// LA-2A: release stretches with gain reduction and has a slow tail.
    Opto,
    /// Vari-mu: soft knee that widens as reduction deepens.
    VariMu,
}

impl ParamEnum for Style {
    const COUNT: u32 = 4;
    fn from_index(i: u32) -> Self {
        match i {
            0 => Self::Vca,
            1 => Self::Fet,
            2 => Self::Opto,
            _ => Self::VariMu,
        }
    }
    fn to_index(self) -> u32 {
        self as u32
    }
    fn name(i: u32) -> &'static str {
        STYLES[Self::from_index(i) as usize]
    }
}

/// How a style shapes detection and the gain curve.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Character {
    pub attack_scale: f32,
    pub release_scale: f32,
    /// Knee width when the band's knee knob is at zero.
    pub knee_db: f32,
    /// How far release stretches as the envelope rises over threshold.
    pub release_dependence: f32,
    /// How far attack shortens on a jump above the envelope, when the
    /// band's transient knob is at zero.
    pub attack_dependence: f32,
    /// The element's slow release tail, before program dependence.
    pub slow_release_ms: Option<f32>,
    pub knee_widens: bool,
}

/// The fastest attack a transient can pull the detector to.
pub const FASTEST_ATTACK_MS: f32 = 0.05;

impl Style {
    pub const fn character(self) -> Character {
        match self {
            Self::Vca => Character {
                attack_scale: 1.0,
                release_scale: 1.0,
                knee_db: 0.0,
                release_dependence: 0.0,
                attack_dependence: 0.0,
                slow_release_ms: None,
                knee_widens: false,
            },
            Self::Fet => Character {
                attack_scale: 0.3,
                release_scale: 0.5,
                knee_db: 2.0,
                release_dependence: 0.15,
                attack_dependence: 0.7,
                slow_release_ms: None,
                knee_widens: false,
            },
            Self::Opto => Character {
                attack_scale: 1.8,
                release_scale: 2.5,
                knee_db: 6.0,
                release_dependence: 0.65,
                attack_dependence: 0.0,
                slow_release_ms: Some(800.0),
                knee_widens: false,
            },
            Self::VariMu => Character {
                attack_scale: 3.0,
                release_scale: 1.5,
                knee_db: 10.0,
                release_dependence: 0.85,
                attack_dependence: 0.0,
                slow_release_ms: Some(400.0),
                knee_widens: true,
            },
        }
    }
}

/// dB of `envelope` over `threshold_db`, in units of 12 dB, at most `cap`.
fn overshoot(envelope: f32, threshold_db: f32, cap: f32) -> f32 {
    ((amp_to_db(envelope) - threshold_db).max(0.0) / 12.0).min(cap)
}

impl Character {
    /// Release (and slow-tail) multiplier: 1 + 2·dependence·overshoot,
    /// so a signal 12 dB hot on OPTO releases ≈ 2.3× slower.
    pub fn release_stretch(&self, envelope: f32, threshold_db: f32) -> f32 {
        1.0 + 2.0 * self.release_dependence * overshoot(envelope, threshold_db, 3.0)
    }

    /// Knee multiplier: a vari-mu tube's grid-current onset is gradual,
    /// so its knee grows up to 1.8× under heavy reduction.
    pub fn knee_stretch(&self, envelope: f32, threshold_db: f32) -> f32 {
        if self.knee_widens {
            1.0 + 0.4 * overshoot(envelope, threshold_db, 2.0)
        } else {
            1.0
        }
    }
}

/// Attack multiplier when the detector input jumps above the envelope:
/// a jump of 8× or more at full `dependence` shortens attack tenfold.
pub fn attack_shrink(dependence: f32, envelope: f32, input: f32) -> f32 {
    if dependence <= 0.0 {
        return 1.0;
    }
    let jump = (input / envelope.max(1e-6)).clamp(1.0, 8.0);
    let intensity = ((jump - 1.0) / 7.0).sqrt();
    (1.0 - dependence * intensity).max(0.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_round_trips() {
        for i in 0..Style::COUNT {
            assert_eq!(Style::from_index(i).to_index(), i);
        }
    }

    #[test]
    fn opto_stretches_release_and_vca_does_not() {
        let hot = stave_dsp::db_to_amp(0.0);
        assert!(Style::Opto.character().release_stretch(hot, -12.0) > 2.0);
        assert_eq!(Style::Vca.character().release_stretch(hot, -12.0), 1.0);
    }

    #[test]
    fn a_big_jump_shortens_attack() {
        assert!(attack_shrink(0.7, 0.01, 0.5) < 0.5);
        assert_eq!(attack_shrink(0.7, 0.5, 0.5), 1.0);
    }
}
