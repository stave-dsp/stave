//! DSP.md R7 in CI: the solver fuzzer's rules (`stave_test::solver_fuzz`)
//! over a seeded corpus — every deck, both methods and DK with ADAA,
//! units drawn up to ±50 % off their values, controls anywhere, rates
//! from 8 to 768 kHz, sines, squares, the Nyquist tone, impulses, steps
//! and noise from a tenth to ten times each deck's hot level — and the
//! committed regressions, the inputs libFuzzer once broke a rule with
//! (`fuzz/fuzz_targets/solvers.rs`; replay a new one with
//! `Case::from_bytes`). Cases run on `stave_test::threads()`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use stave_test::solver_fuzz::{run, Case, Failure, Report, DECKS};

/// Seeded cases a CI run takes: every deck about ten times.
const CORPUS: u64 = 288;

/// The minimised inputs a fuzz run failed on, kept as regressions.
const REGRESSIONS: &[&[u8]] = &[];

/// Run `cases` in parallel; every failure, and each case's report.
fn run_all(cases: &[Case]) -> (Vec<Failure>, Vec<(Case, Report)>) {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let reports = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..stave_test::threads().min(cases.len()).max(1) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(case) = cases.get(i) else { break };
                match run(case) {
                    Ok(r) => reports.lock().unwrap().push((*case, r)),
                    Err(f) => failures.lock().unwrap().push(*f),
                }
            });
        }
    });
    (failures.into_inner().unwrap(), reports.into_inner().unwrap())
}

#[test]
fn seeded_cases_keep_the_solver_rules() {
    let cases: Vec<Case> = (0..CORPUS).map(Case::seeded).collect();
    let (failures, reports) = run_all(&cases);
    let mut per_deck = vec![(0usize, 0usize, 0u64, 0u64, 0.0f64); DECKS.len()];
    for (case, r) in &reports {
        let d = &mut per_deck[case.deck];
        if r.refused.is_some() {
            d.1 += 1;
        } else {
            d.0 += 1;
        }
        d.2 += r.solves;
        d.3 += r.capped;
        d.4 = d.4.max(r.divergence);
    }
    println!("  {:<20} {:>4} {:>8} {:>9} {:>7} {:>10}", "deck", "ran", "refused", "solves", "capped", "divergence");
    for (deck, (ran, refused, solves, capped, divergence)) in DECKS.iter().zip(&per_deck) {
        println!("  {:<20} {ran:>4} {refused:>8} {solves:>9} {capped:>7} {divergence:>10.1e}", deck.name);
    }
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty(), "{} of {CORPUS} cases broke a rule (above)", failures.len());
    assert!(per_deck.iter().all(|d| d.0 > 0), "every deck ran at least once");
}

#[test]
fn committed_regressions_keep_the_solver_rules() {
    let cases: Vec<Case> = REGRESSIONS.iter().map(|b| Case::from_bytes(b)).collect();
    let (failures, _) = run_all(&cases);
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

/// The rules can fail: a solver held to one iteration caps, and the
/// fuzzer's report counts it.
#[test]
fn a_starved_solver_reports_its_caps() {
    use stave_dsp::circuit::methods::dk::{Dk, Settings};
    use stave_dsp::circuit::methods::newton::NewtonSettings;
    use stave_dsp::circuit::Circuit;
    let case = Case {
        deck: 0,
        level: 3.0,
        rate: 48_000.0,
        stimulus: stave_test::solver_fuzz::Stimulus::Sine { hz: 1_000.0 },
        ..Case::seeded(0)
    };
    let netlist = case.netlist().unwrap();
    let mut c = Circuit::<Dk>::new(&netlist).unwrap();
    c.set_settings(Settings {
        newton: NewtonSettings { max_iterations: 1, ..NewtonSettings::DEFAULT },
        continuation_steps: 0,
    });
    c.drive(DECKS[0].source).unwrap();
    c.prepare(case.rate);
    let mut capped = 0;
    for n in 0..case.samples {
        c.tick_f64(case.input(n));
        capped += c.take_health().newton.capped;
    }
    assert!(capped > 0, "one Newton iteration on a hot clipper must cap");
}

/// A tool, not a check: `STAVE_FUZZ_REPLAY='<file>;[76,76];…' cargo
/// test -p stave-test --test solver_fuzz -- --ignored replay
/// --nocapture` runs each fuzz input (a file, or its bytes as a list) —
/// a crash, or a slow unit — and prints its case, its report and how
/// long it took.
#[test]
#[ignore = "a tool: replays fuzz inputs"]
fn replay() {
    let inputs = std::env::var("STAVE_FUZZ_REPLAY").unwrap_or_default();
    for path in inputs.split(';').filter(|p| !p.is_empty()) {
        let bytes = match path.strip_prefix('[').and_then(|p| p.strip_suffix(']')) {
            Some(list) => list.split(',').filter_map(|b| b.trim().parse().ok()).collect(),
            None => std::fs::read(path).unwrap(),
        };
        let case = Case::from_bytes(&bytes);
        let started = std::time::Instant::now();
        let result = run(&case);
        println!(
            "{path}: {} {case:?}\n  {result:?} in {:.2} s",
            DECKS[case.deck].name,
            started.elapsed().as_secs_f64()
        );
    }
}
