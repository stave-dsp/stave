//! `Bank` — the associative memory: what makes this reverb remember.
//!
//! WRITING. Your playing is captured continuously; every grain (¾ s)
//! of it you actually played, and that held still (one harmony for a
//! tonal grain, one sound for a percussive one — a grain straddling a
//! change would key one thing to the audio of another), becomes a
//! memory: the audio, KEYED by three fingerprints (listen.rs) —
//! harmony, timbre, rhythm — plus how tonal it was and where its hits
//! fall. A near-repeat keeps the ORIGINAL (only strengthening it: the
//! room remembers the first time); otherwise the moment takes an empty
//! slot or the one least recalled lately (forgetting is by disuse).
//!
//! RECALLING — attention, as a transformer does it. What you play now
//! is the query; every memory old enough (DÉJÀ) is scored by a
//! weighted similarity — MATCH turns it from harmony toward timbre and
//! groove, and harmony only counts as much as the QUERY is tonal (the
//! chroma of a drum means nothing) — and a softmax at temperature FOCUS
//! makes the weights. When you stop, the last query holds.
//!
//! PLAYBACK — every memory has its own granular VOICE (its own timing,
//! so recalled memories never pulse in lockstep). TEXTURE runs from
//! seamless (long overlapping grains walking through the memory — a
//! continuation of the moment) through scattered (shorter grains at
//! random places — a cloud) to shattered (short grains, reversed and
//! leaping octaves). Percussive memories ANSWER: on each of your hits
//! they fire one of their own remembered hits, sharp-attacked — and
//! their free-running stream comes in only as TEXTURE rises (a drum
//! loop out of time with you is noise; a cloud of remembered hits at
//! the top of TEXTURE is the point).
//!
//! All buffers are allocated in `prepare`; the audio path only copies
//! into them and reads.

extern crate alloc;
#[cfg(test)]
extern crate std;
use alloc::vec;
use alloc::vec::Vec;

use core::f32::consts::PI;

use stave_dsp::Xorshift32;

use super::space::Latent;
use super::space_model::LATENT;
use super::listen::{rhythm_of, timbre_key, unit, Hearing, BANDS, CLASSES, RHYTHM, STEPS, TIMBRE};

pub(super) const SLOTS: usize = 64;
pub(super) const GRAIN_S: f32 = 0.75;
/// A new moment this alike to a stored one only strengthens it.
const SIMILAR: f32 = 0.97;
/// A grain becomes a memory only if you were playing for this share of it…
const PLAYED_SHARE: f32 = 0.85;
/// …and, if tonal, it held still: its two halves' harmonies alike at
/// least this much (a grain straddling a chord change would key one
/// harmony to the other's audio). A PERCUSSIVE grain is not gated: its
/// key describes exactly the hits it holds, and a real groove's halves
/// differ by nature (a kick in one, a snare in the other — the corpus
/// bench measured real loops' half-agreement from −0.2 to 0.9, as low
/// as a kicks→hats change's), so a gate there only refused whole loops.
const COHERENCE_TONAL: f32 = 0.9;
/// Below this tonality a sound is flat noise (and PERCUSSIVE).
pub(super) const TONAL: f32 = 0.15;
/// A memory made of HITS is percussive too, pitched or not (a kick is a
/// pitched thump): an onset, and a level envelope this spiky (peak over
/// mean) — and the more tonal, the spikier it must be. Measured on the
/// corpus: drum hits 4.5–15, sung vocals / piano / bells 1.5–2.7 (at
/// tonality 0.75–1), choppy guitar 2.6–7. (Two hits and a flat 1.5 made
/// sung phrases "drums" — silent at low TEXTURE — and a sparse kick,
/// one per grain, not.)
const MIN_HITS: usize = 1;
const SPIKY: f32 = 2.0;
const SPIKY_PER_TONAL: f32 = 2.5;
const SPIKY_FROM_TONAL: f32 = 0.6;
const BETA: (f32, f32) = (2.0, 60.0);
/// The ATTENTION SINK: a null memory at this similarity, in the softmax
/// with the real ones. Attention must go somewhere — without a sink,
/// playing something the room has never heard still recalls the least
/// unlike memory at full weight (the corpus renders: house drums
/// recalled under a piano). With it, recall fades when nothing reminds
/// the room of now. Placed from the corpus bench (MATCH 0.3): the best
/// memory's similarity is ~0.91 for a sound it learned, ~0.81 for an
/// unseen sound of a kind it knows, ~0.73 for a kind it never heard.
/// It shares FOCUS's temperature: soft at low focus, a gate at high.
pub(super) const SINK: f32 = 0.78;
const GLIDE: f32 = 0.12;
const MAX_ONSETS: usize = 4;
const HEADS: usize = 4;
/// TEXTURE 0 … 1: grain length, seconds.
const GRAIN_LEN_S: (f32, f32) = (0.35, 0.04);
/// An answer: a remembered hit, this long at most, this sharp.
const ANSWER_S: f32 = 0.45;
const ANSWER_ATTACK_S: f32 = 0.002;
const ANSWER_GAIN: f32 = 0.8;

/// A memory's (or the query's) fingerprints.
#[derive(Copy, Clone, Default)]
pub(super) struct Key {
    pub(super) chroma: [f32; CLASSES],
    pub(super) timbre: [f32; TIMBRE],
    pub(super) rhythm: [f32; RHYTHM],
    pub(super) tonal: f32,
    /// Made of hits (or flat noise): answers your hits, streams only
    /// with TEXTURE.
    pub(super) percussive: bool,
}

#[inline]
fn dot<const N: usize>(a: &[f32; N], b: &[f32; N]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Similarity of two grooves: both still = alike; one still = not.
#[inline]
fn groove_sim(a: &[f32; RHYTHM], b: &[f32; RHYTHM]) -> f32 {
    let (za, zb) = (a.iter().all(|v| *v == 0.0), b.iter().all(|v| *v == 0.0));
    match (za, zb) {
        (true, true) => 1.0,
        (true, false) | (false, true) => 0.0,
        _ => dot(a, b),
    }
}

/// How alike memory `m` is to query `q`: MATCH 0 listens for harmony,
/// 1 for timbre and groove — and harmony only counts as much as the
/// query is tonal.
pub(super) fn similarity(q: &Key, m: &Key, matching: f32) -> f32 {
    similarity_with(q, m, &q.timbre, &m.timbre, matching)
}

/// `similarity`, with the timbres given separately (attention passes
/// them CENTRED on the bank's own memories).
fn similarity_with(q: &Key, m: &Key, qt: &[f32; TIMBRE], mt: &[f32; TIMBRE], matching: f32) -> f32 {
    let h = (1.0 - matching) * q.tonal.max(0.05);
    let t = 0.15 + 0.85 * matching;
    (h * dot(&q.chroma, &m.chroma) + t * dot(qt, mt) + t * groove_sim(&q.rhythm, &m.rhythm)) / (h + 2.0 * t)
}

/// The timbre CENTRED: `v` minus the bank's mean timbre, unit. Every
/// sound shares the spectrum's usual tilt, so raw timbres are all
/// rather alike; measured from the memories' own centre, attention
/// tells apart what you actually played (the corpus study: +3 points
/// of category accuracy on top of the fingerprint).
fn centred(v: &[f32; TIMBRE], centre: &[f32; TIMBRE]) -> [f32; TIMBRE] {
    unit(&core::array::from_fn(|i| v[i] - centre[i]))
}

/// The centre is shrunk toward zero while the bank holds few memories
/// (one memory minus itself would be nothing): n / (n + this).
const CENTRE_SHRINK: f32 = 4.0;

#[derive(Copy, Clone, Default)]
struct Head {
    pos: f32,
    rate: f32,
    len: f32,
    t: f32,
    live: bool,
    /// A sharp-attacked answer (else a sin² grain).
    punch: bool,
    gain: f32,
}

struct Voice {
    heads: [Head; HEADS],
    /// Where the voice walks through its memory (seamless playback).
    cursor: f32,
    next: f32,
    active: bool,
    rng: Xorshift32,
}

struct Slot {
    audio: Vec<f32>,
    key: Key,
    onsets: [usize; MAX_ONSETS],
    n_onsets: usize,
    born: u64,
    filled: bool,
    used: f32,
    w: f32,
    target: f32,
    /// The timbre centred on the bank (refreshed each attend).
    tc: [f32; TIMBRE],
    /// The ROOM it was heard in (the generative space's latent), and
    /// where that is on the map (for the face).
    space: Latent,
    at: [f32; 2],
    voice: Voice,
}

/// What became of a grain when it ended (`capture`'s report; the
/// engine ignores it, the corpus bench reads it).
#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) enum Captured {
    /// Mid-grain: nothing decided yet.
    Nothing,
    /// Not writing (HOLD), or you were not playing for enough of it.
    Unplayed,
    /// It did not hold still: coherence below the bar.
    Incoherent { coherence: f32, percussive: bool },
    /// A near-repeat: the original memory in this slot was strengthened.
    Repeat { slot: usize },
    /// A new memory, in this slot.
    Stored { slot: usize },
}

/// Half a grain's running sums: chroma, and band log levels (sum and
/// sum of squares — a timbre).
#[derive(Default, Clone, Copy)]
struct Half {
    chroma: [f32; CLASSES],
    logs: [f32; BANDS],
    logs_sq: [f32; BANDS],
    n: f32,
}

impl Half {
    fn add(&self, o: &Half) -> Half {
        Half {
            chroma: core::array::from_fn(|i| self.chroma[i] + o.chroma[i]),
            logs: core::array::from_fn(|i| self.logs[i] + o.logs[i]),
            logs_sq: core::array::from_fn(|i| self.logs_sq[i] + o.logs_sq[i]),
            n: self.n + o.n,
        }
    }

    fn timbre(&self) -> [f32; TIMBRE] {
        let n = self.n.max(1.0);
        let mean: [f32; BANDS] = core::array::from_fn(|b| self.logs[b] / n);
        let spread: [f32; BANDS] = core::array::from_fn(|b| (self.logs_sq[b] / n - mean[b] * mean[b]).max(0.0).sqrt());
        timbre_key(&mean, &spread)
    }
}

/// The grain being captured: running sums for its fingerprints.
#[derive(Default)]
struct Capture {
    /// Each half of the grain's fingerprints (the whole is their sum;
    /// their agreement is its coherence).
    halves: [Half; 2],
    tonal: f32,
    played: usize,
    steps: [f32; STEPS],
    onsets: [usize; MAX_ONSETS],
    n_onsets: usize,
    hits: usize,
    level_sum: f32,
    level_max: f32,
}

pub(super) struct Bank {
    sr: f32,
    slots: Vec<Slot>,
    grain: usize,
    audio: Vec<f32>,
    at: usize,
    cap: Capture,
    clock: u64,
    query: Key,
    novelty: f32,
    /// The room playing now: a new memory is filed with it.
    here: Latent,
    here_at: [f32; 2],
}

impl Bank {
    pub(super) fn new() -> Self {
        Self {
            sr: 48_000.0,
            slots: Vec::new(),
            grain: 1,
            audio: Vec::new(),
            at: 0,
            cap: Capture::default(),
            clock: 0,
            query: Key::default(),
            novelty: 0.0,
            here: [0.0; LATENT],
            here_at: [0.0; 2],
        }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.grain = ((GRAIN_S * sr) as usize).max(64);
        self.slots = (0..SLOTS as u32)
            .map(|i| Slot {
                audio: vec![0.0; self.grain],
                key: Key::default(),
                onsets: [0; MAX_ONSETS],
                n_onsets: 0,
                born: 0,
                filled: false,
                used: 0.0,
                w: 0.0,
                target: 0.0,
                tc: [0.0; TIMBRE],
                space: [0.0; LATENT],
                at: [0.0; 2],
                voice: Voice { heads: [Head::default(); HEADS], cursor: 0.0, next: 0.0, active: false, rng: Xorshift32::new(0x9E37_79B9 ^ i.wrapping_mul(0x85EB_CA6B)) },
            })
            .collect();
        self.audio = vec![0.0; self.grain];
        self.forget();
    }

    pub(super) fn forget(&mut self) {
        for s in &mut self.slots {
            s.filled = false;
            s.used = 0.0;
            s.w = 0.0;
            s.target = 0.0;
            s.voice.active = false;
            for h in &mut s.voice.heads {
                h.live = false;
            }
        }
        self.at = 0;
        self.cap = Capture::default();
        self.clock = 0;
        self.query = Key::default();
        self.novelty = 0.0;
    }

    /// Hear one host frame of your playing (mono) with the ear's state;
    /// at each grain's end, maybe remember it.
    #[inline]
    pub(super) fn capture(&mut self, x: f32, hearing: &Hearing, present: bool, onset: bool, writing: bool) -> Captured {
        self.clock += 1;
        self.audio[self.at] = x;
        if present {
            let c = &mut self.cap;
            c.played += 1;
            let h = &mut c.halves[(2 * self.at >= self.grain) as usize];
            for (a, v) in h.chroma.iter_mut().zip(&hearing.chroma) {
                *a += v;
            }
            for b in 0..BANDS {
                let l = hearing.logs[b];
                h.logs[b] += l;
                h.logs_sq[b] += l * l;
            }
            h.n += 1.0;
            c.tonal += hearing.tonal;
        }
        self.cap.level_sum += hearing.level;
        self.cap.level_max = self.cap.level_max.max(hearing.level);
        if onset {
            let c = &mut self.cap;
            c.hits += 1;
            c.steps[(self.at * STEPS / self.grain).min(STEPS - 1)] += 1.0;
            if c.n_onsets < MAX_ONSETS {
                c.onsets[c.n_onsets] = self.at;
                c.n_onsets += 1;
            }
        }
        self.at += 1;
        if self.at < self.grain {
            return Captured::Nothing;
        }
        self.at = 0;
        let c = core::mem::take(&mut self.cap);
        if !writing || (c.played as f32) < PLAYED_SHARE * self.grain as f32 {
            return Captured::Unplayed;
        }
        let tonal = c.tonal / c.played as f32;
        let crest = c.level_max / (c.level_sum / self.grain as f32).max(1e-9);
        let spiky = SPIKY + SPIKY_PER_TONAL * (tonal - SPIKY_FROM_TONAL).max(0.0);
        let percussive = tonal < TONAL || (c.hits >= MIN_HITS && crest > spiky);
        let [a, b] = c.halves;
        let harmony_coh = dot(&unit(&a.chroma), &unit(&b.chroma));
        #[cfg(test)]
        if std::env::var_os("ENGRAM_TRACE").is_some() {
            let timbre_coh = dot(&a.timbre(), &b.timbre());
            std::println!("grain: tonal {tonal:.2} hits {} crest {crest:.2} perc {percussive} harmony-coh {harmony_coh:.2} timbre-coh {timbre_coh:.2}", c.hits);
        }
        if !percussive && harmony_coh < COHERENCE_TONAL {
            return Captured::Incoherent { coherence: harmony_coh, percussive };
        }
        let whole = a.add(&b);
        let key = Key { chroma: unit(&whole.chroma), timbre: whole.timbre(), rhythm: rhythm_of(&c.steps), tonal, percussive };
        self.remember(key, c.onsets, c.n_onsets)
    }

    fn remember(&mut self, key: Key, onsets: [usize; MAX_ONSETS], n_onsets: usize) -> Captured {
        let (mut best, mut best_s) = (usize::MAX, -1.0f32);
        for (i, s) in self.slots.iter().enumerate() {
            if s.filled {
                let c = similarity(&key, &s.key, 0.5);
                if c > best_s {
                    best_s = c;
                    best = i;
                }
            }
        }
        if best != usize::MAX && best_s > SIMILAR {
            self.slots[best].used += 0.5;
            return Captured::Repeat { slot: best };
        }
        let slot = if let Some(i) = self.slots.iter().position(|s| !s.filled) {
            i
        } else {
            let mut k = 0;
            for i in 1..SLOTS {
                let (a, b) = (&self.slots[i], &self.slots[k]);
                if a.used < b.used || (a.used == b.used && a.born < b.born) {
                    k = i;
                }
            }
            k
        };
        let s = &mut self.slots[slot];
        s.key = key;
        s.audio.copy_from_slice(&self.audio);
        s.onsets = onsets;
        s.n_onsets = n_onsets;
        s.born = self.clock;
        s.space = self.here;
        s.at = self.here_at;
        s.filled = true;
        s.used = 0.5;
        s.voice.active = false;
        Captured::Stored { slot }
    }

    /// Once per block: score every eligible memory against the query
    /// (what you play now; the last of it once you stop) and set the
    /// attention targets.
    pub(super) fn attend(&mut self, now: Key, present: bool, focus: f32, matching: f32, min_age: u64) {
        if present {
            self.query = now;
        }
        let beta = BETA.0 + (BETA.1 - BETA.0) * focus * focus;
        // The bank's centre, and every timbre measured from it.
        let mut centre = [0.0f32; TIMBRE];
        let mut n = 0.0f32;
        for s in self.slots.iter().filter(|s| s.filled) {
            for (c, v) in centre.iter_mut().zip(&s.key.timbre) {
                *c += v;
            }
            n += 1.0;
        }
        let k = if n > 0.0 { 1.0 / (n + CENTRE_SHRINK) } else { 0.0 };
        centre.iter_mut().for_each(|c| *c *= k);
        let qt = centred(&self.query.timbre, &centre);
        for s in self.slots.iter_mut().filter(|s| s.filled) {
            s.tc = centred(&s.key.timbre, &centre);
        }
        let (mut best, mut best_any) = (f32::MIN, 0.0f32);
        for s in &self.slots {
            if s.filled {
                let c = similarity_with(&self.query, &s.key, &qt, &s.tc, matching);
                best_any = best_any.max(c);
                if self.clock.saturating_sub(s.born) >= min_age {
                    best = best.max(c);
                }
            }
        }
        if present {
            self.novelty = (1.0 - best_any).clamp(0.0, 1.0);
        }
        // The sink joins the softmax (relative to the best, as all do).
        let mut sum = if best > f32::MIN { (beta * (SINK - best)).exp() } else { 0.0 };
        for s in &mut self.slots {
            let eligible = s.filled && self.clock.saturating_sub(s.born) >= min_age && best > f32::MIN;
            s.target = if eligible { (beta * (similarity_with(&self.query, &s.key, &qt, &s.tc, matching) - best)).exp() } else { 0.0 };
            sum += s.target;
        }
        for s in &mut self.slots {
            if sum > 0.0 {
                s.target /= sum;
            }
            s.w += (s.target - s.w) * GLIDE;
            if s.w < 1e-5 && s.target == 0.0 {
                s.w = 0.0;
            }
            s.used = s.used * 0.9995 + s.target * 0.01;
        }
    }

    /// One host frame of what the memory says. `onset`: you just hit
    /// something (percussive memories answer); `texture` 0 … 1.
    #[inline]
    pub(super) fn recall(&mut self, onset: bool, texture: f32) -> f32 {
        let g = self.grain as f32;
        let len = (GRAIN_LEN_S.0 * (GRAIN_LEN_S.1 / GRAIN_LEN_S.0).powf(texture) * self.sr).min(g * 0.9);
        let answer_len = (ANSWER_S * self.sr).min(g);
        let attack = ANSWER_ATTACK_S * self.sr;
        let mut y = 0.0;
        for s in &mut self.slots {
            let v = &mut s.voice;
            if s.w <= 0.0 {
                v.active = false;
                for h in &mut v.heads {
                    h.live = false;
                }
                continue;
            }
            if !v.active {
                // Its own timing: start somewhere in the memory, at an
                // arbitrary point of the overlap cycle.
                v.active = true;
                v.cursor = v.rng.next_unit() * g;
                v.next = 0.0;
            }
            // Answer your hit with one of its own.
            if onset && s.key.percussive && s.n_onsets > 0 && s.w > 0.02 {
                let pick = s.onsets[(v.rng.next_u32() as usize) % s.n_onsets] as f32;
                spawn(v, Head { pos: pick, rate: 1.0, len: answer_len, t: 0.0, live: true, punch: true, gain: ANSWER_GAIN });
            }
            // The grain stream.
            v.next -= 1.0;
            if v.next <= 0.0 {
                v.next += len * 0.5;
                let jump = v.rng.next_unit() < texture;
                let start = if jump { v.rng.next_unit() * g } else { v.cursor };
                let reverse = v.rng.next_unit() < ((texture - 0.45) / 0.55).clamp(0.0, 1.0) * 0.5;
                let leap = v.rng.next_unit() < ((texture - 0.7) / 0.3).clamp(0.0, 1.0) * 0.4;
                let rate = if leap { if v.rng.next_unit() < 0.5 { 2.0 } else { 0.5 } } else { 1.0 };
                let rate = if reverse { -rate } else { rate };
                let stream = if s.key.percussive { texture } else { 1.0 };
                spawn(v, Head { pos: start, rate, len, t: 0.0, live: true, punch: false, gain: stream });
            }
            v.cursor = (v.cursor + 1.0) % g;
            // Sound every live head.
            let mut out = 0.0;
            for h in &mut v.heads {
                if !h.live {
                    continue;
                }
                let x = h.t / h.len;
                let win = if h.punch {
                    if h.t < attack { h.t / attack } else { let d = (h.t - attack) / (h.len - attack); (0.5 + 0.5 * (PI * d).cos()).powi(2) }
                } else {
                    let s = (PI * x).sin();
                    s * s
                };
                out += h.gain * win * read_at(&s.audio, h.pos);
                h.pos += h.rate;
                h.t += 1.0;
                if h.t >= h.len {
                    h.live = false;
                }
            }
            y += s.w * out;
        }
        y
    }

    /// The room playing now (a memory written from here remembers it).
    pub(super) fn set_here(&mut self, z: &Latent, at: [f32; 2]) {
        self.here = *z;
        self.here_at = at;
    }

    /// Where on the map memory `i` was heard.
    pub(super) fn heard_at(&self, i: usize) -> [f32; 2] {
        self.slots[i].at
    }

    /// The REMEMBERED ROOM: the attended memories' rooms, blended by
    /// their (glided) weights, and the attention's mass on memories
    /// (1 − the sink's). `None` while nothing is attended.
    pub(super) fn remembered_space(&self) -> Option<(Latent, f32)> {
        let mut z = [0.0f32; LATENT];
        let mut mass = 0.0f32;
        for s in self.slots.iter().filter(|s| s.filled && s.w > 0.0) {
            mass += s.w;
            for (zk, sk) in z.iter_mut().zip(&s.space) {
                *zk += s.w * sk;
            }
        }
        if mass < 1e-4 {
            return None;
        }
        z.iter_mut().for_each(|v| *v /= mass);
        Some((z, mass.min(1.0)))
    }

    pub(super) fn novelty(&self) -> f32 {
        self.novelty
    }

    pub(super) fn weight(&self, i: usize) -> f32 {
        self.slots[i].w
    }

    pub(super) fn filled(&self, i: usize) -> bool {
        self.slots[i].filled
    }

    pub(super) fn percussive(&self, i: usize) -> bool {
        self.slots[i].filled && self.slots[i].key.percussive
    }

    pub(super) fn query(&self) -> &Key {
        &self.query
    }

    /// Attention's target weight on slot `i` (before the glide).
    #[cfg(test)]
    pub(super) fn target(&self, i: usize) -> f32 {
        self.slots[i].target
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(super) fn key(&self, i: usize) -> &Key {
        &self.slots[i].key
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(super) fn debug_dump(&self) {
        for (i, s) in self.slots.iter().enumerate() {
            if s.filled {
                std::println!(
                    "slot {i}: born {:.2}s tonal {:.2} perc {} onsets {} target {:.3} w {:.3}",
                    s.born as f32 / self.sr,
                    s.key.tonal,
                    s.key.percussive,
                    s.n_onsets,
                    s.target,
                    s.w
                );
            }
        }
    }
}

/// The memory's audio at fractional position `pos` (wrapped), linearly
/// interpolated. `rem_euclid` of a tiny NEGATIVE position (a reversed
/// head just past 0) rounds to exactly the length — so the index is
/// clamped, or it reads one past the end (the corpus renders found it).
#[inline]
fn read_at(audio: &[f32], pos: f32) -> f32 {
    let n = audio.len();
    let p = pos.rem_euclid(n as f32);
    let i = (p as usize).min(n - 1);
    let f = (p - i as f32).clamp(0.0, 1.0);
    let a = audio[i];
    let b = audio[(i + 1) % n];
    a + (b - a) * f
}

/// Start a head in the first free place (or over the oldest).
fn spawn(v: &mut Voice, h: Head) {
    let k = v.heads.iter().position(|x| !x.live).unwrap_or_else(|| {
        let mut k = 0;
        for i in 1..HEADS {
            if v.heads[i].t > v.heads[k].t {
                k = i;
            }
        }
        k
    });
    v.heads[k] = h;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tonal(c: &[usize]) -> Hearing {
        let mut chroma = [0.0; CLASSES];
        for &k in c {
            chroma[k] = 1.0;
        }
        let logs: [f32; BANDS] = core::array::from_fn(|b| -1.0 - 0.2 * b as f32);
        Hearing { chroma: unit(&chroma), timbre: timbre_key(&logs, &[0.0; BANDS]), logs, tonal: 0.9, level: 0.1 }
    }

    fn query(h: &Hearing) -> Key {
        Key { chroma: h.chroma, timbre: h.timbre, rhythm: [0.0; RHYTHM], tonal: h.tonal, percussive: false }
    }

    #[test]
    fn attention_finds_the_matching_memory_and_focus_sharpens_it() {
        let mut b = Bank::new();
        b.prepare(1_000.0);
        let (a, other) = (tonal(&[0, 4, 7]), tonal(&[6]));
        for _ in 0..b.grain {
            b.capture(1.0, &a, true, false, true);
        }
        for _ in 0..b.grain {
            b.capture(-1.0, &other, true, false, true);
        }
        let weights = |b: &Bank| (b.slots[0].target, b.slots[1].target);
        b.attend(query(&a), true, 1.0, 0.0, 0);
        let (wa, wo) = weights(&b);
        assert!(wa > 0.99 && wo < 0.01, "sharp focus: A {wa:.3}, other {wo:.3}");
        b.attend(query(&a), true, 0.0, 0.0, 0);
        let (wa, wo) = weights(&b);
        assert!(wo > 0.05 && wa > wo, "soft focus should blend: A {wa:.3}, other {wo:.3}");
    }

    #[test]
    fn a_near_repeat_keeps_the_original_and_a_change_is_not_remembered() {
        let mut b = Bank::new();
        b.prepare(1_000.0);
        let a = tonal(&[3]);
        for _ in 0..(4 * b.grain) {
            b.capture(0.5, &a, true, false, true);
        }
        assert_eq!(b.slots.iter().filter(|s| s.filled).count(), 1);
        assert_eq!(b.slots[0].born, b.grain as u64, "the original (first) memory must be kept");
        let mut b = Bank::new();
        b.prepare(1_000.0);
        let (x, y) = (tonal(&[0]), tonal(&[6]));
        for i in 0..b.grain {
            b.capture(0.5, if i < b.grain / 2 { &x } else { &y }, true, false, true);
        }
        assert_eq!(b.slots.iter().filter(|s| s.filled).count(), 0);
    }

    #[test]
    fn a_reversed_head_just_past_zero_reads_inside_the_memory() {
        let audio: Vec<f32> = (0..36_000).map(|i| i as f32).collect();
        // rem_euclid(-1e-7, 36000.0) == 36000.0 in f32: one past the end.
        assert_eq!((-1e-7f32).rem_euclid(36_000.0), 36_000.0);
        let v = read_at(&audio, -1e-7);
        assert!(v.is_finite() && (0.0..36_000.0).contains(&v));
        assert_eq!(read_at(&audio, 1.5), 1.5);
    }

    #[test]
    fn harmony_only_counts_as_much_as_the_query_is_tonal() {
        let chord = Key { chroma: unit(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]), timbre: unit(&[1.0; TIMBRE]), rhythm: [0.0; RHYTHM], tonal: 0.9, percussive: false };
        let mut noisy = chord;
        noisy.tonal = 0.02;
        let other = Key { chroma: unit(&[0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]), ..chord };
        // Same timbre/groove, different harmony: a tonal query cares, a
        // percussive one hardly does.
        let gap_tonal = similarity(&chord, &chord, 0.0) - similarity(&chord, &other, 0.0);
        let gap_noisy = similarity(&noisy, &chord, 0.0) - similarity(&noisy, &other, 0.0);
        assert!(gap_tonal > 3.0 * gap_noisy, "tonal gap {gap_tonal:.3} vs percussive {gap_noisy:.3}");
    }
}
