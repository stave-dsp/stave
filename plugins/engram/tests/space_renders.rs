//! The GENERATIVE ROOM, closed loop: the whole plugin's impulse response
//! at points on the space map, for train/space_check.py to measure with
//! the same tool that measured the real IRs. Ignored; driven by
//! `ENGRAM_SPACE_LIST` (TSV: name, x, y) and `ENGRAM_SPACE_OUT` (a dir),
//! optional `ENGRAM_SPACE_AMOUNT` (SPACE, default 1).
//! Writes `<out>/<name>.wav`: stereo, 12 s, full wet, memory off.

use std::path::PathBuf;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const SECONDS: f32 = 12.0;
const DIRECT: f32 = 1.0;

fn render_ir(set: &[(&str, f64)]) -> [Vec<f32>; 2] {
    let mut plugin = stave_engram_vst3::Engram::new();
    plugin.debug_attach(DebugBus::new());
    let table = plugin.param_table();
    for (n, v) in set {
        table.by_name(n).unwrap_or_else(|| panic!("param '{n}'")).set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let n = (SECONDS * FS) as usize;
    let mut out = [Vec::with_capacity(n), Vec::with_capacity(n)];
    let (mut il, mut ir, mut ol, mut or) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    // Two silent blocks first: the room settles where the params say.
    let lead = 2 * BLOCK;
    for at in (0..n + lead).step_by(BLOCK) {
        il.fill(0.0);
        if at == lead {
            il[0] = 1.0;
        }
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
        if at >= lead {
            out[0].extend_from_slice(&ol);
            out[1].extend_from_slice(&or);
        }
    }
    out[0].truncate(n);
    out[1].truncate(n);
    // The DIRECT sound (the dry path's job, at MIX 1 absent): measured
    // IRs start at it, so the measurement aligns to it the same way.
    out[0][0] += DIRECT;
    out[1][0] += DIRECT;
    out
}

#[test]
#[ignore]
fn render_space_irs() {
    let (Some(list), Some(dir)) = (std::env::var_os("ENGRAM_SPACE_LIST"), std::env::var_os("ENGRAM_SPACE_OUT")) else {
        println!("set ENGRAM_SPACE_LIST and ENGRAM_SPACE_OUT");
        return;
    };
    let amount: f64 = std::env::var("ENGRAM_SPACE_AMOUNT").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let size: f64 = std::env::var("ENGRAM_SPACE_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5);
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let text = std::fs::read_to_string(list).unwrap();
    let mut n = 0;
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (name, x, y): (&str, f64, f64) = (f[0], f[1].parse().unwrap(), f[2].parse().unwrap());
        let ir = render_ir(&[
            ("mix", 1.0), ("recall", 0.0), ("plasticity", 0.0), ("drift", 0.0), ("steer", 0.0),
            ("space", amount), ("space x", x), ("space y", y), ("size", size),
        ]);
        let audio = stave_measure::wav::Audio { sample_rate: FS as u32, channels: ir.to_vec() };
        stave_measure::wav::write(&dir.join(format!("{name}.wav")), &audio).unwrap();
        n += 1;
    }
    println!("{n} impulse responses -> {}", dir.display());
}

/// LISTENING tour: one recording (`ENGRAM_TOUR_INPUT`, a WAV) through
/// map points (`ENGRAM_TOUR`, TSV: label, x, y, drift, steer, decay),
/// stereo WAVs (−1 dBFS peak) to `ENGRAM_SPACE_OUT`. MIX 0.5, memory on.
#[test]
#[ignore]
fn render_space_tour() {
    let (Some(list), Some(input), Some(dir)) =
        (std::env::var_os("ENGRAM_TOUR"), std::env::var_os("ENGRAM_TOUR_INPUT"), std::env::var_os("ENGRAM_SPACE_OUT"))
    else {
        println!("set ENGRAM_TOUR, ENGRAM_TOUR_INPUT and ENGRAM_SPACE_OUT");
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = stave_measure::wav::read(std::path::Path::new(&input)).unwrap();
    assert_eq!(a.sample_rate, FS as u32, "tour input must be 48 kHz");
    let k = 1.0 / a.channels.len() as f32;
    let mut x: Vec<f32> = (0..a.channels[0].len()).map(|i| a.channels.iter().map(|c| c[i]).sum::<f32>() * k).collect();
    x.extend(std::iter::repeat_n(0.0, (FS * 4.0) as usize));
    for line in std::fs::read_to_string(list).unwrap().lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let num = |i: usize| -> f64 { f[i].parse().unwrap() };
        let set = [
            ("mix", 0.5), ("recall", 0.5), ("plasticity", 0.3), ("space", 1.0),
            ("space x", num(1)), ("space y", num(2)), ("drift", num(3)), ("steer", num(4)), ("decay", num(5)),
        ];
        let out = render_stream(&set, &x);
        let peak = out.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
        let g = 0.89 / peak;
        let audio = stave_measure::wav::Audio { sample_rate: FS as u32, channels: out.iter().map(|c| c.iter().map(|v| v * g).collect()).collect() };
        stave_measure::wav::write(&dir.join(format!("{}.wav", f[0])), &audio).unwrap();
        println!("{}", f[0]);
    }
}

fn render_stream(set: &[(&str, f64)], x: &[f32]) -> [Vec<f32>; 2] {
    let mut plugin = stave_engram_vst3::Engram::new();
    plugin.debug_attach(DebugBus::new());
    let table = plugin.param_table();
    for (n, v) in set {
        table.by_name(n).unwrap_or_else(|| panic!("param '{n}'")).set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let mut out = [Vec::with_capacity(x.len()), Vec::with_capacity(x.len())];
    let (mut il, mut ir, mut ol, mut or) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for chunk in x.chunks(BLOCK) {
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
        out[0].extend_from_slice(&ol[..chunk.len()]);
        out[1].extend_from_slice(&or[..chunk.len()]);
    }
    out
}

/// MORPH quality: pink-ish noise through the generated room, (a) static,
/// (b) the cursor sweeping a loop across the map in `ENGRAM_MORPH_S` s,
/// (c) static cursor at full DRIFT. Stereo WAVs to `ENGRAM_SPACE_OUT`
/// (morph-static, morph-sweep, morph-drift) for train/morph_check.py.
#[test]
#[ignore]
fn render_morphs() {
    let Some(dir) = std::env::var_os("ENGRAM_SPACE_OUT") else {
        println!("set ENGRAM_SPACE_OUT");
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let secs: f32 = std::env::var("ENGRAM_MORPH_S").ok().and_then(|v| v.parse().ok()).unwrap_or(8.0);
    let n = (secs * FS) as usize;
    // Pink-ish noise (three one-poles over white), steady level.
    let mut s = 0x2468_ACE1u32;
    let (mut a, mut b, mut c) = (0.0f32, 0.0f32, 0.0f32);
    let tone = std::env::var("ENGRAM_MORPH_INPUT").is_ok_and(|v| v == "tone");
    let x: Vec<f32> = (0..n)
        .map(|i| {
            if tone {
                // A held chord, soft harmonics: clicks and zipper are
                // loudest on sustained tones (noise masks them).
                let t = i as f32 / FS;
                return [110.0f32, 164.8, 220.0, 277.2]
                    .iter()
                    .map(|f| (1..4).map(|h| (std::f32::consts::TAU * f * h as f32 * t).sin() * 0.06 / h as f32).sum::<f32>())
                    .sum();
            }
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let w = s as f32 / u32::MAX as f32 * 2.0 - 1.0;
            a = 0.997 * a + 0.03 * w;
            b = 0.96 * b + 0.06 * w;
            c = 0.6 * c + 0.1 * w;
            (a + b + c + 0.05 * w) * 0.8
        })
        .collect();
    let base = [("mix", 1.0), ("recall", 0.0), ("plasticity", 0.0), ("steer", 0.0), ("space", 1.0)];
    let render = |set: &[(&str, f64)], path: &dyn Fn(usize) -> Option<(f64, f64)>| -> [Vec<f32>; 2] {
        let mut plugin = stave_engram_vst3::Engram::new();
        plugin.debug_attach(DebugBus::new());
        let table = plugin.param_table();
        for (k, v) in base.iter().chain(set) {
            table.by_name(k).unwrap().set_plain(*v);
        }
        plugin.activate(FS, BLOCK);
        let mut out = [Vec::with_capacity(n), Vec::with_capacity(n)];
        let (mut il, mut ir, mut ol, mut or) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        for (blk, chunk) in x.chunks(BLOCK).enumerate() {
            if let Some((px, py)) = path(blk * BLOCK) {
                table.by_name("space x").unwrap().set_plain(px);
                table.by_name("space y").unwrap().set_plain(py);
            }
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
            out[0].extend_from_slice(&ol[..chunk.len()]);
            out[1].extend_from_slice(&or[..chunk.len()]);
        }
        out
    };
    let write = |name: &str, o: [Vec<f32>; 2]| {
        let audio = stave_measure::wav::Audio { sample_rate: FS as u32, channels: o.to_vec() };
        stave_measure::wav::write(&dir.join(format!("{name}.wav")), &audio).unwrap();
    };
    write("morph-static", render(&[("drift", 0.0), ("space x", 0.5), ("space y", 0.5)], &|_| None));
    // A loop through the map, after 1 s settled at its start.
    let sweep = move |i: usize| -> Option<(f64, f64)> {
        let t = (i as f64 / f64::from(FS) - 1.0).max(0.0) / f64::from(secs - 1.0);
        let a = std::f64::consts::TAU * t;
        Some((0.5 + 0.38 * a.cos(), 0.5 + 0.38 * (2.0 * a).sin()))
    };
    write("morph-sweep", render(&[("drift", 0.0)], &sweep));
    write("morph-drift", render(&[("drift", 1.0), ("space x", 0.5), ("space y", 0.5)], &|_| None));
    println!("morph renders -> {}", dir.display());
}
