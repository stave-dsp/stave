//! A pn junction as SPICE's transistor loads evaluate one (`bjtload.c`,
//! `jfetload.c`): Shockley forward and down to three knees of reverse
//! bias, then a cubic that settles on −Is, and SPICE's `pnjlim` step
//! bound.

/// `(current, conductance)` of a junction of saturation current `is`
/// and knee `knee = n·Vt` at `volts`, as SPICE evaluates it.
#[inline]
pub(crate) fn current_and_conductance(volts: f64, is: f64, knee: f64) -> (f64, f64) {
    if volts >= -3.0 * knee {
        let e = (volts / knee).exp();
        (is * (e - 1.0), is * e / knee)
    } else {
        let arg = 3.0 * knee / (volts * std::f64::consts::E);
        let arg = arg * arg * arg;
        (-is * (1.0 + arg), is * 3.0 * arg / volts)
    }
}

/// SPICE's `pnjlim`: a step from `old` to `new` above the junction's
/// critical voltage is taken logarithmically, so a Newton iterate never
/// climbs far up the exponential. Steps elsewhere are untouched.
#[inline]
pub(crate) fn limit_step(new: f64, old: f64, is: f64, knee: f64) -> f64 {
    if is <= 0.0 {
        return new;
    }
    let critical = knee * (knee / (std::f64::consts::SQRT_2 * is)).ln();
    if new <= critical || (new - old).abs() <= 2.0 * knee {
        return new;
    }
    if old > 0.0 {
        let arg = 1.0 + (new - old) / knee;
        if arg > 0.0 {
            old + knee * arg.ln()
        } else {
            critical
        }
    } else {
        knee * (new / knee).ln()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reverse_cubic_meets_the_exponential_in_value_and_slope() {
        let (is, knee) = (1e-14, 0.026);
        let v = -3.0 * knee;
        let (a, ga) = current_and_conductance(v + 1e-12, is, knee);
        let (b, gb) = current_and_conductance(v - 1e-12, is, knee);
        assert!((a - b).abs() < 1e-24, "{a} {b}");
        assert!((ga - gb).abs() < 1e-6 * ga, "{ga} {gb}");
        assert!((current_and_conductance(-50.0, is, knee).0 + is).abs() < 1e-20);
    }

    #[test]
    fn a_limited_step_never_climbs_past_where_it_was_asked_to_go() {
        let (is, knee) = (1e-14, 0.026);
        for (new, old) in [(5.0, 0.6), (0.9, 0.0), (30.0, -1.0)] {
            let l = limit_step(new, old, is, knee);
            assert!(l <= new && l.is_finite(), "{new} {old} -> {l}");
        }
        assert_eq!(limit_step(0.3, 0.29, is, knee), 0.3);
    }
}
