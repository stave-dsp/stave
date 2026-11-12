//! The audition bench (ignored by default): every factory preset over
//! a plucked A-minor phrase, written to build/listen/revenant-<preset>.wav,
//! with the dry and wet-only levels printed (the wet-balance record).
//! Run with
//! `cargo test --release -p stave-revenant-vst3 --test listen -- --ignored --nocapture`.

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn rms(x: &[f32]) -> f32 {
    stave_measure::level::rms(x) as f32
}

/// Karplus–Strong plucks: A minor, up and back, then ring out.
fn phrase() -> Vec<f32> {
    let mut out = vec![0.0f32; (FS * 10.0) as usize];
    let notes = [110.0f32, 130.81, 164.81, 220.0, 164.81, 130.81, 110.0, 82.41];
    let mut seed = 0x9E37_79B9u32;
    for (k, hz) in notes.iter().enumerate() {
        let start = (k as f32 * 0.5 * FS) as usize;
        let period = (FS / hz) as usize;
        let mut ring: Vec<f32> = (0..period)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as f32 / u32::MAX as f32 * 2.0 - 1.0
            })
            .collect();
        for i in 0..(FS * 2.0) as usize {
            let j = i % period;
            let next = 0.5 * (ring[j] + ring[(j + 1) % period]) * 0.996;
            out[start + i] += ring[j] * 0.35;
            ring[j] = next;
        }
    }
    out
}

fn render(plugin: &mut stave_revenant_vst3::Revenant, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let (mut il, mut ir, mut ol, mut or) = (vec![0.0; BLOCK], vec![0.0; BLOCK], vec![0.0; BLOCK], vec![0.0; BLOCK]);
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
        l.extend_from_slice(&ol[..chunk.len()]);
        r.extend_from_slice(&or[..chunk.len()]);
    }
    (l, r)
}

#[test]
#[ignore = "writes audition WAVs to build/listen and prints levels; asserts nothing"]
fn audition_every_preset() {
    let input = phrase();
    let dry = rms(&input[..(FS * 4.0) as usize]);
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/listen");
    std::fs::create_dir_all(dir).unwrap();
    let names: Vec<_> = stave_revenant_vst3::Revenant::new().presets().into_iter().map(|p| p.name).collect();
    println!("dry rms {:.1} dBFS", 20.0 * dry.log10());
    for (i, name) in names.iter().enumerate() {
        let run = |wet_only: bool| {
            let mut plugin = stave_revenant_vst3::Revenant::new();
            plugin.debug_attach(DebugBus::new());
            let table = plugin.param_table();
            for (id, v) in &plugin.presets()[i].values {
                table.by_id(*id).unwrap().set_plain(*v);
            }
            if wet_only {
                table.by_name("mix").expect("mix").set_plain(1.0);
            }
            plugin.activate(FS, BLOCK);
            render(&mut plugin, &input)
        };
        let (wl, _) = run(true);
        let (l, r) = run(false);
        let wet = rms(&wl[(FS * 1.0) as usize..(FS * 4.0) as usize]);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, x| m.max(x.abs()));
        println!("{name:<14} wet-only {:+5.1} dB re dry   out peak {:5.2}", 20.0 * (wet / dry).log10(), peak);
        let file = format!("{dir}/revenant-{}.wav", name.replace(' ', "_").to_lowercase());
        let audio = stave_measure::wav::Audio { sample_rate: FS as u32, channels: vec![l, r] };
        stave_measure::wav::write(std::path::Path::new(&file), &audio).unwrap();
    }
}

/// How long each preset keeps sounding after the playing stops
/// (ignored; prints wet level at 0.5 s, 2 s, 5 s, 10 s after the end).
#[test]
#[ignore]
fn tails() {
    let mut input = phrase();
    input.truncate((FS * 4.0) as usize);
    input.extend(std::iter::repeat_n(0.0, (FS * 11.0) as usize));
    let names: Vec<_> = stave_revenant_vst3::Revenant::new().presets().into_iter().map(|p| p.name).collect();
    for (i, name) in names.iter().enumerate() {
        let mut plugin = stave_revenant_vst3::Revenant::new();
        plugin.debug_attach(DebugBus::new());
        let table = plugin.param_table();
        for (id, v) in &plugin.presets()[i].values {
            table.by_id(*id).unwrap().set_plain(*v);
        }
        table.by_name("mix").unwrap().set_plain(1.0);
        plugin.activate(FS, BLOCK);
        let (l, _) = render(&mut plugin, &input);
        let at = |s: f32| {
            let a = ((4.0 + s) * FS) as usize;
            20.0 * rms(&l[a..a + 4_800]).max(1e-9).log10()
        };
        println!("{name:<10} after stop: 0.5s {:6.1}  2s {:6.1}  5s {:6.1}  10s {:6.1} dBFS", at(0.5), at(2.0), at(5.0), at(10.0));
    }
}

/// Tails over a realistic input noise floor (−70 dBFS hiss after the
/// phrase), across CHAOS and DREAM (ignored; prints).
#[test]
#[ignore]
fn tails_on_a_noise_floor() {
    let mut input = phrase();
    input.truncate((FS * 4.0) as usize);
    let mut s = 0x2468_ACE1u32;
    input.extend((0..(FS * 11.0) as usize).map(|_| {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        (s as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.000_55
    }));
    for &(chaos, dream) in &[(0.45, 0.0), (0.6, 0.0), (0.8, 0.0), (1.0, 0.0), (0.45, 0.3), (0.45, 0.6), (0.6, 0.6)] {
        let mut plugin = stave_revenant_vst3::Revenant::new();
        plugin.debug_attach(DebugBus::new());
        let table = plugin.param_table();
        for (n, v) in [("mix", 1.0), ("chaos", chaos), ("dream", dream)] {
            table.by_name(n).unwrap().set_plain(v);
        }
        plugin.activate(FS, BLOCK);
        let (l, _) = render(&mut plugin, &input);
        let at = |t: f32| {
            let a = ((4.0 + t) * FS) as usize;
            20.0 * rms(&l[a..a + 4_800]).max(1e-9).log10()
        };
        println!("chaos {chaos} dream {dream}: 0.5s {:6.1}  2s {:6.1}  5s {:6.1}  10s {:6.1} dBFS", at(0.5), at(2.0), at(5.0), at(10.0));
    }
}
