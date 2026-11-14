//! A characterisation plan: which runs a circuit gets, at which levels,
//! on which bins — one per deck, `spice/<name>.plan`, written here (the
//! stimuli are stave-measure's suite) and read by
//! `scripts/ngspice-simulate.py`, which simulates the same runs on the
//! same deck into `spice/<name>.table`.
//!
//! A plan is a pure function of the circuit's input, its knee and its
//! settling time ([`Plan::circuit`], [`Plan::supply`]); the knee and the
//! settling time come from `Circuit<Dk>` on the deck (`model`), rounded,
//! so the committed plan is the same on every platform.

use std::fmt::Write as _;

use stave_measure::suite::excitation::{Excitation, Tone};
use stave_measure::suite::levels::{Criterion, Knee, FAST_OFFSETS_DB, KNEE_OFFSETS_DB};
use stave_measure::suite::multisine::{Multisine, Phases};
use stave_measure::suite::period::{
    harmonic_bins, products, tone_bin, Period, SHIPPING_RATES,
};

/// Which tier a run belongs to: T0 runs on every commit, T1 (a superset)
/// in the references job.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    T0,
    T1,
}

impl Tier {
    fn word(self) -> &'static str {
        match self {
            Tier::T0 => "t0",
            Tier::T1 => "t1",
        }
    }
}

/// The source a plan drives: the deck's input element, split into its
/// DC level (`dc`, the operating point) and a driven source in series
/// (a voltage) or in parallel (a current) that carries the stimulus.
/// `holds` pins other sources of the deck to DC values (a control
/// voltage the deck ramps).
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub source: String,
    pub dc: f64,
    pub holds: Vec<(String, f64)>,
}

impl Input {
    pub fn new(source: &str, dc: f64) -> Self {
        Self {
            source: source.to_ascii_lowercase(),
            dc,
            holds: Vec::new(),
        }
    }

    pub fn holding(mut self, source: &str, dc: f64) -> Self {
        self.holds.push((source.to_ascii_lowercase(), dc));
        self
    }
}

/// What a run measures, and so how it is analysed.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Kind {
    /// A coherent sine on `bin`, `offset_db` from the knee (C4, C5, C6, C10).
    Tone { bin: u32, offset_db: f64 },
    /// An odd-odd multisine at the RMS of a sine `offset_db` from the
    /// knee (C2 at −40, C3): random phases, realisation `seed` (T1 averages
    /// [`REALISATIONS`]; T0's single run is a single-realisation
    /// response), with its crest factor (peak over RMS) on record.
    Multisine {
        offset_db: f64,
        seed: u64,
        crest: f64,
        rms: f64,
    },
    /// SMPTE (60 Hz + 7 kHz, 4:1) or CCIF (19 + 20 kHz) (C7).
    TwoTone {
        low: u32,
        high: u32,
        offset_db: f64,
        ccif: bool,
    },
    /// A probe tone throughout and a burst on `burst` (C8).
    Burst {
        probe: u32,
        burst: u32,
        offset_db: f64,
    },
    /// A power supply's load level: B+ and its ripple on bin `ripple`.
    Load { ripple: u32 },
}

/// One run: an excitation, settled for `settle` periods, then measured
/// over `measure` periods on `bins`.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub id: String,
    pub tier: Tier,
    pub kind: Kind,
    pub settle: u32,
    pub measure: u32,
    pub bins: Vec<u32>,
    pub excitation: Excitation,
}

impl Run {
    pub fn period(&self) -> Period {
        self.excitation.period
    }
}

/// A circuit's plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub circuit: String,
    pub input: Input,
    pub knee: Option<Knee>,
    pub settle_seconds: f64,
    /// The small-signal response's frequencies (C2): the −40 dB
    /// multisine's bins, so a hand topology's BLA reads the same points.
    pub response_hz: Vec<f64>,
    pub runs: Vec<Run>,
}

/// The multisine period: 50 ms, 20 Hz bins.
pub const MULTISINE_PERIOD: Period = Period::units(15);
/// Random-phase realisations T1 averages per multisine level.
pub const REALISATIONS: u64 = 4;
/// The harmonics a tone run reads (C5: H2–H15).
pub const HARMONICS: u32 = 15;

fn multisine() -> Multisine {
    Multisine::odd_odd(MULTISINE_PERIOD, 20.0, 20_000.0, 3.0)
}

/// A multisine run's excitation from its RMS and seed, as the plan builds
/// it (the text form carries only these: the tones are regenerated).
fn multisine_excitation(rms: f64, seed: u64, fade: f64) -> Excitation {
    rounded(multisine().excitation(rms, Phases::Random { seed }).faded_in(fade))
}

/// Periods to settle `seconds`, plus one: the last settling period is
/// the one steady state is checked against (a memoryless circuit's only
/// one).
fn settle_periods(seconds: f64, period: Period) -> u32 {
    (seconds / period.seconds()).ceil() as u32 + 1
}

/// Faded in over half the settling time (at least 0.5 ms), every
/// amplitude and phase to nine significant digits: the plan's text is
/// short and reads back exactly.
fn faded(e: Excitation, settle_seconds: f64) -> Excitation {
    rounded(e.faded_in(0.5 * settle_seconds.max(1e-3)))
}

fn rounded(mut e: Excitation) -> Excitation {
    for t in &mut e.tones {
        t.amplitude = significant(t.amplitude);
        t.phase = significant(t.phase);
    }
    e.dc = significant(e.dc);
    e
}

fn significant(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    format!("{x:.8e}").parse().unwrap()
}

fn label(offset_db: f64) -> String {
    format!("{offset_db:+}")
}

impl Plan {
    /// The plan for a circuit driven by a voltage (or a current) through
    /// `input`, with its `knee` and its settling time.
    ///
    /// T0: the multisine at −40, 0 and +12 dB re knee (C2 for a hand
    /// topology, C3) and a 1 kHz tone at −20, 0 and +12 (C4–C6, C10).
    /// T1 adds the multisine at every offset of the grid, the 1 kHz tone
    /// at every offset, tones at 82, 330, 3 k and 8 kHz (C5) and 100 Hz
    /// and 5 kHz (C4) at −20, −3, 0, +6, +20, SMPTE and CCIF pairs at
    /// −10, 0, +6 (C7), and bursts at +6 and +20 under a −20 probe (C8).
    pub fn circuit(name: &str, input: Input, knee: Knee, settle_seconds: f64) -> Self {
        let mut runs = Vec::new();
        let ms = multisine();
        let ms_settle = settle_periods(settle_seconds, MULTISINE_PERIOD);
        let at = |offset: f64| knee.at(offset);
        for offset in KNEE_OFFSETS_DB {
            for seed in 1..=REALISATIONS {
                // T0: the first realisation at −40, 0 and +12.
                let tier = if seed == 1 && [-40.0, 0.0, 12.0].contains(&offset) {
                    Tier::T0
                } else {
                    Tier::T1
                };
                // "Re knee" by RMS: the sine's RMS at that offset.
                let rms = at(offset) / std::f64::consts::SQRT_2;
                let phases = Phases::Random { seed };
                let crest = (ms.crest_factor(phases) * 1e3).round() / 1e3;
                runs.push(Run {
                    id: format!("multisine/{}/r{seed}", label(offset)),
                    tier,
                    kind: Kind::Multisine {
                        offset_db: offset,
                        seed,
                        crest,
                        rms,
                    },
                    settle: ms_settle,
                    measure: 1,
                    bins: ms.read_bins(),
                    excitation: multisine_excitation(rms, seed, 0.5 * settle_seconds.max(1e-3)),
                });
            }
        }
        let common = [-20.0, -3.0, 0.0, 6.0, 20.0];
        let tones: [(&str, f64, &[f64]); 7] = [
            ("1k", 1_000.0, &KNEE_OFFSETS_DB),
            ("82", 82.0, &common),
            ("100", 100.0, &common),
            ("330", 330.0, &common),
            ("3k", 3_000.0, &common),
            ("5k", 5_000.0, &common),
            ("8k", 8_000.0, &common),
        ];
        for (tag, hz, offsets) in tones {
            // At least four cycles a period, and never under 20 ms.
            let period = Period::units((4.0 * 300.0 / hz).ceil().max(6.0) as u32);
            let bin = tone_bin(period, hz, HARMONICS, &SHIPPING_RATES)
                .unwrap_or_else(|| panic!("no collision-free bin near {hz} Hz"));
            let settle = settle_periods(settle_seconds, period);
            for &offset in offsets {
                let tier = if tag == "1k" && FAST_OFFSETS_DB.contains(&offset) {
                    Tier::T0
                } else {
                    Tier::T1
                };
                let mut bins = vec![0];
                bins.extend(harmonic_bins(bin, HARMONICS).iter().map(|&b| b as u32));
                runs.push(Run {
                    id: format!("tone/{tag}/{}", label(offset)),
                    tier,
                    kind: Kind::Tone {
                        bin,
                        offset_db: offset,
                    },
                    settle,
                    measure: 1,
                    bins,
                    excitation: faded(Excitation::sine(period, bin, at(offset)), settle_seconds),
                });
            }
        }
        let pair_period = Period::units(15);
        let pair_settle = settle_periods(settle_seconds, pair_period);
        for (tag, ccif) in [("smpte", false), ("ccif", true)] {
            for offset in [-10.0, 0.0, 6.0] {
                let e = if ccif {
                    Excitation::ccif(pair_period, at(offset))
                } else {
                    Excitation::smpte(pair_period, at(offset))
                }
                .expect("a collision-free pair");
                let (low, high) = (e.tones[0].bin, e.tones[1].bin);
                let mut bins = vec![0];
                bins.extend(products(low, high, 4).iter().map(|p| p.2 as u32));
                if !ccif {
                    // RP120's sidebands to n = 6 (beyond fourth order).
                    for n in 4..=6 {
                        bins.extend([high + n * low, high - n * low]);
                    }
                }
                runs.push(Run {
                    id: format!("{tag}/{}", label(offset)),
                    tier: Tier::T1,
                    kind: Kind::TwoTone {
                        low,
                        high,
                        offset_db: offset,
                        ccif,
                    },
                    settle: pair_settle,
                    measure: 1,
                    bins,
                    excitation: faded(e, settle_seconds),
                });
            }
        }
        // Bursts: 10 ms periods (100 Hz bins), a 2.3 kHz probe at −20 dB
        // re knee throughout, a 200 Hz burst for five periods after two
        // quiet ones, then fifteen periods of recovery.
        let burst_period = Period::units(3);
        let burst_settle = settle_periods(settle_seconds, burst_period);
        let (probe, burst) = (23, 2);
        for offset in [6.0, 20.0] {
            let on = f64::from(burst_settle + 2) * burst_period.seconds();
            let off = on + 5.0 * burst_period.seconds();
            let e = Excitation::sine(burst_period, probe, at(-20.0))
                .with_tone(Tone::new(at(offset), burst).gated(on, off));
            runs.push(Run {
                id: format!("burst/{}", label(offset)),
                tier: Tier::T1,
                kind: Kind::Burst {
                    probe,
                    burst,
                    offset_db: offset,
                },
                settle: burst_settle,
                measure: 22,
                bins: vec![0, burst, probe],
                excitation: faded(e, settle_seconds),
            });
        }
        Self {
            circuit: name.to_string(),
            input,
            knee: Some(knee),
            settle_seconds,
            response_hz: ms.bins.iter().map(|&k| MULTISINE_PERIOD.hz(k)).collect(),
            runs,
        }
    }

    /// The plan for a power supply whose input is its load current, from
    /// switch-on: B+ and its ripple (bins of 60 Hz) at each extra load,
    /// and its sag and recovery under a load step.
    pub fn supply(name: &str, input: Input, settle_seconds: f64) -> Self {
        let period = Period::units(15); // 50 ms: 60 Hz is bin 3
        let settle = settle_periods(settle_seconds, period);
        let mut bins = vec![0];
        bins.extend((1..=20).map(|h| 3 * h));
        let mut runs = Vec::new();
        for (milliamps, tier) in [(0.0, Tier::T0), (20.0, Tier::T1), (40.0, Tier::T1), (60.0, Tier::T1)] {
            runs.push(Run {
                id: format!("load/+{milliamps}mA"),
                tier,
                kind: Kind::Load { ripple: 6 },
                settle,
                measure: 1,
                bins: bins.clone(),
                excitation: Excitation::idle(period).with_dc(milliamps * 1e-3),
            });
        }
        for (milliamps, tier) in [(60.0, Tier::T0), (20.0, Tier::T1)] {
            let on = f64::from(settle + 1) * period.seconds();
            let off = on + 2.0 * period.seconds();
            // A DC step is a tone on bin 0 with phase π/2.
            let step = Tone::new(milliamps * 1e-3, 0)
                .phased(std::f64::consts::FRAC_PI_2)
                .gated(on, off);
            runs.push(Run {
                id: format!("burst/+{milliamps}mA"),
                tier,
                kind: Kind::Burst {
                    probe: 6,
                    burst: 0,
                    offset_db: 0.0,
                },
                settle,
                measure: 6,
                bins: bins.clone(),
                excitation: Excitation::idle(period).with_tone(step),
            });
        }
        Self {
            circuit: name.to_string(),
            input,
            knee: None,
            settle_seconds,
            response_hz: Vec::new(),
            runs,
        }
    }

    /// The plan with the runs `ids` moved into T0: a circuit with memory
    /// (a cathode or a coupling cap's bias) holds a burst there, so its
    /// recovery is checked on every commit (DSP.md R15).
    pub fn promoted(mut self, ids: &[&str]) -> Self {
        for id in ids {
            let run = self
                .runs
                .iter_mut()
                .find(|r| r.id == *id)
                .unwrap_or_else(|| panic!("{}: no run {id} to promote", self.circuit));
            run.tier = Tier::T0;
        }
        self
    }

    /// The runs of `tier` (T1 includes T0's).
    pub fn runs(&self, tier: Tier) -> impl Iterator<Item = &Run> {
        self.runs.iter().filter(move |r| r.tier <= tier)
    }

    pub fn run(&self, id: &str) -> Option<&Run> {
        self.runs.iter().find(|r| r.id == id)
    }

    /// The text form (the committed `.plan` file).
    pub fn describe(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "# stave characterisation plan v1: {}\n\
             # Written by stave-test (STAVE_BLESS=1 cargo test -p stave-test --test characterise),\n\
             # simulated by scripts/ngspice-simulate.py into {}.table. One run a line:\n\
             # run <id> <tier> <kind…> settle <periods> measure <periods> bins <k…> | <excitation>",
            self.circuit, self.circuit
        );
        let _ = write!(s, "input {} dc {:e}", self.input.source, self.input.dc);
        for (source, dc) in &self.input.holds {
            let _ = write!(s, " hold {source} {dc:e}");
        }
        s.push('\n');
        if let Some(k) = self.knee {
            let c = match k.criterion {
                Criterion::Compression => "compression",
                Criterion::Distortion => "distortion",
                Criterion::Unreached => "unreached",
            };
            let _ = writeln!(s, "knee {:e} {c}", k.amplitude);
        }
        let _ = writeln!(s, "settle {:e}", self.settle_seconds);
        if !self.response_hz.is_empty() {
            s.push_str("response");
            for hz in &self.response_hz {
                let _ = write!(s, " {hz:e}");
            }
            s.push('\n');
        }
        for r in &self.runs {
            let kind = match r.kind {
                Kind::Tone { bin, offset_db } => format!("tone {bin} {offset_db}"),
                Kind::Multisine {
                    offset_db,
                    seed,
                    crest,
                    rms,
                } => format!("multisine {offset_db} random {seed} crest {crest} rms {rms:e}"),
                Kind::TwoTone {
                    low,
                    high,
                    offset_db,
                    ccif,
                } => format!("{} {low} {high} {offset_db}", if ccif { "ccif" } else { "smpte" }),
                Kind::Burst {
                    probe,
                    burst,
                    offset_db,
                } => format!("burst {probe} {burst} {offset_db}"),
                Kind::Load { ripple } => format!("load {ripple}"),
            };
            let bins: Vec<String> = r.bins.iter().map(u32::to_string).collect();
            // A multisine's tones are regenerated from its RMS and seed.
            let excitation = match r.kind {
                Kind::Multisine { seed, rms, .. } => format!(
                    "period {} dc 0e0 fade {:e} multisine {rms:e} {seed}",
                    r.excitation.period.count(),
                    r.excitation.fade_seconds,
                ),
                _ => r.excitation.describe(),
            };
            let _ = writeln!(
                s,
                "run {} {} {kind} settle {} measure {} bins {} | {}",
                r.id,
                r.tier.word(),
                r.settle,
                r.measure,
                bins.join(" "),
                excitation
            );
        }
        s
    }

    /// Read [`Plan::describe`]'s form back.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut plan = Plan {
            circuit: String::new(),
            input: Input::new("", 0.0),
            knee: None,
            settle_seconds: 0.0,
            response_hz: Vec::new(),
            runs: Vec::new(),
        };
        let num = |w: Option<&&str>| -> Result<f64, String> {
            let w = w.ok_or("plan: truncated line")?;
            w.parse::<f64>().map_err(|e| format!("plan: {w}: {e}"))
        };
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# stave characterisation plan v1: ") {
                plan.circuit = rest.trim().to_string();
                continue;
            }
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let (head, excitation) = line.split_once('|').unwrap_or((line, ""));
            let w: Vec<&str> = head.split_whitespace().collect();
            match w[0] {
                "input" => {
                    plan.input = Input::new(w[1], num(w.get(3))?);
                    let mut i = 4;
                    while w.get(i) == Some(&"hold") {
                        plan.input.holds.push((w[i + 1].to_string(), num(w.get(i + 2))?));
                        i += 3;
                    }
                }
                "knee" => {
                    let criterion = match w.get(2) {
                        Some(&"compression") => Criterion::Compression,
                        Some(&"distortion") => Criterion::Distortion,
                        _ => Criterion::Unreached,
                    };
                    plan.knee = Some(Knee {
                        amplitude: num(w.get(1))?,
                        criterion,
                    });
                }
                "settle" => plan.settle_seconds = num(w.get(1))?,
                "response" => {
                    plan.response_hz = w[1..]
                        .iter()
                        .map(|x| x.parse::<f64>().map_err(|e| e.to_string()))
                        .collect::<Result<_, _>>()?;
                }
                "run" => plan.runs.push(parse_run(&w, excitation)?),
                other => return Err(format!("plan: unknown line `{other}`")),
            }
        }
        Ok(plan)
    }
}

fn parse_run(w: &[&str], excitation: &str) -> Result<Run, String> {
    let int = |i: usize| -> Result<u32, String> {
        w.get(i)
            .ok_or("plan: truncated run")?
            .parse::<u32>()
            .map_err(|e| format!("plan: run {}: {e}", w[1]))
    };
    let float = |i: usize| -> Result<f64, String> {
        w.get(i)
            .ok_or("plan: truncated run")?
            .parse::<f64>()
            .map_err(|e| format!("plan: run {}: {e}", w[1]))
    };
    let tier = match w[2] {
        "t0" => Tier::T0,
        "t1" => Tier::T1,
        t => return Err(format!("plan: tier {t}")),
    };
    let (kind, next) = match w[3] {
        "tone" => (
            Kind::Tone {
                bin: int(4)?,
                offset_db: float(5)?,
            },
            6,
        ),
        "multisine" => (
            Kind::Multisine {
                offset_db: float(4)?,
                seed: u64::from(int(6)?),
                crest: float(8)?,
                rms: float(10)?,
            },
            11,
        ),
        "smpte" | "ccif" => (
            Kind::TwoTone {
                low: int(4)?,
                high: int(5)?,
                offset_db: float(6)?,
                ccif: w[3] == "ccif",
            },
            7,
        ),
        "burst" => (
            Kind::Burst {
                probe: int(4)?,
                burst: int(5)?,
                offset_db: float(6)?,
            },
            7,
        ),
        "load" => (Kind::Load { ripple: int(4)? }, 5),
        k => return Err(format!("plan: kind {k}")),
    };
    if w.get(next) != Some(&"settle") || w.get(next + 2) != Some(&"measure") || w.get(next + 4) != Some(&"bins") {
        return Err(format!("plan: run {}: expected settle/measure/bins", w[1]));
    }
    Ok(Run {
        id: w[1].to_string(),
        tier,
        kind,
        settle: int(next + 1)?,
        measure: int(next + 3)?,
        bins: (next + 5..w.len()).map(int).collect::<Result<_, _>>()?,
        excitation: match kind {
            Kind::Multisine { seed, .. } => {
                let x: Vec<&str> = excitation.split_whitespace().collect();
                let num = |i: usize| -> Result<f64, String> {
                    x.get(i)
                        .ok_or("plan: truncated multisine")?
                        .parse::<f64>()
                        .map_err(|e| e.to_string())
                };
                if x.get(6) != Some(&"multisine") {
                    return Err(format!("plan: run {}: expected a multisine excitation", w[1]));
                }
                multisine_excitation(num(7)?, seed, num(5)?)
            }
            _ => Excitation::parse(excitation)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_round_trip_through_text() {
        let knee = Knee {
            amplitude: 0.0125,
            criterion: Criterion::Compression,
        };
        let plan = Plan::circuit("x", Input::new("VIN", 0.0).holding("vc", 5.0), knee, 0.03);
        let back = Plan::parse(&plan.describe()).unwrap();
        assert_eq!(back, plan);
        assert_eq!(plan.runs(Tier::T0).count(), 6);
        assert!(plan.run("multisine/+0/r4").is_some());
        let supply = Plan::supply("s", Input::new("il", 0.02), 0.3);
        assert_eq!(Plan::parse(&supply.describe()).unwrap(), supply);
    }
}
