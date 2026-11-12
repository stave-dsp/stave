//! Characterises the multiband's saturation types and detector styles,
//! measuring the plugin's own `BandSaturator` and `Detector`.
//!
//! `profile-multiband` prints aligned tables; `--md` prints Markdown;
//! `--csv` prints one CSV block per table, each headed `# <name>`.

use std::time::Instant;

use stave_measure::level::{db, dc, peak, rms};
use stave_measure::signal::{db_to_amp, Noise, Signal, Sine};
use stave_measure::tone::{analyze, ToneAnalysis, ToneReport};
use stave_measure::tone_test::{ToneTest, TwoToneTest};
use stave_multiband_comp_vst3::detector::{Detector, DetectorSettings};
use stave_multiband_comp_vst3::saturation::{BandSaturator, SaturationCharacter, SATURATIONS};
use stave_multiband_comp_vst3::style::{Style, STYLES};

const SR: f64 = 48_000.0;
const WINDOW: usize = 8192;
/// The probe level: −6 dBFS.
const LEVEL_DB: f64 = -6.0;
const DRIVES: [f64; 6] = [0.0, 3.0, 6.0, 10.0, 14.0, 20.0];
const SATURATIONS_ALL: [SaturationCharacter; 4] = [
    SaturationCharacter::Tube,
    SaturationCharacter::Tape,
    SaturationCharacter::Transistor,
    SaturationCharacter::Clipper,
];
const STYLES_ALL: [Style; 4] = [Style::Vca, Style::Fet, Style::Opto, Style::VariMu];
/// The detector knobs every envelope measurement uses.
const DETECTOR: DetectorSettings = DetectorSettings {
    threshold_db: -18.0,
    attack_ms: 8.0,
    release_ms: 100.0,
    transient: 0.0,
    rms: 0.0,
    hpf_hz: 0.0,
};

struct Table {
    name: &'static str,
    title: &'static str,
    corner: &'static str,
    columns: Vec<String>,
    rows: Vec<(&'static str, Vec<f64>)>,
    decimals: usize,
}

impl Table {
    fn by_saturation(
        name: &'static str,
        title: &'static str,
        columns: Vec<String>,
        decimals: usize,
        row: impl Fn(SaturationCharacter) -> Vec<f64>,
    ) -> Self {
        Self {
            name,
            title,
            corner: "type",
            columns,
            rows: SATURATIONS_ALL
                .iter()
                .zip(SATURATIONS)
                .map(|(&ty, &label)| (label, row(ty)))
                .collect(),
            decimals,
        }
    }

    fn by_style(
        name: &'static str,
        title: &'static str,
        columns: Vec<String>,
        decimals: usize,
        row: impl Fn(Style) -> Vec<f64>,
    ) -> Self {
        Self {
            name,
            title,
            corner: "style",
            columns,
            rows: STYLES_ALL
                .iter()
                .zip(STYLES)
                .map(|(&style, &label)| (label, row(style)))
                .collect(),
            decimals,
        }
    }

    fn cells(&self) -> impl Iterator<Item = (&str, Vec<String>)> {
        self.rows.iter().map(|(label, values)| {
            let cells = values
                .iter()
                .map(|v| format!("{v:.*}", self.decimals))
                .collect();
            (*label, cells)
        })
    }

    fn text(&self) {
        println!("\n== {} ==", self.title);
        print!("{:>8}", self.corner);
        for c in &self.columns {
            print!("{c:>12}");
        }
        println!();
        for (label, cells) in self.cells() {
            print!("{label:>8}");
            for c in cells {
                print!("{c:>12}");
            }
            println!();
        }
    }

    fn markdown(&self) {
        println!("\n## {}\n", self.title);
        println!("| {} | {} |", self.corner, self.columns.join(" | "));
        println!("|---|{}", "---:|".repeat(self.columns.len()));
        for (label, cells) in self.cells() {
            println!("| {label} | {} |", cells.join(" | "));
        }
    }

    fn csv(&self) {
        println!("# {}", self.name);
        println!("{},{}", self.corner, self.columns.join(","));
        for (label, cells) in self.cells() {
            println!("{label},{}", cells.join(","));
        }
        println!();
    }
}

fn saturator(ty: SaturationCharacter, drive_db: f64) -> BandSaturator {
    let mut s = BandSaturator::new();
    s.set_type(ty);
    s.set_mix(1.0);
    s.set_drive_db(drive_db as f32);
    s.prepare(SR as f32);
    s
}

fn bin(hz: f64) -> usize {
    (hz * WINDOW as f64 / SR).round() as usize
}

/// The saturator's steady-state response to a tone, after one window.
fn respond(ty: SaturationCharacter, drive_db: f64, hz: f64, level_db: f64) -> (Sine, Vec<f32>) {
    let sine = Sine {
        frequency_hz: hz,
        amplitude: db_to_amp(level_db),
        sample_rate: SR,
    }
    .bin_snapped(WINDOW);
    let mut s = saturator(ty, drive_db);
    let out: Vec<f32> = sine
        .render(0, 2 * WINDOW)
        .into_iter()
        .map(|x| s.process(x))
        .collect();
    (sine, out[WINDOW..].to_vec())
}

fn tone(ty: SaturationCharacter, drive_db: f64, hz: f64, level_db: f64) -> ToneReport {
    let (sine, out) = respond(ty, drive_db, hz, level_db);
    analyze(
        &out,
        &ToneAnalysis {
            input: sine,
            max_harmonic_hz: 20_000.0,
        },
    )
}

fn harmonic_power(report: &ToneReport, even: bool) -> f64 {
    report
        .harmonics
        .iter()
        .filter(|h| (h.order % 2 == 0) == even)
        .map(|h| 10f64.powf(h.dbc / 10.0))
        .sum()
}

fn crest(x: &[f32]) -> f64 {
    peak(x) / rms(x).max(1e-12)
}

fn ns_per_sample(ty: SaturationCharacter, oversample: bool) -> f64 {
    const N: usize = 1 << 18;
    let mut s = saturator(ty, 10.0);
    s.set_oversample(oversample);
    let input = Sine {
        frequency_hz: 1_000.0,
        amplitude: db_to_amp(LEVEL_DB),
        sample_rate: SR,
    }
    .render(0, N);
    let start = Instant::now();
    let sum: f32 = input.iter().map(|&x| s.process(x)).sum();
    std::hint::black_box(sum);
    start.elapsed().as_nanos() as f64 / N as f64
}

/// Milliseconds for the detector to cover 10%, 63% and 90% of a step
/// from `from_db` to `to_db`, after a second at `from_db`.
fn step_ms(style: Style, from_db: f64, to_db: f64) -> Vec<f64> {
    let character = style.character();
    let mut d = Detector::new();
    d.prepare(SR as f32);
    let (from, to) = (db_to_amp(from_db) as f32, db_to_amp(to_db) as f32);
    for _ in 0..SR as usize {
        d.tick(from, &DETECTOR, &character);
    }
    let start = d.tick(from, &DETECTOR, &character);
    let remaining: Vec<f32> = (0..4 * SR as usize)
        .map(|_| (d.tick(to, &DETECTOR, &character) - to) / (start - to))
        .collect();
    [0.1, 0.63, 0.9]
        .iter()
        .map(|progress| {
            stave_measure::dynamics::time_to_fraction(&remaining, 1.0 - progress)
                .map_or(f64::NAN, |n| n as f64 * 1000.0 / SR)
        })
        .collect()
}

fn labels(values: &[f64], unit: &str) -> Vec<String> {
    values.iter().map(|v| format!("{v} {unit}")).collect()
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(ToString::to_string).collect()
}

fn tables() -> Vec<Table> {
    let freqs = [40.0, 100.0, 250.0, 1_000.0, 4_000.0, 10_000.0, 18_000.0];
    let in_levels = [-60.0, -40.0, -24.0, -18.0, -12.0, -6.0, -3.0, 0.0];
    let jumps = [-18.0, -12.0, -6.0];
    let holds = [-18.0, -9.0, -3.0];
    vec![
        Table::by_saturation(
            "thd_vs_drive",
            "Saturation: THD % (1 kHz, −6 dBFS)",
            labels(&DRIVES, "dB"),
            2,
            |ty| {
                DRIVES
                    .iter()
                    .map(|&d| tone(ty, d, 1_000.0, LEVEL_DB).thd_pct)
                    .collect()
            },
        ),
        Table::by_saturation(
            "harmonics",
            "Saturation: harmonics, dBc (1 kHz, 10 dB drive)",
            (2..=8).map(|h| format!("h{h}")).collect(),
            1,
            |ty| {
                tone(ty, 10.0, 1_000.0, LEVEL_DB)
                    .harmonics
                    .iter()
                    .take(7)
                    .map(|h| h.dbc)
                    .collect()
            },
        ),
        Table::by_saturation(
            "symmetry",
            "Saturation: even/odd harmonic power, DC (10 dB drive)",
            names(&["even/odd", "|DC|"]),
            5,
            |ty| {
                let report = tone(ty, 10.0, 1_000.0, LEVEL_DB);
                let (_, out) = respond(ty, 10.0, 1_000.0, LEVEL_DB);
                vec![
                    harmonic_power(&report, true) / harmonic_power(&report, false).max(1e-30),
                    dc(&out).abs(),
                ]
            },
        ),
        Table::by_saturation(
            "frequency_response",
            "Saturation: gain, dB (10 dB drive)",
            labels(&freqs, "Hz"),
            2,
            |ty| {
                freqs
                    .iter()
                    .map(|&f| tone(ty, 10.0, f, LEVEL_DB).gain_db)
                    .collect()
            },
        ),
        Table::by_saturation(
            "static_transfer",
            "Saturation: output peak, dBFS, for input level (1 kHz, 10 dB drive)",
            labels(&in_levels, "dB"),
            1,
            |ty| {
                in_levels
                    .iter()
                    .map(|&l| db(peak(&respond(ty, 10.0, 1_000.0, l).1)))
                    .collect()
            },
        ),
        Table::by_saturation(
            "aliasing_vs_drive",
            "Saturation: audible aliasing, dB (18 kHz, −6 dBFS)",
            labels(&DRIVES, "dB"),
            1,
            |ty| {
                DRIVES
                    .iter()
                    .map(|&d| {
                        let mut s = saturator(ty, d);
                        ToneTest {
                            bin: bin(18_000.0),
                            window: WINDOW,
                            sample_rate: SR,
                            amplitude: db_to_amp(LEVEL_DB),
                            warmup_windows: 1,
                        }
                        .alias(|x| s.process(x))
                        .audible_db
                    })
                    .collect()
            },
        ),
        Table::by_saturation(
            "imd",
            "Saturation: CCIF intermodulation, dB (1 + 1.1 kHz, 10 dB drive)",
            names(&["CCIF"]),
            1,
            |ty| {
                let mut s = saturator(ty, 10.0);
                vec![TwoToneTest {
                    low_bin: bin(1_000.0),
                    high_bin: bin(1_100.0),
                    window: WINDOW,
                    amplitude: db_to_amp(-12.0),
                    warmup_windows: 1,
                }
                .ccif_db(|x| s.process(x))]
            },
        ),
        Table::by_saturation(
            "crest_and_transient",
            "Saturation: crest factor in → out (white noise), impulse peak out/in (10 dB drive)",
            names(&["crest in", "crest out", "impulse"]),
            3,
            |ty| {
                let noise = Noise {
                    seed: 7,
                    amplitude: 0.5,
                }
                .render(0, SR as usize);
                let mut s = saturator(ty, 10.0);
                let out: Vec<f32> = noise.iter().map(|&x| s.process(x)).collect();
                let mut s = saturator(ty, 10.0);
                let impulse: Vec<f32> = (0..4096)
                    .map(|i| s.process(if i == 1000 { 0.5 } else { 0.0 }))
                    .collect();
                vec![crest(&noise), crest(&out), peak(&impulse) / 0.5]
            },
        ),
        Table::by_saturation(
            "cpu",
            "Saturation: ns per sample",
            names(&["4x", "1x"]),
            1,
            |ty| vec![ns_per_sample(ty, true), ns_per_sample(ty, false)],
        ),
        Table::by_style(
            "style_character",
            "Detector: style character at attack 8 ms, release 100 ms",
            names(&["attack ms", "release ms", "knee dB", "rel dep", "atk dep"]),
            2,
            |style| {
                let c = style.character();
                vec![
                    f64::from(DETECTOR.attack_ms * c.attack_scale),
                    f64::from(DETECTOR.release_ms * c.release_scale),
                    f64::from(c.knee_db),
                    f64::from(c.release_dependence),
                    f64::from(c.attack_dependence),
                ]
            },
        ),
        Table::by_style(
            "attack",
            "Detector: attack, ms to 10/63/90% (−60 → −6 dBFS)",
            names(&["t10", "t63", "t90"]),
            2,
            |style| step_ms(style, -60.0, -6.0),
        ),
        Table::by_style(
            "release",
            "Detector: release, ms to 10/63/90% (−6 → −60 dBFS)",
            names(&["t10", "t63", "t90"]),
            1,
            |style| step_ms(style, -6.0, -60.0),
        ),
        Table::by_style(
            "attack_dependence",
            "Detector: attack t63, ms, from −24 dBFS (FET speeds up on bigger jumps)",
            labels(&jumps, "dB"),
            2,
            |style| {
                jumps
                    .iter()
                    .map(|&to| step_ms(style, -24.0, to)[1])
                    .collect()
            },
        ),
        Table::by_style(
            "release_dependence",
            "Detector: release t63, ms, to −60 dBFS (OPTO/VMU slow down when hotter)",
            labels(&holds, "dB"),
            1,
            |style| {
                holds
                    .iter()
                    .map(|&from| step_ms(style, from, -60.0)[1])
                    .collect()
            },
        ),
    ]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let tables = tables();
    if has("--csv") {
        tables.iter().for_each(Table::csv);
    } else if has("--md") {
        println!("# Stave Multiband: DSP characterisation\n\nSample rate: {SR} Hz");
        tables.iter().for_each(Table::markdown);
    } else {
        println!("Stave Multiband: DSP characterisation at {SR} Hz");
        tables.iter().for_each(Table::text);
    }
}
