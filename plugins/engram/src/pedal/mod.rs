//! The face — engram as an instrument of memory: an indigo anodized
//! case with a vacuum-fluorescent display that shows the machine
//! THINKING:
//!
//! * MEMORY — the bank, an 8 × 8 grid: a cell glows faintly when it
//!   holds a memory (cyan a tonal one, amber a percussive one), and
//!   blazes with the attention it is being paid right now — you watch
//!   the room decide what this reminds it of.
//! * HEARING — the query: twelve pitch-class bars (dimmed as what you
//!   play turns percussive — then harmony hardly counts), and a PULSE
//!   lamp that flashes on every hit the ear hears.
//! * The NEURONS — the room's sixteen delay lines, draggable (radius =
//!   a line's length, closeness = coupling, always stable), glowing
//!   with their energy, tethered to where plasticity has pulled them.
//! * RECALL (how loudly it remembers) and NOVELTY (how new what you
//!   play is to it).
//! * SPACE — a second window: the MAP of the ≈700 real reverbs the room
//!   learned, each a faint dot, families labelled. Click or drag to put
//!   the cursor anywhere; the live room (a bright blob) glides after it,
//!   wanders with DRIFT, and is pulled (an amber line) toward where the
//!   recalled memories were heard — each memory is a dot at its room,
//!   blazing with the attention it gets. Below: the nearest real space
//!   and the decay.
//!
//! The scene's params, in order (editor.rs): `[size, decay, tone, space,
//! steer, drift, recall, focus, deja, match, texture, plasticity, mix,
//! bypass, hold, forget, space x, space y, node 1 x, node 1 y, …,
//! node 16 y]`; probes: `crate::FACE_TAPS` (packed).

mod render;

use stave_ui::scene::{pt, Op, Path, Pt, Scene, SceneInputs, Stroke};
use stave_ui::style::Color;

use crate::engine::space_model::{FAMILIES, MAP_FAMILY, MAP_XY, NAMES, SPACES};
use crate::{unpack, FIELD_ASPECT, TAP_ATTN, TAP_ENERGY, TAP_FILLED, TAP_HEARD_AT, TAP_HEARING, TAP_NOVELTY, TAP_PERC, TAP_POS, TAP_PULSE, TAP_RECALLING, TAP_SPACE, TAP_SPACE_INFO, TAP_TONAL};

pub const W: f64 = 940.0;
pub const H: f64 = 520.0;
const CASE: (f64, f64, f64, f64, f64) = (28.0, 24.0, 884.0, 472.0, 18.0);
/// The SPACE window: centre, half-size, corner.
const MAP_WINDOW: (f64, f64, f64, f64, f64) = (770.0, 160.0, 112.0, 112.0, 10.0);
/// The map inside it: x, y, size (square).
const MAP: (f64, f64, f64) = (684.0, 70.0, 172.0);
const DISPLAY: (f64, f64, f64, f64, f64) = (350.0, 160.0, 276.0, 112.0, 10.0);
/// The memory grid: origin, cell pitch, cell size, columns.
const GRID: (f64, f64, f64, f64) = (96.0, 76.0, 19.0, 14.0);
const GRID_COLS: usize = 8;
const SLOTS: usize = 64;
/// The hearing column: x of the bars' left end, top y, pitch, max length.
const HEAR: (f64, f64, f64, f64) = (268.0, 76.0, 12.4, 30.0);
const NOTES: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
/// The neuron field: x, y, height (width = height × aspect).
const FIELD: (f64, f64, f64) = (314.0, 72.0, 150.0);
const STRIPS: [(f64, &str); 2] = [(246.0, "RECALL"), (260.0, "NOVELTY")];
const STRIP_X: (f64, f64) = (206.0, 594.0);
const STRIP_SEGS: usize = 32;
const REACH: f64 = 0.22;

const KNOB_Y: f64 = 352.0;
const KNOB_R: f64 = 19.0;
const KNOBS: [(f64, &str); 13] = [
    (70.0, "SIZE"), (136.7, "DECAY"), (203.3, "TONE"), (270.0, "SPACE"), (336.7, "STEER"), (403.3, "DRIFT"),
    (470.0, "RECALL"), (536.7, "FOCUS"), (603.3, "DÉJÀ"), (670.0, "MATCH"), (736.7, "TEXTURE"), (803.3, "PLASTIC"),
    (870.0, "MIX"),
];
const LOG_SLOT: usize = 1;

/// A footswitch: centre, lamp, legend.
type Foot = ((f64, f64), (f64, f64), &'static str);
const FOOTS: [Foot; 2] =
    [((106.0, 452.0), (156.0, 452.0), "BYPASS"), ((834.0, 452.0), (784.0, 452.0), "HOLD")];
const FOOT_R: f64 = 20.0;
const FORGET: (f64, f64) = (660.0, 448.0);
const FORGET_R: f64 = 10.0;
const SCREWS: [(f64, f64); 4] = [(50.0, 46.0), (890.0, 46.0), (50.0, 474.0), (890.0, 474.0)];
const TITLE_X: f64 = 450.0;

const P_BYPASS: usize = 13;
const P_HOLD: usize = 14;
const P_FORGET: usize = 15;
const P_SPACE_X: usize = 16;
const P_SPACE_Y: usize = 17;
const P_NODES: usize = 18;
/// The PULSE lamp, beside the HEARING label.
pub(crate) const PULSE_AT: (f64, f64) = (296.0, 66.0);
const LINES: usize = 16;

const PHOSPHOR: Color = Color::rgba(0.42, 1.0, 0.86, 1.0);
/// Percussive memories glow amber (VFDs came in both).
const AMBER: Color = Color::rgba(1.0, 0.72, 0.3, 1.0);
const ENGRAVE: Color = Color::rgba(0.86, 0.87, 0.95, 0.92);
const ENGRAVE_SOFT: Color = Color::rgba(0.86, 0.87, 0.95, 0.55);

fn phosphor(a: f64) -> Color {
    Color::rgba(PHOSPHOR.r, PHOSPHOR.g, PHOSPHOR.b, a.clamp(0.0, 1.0))
}

fn amber(a: f64) -> Color {
    Color::rgba(AMBER.r, AMBER.g, AMBER.b, a.clamp(0.0, 1.0))
}

fn knob_angle(v: f64) -> f64 {
    (135.0 + 270.0 * v.clamp(0.0, 1.0)).to_radians()
}

fn norm(slot: usize, plain: f64, (lo, hi): (f64, f64)) -> f64 {
    if slot == LOG_SLOT {
        (plain / lo).ln() / (hi / lo).ln()
    } else {
        (plain - lo) / (hi - lo)
    }
}

fn plain(slot: usize, n: f64, (lo, hi): (f64, f64)) -> f64 {
    let n = n.clamp(0.0, 1.0);
    if slot == LOG_SLOT {
        lo * (hi / lo).powf(n)
    } else {
        lo + (hi - lo) * n
    }
}

pub(crate) fn to_screen(u: f64, v: f64) -> Pt {
    let (x0, y0, h) = FIELD;
    pt(x0 + u * h * FIELD_ASPECT, y0 + v * h)
}

/// A map point (0..1, 0..1) on screen.
pub(crate) fn map_screen(u: f64, v: f64) -> Pt {
    let (x0, y0, s) = MAP;
    pt(x0 + u * s, y0 + v * s)
}

/// Memory cell `k`'s rect.
pub(crate) fn cell(k: usize) -> (f64, f64, f64, f64) {
    let (x0, y0, pitch, size) = GRID;
    let (c, r) = ((k % GRID_COLS) as f64, (k / GRID_COLS) as f64);
    (x0 + c * pitch, y0 + r * pitch, size, size)
}

/// Hearing bar `c`'s slot rect (full length).
pub(crate) fn hear_slot(c: usize) -> (f64, f64, f64, f64) {
    let (x0, y0, pitch, len) = HEAR;
    (x0, y0 + c as f64 * pitch, len, pitch - 4.0)
}

pub(crate) fn strip_segment(k: usize, s: usize) -> (f64, f64, f64, f64) {
    let (y, _) = STRIPS[k];
    let pitch = (STRIP_X.1 - STRIP_X.0) / STRIP_SEGS as f64;
    (STRIP_X.0 + s as f64 * pitch, y - 3.0, pitch - 2.2, 6.0)
}

fn text(sc: &mut Scene, at: Pt, s: &str, size: f64, color: Color) {
    sc.ops.push(Op::Text { at, text: s.into(), size, color });
}

/// The display's decoded feed.
struct Feed {
    energy: Vec<f64>,
    pos: Vec<f64>,
    hearing: Vec<f64>,
    attn: Vec<f64>,
    filled: u64,
    percussive: u64,
    recalling: f64,
    novelty: f64,
    pulse: f64,
    tonal: f64,
    /// The room on the map, where memory pulls it, how hard; the nearest
    /// real space; the T60 (s); every memory's room on the map.
    live: (f64, f64),
    pull_at: (f64, f64),
    pull: f64,
    near: usize,
    t60: f64,
    heard_at: Vec<f64>,
}

fn feed(inp: &SceneInputs<'_>) -> Feed {
    let probe = |k: usize| inp.probes.get(k).copied().unwrap_or(0.0);
    let gather = |from: usize, taps: usize, per: usize, bits: u32| -> Vec<f64> {
        (0..taps).flat_map(|t| unpack(probe(from + t), per, bits)).collect()
    };
    let lo = probe(TAP_FILLED);
    let hi = probe(TAP_FILLED + 1);
    let as_bits = |v: f64| if v.is_finite() && v >= 0.0 { v as u64 & 0xFFFF_FFFF } else { 0 };
    Feed {
        energy: gather(TAP_ENERGY, 4, 4, 13),
        pos: gather(TAP_POS, 8, 4, 13),
        hearing: gather(TAP_HEARING, 3, 4, 13),
        attn: gather(TAP_ATTN, 11, 6, 8),
        filled: as_bits(lo) | (as_bits(hi) << 32),
        percussive: as_bits(probe(TAP_PERC)) | (as_bits(probe(TAP_PERC + 1)) << 32),
        pulse: probe(TAP_PULSE).clamp(0.0, 1.0),
        tonal: probe(TAP_TONAL).clamp(0.0, 1.0),
        recalling: probe(TAP_RECALLING).clamp(0.0, 1.0),
        novelty: probe(TAP_NOVELTY).clamp(0.0, 1.0),
        live: { let s = unpack(probe(TAP_SPACE), 4, 13); (s[0], s[1]) },
        pull_at: { let s = unpack(probe(TAP_SPACE), 4, 13); (s[2], s[3]) },
        pull: unpack(probe(TAP_SPACE_INFO), 3, 13)[1],
        near: ((unpack(probe(TAP_SPACE_INFO), 3, 13)[0] * (SPACES - 1) as f64).round() as usize).min(SPACES - 1),
        t60: 0.1 * 300f64.powf(unpack(probe(TAP_SPACE_INFO), 3, 13)[2]),
        heard_at: gather(TAP_HEARD_AT, 22, 6, 8),
    }
}

/// A node's drawn position (params) and live one (the feed; drawn
/// until the engine has spoken).
fn node_at(inp: &SceneInputs<'_>, f: &Feed, i: usize) -> ((f64, f64), (f64, f64)) {
    let drawn = (inp.params[P_NODES + 2 * i], inp.params[P_NODES + 2 * i + 1]);
    let (x, y) = (f.pos.get(2 * i).copied().unwrap_or(0.0), f.pos.get(2 * i + 1).copied().unwrap_or(0.0));
    let live = if x != 0.0 || y != 0.0 { (x, y) } else { drawn };
    (drawn, live)
}

pub fn build(inp: &SceneInputs<'_>) -> Scene {
    let mut sc = Scene::new();
    let art = render::art();
    sc.ops.push(Op::Image { at: pt(0.0, 0.0), w: W, h: H, image: art.face.clone() });
    let f = feed(inp);

    // MEMORY: the bank, cell by cell — held, and attended.
    text(&mut sc, pt(GRID.0 + 4.0 * GRID.2 - 2.5, 68.0), "MEMORY", 7.0, phosphor(0.6));
    for k in 0..SLOTS {
        let held = f.filled >> k & 1 == 1;
        if !held {
            continue;
        }
        let a = f.attn.get(k).copied().unwrap_or(0.0);
        let ink: fn(f64) -> Color = if f.percussive >> k & 1 == 1 { amber } else { phosphor };
        let (x, y, w, h) = cell(k);
        if a > 0.02 {
            sc.solid(Path::rect(pt(x - 2.5, y - 2.5), w + 5.0, h + 5.0), ink(0.25 * a));
        }
        sc.solid(Path::rect(pt(x, y), w, h), ink(0.18 + 0.82 * a));
    }

    // HEARING: the query's twelve pitch classes.
    text(&mut sc, pt(HEAR.0 + 12.0, 68.0), "HEARING", 7.0, phosphor(0.6));
    if f.pulse > 0.05 {
        sc.solid(Path::circle(pt(PULSE_AT.0, PULSE_AT.1), 4.5), amber(0.3 * f.pulse));
        sc.solid(Path::circle(pt(PULSE_AT.0, PULSE_AT.1), 2.2), amber(0.4 + 0.6 * f.pulse));
    }
    // Harmony dims as what you play turns percussive (it counts less).
    let tonal_ink = 0.3 + 0.7 * f.tonal;
    for c in 0..12 {
        let (x, y, len, h) = hear_slot(c);
        let v = f.hearing.get(c).copied().unwrap_or(0.0);
        if v > 0.02 {
            sc.solid(Path::rect(pt(x, y), len * v, h), phosphor((0.35 + 0.65 * v) * tonal_ink));
        }
        text(&mut sc, pt(x - 7.0, y + h - 1.0), NOTES[c], 5.5, phosphor(0.4));
    }

    // The NEURONS: edges by closeness, anchors and tethers, glow.
    let nodes: Vec<((f64, f64), (f64, f64))> = (0..LINES).map(|i| node_at(inp, &f, i)).collect();
    for i in 0..LINES {
        for j in i + 1..LINES {
            let (a, b) = (nodes[i].1, nodes[j].1);
            let d = ((a.0 - b.0) * FIELD_ASPECT).hypot(a.1 - b.1) / REACH;
            let c = (-d * d).exp();
            if c > 0.06 {
                let lit = (f.energy[i] * f.energy[j]).sqrt();
                sc.stroke(
                    Path::new().move_to(to_screen(a.0, a.1)).line_to(to_screen(b.0, b.1)),
                    Stroke::round(phosphor(0.10 + 0.25 * c + 0.45 * c * lit), 0.6 + 1.4 * c),
                );
            }
        }
    }
    for (drawn, live) in &nodes {
        let (pd, pl) = (to_screen(drawn.0, drawn.1), to_screen(live.0, live.1));
        if (pd.x - pl.x).hypot(pd.y - pl.y) > 2.0 {
            sc.stroke(Path::new().move_to(pd).line_to(pl), Stroke::round(phosphor(0.3), 0.6));
            sc.stroke(Path::circle(pd, 3.5), Stroke::new(phosphor(0.35), 0.8));
        }
    }
    for (i, (_, live)) in nodes.iter().enumerate() {
        let p = to_screen(live.0, live.1);
        let e = f.energy[i];
        sc.solid(Path::circle(p, 6.0 + 8.0 * e), phosphor(0.06 + 0.22 * e));
        sc.solid(Path::circle(p, 3.0 + 1.5 * e), phosphor(0.55 + 0.45 * e));
        sc.solid(Path::circle(p, 1.3), Color::rgba(0.9, 1.0, 0.97, 0.6 + 0.4 * e));
    }
    text(&mut sc, pt(FIELD.0 + 0.5 * FIELD.2 * FIELD_ASPECT, 68.0), "NEURONS  ·  DRAG THEM", 7.0, phosphor(0.6));

    // RECALL and NOVELTY strips.
    for (k, value) in [f.recalling, f.novelty].iter().enumerate() {
        let e = value * STRIP_SEGS as f64;
        for s in 0..STRIP_SEGS {
            let on = (e - s as f64).clamp(0.0, 1.0);
            if on > 0.05 {
                let (x, y, w, h) = strip_segment(k, s);
                sc.solid(Path::rect(pt(x - 1.5, y - 1.5), w + 3.0, h + 3.0), phosphor(0.22 * on));
                sc.solid(Path::rect(pt(x, y), w, h), phosphor(0.35 + 0.65 * on));
            }
        }
        let (y, label) = STRIPS[k];
        text(&mut sc, pt(STRIP_X.0 - 38.0, y + 3.0), label, 7.5, phosphor(0.75));
    }
    let (dx, dy, dhw, dhh, _) = DISPLAY;
    sc.ops.push(Op::Image { at: pt(dx - dhw, dy - dhh), w: 2.0 * dhw, h: 2.0 * dhh, image: art.glass.clone() });

    space(&mut sc, inp, &f);
    let (mx, my, mhw, mhh, _) = MAP_WINDOW;
    sc.ops.push(Op::Image { at: pt(mx - mhw, my - mhh), w: 2.0 * mhw, h: 2.0 * mhh, image: art.map_glass.clone() });

    // Knobs.
    for (k, &(x, label)) in KNOBS.iter().enumerate() {
        let a = knob_angle(norm(k, inp.params[k], inp.ranges[k]));
        let (cx, cy) = (a.cos(), a.sin());
        sc.stroke(
            Path::new().move_to(pt(x + cx * 6.0, KNOB_Y + cy * 6.0)).line_to(pt(x + cx * (KNOB_R - 4.0), KNOB_Y + cy * (KNOB_R - 4.0))),
            Stroke::round(Color::rgba(0.1, 0.12, 0.3, 0.95), 2.2),
        );
        text(&mut sc, pt(x, KNOB_Y + KNOB_R + 19.0), label, 8.5, ENGRAVE);
    }
    let on = [inp.params[P_BYPASS] < 0.5, inp.params[P_HOLD] > 0.5];
    let (lw, lh) = render::LAMP_SPRITE;
    for (k, &(_, (lx, ly), label)) in FOOTS.iter().enumerate() {
        if on[k] {
            let img = if k == 0 { &art.lamp_red } else { &art.lamp_cyan };
            sc.ops.push(Op::Image { at: pt(lx - lw / 2.0, ly - lh / 2.0), w: lw, h: lh, image: img.clone() });
        }
        text(&mut sc, pt(lx, ly + 20.0), label, 8.5, ENGRAVE);
    }
    text(&mut sc, pt(FORGET.0, FORGET.1 + FORGET_R + 12.0), "FORGET", 7.5, ENGRAVE);
    text(&mut sc, pt(TITLE_X, 458.0), "engram", 30.0, ENGRAVE);
    text(&mut sc, pt(TITLE_X, 476.0), "a room that remembers where it heard you", 8.5, ENGRAVE_SOFT);
    sc
}

/// The families' label spots: each family's median position, skipping
/// any too close to one already placed (computed once).
fn family_labels() -> &'static [(f64, f64, &'static str)] {
    static LABELS: std::sync::OnceLock<Vec<(f64, f64, &'static str)>> = std::sync::OnceLock::new();
    LABELS.get_or_init(|| {
        let mut fams: Vec<(usize, Vec<(f64, f64)>)> = (0..FAMILIES.len())
            .map(|k| (k, (0..SPACES).filter(|&i| MAP_FAMILY[i] as usize == k).map(|i| (f64::from(MAP_XY[2 * i]), f64::from(MAP_XY[2 * i + 1]))).collect()))
            .collect();
        fams.sort_by_key(|(_, pts)| core::cmp::Reverse(pts.len()));
        let median = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let mut out: Vec<(f64, f64, &'static str)> = Vec::new();
        for (k, pts) in fams {
            if pts.len() < 12 {
                continue;
            }
            let (x, y) = (median(pts.iter().map(|p| p.0).collect()), median(pts.iter().map(|p| p.1).collect()));
            if out.iter().all(|&(ox, oy, _)| (ox - x).hypot((oy - y) * 2.5) > 0.14) {
                out.push((x, y, FAMILIES[k]));
            }
        }
        out
    })
}

/// The SPACE window: labels, every memory's room, the cursor, the live
/// room and memory's pull, the readout.
fn space(sc: &mut Scene, inp: &SceneInputs<'_>, f: &Feed) {
    let (x0, y0, s) = MAP;
    text(sc, pt(x0 + s / 2.0, y0 - 6.0), "SPACE  ·  CLICK A ROOM", 7.0, phosphor(0.6));
    for &(x, y, name) in family_labels() {
        text(sc, map_screen(x, y), name, 5.5, phosphor(0.32));
    }
    // Memories, at the rooms they were heard in.
    for k in 0..SLOTS {
        if f.filled >> k & 1 == 0 {
            continue;
        }
        let (u, v) = (f.heard_at.get(2 * k).copied().unwrap_or(0.0), f.heard_at.get(2 * k + 1).copied().unwrap_or(0.0));
        let a = f.attn.get(k).copied().unwrap_or(0.0);
        let ink: fn(f64) -> Color = if f.percussive >> k & 1 == 1 { amber } else { phosphor };
        let p = map_screen(u, v);
        if a > 0.05 {
            sc.solid(Path::circle(p, 3.0 + 4.0 * a), ink(0.2 * a));
        }
        sc.solid(Path::rect(pt(p.x - 1.2, p.y - 1.2), 2.4, 2.4), ink(0.3 + 0.7 * a));
    }
    // The cursor (where you put it) and the live room (where it is).
    let cur = map_screen(inp.params[P_SPACE_X], inp.params[P_SPACE_Y]);
    let live = if f.live == (0.0, 0.0) { cur } else { map_screen(f.live.0, f.live.1) };
    if (cur.x - live.x).hypot(cur.y - live.y) > 3.0 {
        sc.stroke(Path::new().move_to(cur).line_to(live), Stroke::round(phosphor(0.25), 0.6));
    }
    sc.stroke(Path::circle(cur, 6.5), Stroke::new(phosphor(0.75), 0.9));
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        sc.stroke(Path::new().move_to(pt(cur.x + 8.5 * dx, cur.y + 8.5 * dy)).line_to(pt(cur.x + 12.0 * dx, cur.y + 12.0 * dy)), Stroke::round(phosphor(0.6), 0.9));
    }
    if f.pull > 0.02 {
        let at = map_screen(f.pull_at.0, f.pull_at.1);
        sc.stroke(Path::new().move_to(live).line_to(at), Stroke::round(amber(0.25 + 0.6 * f.pull), 0.6 + 1.2 * f.pull));
        sc.stroke(Path::circle(at, 3.0 + 3.0 * f.pull), Stroke::new(amber(0.4 + 0.6 * f.pull), 0.9));
    }
    sc.solid(Path::circle(live, 9.0), phosphor(0.16));
    sc.solid(Path::circle(live, 3.4), phosphor(0.95));
    sc.solid(Path::circle(live, 1.4), Color::rgba(0.9, 1.0, 0.97, 1.0));
    // The readout: the nearest real space, and the decay (once the
    // engine has spoken).
    if f.live != (0.0, 0.0) {
        text(sc, pt(x0 + s / 2.0, y0 + s + 10.0), NAMES[f.near], 5.5, phosphor(0.7));
        text(sc, pt(x0 + s / 2.0, y0 + s + 18.0), &format!("T60 {:.1} s", f.t60), 5.5, phosphor(0.5));
    }
}

/// The XY hit-map: the SPACE map (click puts the cursor, dragging moves
/// it), else the nearest neuron (within 14 pt).
pub fn drag_node(inp: &SceneInputs<'_>, from: Pt, now: Pt) -> Option<[(usize, f64); 2]> {
    let (mx, my, ms) = MAP;
    if from.x >= mx - 6.0 && from.x <= mx + ms + 6.0 && from.y >= my - 6.0 && from.y <= my + ms + 6.0 {
        return Some([(P_SPACE_X, ((now.x - mx) / ms).clamp(0.0, 1.0)), (P_SPACE_Y, ((now.y - my) / ms).clamp(0.0, 1.0))]);
    }
    let f = feed(inp);
    let (_, _, h) = FIELD;
    let mut best: Option<(usize, f64)> = None;
    for i in 0..LINES {
        let (drawn, live) = node_at(inp, &f, i);
        for spot in [live, drawn] {
            let p = to_screen(spot.0, spot.1);
            let d = (p.x - from.x).hypot(p.y - from.y);
            if d <= 14.0 && best.is_none_or(|(_, b)| d < b) {
                best = Some((i, d));
            }
        }
    }
    let (i, _) = best?;
    let (sx, sy) = (P_NODES + 2 * i, P_NODES + 2 * i + 1);
    let nx = (inp.params[sx] + (now.x - from.x) / (h * FIELD_ASPECT)).clamp(0.0, 1.0);
    let ny = (inp.params[sy] + (now.y - from.y) / h).clamp(0.0, 1.0);
    Some([(sx, nx), (sy, ny)])
}

/// The one-param hit-map: knobs, footswitches, FORGET.
pub fn drag(inp: &SceneInputs<'_>, from: Pt, now: Pt) -> Option<(usize, f64)> {
    for (k, &(x, _)) in KNOBS.iter().enumerate() {
        if (from.x - x).hypot(from.y - KNOB_Y) <= KNOB_R + 6.0 {
            let v = norm(k, inp.params[k], inp.ranges[k]);
            let moved = v + (from.y - now.y) / 200.0 + (now.x - from.x) / 400.0;
            return Some((k, plain(k, moved, inp.ranges[k])));
        }
    }
    for (k, &((fx, fy), _, _)) in FOOTS.iter().enumerate() {
        if (from.x - fx).hypot(from.y - fy) <= FOOT_R + 4.0 {
            let slot = if k == 0 { P_BYPASS } else { P_HOLD };
            return Some((slot, if inp.params[slot] > 0.5 { 0.0 } else { 1.0 }));
        }
    }
    if (from.x - FORGET.0).hypot(from.y - FORGET.1) <= FORGET_R + 4.0 {
        // Any change of the FORGET param is a press (the engine clears
        // on the edge), so every click flips it.
        return Some((P_FORGET, if inp.params[P_FORGET] > 0.5 { 0.0 } else { 1.0 }));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_ui::scene::ScenePalette;

    fn ranges() -> Vec<(f64, f64)> {
        // size decay tone space steer drift recall focus déjà match texture plastic mix
        let mut r = vec![(0.0, 1.0), (0.3, 30.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 60.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0)];
        // bypass hold forget, space x/y, the nodes
        r.extend(std::iter::repeat_n((0.0, 1.0), 3 + 2 + 32));
        r
    }

    fn params() -> Vec<f64> {
        let mut p = vec![0.5, 3.0, 0.6, 1.0, 0.5, 0.15, 0.5, 0.6, 4.0, 0.3, 0.15, 0.3, 0.35, 0.0, 0.0, 0.0, 0.5, 0.5];
        for i in 0..LINES {
            let (x, y) = crate::params::default_node(i);
            p.push(x);
            p.push(y);
        }
        p
    }

    fn inputs<'a>(params: &'a [f64], ranges: &'a [(f64, f64)], probes: &'a [f64]) -> SceneInputs<'a> {
        let c = Color::rgba(0.0, 0.0, 0.0, 1.0);
        SceneInputs { params, ranges, probes, w: W, h: H, palette: ScenePalette { accent: c, ink_hi: c, ink_muted: c, axis: c, bg: c } }
    }

    #[test]
    fn the_panels_fit_the_display_and_do_not_collide() {
        let (cx, cy, hw, hh, _) = DISPLAY;
        let inside = |(x, y, w, h): (f64, f64, f64, f64)| x > cx - hw && x + w < cx + hw && y > cy - hh && y + h < cy + hh;
        assert!(inside(cell(0)) && inside(cell(SLOTS - 1)));
        assert!(inside(hear_slot(0)) && inside(hear_slot(11)));
        let (gx, _, gw, _) = cell(GRID_COLS - 1);
        let (hx, _, hl, _) = hear_slot(0);
        let field_l = to_screen(0.0, 0.0);
        let field_r = to_screen(1.0, 1.0);
        assert!(gx + gw < hx - 8.0 && hx + hl < field_l.x, "grid / hearing / field overlap");
        assert!(field_r.x < cx + hw && field_r.y < STRIPS[0].0 - 6.0);
        let (_, _, _, ch) = cell(SLOTS - 1);
        assert!(cell(SLOTS - 1).1 + ch < STRIPS[0].0 - 6.0);
    }

    #[test]
    fn packed_feed_round_trips() {
        let vals = [0.0, 0.25, 0.5, 1.0];
        let back = unpack(crate::pack(&vals, 13), 4, 13);
        for (a, b) in vals.iter().zip(&back) {
            assert!((a - b).abs() < 2e-4);
        }
        let six = [0.1, 0.9, 0.0, 1.0, 0.5, 0.33];
        let back = unpack(crate::pack(&six, 8), 6, 8);
        for (a, b) in six.iter().zip(&back) {
            assert!((a - b).abs() < 3e-3);
        }
    }

    #[test]
    fn attention_lights_the_memory_it_is_paying_attention_to() {
        let (p, r) = (params(), ranges());
        let mut probes = vec![0.0; crate::FACE_TAPS.len()];
        probes[TAP_FILLED] = 0b11 as f64; // slots 0 and 1 hold memories
        let held = build(&inputs(&p, &r, &probes));
        probes[TAP_ATTN] = crate::pack(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0], 8); // slot 0 attended
        let attended = build(&inputs(&p, &r, &probes));
        assert_eq!(attended.ops.len(), held.ops.len() + 2, "an attended memory glows in its cell and at its room on the map");
    }

    #[test]
    fn percussive_memories_glow_amber() {
        let (p, r) = (params(), ranges());
        let mut probes = vec![0.0; crate::FACE_TAPS.len()];
        probes[TAP_FILLED] = 0b1 as f64;
        let cyan = build(&inputs(&p, &r, &probes));
        probes[TAP_PERC] = 0b1 as f64;
        let warm = build(&inputs(&p, &r, &probes));
        let colours = |s: &Scene| s.ops.iter().filter_map(|o| match o { Op::Fill { brush: stave_ui::scene::Brush::Solid(c), .. } => Some((c.r * 1000.0) as i32), _ => None }).collect::<Vec<_>>();
        assert_ne!(colours(&cyan), colours(&warm));
    }

    #[test]
    fn neurons_drag_in_pairs_and_forget_clicks() {
        let (p, r) = (params(), ranges());
        let inp = inputs(&p, &r, &[]);
        let at = to_screen(p[P_NODES + 6], p[P_NODES + 7]);
        let [(sx, _), (sy, _)] = drag_node(&inp, at, pt(at.x + 20.0, at.y)).unwrap();
        assert_eq!((sx, sy), (P_NODES + 6, P_NODES + 7));
        assert_eq!(drag(&inp, pt(FORGET.0, FORGET.1), pt(FORGET.0, FORGET.1)), Some((P_FORGET, 1.0)));
        let (x, _) = KNOBS[8];
        let (slot, v) = drag(&inp, pt(x, KNOB_Y), pt(x, KNOB_Y - 1000.0)).unwrap();
        assert_eq!(slot, 8);
        assert!((v - 60.0).abs() < 1e-9, "DÉJÀ tops out at 60 s");
    }

    /// A LIVE face for eyes: memories over the map (some attended, some
    /// percussive), the room trailing the cursor and pulled by memory.
    /// Writes build/engram-face-live.ppm.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn preview_live() {
        let (mut p, r) = (params(), ranges());
        p[P_SPACE_X] = 0.3;
        p[P_SPACE_Y] = 0.35;
        let mut probes = vec![0.6; crate::FACE_TAPS.len()];
        probes[TAP_FILLED] = 0xFFFF_FFFFu32 as f64;
        probes[TAP_FILLED + 1] = 0x0000_0FFFu32 as f64;
        probes[TAP_PERC] = 0x0F0F_0000u32 as f64;
        probes[TAP_PERC + 1] = 0.0;
        let mut heard = Vec::new();
        for k in 0..64 {
            let a = k as f64 * 2.399;
            let rr = 0.08 + 0.25 * ((k * 37 % 64) as f64 / 64.0);
            heard.push((0.55 + rr * a.cos()).clamp(0.0, 1.0));
            heard.push((0.5 + rr * a.sin()).clamp(0.0, 1.0));
        }
        for g in 0..22 {
            let v: Vec<f64> = (0..6).map(|j| heard.get(6 * g + j).copied().unwrap_or(0.0)).collect();
            probes[TAP_HEARD_AT + g] = crate::pack(&v, 8);
        }
        for g in 0..11 {
            let v: Vec<f64> = (0..6).map(|j| if (6 * g + j) % 9 == 0 { 0.9 } else { 0.05 }).collect();
            probes[TAP_ATTN + g] = crate::pack(&v, 8);
        }
        probes[TAP_SPACE] = crate::pack(&[0.4, 0.42, 0.62, 0.55], 13);
        probes[TAP_SPACE_INFO] = crate::pack(&[400.0 / (SPACES - 1) as f64, 0.6, (2.4f64 / 0.1).ln() / 300f64.ln()], 13);
        let scene = build(&inputs(&p, &r, &probes));
        let px = stave_ui_cocoa::offscreen::render_cg(&scene, W, H, 2.0);
        let (w, h) = ((W * 2.0) as usize, (H * 2.0) as usize);
        let mut out = format!("P6 {w} {h} 255\n").into_bytes();
        for c in px.chunks(4) {
            out.extend_from_slice(&c[..3]);
        }
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/engram-face-live.ppm"), out).unwrap();
    }

    #[test]
    fn the_space_window_holds_the_map_and_the_case_holds_everything() {
        let (cx, cy, hw, hh, _) = MAP_WINDOW;
        let (x0, y0, s) = MAP;
        assert!(x0 - 8.0 > cx - hw && x0 + s + 8.0 < cx + hw && y0 - 14.0 > cy - hh && y0 + s + 20.0 < cy + hh, "map / readout outside its window");
        let (dx, _, dhw, _, _) = DISPLAY;
        assert!(dx + dhw + 12.0 < cx - hw, "the windows collide");
        let (kx, ky, kw, _, _) = CASE;
        for &(x, _) in &KNOBS {
            assert!(x - KNOB_R - 10.0 > kx && x + KNOB_R + 10.0 < kx + kw);
        }
        for w in KNOBS.windows(2) {
            assert!(w[1].0 - w[0].0 > 2.0 * KNOB_R + 20.0, "knobs crowd");
        }
        assert!(ky + 20.0 < cy - hh);
    }

    #[test]
    fn clicking_the_map_puts_the_room_there() {
        let (p, r) = (params(), ranges());
        let inp = inputs(&p, &r, &[]);
        let at = map_screen(0.25, 0.75);
        let [(sx, x), (sy, y)] = drag_node(&inp, at, at).unwrap();
        assert_eq!((sx, sy), (P_SPACE_X, P_SPACE_Y));
        assert!((x - 0.25).abs() < 1e-9 && (y - 0.75).abs() < 1e-9);
        // Dragging off the edge holds at the edge.
        let [(_, x), _] = drag_node(&inp, at, pt(at.x - 1000.0, at.y)).unwrap();
        assert_eq!(x, 0.0);
    }

    #[test]
    fn memories_appear_on_the_map_where_they_were_heard() {
        let (p, r) = (params(), ranges());
        let mut probes = vec![0.0; crate::FACE_TAPS.len()];
        probes[TAP_SPACE] = crate::pack(&[0.4, 0.4, 0.0, 0.0], 13);
        let empty = build(&inputs(&p, &r, &probes));
        probes[TAP_FILLED] = 0b1 as f64;
        probes[TAP_HEARD_AT] = crate::pack(&[0.8, 0.2, 0.0, 0.0, 0.0, 0.0], 8);
        let one = build(&inputs(&p, &r, &probes));
        assert!(one.ops.len() > empty.ops.len(), "no memory drawn at its room");
    }
}
