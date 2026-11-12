//! Parameter restore against smoothing (`stave_core::params::control`):
//! a state load writes a param's value and then bumps its restore
//! counter; the audio thread's smoother, seeing the counter move, jumps
//! to the value instead of gliding from wherever it was — a restored
//! session must not sweep its knobs audibly into place.
//!
//! The law, over every interleaving of the two threads' atomic steps:
//! when the smoother takes a restore (it saw the counter move), it lands
//! on the restored value. [`ReadOrder::ValueFirst`] is the reader as
//! first written (S1): it reads the value, then the counter, so a write
//! landing between the two reads makes it snap to the OLD value and then
//! glide to the new one — the checker finds that interleaving.
//! [`ReadOrder::CounterFirst`] is the shipping reader.

use crate::check::Model;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ReadOrder {
    ValueFirst,
    CounterFirst,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    /// Shared: the param's value (0 = before the load, 1 = restored) and
    /// its restore counter.
    pub value: u8,
    pub counter: u8,
    /// Writer progress: 0 nothing, 1 value written, 2 counter bumped.
    pub writer: u8,
    /// Reader progress through its two reads, and what it read.
    pub reader: u8,
    pub seen_value: Option<u8>,
    pub seen_counter: Option<u8>,
    /// The value the smoother snapped to, if it took a restore.
    pub snapped: Option<u8>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    WriteValue,
    BumpCounter,
    ReaderStep,
}

pub struct Restore {
    pub order: ReadOrder,
}

impl Model for Restore {
    type State = State;
    type Action = Action;

    fn init(&self) -> Vec<State> {
        vec![State {
            value: 0,
            counter: 0,
            writer: 0,
            reader: 0,
            seen_value: None,
            seen_counter: None,
            snapped: None,
        }]
    }

    fn actions(&self, s: &State, out: &mut Vec<Action>) {
        match s.writer {
            0 => out.push(Action::WriteValue),
            1 => out.push(Action::BumpCounter),
            _ => {}
        }
        if s.reader < 2 {
            out.push(Action::ReaderStep);
        }
    }

    fn next(&self, s: &State, a: &Action) -> Option<State> {
        let mut n = s.clone();
        match a {
            Action::WriteValue if s.writer == 0 => {
                n.value = 1;
                n.writer = 1;
            }
            Action::BumpCounter if s.writer == 1 => {
                n.counter = 1;
                n.writer = 2;
            }
            Action::ReaderStep if s.reader < 2 => {
                let value_now = (self.order == ReadOrder::ValueFirst) == (s.reader == 0);
                if value_now {
                    n.seen_value = Some(s.value);
                } else {
                    n.seen_counter = Some(s.counter);
                }
                n.reader += 1;
                if n.reader == 2 && n.seen_counter == Some(1) {
                    n.snapped = n.seen_value;
                }
            }
            _ => return None,
        }
        Some(n)
    }

    fn invariant(&self, s: &State) -> Result<(), String> {
        match s.snapped {
            Some(v) if v != 1 => Err("took the restore but snapped to the old value".into()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;

    #[test]
    fn reading_the_value_first_can_snap_to_the_old_value() {
        let cx = check(&Restore { order: ReadOrder::ValueFirst }, 1_000).expect_err("S1");
        let actions: Vec<_> = cx.path.iter().map(|(a, _)| *a).collect();
        assert_eq!(
            actions,
            [Action::ReaderStep, Action::WriteValue, Action::BumpCounter, Action::ReaderStep],
            "{cx}"
        );
    }

    #[test]
    fn reading_the_counter_first_always_lands_on_the_restored_value() {
        let stats = check(&Restore { order: ReadOrder::CounterFirst }, 1_000).expect("the shipping order");
        assert!(stats.states > 5);
    }
}
