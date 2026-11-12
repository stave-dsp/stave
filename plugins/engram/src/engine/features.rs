//! FEATURE STUDY — which timbre fingerprint lets the memory tell kinds
//! of sound apart? Offline, on the Test Corpus (see bench.rs): every
//! candidate fingerprint is computed per 0.75 s grain, and scored by
//! nearest-neighbour category accuracy against grains of OTHER
//! recordings (grain level) and by recording means (leave-one-out).
//! Run: `cargo test -p stave-engram-vst3 --release --lib features -- --ignored --nocapture`.

extern crate std;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use std::println;

use stave_dsp::Svf;

use super::bench::{load, Rig, Src};

const SR: f32 = 48_000.0;
const FRAME: usize = 256;
const GRAIN_FRAMES: usize = 140;

/// A log-spaced filterbank at the host rate, enveloped as the ear does.
struct Bank {
    f: Vec<Svf>,
    env: Vec<f32>,
    att: f32,
    rel: f32,
    q: f32,
}

impl Bank {
    fn new(n: usize, lo: f32, hi: f32, q: f32) -> Self {
        let res = 1.0 - 1.0 / (2.0 * q);
        let f = (0..n)
            .map(|i| {
                let hz = lo * (hi / lo).powf(i as f32 / (n - 1) as f32);
                let mut s = Svf::new();
                s.prepare(SR);
                s.set(hz, res);
                s
            })
            .collect();
        let c = |ms: f32| 1.0 - (-1000.0 / (ms * SR)).exp();
        Self { f, env: vec![0.0; n], att: c(5.0), rel: c(80.0), q }
    }

    fn tick(&mut self, x: f32) {
        for (f, e) in self.f.iter_mut().zip(&mut self.env) {
            let a = (f.process(x, 0).bp / self.q).abs();
            let k = if a > *e { self.att } else { self.rel };
            *e += (a - *e) * k;
        }
    }
}

fn unit(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-9 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

fn logs_centred(env: &[f32]) -> Vec<f32> {
    let l: Vec<f32> = env.iter().map(|e| (e + 1e-6).log10()).collect();
    let m = l.iter().sum::<f32>() / l.len() as f32;
    l.iter().map(|x| x - m).collect()
}

/// DCT-II coefficients 1..=k of `x` (a cepstrum when `x` is log energy).
fn dct(x: &[f32], k: usize) -> Vec<f32> {
    let n = x.len() as f32;
    (1..=k).map(|j| x.iter().enumerate().map(|(i, v)| v * (core::f32::consts::PI * j as f32 * (i as f32 + 0.5) / n).cos()).sum()).collect()
}

/// Every candidate fingerprint of one grain (frames of band logs).
fn candidates(cur: &[Vec<f32>], b16: &[Vec<f32>], b24: &[Vec<f32>], lvl: &[f32]) -> Vec<Vec<f32>> {
    let mean = |fr: &[Vec<f32>]| -> Vec<f32> {
        let n = fr[0].len();
        (0..n).map(|i| fr.iter().map(|f| f[i]).sum::<f32>() / fr.len() as f32).collect()
    };
    let std = |fr: &[Vec<f32>], m: &[f32]| -> Vec<f32> {
        (0..m.len()).map(|i| (fr.iter().map(|f| (f[i] - m[i]).powi(2)).sum::<f32>() / fr.len() as f32).sqrt()).collect()
    };
    // A: the ear's current fingerprint (8 bands ≤ 4.8 kHz), mean unit.
    let mut a = mean(cur);
    unit(&mut a);
    // B: 16 bands 50 Hz … 16 kHz, centred logs, grain mean.
    let l16: Vec<Vec<f32>> = b16.iter().map(|e| logs_centred(e)).collect();
    let m16 = mean(&l16);
    let mut b = m16.clone();
    unit(&mut b);
    // C: B's cepstrum (DCT 1..10).
    let mut c = dct(&m16, 10);
    unit(&mut c);
    // D: B + its modulation (each band's spread over the grain): the
    // spectrum AND how it moves.
    let raw16: Vec<Vec<f32>> = b16.iter().map(|e| e.iter().map(|v| (v + 1e-6).log10()).collect()).collect();
    let rm = mean(&raw16);
    let sd = std(&raw16, &rm);
    let mut d: Vec<f32> = m16.iter().copied().chain(sd.iter().map(|v| v * 1.0)).collect();
    unit(&mut d);
    // E: cepstrum + modulation cepstrum + envelope crest.
    let lm: f32 = lvl.iter().sum::<f32>() / lvl.len() as f32;
    let crest = (lvl.iter().cloned().fold(0.0, f32::max) / lm.max(1e-9)).ln();
    let mut e: Vec<f32> = dct(&m16, 10).into_iter().chain(dct(&sd, 4).into_iter().map(|v| v * 0.7)).chain(sd.iter().take(0).copied()).collect();
    e.push(sd.iter().sum::<f32>() / sd.len() as f32 * 2.0);
    e.push(crest * 0.5);
    unit(&mut e);
    // F: 24 bands, centred logs mean + spread.
    let l24: Vec<Vec<f32>> = b24.iter().map(|e| logs_centred(e)).collect();
    let m24 = mean(&l24);
    let raw24: Vec<Vec<f32>> = b24.iter().map(|e| e.iter().map(|v| (v + 1e-6).log10()).collect()).collect();
    let rm24 = mean(&raw24);
    let sd24 = std(&raw24, &rm24);
    let mut f: Vec<f32> = m24.iter().copied().chain(sd24.iter().copied()).collect();
    unit(&mut f);
    // G/H: D with the modulation weighted 0.5 / 2.
    let wmix = |w: f32| -> Vec<f32> {
        let mut v: Vec<f32> = m16.iter().copied().chain(sd.iter().map(|x| x * w)).collect();
        unit(&mut v);
        v
    };
    let g = wmix(0.5);
    let h = wmix(2.0);
    // I: modulation alone.
    let mut i = sd.clone();
    unit(&mut i);
    // J: D + spectral flux (mean |Δ| of band logs frame to frame).
    let flux: Vec<f32> = (0..16).map(|k| raw16.windows(2).map(|w| (w[1][k] - w[0][k]).abs()).sum::<f32>() / (raw16.len().max(2) - 1) as f32).collect();
    let mut j: Vec<f32> = m16.iter().copied().chain(sd.iter().copied()).chain(flux.iter().map(|v| v * 4.0)).collect();
    unit(&mut j);
    // K: D at 12 bands (cheaper).
    let pick = |v: &[f32]| -> Vec<f32> { (0..12).map(|q| v[q * 15 / 11]).collect() };
    let m12 = pick(&m16);
    let mc = m12.iter().sum::<f32>() / 12.0;
    let mut kk: Vec<f32> = m12.iter().map(|v| v - mc).chain(pick(&sd)).collect();
    unit(&mut kk);
    let p = envelope(lvl, &m16);
    let mix = |blocks: &[(&[f32], f32)]| -> Vec<f32> {
        let mut v = Vec::new();
        for (b, w) in blocks {
            let mut u = b.to_vec();
            unit(&mut u);
            v.extend(u.iter().map(|x| x * w));
        }
        unit(&mut v);
        v
    };
    let gp05 = mix(&[(&g, 1.0), (&p, 0.5)]);
    let gp1 = mix(&[(&g, 1.0), (&p, 1.0)]);
    let gp2 = mix(&[(&g, 1.0), (&p, 1.6)]);
    vec![a, b, c, d, e, f, g, h, i, j, kk, p, gp05, gp1, gp2]
}

/// PITCH-INVARIANT envelope fingerprint of a grain: how its level moves
/// (log-level slope, modulation spectrum at 1–16 Hz, attack sharpness)
/// and the spectrum's overall tilt.
fn envelope(lvl: &[f32], m16: &[f32]) -> Vec<f32> {
    let fr = SR / FRAME as f32;
    let e: Vec<f32> = lvl.iter().map(|v| (v + 1e-5).ln()).collect();
    let n = e.len() as f32;
    let tm = (n - 1.0) / 2.0;
    let em = e.iter().sum::<f32>() / n;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (i, v) in e.iter().enumerate() {
        let t = i as f32 - tm;
        sxy += t * (v - em);
        sxx += t * t;
    }
    let slope = sxy / sxx.max(1e-9) * fr; // nepers per second
    let detr: Vec<f32> = e.iter().enumerate().map(|(i, v)| v - em - sxy / sxx.max(1e-9) * (i as f32 - tm)).collect();
    let mods: Vec<f32> = [1.5f32, 3.0, 5.0, 8.0, 13.0]
        .iter()
        .map(|hz| {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, v) in detr.iter().enumerate() {
                let a = core::f32::consts::TAU * hz * i as f32 / fr;
                re += v * a.cos();
                im += v * a.sin();
            }
            re.hypot(im) / n
        })
        .collect();
    let attack = e.windows(2).map(|w| w[1] - w[0]).fold(0.0f32, f32::max);
    let tilt = {
        let k = m16.len() as f32;
        let km = (k - 1.0) / 2.0;
        let mm = m16.iter().sum::<f32>() / k;
        let (mut a, mut b) = (0.0, 0.0);
        for (i, v) in m16.iter().enumerate() {
            a += (i as f32 - km) * (v - mm);
            b += (i as f32 - km).powi(2);
        }
        a / b
    };
    let mut v = alloc::vec![(slope / 8.0).tanh(), attack.min(3.0) / 3.0, tilt * 4.0];
    v.extend(mods.iter().map(|m| m * 3.0));
    v
}

const NAMES: [&str; 17] = ["A ear 8b≤4.8k", "B 16b≤16k", "C cepstrum16", "D 16b+modulation", "E cep+mod+crest", "F 24b+modulation", "G D mod×0.5", "H D mod×2", "I mod only", "J D+flux", "K 12b+mod", "P envelope", "GP 0.5", "GP 1.0", "GP 1.6", "L G centred", "M G whitened"];

struct Grain {
    file: usize,
    feats: Vec<Vec<f32>>,
}

fn grains_of(i: usize, src: &Src) -> Vec<Grain> {
    let mut rig = Rig::new(0.6, 0.3);
    let mut b16 = Bank::new(16, 50.0, 16_000.0, 1.4);
    let mut b24 = Bank::new(24, 40.0, 18_000.0, 2.0);
    let x = &src.audio[..src.audio.len().min((12.0 * SR) as usize)];
    let (mut cur, mut f16, mut f24, mut lvl) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut out = Vec::new();
    let mut fast = 0.0f32;
    for (n, &v) in x.iter().enumerate() {
        rig.step(v, false);
        b16.tick(v);
        b24.tick(v);
        let a = v.abs();
        fast += (a - fast) * if a > fast { 0.01 } else { 0.0014 };
        if n % FRAME == FRAME - 1 {
            if rig.ear.present() {
                cur.push(rig.h.timbre.to_vec());
                f16.push(b16.env.clone());
                f24.push(b24.env.clone());
                lvl.push(fast);
            }
            if (n / FRAME + 1).is_multiple_of(GRAIN_FRAMES) {
                if cur.len() > GRAIN_FRAMES * 3 / 4 {
                    out.push(Grain { file: i, feats: candidates(&cur, &f16, &f24, &lvl) });
                }
                cur.clear();
                f16.clear();
                f24.clear();
                lvl.clear();
            }
        }
    }
    out
}

fn cos(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
#[ignore]
fn feature_study() {
    let Some(corpus) = load() else {
        println!("no corpus; skipped");
        return;
    };
    let mut grains: Vec<Grain> = corpus.iter().enumerate().flat_map(|(i, s)| grains_of(i, s)).collect();
    // L: G centred on the corpus mean (the embedding space centred), and
    // M: G centred + per-dimension standardised (whitened diagonally).
    let gk = 6; // G
    let dim = grains[0].feats[gk].len();
    let n = grains.len() as f32;
    let mean: Vec<f32> = (0..dim).map(|d| grains.iter().map(|g| g.feats[gk][d]).sum::<f32>() / n).collect();
    let sd: Vec<f32> = (0..dim).map(|d| (grains.iter().map(|g| (g.feats[gk][d] - mean[d]).powi(2)).sum::<f32>() / n).sqrt().max(1e-4)).collect();
    for g in &mut grains {
        let mut l: Vec<f32> = (0..dim).map(|d| g.feats[gk][d] - mean[d]).collect();
        unit(&mut l);
        let mut m: Vec<f32> = (0..dim).map(|d| (g.feats[gk][d] - mean[d]) / sd[d]).collect();
        unit(&mut m);
        g.feats.push(l);
        g.feats.push(m);
    }
    std::println!("corpus mean of G: {:?}", mean.iter().map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>());
    // The same recording filed twice (one loop in two categories) is not
    // "another recording".
    let same = |a: usize, b: usize| a == b || corpus[a].name == corpus[b].name;
    println!("{} grains from {} recordings", grains.len(), corpus.len());
    for (k, name) in NAMES.iter().enumerate() {
        // Grain level: nearest grain of another recording.
        let (mut hit, mut n) = (0, 0);
        let mut per_cat: Vec<(String, usize, usize)> = Vec::new();
        for g in &grains {
            let best = grains.iter().filter(|o| !same(o.file, g.file)).max_by(|a, b| cos(&g.feats[k], &a.feats[k]).total_cmp(&cos(&g.feats[k], &b.feats[k])));
            if let Some(b) = best {
                let ok = corpus[b.file].cat == corpus[g.file].cat;
                hit += ok as usize;
                n += 1;
                let c = &corpus[g.file].cat;
                match per_cat.iter_mut().find(|(x, _, _)| x == c) {
                    Some(e) => {
                        e.1 += ok as usize;
                        e.2 += 1;
                    }
                    None => per_cat.push((c.clone(), ok as usize, 1)),
                }
            }
        }
        let cats: String = per_cat.iter().map(|(c, h, n)| alloc::format!("{}:{:.0}% ", &c[..c.len().min(6)], 100.0 * *h as f32 / *n as f32)).collect();
        let fam = |c: &str| matches!(c, "guitar" | "piano notes" | "synth notes" | "tonal loops" | "vocals" | "vocal phrases");
        let (ph, pn) = per_cat.iter().filter(|(c, _, _)| fam(c)).fold((0, 0), |(a, b), (_, h, n)| (a + h, b + n));
        let pitched = 100.0 * ph as f32 / pn.max(1) as f32;
        let macro_acc = per_cat.iter().map(|(_, h, n)| *h as f32 / *n as f32).sum::<f32>() / per_cat.len().max(1) as f32;
        println!("{name:<20} grain-NN {:.1}%  balanced {:.1}%  pitched {pitched:.1}%   {cats}", 100.0 * hit as f32 / n.max(1) as f32, 100.0 * macro_acc);
    }
}
