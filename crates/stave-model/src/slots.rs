//! Telemetry slot reservation (`stave_telemetry::slot`): a registrant
//! scans the published slots for its name, and if it is absent claims
//! the next index with a compare-exchange on the used count, writes the
//! name and publishes it (Release). Readers scan the published prefix.
//!
//! The laws, over every interleaving of two registrants: each index is
//! handed out once and the count never passes capacity (that a reader
//! never matches an unpublished name is `InlineStr`'s Release/Acquire,
//! which loom models).
//! What the protocol does NOT promise is one slot per name: two threads
//! registering the same name at once can both miss it in the scan and
//! claim two slots (the checker shows the interleaving). Taps register
//! in `Plugin::new`, on one thread, where it cannot happen; the tooling
//! path (`probe!`) tolerates it — readers see the first slot.

use crate::check::Model;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    /// The used count (the CAS target).
    pub used: u8,
    /// Per slot: who claimed it, and whether its name is published.
    pub owner: Vec<Option<u8>>,
    pub published: Vec<bool>,
    /// Per registrant: 0 scan, 1 claim (with the count it read), 2
    /// publish, 3 done; the slot it ended with.
    pub pc: Vec<u8>,
    pub read_used: Vec<u8>,
    pub slot: Vec<Option<u8>>,
    pub broken: Option<&'static str>,
}

/// One registrant's next atomic step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Step(u8),
}

pub struct Slots {
    pub registrants: u8,
    pub capacity: u8,
}

impl Model for Slots {
    type State = State;
    type Action = Action;

    fn init(&self) -> Vec<State> {
        let (r, c) = (self.registrants as usize, self.capacity as usize);
        vec![State {
            used: 0,
            owner: vec![None; c],
            published: vec![false; c],
            pc: vec![0; r],
            read_used: vec![0; r],
            slot: vec![None; r],
            broken: None,
        }]
    }

    fn actions(&self, s: &State, out: &mut Vec<Action>) {
        for r in 0..self.registrants {
            if s.pc[r as usize] < 3 {
                out.push(Action::Step(r));
            }
        }
    }

    fn next(&self, s: &State, a: &Action) -> Option<State> {
        let mut n = s.clone();
        match *a {
            Action::Step(r) => {
                let r = r as usize;
                match s.pc[r] {
                    // Scan: every registrant registers the same name, so a
                    // published slot is a hit.
                    0 => match (0..s.used as usize).find(|&i| s.published[i]) {
                        Some(i) => {
                            n.slot[r] = Some(i as u8);
                            n.pc[r] = 3;
                        }
                        None => {
                            n.read_used[r] = s.used;
                            n.pc[r] = 1;
                        }
                    },
                    // The compare-exchange: fails (re-read) if the count moved.
                    1 => {
                        let cur = s.read_used[r];
                        if cur >= self.capacity {
                            n.pc[r] = 3; // full: no slot
                        } else if s.used == cur {
                            n.used = cur + 1;
                            if n.owner[cur as usize].is_some() {
                                n.broken = Some("an index was handed out twice");
                            }
                            n.owner[cur as usize] = Some(r as u8);
                            n.slot[r] = Some(cur);
                            n.pc[r] = 2;
                        } else {
                            n.read_used[r] = s.used;
                        }
                    }
                    2 => {
                        n.published[s.slot[r].expect("claimed") as usize] = true;
                        n.pc[r] = 3;
                    }
                    _ => return None,
                }
            }
        }
        if n.used > self.capacity {
            n.broken = Some("the count passed capacity");
        }
        Some(n)
    }

    fn invariant(&self, s: &State) -> Result<(), String> {
        s.broken.map_or(Ok(()), |b| Err(b.into()))
    }
}

impl Slots {
    /// Two registrants of one name ended in two different slots.
    pub fn duplicated(s: &State) -> bool {
        let done: Vec<_> = s.slot.iter().flatten().collect();
        done.len() == 2 && done[0] != done[1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;

    #[test]
    fn indices_are_handed_out_once_and_the_count_holds_its_capacity() {
        for capacity in 1..=3 {
            check(&Slots { registrants: 2, capacity }, 100_000).expect("the reservation");
        }
    }

    /// Not a law: concurrent registration of one name can take two slots.
    #[test]
    fn concurrent_registration_of_one_name_can_take_two_slots() {
        let model = Slots { registrants: 2, capacity: 3 };
        let mut seen = false;
        let mut frontier = model.init();
        let mut actions = Vec::new();
        for _ in 0..8 {
            let mut next = Vec::new();
            for s in &frontier {
                seen |= Slots::duplicated(s);
                actions.clear();
                model.actions(s, &mut actions);
                next.extend(actions.iter().filter_map(|a| model.next(s, a)));
            }
            next.sort_by_key(|s| format!("{s:?}"));
            next.dedup();
            frontier = next;
        }
        assert!(seen);
    }
}
