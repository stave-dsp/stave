//! The SPICE subset stave reads — what `stave-test/spice/*.cir` uses, so
//! one deck feeds ngspice and stave:
//!
//! - the first line is the title; `*` starts a comment line, `;` an
//!   inline comment, `+` continues the previous line; case is ignored;
//! - `R`, `C` and `L` (with `IC=`), `V` and `I` (`DC`, `AC`, `SIN(…)`,
//!   `PWL(…)`), `E` (VCVS), `F` (CCCS), `G` (VCCS), `D` with a
//!   `.model … D(…)` card (`IS`, `N`, `RS`, breakdown `BV`, `IBV`,
//!   `NBV` with ngspice's defaults, and charge storage `CJO`, `VJ`, `M`,
//!   `FC`, `TT`), `B` sources (`V = {…}` or `I = {…}`, see [`Expr`]),
//!   and transistors at level 1: `Q` with an `NPN`/`PNP` card
//!   (Gummel–Poon, its charges `CJE`/`VJE`/`MJE`, `CJC`/`VJC`/`MJC`/
//!   `XCJC`, `CJS`/`VJS`/`MJS`/`SUBS`, `FC`, `TF`/`XTF`/`VTF`/`ITF`, `TR`
//!   included), `J` with `NJF`/`PJF` (Shichman–Hodges, with `CGS`, `CGD`,
//!   `PB`, `FC`), `M` with `NMOS`/`PMOS` (the square law, `W`/`L`/`M`
//!   on the element, with the overlaps `CGSO`, `CGDO`, `CGBO` and the
//!   bulk junctions `CBD`, `CBS` or `CJ` over the element's `AD`/`AS`,
//!   `PB`, `MJ`, `FC`), each with an optional area factor where SPICE
//!   has one; temperature coefficients (`XTI`, `EG`, `XTB` …) are inert
//!   at the card's own temperature and read as such; anything else
//!   (noise, Meyer's `TOX` capacitance, sidewalls, excess phase) is
//!   accepted and listed in [`Netlist::unmodelled`];
//! - `.temp` or `.options temp= tnom=` (equal: model cards are read at
//!   the circuit's temperature, with no temperature scaling), and
//!   `.options gmin=` (the junction conductance, [`Netlist::gmin_siemens`]);
//! - `.param`, `.model`, `.tran … [UIC]`, `.ac dec …`, and a `.control`
//!   block's `wrdata FILE v(a[, b])` (or `vm(…)`) as the probe;
//!   `.options`, `.op`, `.end` and other control lines are skipped.
//!   A `wrdata` of several vectors (`vm(out) vp(out)`) probes the first.
//!
//! Values take SPICE's scale suffixes (`f p n u m k meg g t`, `mil`);
//! letters after them are units and ignored (`10uF`, `1kohm`).

use super::expr::{Expr, Scope};
use super::{Ac, ElementKind, Netlist, NetlistError, Node, Probe, Signal, Source, Tran};
use crate::circuit::devices::bjt::{GummelPoon, Substrate};
use crate::circuit::devices::capacitance::Depletion;
use crate::circuit::devices::diode::{Breakdown, Shockley, THERMAL_VOLTS_27C};
use crate::circuit::devices::{jfet, mosfet, thermal_volts, Polarity};

/// A SPICE number: `2.2k`, `1meg`, `20.8333333u`, `1e-3`, `10uF`.
pub fn value(word: &str) -> Result<f64, String> {
    let lower = word.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    if i < b.len() && b[i] == b'e' {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            i = j;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    let number: f64 = lower[..i]
        .parse()
        .map_err(|_| format!("not a number: {word:?}"))?;
    let suffix = &lower[i..];
    let scale = if suffix.starts_with("meg") {
        1e6
    } else if suffix.starts_with("mil") {
        25.4e-6
    } else {
        match suffix.as_bytes().first() {
            Some(b't') => 1e12,
            Some(b'g') => 1e9,
            Some(b'k') => 1e3,
            Some(b'm') => 1e-3,
            Some(b'u') => 1e-6,
            Some(b'n') => 1e-9,
            Some(b'p') => 1e-12,
            Some(b'f') => 1e-15,
            Some(c) if c.is_ascii_alphabetic() => 1.0,
            None => 1.0,
            Some(_) => return Err(format!("not a number: {word:?}")),
        }
    };
    Ok(number * scale)
}

/// One logical line: its first physical line number and its tokens.
struct Line {
    number: usize,
    tokens: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    /// A `{…}` group, braces removed.
    Braced(String),
    Equals,
}

impl Token {
    fn word(&self) -> Option<&str> {
        match self {
            Token::Word(w) => Some(w),
            _ => None,
        }
    }
}

fn tokenize(text: &str, line: usize) -> Result<Vec<Token>, NetlistError> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars();
    let flush = |word: &mut String, out: &mut Vec<Token>| {
        if !word.is_empty() {
            out.push(Token::Word(std::mem::take(word).to_ascii_lowercase()));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                flush(&mut word, &mut out);
                let mut depth = 1;
                let mut inner = String::new();
                for c in chars.by_ref() {
                    match c {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        break;
                    }
                    inner.push(c);
                }
                if depth != 0 {
                    return Err(NetlistError::Syntax {
                        line,
                        message: "unclosed `{`".into(),
                    });
                }
                out.push(Token::Braced(inner));
            }
            '=' => {
                flush(&mut word, &mut out);
                out.push(Token::Equals);
            }
            c if c.is_whitespace() || c == '(' || c == ')' || c == ',' => {
                flush(&mut word, &mut out);
            }
            c => word.push(c),
        }
    }
    flush(&mut word, &mut out);
    Ok(out)
}

/// Logical lines (continuations joined, comments dropped), the title,
/// and the `.control` block's lines kept apart.
fn lines(text: &str) -> Result<(String, Vec<Line>, Vec<Line>), NetlistError> {
    let mut physical = text.lines().enumerate();
    let title = physical
        .next()
        .map(|(_, l)| l.trim().trim_start_matches('*').trim().to_string())
        .unwrap_or_default();
    let mut joined: Vec<(usize, String)> = Vec::new();
    for (i, raw) in physical {
        let raw = raw.trim();
        let text = match raw.find(';') {
            Some(at) => &raw[..at],
            None => raw,
        };
        if text.is_empty() || text.starts_with('*') {
            continue;
        }
        if let Some(rest) = text.strip_prefix('+') {
            match joined.last_mut() {
                Some((_, last)) => {
                    last.push(' ');
                    last.push_str(rest);
                }
                None => {
                    return Err(NetlistError::Syntax {
                        line: i + 1,
                        message: "a continuation with nothing to continue".into(),
                    })
                }
            }
            continue;
        }
        joined.push((i + 1, text.to_string()));
    }
    let (mut deck, mut control, mut in_control) = (Vec::new(), Vec::new(), false);
    for (number, text) in joined {
        let tokens = tokenize(&text, number)?;
        let first = tokens.first().and_then(Token::word).unwrap_or("");
        match first {
            ".control" => in_control = true,
            ".endc" => in_control = false,
            _ if in_control => control.push(Line { number, tokens }),
            _ => deck.push(Line { number, tokens }),
        }
    }
    Ok((title, deck, control))
}

/// A `.model` card, read at the netlist's temperature.
enum Model {
    Diode(Shockley),
    Bjt(GummelPoon),
    Jfet(jfet::ShichmanHodges),
    /// `beta` holds `KP` until an element's `W/L` scales it, and the
    /// overlaps their per-metre factors until its `W` and `L` do.
    Mosfet { mosfet: mosfet::ShichmanHodges, lateral_diffusion: f64, bulk: BulkCard },
    /// A card this subset reads but cannot solve, and why: an error only
    /// if an element uses it.
    Unsupported(String),
}

impl Netlist {
    /// Read a SPICE deck in the subset this module documents.
    pub fn parse_spice(text: &str) -> Result<Self, NetlistError> {
        let (title, deck, control) = lines(text)?;
        let mut netlist = Netlist::new();
        netlist.title = title;
        let mut params: Vec<(String, f64)> = Vec::new();
        let mut models: Vec<(String, Model)> = Vec::new();
        // The temperature, then parameters and models: SPICE allows use
        // before definition, and cards are read at the temperature.
        let temp = temperature(&deck)?;
        netlist.temperature_celsius = temp;
        netlist.gmin_siemens = option(&deck, "gmin")?;
        let thermal = temp.map_or(THERMAL_VOLTS_27C, thermal_volts);
        for line in &deck {
            match line.tokens[0].word() {
                Some(".param") => parse_params(line, &mut params)?,
                Some(".model") => {
                    if let Some(model) = parse_model(line, thermal, &mut netlist)? {
                        models.push(model);
                    }
                }
                _ => {}
            }
        }
        for line in &deck {
            let Some(first) = line.tokens[0].word() else {
                return Err(syntax(line, "a line must start with a name"));
            };
            if first.starts_with('.') {
                parse_directive(line, first, &mut netlist)?;
            } else {
                parse_element(line, first, &params, &models, &mut netlist)?;
            }
        }
        for line in &control {
            if line.tokens[0].word() == Some("wrdata") {
                netlist.probe = Some(parse_probe(line, &netlist)?);
            }
        }
        Ok(netlist)
    }
}

fn syntax(line: &Line, message: &str) -> NetlistError {
    NetlistError::Syntax {
        line: line.number,
        message: message.into(),
    }
}

fn number(line: &Line, token: Option<&Token>, params: &[(String, f64)]) -> Result<f64, NetlistError> {
    match token {
        Some(Token::Word(w)) => value(w).map_err(|m| syntax(line, &m)),
        Some(Token::Braced(text)) => {
            let mut no_nodes = |_: &str| Node::GROUND;
            let mut scope = Scope {
                params,
                node: &mut no_nodes,
            };
            match Expr::parse(text, &mut scope, line.number)? {
                Expr::Number(x) => Ok(x),
                _ => Err(syntax(line, "a value must be constant")),
            }
        }
        _ => Err(syntax(line, "expected a value")),
    }
}

fn node(line: &Line, token: Option<&Token>, netlist: &mut Netlist) -> Result<Node, NetlistError> {
    match token.and_then(Token::word) {
        Some(name) => Ok(netlist.add_node(name)),
        None => Err(syntax(line, "expected a node")),
    }
}

fn parse_params(line: &Line, params: &mut Vec<(String, f64)>) -> Result<(), NetlistError> {
    let t = &line.tokens[1..];
    let mut i = 0;
    while i < t.len() {
        let (Some(name), Some(Token::Equals)) = (t[i].word(), t.get(i + 1)) else {
            return Err(syntax(line, "expected name=value"));
        };
        let x = number(line, t.get(i + 2), params)?;
        params.push((name.to_string(), x));
        i += 3;
    }
    Ok(())
}

/// The circuit temperature from `.temp` or `.options temp=`, which must
/// equal `tnom` (cards are read as written: no temperature scaling).
fn temperature(deck: &[Line]) -> Result<Option<f64>, NetlistError> {
    let (mut temp, mut tnom) = (None, None);
    for line in deck {
        let t = &line.tokens;
        match t[0].word() {
            Some(".temp") => temp = Some(number(line, t.get(1), &[])?),
            Some(".options" | ".option" | ".opt") => {
                for i in 1..t.len() {
                    if let (Some(key @ ("temp" | "tnom")), Some(Token::Equals)) =
                        (t[i].word(), t.get(i + 1))
                    {
                        let x = Some(number(line, t.get(i + 2), &[])?);
                        if key == "temp" {
                            temp = x;
                        } else {
                            tnom = x;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if temp.unwrap_or(27.0) != tnom.unwrap_or(27.0) {
        return Err(NetlistError::Unsupported {
            element: ".options".into(),
            reason: "temp and tnom differ: temperature scaling of model cards is not modelled".into(),
        });
    }
    Ok(temp)
}

/// The last `.options key=value` the deck sets.
fn option(deck: &[Line], key: &str) -> Result<Option<f64>, NetlistError> {
    let mut found = None;
    for line in deck {
        let t = &line.tokens;
        if let Some(".options" | ".option" | ".opt") = t[0].word() {
            for i in 1..t.len() {
                if let (Some(k), Some(Token::Equals)) = (t[i].word(), t.get(i + 1)) {
                    if k == key {
                        found = Some(number(line, t.get(i + 2), &[])?);
                    }
                }
            }
        }
    }
    Ok(found)
}

/// A card's `KEY=value` pairs.
fn model_parameters(line: &Line) -> Result<Vec<(String, f64)>, NetlistError> {
    let t = &line.tokens;
    let mut out = Vec::new();
    let mut i = 3;
    while i < t.len() {
        let (Some(key), Some(Token::Equals)) = (t[i].word(), t.get(i + 1)) else {
            return Err(syntax(line, "expected PARAM=value in a .model card"));
        };
        out.push((key.to_string(), number(line, t.get(i + 2), &[])?));
        i += 3;
    }
    Ok(out)
}

/// The parts of a MOSFET card an element's geometry scales: the bulk
/// junctions per area when `CBD`/`CBS` are absent.
#[derive(Clone, Copy, Debug, Default)]
struct BulkCard {
    /// `CBD`, `CBS` (F) as given.
    drain: Option<f64>,
    source: Option<f64>,
    /// `CJ` (F/m²): each junction's when its own is not given.
    per_area: Option<f64>,
}

impl BulkCard {
    /// A junction's zero-bias capacitance (`mos1temp.c`): its own value,
    /// else `CJ` over its area, else none; `M` devices in parallel.
    fn farads(given: Option<f64>, per_area: Option<f64>, area: f64, multiplier: f64) -> f64 {
        multiplier * given.unwrap_or_else(|| per_area.map_or(0.0, |cj| cj * area))
    }
}

/// Temperature parameters: a card's own temperature is the only one
/// modelled (`temperature` refuses any other), where these change
/// nothing.
fn inert_at_tnom(key: &str) -> bool {
    matches!(
        key,
        "xti" | "eg" | "xtb" | "tnom" | "tlev" | "tlevc" | "trs" | "trs1" | "trs2" | "tbv" | "tbv1" | "tbv2"
            | "vtotc" | "betatce" | "tcv" | "bex"
    )
}

/// SPICE's "zero means none" for Early voltages and knee currents.
fn infinite_at_zero(x: f64) -> f64 {
    if x == 0.0 {
        f64::INFINITY
    } else {
        x
    }
}

fn parse_model(
    line: &Line,
    thermal: f64,
    netlist: &mut Netlist,
) -> Result<Option<(String, Model)>, NetlistError> {
    let t = &line.tokens;
    let (Some(name), Some(kind)) = (t.get(1).and_then(Token::word), t.get(2).and_then(Token::word))
    else {
        return Err(syntax(line, "expected .model NAME TYPE(...)"));
    };
    if !matches!(kind, "d" | "npn" | "pnp" | "njf" | "pjf" | "nmos" | "pmos") {
        return Ok(None);
    }
    let parameters = model_parameters(line)?;
    let level = parameters
        .iter()
        .find(|(k, _)| k == "level")
        .map_or(1.0, |&(_, x)| x);
    let mut skipped = Vec::new();
    let model = match kind {
        "d" => {
            let mut diode = Shockley {
                thermal_volts: thermal,
                ..Shockley::DEFAULT
            };
            let (mut bv, mut ibv, mut nbv) = (None, 1e-3, None);
            for (key, x) in &parameters {
                let x = *x;
                let c = &mut diode.capacitance;
                match key.as_str() {
                    "is" => diode.saturation_amps = x,
                    "n" => diode.emission = x,
                    "rs" => diode.series_ohms = x,
                    "bv" => bv = Some(x),
                    "ibv" | "ib" => ibv = x,
                    "nbv" => nbv = Some(x),
                    "cjo" | "cj0" | "cj" => c.zero_bias_farads = x,
                    "vj" | "pb" => c.potential_volts = x,
                    "m" | "mj" => c.grading = x,
                    "fc" => c.forward_coefficient = x,
                    "tt" => diode.transit_seconds = x,
                    k if inert_at_tnom(k) => {}
                    _ => skipped.push(key),
                }
            }
            // diotemp.c: M is limited to 0.9 and VJ to 1/FC.
            let c = &mut diode.capacitance;
            c.grading = c.grading.min(0.9);
            if c.forward_coefficient * c.potential_volts > 1.0 {
                c.potential_volts = 1.0 / c.forward_coefficient;
            }
            // ngspice's defaults: IBV 1 mA, NBV the card's N.
            diode.breakdown = bv.map(|volts| Breakdown {
                volts,
                amps: ibv,
                emission: nbv.unwrap_or(diode.emission),
            });
            Model::Diode(diode)
        }
        _ if level != 1.0 => Model::Unsupported(format!(
            "level {level}: only level 1 (Gummel–Poon, Shichman–Hodges, the square law)"
        )),
        "npn" | "pnp" => {
            let polarity = if kind == "npn" { Polarity::N } else { Polarity::P };
            let mut q = GummelPoon {
                polarity,
                thermal_volts: thermal,
                substrate: Substrate::default_for(polarity),
                ..GummelPoon::DEFAULT_NPN
            };
            let mut fc = 0.5;
            for (key, x) in &parameters {
                let x = *x;
                let (je, jc, js) = (
                    &mut q.emitter_capacitance,
                    &mut q.collector_capacitance,
                    &mut q.substrate_capacitance,
                );
                let tf = &mut q.forward_transit;
                match key.as_str() {
                    "cje" => je.zero_bias_farads = x,
                    "vje" | "pe" => je.potential_volts = x,
                    "mje" | "me" => je.grading = x,
                    "cjc" => jc.zero_bias_farads = x,
                    "vjc" | "pc" => jc.potential_volts = x,
                    "mjc" | "mc" => jc.grading = x,
                    "xcjc" | "cdis" => q.base_collector_fraction = x,
                    "cjs" | "csub" | "ccs" => js.zero_bias_farads = x,
                    "vjs" | "ps" => js.potential_volts = x,
                    "mjs" | "ms" => js.grading = x,
                    "fc" => fc = x.min(0.9999),
                    "tf" => tf.seconds = x,
                    "xtf" => tf.bias_coefficient = x,
                    "vtf" => tf.vbc_volts = infinite_at_zero(x),
                    "itf" => tf.high_current_amps = x,
                    "tr" => q.reverse_transit_seconds = x,
                    "subs" => q.substrate = if x < 0.0 { Substrate::Lateral } else { Substrate::Vertical },
                    "ptf" if x == 0.0 => {}
                    k if inert_at_tnom(k) => {}
                    "level" => {}
                    "is" => q.saturation_amps = x,
                    "bf" => q.forward_beta = x,
                    "br" => q.reverse_beta = x,
                    "nf" => q.forward_emission = x,
                    "nr" => q.reverse_emission = x,
                    "vaf" | "va" => q.forward_early_volts = infinite_at_zero(x),
                    "var" | "vb" => q.reverse_early_volts = infinite_at_zero(x),
                    "ikf" | "ik" => q.forward_knee_amps = infinite_at_zero(x),
                    "ikr" => q.reverse_knee_amps = infinite_at_zero(x),
                    "ise" => q.emitter_leakage_amps = x,
                    "ne" => q.emitter_leakage_emission = x,
                    "isc" => q.collector_leakage_amps = x,
                    "nc" => q.collector_leakage_emission = x,
                    "rb" => q.base_ohms = x,
                    "re" => q.emitter_ohms = x,
                    "rc" => q.collector_ohms = x,
                    _ => skipped.push(key),
                }
            }
            q.emitter_capacitance.forward_coefficient = fc;
            q.collector_capacitance.forward_coefficient = fc;
            Model::Bjt(q)
        }
        "njf" | "pjf" => {
            let mut j = jfet::ShichmanHodges {
                polarity: if kind == "njf" { Polarity::N } else { Polarity::P },
                thermal_volts: thermal,
                ..jfet::ShichmanHodges::DEFAULT_N
            };
            let (mut cgs, mut cgd, mut pb, mut fc) = (0.0, 0.0, 1.0, 0.5);
            for (key, x) in &parameters {
                let x = *x;
                match key.as_str() {
                    "level" => {}
                    "cgs" => cgs = x,
                    "cgd" => cgd = x,
                    "pb" => pb = x,
                    // jfettemp.c limits FC to 0.95.
                    "fc" => fc = x.min(0.95),
                    k if inert_at_tnom(k) => {}
                    "vto" | "vt0" => j.threshold_volts = x,
                    "beta" => j.beta = x,
                    "lambda" => j.channel_modulation = x,
                    "is" => j.saturation_amps = x,
                    "n" => j.emission = x,
                    "rd" => j.drain_ohms = x,
                    "rs" => j.source_ohms = x,
                    _ => skipped.push(key),
                }
            }
            let law = |farads| Depletion {
                zero_bias_farads: farads,
                potential_volts: pb,
                grading: 0.5,
                forward_coefficient: fc,
            };
            j.gate_source_capacitance = law(cgs);
            j.gate_drain_capacitance = law(cgd);
            Model::Jfet(j)
        }
        _ => {
            let mut m = mosfet::ShichmanHodges {
                polarity: if kind == "nmos" { Polarity::N } else { Polarity::P },
                ..mosfet::ShichmanHodges::DEFAULT_N
            };
            let given = |k: &str| parameters.iter().any(|(p, _)| p == k);
            let derived = if given("tox") && !given("kp") {
                Some("KP from TOX and UO")
            } else if given("nsub") && !(given("vto") && given("gamma") && given("phi")) {
                Some("VTO, GAMMA or PHI from NSUB")
            } else {
                None
            };
            let mut lateral_diffusion = 0.0;
            let mut bulk = BulkCard::default();
            let mut junction = mosfet::shichman_hodges::NO_BULK_CAPACITANCE;
            for (key, x) in &parameters {
                let x = *x;
                match key.as_str() {
                    "level" => {}
                    // Per metre until the element's W and L scale them.
                    "cgso" => m.gate_source_overlap_farads = x,
                    "cgdo" => m.gate_drain_overlap_farads = x,
                    "cgbo" => m.gate_bulk_overlap_farads = x,
                    "cbd" => bulk.drain = Some(x),
                    "cbs" => bulk.source = Some(x),
                    "cj" => bulk.per_area = Some(x),
                    "pb" => junction.potential_volts = x,
                    "mj" => junction.grading = x,
                    "fc" => junction.forward_coefficient = x,
                    "tox" if x == 0.0 => {}
                    k if inert_at_tnom(k) => {}
                    "vto" | "vt0" => m.threshold_volts = x,
                    "kp" => m.beta = x,
                    "gamma" => m.body_effect = x,
                    "phi" => m.surface_potential = x,
                    "lambda" => m.channel_modulation = x,
                    "rd" => m.drain_ohms = x,
                    "rs" => m.source_ohms = x,
                    "ld" => lateral_diffusion = x,
                    _ => skipped.push(key),
                }
            }
            // The law; its size waits for the element (CBD/CBS, or CJ
            // over AD/AS).
            m.bulk_drain_capacitance = junction;
            m.bulk_source_capacitance = junction;
            match derived {
                Some(what) => Model::Unsupported(format!("derives {what}: give them in the card")),
                None => Model::Mosfet {
                    mosfet: m,
                    lateral_diffusion,
                    bulk,
                },
            }
        }
    };
    for key in skipped {
        netlist.unmodelled.push(format!("{name}: {key}"));
    }
    Ok(Some((name.to_string(), model)))
}

fn parse_directive(line: &Line, first: &str, netlist: &mut Netlist) -> Result<(), NetlistError> {
    let t = &line.tokens;
    match first {
        ".tran" => {
            let step = number(line, t.get(1), &[])?;
            let stop = number(line, t.get(2), &[])?;
            let uic = t.iter().any(|t| t.word() == Some("uic"));
            netlist.tran = Some(Tran {
                step_seconds: step,
                stop_seconds: stop,
                uic,
            });
        }
        ".ac" => {
            if t.get(1).and_then(Token::word) != Some("dec") {
                return Err(syntax(line, "only `.ac dec` sweeps are in the subset"));
            }
            netlist.ac = Some(Ac {
                points_per_decade: number(line, t.get(2), &[])? as u32,
                start_hz: number(line, t.get(3), &[])?,
                stop_hz: number(line, t.get(4), &[])?,
            });
        }
        ".subckt" | ".include" | ".lib" => {
            return Err(syntax(line, &format!("`{first}` is not in the subset")));
        }
        // .param and .model were read first; the rest configure ngspice.
        _ => {}
    }
    Ok(())
}

fn parse_source(line: &Line, t: &[Token], params: &[(String, f64)]) -> Result<Source, NetlistError> {
    let mut source = Source::default();
    let mut explicit_dc = false;
    let is_number = |t: &Token| match t {
        Token::Word(w) => value(w).is_ok(),
        Token::Braced(_) => true,
        Token::Equals => false,
    };
    let mut i = 0;
    while i < t.len() {
        match t[i].word() {
            Some("dc") => {
                source.dc = number(line, t.get(i + 1), params)?;
                explicit_dc = true;
                i += 2;
            }
            Some("ac") => {
                source.ac = number(line, t.get(i + 1), params)?;
                i += 2;
                if t.get(i).is_some_and(is_number) {
                    i += 1; // the phase: small-signal only
                }
            }
            Some(kind @ ("sin" | "pwl")) => {
                let mut args = Vec::new();
                i += 1;
                while i < t.len() && is_number(&t[i]) {
                    args.push(number(line, t.get(i), params)?);
                    i += 1;
                }
                source.signal = Some(if kind == "sin" {
                    if args.len() < 3 {
                        return Err(syntax(line, "SIN needs offset, amplitude and frequency"));
                    }
                    let at = |k: usize| args.get(k).copied().unwrap_or(0.0);
                    Signal::Sine {
                        offset: at(0),
                        amplitude: at(1),
                        hz: at(2),
                        delay: at(3),
                        damping: at(4),
                        phase_degrees: at(5),
                    }
                } else {
                    if args.len() < 2 || args.len() % 2 != 0 {
                        return Err(syntax(line, "PWL needs time-value pairs"));
                    }
                    Signal::Pwl(args.chunks(2).map(|p| (p[0], p[1])).collect())
                });
            }
            _ if i == 0 && is_number(&t[0]) => {
                source.dc = number(line, t.first(), params)?;
                explicit_dc = true;
                i += 1;
            }
            _ => return Err(syntax(line, "expected DC, AC, SIN or PWL")),
        }
    }
    if !explicit_dc {
        if let Some(signal) = &source.signal {
            source.dc = signal.value_at(0.0);
        }
    }
    Ok(source)
}

/// `IC=value` among an element's trailing tokens.
fn initial_condition(line: &Line, t: &[Token]) -> Result<Option<f64>, NetlistError> {
    match t {
        [] => Ok(None),
        [Token::Word(ic), Token::Equals, v] if ic == "ic" => number(line, Some(v), &[]).map(Some),
        _ => Err(syntax(line, "expected IC=value")),
    }
}

fn parse_element(
    line: &Line,
    name: &str,
    params: &[(String, f64)],
    models: &[(String, Model)],
    netlist: &mut Netlist,
) -> Result<(), NetlistError> {
    let t = &line.tokens;
    let kind = match name.as_bytes()[0] {
        b'r' => ElementKind::Resistor {
            a: node(line, t.get(1), netlist)?,
            b: node(line, t.get(2), netlist)?,
            ohms: number(line, t.get(3), params)?,
        },
        b'c' => ElementKind::Capacitor {
            a: node(line, t.get(1), netlist)?,
            b: node(line, t.get(2), netlist)?,
            farads: number(line, t.get(3), params)?,
            initial_volts: initial_condition(line, t.get(4..).unwrap_or(&[]))?,
        },
        b'l' => ElementKind::Inductor {
            a: node(line, t.get(1), netlist)?,
            b: node(line, t.get(2), netlist)?,
            henries: number(line, t.get(3), params)?,
            initial_amps: initial_condition(line, t.get(4..).unwrap_or(&[]))?,
        },
        b'v' => ElementKind::VoltageSource {
            a: node(line, t.get(1), netlist)?,
            b: node(line, t.get(2), netlist)?,
            source: parse_source(line, t.get(3..).unwrap_or(&[]), params)?,
        },
        b'i' => ElementKind::CurrentSource {
            a: node(line, t.get(1), netlist)?,
            b: node(line, t.get(2), netlist)?,
            source: parse_source(line, t.get(3..).unwrap_or(&[]), params)?,
        },
        b'e' => ElementKind::VoltageControlledVoltage {
            a: node(line, t.get(1), netlist)?,
            b: node(line, t.get(2), netlist)?,
            control: (node(line, t.get(3), netlist)?, node(line, t.get(4), netlist)?),
            gain: number(line, t.get(5), params)?,
        },
        b'g' => ElementKind::VoltageControlledCurrent {
            a: node(line, t.get(1), netlist)?,
            b: node(line, t.get(2), netlist)?,
            control: (node(line, t.get(3), netlist)?, node(line, t.get(4), netlist)?),
            gain: number(line, t.get(5), params)?,
        },
        b'f' => ElementKind::CurrentControlledCurrent {
            a: node(line, t.get(1), netlist)?,
            b: node(line, t.get(2), netlist)?,
            control: t
                .get(3)
                .and_then(Token::word)
                .ok_or_else(|| syntax(line, "expected the controlling source"))?
                .to_string(),
            gain: number(line, t.get(4), params)?,
        },
        b'd' => {
            let (anode, cathode) = (node(line, t.get(1), netlist)?, node(line, t.get(2), netlist)?);
            let Model::Diode(diode) = model(line, name, t.get(3), models)? else {
                return Err(wrong_model(line, "a D element", t.get(3)));
            };
            ElementKind::Diode {
                anode,
                cathode,
                diode: *diode,
            }
        }
        b'q' => parse_bjt(line, name, models, netlist)?,
        b'j' => {
            let nodes = [1, 2, 3].map(|i| t.get(i));
            let Model::Jfet(jfet) = model(line, name, t.get(4), models)? else {
                return Err(wrong_model(line, "a J element", t.get(4)));
            };
            let area = area(line, t.get(5..).unwrap_or(&[]))?;
            ElementKind::Jfet {
                drain: node(line, nodes[0], netlist)?,
                gate: node(line, nodes[1], netlist)?,
                source: node(line, nodes[2], netlist)?,
                jfet: jfet.with_area(area),
            }
        }
        b'm' => parse_mosfet(line, name, models, netlist)?,
        b'b' => {
            let (a, b) = (node(line, t.get(1), netlist)?, node(line, t.get(2), netlist)?);
            let (Some(which), Some(Token::Equals), Some(Token::Braced(text))) =
                (t.get(3).and_then(Token::word), t.get(4), t.get(5))
            else {
                return Err(syntax(line, "expected V = {expr} or I = {expr}"));
            };
            let which = which.to_string();
            let text = text.clone();
            let mut add = |name: &str| netlist.add_node(name);
            let mut scope = Scope {
                params,
                node: &mut add,
            };
            let expression = Expr::parse(&text, &mut scope, line.number)?;
            match which.as_str() {
                "v" => ElementKind::BehaviouralVoltage { a, b, expression },
                "i" => ElementKind::BehaviouralCurrent { a, b, expression },
                _ => return Err(syntax(line, "a B source is V = {…} or I = {…}")),
            }
        }
        _ => {
            return Err(syntax(
                line,
                &format!("element `{name}`: its type is not in the subset (R C L V I E F G D B Q J M)"),
            ))
        }
    };
    netlist.add(name, kind)
}

/// The card an element names.
fn model<'m>(
    line: &Line,
    element: &str,
    token: Option<&Token>,
    models: &'m [(String, Model)],
) -> Result<&'m Model, NetlistError> {
    let name = token
        .and_then(Token::word)
        .ok_or_else(|| syntax(line, "expected a model name"))?;
    match models.iter().find(|(n, _)| n == name).map(|(_, m)| m) {
        None => Err(NetlistError::UnknownModel {
            line: line.number,
            model: name.to_string(),
        }),
        Some(Model::Unsupported(reason)) => Err(NetlistError::Unsupported {
            element: element.to_string(),
            reason: format!(".model {name}: {reason}"),
        }),
        Some(m) => Ok(m),
    }
}

fn wrong_model(line: &Line, what: &str, token: Option<&Token>) -> NetlistError {
    let name = token.and_then(Token::word).unwrap_or("");
    syntax(line, &format!("{what} needs a model of its own type, not .model {name}"))
}

/// An optional area factor (`… model [area] [OFF]`), 1 when absent.
fn area(line: &Line, rest: &[Token]) -> Result<f64, NetlistError> {
    let mut area = 1.0;
    for (i, token) in rest.iter().enumerate() {
        match token {
            Token::Word(w) if w == "off" => {}
            Token::Word(w) if i == 0 && value(w).is_ok() => area = number(line, Some(token), &[])?,
            _ => return Err(syntax(line, "after the model: an area factor and OFF only")),
        }
    }
    if area > 0.0 {
        Ok(area)
    } else {
        Err(syntax(line, "the area factor must be positive"))
    }
}

/// `Qname collector base emitter [substrate] model [area] [OFF]`. The
/// substrate junction carries no DC current (`ISS` = 0), only its
/// capacitance: its node joins the netlist when the card has `CJS`, and
/// is ground (SPICE's default) when the element names none.
fn parse_bjt(
    line: &Line,
    name: &str,
    models: &[(String, Model)],
    netlist: &mut Netlist,
) -> Result<ElementKind, NetlistError> {
    let t = &line.tokens;
    let names_a_model = |i: usize| {
        t.get(i)
            .and_then(Token::word)
            .is_some_and(|w| models.iter().any(|(n, _)| n == w))
    };
    let at = if names_a_model(4) || !names_a_model(5) { 4 } else { 5 };
    let Model::Bjt(bjt) = model(line, name, t.get(at), models)? else {
        return Err(wrong_model(line, "a Q element", t.get(at)));
    };
    let area = area(line, t.get(at + 1..).unwrap_or(&[]))?;
    let (collector, base, emitter) = (
        node(line, t.get(1), netlist)?,
        node(line, t.get(2), netlist)?,
        node(line, t.get(3), netlist)?,
    );
    let substrate = if at == 5 && !bjt.substrate_capacitance.is_none() {
        node(line, t.get(4), netlist)?
    } else {
        Node::GROUND
    };
    Ok(ElementKind::Bjt {
        collector,
        base,
        emitter,
        substrate,
        bjt: bjt.with_area(area),
    })
}

/// `Mname drain gate source bulk model [L=…] [W=…] [M=…] [AD=…] [AS=…]`;
/// the default channel is SPICE's 100 µm × 100 µm. `AD`/`AS` scale the
/// card's `CJ`; the rest of the geometry (`PD`, `PS`, `NRD`, `NRS`) is
/// listed in `unmodelled`.
fn parse_mosfet(
    line: &Line,
    name: &str,
    models: &[(String, Model)],
    netlist: &mut Netlist,
) -> Result<ElementKind, NetlistError> {
    let t = &line.tokens;
    let Model::Mosfet {
        mosfet,
        lateral_diffusion,
        bulk,
    } = model(line, name, t.get(5), models)?
    else {
        return Err(wrong_model(line, "an M element", t.get(5)));
    };
    let (mut length, mut width, mut multiplier) = (100e-6, 100e-6, 1.0);
    let (mut drain_area, mut source_area) = (0.0, 0.0);
    let rest = t.get(6..).unwrap_or(&[]);
    let mut i = 0;
    while i < rest.len() {
        if rest[i].word() == Some("off") {
            i += 1;
            continue;
        }
        let (Some(key), Some(Token::Equals)) = (rest[i].word(), rest.get(i + 1)) else {
            return Err(syntax(line, "expected KEY=value after an M element's model"));
        };
        let x = number(line, rest.get(i + 2), &[])?;
        match key {
            "l" => length = x,
            "w" => width = x,
            "m" => multiplier = x,
            "ad" => drain_area = x,
            "as" => source_area = x,
            _ => netlist.unmodelled.push(format!("{name}: {key}")),
        }
        i += 3;
    }
    let effective = length - 2.0 * lateral_diffusion;
    if effective <= 0.0 || width <= 0.0 || multiplier <= 0.0 {
        return Err(syntax(line, "W, M and L − 2·LD must be positive"));
    }
    Ok(ElementKind::Mosfet {
        drain: node(line, t.get(1), netlist)?,
        gate: node(line, t.get(2), netlist)?,
        source: node(line, t.get(3), netlist)?,
        bulk: node(line, t.get(4), netlist)?,
        mosfet: mosfet::ShichmanHodges {
            beta: mosfet.beta * width / effective * multiplier,
            drain_ohms: mosfet.drain_ohms / multiplier,
            source_ohms: mosfet.source_ohms / multiplier,
            // mos1load.c: overlaps by W (and Leff for the bulk's), junctions
            // by M, CJ by the element's areas.
            gate_source_overlap_farads: mosfet.gate_source_overlap_farads * width * multiplier,
            gate_drain_overlap_farads: mosfet.gate_drain_overlap_farads * width * multiplier,
            gate_bulk_overlap_farads: mosfet.gate_bulk_overlap_farads * effective * multiplier,
            bulk_drain_capacitance: Depletion {
                zero_bias_farads: BulkCard::farads(bulk.drain, bulk.per_area, drain_area, multiplier),
                ..mosfet.bulk_drain_capacitance
            },
            bulk_source_capacitance: Depletion {
                zero_bias_farads: BulkCard::farads(bulk.source, bulk.per_area, source_area, multiplier),
                ..mosfet.bulk_source_capacitance
            },
            ..*mosfet
        },
    })
}

fn parse_probe(line: &Line, netlist: &Netlist) -> Result<Probe, NetlistError> {
    // wrdata FILE v a [b]
    let t = &line.tokens;
    match t.get(2).and_then(Token::word) {
        Some("v" | "vm") => {}
        _ => return Err(syntax(line, "wrdata: the subset reads one v(…) or vm(…)")),
    }
    let find = |token: Option<&Token>| -> Result<Node, NetlistError> {
        let name = token
            .and_then(Token::word)
            .ok_or_else(|| syntax(line, "wrdata: expected a node"))?;
        netlist
            .node(name)
            .ok_or_else(|| syntax(line, &format!("wrdata: no node {name}")))
    };
    let plus = find(t.get(3))?;
    // A second node, unless what follows is the next vector (`vm(out)
    // vp(out)`: the probe is the first; ngspice writes the rest as further
    // columns).
    let next_vector = |t: Option<&Token>| matches!(t.and_then(Token::word), Some("v" | "vm" | "vp" | "vdb"));
    let minus = if t.len() > 4 && !next_vector(t.get(4)) { find(t.get(4))? } else { Node::GROUND };
    Ok(Probe { plus, minus })
}
