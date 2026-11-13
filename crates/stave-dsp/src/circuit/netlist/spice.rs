//! The SPICE subset stave reads — what `stave-test/spice/*.cir` uses, so
//! one deck feeds ngspice and stave:
//!
//! - the first line is the title; `*` starts a comment line, `;` an
//!   inline comment, `+` continues the previous line; case is ignored;
//! - `R`, `C` and `L` (with `IC=`), `V` and `I` (`DC`, `AC`, `SIN(…)`,
//!   `PWL(…)`), `E` (VCVS), `F` (CCCS), `D` with a `.model … D(…)` card
//!   (`IS`, `N`, `RS`; capacitance, transit time and breakdown are
//!   accepted and listed in [`Netlist::unmodelled`]), and `B` sources
//!   (`V = {…}` or `I = {…}`, see [`Expr`]);
//! - `.param`, `.model`, `.tran … [UIC]`, `.ac dec …`, and a `.control`
//!   block's `wrdata FILE v(a[, b])` (or `vm(…)`) as the probe;
//!   `.options`, `.op`, `.end` and other control lines are skipped.
//!
//! Values take SPICE's scale suffixes (`f p n u m k meg g t`, `mil`);
//! letters after them are units and ignored (`10uF`, `1kohm`).

use super::expr::{Expr, Scope};
use super::{Ac, ElementKind, Netlist, NetlistError, Node, Probe, Signal, Source, Tran};
use crate::circuit::devices::diode::{Shockley, THERMAL_VOLTS_27C};

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

struct Model {
    diode: Shockley,
    series_ohms: f64,
}

impl Netlist {
    /// Read a SPICE deck in the subset this module documents.
    pub fn parse_spice(text: &str) -> Result<Self, NetlistError> {
        let (title, deck, control) = lines(text)?;
        let mut netlist = Netlist::new();
        netlist.title = title;
        let mut params: Vec<(String, f64)> = Vec::new();
        let mut models: Vec<(String, Model)> = Vec::new();
        // Parameters and models first: SPICE allows use before definition.
        for line in &deck {
            match line.tokens[0].word() {
                Some(".param") => parse_params(line, &mut params)?,
                Some(".model") => {
                    if let Some(model) = parse_model(line, &mut netlist)? {
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

fn parse_model(line: &Line, netlist: &mut Netlist) -> Result<Option<(String, Model)>, NetlistError> {
    let t = &line.tokens;
    let (Some(name), Some(kind)) = (t.get(1).and_then(Token::word), t.get(2).and_then(Token::word))
    else {
        return Err(syntax(line, "expected .model NAME TYPE(...)"));
    };
    if kind != "d" {
        return Ok(None);
    }
    let mut model = Model {
        diode: Shockley {
            saturation_amps: 1e-14,
            emission: 1.0,
            thermal_volts: THERMAL_VOLTS_27C,
        },
        series_ohms: 0.0,
    };
    let mut i = 3;
    while i < t.len() {
        let (Some(key), Some(Token::Equals)) = (t[i].word(), t.get(i + 1)) else {
            return Err(syntax(line, "expected PARAM=value in a .model card"));
        };
        let x = number(line, t.get(i + 2), &[])?;
        match key {
            "is" => model.diode.saturation_amps = x,
            "n" => model.diode.emission = x,
            "rs" => model.series_ohms = x,
            _ => netlist.unmodelled.push(format!("{name}: {key}")),
        }
        i += 3;
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
            let model_name = t
                .get(3)
                .and_then(Token::word)
                .ok_or_else(|| syntax(line, "expected a model name"))?;
            let model = models
                .iter()
                .find(|(n, _)| n == model_name)
                .map(|(_, m)| m)
                .ok_or_else(|| NetlistError::UnknownModel {
                    line: line.number,
                    model: model_name.to_string(),
                })?;
            ElementKind::Diode {
                anode,
                cathode,
                diode: model.diode,
                series_ohms: model.series_ohms,
            }
        }
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
                &format!("element `{name}`: its type is not in the subset (R C L V I E F D B)"),
            ))
        }
    };
    netlist.add(name, kind)
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
    let minus = if t.len() > 4 { find(t.get(4))? } else { Node::GROUND };
    Ok(Probe { plus, minus })
}
