//! A curve tracer and a bench in code: the operating points a datasheet's
//! figure is measured at, shared by `matches_datasheet.rs` (DSP.md R13),
//! which holds the device models to the R23 tables, and
//! `fit_datasheet.rs` (E11), which fits the presets to them.
#![allow(dead_code)]

/// The model value across `x ± ux` nearest the reading `y`.
pub fn nearest(x: f64, y: f64, ux: f64, model: &impl Fn(f64) -> f64) -> f64 {
    (0..=8)
        .map(|k| model(x + ux * (f64::from(k) / 4.0 - 1.0)))
        .min_by(|a, b| (a - y).abs().total_cmp(&(b - y).abs()))
        .expect("nine samples")
}

/// The root of an increasing `f` in `[lo, hi]` by bisection, to 1e-12.
pub fn bisect(mut lo: f64, mut hi: f64, f: impl Fn(f64) -> f64) -> f64 {
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if f(mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
        if hi - lo < 1e-12 {
            break;
        }
    }
    0.5 * (lo + hi)
}

// ----------------------------------------------------------- transistors

pub mod bjt {
    use stave_dsp::circuit::devices::bjt::{Bjt, GummelPoon};

    use super::bisect;

    /// Collector and base current magnitudes at intrinsic junction
    /// magnitudes (`vbe`, `vce`), N-sense for either polarity.
    fn intrinsic(q: &GummelPoon, vbe: f64, vce: f64) -> (f64, f64) {
        let s = q.polarity.sign();
        let c = q.currents(s * vbe, s * (vbe - vce));
        (s * c.collector, s * c.base)
    }

    /// The intrinsic `vbe` that draws base current `ib` at intrinsic `vce`.
    fn vbe_for_base(q: &GummelPoon, ib: f64, vce: f64) -> f64 {
        bisect(-0.5, 1.5, |vbe| intrinsic(q, vbe, vce).1 - ib)
    }

    /// The operating point with base current |Ib| forced at terminal
    /// |Vce| (a curve tracer): `(intrinsic vce, intrinsic vbe, |Ic|)`,
    /// the card's ohmic emitter and collector resistances included.
    fn operating(q: &GummelPoon, ib: f64, vce: f64) -> (f64, f64, f64) {
        let at = |vce_int: f64| {
            let vbe = vbe_for_base(q, ib, vce_int);
            (vbe, intrinsic(q, vbe, vce_int).0)
        };
        let terminal = |vce_int: f64| {
            let ic = at(vce_int).1;
            vce_int + ic * q.collector_ohms + (ic + ib) * q.emitter_ohms
        };
        let vce_int = bisect(-0.5, vce, |v| terminal(v) - vce);
        let (vbe, ic) = at(vce_int);
        (vce_int, vbe, ic)
    }

    /// |Ic| at terminal |Vce| with |Ib| forced: the output family.
    pub fn collector_at(q: &GummelPoon, ib: f64, vce: f64) -> f64 {
        operating(q, ib, vce).2
    }

    /// Terminal |VBE| with |Ib| forced at terminal |Vce|: the junction's
    /// plus the base and emitter resistances' drops.
    pub fn terminal_vbe(q: &GummelPoon, ib: f64, vce: f64) -> f64 {
        let (_, vbe, ic) = operating(q, ib, vce);
        vbe + ib * q.base_ohms + (ic + ib) * q.emitter_ohms
    }

    /// The terminal |Vce| at which |Ib| draws |Ic| (the saturation
    /// region); 5 V when it cannot below that.
    pub fn vce_for(q: &GummelPoon, ib: f64, ic: f64) -> f64 {
        bisect(0.0, 5.0, |vce| collector_at(q, ib, vce) - ic)
    }

    /// DC current gain Ic/Ib at |Ic| and terminal |Vce|.
    pub fn hfe_at(q: &GummelPoon, ic: f64, vce: f64) -> f64 {
        let ib = bisect(0.0, ic, |ib| collector_at(q, ib, vce) - ic);
        ic / ib
    }
}

/// A diode's forward voltage at `amps`: the junction's plus the series
/// resistance's drop.
pub fn diode_volts(d: &stave_dsp::circuit::devices::diode::Shockley, amps: f64) -> f64 {
    let junction = bisect(-1.0, 3.0, |v| d.current(v) - amps);
    junction + amps * d.series_ohms
}

// --------------------------------------------------------------- op-amps

pub mod opamp {
    use stave_dsp::circuit::devices::opamp::{Amp, Macromodel};
    use stave_dsp::circuit::methods::dk::Dk;
    use stave_dsp::circuit::netlist::{ElementKind, Netlist, Node, Probe, Source};

    /// A unity follower of `part` on `supply`, its input source driven.
    pub fn follower(part: Macromodel, supply: (f64, f64)) -> Dk {
        let mut n = Netlist::new();
        let (input, out) = (n.add_node("in"), n.add_node("out"));
        n.add(
            "v1",
            ElementKind::VoltageSource {
                a: input,
                b: Node::GROUND,
                source: Source::dc(0.0),
            },
        )
        .unwrap();
        n.add(
            "u1",
            ElementKind::OpAmp {
                plus: input,
                minus: out,
                output: out,
                amp: Amp::Macromodel(part),
                supply,
            },
        )
        .unwrap();
        n.probe = Some(Probe::node(out));
        let mut dk = Dk::new(&n).unwrap();
        dk.drive("v1").unwrap();
        dk
    }

    /// The follower's output at each of `times` (seconds, ascending) for
    /// an input `input(t)`, stepped at `rate` and read by linear
    /// interpolation.
    pub fn response(dk: &mut Dk, rate: f64, input: impl Fn(f64) -> f64, times: &[f64]) -> Vec<f64> {
        dk.prepare(rate as f32);
        let end = times.iter().copied().fold(0.0, f64::max);
        let steps = (end * rate).ceil() as usize + 1;
        let trace: Vec<f64> = (0..=steps)
            .map(|i| dk.step(input(i as f64 / rate)))
            .collect();
        times
            .iter()
            .map(|&t| {
                let k = t * rate;
                let i = (k.floor() as usize).min(steps - 1);
                let frac = k - i as f64;
                trace[i] + frac * (trace[i + 1] - trace[i])
            })
            .collect()
    }

    /// The largest output slope (V/s) of the follower for a step from
    /// `from` to `to` volts on `supply`, stepped at 1 GHz.
    pub fn slew(part: Macromodel, supply: (f64, f64), from: f64, to: f64) -> f64 {
        let mut dk = follower(part, supply);
        let rate = 1e9;
        // Settle at `from` (the driven source rests at 0), then step.
        let slowest = part.slew_rate.min(part.slew_rate_falling);
        let start = 1e-6 + 2.0 * from.abs() / slowest;
        let swing = (to - from).abs();
        let times: Vec<f64> = (0..=(4.0 * swing / slowest * rate) as usize)
            .map(|i| start + i as f64 / rate)
            .collect();
        let out = response(&mut dk, rate, |t| if t < start { from } else { to }, &times);
        out.windows(2)
            .map(|w| (w[1] - w[0]).abs() * rate)
            .fold(0.0, f64::max)
    }

    /// The least-squares slope (V/s) of the points of `(t, v)` between
    /// 20% and 80% of the way from `low` to `high`, within `[from, to]`.
    pub fn edge_slope(t: &[f64], v: &[f64], low: f64, high: f64, from: f64, to: f64) -> f64 {
        let span = high - low;
        let inside: Vec<(f64, f64)> = t
            .iter()
            .zip(v)
            .filter(|(&t, &v)| {
                let k = (v - low) / span;
                t >= from && t <= to && (0.2..=0.8).contains(&k)
            })
            .map(|(&t, &v)| (t, v))
            .collect();
        assert!(inside.len() >= 3, "an edge needs points: {inside:?}");
        let n = inside.len() as f64;
        let (mt, mv) = inside
            .iter()
            .fold((0.0, 0.0), |(a, b), (t, v)| (a + t / n, b + v / n));
        let (num, den) = inside.iter().fold((0.0, 0.0), |(a, b), (t, v)| {
            (a + (t - mt) * (v - mv), b + (t - mt) * (t - mt))
        });
        num / den
    }

    /// One edge of a pulse response: its slope (V/s, magnitude) on the
    /// sheet and in the model, and the sheet slope's relative reading
    /// uncertainty.
    pub struct Edge {
        pub name: &'static str,
        pub sheet: f64,
        pub model: f64,
        pub reading: f64,
    }

    /// The voltage-follower pulse response of `table` (curves `output=1`
    /// and `input=1`): the model driven by the sheet's drawn input, and
    /// each edge's slew (20–80% of the swing, least squares) read off
    /// both the same way. The drawing is idealised (straight segments, a
    /// drawn step at the edge's start), so its slopes are compared, not
    /// its points.
    pub fn edges(table: &str, part: Macromodel) -> [Edge; 2] {
        let sheet = stave_test::reference::Datasheet::load(table);
        let vcc = sheet.condition("vcc").expect("the chart's supply");
        let output = &sheet.curve(&[("output", 1.0)]).reference;
        let input = &sheet.curve(&[("input", 1.0)]).reference;
        let (low, high) = (
            input.values()[0],
            input.values().iter().copied().fold(f64::MIN, f64::max),
        );
        // The input's falling edge: between its last high and first low point after the rise.
        let fall = {
            let (t, v) = (input.abscissa(), input.values());
            let i = (1..v.len())
                .find(|&i| t[i] > 0.0 && v[i] < 0.5 * (low + high))
                .expect("a falling edge");
            0.5 * (t[i - 1] + t[i])
        };
        let slowest = part.slew_rate.min(part.slew_rate_falling);
        let start = -output.abscissa()[0] + 2.0 * (high - low) / slowest;
        let mut dk = follower(part, (-vcc, vcc));
        let rate = 1e9;
        let times: Vec<f64> = output.abscissa().iter().map(|t| t + start).collect();
        let drive = |t: f64| {
            let t = t - start;
            if t < -0.5 * start {
                0.0
            } else if (0.0..fall).contains(&t) {
                high
            } else {
                low
            }
        };
        let model = response(&mut dk, rate, drive, &times);
        let t = output.abscissa();
        let ux = sheet.uncertainty_x().at(0.0);
        let uy = sheet.uncertainty_y().at(0.0);
        [("rising", 0.0, fall), ("falling", fall, f64::MAX)].map(|(name, from, to)| {
            let sheet_slope = edge_slope(t, output.values(), low, high, from, to).abs();
            // The slope's reading uncertainty: a point's x and y error
            // over the 20-80% edge's extent.
            let rise = 0.6 * (high - low);
            Edge {
                name,
                sheet: sheet_slope,
                model: edge_slope(t, &model, low, high, from, to).abs(),
                reading: 2.0 * uy / rise + 2.0 * ux * sheet_slope / rise,
            }
        })
    }

    /// The largest undistorted swing (V peak to peak) at each frequency
    /// on a ±`vcc` supply, the full-power bandwidth: the model's own slew
    /// (simulated) as SR/(π·f), capped at its own simulated output swing.
    pub fn swing_at(part: Macromodel, vcc: f64) -> impl Fn(f64) -> f64 {
        let supply = (-vcc, vcc);
        let sr = slew(part, supply, -5.0, 5.0);
        let rails = {
            let mut dk = follower(part, supply);
            let times = [20e-6];
            let up = response(&mut dk, 1e8, |_| 2.0 * vcc, &times)[0];
            let down = response(&mut dk, 1e8, |_| -2.0 * vcc, &times)[0];
            up - down
        };
        move |f| (sr / (std::f64::consts::PI * f)).min(rails)
    }
}
