//! The capture workflow (DSP.md M4): one stimulus to play through a
//! device — random-phase multisines at several levels for the best
//! linear approximation and the block fit, a synchronized sweep for the
//! Hammerstein kernels, noise bursts held out for validation, and
//! optionally an excerpt of real program material (DSP.md R22's corpus),
//! held out too — and the measurements read back from a recording of it
//! (aligned by cross-correlation) or from a system run on it directly.

use std::fmt::Write as _;

use super::hammerstein::stimulus;
use super::periodic::{multisine, oversampled, resample, spectrum, steady_state, waveform, Record, Records, Seeded};
use crate::suite::sweep::Sweep;
use crate::suite::Complex;

/// The stimulus design.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    pub rate: f64,
    /// Samples of a multisine period (a power of two).
    pub period: usize,
    /// Periods played before the one measured.
    pub settle: usize,
    /// Random-phase realisations a level.
    pub realisations: u64,
    /// The band the multisines and bursts excite, Hz.
    pub low_hz: f64,
    pub high_hz: f64,
    /// The input RMS that levels are relative to.
    pub reference: f64,
    /// The multisines' levels, dB re `reference`.
    pub levels_db: Vec<f64>,
    /// The sweep's peak, dB re `reference`·√2 (a sine of the
    /// reference's RMS), and its band and length.
    pub sweep_db: f64,
    pub sweep_low_hz: f64,
    pub sweep_high_hz: f64,
    pub sweep_seconds: f64,
    /// The noise bursts' levels, dB re `reference` (validation only).
    pub bursts_db: Vec<f64>,
    /// Seconds of silence between segments, and after the sweep (its
    /// response's tail).
    pub gap: f64,
    /// Program material played last (validation only).
    pub program: Option<Program>,
}

/// Program material in the stimulus: a name that finds it again (a
/// corpus role and item) and its samples at the capture's rate, already
/// at the level they play at. A description read back
/// ([`Capture::parse`]) holds zeros of the right length: the samples are
/// the stimulus file's, which the reader takes them from.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub name: String,
    pub samples: Vec<f64>,
}

/// Where one part of the stimulus sits.
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    /// `settle + 1` periods of one realisation; the last is measured.
    Multisine { rms: f64, seed: u64, start: usize },
    /// The sweep and its silence ([`stimulus`]), `len` samples.
    Sweep { start: usize, len: usize },
    /// The bursts, `len` samples.
    Bursts { start: usize, len: usize },
    /// The program material, `len` samples.
    Program { start: usize, len: usize },
}

/// What a fit reads: periodic records, the sweep's input and output,
/// and the held-out bursts' input and output.
#[derive(Clone, Debug)]
pub struct Measurements {
    pub records: Records,
    pub sweep: Sweep,
    pub sweep_input: Vec<f64>,
    pub sweep_output: Vec<f64>,
    pub bursts_input: Vec<f64>,
    pub bursts_output: Vec<f64>,
    /// The program material's input and output (empty without one).
    pub program_input: Vec<f64>,
    pub program_output: Vec<f64>,
}

impl Capture {
    /// The default design at `rate` around an input RMS of `reference`:
    /// 8192-sample periods at 48 kHz (scaled with the rate), 10 Hz to
    /// 20 kHz, four realisations at −20, −6, 0, +6 and +12 dB, a 2 s
    /// sweep from 5 Hz peaking at the 0 dB sine's peak, and bursts from
    /// −20 to +12 dB.
    pub fn standard(rate: f64, reference: f64) -> Self {
        let period = ((rate * 8192.0 / 48_000.0) as usize).next_power_of_two();
        Self {
            rate,
            period,
            settle: 3,
            realisations: 4,
            low_hz: 10.0,
            high_hz: 20_000.0f64.min(0.45 * rate),
            reference,
            levels_db: vec![-20.0, -6.0, 0.0, 6.0, 12.0],
            sweep_db: 0.0,
            sweep_low_hz: 5.0,
            sweep_high_hz: 0.47 * rate,
            sweep_seconds: 2.0,
            bursts_db: vec![-20.0, -10.0, -3.0, 0.0, 6.0, 12.0],
            gap: 0.25,
            program: None,
        }
    }

    fn rms(&self, db: f64) -> f64 {
        self.reference * 10f64.powf(db / 20.0)
    }

    /// The synchronized sweep.
    pub fn sweep(&self) -> Sweep {
        Sweep::synchronized(
            self.sweep_low_hz,
            self.sweep_high_hz,
            self.sweep_seconds,
            self.rms(self.sweep_db) * std::f64::consts::SQRT_2,
        )
    }

    fn sweep_samples(&self, rate: f64) -> Vec<f64> {
        stimulus(&self.sweep(), rate, 0.01, self.gap.max(0.1))
    }

    /// The bursts at `rate`: Gaussian noise over the band (shaped on one
    /// power-of-two period), 0.4 s a level with 10 ms raised-cosine ends
    /// and 0.1 s of silence after each.
    fn bursts(&self, rate: f64) -> Vec<f64> {
        let on = (0.4 * rate) as usize;
        let off = (0.1 * rate) as usize;
        let len = ((on + off) * self.bursts_db.len()).next_power_of_two();
        let mut rng = Seeded::new(0x5eed);
        let white: Vec<f64> = (0..len).map(|_| rng.normal()).collect();
        let mut s = spectrum(&white);
        let df = rate / len as f64;
        for (k, v) in s.iter_mut().enumerate() {
            let f = k.min(len - k) as f64 * df;
            if f < self.low_hz || f > self.high_hz {
                *v = Complex::new(0.0, 0.0);
            }
        }
        let noise = waveform(&s);
        let rms = (noise.iter().map(|v| v * v).sum::<f64>() / len as f64).sqrt();
        let ramp = (0.01 * rate) as usize;
        let mut out = vec![0.0; len];
        for (b, &db) in self.bursts_db.iter().enumerate() {
            let start = b * (on + off);
            let g = self.rms(db) / rms;
            for i in 0..on {
                let edge = i.min(on - 1 - i);
                let w = if edge < ramp {
                    0.5 - 0.5 * (std::f64::consts::PI * edge as f64 / ramp as f64).cos()
                } else {
                    1.0
                };
                out[start + i] = noise[start + i] * g * w;
            }
        }
        out
    }

    /// Each burst's level (dB re `reference`) and its samples within
    /// the bursts segment (the ramps included).
    pub fn burst_spans(&self) -> Vec<(f64, std::ops::Range<usize>)> {
        let on = (0.4 * self.rate) as usize;
        let off = (0.1 * self.rate) as usize;
        self.bursts_db
            .iter()
            .enumerate()
            .map(|(b, &db)| (db, b * (on + off)..b * (on + off) + on))
            .collect()
    }

    /// The segments in playing order, after `gap` seconds of lead-in.
    pub fn layout(&self) -> Vec<Segment> {
        let gap = (self.gap * self.rate) as usize;
        let mut at = gap;
        let mut out = Vec::new();
        for &db in &self.levels_db {
            for seed in 1..=self.realisations {
                out.push(Segment::Multisine {
                    rms: self.rms(db),
                    seed,
                    start: at,
                });
                at += (self.settle + 1) * self.period + gap;
            }
        }
        let len = self.sweep_samples(self.rate).len();
        out.push(Segment::Sweep { start: at, len });
        at += len + gap;
        let len = self.bursts(self.rate).len();
        out.push(Segment::Bursts { start: at, len });
        if let Some(p) = &self.program {
            at += len + gap;
            out.push(Segment::Program {
                start: at,
                len: p.samples.len(),
            });
        }
        out
    }

    /// The whole stimulus, ending in `gap` seconds of silence.
    pub fn samples(&self) -> Vec<f64> {
        let mut out = Vec::new();
        for seg in self.layout() {
            let (start, data) = match seg {
                Segment::Multisine { rms, seed, start } => {
                    let p = multisine(self.period, self.rate, self.low_hz, self.high_hz, rms, seed);
                    (start, p.repeat(self.settle + 1))
                }
                Segment::Sweep { start, .. } => (start, self.sweep_samples(self.rate)),
                Segment::Bursts { start, .. } => (start, self.bursts(self.rate)),
                Segment::Program { start, .. } => (start, self.program.as_ref().map_or_else(Vec::new, |p| p.samples.clone())),
            };
            out.resize(start, 0.0);
            out.extend(data);
        }
        out.resize(out.len() + (self.gap * self.rate) as usize, 0.0);
        out
    }

    /// The recording's delay against the stimulus, samples: the peak of
    /// their cross-correlation's magnitude (a polarity-inverting device
    /// reads as the same delay), searched up to `max_lag`. A device's own
    /// group delay is part of it; a loopback measurement of the
    /// interface (pass it to [`read`](Self::read) instead) is the
    /// interface's alone.
    pub fn align(&self, recorded: &[f64], max_lag: usize) -> usize {
        let x = self.samples();
        let n = (x.len() + recorded.len()).next_power_of_two();
        let pad = |v: &[f64]| {
            let mut p = v.to_vec();
            p.resize(n, 0.0);
            spectrum(&p)
        };
        let (sx, sy) = (pad(&x), pad(recorded));
        let c = waveform(&sx.iter().zip(&sy).map(|(a, b)| a.conj() * *b).collect::<Vec<_>>());
        (0..=max_lag.min(n - 1))
            .max_by(|&a, &b| c[a].abs().total_cmp(&c[b].abs()))
            .unwrap_or(0)
    }

    /// The measurements in `recorded` (the device's response to
    /// [`samples`](Self::samples), at the same rate), `latency` samples
    /// late. Too short a recording is an error naming the segment.
    pub fn read(&self, recorded: &[f64], latency: usize) -> Result<Measurements, String> {
        let slice = |start: usize, len: usize, what: &str| -> Result<Vec<f64>, String> {
            let s = start + latency;
            recorded
                .get(s..s + len)
                .map(<[f64]>::to_vec)
                .ok_or_else(|| format!("the recording ends before the {what} (needs {} samples, has {})", s + len, recorded.len()))
        };
        let mut records = Vec::new();
        let mut sweep = None;
        let mut bursts = None;
        let mut program = (Vec::new(), Vec::new());
        for seg in self.layout() {
            match seg {
                Segment::Multisine { rms, seed, start } => {
                    let input = multisine(self.period, self.rate, self.low_hz, self.high_hz, rms, seed);
                    let output = slice(start + self.settle * self.period, self.period, "multisines")?;
                    records.push(Record { input, output, rms });
                }
                Segment::Sweep { start, len } => sweep = Some((self.sweep_samples(self.rate), slice(start, len, "sweep")?)),
                Segment::Bursts { start, len } => bursts = Some((self.bursts(self.rate), slice(start, len, "bursts")?)),
                Segment::Program { start, len } => {
                    let input = self.program.as_ref().map_or_else(Vec::new, |p| p.samples.clone());
                    program = (input, slice(start, len, "program material")?);
                }
            }
        }
        let (sweep_input, sweep_output) = sweep.expect("the layout has a sweep");
        let (bursts_input, bursts_output) = bursts.expect("the layout has bursts");
        Ok(Measurements {
            records: Records {
                rate: self.rate,
                low_hz: self.low_hz,
                high_hz: self.high_hz,
                records,
            },
            sweep: self.sweep(),
            sweep_input,
            sweep_output,
            bursts_input,
            bursts_output,
            program_input: program.0,
            program_output: program.1,
        })
    }

    /// The measurements of a system run directly: `make()` builds it
    /// fresh at `factor`× the capture's rate (a power of two), every
    /// segment is band-limit interpolated up to it and the response
    /// brought back down — the anti-aliased measurement a perfect
    /// interface would make. Factor 1 runs the system as a plugin runs.
    pub fn run(&self, factor: usize, mut make: impl FnMut() -> Box<dyn FnMut(f64) -> f64>) -> Measurements {
        assert!(factor.is_power_of_two(), "oversampling factor {factor} is not a power of two");
        let mut records = Vec::new();
        {
            let mut system = oversampled(factor, self.settle, &mut make);
            for &db in &self.levels_db {
                let rms = self.rms(db);
                for seed in 1..=self.realisations {
                    let input = multisine(self.period, self.rate, self.low_hz, self.high_hz, rms, seed);
                    let output = system(&input);
                    records.push(Record { input, output, rms });
                }
            }
        }
        let mut once = |x: &[f64]| {
            let up = resample(x, x.len() * factor);
            let mut tick = make();
            let out = steady_state(&mut *tick, &up, 0);
            (resample(&up, x.len()), resample(&out, x.len()))
        };
        let (sweep_input, sweep_output) = once(&self.sweep_samples(self.rate));
        let (bursts_input, bursts_output) = once(&self.bursts(self.rate));
        let (program_input, program_output) = match &self.program {
            // Not a period: zero-padded to one so the band-limited
            // interpolation does not wrap its end onto its start.
            Some(p) => {
                let n = p.samples.len();
                let mut x = p.samples.clone();
                x.resize((2 * n).next_power_of_two(), 0.0);
                let (i, o) = once(&x);
                (i[..n].to_vec(), o[..n].to_vec())
            }
            None => (Vec::new(), Vec::new()),
        };
        Measurements {
            records: Records {
                rate: self.rate,
                low_hz: self.low_hz,
                high_hz: self.high_hz,
                records,
            },
            sweep: self.sweep(),
            sweep_input,
            sweep_output,
            bursts_input,
            bursts_output,
            program_input,
            program_output,
        }
    }

    /// The design as `key value` lines (the stimulus file's sidecar).
    pub fn describe(&self) -> String {
        let list = |v: &[f64]| v.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(" ");
        let mut s = String::from("# stave-fit capture v1\n");
        let _ = writeln!(s, "rate {}", self.rate);
        let _ = writeln!(s, "period {}", self.period);
        let _ = writeln!(s, "settle {}", self.settle);
        let _ = writeln!(s, "realisations {}", self.realisations);
        let _ = writeln!(s, "band {} {}", self.low_hz, self.high_hz);
        let _ = writeln!(s, "reference {:e}", self.reference);
        let _ = writeln!(s, "levels {}", list(&self.levels_db));
        let _ = writeln!(s, "sweep {} {} {} {}", self.sweep_db, self.sweep_low_hz, self.sweep_high_hz, self.sweep_seconds);
        let _ = writeln!(s, "bursts {}", list(&self.bursts_db));
        let _ = writeln!(s, "gap {}", self.gap);
        if let Some(p) = &self.program {
            let _ = writeln!(s, "program {} {}", p.samples.len(), p.name);
        }
        s
    }

    /// Read [`describe`](Self::describe)'s lines back.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut c = Self::standard(48_000.0, 0.1);
        let mut seen = 0;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            if let Some(rest) = line.strip_prefix("program ") {
                let (len, name) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
                let len: usize = len.parse().map_err(|e| format!("program: {len}: {e}"))?;
                c.program = Some(Program {
                    name: name.trim().to_string(),
                    samples: vec![0.0; len],
                });
                continue;
            }
            let mut words = line.split_whitespace();
            let key = words.next().unwrap_or_default();
            let v: Vec<f64> = words
                .map(|w| w.parse::<f64>().map_err(|e| format!("{key}: {w}: {e}")))
                .collect::<Result<_, _>>()?;
            let one = |i: usize| v.get(i).copied().ok_or_else(|| format!("{key}: too few values"));
            match key {
                "rate" => c.rate = one(0)?,
                "period" => c.period = one(0)? as usize,
                "settle" => c.settle = one(0)? as usize,
                "realisations" => c.realisations = one(0)? as u64,
                "band" => (c.low_hz, c.high_hz) = (one(0)?, one(1)?),
                "reference" => c.reference = one(0)?,
                "levels" => c.levels_db = v.clone(),
                "sweep" => (c.sweep_db, c.sweep_low_hz, c.sweep_high_hz, c.sweep_seconds) = (one(0)?, one(1)?, one(2)?, one(3)?),
                "bursts" => c.bursts_db = v.clone(),
                "gap" => c.gap = one(0)?,
                other => return Err(format!("unknown key {other}")),
            }
            seen += 1;
        }
        if seen < 10 || !c.period.is_power_of_two() {
            return Err("an incomplete capture description".into());
        }
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_is_aligned_and_read_back() {
        let mut c = Capture::standard(48_000.0, 0.1);
        c.levels_db = vec![0.0];
        c.realisations = 1;
        c.period = 2048;
        c.sweep_seconds = 0.5;
        c.bursts_db = vec![0.0];
        assert_eq!(Capture::parse(&c.describe()).unwrap(), c);
        c.program = Some(Program {
            name: "drums_loop#0 a loop".into(),
            samples: (0..3_000).map(|i| (f64::from(i) * 0.01).sin() * 0.2).collect(),
        });
        let mut back = Capture::parse(&c.describe()).unwrap();
        assert_eq!(back.program.as_ref().map(|p| (p.name.as_str(), p.samples.len())), Some(("drums_loop#0 a loop", 3_000)));
        back.program.clone_from(&c.program);
        assert_eq!(back, c);
        let x = c.samples();
        // A device: 0.5·x − 0.1·x², 37 samples of latency, inverted.
        let mut rec = vec![0.0; 37];
        rec.extend(x.iter().map(|v| -(0.5 * v - 0.1 * v * v)));
        let lag = c.align(&rec, 4_800);
        assert_eq!(lag, 37);
        let m = c.read(&rec, lag).unwrap();
        let r = &m.records.records[0];
        let worst = r.input.iter().zip(&r.output).fold(0.0f64, |w, (a, b)| w.max((-(0.5 * a - 0.1 * a * a) - b).abs()));
        assert!(worst < 1e-15, "{worst}");
        let p = &c.program.as_ref().unwrap().samples;
        assert_eq!(&m.program_input, p);
        let worst = p.iter().zip(&m.program_output).fold(0.0f64, |w, (a, b)| w.max((-(0.5 * a - 0.1 * a * a) - b).abs()));
        assert!(worst < 1e-15, "{worst}");
        assert!(c.read(&rec[..rec.len() / 2], lag).is_err());
        // Run directly: the program comes back through the system too.
        let direct = c.run(2, || Box::new(|x: f64| 0.5 * x));
        let worst = direct.program_input.iter().zip(&direct.program_output).fold(0.0f64, |w, (a, b)| w.max((0.5 * a - b).abs()));
        assert!(direct.program_output.len() == 3_000 && worst < 1e-9, "{worst}");
    }
}
