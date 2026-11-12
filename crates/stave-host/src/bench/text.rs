//! Human-readable renderings of bench reports (the JSON is the API;
//! this is for eyes in a terminal).

use std::fmt::Write;

use super::describe::Description;
use super::report::BenchReport;
use crate::response::LevelResponse;
use stave_measure::stats::Summary;

/// Picks one statistic out of a summary.
type Pick = fn(&Summary) -> f64;

pub fn bench(r: &BenchReport, title: &str) -> String {
    let mut s = String::new();
    let w = &mut s;
    let _ = writeln!(w, "── stave-bench: {title}");
    let _ = writeln!(
        w,
        "   {} Hz, block {}, {}",
        r.sample_rate, r.block, r.stimulus
    );
    for warning in &r.warnings {
        let _ = writeln!(w, "   ! {warning}");
    }
    if let Some(cpu) = &r.cpu {
        let _ = writeln!(
            w,
            "\nCPU (process() wall time, {} blocks, budget {:.1} µs/block):",
            cpu.blocks, cpu.budget_us
        );
        let _ = writeln!(
            w,
            "   {:<8} {:>10} {:>12} {:>10}",
            "", "µs/block", "ns/sample", "% of RT"
        );
        let rows: [(&str, Pick); 5] = [
            ("mean", |s| s.mean),
            ("median", |s| s.median),
            ("p95", |s| s.p95),
            ("p99", |s| s.p99),
            ("max", |s| s.max),
        ];
        for (name, pick) in rows {
            let _ = writeln!(
                w,
                "   {name:<8} {:>10.1} {:>12.1} {:>9.2}%",
                pick(&cpu.us_per_block),
                pick(&cpu.ns_per_sample),
                pick(&cpu.rt_pct)
            );
        }
    }
    let _ = writeln!(
        w,
        "\nLatency:\n   reported PDC     {} samples",
        r.latency.reported
    );
    match &r.latency.measured {
        None => {
            let _ = writeln!(
                w,
                "   measured         — (instrument: no input for an impulse)"
            );
        }
        Some(chs) => {
            for (c, m) in chs.iter().enumerate() {
                let _ = match m {
                    Some(m) => writeln!(
                        w,
                        "   ch{c} measured    onset {} (Δ {:+}), peak {} (Δ {:+})",
                        m.onset,
                        m.onset as i64 - i64::from(r.latency.reported),
                        m.peak,
                        m.peak as i64 - i64::from(r.latency.reported)
                    ),
                    None => writeln!(w, "   ch{c} measured    no impulse arrived"),
                };
            }
        }
    }
    let _ = writeln!(w, "\nOutput:");
    for l in &r.output.levels {
        let channels: Vec<String> = l
            .channels
            .iter()
            .enumerate()
            .map(|(c, v)| format!("ch{c} {}", level(*v)))
            .collect();
        let _ = writeln!(
            w,
            "   {:<10} {:>8} {:<4}   {}",
            l.measure,
            level(l.program),
            l.unit,
            channels.join("  ")
        );
    }
    let residuals: Vec<String> = r
        .output
        .silence_residual_dbfs
        .iter()
        .enumerate()
        .map(|(c, v)| format!("ch{c} {v:.1}"))
        .collect();
    let _ = writeln!(w, "   silence residual (dBFS)   {}", residuals.join("  "));
    if r.output.non_finite_samples > 0 {
        let _ = writeln!(w, "   ! {} non-finite samples", r.output.non_finite_samples);
    }
    for (c, t) in r.output.tone.iter().flatten().enumerate() {
        let _ = writeln!(
            w,
            "   ch{c} tone {:.1} Hz: gain {:+.2} dB, THD {:.4}%, THD+N {:.4}%",
            t.freq_hz, t.gain_db, t.thd_pct, t.thdn_pct
        );
    }
    for (c, d) in r.output.dc.iter().enumerate() {
        let _ = writeln!(
            w,
            "   ch{c} dc offset {:+.2e}, drift {:+.2e}",
            d.offset, d.drift
        );
    }
    for (c, s) in r.output.step.iter().flatten().enumerate() {
        let opt = |v: Option<f64>, scale: f64, unit: &str| {
            v.map_or_else(|| "—".into(), |v| format!("{:.3}{unit}", v * scale))
        };
        let _ = writeln!(
            w,
            "   ch{c} step: settles at {:.3}×, peak {:.3}×, overshoot {}, rise {}, settle {}",
            s.final_gain,
            s.peak_gain,
            opt(s.overshoot_pct, 1.0, "%"),
            opt(s.rise_s, 1e3, " ms"),
            opt(s.settle_s, 1e3, " ms"),
        );
    }
    if let Some(s) = &r.output.stereo {
        let delay = s
            .delay_samples
            .map_or_else(|| "none".into(), |d| format!("{d} samples"));
        let _ = writeln!(
            w,
            "   stereo: correlation {:+.3}, side/mid {:.1} dB, balance {:+.2} dB, \
             mono sum {:+.2} dB, delay {delay}{}",
            s.correlation,
            s.side_to_mid_db,
            s.balance_db,
            s.mono_sum_db,
            if s.identical {
                ", channels identical"
            } else {
                ""
            }
        );
    }
    if !r.probes.is_empty() {
        let _ = writeln!(w, "\nPlugin probes (read after the timed run):");
        for p in &r.probes {
            let _ = writeln!(
                w,
                "   {:<22} {:>12.4}  min {:>12.4}  max {:>12.4}",
                p.name, p.latest, p.min, p.max
            );
        }
    }
    let _ = writeln!(w, "\nPhases:");
    for p in &r.phases {
        let _ = writeln!(
            w,
            "   {:<8} {:>9} frames {:>9.1} ms",
            p.phase, p.frames, p.wall_ms
        );
    }
    s
}

/// Gain at each read frequency per input level, then the spread.
pub fn level_response(r: &LevelResponse) -> String {
    let mut w = String::new();
    let cell = |v: Option<f64>| v.map_or_else(|| "     —".into(), |v| format!("{v:>+6.1}"));
    let _ = write!(w, "{:>10}", "Hz");
    for hz in &r.read_hz {
        let _ = write!(w, " {hz:>6.0}");
    }
    let _ = writeln!(w);
    for (level, gains) in r.levels_db.iter().zip(&r.gain_db) {
        let _ = write!(w, "{:>6.0} dBFS", level);
        for g in gains {
            let _ = write!(w, " {}", cell(*g));
        }
        let _ = writeln!(w);
    }
    let _ = write!(w, "{:>10}", "spread");
    for s in &r.spread_db {
        let _ = write!(w, " {}", cell(*s));
    }
    let _ = writeln!(w);
    w
}

pub fn description(d: &Description) -> String {
    let mut s = String::new();
    let w = &mut s;
    if let Some(p) = &d.plugin {
        let _ = writeln!(
            w,
            "{} — {} {}",
            p.class.name, p.class.vendor, p.class.version
        );
        let _ = writeln!(
            w,
            "   controller: {:?}, telemetry: {}",
            p.controller, p.stave_telemetry
        );
    }
    let l = &d.layout;
    let _ = writeln!(
        w,
        "   {} in / {} out @ {} Hz, latency {} samples, tail {:?}",
        l.inputs, l.outputs, l.sample_rate, d.latency_samples, d.tail
    );
    let _ = writeln!(w, "controls ({}):", d.controls.len());
    for c in &d.controls {
        let value = c
            .step_names
            .get(c.value as usize)
            .filter(|_| c.steps > 0)
            .map_or_else(|| format!("{:.4}", c.value), |name| name.clone());
        let _ = writeln!(
            w,
            "   {:<28} {:>10} {:<6} [{} .. {}]{}",
            c.name,
            value,
            c.unit,
            c.min,
            c.max,
            if c.flags.read_only {
                "  (read-only)"
            } else {
                ""
            }
        );
    }
    let _ = writeln!(w, "presets ({}):", d.presets.len());
    for p in &d.presets {
        let _ = writeln!(w, "   {p}");
    }
    s
}

/// A level to two decimals, or "—" where it is undefined.
fn level(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_string(), |v| format!("{v:.2}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{describe::describe, run, BenchConfig};
    use crate::testing::{Delay, Gain};

    #[test]
    fn bench_text_names_every_section() {
        let mut dl = Delay::new(1, 4);
        let cfg = BenchConfig {
            seconds: 0.1,
            ..BenchConfig::default()
        };
        let t = bench(&run(&mut dl, &cfg).unwrap().report, "delay");
        for want in [
            "CPU",
            "Latency:",
            "ch0 measured    onset 4 (Δ +0)",
            "Output:",
            "Phases:",
        ] {
            assert!(t.contains(want), "missing {want:?} in\n{t}");
        }
    }

    #[test]
    fn description_lists_controls_and_flags() {
        let t = description(&describe(&Gain::new(1, 1)));
        assert!(
            t.contains("controls (2):") && t.contains("Level · gain"),
            "{t}"
        );
    }
}
