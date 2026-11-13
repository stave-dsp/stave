//! KiCad netlist import (DSP.md G1): the schematic editor's own netlist
//! export (*File → Export → Netlist… → KiCad*, the `.net`
//! s-expression) read into a [`Netlist`], so a circuit drawn in KiCad
//! runs in stave.
//!
//! The file lists components (`comp`, with `ref`, `value` and fields —
//! KiCad 6 and later write `(property (name …) (value …))`, older files
//! `(fields (field (name …) "…"))`) and nets (`net`, each a name and
//! the `(node (ref …) (pin …))` pins on it). A component's simulation
//! model is read from the fields KiCad's simulator keeps on the symbol:
//!
//! - **`Sim.Device`** — `R`, `C`, `L`; `V`, `I` (`Sim.Type` `DC`, `SIN`,
//!   `PWL`, or `=` for a behavioural source); `E`, `G` (`gain=`); `D`;
//!   `NPN`, `PNP` (`Sim.Type` `GUMMELPOON` or none); `NJFET`, `PJFET`
//!   (`SHICHMANHODGES` or none); `NMOS`, `PMOS` (`MOS1` or none); a
//!   resistor of `Sim.Type` `POT` (`r=`, `pos=`) becomes two resistors
//!   declared one [`Variable::pot`](super::Variable::pot) (linear taper);
//!   `SPICE` (a raw element: `type=`, `model=`);
//! - **`Sim.Params`** — `key=value` pairs (a value with spaces in
//!   quotes): the principal value (`r=`, `c=`, `l=`, `dc=` …), a source's
//!   waveform (`dc ampl f td theta phase`, `pwl="t v …"`, `ac=`), or a
//!   device's model card (`is=2.52n n=1.75`); instance parameters
//!   (`area`, `m`, `w`, `l`, `ad`, `as`, `pd`, `ps`) go on the element;
//! - **`Sim.Pins`** — `symbol-pin=model-pin` pairs (`1=K 2=A`; model
//!   pins `+ -`, `A K`, `C B E`, `D G S`, `D G S B`, `r0 wiper r1`);
//!   without it the model's pins take the symbol's in pin-number order;
//! - **`Sim.Library`**, **`Sim.Name`** — a `.model` card from a SPICE
//!   library the caller supplies ([`Import::library`]); `Sim.Params`
//!   then overrides the card's parameters;
//! - **`Sim.Enable`** `0` (or KiCad 8's `exclude_from_sim`) leaves the
//!   part out.
//!
//! With no `Sim.*` field a two-pin part whose reference starts `R`, `C`
//! or `L` is inferred from its value, as KiCad infers it. KiCad 5's
//! fields are read too: `Spice_Primitive`, `Spice_Model` (else the
//! value), `Spice_Node_Sequence`, `Spice_Lib_File`,
//! `Spice_Netlist_Enabled`.
//!
//! Values are KiCad's notation: SI prefixes, case-sensitive (`M` is
//! mega, `m` milli; `a f p n u µ m k M G T P E`), RKM (`4k7`, `2R2`),
//! units after the prefix ignored (`10uF`, `1kΩ`). A library's cards
//! stay SPICE text (`1M` is milli there, `1meg` mega).
//!
//! Nets become nodes by name: a leading `/` (the root sheet) is dropped,
//! `GND` and `0` are ground, and the characters SPICE reads as
//! separators (`( ) , = { } ;` and spaces) become `_`, as KiCad's own
//! SPICE export writes them (`Net-(C1-Pad2)` is `net-_c1-pad2_`). An
//! element is named by its reference, prefixed with its SPICE letter
//! when the reference starts with another (`CR1`, a diode, is `dcr1`).
//!
//! The netlist carries no analyses: [`Import::directives`] appends SPICE
//! directives (`.tran`, `.options`, a `.control` block's `wrdata` as the
//! probe), as a schematic's text directives would. Anything the subset
//! cannot run is an error that names the part: a subcircuit, an XSPICE
//! model, a VBIC transistor, a source of a type stave does not run.
//!
//! ```
//! use stave_dsp::circuit::netlist::Netlist;
//!
//! let netlist = Netlist::parse_kicad(
//!     r#"(export (version "E")
//!       (components
//!         (comp (ref "R1") (value "1k"))
//!         (comp (ref "C1") (value "100n")))
//!       (nets
//!         (net (code "1") (name "/out") (node (ref "R1") (pin "2")) (node (ref "C1") (pin "1")))
//!         (net (code "2") (name "/in") (node (ref "R1") (pin "1")))
//!         (net (code "3") (name "GND") (node (ref "C1") (pin "2")))))"#,
//! )
//! .unwrap();
//! assert_eq!(netlist.elements().len(), 2);
//! assert!(netlist.node("out").is_some());
//! ```

use super::{Netlist, NetlistError, Variable};
use super::variable::Taper;

/// One s-expression: an atom (bare or quoted) or a list, with the line
/// it starts on.
#[derive(Clone, Debug, PartialEq)]
enum Sexp {
    Atom(String, usize),
    List(Vec<Sexp>, usize),
}

impl Sexp {
    fn line(&self) -> usize {
        match self {
            Sexp::Atom(_, l) | Sexp::List(_, l) => *l,
        }
    }

    fn atom(&self) -> Option<&str> {
        match self {
            Sexp::Atom(a, _) => Some(a),
            Sexp::List(..) => None,
        }
    }

    fn items(&self) -> &[Sexp] {
        match self {
            Sexp::List(items, _) => items,
            Sexp::Atom(..) => &[],
        }
    }

    /// A list's head atom: `comp` for `(comp …)`.
    fn head(&self) -> Option<&str> {
        self.items().first().and_then(Sexp::atom)
    }

    /// The child lists whose head is `name`.
    fn children<'a, 'n>(&'a self, name: &'n str) -> impl Iterator<Item = &'a Sexp> + use<'a, 'n> {
        self.items().iter().skip(1).filter(move |s| s.head() == Some(name))
    }

    fn child(&self, name: &str) -> Option<&Sexp> {
        self.children(name).next()
    }

    /// The first atom after the head: `"R1"` in `(ref "R1")`.
    fn value(&self) -> Option<&str> {
        self.items().get(1).and_then(Sexp::atom)
    }

    /// `(name (value))`'s value.
    fn field(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(Sexp::value)
    }
}

fn syntax(line: usize, message: impl Into<String>) -> NetlistError {
    NetlistError::Syntax {
        line,
        message: message.into(),
    }
}

/// Read one s-expression (the whole file).
fn parse_sexp(text: &str) -> Result<Sexp, NetlistError> {
    let mut stack: Vec<(Vec<Sexp>, usize)> = Vec::new();
    let mut top: Option<Sexp> = None;
    let mut line = 1;
    let mut chars = text.chars().peekable();
    let push = |stack: &mut Vec<(Vec<Sexp>, usize)>, top: &mut Option<Sexp>, s: Sexp| -> Result<(), NetlistError> {
        match stack.last_mut() {
            Some((items, _)) => items.push(s),
            None if top.is_none() => *top = Some(s),
            None => return Err(syntax(s.line(), "more than one expression at the top level")),
        }
        Ok(())
    };
    while let Some(c) = chars.next() {
        match c {
            '\n' => line += 1,
            c if c.is_whitespace() => {}
            '(' => stack.push((Vec::new(), line)),
            ')' => {
                let (items, at) = stack.pop().ok_or_else(|| syntax(line, "a `)` with nothing open"))?;
                push(&mut stack, &mut top, Sexp::List(items, at))?;
            }
            '"' => {
                let (at, mut s) = (line, String::new());
                loop {
                    match chars.next() {
                        None => return Err(syntax(at, "an unclosed string")),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(e) => s.push(e),
                            None => return Err(syntax(at, "an unclosed string")),
                        },
                        Some(c) => {
                            if c == '\n' {
                                line += 1;
                            }
                            s.push(c);
                        }
                    }
                }
                push(&mut stack, &mut top, Sexp::Atom(s, at))?;
            }
            c => {
                let mut s = String::from(c);
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '(' || n == ')' || n == '"' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                push(&mut stack, &mut top, Sexp::Atom(s, line))?;
            }
        }
    }
    if let Some((_, at)) = stack.last() {
        return Err(syntax(*at, "a `(` never closed"));
    }
    top.ok_or_else(|| syntax(1, "an empty file"))
}

/// A number in KiCad's value notation: `10k`, `4k7`, `2R2`, `100n`,
/// `1M` (mega), `3e-4k`, `10uF`. The scale is applied as the SPICE
/// reader applies it (the number, then times the prefix), so a value
/// reads to the same `f64` either way.
pub fn value(word: &str) -> Result<f64, String> {
    let fail = || format!("not a number: {word:?}");
    let w = word.trim();
    let scale_of = |c: char| -> Option<f64> {
        Some(match c {
            'a' => 1e-18,
            'f' => 1e-15,
            'p' => 1e-12,
            'n' => 1e-9,
            'u' | 'µ' | 'μ' => 1e-6,
            'm' => 1e-3,
            'k' | 'K' => 1e3,
            'M' => 1e6,
            'G' => 1e9,
            'T' => 1e12,
            'P' => 1e15,
            'E' => 1e18,
            _ => return None,
        })
    };
    let chars: Vec<char> = w.chars().collect();
    let mut i = 0;
    let mut number = String::new();
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        number.push(chars[i]);
        i += 1;
    }
    let digits = |i: &mut usize, out: &mut String| {
        while *i < chars.len() && (chars[*i].is_ascii_digit() || chars[*i] == '.') {
            out.push(chars[*i]);
            *i += 1;
        }
    };
    digits(&mut i, &mut number);
    if !number.chars().any(|c| c.is_ascii_digit()) {
        return Err(fail());
    }
    // An exponent: `e`, an optional sign, then digits.
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        if j < chars.len() && chars[j].is_ascii_digit() {
            number.extend(&chars[i..j]);
            i = j;
            while i < chars.len() && chars[i].is_ascii_digit() {
                number.push(chars[i]);
                i += 1;
            }
        }
    }
    let mut scale = 1.0;
    if i < chars.len() {
        let c = chars[i];
        let rkm = i + 1 < chars.len() && chars[i + 1].is_ascii_digit() && !number.contains(['.', 'e', 'E']);
        if let Some(s) = scale_of(c).or((c == 'R' || c == 'r').then_some(1.0)) {
            scale = s;
            i += 1;
            if rkm {
                // RKM: the letter is the decimal point (`4k7` is 4.7k).
                number.push('.');
                digits(&mut i, &mut number);
            }
        }
        // What follows is a unit (`F`, `Ω`, `ohm`, `Hz`), not a number.
        if chars[i..].iter().any(|c| c.is_ascii_digit() || *c == '.') {
            return Err(fail());
        }
    }
    let x: f64 = number.parse().map_err(|_| fail())?;
    Ok(if scale == 1.0 { x } else { x * scale })
}

/// A component as the netlist lists it.
struct Component {
    reference: String,
    value: String,
    /// Name → value; KiCad 6+'s properties over older fields.
    fields: Vec<(String, String)>,
    /// Symbol pin number → net name, from the nets.
    pins: Vec<(String, String)>,
    line: usize,
}

impl Component {
    fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.trim().is_empty())
    }

    fn refuse(&self, reason: impl Into<String>) -> NetlistError {
        NetlistError::Unsupported {
            element: self.reference.clone(),
            reason: reason.into(),
        }
    }

    /// Symbol pins in KiCad's order: by number, numerically where they
    /// are numbers.
    fn sorted_pins(&self) -> Vec<&(String, String)> {
        let mut pins: Vec<&(String, String)> = self.pins.iter().collect();
        pins.sort_by(|a, b| match (a.0.parse::<u64>(), b.0.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => a.0.cmp(&b.0),
        });
        pins
    }
}

/// A component's `Sim.Params`: `key=value` pairs and bare flags.
fn params(c: &Component, text: &str) -> Result<Vec<(String, String)>, NetlistError> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' {
            i += 1;
        }
        let key: String = chars[start..i].iter().collect::<String>().to_ascii_lowercase();
        if i >= chars.len() || chars[i] != '=' {
            out.push((key, "1".into()));
            continue;
        }
        i += 1;
        let mut v = String::new();
        if i < chars.len() && chars[i] == '"' {
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err(c.refuse(format!("Sim.Params: an unclosed quote after {key}="))),
                    Some('\\') if chars.get(i + 1) == Some(&'"') => {
                        v.push('"');
                        i += 2;
                    }
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        v.push(ch);
                        i += 1;
                    }
                }
            }
        } else {
            while i < chars.len() && !chars[i].is_whitespace() {
                v.push(chars[i]);
                i += 1;
            }
        }
        out.push((key.trim_end_matches('_').to_string(), v));
    }
    Ok(out)
}

/// SPICE-subset text for a KiCad value: the number, exactly.
fn spice_number(c: &Component, what: &str, word: &str) -> Result<String, NetlistError> {
    let x = value(word).map_err(|e| c.refuse(format!("{what}: {e}")))?;
    Ok(format!("{x:e}"))
}

/// A net name as a node name (module docs).
fn node_name(net: &str) -> String {
    let net = net.strip_prefix('/').unwrap_or(net);
    if net.eq_ignore_ascii_case("gnd") || net == "0" {
        return "0".into();
    }
    net.chars()
        .map(|c| if c.is_whitespace() || "(),={};".contains(c) { '_' } else { c })
        .collect()
}

/// Where a line of the generated deck came from, so an error names it.
#[derive(Clone)]
enum Origin {
    Header,
    Part(String),
    Library(String),
    Directives(usize),
}

/// A KiCad netlist and what it needs to run: the SPICE libraries its
/// parts name, and the directives a schematic would carry as text.
///
/// # Derivation
/// KiCad's documented netlist export and simulation-model fields (the
/// Schematic Editor manual's *Netlist formats* and *Simulator* chapters),
/// translated part by part to the SPICE subset
/// [`Netlist::parse_spice`] reads, so a part is read by one parser
/// whichever file it came from.
///
/// # Verified
/// `crates/stave-test/tests/kicad_import.rs::diode_clipper_round_trips`
/// and `::fuzz_face_round_trips`: a KiCad netlist of each deck
/// reads to the deck's elements on the deck's nodes (by name), and,
/// numbered alike, runs to the deck's output bit for bit (budget 1e-12
/// of peak); the Fuzz Face in KiCad's part order against the deck's own
/// differs by the DK solver's rounding under a renumbering (5e-12).
///
/// # Cost
/// Import time only; the circuit it builds costs what its method does.
///
/// # Valid ranges
/// KiCad 5 through 8 netlists (the `.net` s-expression); the parts the
/// module docs list.
pub struct Import<'a> {
    text: &'a str,
    libraries: Vec<(String, String)>,
    directives: String,
}

impl<'a> Import<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            libraries: Vec::new(),
            directives: String::new(),
        }
    }

    /// A SPICE library a part's `Sim.Library` (or `Spice_Lib_File`) names,
    /// matched by file name (any directory): its `.model` cards.
    pub fn library(mut self, name: &str, text: &str) -> Self {
        self.libraries.push((file_name(name).to_string(), text.to_string()));
        self
    }

    /// SPICE directives to append: analyses, options, the probe.
    pub fn directives(mut self, text: &str) -> Self {
        self.directives.push_str(text);
        self.directives.push('\n');
        self
    }

    /// Read the netlist.
    pub fn netlist(&self) -> Result<Netlist, NetlistError> {
        let root = parse_sexp(self.text)?;
        if root.head() != Some("export") {
            return Err(syntax(root.line(), "not a KiCad netlist: it does not start `(export`"));
        }
        let mut components = components(&root)?;
        assign_pins(&root, &mut components)?;
        let title = root
            .child("design")
            .and_then(|d| d.field("source"))
            .map_or("KiCad netlist".to_string(), |s| format!("KiCad netlist: {}", file_name(s)));
        let mut deck = Deck::default();
        deck.line(Origin::Header, format!("* {title}"));
        let mut pots = Vec::new();
        for c in &components {
            if let Some(pot) = self.component(c, &mut deck)? {
                pots.push(pot);
            }
        }
        for (i, line) in self.directives.lines().enumerate() {
            deck.line(Origin::Directives(i + 1), line.to_string());
        }
        let mut netlist = Netlist::parse_spice(&deck.text).map_err(|e| deck.blame(e))?;
        netlist.title = title;
        for pot in pots {
            netlist.add_variable(pot)?;
        }
        Ok(netlist)
    }

    /// One component's lines; a pot's declaration to add once parsed.
    fn component(&self, c: &Component, deck: &mut Deck) -> Result<Option<Variable>, NetlistError> {
        let excluded = c
            .fields
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case("exclude_from_sim") && !matches!(v.trim(), "no" | "0" | "false"))
            || c.field("Sim.Enable").is_some_and(disabled)
            || c.field("Spice_Netlist_Enabled").is_some_and(disabled);
        if excluded {
            return Ok(None);
        }
        let origin = Origin::Part(c.reference.clone());
        if let Some(device) = c.field("Sim.Device") {
            return self.sim_device(c, &device.to_ascii_uppercase(), deck);
        }
        if let Some(primitive) = c.field("Spice_Primitive") {
            let letter = primitive.trim().to_ascii_uppercase();
            let order: Vec<String> = match c.field("Spice_Node_Sequence") {
                Some(seq) => seq.split_whitespace().map(str::to_string).collect(),
                None => c.sorted_pins().iter().map(|(p, _)| p.clone()).collect(),
            };
            let nodes = order
                .iter()
                .map(|p| net_of(c, p))
                .collect::<Result<Vec<_>, _>>()?
                .join(" ");
            let tail = match (letter.as_str(), c.field("Spice_Model")) {
                ("R" | "C" | "L", None) => spice_number(c, "value", &c.value)?,
                (_, Some(model)) => model.to_string(),
                (_, None) => c.value.clone(),
            };
            if letter == "X" {
                return Err(c.refuse("a subcircuit (Spice_Primitive X): the SPICE subset has no X elements"));
            }
            if let Some(lib) = c.field("Spice_Lib_File") {
                self.include_model(c, lib, tail.split_whitespace().next().unwrap_or(""), deck)?;
            }
            deck.line(origin, format!("{} {nodes} {tail}", element_name(&letter, &c.reference)));
            return Ok(None);
        }
        let prefix: String = c.reference.chars().take(1).collect::<String>().to_ascii_uppercase();
        if c.pins.len() == 2 && matches!(prefix.as_str(), "R" | "C" | "L") {
            return self.sim_device(c, &prefix, deck);
        }
        Err(c.refuse(format!(
            "no simulation model: give it Sim.Device (its value {:?} names no model stave can infer)",
            c.value
        )))
    }

    fn sim_device(&self, c: &Component, device: &str, deck: &mut Deck) -> Result<Option<Variable>, NetlistError> {
        let kind = c.field("Sim.Type").unwrap_or("").to_ascii_uppercase();
        let p = match c.field("Sim.Params") {
            Some(text) => params(c, text)?,
            None => Vec::new(),
        };
        let get = |k: &str| p.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        let origin = Origin::Part(c.reference.clone());
        let nodes = |model_pins: &[&str]| model_nodes(c, model_pins);
        match (device, kind.as_str()) {
            ("R" | "C" | "L", "") => {
                let key = device.to_ascii_lowercase();
                let v = get(&key).unwrap_or(&c.value);
                let line = format!(
                    "{} {} {}",
                    element_name(device, &c.reference),
                    nodes(&["+", "-"])?.join(" "),
                    spice_number(c, &key, v)?
                );
                deck.line(origin, line);
                Ok(None)
            }
            ("R", "POT") => {
                let r = value(get("r").unwrap_or(&c.value)).map_err(|e| c.refuse(format!("r: {e}")))?;
                let pos = value(get("pos").unwrap_or("0.5")).map_err(|e| c.refuse(format!("pos: {e}")))?;
                if get("log").is_some_and(|v| v != "0") {
                    return Err(c.refuse("a logarithmic pot: give the taper as a Variable in code"));
                }
                let n = nodes(&["r0", "wiper", "r1"])?;
                let name = element_name("R", &c.reference);
                let (a, b) = (format!("{name}_a"), format!("{name}_b"));
                deck.line(origin.clone(), format!("{a} {} {} {:e}", n[0], n[1], r * pos));
                deck.line(origin, format!("{b} {} {} {:e}", n[1], n[2], r * (1.0 - pos)));
                Ok(Some(Variable::pot(&name, [&a, &b], Taper::Linear)))
            }
            ("V" | "I", "" | "DC" | "SIN" | "PWL") => {
                let waveform = source(c, &kind, &get)?;
                let line = format!(
                    "{} {} {waveform}",
                    element_name(device, &c.reference),
                    nodes(&["+", "-"])?.join(" ")
                );
                deck.line(origin, line);
                Ok(None)
            }
            ("V" | "I", "=") => {
                let key = device.to_ascii_lowercase();
                let expr = get(&key).ok_or_else(|| c.refuse(format!("a behavioural source needs {key}=")))?;
                let line = format!(
                    "{} {} {device} = {{{expr}}}",
                    element_name("B", &c.reference),
                    nodes(&["+", "-"])?.join(" ")
                );
                deck.line(origin, line);
                Ok(None)
            }
            ("E" | "G", "") => {
                let gain = spice_number(c, "gain", get("gain").unwrap_or(&c.value))?;
                let line = format!(
                    "{} {} {gain}",
                    element_name(device, &c.reference),
                    nodes(&["+", "-", "C+", "C-"])?.join(" ")
                );
                deck.line(origin, line);
                Ok(None)
            }
            ("D", "") => self.semiconductor(c, "D", "D", &["A", "K"], &p, deck),
            ("NPN" | "PNP", "" | "GUMMELPOON") => {
                let pins: &[&str] = if c.pins.len() >= 4 { &["C", "B", "E", "<S>"] } else { &["C", "B", "E"] };
                self.semiconductor(c, "Q", device, pins, &p, deck)
            }
            ("NJFET" | "PJFET", "" | "SHICHMANHODGES") => {
                let card = if device == "NJFET" { "NJF" } else { "PJF" };
                self.semiconductor(c, "J", card, &["D", "G", "S"], &p, deck)
            }
            ("NMOS" | "PMOS", "" | "MOS1") => self.semiconductor(c, "M", device, &["D", "G", "S", "B"], &p, deck),
            ("SPICE", _) => {
                let letter = get("type").ok_or_else(|| c.refuse("a raw SPICE element needs type="))?;
                let letter = letter.to_ascii_uppercase();
                if letter == "X" {
                    return Err(c.refuse("a subcircuit: the SPICE subset has no X elements"));
                }
                let model = get("model").unwrap_or(&c.value).to_string();
                if let Some(lib) = get("lib").or(c.field("Sim.Library")) {
                    self.include_model(c, lib, model.split_whitespace().next().unwrap_or(""), deck)?;
                }
                let pins: Vec<String> = c.sorted_pins().iter().map(|(p, _)| p.clone()).collect();
                let order: Vec<&str> = pins.iter().map(String::as_str).collect();
                let n = match c.field("Sim.Pins") {
                    Some(_) => nodes(&order)?,
                    None => order.iter().map(|p| net_of(c, p)).collect::<Result<_, _>>()?,
                };
                deck.line(origin, format!("{} {} {model}", element_name(&letter, &c.reference), n.join(" ")));
                Ok(None)
            }
            ("SUBCKT", _) => Err(c.refuse("a subcircuit (Sim.Device SUBCKT): the SPICE subset has no X elements")),
            (_, "") => Err(c.refuse(format!("Sim.Device {device}: not a device stave runs"))),
            (_, _) => Err(c.refuse(format!("Sim.Device {device}, Sim.Type {kind}: not a model stave runs"))),
        }
    }

    /// A diode or transistor: its element line and its card — the
    /// library's (with `Sim.Params` over it) or one built from `Sim.Params`.
    fn semiconductor(
        &self,
        c: &Component,
        letter: &str,
        card_type: &str,
        model_pins: &[&str],
        p: &[(String, String)],
        deck: &mut Deck,
    ) -> Result<Option<Variable>, NetlistError> {
        // Instance parameters go on the element; the rest (a diode's
        // grading coefficient `m` among them) make the card.
        let instance_keys: &[&str] = match letter {
            "M" => &["w", "l", "ad", "as", "pd", "ps", "m"],
            _ => &["area"],
        };
        let mut instance = Vec::new();
        let mut card = Vec::new();
        for (k, v) in p {
            let text = format!("{k}={}", spice_number(c, k, v)?);
            if instance_keys.contains(&k.as_str()) {
                instance.push((k.as_str(), text));
            } else {
                card.push(text);
            }
        }
        let name = element_name(letter, &c.reference);
        let model = match (c.field("Sim.Library"), c.field("Sim.Name")) {
            (Some(lib), Some(model)) => {
                let card_text = self.find_model(c, lib, model)?;
                if card.is_empty() {
                    self.include_model(c, lib, model, deck)?;
                    model.to_ascii_lowercase()
                } else {
                    let own = format!("{name}_model");
                    deck.line(Origin::Part(c.reference.clone()), override_card(&card_text, &own, &card.join(" ")));
                    own
                }
            }
            (Some(_), None) => return Err(c.refuse("Sim.Library without Sim.Name: which model?")),
            _ => {
                let own = format!("{name}_model");
                deck.line(Origin::Part(c.reference.clone()), format!(".model {own} {card_type}({})", card.join(" ")));
                own
            }
        };
        let nodes = model_nodes(c, model_pins)?.join(" ");
        // SPICE writes a BJT, JFET or diode's area as a bare number after
        // the model, a MOSFET's geometry as KEY=value.
        let mut tail = String::new();
        for (k, text) in &instance {
            match *k {
                "area" => tail.push_str(&format!(" {}", text.trim_start_matches("area="))),
                _ => tail.push_str(&format!(" {text}")),
            }
        }
        deck.line(Origin::Part(c.reference.clone()), format!("{name} {nodes} {model}{tail}"));
        Ok(None)
    }

    fn library_text(&self, c: &Component, lib: &str) -> Result<&str, NetlistError> {
        let file = file_name(lib);
        self.libraries
            .iter()
            .find(|(n, _)| n == file)
            .map(|(_, t)| t.as_str())
            .ok_or_else(|| c.refuse(format!("its library {file} was not given (Import::library)")))
    }

    /// The `.model` card named `model` in the library, as one line.
    fn find_model(&self, c: &Component, lib: &str, model: &str) -> Result<String, NetlistError> {
        let text = self.library_text(c, lib)?;
        let mut found: Option<String> = None;
        for line in text.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix('+') {
                if let Some(card) = &mut found {
                    card.push(' ');
                    card.push_str(rest);
                }
                continue;
            }
            if found.is_some() {
                break;
            }
            let words: Vec<&str> = t.split_whitespace().collect();
            let (Some(head), Some(name)) = (words.first(), words.get(1)) else {
                continue;
            };
            if head.eq_ignore_ascii_case(".subckt") && name.eq_ignore_ascii_case(model) {
                return Err(c.refuse(format!("{model} is a subcircuit (.subckt): the SPICE subset has no X elements")));
            }
            if head.eq_ignore_ascii_case(".model") && name.eq_ignore_ascii_case(model) {
                found = Some(t.to_string());
            }
        }
        found.ok_or_else(|| c.refuse(format!("no .model {model} in {}", file_name(lib))))
    }

    fn include_model(&self, c: &Component, lib: &str, model: &str, deck: &mut Deck) -> Result<(), NetlistError> {
        let key = model.to_ascii_lowercase();
        if deck.models.contains(&key) {
            return Ok(());
        }
        let card = self.find_model(c, lib, model)?;
        deck.models.push(key);
        deck.line(
            Origin::Library(file_name(lib).to_string()),
            card,
        );
        Ok(())
    }
}

/// `Sim.Enable`-style falsehood: `0`, `n…`, `f…`.
fn disabled(v: &str) -> bool {
    matches!(v.trim().chars().next().map(|c| c.to_ascii_lowercase()), Some('0' | 'n' | 'f'))
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The SPICE name: the reference, behind its element letter when it
/// starts with another.
fn element_name(letter: &str, reference: &str) -> String {
    let r = reference.to_ascii_lowercase();
    let l = letter.to_ascii_lowercase();
    if r.starts_with(&l) {
        r
    } else {
        format!("{l}{r}")
    }
}

/// A card renamed `own`, with `extra` parameters after its own (the
/// reader keeps the last value of a key).
fn override_card(card: &str, own: &str, extra: &str) -> String {
    let mut words = card.splitn(3, char::is_whitespace);
    let (_, _, rest) = (words.next(), words.next(), words.next().unwrap_or(""));
    let rest = rest.trim();
    match rest.strip_suffix(')') {
        Some(open) => format!(".model {own} {open} {extra})"),
        None => format!(".model {own} {rest} {extra}"),
    }
}

/// A symbol pin's node.
fn net_of(c: &Component, pin: &str) -> Result<String, NetlistError> {
    c.pins
        .iter()
        .find(|(p, _)| p == pin)
        .map(|(_, net)| node_name(net))
        .ok_or_else(|| c.refuse(format!("pin {pin} is on no net")))
}

/// The nodes on a model's pins, in the model's order, through `Sim.Pins`
/// (else the symbol's pins in order).
fn model_nodes(c: &Component, model_pins: &[&str]) -> Result<Vec<String>, NetlistError> {
    match c.field("Sim.Pins") {
        Some(map) => {
            let pairs: Vec<(&str, &str)> = map
                .split_whitespace()
                .filter_map(|pair| pair.split_once('='))
                .collect();
            model_pins
                .iter()
                .map(|m| {
                    let symbol = pairs
                        .iter()
                        .find(|(_, model)| model.eq_ignore_ascii_case(m))
                        .map(|(s, _)| *s)
                        .ok_or_else(|| c.refuse(format!("Sim.Pins maps no symbol pin to model pin {m}")))?;
                    net_of(c, symbol)
                })
                .collect()
        }
        None => {
            let pins = c.sorted_pins();
            if pins.len() < model_pins.len() {
                return Err(c.refuse(format!(
                    "{} pins on the symbol, {} on its model ({})",
                    pins.len(),
                    model_pins.len(),
                    model_pins.join(" ")
                )));
            }
            Ok(pins[..model_pins.len()].iter().map(|(_, net)| node_name(net)).collect())
        }
    }
}

/// A source's SPICE waveform from its `Sim.Type` and parameters, else
/// its value.
fn source<'p>(c: &Component, kind: &str, get: &dyn Fn(&str) -> Option<&'p str>) -> Result<String, NetlistError> {
    let num = |k: &str, default: Option<&str>| -> Result<String, NetlistError> {
        match get(k).or(default) {
            Some(v) => spice_number(c, k, v),
            None => Err(c.refuse(format!("a {kind} source needs {k}="))),
        }
    };
    let mut out = match kind {
        "SIN" => format!(
            "SIN({} {} {} {} {} {})",
            num("dc", Some("0"))?,
            num("ampl", None)?,
            num("f", None)?,
            num("td", Some("0"))?,
            num("theta", Some("0"))?,
            num("phase", Some("0"))?
        ),
        "PWL" => {
            let points = get("pwl").ok_or_else(|| c.refuse("a PWL source needs pwl="))?;
            let numbers = points
                .split(|ch: char| ch.is_whitespace() || ch == ',')
                .filter(|w| !w.is_empty())
                .map(|w| spice_number(c, "pwl", w))
                .collect::<Result<Vec<_>, _>>()?;
            format!("PWL({})", numbers.join(" "))
        }
        _ => match get("dc") {
            Some(v) => format!("DC {}", spice_number(c, "dc", v)?),
            None if value(&c.value).is_ok() => format!("DC {}", spice_number(c, "value", &c.value)?),
            // KiCad 5's value as SPICE text: `AC 1`, `DC 10`, `SIN(0 1 1k)`.
            None if get("ac").is_none() => c.value.clone(),
            None => "DC 0".into(),
        },
    };
    if let Some(ac) = get("ac") {
        out.push_str(&format!(" AC {}", spice_number(c, "ac", ac)?));
    }
    if get("ph").is_some_and(|v| value(v) != Ok(0.0)) {
        return Err(c.refuse("an AC phase: the small-signal source is real in the subset"));
    }
    Ok(out)
}

/// Every component, with its fields.
fn components(root: &Sexp) -> Result<Vec<Component>, NetlistError> {
    let Some(list) = root.child("components") else {
        return Err(syntax(root.line(), "no (components …)"));
    };
    let mut out: Vec<Component> = Vec::new();
    for comp in list.children("comp") {
        let reference = comp
            .field("ref")
            .ok_or_else(|| syntax(comp.line(), "a comp without a ref"))?
            .to_string();
        if out.iter().any(|c| c.reference == reference) {
            return Err(NetlistError::Duplicate { name: reference });
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        if let Some(fs) = comp.child("fields") {
            for f in fs.children("field") {
                if let (Some(name), Some(v)) = (f.field("name"), f.items().get(2).and_then(Sexp::atom)) {
                    fields.push((name.to_string(), v.to_string()));
                }
            }
        }
        for p in comp.children("property") {
            if let Some(name) = p.field("name") {
                let v = p.field("value").unwrap_or("").to_string();
                fields.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
                fields.push((name.to_string(), v));
            }
        }
        // KiCad 8 may mark the comp itself.
        if comp.child("excluded_from_sim").is_some() {
            fields.push(("exclude_from_sim".into(), "yes".into()));
        }
        out.push(Component {
            reference,
            value: comp.field("value").unwrap_or("").to_string(),
            fields,
            pins: Vec::new(),
            line: comp.line(),
        });
    }
    Ok(out)
}

/// Each net's pins onto its components.
fn assign_pins(root: &Sexp, components: &mut [Component]) -> Result<(), NetlistError> {
    let Some(nets) = root.child("nets") else {
        return Err(syntax(root.line(), "no (nets …)"));
    };
    let mut names: Vec<(String, String)> = Vec::new();
    for net in nets.children("net") {
        let name = net
            .field("name")
            .ok_or_else(|| syntax(net.line(), "a net without a name"))?
            .to_string();
        let node = node_name(&name).to_ascii_lowercase();
        if let Some((other, _)) = names.iter().find(|(o, n)| *n == node && *o != name) {
            return Err(syntax(net.line(), format!("nets {other:?} and {name:?} both read as node {node}")));
        }
        names.push((name.clone(), node));
        for n in net.children("node") {
            let (Some(r), Some(pin)) = (n.field("ref"), n.field("pin")) else {
                return Err(syntax(n.line(), "a net node without ref and pin"));
            };
            // A pin on a part the components list lacks (a power flag) is
            // nothing to simulate.
            if let Some(c) = components.iter_mut().find(|c| c.reference == r) {
                if c.pins.iter().any(|(p, _)| p == pin) {
                    return Err(syntax(n.line(), format!("{r} pin {pin} on two nets (component at line {})", c.line)));
                }
                c.pins.push((pin.to_string(), name.clone()));
            }
        }
    }
    Ok(())
}

/// The SPICE deck being written, each line's origin beside it.
#[derive(Default)]
struct Deck {
    text: String,
    origins: Vec<Origin>,
    /// Library cards already written (lower case).
    models: Vec<String>,
}

impl Deck {
    fn line(&mut self, origin: Origin, text: String) {
        // A card spanning lines keeps its origin on each.
        for l in text.lines() {
            self.origins.push(origin.clone());
            self.text.push_str(l);
            self.text.push('\n');
        }
    }

    /// A SPICE reader's error, told in the KiCad netlist's terms.
    fn blame(&self, e: NetlistError) -> NetlistError {
        let origin = |line: usize| self.origins.get(line.wrapping_sub(1)).cloned();
        match e {
            NetlistError::UnknownModel { line, model } => match origin(line) {
                Some(Origin::Part(r)) => NetlistError::Unsupported {
                    element: r,
                    reason: format!("no .model {model}"),
                },
                _ => NetlistError::UnknownModel { line, model },
            },
            NetlistError::Syntax { line, message } => match origin(line) {
                Some(Origin::Part(r)) => NetlistError::Unsupported {
                    element: r,
                    reason: message,
                },
                Some(Origin::Library(name)) => NetlistError::Unsupported {
                    element: name,
                    reason: message,
                },
                Some(Origin::Directives(l)) => NetlistError::Syntax {
                    line: l,
                    message: format!("in the directives: {message}"),
                },
                _ => NetlistError::Syntax { line, message },
            },
            other => other,
        }
    }
}

impl Netlist {
    /// Read a KiCad netlist ([`kicad`](self) module docs) with no
    /// libraries or directives: [`Import`] for those.
    pub fn parse_kicad(text: &str) -> Result<Self, NetlistError> {
        Import::new(text).netlist()
    }
}

#[cfg(test)]
mod tests;
