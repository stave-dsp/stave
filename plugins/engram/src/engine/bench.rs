//! The CORPUS BENCH — Engram's memory measured on real recordings (the
//! sample library's Test Corpus), not synthetic tones. Ignored tests;
//! run with
//! `cargo test -p stave-engram-vst3 --release --lib bench -- --ignored --nocapture`
//! (`ENGRAM_CORPUS` overrides the corpus path; a missing corpus skips).
//!
//! Two reports:
//! * EAR — per recording: how tonal it reads, its onsets, and what
//!   became of its grains (stored / repeats / incoherent / unplayed).
//!   The memory can only recall what it was willing to remember.
//! * RECALL — retrieval quality, the memory treated as what it is (an
//!   attention lookup): play a SESSION of recordings from different
//!   categories into a fresh bank, then play each again (a later part
//!   of it where there is one) with writing off, and measure where
//!   attention puts its weight — on the same recording (SELF), on its
//!   category (CAT), and how often the single strongest memory is the
//!   recording's own (TOP-1). And GENERALISATION: an unseen recording
//!   of a category in the session — does attention find its kin?
//!   Every figure sits next to CHANCE (the share of memories that
//!   would be right if attention were uniform).

extern crate std;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::path::PathBuf;
use std::println;

use super::bank::{Bank, Captured, Key, GRAIN_S, SLOTS};
use super::listen::{Hearing, Listen};

const SR: f32 = 48_000.0;
const BLOCK: usize = 256;
const DECIM: usize = 4;

pub(super) struct Src {
    pub(super) cat: String,
    pub(super) name: String,
    pub(super) audio: Vec<f32>,
    /// The recording's own length (s), before a one-shot was patterned.
    pub(super) dur: f32,
}

/// One-shots shorter than this are PLAYED, as a player would: repeated
/// into a 12 s pattern (eighths for hats, quarters at 120 for the rest),
/// accents alternating, each hit cut at the next with a short fade.
const ONESHOT_S: f32 = 1.5;

fn pattern(hit: &[f32], cat: &str) -> Vec<f32> {
    let every = if cat == "hats" { 0.25 } else { 0.5 };
    let (step, total) = ((every * SR) as usize, (12.0 * SR) as usize);
    let fade = (0.005 * SR) as usize;
    let mut out = alloc::vec![0.0f32; total];
    let len = hit.len().min(step);
    for (k, at) in (0..total).step_by(step).enumerate() {
        let accent = if k % 2 == 0 { 1.0 } else { 0.75 };
        for i in 0..len.min(total - at) {
            let f = if i + fade > len { (len - i) as f32 / fade as f32 } else { 1.0 };
            out[at + i] += hit[i] * accent * f;
        }
    }
    out
}

fn corpus_dir() -> PathBuf {
    std::env::var_os("ENGRAM_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("refs/corpus"))
}

/// Every WAV under the corpus (mono-summed, 48 kHz only), by category.
pub(super) fn load() -> Option<Vec<Src>> {
    let dir = corpus_dir();
    let mut cats: Vec<_> = std::fs::read_dir(&dir).ok()?.flatten().filter(|e| e.path().is_dir()).collect();
    cats.sort_by_key(|e| e.file_name());
    let mut out = Vec::new();
    for c in cats {
        let mut files: Vec<_> = std::fs::read_dir(c.path()).ok()?.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            if name.starts_with("._") || !name.ends_with(".wav") {
                continue;
            }
            let Ok(a) = stave_measure::wav::read(&f) else { continue };
            if a.sample_rate != SR as u32 {
                continue;
            }
            let n = a.channels[0].len();
            let k = 1.0 / a.channels.len() as f32;
            let audio: Vec<f32> = (0..n).map(|i| a.channels.iter().map(|ch| ch[i]).sum::<f32>() * k).collect();
            let cat = c.file_name().to_string_lossy().to_string();
            let dur = n as f32 / SR;
            let audio = if dur < ONESHOT_S { pattern(&audio, &cat) } else { audio };
            out.push(Src { cat, name: name.trim_end_matches(".wav").to_string(), audio, dur });
        }
    }
    Some(out)
}

/// The ear + bank, driven exactly as the engine drives them.
pub(super) struct Rig {
    pub(super) ear: Listen,
    pub(super) bank: Bank,
    acc: f32,
    phase: usize,
    pub(super) h: Hearing,
    n: usize,
    pub(super) focus: f32,
    pub(super) matching: f32,
    pub(super) min_age: u64,
}

impl Rig {
    pub(super) fn new(focus: f32, matching: f32) -> Self {
        let mut ear = Listen::new();
        ear.prepare(SR, SR / DECIM as f32, GRAIN_S);
        let mut bank = Bank::new();
        bank.prepare(SR);
        Self { ear, bank, acc: 0.0, phase: 0, h: Hearing::default(), n: 0, focus, matching, min_age: 0 }
    }

    pub(super) fn now(&self) -> Key {
        Key { chroma: self.h.chroma, timbre: self.h.timbre, rhythm: self.ear.rhythm(), tonal: self.h.tonal, percussive: false }
    }

    /// One frame. Returns what the bank did, the onset, and whether
    /// attention ran (a block boundary).
    pub(super) fn step(&mut self, x: f32, writing: bool) -> (Captured, bool, bool) {
        let mut attended = false;
        if self.n.is_multiple_of(BLOCK) {
            let now = self.now();
            self.bank.attend(now, self.ear.present(), self.focus, self.matching, self.min_age);
            attended = true;
        }
        self.n += 1;
        self.ear.air(x);
        self.acc += x;
        self.phase += 1;
        let mut onset = false;
        if self.phase == DECIM {
            self.ear.hear(self.acc / DECIM as f32);
            onset = self.ear.onset();
            self.h = self.ear.hearing();
            self.acc = 0.0;
            self.phase = 0;
        }
        let c = self.bank.capture(x, &self.h, self.ear.present(), onset, writing);
        let _ = self.bank.recall(onset, 0.15);
        (c, onset, attended)
    }

    pub(super) fn silence(&mut self, seconds: f32) {
        for _ in 0..(seconds * SR) as usize {
            self.step(0.0, false);
        }
    }
}

fn secs(x: &[f32], from: f32, len: f32) -> &[f32] {
    let a = ((from * SR) as usize).min(x.len());
    let b = (a + (len * SR) as usize).min(x.len());
    &x[a..b]
}

#[derive(Default, Debug, Clone, Copy)]
struct EarRow {
    present: f32,
    tonal: f32,
    onsets_s: f32,
    stored: usize,
    stored_perc: usize,
    repeats: usize,
    incoherent: usize,
    unplayed: usize,
    coh: f32,
}

fn ear_row(src: &Src) -> EarRow {
    let mut rig = Rig::new(0.6, 0.3);
    let x = secs(&src.audio, 0.0, 20.0);
    let mut r = EarRow::default();
    let (mut pres, mut tonal, mut onsets, mut coh_n) = (0usize, 0.0f32, 0usize, 0usize);
    for &v in x {
        let (c, onset, _) = rig.step(v, true);
        if rig.ear.present() {
            pres += 1;
            tonal += rig.h.tonal;
        }
        onsets += onset as usize;
        match c {
            Captured::Stored { slot } => {
                r.stored += 1;
                r.stored_perc += rig.bank.percussive(slot) as usize;
            }
            Captured::Repeat { .. } => r.repeats += 1,
            Captured::Incoherent { coherence, .. } => {
                r.incoherent += 1;
                r.coh += coherence;
                coh_n += 1;
            }
            Captured::Unplayed => r.unplayed += 1,
            Captured::Nothing => {}
        }
    }
    r.present = pres as f32 / x.len().max(1) as f32;
    r.tonal = tonal / pres.max(1) as f32;
    r.onsets_s = onsets as f32 / (x.len() as f32 / SR);
    r.coh /= coh_n.max(1) as f32;
    r
}

#[test]
#[ignore]
fn ear_report() {
    let Some(corpus) = load() else {
        println!("no corpus at {}; skipped", corpus_dir().display());
        return;
    };
    println!("{:<14} {:<34} {:>5} {:>5} {:>5} {:>6} | {:>4} {:>4} {:>4} {:>4} {:>4} {:>5}", "category", "recording", "dur", "pres", "tonal", "ons/s", "new", "perc", "rep", "inco", "unpl", "coh");
    let mut last = String::new();
    for s in &corpus {
        let r = ear_row(s);
        let cat = if s.cat != last { s.cat.clone() } else { String::new() };
        last = s.cat.clone();
        let name: String = s.name.chars().take(34).collect();
        println!(
            "{:<14} {:<34} {:>5.1} {:>5.2} {:>5.2} {:>6.1} | {:>4} {:>4} {:>4} {:>4} {:>4} {:>5.2}",
            cat, name, s.dur, r.present, r.tonal, r.onsets_s, r.stored, r.stored_perc, r.repeats, r.incoherent, r.unplayed, r.coh
        );
    }
}

/// One session's retrieval figures, summed over probes.
#[derive(Default, Clone, Copy)]
struct Score {
    self_mass: f32,
    cat_mass: f32,
    top1: f32,
    chance_self: f32,
    chance_cat: f32,
    probes: usize,
    // Generalisation (unseen recording of a session category).
    gen_cat: f32,
    gen_top1: f32,
    gen_chance: f32,
    gen_family: f32,
    gens: usize,
}

/// Sessions: groups of `per` recordings from distinct categories,
/// deterministic, covering every recording once.
fn sessions(corpus: &[Src], per: usize) -> Vec<Vec<usize>> {
    // A fixed shuffle, so groups mix categories without bias to order.
    let mut idx: Vec<usize> = (0..corpus.len()).collect();
    let mut s = 0x2545_F491u32;
    for i in (1..idx.len()).rev() {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        idx.swap(i, s as usize % (i + 1));
    }
    let mut left = idx;
    let mut out = Vec::new();
    while !left.is_empty() {
        let mut g: Vec<usize> = Vec::new();
        left.retain(|&i| {
            if g.len() < per && !g.iter().any(|&j| corpus[j].cat == corpus[i].cat) {
                g.push(i);
                false
            } else {
                true
            }
        });
        out.push(g);
    }
    out
}

const LEARN_S: f32 = 4.0;
const PROBE_S: f32 = 3.0;
/// Measure only once the ear has heard this much of a probe.
const SETTLE_S: f32 = 0.4;

/// The coarse FAMILY of a category: what a listener would call the
/// same kind of sound.
fn family(cat: &str) -> u8 {
    match cat {
        "drum loops" | "hats" | "kicks" | "snares" | "percussion" => 0,
        "guitar" | "piano notes" | "synth notes" | "tonal loops" => 1,
        "vocals" | "vocal phrases" => 2,
        _ => 3,
    }
}

/// Kin: the same category, or the very same audio filed elsewhere.
fn kin(corpus: &[Src], a: usize, b: usize) -> bool {
    corpus[a].cat == corpus[b].cat || corpus[a].name == corpus[b].name
}

/// Probe with `x` (writing off); accumulate attention mass per label.
fn probe(rig: &mut Rig, x: &[f32], labels: &[Option<usize>], corpus: &[Src], want: usize, cat_only: bool) -> Option<(f32, f32, f32, usize, f32)> {
    let (mut m_self, mut m_cat, mut top1, mut n, mut fam1) = (0.0f32, 0.0f32, 0.0f32, 0usize, 0.0f32);
    let mut votes = alloc::vec![0usize; corpus.len()];
    for (i, &v) in x.iter().enumerate() {
        let (_, _, attended) = rig.step(v, false);
        if !attended || (i as f32) < SETTLE_S * SR || !rig.ear.present() {
            continue;
        }
        let (mut best, mut best_w) = (None, 0.0f32);
        for (slot, l) in labels.iter().enumerate() {
            let Some(l) = *l else { continue };
            let w = rig.bank.target(slot);
            if w > best_w {
                best_w = w;
                best = Some(l);
            }
            if l == want {
                m_self += w;
            }
            if kin(corpus, l, want) {
                m_cat += w;
            }
        }
        if let Some(b) = best {
            votes[b] += 1;
            fam1 += (family(&corpus[b].cat) == family(&corpus[want].cat)) as u8 as f32;
        }
        let hit = match best {
            Some(b) if cat_only => kin(corpus, b, want),
            Some(b) => b == want,
            None => false,
        };
        top1 += hit as u8 as f32;
        n += 1;
    }
    let winner = (0..votes.len()).max_by_key(|&k| votes[k]).unwrap_or(want);
    (n > 0).then(|| (m_self / n as f32, m_cat / n as f32, top1 / n as f32, winner, fam1 / n as f32))
}

fn run_sessions(corpus: &[Src], focus: f32, matching: f32, verbose: bool) -> Score {
    let mut total = Score::default();
    for group in sessions(corpus, 6) {
        let mut rig = Rig::new(focus, matching);
        let mut labels: Vec<Option<usize>> = alloc::vec![None; SLOTS];
        for &i in &group {
            for &v in secs(&corpus[i].audio, 0.0, LEARN_S) {
                if let (Captured::Stored { slot }, _, _) = rig.step(v, true) {
                    labels[slot] = Some(i);
                }
            }
            rig.silence(1.0);
        }
        let filled = labels.iter().flatten().count().max(1) as f32;
        for &i in &group {
            let own = labels.iter().flatten().filter(|&&l| l == i).count() as f32;
            let kin = labels.iter().flatten().filter(|&&l| corpus[l].cat == corpus[i].cat).count() as f32;
            // A later part where the recording has one.
            let a = &corpus[i].audio;
            let from = if a.len() as f32 / SR >= LEARN_S + PROBE_S { LEARN_S } else { 0.0 };
            if let Some((s, c, t, _, _)) = probe(&mut rig, secs(a, from, PROBE_S), &labels, corpus, i, false) {
                total.self_mass += s;
                total.cat_mass += c;
                total.top1 += t;
                total.chance_self += own / filled;
                total.chance_cat += kin / filled;
                total.probes += 1;
                if verbose {
                    println!("  {:<12} {:<30} own {:>2}  self {:.2} (chance {:.2})  top1 {:.2}", corpus[i].cat, corpus[i].name.chars().take(30).collect::<String>(), own as usize, s, own / filled, t);
                }
            }
            rig.silence(1.0);
            // Generalisation: an unseen sibling — the next one along in
            // its category (so every recording gets probed in turn), never
            // the same audio filed twice.
            let n = corpus.len();
            if let Some(j) = (1..n).map(|d| (i + d) % n).find(|&j| corpus[j].cat == corpus[i].cat && corpus[j].name != corpus[i].name && !group.contains(&j)) {
                if kin > 0.0 {
                    if let Some((_, c, t, w, f)) = probe(&mut rig, secs(&corpus[j].audio, 0.0, PROBE_S), &labels, corpus, i, true) {
                        total.gen_cat += c;
                        total.gen_top1 += t;
                        total.gen_family += f;
                        total.gen_chance += kin / filled;
                        total.gens += 1;
                        if verbose {
                            println!(
                                "  GEN {:<12} {:<26} → kin mass {:.2} top-1 {:.2}  mostly: {} / {}",
                                corpus[j].cat,
                                corpus[j].name.chars().take(26).collect::<String>(),
                                c,
                                t,
                                corpus[w].cat,
                                corpus[w].name.chars().take(22).collect::<String>()
                            );
                        }
                    }
                }
                rig.silence(1.0);
            }
        }
    }
    total
}

fn print_score(label: &str, s: &Score) {
    let p = s.probes.max(1) as f32;
    let g = s.gens.max(1) as f32;
    println!(
        "{label:<22} SELF {:.2} (chance {:.2})  TOP-1 {:.2} | GEN kin {:.2} (chance {:.2}) top-1 {:.2} family-top-1 {:.2}  [{} probes, {} gen]",
        s.self_mass / p, s.chance_self / p, s.top1 / p, s.gen_cat / g, s.gen_chance / g, s.gen_top1 / g, s.gen_family / g, s.probes, s.gens
    );
}

#[test]
#[ignore]
fn recall_report() {
    let Some(corpus) = load() else {
        println!("no corpus at {}; skipped", corpus_dir().display());
        return;
    };
    let verbose = std::env::var_os("ENGRAM_VERBOSE").is_some();
    if std::env::var_os("ENGRAM_QUICK").is_some() {
        let s = run_sessions(&corpus, 0.6, 0.3, verbose);
        print_score("match 0.3 focus 0.6", &s);
        let s = run_sessions(&corpus, 0.6, 1.0, false);
        print_score("match 1.0 focus 0.6", &s);
        return;
    }
    for matching in [0.0, 0.3, 0.6, 1.0] {
        let s = run_sessions(&corpus, 0.6, matching, verbose);
        print_score(&std::format!("match {matching:.1} focus 0.6"), &s);
    }
    for focus in [0.2, 1.0] {
        let s = run_sessions(&corpus, focus, 0.3, false);
        print_score(&std::format!("match 0.3 focus {focus:.1}"), &s);
    }
}

#[test]
#[ignore]
fn trace_one() {
    let Some(corpus) = load() else { return };
    let want = std::env::var("ENGRAM_FILE").unwrap_or_default();
    for s in corpus.iter().filter(|s| s.name.contains(&want)) {
        println!("== {} / {}", s.cat, s.name);
        let mut rig = Rig::new(0.6, 0.3);
        for &v in secs(&s.audio, 0.0, 8.0) {
            rig.step(v, true);
        }
    }
}

/// How alike is the BEST memory, when the sound is (a) one it learned,
/// (b) an unseen sibling, (c) of a kind it never heard? An attention
/// sink must sit between (b)/(a) and (c).
#[test]
#[ignore]
fn best_similarity_report() {
    let Some(corpus) = load() else { return };
    let matching: f32 = std::env::var("ENGRAM_MATCH").ok().and_then(|v| v.parse().ok()).unwrap_or(0.3);
    let (mut own, mut sib, mut absent) = (Vec::new(), Vec::new(), Vec::new());
    let measure = |rig: &mut Rig, x: &[f32], out: &mut Vec<f32>| {
        let mut acc = Vec::new();
        for (i, &v) in x.iter().enumerate() {
            let (_, _, attended) = rig.step(v, false);
            if attended && i as f32 > SETTLE_S * SR && rig.ear.present() {
                let mass: f32 = (0..SLOTS).map(|k| rig.bank.target(k)).sum();
                acc.push(if std::env::var_os("ENGRAM_MASS").is_some() { mass } else { 1.0 - rig.bank.novelty() });
            }
        }
        if !acc.is_empty() {
            out.push(acc.iter().sum::<f32>() / acc.len() as f32);
        }
        rig.silence(1.0);
    };
    for group in sessions(&corpus, 6) {
        let mut rig = Rig::new(0.6, matching);
        for &i in &group {
            for &v in secs(&corpus[i].audio, 0.0, LEARN_S) {
                rig.step(v, true);
            }
            rig.silence(1.0);
        }
        let fams: Vec<u8> = group.iter().map(|&i| family(&corpus[i].cat)).collect();
        for &i in &group {
            let a = &corpus[i].audio;
            let from = if a.len() as f32 / SR >= LEARN_S + PROBE_S { LEARN_S } else { 0.0 };
            measure(&mut rig, secs(a, from, PROBE_S), &mut own);
        }
        for j in 0..corpus.len() {
            if group.contains(&j) {
                continue;
            }
            if group.iter().any(|&i| corpus[i].cat == corpus[j].cat) {
                measure(&mut rig, secs(&corpus[j].audio, 0.0, PROBE_S), &mut sib);
            } else if !fams.contains(&family(&corpus[j].cat)) {
                measure(&mut rig, secs(&corpus[j].audio, 0.0, PROBE_S), &mut absent);
            }
        }
    }
    let q = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        let at = |p: f32| v[((v.len() - 1) as f32 * p) as usize];
        alloc::format!("p10 {:.2} p25 {:.2} p50 {:.2} p75 {:.2} p90 {:.2} (n {})", at(0.1), at(0.25), at(0.5), at(0.75), at(0.9), v.len())
    };
    println!("match {matching}: {}", if std::env::var_os("ENGRAM_MASS").is_some() { "attention on memories (1 − sink)" } else { "best similarity" });
    println!("  learned      {}", q(&mut own));
    println!("  sibling      {}", q(&mut sib));
    println!("  other family {}", q(&mut absent));
}

/// TRAINING DATA for the learned embedding (train/train.py): every
/// recording in `ENGRAM_TRAIN_LIST` (train/prep.py's list.tsv), heard by
/// the engine's own ear under augmentation — pitch (resampled ±7
/// semitones; view 0 untouched), tilt EQ, level — and described window
/// by window. Writes `<list dir>/desc.f32` (D floats per window) and
/// `desc.idx` (u32 recording, u32 view per window).
#[test]
#[ignore]
fn dump_training() {
    use super::describe::D;
    let Some(list) = std::env::var_os("ENGRAM_TRAIN_LIST").map(PathBuf::from) else {
        println!("set ENGRAM_TRAIN_LIST");
        return;
    };
    let views: usize = std::env::var("ENGRAM_VIEWS").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let text = std::fs::read_to_string(&list).unwrap();
    let files: Vec<String> = text.lines().map(|l| l.split('\t').next().unwrap().to_string()).collect();
    let threads = 8;
    let chunks: Vec<Vec<(usize, String)>> = (0..threads).map(|t| files.iter().cloned().enumerate().filter(|(i, _)| i % threads == t).collect()).collect();
    let handles: Vec<_> = chunks
        .into_iter()
        .map(|chunk| {
            std::thread::spawn(move || {
                let mut out: Vec<(u32, u32, [f32; D])> = Vec::new();
                for (fi, path) in chunk {
                    // Header only: the library holds multi-hour renders
                    // (GBs each). Decoding whole files on 8 threads took
                    // the machine down (2026-11-10 watchdog panic), so each
                    // view reads just its own window.
                    let path = std::path::Path::new(&path);
                    let Ok(info) = stave_measure::wav::probe(path) else { continue };
                    let n = info.frames;
                    if n < 64 {
                        continue;
                    }
                    let mut rng = 0x9E37_79B9u32 ^ (fi as u32).wrapping_mul(0x85EB_CA6B);
                    let mut rnd = move || {
                        rng ^= rng << 13;
                        rng ^= rng >> 17;
                        rng ^= rng << 5;
                        rng as f32 / u32::MAX as f32
                    };
                    for v in 0..views {
                        let semis = if v == 0 { 0.0 } else { (rnd() * 14.0 - 7.0).round() };
                        let ratio = 2f64.powf(f64::from(semis) / 12.0) * f64::from(info.sample_rate) / f64::from(SR);
                        // A random ≤ 8 s stretch of the source (long
                        // recordings are minutes to hours), read alone,
                        // resampled (linear) at the pitch ratio, positions
                        // in f64.
                        let span = ((8.0 * f64::from(info.sample_rate)) as usize).min(n);
                        let from = if n > span { (rnd() * (n - span) as f32) as usize } else { 0 };
                        let Ok(a) = stave_measure::wav::read_range(path, from, span) else { continue };
                        let span = a.channels[0].len();
                        if span < 64 {
                            continue;
                        }
                        let k = 1.0 / a.channels.len() as f32;
                        let src: Vec<f32> = (0..span).map(|i| a.channels.iter().map(|c| c[i]).sum::<f32>() * k).collect();
                        drop(a);
                        let len = ((span as f64 - 1.0) / ratio) as usize;
                        let mut x: Vec<f32> = (0..len)
                            .map(|i| {
                                let p = i as f64 * ratio;
                                let j = (p as usize).min(span - 1);
                                let f = (p - j as f64) as f32;
                                src[j] + (src[(j + 1).min(span - 1)] - src[j]) * f
                            })
                            .collect();
                        // Tilt EQ: blend toward a one-pole low- or high-pass.
                        if v > 0 {
                            let tilt = rnd() * 0.8 - 0.4;
                            let c = 0.15;
                            let mut lp = 0.0f32;
                            for s in x.iter_mut() {
                                lp += (*s - lp) * c;
                                let hp = *s - lp;
                                *s = if tilt > 0.0 { *s + tilt * hp } else { *s + (-tilt) * (lp - *s) };
                            }
                        }
                        let peak = x.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-9);
                        let gain = 10f32.powf((-(rnd() * 24.0) - 2.0) / 20.0) / peak;
                        x.iter_mut().for_each(|s| *s *= gain);
                        // A short sound is PLAYED: repeated at a random pace.
                        let dur = x.len() as f32 / SR;
                        let input: Vec<f32> = if dur < 1.5 {
                            let every = ((0.25 + rnd() * 0.5) * SR) as usize;
                            let total = (4.0 * SR) as usize;
                            let mut o = alloc::vec![0.0f32; total];
                            let hit = x.len().min(every);
                            for at in (0..total).step_by(every) {
                                for i in 0..hit.min(total - at) {
                                    let f = if i + 240 > hit { (hit - i) as f32 / 240.0 } else { 1.0 };
                                    o[at + i] += x[i] * f;
                                }
                            }
                            o
                        } else {
                            let seg = (6.0 * SR) as usize;
                            let start = if x.len() > seg { (rnd() * (x.len() - seg) as f32) as usize } else { 0 };
                            x[start..(start + seg).min(x.len())].to_vec()
                        };
                        let mut ear = Listen::new();
                        ear.prepare(SR, SR / DECIM as f32, GRAIN_S);
                        let (mut acc, mut windows, mut got) = (0.0f32, 0usize, Vec::new());
                        for (i, &s) in input.iter().enumerate() {
                            ear.air(s);
                            acc += s;
                            if i % DECIM == DECIM - 1 {
                                ear.hear(acc / DECIM as f32);
                                acc = 0.0;
                                if ear.window_fresh() {
                                    windows += 1;
                                    if windows >= 8 && ear.window_played() >= 0.85 {
                                        if let Some(d) = ear.descriptor() {
                                            got.push(d);
                                        }
                                    }
                                }
                            }
                        }
                        // At most 8 windows per view, spread evenly.
                        let step = got.len().div_ceil(8).max(1);
                        for d in got.into_iter().step_by(step) {
                            out.push((fi as u32, v as u32, d));
                        }
                    }
                }
                out
            })
        })
        .collect();
    let mut all = Vec::new();
    for h in handles {
        all.extend(h.join().unwrap());
    }
    let dir = list.parent().unwrap();
    let mut desc = Vec::with_capacity(all.len() * D * 4);
    let mut idx = Vec::with_capacity(all.len() * 8);
    for (f, v, d) in &all {
        for x in d {
            desc.extend_from_slice(&x.to_le_bytes());
        }
        idx.extend_from_slice(&f.to_le_bytes());
        idx.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(dir.join("desc.f32"), desc).unwrap();
    std::fs::write(dir.join("desc.idx"), idx).unwrap();
    println!("{} windows (D = {D}) from {} recordings x {views} views -> {}", all.len(), files.len(), dir.display());
}
