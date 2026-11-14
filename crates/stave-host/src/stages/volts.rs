//! A netlist as a stage in volts, its knobs turned by settings and by a
//! sweep running inside the stage (an LFO on a held source, or on knobs
//! at an automation lane's block rate).

use stave_dsp::circuit::methods::dk::{Dk, Supply};
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::{Circuit, Pot, Smoothing};
use stave_dsp::units::Seconds;
use stave_dsp::{NewtonStats, Stage, Tail};

/// A netlist in volts: audio × `full_scale_in` drives its input source,
/// the probe's departure from its resting value ÷ `full_scale_out` comes
/// out. Its knobs (the netlist's declared pots, gliding as the circuit's
/// [`Smoothing`] says) and an optional [`Sweep`] turn it while it runs.
/// Every handle is found when it builds, so nothing allocates after.
#[derive(Clone)]
pub struct Volts {
    circuit: Circuit<Dk>,
    rest: f64,
    pub full_scale_in: f64,
    pub full_scale_out: f64,
    /// Each declared pot: its handle, the knob turning it, and whether
    /// the knob turns it the other way.
    pots: Vec<(Pot, usize, bool)>,
    /// Each knob's set position, 0..=1.
    knobs: Vec<f64>,
    pub sweep: Sweep,
    sample_rate: f64,
}

/// A triangle LFO, 0 to 1, starting at 0.
#[derive(Copy, Clone, Debug, Default)]
pub struct Lfo {
    pub rate_hz: f64,
    phase: f64,
}

impl Lfo {
    pub fn new(rate_hz: f64) -> Self {
        Self { rate_hz, phase: 0.0 }
    }

    /// `samples` on at `sample_rate`: the value there.
    fn advance(&mut self, sample_rate: f64, samples: u32) -> f64 {
        self.phase = (self.phase + self.rate_hz * f64::from(samples) / sample_rate).fract();
        1.0 - (2.0 * self.phase - 1.0).abs()
    }
}

/// What turns a running netlist from inside the stage.
#[derive(Copy, Clone, Debug, Default)]
pub enum Sweep {
    /// Nothing: only settings move it.
    #[default]
    Still,
    /// A source held from outside (the Phase 90's gate bus): at `frozen`
    /// volts, or swept by the LFO from `low` to `high`.
    Gate {
        supply: Supply,
        low: f64,
        high: f64,
        frozen: Option<f64>,
        lfo: Lfo,
    },
    /// Knobs (bit `k` of `mask` is knob `k`) swept by the LFO `depth`
    /// either side of where they are set, re-set every `step` seconds:
    /// an automation lane at that block rate. The circuit's smoothing
    /// decides whether each step jumps (zipper noise) or glides.
    Knobs {
        mask: u32,
        depth: f64,
        step: f64,
        countdown: u32,
        lfo: Lfo,
    },
}

impl Volts {
    /// `netlist` driven at `source`, its declared pots grouped into knobs
    /// (`knobs[k]` names knob `k`'s pots, a leading `-` on one it turns
    /// the other way).
    pub fn new(netlist: &Netlist, source: &str, knobs: &[&[&str]]) -> Self {
        let mut circuit = Circuit::<Dk>::new(netlist).unwrap_or_else(|e| panic!("a topology does not build: {e:?}"));
        circuit.drive(source).unwrap_or_else(|e| panic!("a topology has no source {source}: {e:?}"));
        let mut pots = Vec::new();
        for (k, names) in knobs.iter().enumerate() {
            for name in *names {
                let (reversed, name) = name.strip_prefix('-').map_or((false, *name), |n| (true, n));
                let pot = circuit
                    .pot(name)
                    .unwrap_or_else(|e| panic!("a stage turns an undeclared pot {name}: {e:?}"));
                pots.push((pot, k, reversed));
            }
        }
        let rest = circuit.output();
        Self {
            circuit,
            rest,
            full_scale_in: 1.0,
            full_scale_out: 1.0,
            pots,
            knobs: vec![0.5; knobs.len()],
            sweep: Sweep::Still,
            sample_rate: 48_000.0,
        }
    }

    /// The source called `name`, to hold from outside.
    pub fn supply(&self, name: &str) -> Supply {
        self.circuit
            .supply(name)
            .unwrap_or_else(|e| panic!("a stage holds an undeclared source {name}: {e:?}"))
    }

    /// Turn knob `k` to `position` (0..=1), gliding.
    pub fn set_knob(&mut self, k: usize, position: f64) {
        let position = position.clamp(0.0, 1.0);
        if let Some(p) = self.knobs.get_mut(k) {
            *p = position;
        }
        self.turn(k, position);
    }

    fn turn(&mut self, k: usize, position: f64) {
        for &(pot, knob, reversed) in &self.pots {
            if knob == k {
                self.circuit.set_position(pot, if reversed { 1.0 - position } else { position });
            }
        }
    }

    /// How set positions are reached: a glide of `seconds` (0 jumps).
    pub fn set_glide(&mut self, seconds: f64) {
        self.circuit.set_smoothing(Smoothing {
            time: Seconds::new(seconds.max(0.0)),
            interval: 1,
        });
    }

    pub fn stats(&self) -> NewtonStats {
        self.circuit.stats()
    }

    fn run_sweep(&mut self) {
        let rate = self.sample_rate;
        match &mut self.sweep {
            Sweep::Still => {}
            Sweep::Gate {
                supply,
                low,
                high,
                frozen,
                lfo,
            } => {
                let volts = frozen.unwrap_or_else(|| *low + (*high - *low) * lfo.advance(rate, 1));
                let supply = *supply;
                self.circuit.set_supply(supply, volts);
            }
            Sweep::Knobs {
                mask,
                depth,
                step,
                countdown,
                lfo,
            } => {
                if *countdown > 1 {
                    *countdown -= 1;
                    return;
                }
                let samples = ((*step * rate).round() as u32).max(1);
                *countdown = samples;
                let swing = *depth * (2.0 * lfo.advance(rate, samples) - 1.0);
                let mask = *mask;
                for k in 0..self.knobs.len().min(32) {
                    if mask & (1 << k) != 0 {
                        self.turn(k, (self.knobs[k] + swing).clamp(0.0, 1.0));
                    }
                }
            }
        }
    }

    fn restart_sweep(&mut self) {
        match &mut self.sweep {
            Sweep::Still => {}
            Sweep::Gate { lfo, .. } => lfo.phase = 0.0,
            Sweep::Knobs { lfo, countdown, .. } => {
                lfo.phase = 0.0;
                *countdown = 0;
            }
        }
    }
}

impl Stage for Volts {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = f64::from(sample_rate);
        self.circuit.prepare(sample_rate);
        self.rest = self.circuit.output();
        self.restart_sweep();
    }
    fn reset(&mut self) {
        self.circuit.reset();
        self.rest = self.circuit.output();
        self.restart_sweep();
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.run_sweep();
        let v = self.circuit.tick_f64(f64::from(x) * self.full_scale_in);
        ((v - self.rest) / self.full_scale_out) as f32
    }
    fn tail(&self) -> Tail {
        self.circuit.tail()
    }
}
