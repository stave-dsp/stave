//! An explicit-state model checker (TLC's method, in Rust, small):
//! breadth-first search over every state a [`Model`] can reach, checking
//! its invariants in each and — for liveness — that from every
//! reachable state a quiescent one stays reachable. A violation comes
//! back as the shortest action sequence that shows it.
//!
//! [`validate`] runs the other way: a trace recorded from a real run is
//! checked to be a behaviour of the model, step by step.

use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::hash::Hash;

/// A protocol as a state machine.
pub trait Model {
    type State: Clone + Eq + Hash + Debug;
    type Action: Clone + Debug;

    fn init(&self) -> Vec<Self::State>;
    /// Every action enabled in `state`.
    fn actions(&self, state: &Self::State, out: &mut Vec<Self::Action>);
    /// The state after `action`; `None` when it is not enabled.
    fn next(&self, state: &Self::State, action: &Self::Action) -> Option<Self::State>;
    /// A safety property every reachable state keeps.
    fn invariant(&self, _state: &Self::State) -> Result<(), String> {
        Ok(())
    }
    /// A state the protocol must always be able to return to (every
    /// begun gesture ended, every queued event delivered). `None`: no
    /// liveness property.
    fn quiescent(&self, _state: &Self::State) -> Option<bool> {
        None
    }
}

/// What a check explored.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    pub states: usize,
    pub transitions: usize,
}

/// A violation and the shortest way to it.
#[derive(Debug)]
pub struct Counterexample<S, A> {
    pub property: String,
    pub init: S,
    /// Actions from `init`, with the state after each.
    pub path: Vec<(A, S)>,
}

impl<S: Debug, A: Debug> std::fmt::Display for Counterexample<S, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "violated: {}", self.property)?;
        writeln!(f, "  from {:?}", self.init)?;
        for (a, s) in &self.path {
            writeln!(f, "  {a:?} → {s:?}")?;
        }
        Ok(())
    }
}

/// Explore every reachable state (up to `limit`; past it the check
/// fails rather than claim a proof it did not finish).
pub fn check<M: Model>(
    model: &M,
    limit: usize,
) -> Result<Stats, Counterexample<M::State, M::Action>> {
    // index → (state, parent index and action)
    let mut states: Vec<M::State> = Vec::new();
    let mut parent: Vec<Option<(usize, M::Action)>> = Vec::new();
    let mut index: HashMap<M::State, usize> = HashMap::new();
    let mut edges: Vec<Vec<usize>> = Vec::new();
    let mut queue = VecDeque::new();
    let mut transitions = 0;
    let path_to = |i: usize, states: &[M::State], parent: &[Option<(usize, M::Action)>]| {
        let mut path = Vec::new();
        let mut at = i;
        while let Some((p, a)) = &parent[at] {
            path.push((a.clone(), states[at].clone()));
            at = *p;
        }
        path.reverse();
        (states[at].clone(), path)
    };
    for s in model.init() {
        if !index.contains_key(&s) {
            index.insert(s.clone(), states.len());
            queue.push_back(states.len());
            states.push(s);
            parent.push(None);
            edges.push(Vec::new());
        }
    }
    let mut actions = Vec::new();
    while let Some(i) = queue.pop_front() {
        if let Err(why) = model.invariant(&states[i]) {
            let (init, path) = path_to(i, &states, &parent);
            return Err(Counterexample {
                property: why,
                init,
                path,
            });
        }
        actions.clear();
        model.actions(&states[i], &mut actions);
        for a in actions.drain(..) {
            let Some(n) = model.next(&states[i], &a) else {
                continue;
            };
            transitions += 1;
            let j = match index.get(&n) {
                Some(&j) => j,
                None => {
                    if states.len() >= limit {
                        let (init, path) = path_to(i, &states, &parent);
                        return Err(Counterexample {
                            property: format!("more than {limit} states: raise the limit"),
                            init,
                            path,
                        });
                    }
                    let j = states.len();
                    index.insert(n.clone(), j);
                    states.push(n);
                    parent.push(Some((i, a)));
                    edges.push(Vec::new());
                    queue.push_back(j);
                    j
                }
            };
            edges[i].push(j);
        }
    }
    // Liveness: every state reaches a quiescent one (backward search
    // from the quiescent states over reversed edges).
    if states.iter().any(|s| model.quiescent(s).is_some()) {
        let mut reverse = vec![Vec::new(); states.len()];
        for (i, out) in edges.iter().enumerate() {
            for &j in out {
                reverse[j].push(i);
            }
        }
        let mut reaches = vec![false; states.len()];
        let mut work: Vec<usize> = (0..states.len())
            .filter(|&i| model.quiescent(&states[i]) == Some(true))
            .collect();
        for &i in &work {
            reaches[i] = true;
        }
        while let Some(j) = work.pop() {
            for &i in &reverse[j] {
                if !reaches[i] {
                    reaches[i] = true;
                    work.push(i);
                }
            }
        }
        if let Some(stuck) = reaches.iter().position(|r| !r) {
            let (init, path) = path_to(stuck, &states, &parent);
            return Err(Counterexample {
                property: "liveness: a quiescent state is reachable from every state".into(),
                init,
                path,
            });
        }
    }
    Ok(Stats {
        states: states.len(),
        transitions,
    })
}

/// Where a recorded trace left the model.
#[derive(Debug, PartialEq)]
pub struct Divergence<S, A> {
    /// Index of the first step the model does not allow.
    pub step: usize,
    pub state: S,
    pub action: A,
    pub why: String,
}

/// Check that `trace`, started in `init`, is a behaviour of `model`:
/// every action enabled where it happened, every state on the way
/// within the invariant. Returns the final state.
pub fn validate<M: Model>(
    model: &M,
    init: M::State,
    trace: &[M::Action],
) -> Result<M::State, Divergence<M::State, M::Action>> {
    let mut state = init;
    for (step, action) in trace.iter().enumerate() {
        let Some(next) = model.next(&state, action) else {
            return Err(Divergence {
                step,
                state,
                action: action.clone(),
                why: "not enabled in this state".into(),
            });
        };
        if let Err(why) = model.invariant(&next) {
            return Err(Divergence {
                step,
                state,
                action: action.clone(),
                why,
            });
        }
        state = next;
    }
    Ok(state)
}
