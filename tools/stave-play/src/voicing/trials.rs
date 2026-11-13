//! Blind trials: ABX runs and MUSHRA-style rating runs, as state
//! machines over take indices. Randomisation comes from a logged seed,
//! so a run can be replayed; nothing here knows about audio or the UI.

use std::time::Instant;

use serde::Serialize;
use stave_measure::listening::{passes_screening, Abx, AbxVerdict, RatingSummary, ALPHA, ASSUMED_HIT_RATE};

use super::rng::Rng;

/// What the listener is hearing in an ABX trial.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub enum AbxListen {
    A,
    B,
    X,
}

#[derive(Clone, Debug, Serialize)]
pub struct AbxTrial {
    /// Which take X was (hidden until the trial is answered).
    pub x_is_a: bool,
    /// The answer: "X is A".
    pub answered_a: bool,
    pub correct: bool,
    /// Times A, B and X were selected.
    pub listens: [u32; 3],
    pub seconds: f64,
}

/// One ABX run between two takes.
pub struct AbxRun {
    /// Take indices of A and B.
    pub pair: (usize, usize),
    pub planned: u32,
    pub seed: u64,
    pub trials: Vec<AbxTrial>,
    rng: Rng,
    x_is_a: bool,
    listens: [u32; 3],
    started: Instant,
    /// The listener's pending answer for this trial.
    pub pending: Option<bool>,
    pub listening: AbxListen,
}

impl AbxRun {
    pub fn new(pair: (usize, usize), planned: u32, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let x_is_a = rng.coin();
        Self {
            pair,
            planned: planned.max(1),
            seed,
            trials: Vec::new(),
            rng,
            x_is_a,
            listens: [0; 3],
            started: Instant::now(),
            pending: None,
            listening: AbxListen::A,
        }
    }

    pub fn finished(&self) -> bool {
        self.trials.len() as u32 >= self.planned
    }

    /// The 1-based number of the trial being run.
    pub fn trial_number(&self) -> u32 {
        (self.trials.len() as u32 + 1).min(self.planned)
    }

    /// Select A, B or X; returns the take to play.
    pub fn listen(&mut self, which: AbxListen) -> usize {
        if !self.finished() && which != self.listening {
            self.listens[which as usize] += 1;
        }
        self.listening = which;
        self.take_for(which)
    }

    /// The take behind A, B or X in the current trial.
    pub fn take_for(&self, which: AbxListen) -> usize {
        match which {
            AbxListen::A => self.pair.0,
            AbxListen::B => self.pair.1,
            AbxListen::X if self.x_is_a => self.pair.0,
            AbxListen::X => self.pair.1,
        }
    }

    /// Choose "X is A" (`true`) or "X is B"; change it until confirmed.
    pub fn answer(&mut self, x_is_a: bool) {
        if !self.finished() {
            self.pending = Some(x_is_a);
        }
    }

    /// Record the pending answer and draw the next trial. `false` when
    /// there is nothing to confirm.
    pub fn confirm(&mut self) -> bool {
        let Some(answered_a) = self.pending.take() else {
            return false;
        };
        if self.finished() {
            return false;
        }
        self.trials.push(AbxTrial {
            x_is_a: self.x_is_a,
            answered_a,
            correct: answered_a == self.x_is_a,
            listens: self.listens,
            seconds: self.started.elapsed().as_secs_f64(),
        });
        self.x_is_a = self.rng.coin();
        self.listens = [0; 3];
        self.started = Instant::now();
        self.listening = AbxListen::A;
        true
    }

    pub fn score(&self) -> Abx {
        Abx {
            trials: self.trials.len() as u32,
            correct: self.trials.iter().filter(|t| t.correct).count() as u32,
        }
    }

    pub fn stats(&self) -> AbxStats {
        let s = self.score();
        let (lo, hi) = s.hit_rate_interval();
        AbxStats {
            trials: s.trials,
            correct: s.correct,
            planned: self.planned,
            p_value: s.p_value(),
            alpha: ALPHA,
            critical: Abx::critical(self.planned, ALPHA),
            assumed_hit_rate: ASSUMED_HIT_RATE,
            power: Abx::power(self.planned, ALPHA, ASSUMED_HIT_RATE),
            d_prime: s.d_prime(),
            hit_rate_ci95: [lo, hi],
            verdict: s.verdict(self.planned),
        }
    }
}

/// What a session log records for an ABX run.
#[derive(Clone, Debug, Serialize)]
pub struct AbxStats {
    pub trials: u32,
    pub correct: u32,
    pub planned: u32,
    pub p_value: f64,
    pub alpha: f64,
    /// The score the planned run needs for significance.
    pub critical: Option<u32>,
    pub assumed_hit_rate: f64,
    /// The planned run's power at the assumed hit rate.
    pub power: f64,
    pub d_prime: f64,
    pub hit_rate_ci95: [f64; 2],
    pub verdict: AbxVerdict,
}

/// A rating stimulus: what it is, and the take it plays.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "role")]
pub enum Stimulus {
    Candidate { index: usize, take: usize },
    HiddenReference { take: usize },
    Anchor { take: usize },
}

impl Stimulus {
    pub fn take(&self) -> usize {
        match *self {
            Self::Candidate { take, .. } | Self::HiddenReference { take } | Self::Anchor { take } => take,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RatingTrial {
    /// Slot → stimulus index (the randomised order the listener saw).
    pub order: Vec<usize>,
    /// Ratings by stimulus index, 0–100.
    pub ratings: Vec<f64>,
    pub seconds: f64,
}

/// A MUSHRA-style run: every stimulus rated on one screen, in a fresh
/// random order per trial, with the hidden reference and anchor among
/// them.
pub struct RatingRun {
    pub stimuli: Vec<Stimulus>,
    /// The open reference's take, when there is a reference.
    pub reference: Option<usize>,
    pub planned: u32,
    pub seed: u64,
    pub trials: Vec<RatingTrial>,
    rng: Rng,
    /// The current trial's order and sliders (by slot).
    pub order: Vec<usize>,
    pub sliders: Vec<f64>,
    started: Instant,
}

/// Sliders start here, at the middle of the scale (not at an end, which
/// invites leaving them there).
pub const SLIDER_START: f64 = 50.0;

impl RatingRun {
    pub fn new(stimuli: Vec<Stimulus>, reference: Option<usize>, planned: u32, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let order = shuffled(stimuli.len(), &mut rng);
        Self {
            sliders: vec![SLIDER_START; stimuli.len()],
            stimuli,
            reference,
            planned: planned.max(1),
            seed,
            trials: Vec::new(),
            rng,
            order,
            started: Instant::now(),
        }
    }

    pub fn finished(&self) -> bool {
        self.trials.len() as u32 >= self.planned
    }

    /// The take behind slot `slot` of the current trial.
    pub fn take_at(&self, slot: usize) -> Option<usize> {
        self.order.get(slot).map(|&s| self.stimuli[s].take())
    }

    /// Record the sliders and draw a fresh order.
    pub fn confirm(&mut self) {
        if self.finished() {
            return;
        }
        let mut ratings = vec![0.0; self.stimuli.len()];
        for (slot, &s) in self.order.iter().enumerate() {
            ratings[s] = self.sliders[slot];
        }
        self.trials.push(RatingTrial {
            order: self.order.clone(),
            ratings,
            seconds: self.started.elapsed().as_secs_f64(),
        });
        self.order = shuffled(self.stimuli.len(), &mut self.rng);
        self.sliders = vec![SLIDER_START; self.stimuli.len()];
        self.started = Instant::now();
    }

    /// BS.1534-3 asks that the hidden reference be found: some slider at
    /// 100 when there is a reference.
    pub fn missing_top_rating(&self) -> bool {
        self.reference.is_some() && !self.sliders.iter().any(|&v| v >= 100.0)
    }

    pub fn summary(&self) -> RatingStats {
        let per_stimulus = (0..self.stimuli.len())
            .map(|s| {
                let rs: Vec<f64> = self.trials.iter().map(|t| t.ratings[s]).collect();
                (self.stimuli[s].clone(), RatingSummary::of(&rs))
            })
            .collect();
        let hidden: Vec<f64> = self
            .stimuli
            .iter()
            .position(|s| matches!(s, Stimulus::HiddenReference { .. }))
            .map(|h| self.trials.iter().map(|t| t.ratings[h]).collect())
            .unwrap_or_default();
        RatingStats {
            trials: self.trials.len() as u32,
            planned: self.planned,
            per_stimulus,
            hidden_reference_ratings: hidden.clone(),
            passes_screening: passes_screening(&hidden),
        }
    }
}

fn shuffled(n: usize, rng: &mut Rng) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    rng.shuffle(&mut v);
    v
}

#[derive(Clone, Debug, Serialize)]
pub struct RatingStats {
    pub trials: u32,
    pub planned: u32,
    pub per_stimulus: Vec<(Stimulus, Option<RatingSummary>)>,
    pub hidden_reference_ratings: Vec<f64>,
    /// BS.1534-3 post-screening (hidden reference ≥ 90 on ≥ 85 % of trials).
    pub passes_screening: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abx_draws_x_fairly_and_replays_from_its_seed() {
        let mut run = AbxRun::new((0, 1), 4_000, 7);
        let mut xs = Vec::new();
        while !run.finished() {
            xs.push(run.take_for(AbxListen::X) == 0);
            run.answer(true);
            assert!(run.confirm());
        }
        let a = xs.iter().filter(|&&x| x).count() as f64 / xs.len() as f64;
        // 4000 fair coins: 3.2 standard deviations is ±0.025.
        assert!((a - 0.5).abs() < 0.025, "{a}");
        // No long runs that would give the sequence away: the longest
        // streak of 4000 fair flips is ~12.
        let longest = xs
            .chunk_by(|a, b| a == b)
            .map(<[bool]>::len)
            .max()
            .unwrap();
        assert!(longest < 20, "{longest}");
        // Same seed, same sequence.
        let mut again = AbxRun::new((0, 1), 50, 7);
        for &x in xs.iter().take(50) {
            assert_eq!(again.take_for(AbxListen::X) == 0, x);
            again.answer(true);
            again.confirm();
        }
        // Answering "X is A" every time scores exactly the A draws.
        assert_eq!(run.score().correct as usize, xs.iter().filter(|&&x| x).count());
    }

    #[test]
    fn abx_needs_an_answer_and_stops_at_the_plan() {
        let mut run = AbxRun::new((2, 5), 2, 1);
        assert!(!run.confirm(), "nothing chosen");
        assert_eq!(run.listen(AbxListen::B), 5);
        run.listen(AbxListen::X);
        run.answer(false);
        run.answer(true); // changing your mind is free
        assert!(run.confirm());
        assert_eq!(run.trials[0].listens, [0, 1, 1]);
        assert!(run.trials[0].answered_a);
        run.answer(true);
        run.confirm();
        assert!(run.finished());
        run.answer(true);
        assert!(!run.confirm());
        assert_eq!(run.trials.len(), 2);
        assert!(matches!(run.stats().verdict, AbxVerdict::Inconclusive { .. }));
    }

    #[test]
    fn rating_orders_are_uniform_permutations() {
        let stimuli: Vec<Stimulus> = (0..4).map(|i| Stimulus::Candidate { index: i, take: i }).collect();
        let mut run = RatingRun::new(stimuli, None, 4_000, 99);
        let mut first = [0usize; 4];
        while !run.finished() {
            let mut sorted = run.order.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, [0, 1, 2, 3], "a permutation");
            first[run.order[0]] += 1;
            run.confirm();
        }
        // Chi-square, 3 degrees of freedom: below 16.3 at p = 0.001.
        let expected = 1_000.0;
        let chi: f64 = first.iter().map(|&o| (o as f64 - expected).powi(2) / expected).sum();
        assert!(chi < 16.3, "{first:?} chi² = {chi}");
    }

    #[test]
    fn ratings_land_on_their_stimulus_not_their_slot() {
        let stimuli = vec![
            Stimulus::HiddenReference { take: 0 },
            Stimulus::Candidate { index: 1, take: 1 },
            Stimulus::Anchor { take: 2 },
        ];
        let mut run = RatingRun::new(stimuli, Some(0), 2, 3);
        for _ in 0..2 {
            assert!(run.missing_top_rating());
            for slot in 0..3 {
                run.sliders[slot] = match run.stimuli[run.order[slot]] {
                    Stimulus::HiddenReference { .. } => 100.0,
                    Stimulus::Candidate { .. } => 70.0,
                    Stimulus::Anchor { .. } => 20.0,
                };
            }
            assert!(!run.missing_top_rating());
            run.confirm();
        }
        let s = run.summary();
        let mean = |i: usize| s.per_stimulus[i].1.unwrap().mean;
        assert_eq!((mean(0), mean(1), mean(2)), (100.0, 70.0, 20.0));
        assert!(s.passes_screening);
    }
}
