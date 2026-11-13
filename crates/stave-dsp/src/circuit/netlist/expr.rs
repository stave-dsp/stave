//! Behavioural-source expressions (SPICE `B` elements): parsed once to
//! an [`Expr`], compiled to a [`Tape`] whose evaluation returns the
//! value and its slope in every node voltage the expression reads, so a
//! Newton solve gets an exact Jacobian from any `I = {…}` or `V = {…}`.

use super::{Node, NetlistError};

/// The most node voltages one expression may read (a triode's plate law
/// reads two). Slopes are carried inline, so evaluation never allocates.
pub const MAX_CONTROLS: usize = 4;

/// A parsed expression. Parameters and constants are already folded in.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Number(f64),
    /// The simulation time, seconds.
    Time,
    /// `V(plus, minus)`.
    Volts(Node, Node),
    Neg(Box<Expr>),
    Binary(Binary, Box<Expr>, Box<Expr>),
    Call(Function, Vec<Expr>),
    /// `condition ? then : otherwise`.
    Select(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Binary {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Greater,
    Less,
    GreaterEqual,
    LessEqual,
    Equal,
    NotEqual,
}

/// The functions an expression may call (ngspice's names; `log` is
/// natural, as in ngspice).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Function {
    Exp,
    Ln,
    Log10,
    Sqrt,
    Abs,
    Sin,
    Cos,
    Tan,
    Tanh,
    Sinh,
    Cosh,
    Atan,
    Pow,
    Min,
    Max,
    /// `ln(1 + exp(x))`, recognised where an expression spells it out, so
    /// a large argument cannot overflow.
    Softplus,
}

impl Function {
    fn named(name: &str) -> Option<(Self, usize)> {
        Some(match name {
            "exp" => (Self::Exp, 1),
            "ln" | "log" => (Self::Ln, 1),
            "log10" => (Self::Log10, 1),
            "sqrt" => (Self::Sqrt, 1),
            "abs" => (Self::Abs, 1),
            "sin" => (Self::Sin, 1),
            "cos" => (Self::Cos, 1),
            "tan" => (Self::Tan, 1),
            "tanh" => (Self::Tanh, 1),
            "sinh" => (Self::Sinh, 1),
            "cosh" => (Self::Cosh, 1),
            "atan" => (Self::Atan, 1),
            "pow" | "pwr" => (Self::Pow, 2),
            "min" => (Self::Min, 2),
            "max" => (Self::Max, 2),
            _ => return None,
        })
    }
}

/// What an expression's names resolve to while it is parsed.
pub(crate) struct Scope<'a> {
    pub params: &'a [(String, f64)],
    pub node: &'a mut dyn FnMut(&str) -> Node,
}

impl Expr {
    /// Parse `text` (without its braces). Names are parameters, `time`,
    /// `pi`, functions, or `v(node)` / `v(plus, minus)`.
    pub(crate) fn parse(text: &str, scope: &mut Scope<'_>, line: usize) -> Result<Self, NetlistError> {
        let tokens = lex(text).map_err(|message| NetlistError::Syntax { line, message })?;
        let mut parser = Parser {
            tokens,
            at: 0,
            scope,
            line,
        };
        let expr = parser.ternary()?;
        if parser.at != parser.tokens.len() {
            return Err(parser.error("trailing input"));
        }
        Ok(expr.folded())
    }

    /// The node voltages this expression reads, in first-use order.
    pub fn controls(&self) -> Vec<(Node, Node)> {
        let mut out = Vec::new();
        self.visit(&mut |e| {
            if let Expr::Volts(a, b) = e {
                if !out.contains(&(*a, *b)) {
                    out.push((*a, *b));
                }
            }
        });
        out
    }

    /// Whether the expression reads the time.
    pub fn reads_time(&self) -> bool {
        let mut found = false;
        self.visit(&mut |e| found |= matches!(e, Expr::Time));
        found
    }

    fn visit(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Neg(a) => a.visit(f),
            Expr::Binary(_, a, b) => {
                a.visit(f);
                b.visit(f);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.visit(f)),
            Expr::Select(c, a, b) => {
                c.visit(f);
                a.visit(f);
                b.visit(f);
            }
            Expr::Number(_) | Expr::Time | Expr::Volts(..) => {}
        }
    }

    /// Constants folded, and `ln(1 + exp(x))` recognised as softplus.
    fn folded(self) -> Self {
        match self {
            Expr::Neg(a) => match a.folded() {
                Expr::Number(x) => Expr::Number(-x),
                a => Expr::Neg(Box::new(a)),
            },
            Expr::Binary(op, a, b) => match (a.folded(), b.folded()) {
                (Expr::Number(x), Expr::Number(y)) => Expr::Number(binary(op, x, y)),
                (a, b) => Expr::Binary(op, Box::new(a), Box::new(b)),
            },
            Expr::Call(f, args) => {
                let args: Vec<Expr> = args.into_iter().map(Expr::folded).collect();
                if f == Function::Ln {
                    if let Some(x) = softplus_argument(&args[0]) {
                        return Expr::Call(Function::Softplus, vec![x]);
                    }
                }
                if args.iter().all(|a| matches!(a, Expr::Number(_))) {
                    let x: Vec<f64> = args
                        .iter()
                        .map(|a| match a {
                            Expr::Number(x) => *x,
                            _ => unreachable!(),
                        })
                        .collect();
                    return Expr::Number(call(f, &x));
                }
                Expr::Call(f, args)
            }
            Expr::Select(c, a, b) => match c.folded() {
                Expr::Number(x) => {
                    if x != 0.0 {
                        a.folded()
                    } else {
                        b.folded()
                    }
                }
                c => Expr::Select(Box::new(c), Box::new(a.folded()), Box::new(b.folded())),
            },
            e => e,
        }
    }

    /// Evaluate with every node voltage read from `volts` (by node
    /// index) — the plain, slope-free form, for checks and sources.
    pub fn value(&self, volts: &dyn Fn(Node) -> f64, time: f64) -> f64 {
        match self {
            Expr::Number(x) => *x,
            Expr::Time => time,
            Expr::Volts(a, b) => volts(*a) - volts(*b),
            Expr::Neg(a) => -a.value(volts, time),
            Expr::Binary(op, a, b) => binary(*op, a.value(volts, time), b.value(volts, time)),
            Expr::Call(f, args) => {
                let x: Vec<f64> = args.iter().map(|a| a.value(volts, time)).collect();
                call(*f, &x)
            }
            Expr::Select(c, a, b) => {
                if c.value(volts, time) != 0.0 {
                    a.value(volts, time)
                } else {
                    b.value(volts, time)
                }
            }
        }
    }
}

/// `1 + exp(x)` or `exp(x) + 1` → `x`.
fn softplus_argument(e: &Expr) -> Option<Expr> {
    let Expr::Binary(Binary::Add, a, b) = e else {
        return None;
    };
    match (a.as_ref(), b.as_ref()) {
        (Expr::Number(one), Expr::Call(Function::Exp, x))
        | (Expr::Call(Function::Exp, x), Expr::Number(one))
            if *one == 1.0 =>
        {
            Some(x[0].clone())
        }
        _ => None,
    }
}

fn binary(op: Binary, x: f64, y: f64) -> f64 {
    let truth = |b: bool| if b { 1.0 } else { 0.0 };
    match op {
        Binary::Add => x + y,
        Binary::Sub => x - y,
        Binary::Mul => x * y,
        Binary::Div => x / y,
        Binary::Pow => x.powf(y),
        Binary::Greater => truth(x > y),
        Binary::Less => truth(x < y),
        Binary::GreaterEqual => truth(x >= y),
        Binary::LessEqual => truth(x <= y),
        Binary::Equal => truth(x == y),
        Binary::NotEqual => truth(x != y),
    }
}

fn call(f: Function, x: &[f64]) -> f64 {
    match f {
        Function::Exp => x[0].exp(),
        Function::Ln => x[0].ln(),
        Function::Log10 => x[0].log10(),
        Function::Sqrt => x[0].sqrt(),
        Function::Abs => x[0].abs(),
        Function::Sin => x[0].sin(),
        Function::Cos => x[0].cos(),
        Function::Tan => x[0].tan(),
        Function::Tanh => x[0].tanh(),
        Function::Sinh => x[0].sinh(),
        Function::Cosh => x[0].cosh(),
        Function::Atan => x[0].atan(),
        Function::Pow => x[0].powf(x[1]),
        Function::Min => x[0].min(x[1]),
        Function::Max => x[0].max(x[1]),
        Function::Softplus => softplus(x[0]).0,
    }
}

/// ln(1 + eˣ) and its slope, the logistic, without overflow.
fn softplus(z: f64) -> (f64, f64) {
    let tail = (-z.abs()).exp();
    let value = z.max(0.0) + tail.ln_1p();
    let slope = if z >= 0.0 {
        1.0 / (1.0 + tail)
    } else {
        tail / (1.0 + tail)
    };
    (value, slope)
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Symbol(&'static str),
}

const SYMBOLS: [&str; 17] = [
    ">=", "<=", "==", "!=", "**", "+", "-", "*", "/", "^", "(", ")", ",", "?", ":", ">", "<",
];

fn lex(text: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    'outer: while let Some(c) = rest.chars().next() {
        if c.is_whitespace() {
            rest = &rest[c.len_utf8()..];
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let end = number_end(rest);
            let (word, tail) = rest.split_at(end);
            out.push(Token::Number(super::spice::value(word)?));
            rest = tail;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            out.push(Token::Name(rest[..end].to_ascii_lowercase()));
            rest = &rest[end..];
            continue;
        }
        for s in SYMBOLS {
            if let Some(tail) = rest.strip_prefix(s) {
                out.push(Token::Symbol(if s == "**" { "^" } else { s }));
                rest = tail;
                continue 'outer;
            }
        }
        return Err(format!("unexpected {c:?} in expression"));
    }
    Ok(out)
}

/// A number with an optional exponent and SPICE scale suffix (`2.2k`,
/// `1e-3`, `20meg`).
fn number_end(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
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
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    i
}

struct Parser<'s, 'a> {
    tokens: Vec<Token>,
    at: usize,
    scope: &'s mut Scope<'a>,
    line: usize,
}

impl Parser<'_, '_> {
    fn error(&self, what: &str) -> NetlistError {
        NetlistError::Syntax {
            line: self.line,
            message: format!("{what} in expression (token {})", self.at + 1),
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, symbol: &str) -> bool {
        if self.peek() == Some(&Token::Symbol(symbol_static(symbol))) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, symbol: &str) -> Result<(), NetlistError> {
        if self.eat(symbol) {
            Ok(())
        } else {
            Err(self.error(&format!("expected `{symbol}`")))
        }
    }

    fn ternary(&mut self) -> Result<Expr, NetlistError> {
        let condition = self.comparison()?;
        if self.eat("?") {
            let then = self.ternary()?;
            self.expect(":")?;
            let otherwise = self.ternary()?;
            return Ok(Expr::Select(
                Box::new(condition),
                Box::new(then),
                Box::new(otherwise),
            ));
        }
        Ok(condition)
    }

    fn comparison(&mut self) -> Result<Expr, NetlistError> {
        let left = self.additive()?;
        for (symbol, op) in [
            (">=", Binary::GreaterEqual),
            ("<=", Binary::LessEqual),
            ("==", Binary::Equal),
            ("!=", Binary::NotEqual),
            (">", Binary::Greater),
            ("<", Binary::Less),
        ] {
            if self.eat(symbol) {
                let right = self.additive()?;
                return Ok(Expr::Binary(op, Box::new(left), Box::new(right)));
            }
        }
        Ok(left)
    }

    fn additive(&mut self) -> Result<Expr, NetlistError> {
        let mut left = self.multiplicative()?;
        loop {
            let op = if self.eat("+") {
                Binary::Add
            } else if self.eat("-") {
                Binary::Sub
            } else {
                return Ok(left);
            };
            let right = self.multiplicative()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, NetlistError> {
        let mut left = self.unary()?;
        loop {
            let op = if self.eat("*") {
                Binary::Mul
            } else if self.eat("/") {
                Binary::Div
            } else {
                return Ok(left);
            };
            let right = self.unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Expr, NetlistError> {
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat("+") {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr, NetlistError> {
        let base = self.primary()?;
        if self.eat("^") {
            let exponent = self.unary()?;
            return Ok(Expr::Binary(Binary::Pow, Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Expr, NetlistError> {
        match self.peek().cloned() {
            Some(Token::Number(x)) => {
                self.at += 1;
                Ok(Expr::Number(x))
            }
            Some(Token::Symbol("(")) => {
                self.at += 1;
                let e = self.ternary()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(Token::Name(name)) => {
                self.at += 1;
                if self.eat("(") {
                    return self.call(&name);
                }
                match name.as_str() {
                    "time" => Ok(Expr::Time),
                    "pi" => Ok(Expr::Number(std::f64::consts::PI)),
                    _ => self
                        .scope
                        .params
                        .iter()
                        .rev()
                        .find(|(n, _)| *n == name)
                        .map(|&(_, x)| Expr::Number(x))
                        .ok_or_else(|| self.error(&format!("unknown name `{name}`"))),
                }
            }
            _ => Err(self.error("expected a value")),
        }
    }

    fn call(&mut self, name: &str) -> Result<Expr, NetlistError> {
        if name == "v" {
            let plus = self.node_name()?;
            let minus = if self.eat(",") {
                self.node_name()?
            } else {
                Node::GROUND
            };
            self.expect(")")?;
            return Ok(Expr::Volts(plus, minus));
        }
        let (f, arity) =
            Function::named(name).ok_or_else(|| self.error(&format!("unknown function `{name}`")))?;
        let mut args = vec![self.ternary()?];
        while self.eat(",") {
            args.push(self.ternary()?);
        }
        self.expect(")")?;
        if args.len() != arity {
            return Err(self.error(&format!("`{name}` takes {arity} argument(s)")));
        }
        Ok(Expr::Call(f, args))
    }

    fn node_name(&mut self) -> Result<Node, NetlistError> {
        let name = match self.peek().cloned() {
            Some(Token::Name(n)) => n,
            Some(Token::Number(_)) => {
                // Numeric node names (`0`, `12`) lex as numbers; take the text back.
                return self.numeric_node();
            }
            _ => return Err(self.error("expected a node name")),
        };
        self.at += 1;
        Ok((self.scope.node)(&name))
    }

    fn numeric_node(&mut self) -> Result<Node, NetlistError> {
        let Some(Token::Number(x)) = self.peek().cloned() else {
            return Err(self.error("expected a node name"));
        };
        self.at += 1;
        if x.fract() != 0.0 || x < 0.0 {
            return Err(self.error("a numeric node name must be a whole number"));
        }
        Ok((self.scope.node)(&format!("{}", x as u64)))
    }
}

fn symbol_static(s: &str) -> &'static str {
    SYMBOLS.iter().find(|&&t| t == s).copied().unwrap_or("")
}

/// A value and its slope in each control, carried through evaluation.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Dual {
    pub value: f64,
    pub slope: [f64; MAX_CONTROLS],
}

impl Dual {
    fn constant(value: f64) -> Self {
        Self {
            value,
            slope: [0.0; MAX_CONTROLS],
        }
    }

    /// f(self) given f and f′ at self.value.
    fn chain(self, value: f64, derivative: f64) -> Self {
        let mut slope = self.slope;
        for s in &mut slope {
            *s *= derivative;
        }
        Self { value, slope }
    }

    fn combine(self, other: Self, value: f64, da: f64, db: f64) -> Self {
        let mut slope = [0.0; MAX_CONTROLS];
        for i in 0..MAX_CONTROLS {
            slope[i] = da * self.slope[i] + db * other.slope[i];
        }
        Self { value, slope }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum Op {
    Constant(f64),
    Time,
    Control(usize),
    Neg,
    Binary(Binary),
    /// `x ^ c` with c known when compiled.
    PowConstant(f64),
    Call1(Function),
    Call2(Function),
    Select,
}

/// An [`Expr`] flattened for evaluation: a postfix program over a stack
/// of [`Dual`]s, its controls numbered in [`Expr::controls`] order.
#[derive(Clone, Debug, PartialEq)]
pub struct Tape {
    ops: Vec<Op>,
    controls: Vec<(Node, Node)>,
    depth: usize,
}

impl Tape {
    /// `None` when the expression reads more than [`MAX_CONTROLS`]
    /// node voltages.
    pub fn compile(expr: &Expr) -> Option<Self> {
        let controls = expr.controls();
        if controls.len() > MAX_CONTROLS {
            return None;
        }
        let mut tape = Self {
            ops: Vec::new(),
            controls,
            depth: 0,
        };
        let mut depth = 0;
        tape.emit(expr, &mut depth);
        Some(tape)
    }

    /// The node voltages the program reads, in its control order.
    pub fn controls(&self) -> &[(Node, Node)] {
        &self.controls
    }

    /// The stack an evaluation needs.
    pub fn depth(&self) -> usize {
        self.depth
    }

    fn push(&mut self, op: Op, depth: &mut usize, change: isize) {
        self.ops.push(op);
        *depth = (*depth as isize + change) as usize;
        self.depth = self.depth.max(*depth);
    }

    fn emit(&mut self, e: &Expr, depth: &mut usize) {
        match e {
            Expr::Number(x) => self.push(Op::Constant(*x), depth, 1),
            Expr::Time => self.push(Op::Time, depth, 1),
            Expr::Volts(a, b) => {
                let i = self.controls.iter().position(|c| *c == (*a, *b)).unwrap_or(0);
                self.push(Op::Control(i), depth, 1);
            }
            Expr::Neg(a) => {
                self.emit(a, depth);
                self.push(Op::Neg, depth, 0);
            }
            Expr::Binary(Binary::Pow, a, b) if matches!(**b, Expr::Number(_)) => {
                let Expr::Number(c) = **b else { unreachable!() };
                self.emit(a, depth);
                self.push(Op::PowConstant(c), depth, 0);
            }
            Expr::Binary(op, a, b) => {
                self.emit(a, depth);
                self.emit(b, depth);
                self.push(Op::Binary(*op), depth, -1);
            }
            Expr::Call(Function::Pow, args) if matches!(args[1], Expr::Number(_)) => {
                let Expr::Number(c) = args[1] else { unreachable!() };
                self.emit(&args[0], depth);
                self.push(Op::PowConstant(c), depth, 0);
            }
            Expr::Call(f, args) => {
                for a in args {
                    self.emit(a, depth);
                }
                if args.len() == 1 {
                    self.push(Op::Call1(*f), depth, 0);
                } else {
                    self.push(Op::Call2(*f), depth, -1);
                }
            }
            Expr::Select(c, a, b) => {
                self.emit(c, depth);
                self.emit(a, depth);
                self.emit(b, depth);
                self.push(Op::Select, depth, -2);
            }
        }
    }

    /// The value and its slopes, with `controls[i]` the voltage of
    /// control `i` and `stack` at least [`depth`](Self::depth) long.
    pub fn eval(&self, controls: &[f64], time: f64, stack: &mut [Dual]) -> Dual {
        let mut top = 0;
        for op in &self.ops {
            match *op {
                Op::Constant(x) => {
                    stack[top] = Dual::constant(x);
                    top += 1;
                }
                Op::Time => {
                    stack[top] = Dual::constant(time);
                    top += 1;
                }
                Op::Control(i) => {
                    let mut d = Dual::constant(controls[i]);
                    d.slope[i] = 1.0;
                    stack[top] = d;
                    top += 1;
                }
                Op::Neg => {
                    let a = stack[top - 1];
                    stack[top - 1] = a.chain(-a.value, -1.0);
                }
                Op::PowConstant(c) => {
                    let a = stack[top - 1];
                    let value = a.value.powf(c);
                    let derivative = if c == 0.0 { 0.0 } else { c * a.value.powf(c - 1.0) };
                    stack[top - 1] = a.chain(value, derivative);
                }
                Op::Binary(op) => {
                    let (a, b) = (stack[top - 2], stack[top - 1]);
                    top -= 1;
                    stack[top - 1] = binary_dual(op, a, b);
                }
                Op::Call1(f) => {
                    let a = stack[top - 1];
                    stack[top - 1] = call1_dual(f, a);
                }
                Op::Call2(f) => {
                    let (a, b) = (stack[top - 2], stack[top - 1]);
                    top -= 1;
                    stack[top - 1] = call2_dual(f, a, b);
                }
                Op::Select => {
                    let (c, a, b) = (stack[top - 3], stack[top - 2], stack[top - 1]);
                    top -= 2;
                    stack[top - 1] = if c.value != 0.0 { a } else { b };
                }
            }
        }
        stack[0]
    }
}

fn binary_dual(op: Binary, a: Dual, b: Dual) -> Dual {
    let (x, y) = (a.value, b.value);
    match op {
        Binary::Add => a.combine(b, x + y, 1.0, 1.0),
        Binary::Sub => a.combine(b, x - y, 1.0, -1.0),
        Binary::Mul => a.combine(b, x * y, y, x),
        Binary::Div => a.combine(b, x / y, 1.0 / y, -x / (y * y)),
        Binary::Pow => {
            let value = x.powf(y);
            a.combine(b, value, y * x.powf(y - 1.0), value * x.ln())
        }
        _ => Dual::constant(binary(op, x, y)),
    }
}

fn call1_dual(f: Function, a: Dual) -> Dual {
    let x = a.value;
    let (value, derivative) = match f {
        Function::Exp => {
            let e = x.exp();
            (e, e)
        }
        Function::Ln => (x.ln(), 1.0 / x),
        Function::Log10 => (x.log10(), 1.0 / (x * std::f64::consts::LN_10)),
        Function::Sqrt => {
            let s = x.sqrt();
            (s, 0.5 / s)
        }
        Function::Abs => (x.abs(), if x < 0.0 { -1.0 } else { 1.0 }),
        Function::Sin => (x.sin(), x.cos()),
        Function::Cos => (x.cos(), -x.sin()),
        Function::Tan => {
            let t = x.tan();
            (t, 1.0 + t * t)
        }
        Function::Tanh => {
            let t = x.tanh();
            (t, 1.0 - t * t)
        }
        Function::Sinh => (x.sinh(), x.cosh()),
        Function::Cosh => (x.cosh(), x.sinh()),
        Function::Atan => (x.atan(), 1.0 / (1.0 + x * x)),
        Function::Softplus => softplus(x),
        Function::Pow | Function::Min | Function::Max => (f64::NAN, f64::NAN),
    };
    a.chain(value, derivative)
}

fn call2_dual(f: Function, a: Dual, b: Dual) -> Dual {
    match f {
        Function::Pow => binary_dual(Binary::Pow, a, b),
        Function::Min => {
            if a.value <= b.value {
                a
            } else {
                b
            }
        }
        Function::Max => {
            if a.value >= b.value {
                a
            } else {
                b
            }
        }
        _ => Dual::constant(f64::NAN),
    }
}
