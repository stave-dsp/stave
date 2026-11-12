//! LISTENING renders: Engram played real recordings from the sample
//! library's Test Corpus, through the whole plugin — for the ear, not
//! for an assert. Ignored; run with
//! `cargo test -p stave-engram-vst3 --release --test corpus_renders -- --ignored --nocapture`.
//! Writes to `build/engram-demos/`: each scene as `<scene>.wav` (what you
//! would hear) and `<scene>-memory.wav` (ONLY what the memory added:
//! the render with RECALL up minus the same with RECALL 0, plasticity
//! off so the room is the same in both). Corpus audio is licensed for
//! productions — these renders stay local (build/ is not committed).

use std::path::{Path, PathBuf};

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn corpus() -> PathBuf {
    std::env::var_os("ENGRAM_CORPUS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("refs/corpus"))
}

/// A corpus recording (mono) by category and name prefix.
fn find(cat: &str, name: &str) -> Option<Vec<f32>> {
    let dir = corpus().join(cat);
    let mut files: Vec<_> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).collect();
    files.sort();
    let f = files.into_iter().find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(name)))?;
    let a = stave_measure::wav::read(&f).ok()?;
    let k = 1.0 / a.channels.len() as f32;
    Some((0..a.channels[0].len()).map(|i| a.channels.iter().map(|c| c[i]).sum::<f32>() * k).collect())
}

/// `secs` of `x` from `from`, looped if it runs out.
fn take(x: &[f32], from: f32, secs: f32) -> Vec<f32> {
    let n = (secs * FS) as usize;
    let s = (from * FS) as usize % x.len().max(1);
    (0..n).map(|i| x[(s + i) % x.len()]).collect()
}

/// A one-shot played as a pattern, `every` seconds, accents alternating.
fn pattern(hit: &[f32], every: f32, secs: f32) -> Vec<f32> {
    let step = (every * FS) as usize;
    let mut out = vec![0.0f32; (secs * FS) as usize];
    let len = hit.len().min(step);
    let fade = 240;
    let total = out.len();
    for (k, at) in (0..total).step_by(step).enumerate() {
        let accent = if k % 4 == 0 { 1.0 } else if k % 2 == 0 { 0.8 } else { 0.65 };
        for i in 0..len.min(total - at) {
            let f = if i + fade > len { (len - i) as f32 / fade as f32 } else { 1.0 };
            out[at + i] += hit[i] * accent * f;
        }
    }
    out
}

fn silence(secs: f32) -> Vec<f32> {
    vec![0.0; (secs * FS) as usize]
}

fn render(set: &[(&str, f64)], input: &[f32]) -> Vec<f32> {
    let mut plugin = stave_engram_vst3::Engram::new();
    plugin.debug_attach(DebugBus::new());
    let table = plugin.param_table();
    for (n, v) in set {
        table.by_name(n).unwrap_or_else(|| panic!("param '{n}'")).set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let mut out = Vec::with_capacity(input.len());
    let (mut il, mut ir, mut ol, mut or) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for chunk in input.chunks(BLOCK) {
        il[..chunk.len()].copy_from_slice(chunk);
        il[chunk.len()..].fill(0.0);
        ir.copy_from_slice(&il);
        let mut ip = [core::ptr::null_mut(); 2];
        let mut op = [core::ptr::null_mut(); 2];
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input: AudioBuffer::from_slices(&mut ip, [&mut il, &mut ir], BLOCK),
            output: AudioBuffer::from_slices(&mut op, [&mut ol, &mut or], BLOCK),
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        out.extend_from_slice(&ol[..chunk.len()]);
    }
    out
}

fn write(dir: &Path, name: &str, x: &[f32]) {
    // Normalise to −1 dBFS peak so every file is comfortable to audition.
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    let g = 0.89 / peak;
    let audio = stave_measure::wav::Audio { sample_rate: FS as u32, channels: vec![x.iter().map(|v| v * g).collect()] };
    stave_measure::wav::write(&dir.join(format!("{name}.wav")), &audio).expect("write");
}

struct Scene {
    name: &'static str,
    input: Vec<f32>,
    set: Vec<(&'static str, f64)>,
    why: &'static str,
}

fn cat(parts: &[Vec<f32>]) -> Vec<f32> {
    parts.concat()
}

#[test]
#[ignore]
fn render_corpus_scenes() {
    let Some(house) = find("drum loops", "Tech House") else {
        println!("no corpus at {}; skipped", corpus().display());
        return;
    };
    let piano = find("piano notes", "GrandPiano C2").unwrap();
    let vocal = find("vocal phrases", "Vocal Demo Lead").unwrap();
    let bells = find("tonal loops", "Organic Bells").unwrap();
    let kick = find("kicks", "Kick 007").unwrap();
    let snare = find("snares", "Snare Crack").unwrap();
    let hat = find("hats", "Hihat Closed 2B").unwrap();
    let jazz = find("drum loops", "Jazz Break").unwrap();
    let guitar = find("guitar", "Electric Guitar Funky").unwrap();

    let scenes = vec![
        Scene {
            name: "1-groove-returns",
            input: cat(&[take(&house, 0.0, 7.0), take(&piano, 0.0, 6.0), take(&house, 0.0, 7.0), silence(5.0)]),
            set: vec![("match", 0.6)],
            why: "house, then a piano note, then the house again: the room should bring the first groove back under the second",
        },
        Scene {
            name: "2-voice-and-bells",
            input: cat(&[take(&vocal, 0.0, 9.0), take(&bells, 0.0, 6.0), take(&vocal, 9.0, 9.0), silence(6.0)]),
            set: vec![],
            why: "a sung lead, bells, more of the lead: harmony-led recall of the earlier phrases",
        },
        Scene {
            name: "3-kit-answers",
            input: cat(&[pattern(&kick, 0.5, 6.0), pattern(&hat, 0.25, 6.0), pattern(&snare, 0.5, 6.0), pattern(&kick, 0.5, 6.0), silence(4.0)]),
            set: vec![("match", 0.8), ("texture", 0.35)],
            why: "kicks, hats, snares, kicks: remembered hits answer yours, by sound",
        },
        Scene {
            name: "4-break-cloud",
            input: cat(&[take(&jazz, 0.0, 8.0), take(&guitar, 0.0, 6.0), take(&jazz, 0.0, 8.0), silence(5.0)]),
            set: vec![("match", 1.0), ("texture", 0.75), ("focus", 0.4)],
            why: "a jazz break (rejected entirely before today) recalled as a scattered cloud",
        },
        Scene {
            name: "5-shattered-voice",
            input: cat(&[take(&vocal, 0.0, 9.0), take(&bells, 0.0, 6.0), take(&vocal, 9.0, 9.0), silence(6.0)]),
            set: vec![("texture", 0.95), ("match", 0.5), ("recall", 0.9), ("decay", 6.0)],
            why: "the voice scene at the experimental end: reversed, octave-leaping shards",
        },
    ];

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/engram-demos");
    std::fs::create_dir_all(&dir).unwrap();
    for s in &scenes {
        let mut heard: Vec<(&str, f64)> = vec![("mix", 0.5), ("recall", 0.8)];
        heard.extend_from_slice(&s.set);
        let mut on: Vec<(&str, f64)> = vec![("mix", 1.0), ("recall", 0.8), ("plasticity", 0.0)];
        on.extend(s.set.iter().filter(|(n, _)| *n != "mix"));
        let mut off = on.clone();
        off.push(("recall", 0.0));
        let mix = render(&heard, &s.input);
        let (a, b) = (render(&on, &s.input), render(&off, &s.input));
        let memory: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        println!("{:<20} memory/room {:>5.1} dB   {}", s.name, 20.0 * (rms(&memory) / rms(&b).max(1e-9)).log10(), s.why);
        write(&dir, s.name, &mix);
        write(&dir, &format!("{}-memory", s.name), &memory);
    }
    println!("→ {}", dir.canonicalize().unwrap().display());
}
