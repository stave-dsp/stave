//! A netlist's linear one-ports as a forest of adapted wave digital
//! trees: every resistor, capacitor and inductor a leaf, joined by
//! series and parallel adaptors wherever the topology allows (the
//! series–parallel reduction of the network graph), each tree's root a
//! port of the R-type adaptor that holds what no tree can.
//!
//! The trees live flat in one arena, children before parents, so a
//! sample's pass up runs in index order and its pass down in reverse,
//! with no recursion and no allocation.

use crate::circuit::netlist::Node;

/// What a cell of the forest is.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Resistor,
    /// `initial_volts` is the element's `IC=`.
    Capacitor { farads: f64, initial_volts: Option<f64> },
    /// `ordinal` numbers it among the netlist's inductors, in order.
    Inductor { henries: f64, initial_amps: Option<f64>, ordinal: usize },
    /// An ideal voltage source: zero port resistance, adaptable only in
    /// series with something that has some.
    Source,
    Series(usize, usize),
    Parallel(usize, usize),
    Inverter(usize),
}

/// One cell: an element or an adaptor, with its port resistance and the
/// waves of its last sample.
#[derive(Clone, Debug)]
pub(crate) struct Cell {
    pub kind: Kind,
    pub ohms: f64,
    /// A parallel adaptor's left share `G₁/G`.
    pub share: f64,
    /// The wave sent up (β), and the one that came down (α).
    pub beta: f64,
    pub alpha: f64,
    /// Under ADAA, the last sample's β (`synchronize`).
    pub previous: f64,
    /// A reactance's stored wave; a source's volts.
    pub state: f64,
    /// It (or something below it) reflects a nonzero wave.
    pub reflective: bool,
    /// It (or something below it) stores a wave.
    pub stateful: bool,
    /// The element's terminals (the cell's orientation), for leaves.
    pub terminals: (Node, Node),
}

impl Cell {
    fn leaf(kind: Kind, ohms: f64, terminals: (Node, Node)) -> Self {
        let stateful = matches!(kind, Kind::Capacitor { .. } | Kind::Inductor { .. });
        Self {
            kind,
            ohms,
            share: 0.0,
            beta: 0.0,
            alpha: 0.0,
            previous: 0.0,
            state: 0.0,
            reflective: stateful || kind == Kind::Source,
            stateful,
            terminals,
        }
    }
}

/// A subtree during reduction: its root cell and the nodes it joins.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Edge {
    pub cell: usize,
    pub plus: Node,
    pub minus: Node,
}

/// The forest: cells, and the trees' roots that face the R-type adaptor.
#[derive(Clone, Debug, Default)]
pub(crate) struct Forest {
    pub cells: Vec<Cell>,
    pub roots: Vec<Edge>,
    /// The cells a pass up visits (reflective, children first) and a
    /// pass down visits (stateful, parents first).
    up_order: Vec<usize>,
    down_order: Vec<usize>,
}

impl Forest {
    pub fn leaf(&mut self, kind: Kind, ohms: f64, plus: Node, minus: Node) -> Edge {
        self.cells.push(Cell::leaf(kind, ohms, (plus, minus)));
        Edge {
            cell: self.cells.len() - 1,
            plus,
            minus,
        }
    }

    fn push(&mut self, kind: Kind, children: &[usize]) -> usize {
        let reflective = children.iter().any(|&c| self.cells[c].reflective);
        let stateful = children.iter().any(|&c| self.cells[c].stateful);
        self.cells.push(Cell {
            kind,
            ohms: 0.0,
            share: 0.0,
            beta: 0.0,
            alpha: 0.0,
            previous: 0.0,
            state: 0.0,
            reflective,
            stateful,
            terminals: (Node::GROUND, Node::GROUND),
        });
        self.cells.len() - 1
    }

    /// `edge` read from `plus` to the other end: wrapped in an inverter
    /// when it runs the other way.
    fn oriented(&mut self, edge: Edge, plus: Node) -> usize {
        if edge.plus == plus {
            edge.cell
        } else {
            self.push(Kind::Inverter(edge.cell), &[edge.cell])
        }
    }

    /// A zero-resistance subtree (an ideal source on its own).
    fn ideal(&self, edge: &Edge) -> bool {
        self.cells[edge.cell].kind == Kind::Source
    }

    /// Reduce `edges` by series and parallel adaptors until neither
    /// applies; a node in `pinned` (the R-type adaptor's, the probe's,
    /// ground) is never reduced away. The survivors become the roots.
    pub fn reduce(&mut self, mut edges: Vec<Edge>, pinned: &[bool]) {
        loop {
            if let Some((i, j)) = self.parallel_pair(&edges) {
                let (a, b) = (edges[i], edges[j]);
                let right = self.oriented(b, a.plus);
                let cell = self.push(Kind::Parallel(a.cell, right), &[a.cell, right]);
                edges[i] = Edge { cell, ..a };
                edges.swap_remove(j);
                continue;
            }
            if let Some((i, j, middle)) = self.series_pair(&edges, pinned) {
                let (a, b) = (edges[i], edges[j]);
                let start = if a.plus == middle { a.minus } else { a.plus };
                let end = if b.plus == middle { b.minus } else { b.plus };
                let left = self.oriented(a, start);
                let right = self.oriented(b, middle);
                let cell = self.push(Kind::Series(left, right), &[left, right]);
                let (lo, hi) = (i.min(j), i.max(j));
                edges[lo] = Edge {
                    cell,
                    plus: start,
                    minus: end,
                };
                edges.swap_remove(hi);
                continue;
            }
            break;
        }
        self.roots = edges;
    }

    fn parallel_pair(&self, edges: &[Edge]) -> Option<(usize, usize)> {
        for i in 0..edges.len() {
            if self.ideal(&edges[i]) {
                continue;
            }
            for j in i + 1..edges.len() {
                let (a, b) = (edges[i], edges[j]);
                let same = (a.plus == b.plus && a.minus == b.minus) || (a.plus == b.minus && a.minus == b.plus);
                if same && !self.ideal(&b) {
                    return Some((i, j));
                }
            }
        }
        None
    }

    fn series_pair(&self, edges: &[Edge], pinned: &[bool]) -> Option<(usize, usize, Node)> {
        let mut seen: Vec<(usize, Vec<usize>)> = Vec::new();
        for (k, e) in edges.iter().enumerate() {
            for node in [e.plus, e.minus] {
                match seen.iter_mut().find(|(n, _)| *n == node.index()) {
                    Some((_, list)) => list.push(k),
                    None => seen.push((node.index(), vec![k])),
                }
            }
        }
        for (node, list) in seen {
            if pinned[node] || list.len() != 2 || list[0] == list[1] {
                continue;
            }
            let (i, j) = (list[0], list[1]);
            if self.ideal(&edges[i]) && self.ideal(&edges[j]) {
                continue;
            }
            let middle = if edges[i].plus.index() == node { edges[i].plus } else { edges[i].minus };
            return Some((i, j, middle));
        }
        None
    }

    /// Port resistances bottom-up, for capacitors and inductors at
    /// `sample_rate`.
    pub fn adapt(&mut self, sample_rate: f64) {
        self.resize(sample_rate);
        let cells = &self.cells;
        self.up_order = (0..cells.len()).filter(|&i| cells[i].reflective).collect();
        self.down_order = (0..cells.len()).rev().filter(|&i| cells[i].stateful).collect();
    }

    /// Port resistances and parallel shares bottom-up from the leaves'
    /// values, the visiting orders kept: a knob move's re-adaptation
    /// (N6), realtime.
    pub fn resize(&mut self, sample_rate: f64) {
        for i in 0..self.cells.len() {
            let c = &self.cells[i];
            let (ohms, share) = match c.kind {
                Kind::Resistor => (c.ohms, 0.0),
                Kind::Capacitor { farads, .. } => (1.0 / (2.0 * farads * sample_rate), 0.0),
                Kind::Inductor { henries, .. } => (2.0 * henries * sample_rate, 0.0),
                Kind::Source => (0.0, 0.0),
                Kind::Series(l, r) => (self.cells[l].ohms + self.cells[r].ohms, 0.0),
                Kind::Parallel(l, r) => {
                    let (gl, gr) = (1.0 / self.cells[l].ohms, 1.0 / self.cells[r].ohms);
                    (1.0 / (gl + gr), gl / (gl + gr))
                }
                Kind::Inverter(c) => (self.cells[c].ohms, 0.0),
            };
            self.cells[i].ohms = ohms;
            self.cells[i].share = share;
        }
    }

    /// The cells under `cell`, itself included.
    pub fn subtree(&self, cell: usize) -> Vec<usize> {
        let mut out = vec![cell];
        let mut i = 0;
        while i < out.len() {
            match self.cells[out[i]].kind {
                Kind::Series(l, r) | Kind::Parallel(l, r) => out.extend([l, r]),
                Kind::Inverter(c) => out.push(c),
                _ => {}
            }
            i += 1;
        }
        out
    }

    /// The pass up: every reflective cell's β from its children's.
    #[inline]
    pub fn up(&mut self) {
        for &i in &self.up_order {
            let beta = match self.cells[i].kind {
                Kind::Resistor => 0.0,
                Kind::Capacitor { .. } | Kind::Source => self.cells[i].state,
                Kind::Inductor { .. } => -self.cells[i].state,
                Kind::Series(l, r) => self.cells[l].beta + self.cells[r].beta,
                Kind::Parallel(l, r) => {
                    let s = self.cells[i].share;
                    s * self.cells[l].beta + (1.0 - s) * self.cells[r].beta
                }
                Kind::Inverter(c) => -self.cells[c].beta,
            };
            self.cells[i].beta = beta;
        }
    }

    /// The pass down, from the α each stateful root was given.
    #[inline]
    pub fn down(&mut self) {
        for &i in &self.down_order {
            let (alpha, beta, ohms) = (self.cells[i].alpha, self.cells[i].beta, self.cells[i].ohms);
            match self.cells[i].kind {
                Kind::Capacitor { .. } | Kind::Inductor { .. } => {
                    self.cells[i].state = crate::kernel::denormal::flush64(alpha);
                }
                Kind::Series(l, r) => {
                    let excess = (alpha - beta) / ohms;
                    for c in [l, r] {
                        let cell = &mut self.cells[c];
                        if cell.stateful {
                            cell.alpha = cell.beta + cell.ohms * excess;
                        }
                    }
                }
                Kind::Parallel(l, r) => {
                    for c in [l, r] {
                        let cell = &mut self.cells[c];
                        if cell.stateful {
                            cell.alpha = alpha + beta - cell.beta;
                        }
                    }
                }
                Kind::Inverter(c) => self.cells[c].alpha = -alpha,
                Kind::Resistor | Kind::Source => {}
            }
        }
    }

    /// ADAA's synchronization (Albertini, Bernardini & Sarti 2021): every
    /// wave sent up becomes its mean with the last sample's, so the pass
    /// down sees the trees at `n − ½` like the root's mean output.
    #[inline]
    pub fn synchronize(&mut self) {
        for &i in &self.up_order {
            let cell = &mut self.cells[i];
            let raw = cell.beta;
            cell.beta = 0.5 * (raw + cell.previous);
            cell.previous = raw;
        }
    }

    /// The waves sent up now are the last sample's (at rest, for ADAA).
    pub fn rest(&mut self) {
        for &i in &self.up_order {
            self.cells[i].previous = self.cells[i].beta;
        }
    }

    /// `(series, parallel, inverters, leaves)` in the forest.
    pub fn census(&self) -> (usize, usize, usize, usize) {
        let mut n = (0, 0, 0, 0);
        for c in &self.cells {
            match c.kind {
                Kind::Series(..) => n.0 += 1,
                Kind::Parallel(..) => n.1 += 1,
                Kind::Inverter(..) => n.2 += 1,
                _ => n.3 += 1,
            }
        }
        n
    }
}
