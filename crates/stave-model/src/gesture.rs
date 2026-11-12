//! Edit gestures on their way from a face to a CLAP host: the UI thread
//! begins, performs and ends gestures on parameters into a bounded ring
//! (`stave_core::runtime::GestureQueue`), the audio thread drains the
//! ring into the host's out-events.
//!
//! The host-visible laws: a gesture is never begun twice or ended
//! unbegun (safety), and once the UI lets go and the ring drains, no
//! gesture is left open at the host (liveness) — a DAW with a gesture
//! stuck open keeps the parameter "touched" and records no automation
//! over it.
//!
//! [`Admission::DropNewest`] is the ring as first written: a full ring
//! refuses whatever comes next. The checker finds the trace where that
//! loses an `End` (begin, perform until full, release: the host's
//! gesture never closes, and the next press begins it twice). [`Admission::Reserved`] is the shipping rule
//! (each open gesture keeps a slot for its `End`; a refused `Begin`
//! refuses its `End` too) and is checked to keep both laws.

use crate::check::Model;

/// How a full (or nearly full) ring admits an event.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    /// Refuse whatever does not fit.
    DropNewest,
    /// `End` always fits (every admitted `Begin` reserves its slot); a
    /// `Begin` needs room for itself and its `End` beyond the open
    /// gestures' reserve; a `Value` needs room beyond that reserve; the
    /// `End` of a refused `Begin` is refused too.
    Reserved,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    Begin(u8),
    Value(u8),
    End(u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    /// The pointer holds parameter `p` (the UI's own bracket).
    pub ui_open: Vec<bool>,
    /// The producer admitted `p`'s `Begin` and not yet its `End`.
    pub admitted: Vec<bool>,
    pub ring: Vec<Event>,
    /// The host has seen `p`'s `Begin` and not yet its `End`.
    pub host_open: Vec<bool>,
    /// A safety law the host saw broken.
    pub broken: Option<&'static str>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Press(u8),
    Drag(u8),
    Release(u8),
    /// The audio thread drains one event to the host.
    Deliver,
}

pub struct Gestures {
    pub params: u8,
    pub capacity: usize,
    pub admission: Admission,
}

impl Gestures {
    fn push(&self, s: &mut State, e: Event) {
        let free = self.capacity - s.ring.len();
        let open = s.admitted.iter().filter(|&&a| a).count();
        let admit = match (self.admission, e) {
            (Admission::DropNewest, _) => free > 0,
            (Admission::Reserved, Event::Begin(_)) => free >= open + 2,
            (Admission::Reserved, Event::Value(_)) => free > open,
            (Admission::Reserved, Event::End(p)) => s.admitted[p as usize],
        };
        if !admit {
            return;
        }
        match e {
            Event::Begin(p) => s.admitted[p as usize] = true,
            Event::End(p) => s.admitted[p as usize] = false,
            Event::Value(_) => {}
        }
        s.ring.push(e);
    }
}

impl Model for Gestures {
    type State = State;
    type Action = Action;

    fn init(&self) -> Vec<State> {
        let n = self.params as usize;
        vec![State {
            ui_open: vec![false; n],
            admitted: vec![false; n],
            ring: Vec::new(),
            host_open: vec![false; n],
            broken: None,
        }]
    }

    fn actions(&self, s: &State, out: &mut Vec<Action>) {
        for p in 0..self.params {
            if s.ui_open[p as usize] {
                out.extend([Action::Drag(p), Action::Release(p)]);
            } else {
                out.push(Action::Press(p));
            }
        }
        if !s.ring.is_empty() {
            out.push(Action::Deliver);
        }
    }

    fn next(&self, s: &State, a: &Action) -> Option<State> {
        let mut n = s.clone();
        match *a {
            Action::Press(p) if !s.ui_open[p as usize] => {
                n.ui_open[p as usize] = true;
                self.push(&mut n, Event::Begin(p));
            }
            Action::Drag(p) if s.ui_open[p as usize] => self.push(&mut n, Event::Value(p)),
            Action::Release(p) if s.ui_open[p as usize] => {
                n.ui_open[p as usize] = false;
                self.push(&mut n, Event::End(p));
            }
            Action::Deliver if !s.ring.is_empty() => match n.ring.remove(0) {
                Event::Begin(p) if n.host_open[p as usize] => n.broken = Some("begun twice"),
                Event::Begin(p) => n.host_open[p as usize] = true,
                Event::End(p) if !n.host_open[p as usize] => n.broken = Some("ended unbegun"),
                Event::End(p) => n.host_open[p as usize] = false,
                Event::Value(_) => {}
            },
            _ => return None,
        }
        Some(n)
    }

    fn invariant(&self, s: &State) -> Result<(), String> {
        match s.broken {
            Some(why) => Err(format!("the host saw a gesture {why}")),
            None => Ok(()),
        }
    }

    fn quiescent(&self, s: &State) -> Option<bool> {
        Some(
            s.ring.is_empty()
                && !s.ui_open.iter().any(|&o| o)
                && !s.host_open.iter().any(|&o| o),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;

    /// The ring as first written: an `End` refused by a full ring
    /// leaves the host's gesture open, and the next press begins it
    /// again — the shortest witness, found by the checker.
    #[test]
    fn dropping_the_newest_loses_an_end_and_the_host_sees_a_gesture_begun_twice() {
        let model = Gestures {
            params: 1,
            capacity: 2,
            admission: Admission::DropNewest,
        };
        let cx = check(&model, 100_000).expect_err("the old ring loses an End");
        assert_eq!(cx.property, "the host saw a gesture begun twice", "{cx}");
        let actions: Vec<_> = cx.path.iter().map(|(a, _)| *a).collect();
        assert_eq!(
            actions[..3],
            [Action::Press(0), Action::Drag(0), Action::Release(0)],
            "{cx}"
        );
    }

    #[test]
    fn the_reserved_ring_keeps_every_gesture_bracketed_and_closes_it() {
        for params in 1..=3 {
            for capacity in 1..=5 {
                let model = Gestures {
                    params,
                    capacity,
                    admission: Admission::Reserved,
                };
                if let Err(cx) = check(&model, 1_000_000) {
                    panic!("{params} params, capacity {capacity}: {cx}");
                }
            }
        }
    }
}
