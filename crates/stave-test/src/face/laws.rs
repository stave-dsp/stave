//! The laws every face keeps, checked at exact coordinates on every
//! page and step of the face (each [`Visibility`] state), through
//! [`FaceDriver`]: what a person's hands can rely on.
//!
//! * [`gestures`] (U3) — a press at a control's centre grabs it; a drag
//!   of the declared travel sweeps the whole range and Shift is ten
//!   times finer; double-click and Option-click reset; the wheel nudges;
//!   toggles step and wrap; options choose; latches flip; momentaries
//!   hold; pads put x/y exactly under the pointer; a press just outside a
//!   grab misses it; every edit is bracketed begin → end.
//! * [`layout`] (U4) — parts, grabs and text inside the face; grabs
//!   within their part; no two parts' grabs overlap; no two parts' text
//!   collides; every parameter reachable by some grab on some page.
//! * [`overlays`] (U5) — hovering or dragging a control paints it last
//!   (its readout on top) and steals no other grab; leaving it puts the
//!   face back exactly as it was; the hover target and cursor follow.
//! * [`sessions`] (U7) — seeded random pointer and key sessions keep
//!   every value in range, every edit bracketed, no drag stuck after a
//!   release and no hover after an exit; a failure prints its seed and
//!   the trace that replays it.

use stave::params::ParamId;
use stave_face::{step_of, Act, Area, Cursor, Gesture, Grab, Key, Mods, PartId, Visibility};
use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Pt};

use super::driver::{bracketed, by, FaceDriver};

/// What a plugin may leave off its face.
#[derive(Clone, Debug, Default)]
pub struct Laws {
    /// Parameters no grab moves, by name (host-only settings).
    pub off_face: &'static [&'static str],
}

/// Every law, on every state of the face `make` builds (a fresh face
/// per check, so no check sees another's edits).
pub fn assert_all(project: &str, make: impl Fn() -> FaceDriver, laws: &Laws) {
    layout(project, &make, laws);
    gestures(project, &make);
    overlays(project, &make);
    sessions(project, &make, 0..4, 120);
}

/// A fresh face in `state`, its setup edits forgotten.
fn fresh(make: &impl Fn() -> FaceDriver, state: &[Visibility]) -> FaceDriver {
    let mut ui = make();
    ui.show(state);
    ui.take_edits();
    ui
}

/// The grab of `part` with `gesture`, as it shows now (grabs can move
/// with their values).
fn find(ui: &FaceDriver, part: Option<PartId>, gesture: &Gesture) -> Option<Grab> {
    ui.runtime().all_grabs(ui.controller()).into_iter().find(|(p, _, g)| *p == part && g.gesture == *gesture).map(|(_, _, g)| g)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ------------------------------------------------------------ U3

/// The gesture laws (U3), on every state.
pub fn gestures(project: &str, make: &impl Fn() -> FaceDriver) {
    for state in make().states() {
        let shown = fresh(make, &state);
        let grabs = shown.runtime().all_grabs(shown.controller());
        for (part, _, grab) in grabs {
            let mut ui = fresh(make, &state);
            let here = format!("{project}: {part:?} {:?} in {state:?}", grab.gesture);
            gesture(&mut ui, part, &grab, &here);
            if let Err(e) = bracketed(&ui.edits()) {
                panic!("{here}: {e}\nedits: {:?}\ntrace:\n{}", ui.edits(), ui.trace());
            }
            just_outside(&ui, part, &grab, &here);
        }
    }
}

fn gesture(ui: &mut FaceDriver, part: Option<PartId>, grab: &Grab, here: &str) {
    let c = grab.area.centre();
    assert_eq!(ui.grab_at(c), Some((part, grab.gesture.clone())), "{here}: a press at its centre grabs something else");
    let regrab = |ui: &FaceDriver| find(ui, part, &grab.gesture).map(|g| g.area.centre());
    match grab.gesture.clone() {
        Gesture::Rotary { bind, travel } => {
            let id = bind.id;
            ui.set(id, ui.controller().param(id).expect("bound").denormalize(0.0));
            let c = regrab(ui).unwrap_or(c);
            ui.drag(c, by(0.0, -travel));
            assert!(close(ui.normalized(id), 1.0), "{here}: a drag of the travel ({travel} pt) from the bottom reads {}", ui.normalized(id));
            let c = regrab(ui).unwrap_or(c);
            ui.drag_with(c, by(0.0, travel), Mods { fine: true, ..Mods::default() });
            assert!(close(ui.normalized(id), 0.9), "{here}: a Shift-drag of the travel moves a tenth, reads {}", ui.normalized(id));
            let default = ui.controller().param(id).expect("bound").default_normalized();
            let c = regrab(ui).unwrap_or(c);
            ui.double_click(c);
            assert!(close(ui.normalized(id), default), "{here}: double-click resets");
            ui.set(id, ui.controller().param(id).expect("bound").denormalize(0.5));
            let c = regrab(ui).unwrap_or(c);
            ui.click_with(c, Mods { reset: true, ..Mods::default() });
            assert!(close(ui.normalized(id), default), "{here}: Option-click resets");
            ui.set(id, ui.controller().param(id).expect("bound").denormalize(0.5));
            let c = regrab(ui).unwrap_or(c);
            ui.wheel(c, 1.0);
            assert!(ui.normalized(id) > 0.5, "{here}: the wheel up raises");
        }
        Gesture::Stepped { bind, .. } => {
            let n = ui.controller().steps(bind.id).max(2);
            let start = step_of(ui.normalized(bind.id), n);
            for k in 1..=n {
                let c = regrab(ui).unwrap_or(c);
                ui.click(c);
                assert_eq!(step_of(ui.normalized(bind.id), n), (start + k) % n, "{here}: click {k} steps forward, wrapping");
            }
        }
        Gesture::Choose { bind, step } => {
            let n = ui.controller().steps(bind.id).max(2);
            ui.click(c);
            assert_eq!(step_of(ui.normalized(bind.id), n), step.min(n - 1), "{here}: an option chooses its step");
        }
        Gesture::Latch { bind } => {
            let start = ui.normalized(bind.id) >= 0.5;
            ui.click(c);
            assert_eq!(ui.normalized(bind.id) >= 0.5, !start, "{here}: a click flips");
            let c = regrab(ui).unwrap_or(c);
            ui.click(c);
            assert_eq!(ui.normalized(bind.id) >= 0.5, start, "{here}: a second click flips back");
        }
        Gesture::Momentary { bind } => {
            ui.press(c);
            assert_eq!(ui.normalized(bind.id), 1.0, "{here}: on while held");
            ui.release();
            assert_eq!(ui.normalized(bind.id), 0.0, "{here}: off on release");
        }
        Gesture::Pad { x, y, area } => {
            for (fx, fy) in [(0.5, 0.5), (0.25, 0.75), (0.0, 0.0), (1.0, 1.0), (0.9, 0.1)] {
                let p = pt(area.x + fx * area.w, area.y + (1.0 - fy) * area.h);
                if !grab.area.contains(p) {
                    continue;
                }
                ui.press(p);
                assert!(
                    close(ui.normalized(x.id), fx) && close(ui.normalized(y.id), fy),
                    "{here}: a press at ({fx}, {fy}) of the pad reads ({}, {})",
                    ui.normalized(x.id),
                    ui.normalized(y.id)
                );
                ui.release();
            }
        }
        Gesture::Nudge { bind, delta } => {
            let n = ui.controller().steps(bind.id).max(2);
            ui.show(&[Visibility::Step { param: bind.id, step: n / 2 }]);
            let Some(c) = regrab(ui) else { return };
            ui.click(c);
            let want = (i64::from(n / 2) + i64::from(delta)).clamp(0, i64::from(n) - 1) as u32;
            assert_eq!(step_of(ui.normalized(bind.id), n), want, "{here}: a nudge steps by {delta}");
        }
        Gesture::Preset { delta } => {
            let n = ui.controller().presets().len();
            if n > 0 {
                ui.click(c);
                let want = if delta > 0 { (delta as usize - 1) % n } else { (n as i64 + i64::from(delta)).rem_euclid(n as i64) as usize };
                assert_eq!(ui.controller().preset(), Some(want), "{here}: a preset button steps the presets");
            }
        }
        Gesture::Page { key, page } => {
            ui.click(c);
            assert_eq!(ui.page(key), page, "{here}: a page button shows its page");
        }
    }
}

/// Points just outside a grab's area: on a circle 1.5 pt past its rim,
/// at eight angles; past the middle of each side of a rect.
fn rim(area: &Area) -> Vec<Pt> {
    const OUT: f64 = 1.5;
    match *area {
        Area::Circle { c, r } => (0..8)
            .map(|k| {
                let a = f64::from(k) * core::f64::consts::FRAC_PI_4;
                pt(c.x + (r + OUT) * a.cos(), c.y + (r + OUT) * a.sin())
            })
            .collect(),
        Area::Rect(r) => vec![
            pt(r.x - OUT, r.y + r.h / 2.0),
            pt(r.x + r.w + OUT, r.y + r.h / 2.0),
            pt(r.x + r.w / 2.0, r.y - OUT),
            pt(r.x + r.w / 2.0, r.y + r.h + OUT),
        ],
    }
}

/// A press just outside a grab does not make that grab (unless another
/// of the same part's grabs with the same gesture covers the point).
fn just_outside(ui: &FaceDriver, part: Option<PartId>, grab: &Grab, here: &str) {
    let all = ui.runtime().all_grabs(ui.controller());
    for p in rim(&grab.area) {
        let covered = all.iter().any(|(q, _, g)| *q == part && g.gesture == grab.gesture && g.area.contains(p));
        if !covered {
            assert_ne!(ui.grab_at(p), Some((part, grab.gesture.clone())), "{here}: a press just outside it at {p:?} grabs it");
        }
    }
}

// ------------------------------------------------------------ U4

/// A circle's or rect's bounding box.
fn bounds(a: &Area) -> Rect {
    match *a {
        Area::Circle { c, r } => Rect { x: c.x - r, y: c.y - r, w: 2.0 * r, h: 2.0 * r },
        Area::Rect(r) => r,
    }
}

fn inside(inner: Rect, outer: Rect, slack: f64) -> bool {
    inner.x >= outer.x - slack
        && inner.y >= outer.y - slack
        && inner.x + inner.w <= outer.x + outer.w + slack
        && inner.y + inner.h <= outer.y + outer.h + slack
}

/// The two areas share some area (touching edges do not count).
fn overlap(a: &Area, b: &Area) -> bool {
    const EPS: f64 = 0.25;
    match (*a, *b) {
        (Area::Circle { c, r }, Area::Circle { c: d, r: s }) => (c.x - d.x).hypot(c.y - d.y) < r + s - EPS,
        (Area::Rect(p), Area::Rect(q)) => {
            p.x < q.x + q.w - EPS && q.x < p.x + p.w - EPS && p.y < q.y + q.h - EPS && q.y < p.y + p.h - EPS
        }
        (Area::Circle { c, r }, Area::Rect(q)) | (Area::Rect(q), Area::Circle { c, r }) => {
            let nx = c.x.clamp(q.x, q.x + q.w);
            let ny = c.y.clamp(q.y, q.y + q.h);
            (c.x - nx).hypot(c.y - ny) < r - EPS
        }
    }
}

fn boxes_overlap(p: Rect, q: Rect) -> bool {
    const EPS: f64 = 0.5;
    p.x < q.x + q.w - EPS && q.x < p.x + p.w - EPS && p.y < q.y + q.h - EPS && q.y < p.y + p.h - EPS
}

/// The layout laws (U4), on every state.
pub fn layout(project: &str, make: &impl Fn() -> FaceDriver, laws: &Laws) {
    let mut moved: Vec<ParamId> = Vec::new();
    let states = make().states();
    for state in &states {
        let ui = fresh(make, state);
        let (w, h) = ui.size();
        let face = Rect { x: 0.0, y: 0.0, w, h };
        let here = |what: &str| format!("{project}: in {state:?}, {what}");
        for (id, r) in ui.runtime().visible(ui.controller()) {
            assert!(inside(r, face, 0.5), "{}", here(&format!("part {id:?} at {r:?} lies outside the {w}×{h} face")));
        }
        let grabs = ui.runtime().all_grabs(ui.controller());
        for (i, (id, rect, g)) in grabs.iter().enumerate() {
            let b = bounds(&g.area);
            assert!(inside(b, face, 0.5), "{}", here(&format!("{id:?}'s {:?} grab at {b:?} lies outside the face", g.gesture)));
            assert!(inside(b, *rect, 0.5), "{}", here(&format!("{id:?}'s {:?} grab at {b:?} lies outside its part {rect:?}", g.gesture)));
            for (jd, _, other) in &grabs[i + 1..] {
                if jd != id {
                    assert!(
                        !overlap(&g.area, &other.area),
                        "{}",
                        here(&format!("{id:?}'s {:?} grab overlaps {jd:?}'s {:?}", g.gesture, other.gesture))
                    );
                }
            }
            for b in g.gesture.binds() {
                moved.push(b.id);
            }
        }
        let texts = ui.texts();
        for (i, t) in texts.iter().enumerate() {
            assert!(inside(t.bounds, face, 0.5), "{}", here(&format!("{:?}'s text {:?} runs outside the face", t.part, t.text)));
            for u in &texts[i + 1..] {
                if u.part != t.part && !t.text.trim().is_empty() && !u.text.trim().is_empty() {
                    assert!(
                        !boxes_overlap(t.bounds, u.bounds),
                        "{}",
                        here(&format!("{:?}'s text {:?} collides with {:?}'s {:?}", t.part, t.text, u.part, u.text))
                    );
                }
            }
        }
    }
    let ui = make();
    for (name, id) in ui.table_params() {
        let hidden = ui.controller().param(id).is_some_and(|p| p.meta().flags.hidden);
        if !hidden && !moved.contains(&id) && !laws.off_face.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            panic!("{project}: no grab on any page moves {name:?} (list it in `off_face` if it is host-only)");
        }
    }
}

// ------------------------------------------------------------ U5

/// Every parameter's value, by id.
fn values(ui: &FaceDriver) -> Vec<(ParamId, f64)> {
    ui.table_params().into_iter().map(|(_, id)| (id, ui.value(id))).collect()
}

/// Put every parameter back to `values` (as automation would), in its
/// own units: the exact values read, so a log-tapered value does not
/// round-trip through normalized and move its knob's pointer by an ulp.
fn restore(ui: &mut FaceDriver, values: &[(ParamId, f64)]) {
    for &(id, v) in values {
        ui.set(id, v);
    }
}

/// The overlay laws (U5), on every state. One face per state does the
/// touching and a second, untouched, is the reference picture (each
/// keeps its sprites warm, so a check costs a paint, not a bake).
pub fn overlays(project: &str, make: &impl Fn() -> FaceDriver) {
    for state in make().states() {
        let mut reference = fresh(make, &state);
        let idle_layers = reference.layers();
        let grabs = reference.runtime().all_grabs(reference.controller());
        let map = |ui: &FaceDriver| -> Vec<_> { grabs.iter().map(|(_, _, g)| ui.grab_at(g.area.centre())).collect() };
        let idle_map = map(&reference);
        let idle_values = values(&reference);
        let mut ui = fresh(make, &state);
        for (part, _, grab) in &grabs {
            let here = format!("{project}: {part:?} {:?} in {state:?}", grab.gesture);
            let c = grab.area.centre();
            ui.hover(c);
            assert_eq!(ui.hovered(), *part, "{here}: hovering its centre hovers it");
            assert_ne!(ui.cursor(), Cursor::Arrow, "{here}: the cursor shows it can be grabbed");
            assert_eq!(ui.layers().last().map(|l| l.id), Some(*part), "{here}: the hovered part paints last (on top)");
            assert_eq!(map(&ui), idle_map, "{here}: hovering it changes what other presses grab");
            ui.exit();
            assert_eq!((ui.hovered(), ui.cursor()), (None, Cursor::Arrow), "{here}: leaving the face clears the hover");
            assert!(ui.layers() == idle_layers, "{here}: after the pointer leaves, the face is not as it was");
            if let Gesture::Rotary { .. } = grab.gesture {
                ui.press(c).move_to(by(0.0, -10.0));
                assert_eq!(ui.active(), *part, "{here}: dragging it makes it active");
                assert_eq!(ui.layers().last().map(|l| l.id), Some(*part), "{here}: the dragged part paints last");
                assert_eq!(ui.cursor(), Cursor::Grabbing, "{here}: the cursor grabs");
                let others: Vec<_> = grabs.iter().filter(|(p, _, _)| p != part).map(|(_, _, g)| ui.grab_at(g.area.centre())).collect();
                let idle_others: Vec<_> =
                    grabs.iter().zip(&idle_map).filter(|((p, _, _), _)| p != part).map(|(_, m)| m.clone()).collect();
                assert_eq!(others, idle_others, "{here}: a drag's readout steals another part's grab");
                ui.release().exit();
                assert_eq!(ui.active(), None, "{here}: no drag after release");
                let moved = values(&ui);
                restore(&mut reference, &moved);
                assert!(ui.layers() == reference.layers(), "{here}: after release and exit the readout stays");
                restore(&mut reference, &idle_values);
                restore(&mut ui, &idle_values);
            }
        }
    }
}

// ------------------------------------------------------------ U7

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Random sessions (U7): for each seed, `steps` random events, the
/// invariants checked after each.
pub fn sessions(project: &str, make: &impl Fn() -> FaceDriver, seeds: core::ops::Range<u64>, steps: usize) {
    for seed in seeds {
        let mut ui = make();
        let mut rng = Rng::new(seed);
        let targets: Vec<Pt> = ui.runtime().all_grabs(ui.controller()).iter().map(|(_, _, g)| g.area.centre()).collect();
        let (w, h) = ui.size();
        let mut pressed = false;
        for step in 0..steps {
            let p = if !targets.is_empty() && rng.unit() < 0.7 {
                let t = targets[rng.below(targets.len())];
                pt(t.x + (rng.unit() - 0.5) * 8.0, t.y + (rng.unit() - 0.5) * 8.0)
            } else {
                pt(rng.unit() * w * 1.2 - w * 0.1, rng.unit() * h * 1.2 - h * 0.1)
            };
            let mods = Mods { fine: rng.below(4) == 0, reset: rng.below(10) == 0 };
            let act = match (pressed, rng.below(10)) {
                (false, 0..=2) => Act::Hover(p),
                (false, 3..=5) => Act::Press { at: p, mods, clicks: 1 + u32::from(rng.below(4) == 0) },
                (false, 6) => Act::Wheel { at: p, lines: (rng.unit() - 0.5) * 6.0, mods },
                (false, 7) => Act::Key {
                    key: [Key::Up, Key::Down, Key::Left, Key::Right, Key::Home, Key::End, Key::Escape][rng.below(7)],
                    mods,
                },
                (false, _) => Act::Exit,
                (true, 0..=5) => Act::Move { at: p, mods },
                (true, 6) => Act::Key { key: Key::Escape, mods },
                (true, _) => Act::Release,
            };
            ui.act(act);
            pressed = match act {
                Act::Press { .. } => true,
                Act::Release => false,
                _ => pressed,
            };
            let fail = |why: String| -> ! {
                panic!("{project}: session seed {seed}, step {step} ({act}): {why}\nreplay:\n{}", ui.trace());
            };
            for (name, id) in ui.table_params() {
                let n = ui.normalized(id);
                if !(0.0..=1.0).contains(&n) {
                    fail(format!("{name} at normalized {n}"));
                }
            }
            match act {
                Act::Release if ui.active().is_some() || ui.runtime().dragging().is_some() => fail("a drag is stuck after release".into()),
                Act::Exit if ui.runtime().dragging().is_none() && ui.runtime().hovered().is_some() => {
                    fail("the hover stays after the pointer left".into())
                }
                _ => {}
            }
            if !pressed || ui.runtime().dragging().is_none() {
                if let Err(e) = bracketed(&ui.edits()) {
                    fail(e);
                }
            }
        }
        ui.act(Act::Release);
        if let Err(e) = bracketed(&ui.edits()) {
            panic!("{project}: session seed {seed}, after the last release: {e}\nreplay:\n{}", ui.trace());
        }
    }
}
