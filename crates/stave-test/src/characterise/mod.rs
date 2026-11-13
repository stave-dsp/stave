//! A characterisation matrix per circuit (DSP.md R17 and its
//! "Measurement design"): one builder call holds a model to its
//! reference on every characteristic at once — operating point,
//! small-signal response in magnitude and phase, the best linear
//! approximation against level, compression, each harmonic H2–H15 in
//! magnitude and phase, THD+N, intermodulation, dynamics after a burst
//! and aliasing — each with its own budget, oversampled and at the
//! shipping rates.
//!
//! The pieces:
//!
//! - a **plan** ([`plan`], `spice/<name>.plan`): the runs, built from
//!   stave-measure's [`suite`](stave_measure::suite) at levels relative
//!   to the circuit's knee;
//! - a **reference** ([`table`]): ngspice's simulation of the same plan
//!   on the same deck (`spice/<name>.table`, by
//!   `scripts/ngspice-simulate.py`), or a pinned fingerprint, a measured
//!   target or a capture — one table format for all;
//! - a **model** ([`model`]): `Circuit<Dk>` or `Circuit<Wdf>` on the
//!   deck, or a hand topology's ticking stage, run through the plan into
//!   a table;
//! - the **comparison** ([`compare`]), check by check.
//!
//! Tiers: [`Tier::T0`] (every commit: C1, C2, C3 and C5 at three levels
//! at 1 kHz, C10 at 48 kHz, well under a second) and [`Tier::T1`] (the
//! references job: the whole grid, at 44.1, 48 and 96 kHz too).
//!
//! ```
//! use stave_test::characterise::{Budget, Characterise, Check};
//!
//! // Circuit<Dk> on spice/diode_clipper.cir, held to ngspice's run of
//! // spice/diode_clipper.plan: the T0 matrix, oversampled 8× and at 48 kHz.
//! Characterise::ngspice("diode_clipper")
//!     .circuit()
//!     .oversampled(8)
//!     .budget(Check::OperatingPoint, Budget::volts(1e-9))
//!     // The deck keeps the 1N4148's 4 pF junction, which DK leaves out (R11).
//!     .budget(Check::Response, Budget::db(0.01).degrees(0.05))
//!     .budget(Check::Harmonics, Budget::db(0.5).degrees(5.0))
//!     .shipping(Check::Aliasing, Budget::dbc(-20.0))
//!     .run()
//!     .assert();
//! ```

pub mod compare;
pub mod fingerprint;
pub mod model;
pub mod plan;
pub mod table;

use std::fmt;

use stave_dsp::circuit::netlist::Netlist;
use stave_measure::suite::period::SHIPPING_RATES;

pub use compare::{Check, Measurement};
pub use fingerprint::Fingerprint;
pub use model::Model;
pub use plan::{Input, Plan, Tier};
pub use table::{Table, Target};

use table::fixtures;

/// The rates a check is judged at.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rates {
    /// The solver's rate: 48 kHz times the case's oversampling.
    Oversampled,
    /// The host rates a plugin ships at (T0: 48 kHz; T1: 44.1, 48 and
    /// 96 kHz), no oversampling of the model's own.
    Shipping,
}

/// A check's budget: its first error (dB, volts, or a dBc ceiling for
/// aliasing) and, where it has one, its second (degrees; dB for
/// dynamics).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Budget {
    pub first: f64,
    pub second: Option<f64>,
}

impl Budget {
    pub fn db(db: f64) -> Self {
        Self {
            first: db,
            second: None,
        }
    }

    pub fn volts(volts: f64) -> Self {
        Self::db(volts)
    }

    /// A ceiling on the model's own level, re the fundamental.
    pub fn dbc(dbc: f64) -> Self {
        Self::db(dbc)
    }

    pub fn degrees(self, degrees: f64) -> Self {
        Self {
            second: Some(degrees),
            ..self
        }
    }

    /// The second error of [`Check::Dynamics`]: the probe's level, in dB.
    pub fn probe_db(self, db: f64) -> Self {
        self.degrees(db)
    }
}

/// A characterisation: a circuit's plan held to a target on a model.
pub struct Characterise {
    name: String,
    target: Target,
    plan: Plan,
    deck: Netlist,
    tier: Tier,
    oversample: u32,
    budgets: Vec<(Check, Rates, Budget)>,
    model: Option<Model>,
    deck_name: String,
    catalogue: Option<String>,
}

impl Characterise {
    /// `spice/<name>.cir` through `spice/<name>.plan`, against ngspice's
    /// `spice/<name>.table`.
    pub fn ngspice(name: &str) -> Self {
        let plan_path = fixtures().join(format!("spice/{name}.plan"));
        let text = std::fs::read_to_string(&plan_path).unwrap_or_else(|e| {
            panic!(
                "{}: {e} (write it with STAVE_BLESS=1 cargo test -p stave-test --test characterise)",
                plan_path.display()
            )
        });
        let plan = Plan::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", plan_path.display()));
        Self {
            name: name.to_string(),
            target: Target::ngspice(name),
            plan,
            deck: deck(name),
            tier: Tier::T0,
            oversample: 8,
            budgets: Vec::new(),
            model: None,
            deck_name: name.to_string(),
            catalogue: None,
        }
    }

    /// The catalogued model this case pins (its `stave_dsp` path), for
    /// the fingerprint and the model catalogue's plots.
    pub fn catalogue(self, path: &str) -> Self {
        Self {
            catalogue: Some(path.into()),
            ..self
        }
    }

    /// Hold the model to another reference in the table format: a pinned
    /// fingerprint, a measured target, a capture.
    pub fn against(self, target: Target) -> Self {
        Self { target, ..self }
    }

    /// Rename the case (several models on one plan).
    pub fn named(self, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..self
        }
    }

    pub fn tier(self, tier: Tier) -> Self {
        Self { tier, ..self }
    }

    /// The solver's rate: `oversample` × 48 kHz (8 unless set).
    pub fn oversampled(self, oversample: u32) -> Self {
        Self { oversample, ..self }
    }

    /// The budget for `check` at the oversampled rate.
    pub fn budget(mut self, check: Check, budget: Budget) -> Self {
        self.budgets.push((check, Rates::Oversampled, budget));
        self
    }

    /// The budget for `check` at the shipping rates.
    pub fn shipping(mut self, check: Check, budget: Budget) -> Self {
        self.budgets.push((check, Rates::Shipping, budget));
        self
    }

    /// `Circuit<Dk>` on the deck.
    pub fn circuit(self) -> Self {
        let netlist = self.deck.clone();
        self.netlist(netlist)
    }

    /// `Circuit<Dk>` on a netlist built from the deck (stave's devices in
    /// place of its behavioural ones).
    pub fn netlist(self, netlist: Netlist) -> Self {
        Self {
            model: Some(Model::Circuit(netlist)),
            ..self
        }
    }

    /// `Circuit<Wdf>` on the deck: the wave digital method.
    pub fn wdf(self) -> Self {
        let netlist = self.deck.clone();
        self.wdf_netlist(netlist)
    }

    /// `Circuit<Wdf>` on a netlist built from the deck.
    pub fn wdf_netlist(self, netlist: Netlist) -> Self {
        Self {
            model: Some(Model::Wdf(netlist)),
            ..self
        }
    }

    /// A hand topology: `make(rate)` returns a ticker that takes the
    /// stimulus at `i / rate` and returns the output then.
    pub fn stage<T: FnMut(f64) -> f64 + 'static>(
        self,
        make: impl Fn(f64) -> T + 'static,
    ) -> Self {
        Self {
            model: Some(Model::Stage {
                make: Box::new(move |rate| Box::new(make(rate))),
                operating_point: Vec::new(),
            }),
            ..self
        }
    }

    /// A node voltage the hand topology reports at rest (C1).
    pub fn operating_point(mut self, node: &str, volts: f64) -> Self {
        if let Some(Model::Stage {
            operating_point, ..
        }) = &mut self.model
        {
            operating_point.push((node.to_ascii_lowercase(), volts));
        }
        self
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Run the model through the plan at every rate and compare.
    pub fn run(self) -> Characterisation {
        let reference = match self.target.load() {
            Ok(t) => t,
            Err(skip) => {
                return Characterisation {
                    name: self.name,
                    against: String::new(),
                    tier: self.tier,
                    rows: Vec::new(),
                    tables: Vec::new(),
                    skipped: Some(skip),
                    capped: 0,
                    seconds: 0.0,
                    plan: self.plan,
                    reference: None,
                    model: String::new(),
                    catalogue: self.catalogue,
                }
            }
        };
        let model = self
            .model
            .as_ref()
            .expect("no model: call .circuit(), .netlist(), .wdf() or .stage()");
        let nodes: Vec<String> = reference.op.iter().map(|(n, _)| n.clone()).collect();
        let shipping: &[f64] = match self.tier {
            Tier::T0 => &[48_000.0],
            Tier::T1 => &SHIPPING_RATES,
        };
        let mut rates = vec![(48_000.0 * f64::from(self.oversample), Rates::Oversampled)];
        rates.extend(shipping.iter().map(|&r| (r, Rates::Shipping)));
        let mut rows = Vec::new();
        let mut tables = Vec::new();
        let mut capped = 0;
        let started = std::time::Instant::now();
        for (rate, class) in rates {
            // A memoryless circuit runs "oversampled" at a shipping rate:
            // measure once.
            let cached = tables
                .iter()
                .find(|(r, _): &&(f64, Table)| *r == rate)
                .map(|(_, t)| t.clone());
            let table = match cached {
                Some(t) => t,
                None => {
                    let (t, health) = model.measure(
                        &self.plan,
                        &self.deck,
                        self.tier,
                        rate,
                        class == Rates::Oversampled,
                        &nodes,
                    );
                    capped += health.capped;
                    t
                }
            };
            for check in Check::ALL {
                let Some(m) = compare::compare(check, &self.plan, self.tier, &reference, &table)
                else {
                    continue;
                };
                let budget = self
                    .budgets
                    .iter()
                    .find(|(c, r, _)| *c == check && *r == class)
                    .map(|b| b.2);
                rows.push(Row {
                    check,
                    rate,
                    class,
                    measurement: m,
                    budget,
                });
            }
            if !tables.iter().any(|(r, _)| *r == rate) {
                tables.push((rate, table));
            }
        }
        let what = match model {
            Model::Circuit(_) => "Circuit<Dk>",
            Model::Wdf(_) => "Circuit<Wdf>",
            Model::Stage { .. } => "a hand topology",
        };
        Characterisation {
            name: self.name,
            against: format!("{} ({})", reference.source, self.target.regenerate()),
            tier: self.tier,
            rows,
            tables,
            skipped: None,
            capped,
            seconds: started.elapsed().as_secs_f64(),
            model: format!("{what} on spice/{}.cir", self.deck_name),
            reference: Some((format!("{} ({})", reference.source, self.target.regenerate()), reference)),
            plan: self.plan,
            catalogue: self.catalogue,
        }
    }
}

/// The reference program material for C13 (DSP.md R22): the library
/// `STAVE_SAMPLE_LIBRARY` names, or why a test that needs it skips. The
/// audio is never committed; a manifest in the repo names its files.
pub fn sample_library() -> Result<std::path::PathBuf, String> {
    let root = std::env::var_os("STAVE_SAMPLE_LIBRARY")
        .ok_or("skipped: library absent (STAVE_SAMPLE_LIBRARY is not set)")?;
    let root = std::path::PathBuf::from(root);
    if root.is_dir() {
        Ok(root)
    } else {
        Err(format!("skipped: library absent ({} is not a directory)", root.display()))
    }
}

/// `spice/<name>.cir`, parsed.
pub fn deck(name: &str) -> Netlist {
    let path = fixtures().join(format!("spice/{name}.cir"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    Netlist::parse_spice(&text).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()))
}

/// One check at one rate, against its budget.
#[derive(Clone, Debug)]
pub struct Row {
    pub check: Check,
    pub rate: f64,
    pub class: Rates,
    pub measurement: Measurement,
    pub budget: Option<Budget>,
}

impl Row {
    /// Over budget: the first error (or for aliasing, the level) above
    /// its budget, or the second above its own.
    pub fn failed(&self) -> bool {
        let Some(b) = self.budget else { return false };
        let over = |e: f64, b: f64| e.is_nan() || e > b;
        over(self.measurement.first.error, b.first)
            || b.second
                .is_some_and(|s| self.check.second_unit().is_some() && over(self.measurement.second.error, s))
    }
}

/// The matrix: every check at every rate.
#[derive(Clone, Debug)]
#[must_use = "a characterisation fails nothing until asserted"]
pub struct Characterisation {
    pub name: String,
    pub against: String,
    pub tier: Tier,
    pub rows: Vec<Row>,
    /// The model's own table at each rate (its fingerprint).
    pub tables: Vec<(f64, Table)>,
    /// Why nothing ran (an absent capture).
    pub skipped: Option<String>,
    /// Newton solves that hit their cap, over every run.
    pub capped: u64,
    /// Wall-clock seconds the model took over every rate.
    pub seconds: f64,
    pub plan: Plan,
    /// The reference the rows were compared with, and its description.
    pub reference: Option<(String, Table)>,
    /// What ran, for the fingerprint.
    pub model: String,
    pub catalogue: Option<String>,
}

impl Characterisation {
    pub fn passed(&self) -> bool {
        self.capped == 0 && self.rows.iter().all(|r| !r.failed())
    }

    /// The row for `check` at `rate`.
    pub fn row(&self, check: Check, rate: f64) -> Option<&Row> {
        self.rows.iter().find(|r| r.check == check && r.rate == rate)
    }

    /// The worst first error of `check` over a class of rates.
    pub fn worst(&self, check: Check, class: Rates) -> Option<f64> {
        self.rows
            .iter()
            .filter(|r| r.check == check && r.class == class)
            .map(|r| r.measurement.first.error)
            .reduce(f64::max)
    }

    /// The model's T0 tables as its fingerprint (DSP.md R18): the first
    /// table is the oversampled rate's (both, when the model ran at one
    /// rate), the rest the shipping rates'. Metrics and plots are not
    /// computed here ([`Fingerprint::pin`] does when blessing).
    pub fn fingerprint(&self) -> Fingerprint {
        let tables = self
            .tables
            .iter()
            .enumerate()
            .map(|(i, (rate, t))| {
                let role = match (i, self.tables.len()) {
                    (_, 1) => fingerprint::Role::Both,
                    (0, _) => fingerprint::Role::Oversampled,
                    _ => fingerprint::Role::Shipping,
                };
                (*rate, role, t.clone())
            })
            .collect();
        Fingerprint::new(&self.name, &self.model, self.catalogue.as_deref(), tables, self.reference.as_ref().map(|r| r.0.as_str()), None, None)
    }

    /// Hold the T0 matrix to the committed fingerprint (re-bless with
    /// `STAVE_BLESS=1`): panics naming every characteristic that moved.
    /// Nothing to pin for a skipped case or another tier.
    #[track_caller]
    pub fn pinned(self) -> Self {
        if self.skipped.is_none() && self.tier == Tier::T0 {
            self.fingerprint().pin(&self.plan, self.reference.as_ref().map(|r| &r.1));
        }
        self
    }

    /// Print the matrix and panic if any check is over its budget or any
    /// solve hit its cap.
    #[track_caller]
    pub fn assert(self) {
        println!("{self}");
        assert!(self.passed(), "{self}");
    }
}

fn number(x: f64, unit: &str) -> String {
    if unit == "dBc" || unit == "dB NMR" {
        format!("{x:.1} {unit}")
    } else {
        format!("{x:.2e} {unit}")
    }
}

impl fmt::Display for Characterisation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(skip) = &self.skipped {
            return write!(f, "characterise `{}`: {skip}", self.name);
        }
        let verdict = if self.passed() { "ok" } else { "FAILED" };
        writeln!(
            f,
            "characterise `{}` {:?} {verdict} in {:.2} s: against {}",
            self.name, self.tier, self.seconds, self.against
        )?;
        if self.capped > 0 {
            writeln!(f, "  {} Newton solves hit their cap", self.capped)?;
        }
        for r in &self.rows {
            let m = &r.measurement;
            let unit = r.check.unit();
            let mut error = number(m.first.error, unit);
            let mut budget = r.budget.map_or("—".into(), |b| number(b.first, unit));
            if let Some(second) = r.check.second_unit() {
                if !m.second.at.is_empty() || r.budget.and_then(|b| b.second).is_some() {
                    error.push_str(&format!(" / {}", number(m.second.error, second)));
                    if let Some(s) = r.budget.and_then(|b| b.second) {
                        budget.push_str(&format!(" / {}", number(s, second)));
                    }
                }
            }
            let mark = if r.failed() {
                "OVER"
            } else if r.budget.is_none() {
                "    "
            } else {
                "ok  "
            };
            writeln!(
                f,
                "  {mark} {:<22} {:>7.1} kHz {:<4} {:<26} budget {:<24} worst {}{}",
                r.check.name(),
                r.rate / 1e3,
                if r.class == Rates::Oversampled { "os" } else { "host" },
                error,
                budget,
                m.first.at,
                if m.second.at.is_empty() {
                    String::new()
                } else {
                    format!("; {}", m.second.at)
                }
            )?;
        }
        Ok(())
    }
}
