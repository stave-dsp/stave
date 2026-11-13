//! The bow: a friction force at the contact point, solved against the
//! string's travelling waves each sample.

/// A bow on the string: how hard and how fast. Its position is the
/// string's contact point ([`Waveguide::set_position`](super::Waveguide::set_position)).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bow {
    /// Normal force on the string, N (a violinist's ~0.1–2 N).
    pub force: f64,
    /// Bow speed, m/s (~0.05–1).
    pub velocity: f64,
}

/// The bow–string friction curve: the coefficient falls from
/// `stiction` when stuck toward `kinetic` as the slip speed grows,
/// `μ(Δv) = μ_d + (μ_s − μ_d) v₀/(v₀ + |Δv|)` — the hyperbolic curve of
/// the bowed-string literature (McIntyre, Schumacher & Woodhouse 1983;
/// Woodhouse 2004).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Friction {
    /// `μ_s`, the static coefficient.
    pub stiction: f64,
    /// `μ_d`, the limit at high slip speed.
    pub kinetic: f64,
    /// `v₀`, m/s: the slip speed at which the excess over `μ_d` halves.
    pub knee: f64,
}

impl Friction {
    /// Rosin on gut or steel: `μ_s = 0.8`, `μ_d = 0.3`, `v₀ = 0.1` m/s,
    /// the values commonly used with the hyperbolic curve (a stated
    /// voicing, not a measurement made here).
    pub const ROSIN: Self = Self { stiction: 0.8, kinetic: 0.3, knee: 0.1 };

    /// `μ` at slip speed `slip` (m/s, either sign).
    pub fn coefficient(&self, slip: f64) -> f64 {
        self.kinetic + (self.stiction - self.kinetic) * self.knee / (self.knee + slip.abs())
    }
}

/// The contact point's velocity under the bow, given the velocity the
/// incoming waves alone would give it (`v_h`), the bow, and `φ = F/2Z`
/// (`Z` the string's wave impedance): stick when the friction can hold
/// the string to the bow (`|v_b − v_h| ≤ φ μ_s`) and it was stuck, or no
/// slipping solution exists; otherwise slip, solving
/// `(v_b − v_h) − Δ = φ μ(Δ) sgn Δ` for the slip speed `Δ = v_b − v`,
/// a quadratic on the hyperbolic curve, its larger root (the
/// hysteresis rule of McIntyre, Schumacher & Woodhouse: the state
/// changes only when the old one ceases to exist).
#[inline]
pub(super) fn contact(friction: &Friction, phi: f64, bow: f64, incoming: f64, sticking: &mut bool) -> f64 {
    let dh = bow - incoming;
    let a = dh.abs();
    let hold = phi * friction.stiction;
    // The slip solution with the sign of dh, if any: x² − b x − c = 0,
    // b = a − φμ_d − v₀, c = v₀(a − φμ_s).
    let b = a - phi * friction.kinetic - friction.knee;
    let c = friction.knee * (a - hold);
    let disc = b * b + 4.0 * c;
    let slip = if disc >= 0.0 {
        let x = 0.5 * (b + disc.sqrt());
        (x > 0.0).then_some(x)
    } else {
        None
    };
    let stick = a <= hold && (*sticking || slip.is_none());
    match slip {
        Some(x) if !stick => {
            *sticking = false;
            bow - dh.signum() * x
        }
        _ => {
            *sticking = true;
            bow
        }
    }
}
