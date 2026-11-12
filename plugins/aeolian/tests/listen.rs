//! The audition bench (ignored by default): every factory preset over
//! a plucked A-minor phrase, written to build/listen/aeolian-<preset>.wav,
//! with the dry and wet-only levels printed (the wet-balance record).
//! Run with
//! `cargo test --release -p stave-aeolian-vst3 --test listen -- --ignored --nocapture`.

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

fn render(plugin: &mut stave_aeolian_vst3::Aeolian, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
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
    let names: Vec<_> = stave_aeolian_vst3::Aeolian::new().presets().into_iter().map(|p| p.name).collect();
    println!("dry rms {:.1} dBFS", 20.0 * dry.log10());
    for (i, name) in names.iter().enumerate() {
        let run = |wet_only: bool| {
            let mut plugin = stave_aeolian_vst3::Aeolian::new();
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
        let file = format!("{dir}/aeolian-{}.wav", name.replace(' ', "_").to_lowercase());
        let audio = stave_measure::wav::Audio { sample_rate: FS as u32, channels: vec![l, r] };
        stave_measure::wav::write(std::path::Path::new(&file), &audio).unwrap();
    }
}
